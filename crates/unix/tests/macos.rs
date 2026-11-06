//! The macOS-only parts of kanban-unix, on a real Mac (CI's macos-latest): the exit waiters,
//! libproc, the reaper and the update helper. On other systems this file is empty.
#![cfg(target_os = "macos")]

use kanban_unix::{reap, sys, text, track};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn wait_until(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    f()
}

#[test]
fn wait_exit_does_not_reap() {
    let mut child = Command::new("sleep").arg("0.2").spawn().unwrap();
    let t = Instant::now();
    sys::wait_exit(child.id() as i32);
    assert!(t.elapsed() >= Duration::from_millis(150));
    // WNOWAIT left the status for try_wait.
    assert!(child.try_wait().unwrap().is_some_and(|s| s.success()));
    // Not a child (any more): returns at once.
    let t = Instant::now();
    sys::wait_exit(child.id() as i32);
    assert!(t.elapsed() < Duration::from_secs(1));
}

#[test]
fn watch_exit_on_a_non_child() {
    // sh starts sleep in the background and exits, so the sleep is not our child.
    let out = Command::new("sh").arg("-c").arg("sleep 0.5 >/dev/null 2>&1 & echo $!").output().unwrap();
    let pid: i32 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();
    let t = Instant::now();
    sys::watch_exit(pid);
    assert!(t.elapsed() < Duration::from_secs(5));
    assert_eq!(sys::process_start(pid), None);
    // Already gone: at once.
    let t = Instant::now();
    sys::watch_exit(pid);
    assert!(t.elapsed() < Duration::from_millis(500));
}

#[test]
fn process_start_and_listing() {
    let me = std::process::id() as i32;
    let start = sys::process_start(me).expect("our own start time");
    let now_us = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_micros() as u64;
    assert!(start <= now_us && now_us - start < 3_600_000_000, "started within the hour");
    assert!(sys::alive(&track::Proc { pid: me, start }));
    assert!(!sys::alive(&track::Proc { pid: me, start: start + 1 }));
    let all = sys::all_procs();
    // SAFETY: getsid(0) is our own session.
    let my_sid = unsafe { libc::getsid(0) };
    assert!(all.iter().any(|(p, sid)| p.pid == me && p.start == start && *sid == my_sid));
    assert_eq!(sys::process_start(-1), None);
}

#[test]
fn nofile_and_shell() {
    let (before, after) = sys::raise_nofile();
    assert!(after >= before);
    assert!(sys::user_shell().is_some_and(|s| s.starts_with('/')));
}

#[test]
fn private_files() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().join("data");
    sys::private_dir(&d).unwrap();
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
    sys::private_dir(&d).unwrap();
    assert_eq!(std::fs::metadata(&d).unwrap().permissions().mode() & 0o777, 0o700);
    let f = d.join("api.json");
    std::fs::write(&f, "old").unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
    sys::write_private(&f, b"{\"token\":\"x\"}").unwrap();
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "{\"token\":\"x\"}");
    assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1, "no temp file left");
}

#[test]
fn reap_ends_a_recorded_session() {
    let dir = tempfile::tempdir().unwrap();
    // A "claude": sleep 600 as a session leader of its own.
    let mut leader = Command::new("sleep");
    leader.arg("600").stdin(Stdio::null());
    // SAFETY: setsid is async-signal-safe.
    unsafe {
        leader.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut leader = leader.spawn().unwrap();
    let lpid = leader.id() as i32;
    assert!(wait_until(Duration::from_secs(2), || sys::process_start(lpid).is_some()));
    // The "app": a short sleep the reaper watches.
    let mut app = Command::new("sleep").arg("1").spawn().unwrap();
    let mut record = track::Record { app_pid: app.id() as i32, ..Default::default() };
    record.add_session(track::Proc { pid: lpid, start: sys::process_start(lpid).unwrap() });
    record.observe(&sys::all_procs());
    assert!(record.procs.iter().any(|p| p.pid == lpid));
    let file = dir.path().join("pids.json");
    sys::write_private(&file, record.to_json().as_bytes()).unwrap();
    let app_pid = app.id() as i32;
    let reaper = std::thread::spawn({
        let file = file.clone();
        move || reap::run(app_pid, &file)
    });
    app.wait().unwrap();
    assert!(wait_until(Duration::from_secs(5), || leader.try_wait().unwrap().is_some()), "the recorded session ended");
    assert_eq!(reaper.join().unwrap(), 0);
    assert!(!file.exists(), "the record is removed");
    // A missing record: nothing to do.
    assert_eq!(reap::run(999_999, &dir.path().join("none.json")), 0);
}

/// A dummy bundle, zipped with ditto; a stub `open` first on PATH that records its argument.
struct Helper {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    apps: std::path::PathBuf,
    app: std::path::PathBuf,
    update: std::path::PathBuf,
    opened: std::path::PathBuf,
    path: String,
}

fn helper_setup() -> Helper {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let apps = root.join("Applications");
    let app = apps.join("Claude Kanban DSH.app");
    std::fs::create_dir_all(app.join("Contents")).unwrap();
    std::fs::write(app.join("Contents/version"), "old").unwrap();
    let update = root.join("update");
    let staged = root.join("staged/Claude Kanban DSH.app");
    std::fs::create_dir_all(staged.join("Contents")).unwrap();
    std::fs::create_dir_all(&update).unwrap();
    std::fs::write(staged.join("Contents/version"), "new").unwrap();
    let st = Command::new("ditto").args(["-c", "-k", "--keepParent"]).arg(&staged).arg(update.join("Claude Kanban DSH.zip")).status().unwrap();
    assert!(st.success());
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let opened = root.join("opened.txt");
    let stub = bin.join("open");
    std::fs::write(&stub, format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\n", text::sh_quote(&opened.to_string_lossy()))).unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    Helper { _dir: dir, root, apps, app, update, opened, path }
}

fn dead_pid() -> u32 {
    let mut c = Command::new("true").spawn().unwrap();
    let pid = c.id();
    c.wait().unwrap();
    pid
}

fn run_helper(h: &Helper) -> (bool, Option<String>, String) {
    let script = text::update_helper_script(
        dead_pid(),
        &h.update.join("Claude Kanban DSH.zip"),
        &h.apps,
        &h.app,
        &h.update.join("restart.log"),
        &h.update.join("restart-result.json"),
    );
    let f = h.root.join("helper.sh");
    std::fs::write(&f, script).unwrap();
    Command::new("sh").arg(&f).env("PATH", &h.path).output().unwrap();
    let json = std::fs::read_to_string(h.update.join("restart-result.json")).unwrap();
    text::restart_result(&json).expect("a result file")
}

#[test]
fn update_helper_swaps_the_bundle() {
    let h = helper_setup();
    let (ok, error, at) = run_helper(&h);
    assert!(ok, "error: {error:?}; log: {}", std::fs::read_to_string(h.update.join("restart.log")).unwrap_or_default());
    assert_eq!(error, None);
    assert!(at.ends_with('Z'));
    assert_eq!(std::fs::read_to_string(h.app.join("Contents/version")).unwrap(), "new");
    assert_eq!(std::fs::read_to_string(h.update.join("previous.app/Contents/version")).unwrap(), "old");
    assert_eq!(std::fs::read_to_string(&h.opened).unwrap().trim(), h.app.to_string_lossy());
    let leftovers: Vec<_> = std::fs::read_dir(&h.update).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("unpack.")).collect();
    assert!(leftovers.is_empty(), "the temp folder is removed: {leftovers:?}");
}

#[test]
fn update_helper_reports_a_failed_move() {
    let h = helper_setup();
    std::fs::set_permissions(&h.apps, std::fs::Permissions::from_mode(0o555)).unwrap();
    let (ok, error, _) = run_helper(&h);
    std::fs::set_permissions(&h.apps, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!ok);
    let error = error.expect("the error text");
    assert!(error.contains("could not move") && error.to_lowercase().contains("permission denied"), "{error}");
    assert_eq!(std::fs::read_to_string(h.app.join("Contents/version")).unwrap(), "old", "the old app is still there");
    let log = std::fs::read_to_string(h.update.join("restart.log")).unwrap();
    assert!(log.contains("FAILED"), "{log}");
}

#[test]
fn update_helper_gives_up_on_a_running_app() {
    // Not run in full (it waits 5 min): only that a live pid is waited for.
    let h = helper_setup();
    let script = text::update_helper_script(std::process::id(), &h.update.join("Claude Kanban DSH.zip"), &h.apps, &h.app, &h.update.join("restart.log"), &h.update.join("restart-result.json"));
    let f = h.root.join("helper.sh");
    std::fs::write(&f, script).unwrap();
    let mut c = Command::new("sh").arg(&f).env("PATH", &h.path).spawn().unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    assert!(c.try_wait().unwrap().is_none(), "still waiting for the app");
    c.kill().unwrap();
    c.wait().unwrap();
    assert!(!Path::new(&h.update.join("restart-result.json")).exists());
}
