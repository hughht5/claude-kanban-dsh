//! The test-app install lock off Windows (card #117 item 11, card #115's protocol). install.sh
//! opens `<D>/install.lock` on its own descriptor and runs `kb install-lock --fd <n>`, which
//! waits by #115's rules, takes `flock(LOCK_EX | LOCK_NB)` on the inherited descriptor, writes
//! the lock's JSON and exits. The lock belongs to the shell's open file, so it is held until
//! the shell ends, even if install.sh is killed.
//!
//! The files in D (card #115; parsed defensively, unknown fields ignored):
//! - turn-live.json, turn-test.json: {kind, card, owner, label, givenAt, expiresAt, appPid, appStart}
//! - hold.json: {until, by} (a Hold, at most 2 hours) or {allowUntil} (no wait from a hold)
//! - input-test.json: {at}, Hugh's last input in the test app
//!
//! Times are RFC 3339 strings or integer milliseconds; appStart and pidStart are milliseconds
//! since the epoch (0: unknown).

use serde_json::{json, Value};
use std::path::Path;

/// An install waits while Hugh used the test app within this long.
pub const INPUT_WAIT_MS: u64 = 10 * 60 * 1000;
/// A Hold lasts at most this long.
pub const HOLD_CAP_MS: u64 = 2 * 60 * 60 * 1000;
/// Input within this long moves the restart 2 minutes on.
pub const NOTICE_WINDOW_MS: u64 = 60 * 60 * 1000;
/// The notice before a restart when Hugh is about.
pub const NOTICE_MS: u64 = 2 * 60 * 1000;

/// One turn file's content.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Turn {
    /// "live" or "test", from the file name.
    pub channel: String,
    pub kind: String,
    /// The card number as written ("117", "#117"), if any.
    pub card: Option<String>,
    pub owner: String,
    pub label: String,
    pub expires_at_ms: Option<u64>,
    pub app_pid: i32,
    /// Milliseconds since the epoch; 0 when unknown.
    pub app_start_ms: u64,
}

/// Everything the waiting rules read, at one moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inputs {
    pub now_ms: u64,
    pub owner: String,
    pub ignore_turn: bool,
    pub turns: Vec<Turn>,
    /// The end of a Hold that is on (already capped), if any.
    pub hold_until_ms: Option<u64>,
    /// Hugh's last input in the test app.
    pub input_at_ms: Option<u64>,
}

/// A time as RFC 3339 text or integer milliseconds since the epoch.
pub fn time_ms(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)),
        Value::String(s) => {
            let s = s.trim();
            if let Ok(n) = s.parse::<u64>() {
                return Some(n);
            }
            chrono::DateTime::parse_from_rfc3339(s).ok().and_then(|t| u64::try_from(t.timestamp_millis()).ok())
        }
        _ => None,
    }
}

/// Milliseconds since the epoch as RFC 3339 UTC text, with milliseconds.
pub fn rfc3339(ms: u64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms as i64)
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}

/// A recorded start time (ms) matches a process's real one (µs): equal to the millisecond,
/// allowing 1 ms for rounding. 0 means unknown and matches anything.
pub fn start_matches(actual_us: u64, recorded_ms: u64) -> bool {
    recorded_ms == 0 || (actual_us / 1000).abs_diff(recorded_ms) <= 1
}

fn read_json(p: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(p).ok()?;
    let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    v.is_object().then_some(v)
}

fn text(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn mtime_ms(p: &Path) -> Option<u64> {
    let t = std::fs::metadata(p).ok()?.modified().ok()?;
    t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_millis() as u64)
}

/// A turn file's fields; missing or wrong-typed fields are empty.
pub fn parse_turn(channel: &str, v: &Value) -> Turn {
    let card = match v.get("card") {
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    };
    Turn {
        channel: channel.to_string(),
        kind: text(v, "kind"),
        card,
        owner: text(v, "owner"),
        label: text(v, "label"),
        expires_at_ms: v.get("expiresAt").and_then(time_ms),
        app_pid: v.get("appPid").and_then(Value::as_i64).and_then(|n| i32::try_from(n).ok()).unwrap_or(0),
        app_start_ms: v.get("appStart").and_then(time_ms).unwrap_or(0),
    }
}

/// The end of a Hold from hold.json, capped at 2 hours from when it was set (its own
/// "at"/"givenAt"/"setAt" field, else the file's time, else now) and at now + 2 h. None when
/// there is no Hold, it has ended, or the file says {allowUntil}.
pub fn parse_hold(v: &Value, file_ms: Option<u64>, now_ms: u64) -> Option<u64> {
    if v.get("allowUntil").is_some_and(|a| !a.is_null()) {
        return None;
    }
    let until = v.get("until").and_then(time_ms)?;
    let set_at = ["at", "givenAt", "setAt"].iter().find_map(|k| v.get(*k).and_then(time_ms)).or(file_ms).unwrap_or(now_ms);
    let end = until.min(set_at.saturating_add(HOLD_CAP_MS)).min(now_ms.saturating_add(HOLD_CAP_MS));
    (end > now_ms).then_some(end)
}

/// Read the shared folder's files at `now_ms`. Missing or unreadable files count as absent.
pub fn read_inputs(dir: &Path, owner: &str, ignore_turn: bool, now_ms: u64) -> Inputs {
    let mut turns = Vec::new();
    for channel in ["live", "test"] {
        if let Some(v) = read_json(&dir.join(format!("turn-{channel}.json"))) {
            turns.push(parse_turn(channel, &v));
        }
    }
    let hold = dir.join("hold.json");
    let hold_until_ms = read_json(&hold).and_then(|v| parse_hold(&v, mtime_ms(&hold), now_ms));
    let input_at_ms = read_json(&dir.join("input-test.json")).and_then(|v| v.get("at").and_then(time_ms));
    Inputs { now_ms, owner: owner.to_string(), ignore_turn, turns, hold_until_ms, input_at_ms }
}

/// The turn belongs to this run: its owner is this run's owner, or (a card's own tester turn)
/// its card is the card this run's owner names.
fn is_this_run(t: &Turn, owner: &str) -> bool {
    if owner.is_empty() {
        return false;
    }
    if t.owner == owner {
        return true;
    }
    let bare = |s: &str| s.trim().trim_start_matches('#').to_string();
    t.card.as_deref().is_some_and(|c| !bare(c).is_empty() && bare(c) == bare(owner))
}

/// The turn names a live holder: it has a kind, its app process is alive (with a matching
/// start time when one is given) and it hasn't expired.
fn live_holder(t: &Turn, now_ms: u64, alive: &dyn Fn(i32, u64) -> bool) -> bool {
    !t.kind.is_empty() && t.app_pid > 0 && alive(t.app_pid, t.app_start_ms) && t.expires_at_ms.map_or(true, |e| e > now_ms)
}

fn minutes(ms: u64) -> u64 {
    ms.div_ceil(60_000)
}

/// The first reason to wait, in plain English, or None to go ahead. `alive(pid, start_ms)`
/// says whether that process runs (start 0: any start). Another install holding the lock is
/// the caller's own check (the flock).
pub fn wait_reason(i: &Inputs, alive: &dyn Fn(i32, u64) -> bool) -> Option<String> {
    if !i.ignore_turn {
        for t in &i.turns {
            if is_this_run(t, &i.owner) || !live_holder(t, i.now_ms, alive) {
                continue;
            }
            let who = if !t.label.is_empty() { t.label.clone() } else if !t.owner.is_empty() { t.owner.clone() } else { "someone".into() };
            let card = t.card.as_deref().map(|c| format!(", card #{}", c.trim_start_matches('#'))).unwrap_or_default();
            let until = t.expires_at_ms.map(|e| format!(" until {} at the latest", rfc3339(e))).unwrap_or_default();
            return Some(format!("the {} app gave a {} turn on the test app to {who}{card}{until}", t.channel, t.kind));
        }
    }
    if let Some(at) = i.input_at_ms {
        let ago = i.now_ms.saturating_sub(at);
        if ago < INPUT_WAIT_MS {
            return Some(format!(
                "Hugh used the test app {} min ago; installs wait until 10 min after his last input (about {} min more)",
                ago / 60_000,
                minutes(INPUT_WAIT_MS - ago)
            ));
        }
    }
    if let Some(until) = i.hold_until_ms {
        if until > i.now_ms {
            return Some(format!("a Hold is on until {} ({} min more)", rfc3339(until), minutes(until - i.now_ms)));
        }
    }
    None
}

/// When the test app restarts: now + 2 min if Hugh used it in the last 60 min, else now.
pub fn restart_at_ms(i: &Inputs) -> u64 {
    match i.input_at_ms {
        Some(at) if i.now_ms.saturating_sub(at) < NOTICE_WINDOW_MS => i.now_ms + NOTICE_MS,
        _ => i.now_ms,
    }
}

/// The lock file's JSON. `pid_start_ms` is milliseconds since the epoch (0: unknown);
/// restartAt is RFC 3339 text.
#[allow(clippy::too_many_arguments)]
pub fn lock_json(pid: i32, pid_start_ms: u64, owner: &str, label: &str, card: Option<i64>, commit: &str, worktree: &str, phase: &str, restart_at_ms: u64) -> String {
    json!({
        "pid": pid,
        // RFC 3339 text, as install.ps1 writes it and the app reads it (testapp.rs, card #117).
        "pidStart": rfc3339(pid_start_ms),
        "owner": owner,
        "label": label,
        "card": card,
        "commit": commit,
        "worktree": worktree,
        "phase": phase,
        "restartAt": rfc3339(restart_at_ms),
    })
    .to_string()
}

/// The lock JSON with its phase set to `phase` (other fields kept). An unreadable old JSON
/// gives an object with the phase only.
pub fn with_phase(old: &str, phase: &str) -> String {
    let mut v: Value = serde_json::from_str(old.trim().trim_start_matches('\u{feff}')).unwrap_or_else(|_| json!({}));
    if !v.is_object() {
        v = json!({});
    }
    v["phase"] = json!(phase);
    v.to_string()
}

/// The run's owner: KANBAN_LOCK_OWNER, else KANBAN_CARD, else "hugh".
pub fn owner_from_env(lock_owner: Option<&str>, card: Option<&str>) -> String {
    [lock_owner, card].iter().flatten().map(|s| s.trim()).find(|s| !s.is_empty()).unwrap_or("hugh").to_string()
}

/// The run's label: the --label given, else "#<card>", else the owner.
pub fn label_for(label: Option<&str>, card: Option<i64>, owner: &str) -> String {
    match (label.map(str::trim).filter(|l| !l.is_empty()), card) {
        (Some(l), _) => l.to_string(),
        (None, Some(n)) => format!("#{n}"),
        (None, None) => owner.to_string(),
    }
}

/// `kb install-lock`'s arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct LockArgs {
    pub fd: i32,
    pub lock_dir: std::path::PathBuf,
    pub phase: Option<String>,
    pub label: Option<String>,
    pub card: Option<i64>,
    pub commit: String,
    pub worktree: String,
    pub wait_minutes: u64,
    pub ignore_turn: bool,
}

/// Parse `--fd <n> --lock-dir <D> [--phase p] [--label l] [--card n] [--commit c]
/// [--worktree w] [--wait-minutes 30] [--ignore-turn]`.
pub fn parse_args(args: &[String]) -> Result<LockArgs, String> {
    let mut a = LockArgs {
        fd: -1,
        lock_dir: Default::default(),
        phase: None,
        label: None,
        card: None,
        commit: String::new(),
        worktree: String::new(),
        wait_minutes: 30,
        ignore_turn: false,
    };
    let mut it = args.iter();
    while let Some(k) = it.next() {
        if k == "--ignore-turn" {
            a.ignore_turn = true;
            continue;
        }
        let mut val = || it.next().cloned().ok_or(format!("{k} needs a value"));
        match k.as_str() {
            "--fd" => a.fd = val()?.parse().map_err(|_| "--fd needs a descriptor number".to_string())?,
            "--lock-dir" => a.lock_dir = val()?.into(),
            "--phase" => a.phase = Some(val()?),
            "--label" => a.label = Some(val()?),
            "--card" => a.card = Some(val()?.trim_start_matches('#').parse().map_err(|_| "--card needs a card number".to_string())?),
            "--commit" => a.commit = val()?,
            "--worktree" => a.worktree = val()?,
            "--wait-minutes" => a.wait_minutes = val()?.parse().map_err(|_| "--wait-minutes needs a number".to_string())?,
            other => return Err(format!("unknown option {other}")),
        }
    }
    if a.fd < 0 {
        return Err("--fd <n> is needed (the descriptor install.sh opened on install.lock)".into());
    }
    if a.lock_dir.as_os_str().is_empty() {
        return Err("--lock-dir <folder> is needed".into());
    }
    if a.phase.as_deref().is_some_and(|p| p.trim().is_empty()) {
        return Err("--phase needs a value".into());
    }
    Ok(a)
}

/// `flock(fd, LOCK_EX | LOCK_NB)`: Ok(true) when taken (or already held through this open
/// file), Ok(false) when another open file holds it.
#[cfg(unix)]
pub fn flock_nb(fd: i32) -> std::io::Result<bool> {
    loop {
        // SAFETY: flock only reads the descriptor number.
        if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(true);
        }
        let e = std::io::Error::last_os_error();
        match e.raw_os_error() {
            Some(libc::EWOULDBLOCK) => return Ok(false),
            Some(libc::EINTR) => continue,
            _ => return Err(e),
        }
    }
}

/// Release the flock taken through `fd`.
#[cfg(unix)]
pub fn unlock(fd: i32) {
    // SAFETY: as flock_nb.
    unsafe { libc::flock(fd, libc::LOCK_UN) };
}

/// Replace the file's content through `fd`: ftruncate to 0, pwrite at 0, fsync.
#[cfg(unix)]
pub fn write_fd(fd: i32, json: &str) -> std::io::Result<()> {
    let err = std::io::Error::last_os_error;
    // SAFETY: plain calls on a descriptor the caller owns; the buffer outlives each call.
    unsafe {
        if libc::ftruncate(fd, 0) != 0 {
            return Err(err());
        }
        let bytes = json.as_bytes();
        let mut done = 0usize;
        while done < bytes.len() {
            let n = libc::pwrite(fd, bytes[done..].as_ptr().cast(), bytes.len() - done, done as libc::off_t);
            if n < 0 {
                let e = err();
                if e.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                return Err(e);
            }
            done += n as usize;
        }
        if libc::fsync(fd) != 0 {
            return Err(err());
        }
    }
    Ok(())
}

/// The file's content through `fd` (pread from 0). Fails with EBADF on a write-only descriptor.
#[cfg(unix)]
pub fn read_fd(fd: i32) -> std::io::Result<String> {
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        // SAFETY: the buffer is valid for its length.
        let n = unsafe { libc::pread(fd, buf.as_mut_ptr().cast(), buf.len(), out.len() as libc::off_t) };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(e);
        }
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

#[cfg(target_os = "macos")]
fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// `kb install-lock`: wait by the rules and take the lock (no --phase), or rewrite the phase
/// (--phase p). The exit code: 0 done, 1 an error, 3 gave up waiting for another install.
#[cfg(target_os = "macos")]
pub fn install_lock(a: &LockArgs) -> i32 {
    match &a.phase {
        Some(p) => set_phase(a, p),
        None => take(a),
    }
}

#[cfg(target_os = "macos")]
fn set_phase(a: &LockArgs, phase: &str) -> i32 {
    match flock_nb(a.fd) {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("kb install-lock: this run doesn't hold the lock (another install does)");
            return 1;
        }
        Err(e) => {
            eprintln!("kb install-lock: flock on descriptor {}: {e}", a.fd);
            return 1;
        }
    }
    // A descriptor opened with >> is write-only, so read through the path then.
    let old = read_fd(a.fd).or_else(|_| std::fs::read_to_string(a.lock_dir.join("install.lock"))).unwrap_or_default();
    match write_fd(a.fd, &with_phase(&old, phase)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("kb install-lock: writing the phase: {e}");
            1
        }
    }
}

#[cfg(target_os = "macos")]
fn take(a: &LockArgs) -> i32 {
    use std::time::Duration;
    let owner = owner_from_env(std::env::var("KANBAN_LOCK_OWNER").ok().as_deref(), std::env::var("KANBAN_CARD").ok().as_deref());
    let card = a.card.or_else(|| std::env::var("KANBAN_CARD").ok().and_then(|c| c.trim().trim_start_matches('#').parse().ok()));
    let label = label_for(a.label.as_deref(), card, &owner);
    if a.ignore_turn {
        eprintln!("kb install-lock: --ignore-turn: not waiting for test-app turns");
    }
    let alive = |pid: i32, start_ms: u64| crate::sys::process_start(pid).is_some_and(|us| start_matches(us, start_ms));
    let step = Duration::from_secs(5);
    let limit_ms = a.wait_minutes.saturating_mul(60_000);
    let mut lock_waited_ms = 0u64;
    let mut last_said: Option<(String, u64)> = None;
    let mut say = |reason: &str| {
        let now = now_ms();
        let due = match &last_said {
            Some((r, at)) => r != reason || now.saturating_sub(*at) >= 60_000,
            None => true,
        };
        if due {
            eprintln!("kb install-lock: waiting: {reason}");
            last_said = Some((reason.to_string(), now));
        }
    };
    loop {
        match flock_nb(a.fd) {
            Err(e) => {
                eprintln!("kb install-lock: flock on descriptor {}: {e}", a.fd);
                return 1;
            }
            Ok(false) => {
                if lock_waited_ms >= limit_ms {
                    eprintln!("kb install-lock: gave up after {} min: another install still holds the lock", a.wait_minutes);
                    return 3;
                }
                say("another install holds the lock");
                std::thread::sleep(step);
                lock_waited_ms += step.as_millis() as u64;
                continue;
            }
            Ok(true) => {}
        }
        let inputs = read_inputs(&a.lock_dir, &owner, a.ignore_turn, now_ms());
        if let Some(reason) = wait_reason(&inputs, &alive) {
            // Don't hold the lock while waiting on a turn, a Hold or Hugh's input.
            unlock(a.fd);
            say(&reason);
            std::thread::sleep(step);
            continue;
        }
        // SAFETY: getppid has no preconditions.
        let ppid = unsafe { libc::getppid() };
        let ppid_start_ms = crate::sys::process_start(ppid).map_or(0, |us| us / 1000);
        let json = lock_json(ppid, ppid_start_ms, &owner, &label, card, &a.commit, &a.worktree, "quitting", restart_at_ms(&inputs));
        if let Err(e) = write_fd(a.fd, &json) {
            unlock(a.fd);
            eprintln!("kb install-lock: writing the lock file: {e}");
            return 1;
        }
        println!("locked");
        return 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_762_430_000_000; // 2025-11-06T11:53:20Z

    fn turn(kind: &str, owner: &str, pid: i32) -> Turn {
        Turn { channel: "live".into(), kind: kind.into(), owner: owner.into(), label: format!("{owner}'s turn"), app_pid: pid, ..Default::default() }
    }

    fn inputs() -> Inputs {
        Inputs { now_ms: NOW, owner: "router".into(), ..Default::default() }
    }

    fn alive_42(pid: i32, _: u64) -> bool {
        pid == 42
    }

    #[test]
    fn times() {
        assert_eq!(time_ms(&json!(NOW)), Some(NOW));
        assert_eq!(time_ms(&json!(NOW.to_string())), Some(NOW));
        assert_eq!(time_ms(&json!("2025-11-06T11:53:20Z")), Some(NOW));
        assert_eq!(time_ms(&json!("2025-11-06T11:53:20.000+00:00")), Some(NOW));
        assert_eq!(time_ms(&json!("2025-11-06T12:53:20+01:00")), Some(NOW));
        assert_eq!(time_ms(&json!("soon")), None);
        assert_eq!(time_ms(&json!(null)), None);
        assert_eq!(time_ms(&json!(-5)), None);
        assert_eq!(rfc3339(NOW), "2025-11-06T11:53:20.000Z");
        assert!(start_matches(1_000_500, 1000));
        assert!(start_matches(1_001_999, 1000), "rounded up");
        assert!(!start_matches(1_005_000, 1000));
        assert!(start_matches(123, 0), "0 is unknown");
    }

    #[test]
    fn no_files_no_wait() {
        let dir = tempfile::tempdir().unwrap();
        let i = read_inputs(dir.path(), "hugh", false, NOW);
        assert_eq!(i, Inputs { now_ms: NOW, owner: "hugh".into(), ..Default::default() });
        assert_eq!(wait_reason(&i, &|_, _| true), None);
        assert_eq!(restart_at_ms(&i), NOW);
    }

    #[test]
    fn files_are_read_defensively() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::write(
            d.join("turn-live.json"),
            r##"{"kind":"tester","card":96,"owner":"card-96","label":"#96","givenAt":"2025-11-06T11:00:00Z","expiresAt":"2025-11-06T12:00:00Z","appPid":42,"appStart":1762400000000,"extra":[1]}"##,
        )
        .unwrap();
        std::fs::write(d.join("turn-test.json"), "\u{feff}{\"kind\":\"\",\"appPid\":\"x\"}").unwrap();
        std::fs::write(d.join("hold.json"), r#"{"until":"2025-11-06T12:23:20Z","by":"hugh","at":"2025-11-06T11:50:00Z"}"#).unwrap();
        std::fs::write(d.join("input-test.json"), format!("{{\"at\":{}}}", NOW - 120_000)).unwrap();
        let i = read_inputs(d, "router", false, NOW);
        assert_eq!(i.turns.len(), 2);
        assert_eq!(
            i.turns[0],
            Turn {
                channel: "live".into(),
                kind: "tester".into(),
                card: Some("96".into()),
                owner: "card-96".into(),
                label: "#96".into(),
                expires_at_ms: Some(NOW + 400_000),
                app_pid: 42,
                app_start_ms: 1_762_400_000_000,
            }
        );
        assert_eq!(i.turns[1], Turn { channel: "test".into(), ..Default::default() });
        assert_eq!(i.hold_until_ms, Some(NOW + 30 * 60_000));
        assert_eq!(i.input_at_ms, Some(NOW - 120_000));
        // A broken file is absent.
        std::fs::write(d.join("hold.json"), "{not json").unwrap();
        assert_eq!(read_inputs(d, "router", false, NOW).hold_until_ms, None);
    }

    #[test]
    fn turn_waits() {
        let mut i = inputs();
        i.turns = vec![turn("tester", "card-96", 42)];
        let r = wait_reason(&i, &alive_42).unwrap();
        assert!(r.contains("tester turn") && r.contains("card-96's turn"), "{r}");
        // Its app isn't running: no wait.
        i.turns = vec![turn("tester", "card-96", 7)];
        assert_eq!(wait_reason(&i, &alive_42), None);
        // No kind: no holder.
        i.turns = vec![turn("", "card-96", 42)];
        assert_eq!(wait_reason(&i, &alive_42), None);
        // Expired.
        let mut t = turn("tester", "card-96", 42);
        t.expires_at_ms = Some(NOW - 1);
        i.turns = vec![t.clone()];
        assert_eq!(wait_reason(&i, &alive_42), None);
        t.expires_at_ms = Some(NOW + 60_000);
        i.turns = vec![t];
        assert!(wait_reason(&i, &alive_42).is_some());
        // --ignore-turn.
        i.ignore_turn = true;
        assert_eq!(wait_reason(&i, &alive_42), None);
    }

    #[test]
    fn the_turn_of_this_run_is_skipped() {
        let mut i = inputs();
        i.turns = vec![turn("install", "router", 42)];
        assert_eq!(wait_reason(&i, &alive_42), None, "the router's own install turn");
        i.turns = vec![turn("tester", "router", 42)];
        assert_eq!(wait_reason(&i, &alive_42), None);
        // A card's own tester turn: its owner is KANBAN_CARD.
        let mut t = turn("tester", "someone", 42);
        t.card = Some("#117".into());
        i.owner = "117".into();
        i.turns = vec![t.clone()];
        assert_eq!(wait_reason(&i, &alive_42), None);
        i.owner = "118".into();
        assert!(wait_reason(&i, &alive_42).unwrap().contains("card #117"));
        i.owner = String::new();
        t.owner = String::new();
        i.turns = vec![t];
        assert!(wait_reason(&i, &alive_42).is_some(), "an empty owner never matches");
    }

    #[test]
    fn the_start_time_is_passed_to_alive() {
        let mut i = inputs();
        let mut t = turn("tester", "x", 42);
        t.app_start_ms = 5000;
        i.turns = vec![t];
        assert!(wait_reason(&i, &|pid, start| pid == 42 && start == 5000).is_some());
        assert_eq!(wait_reason(&i, &|pid, start| pid == 42 && start == 0), None);
    }

    #[test]
    fn input_waits_ten_minutes() {
        let mut i = inputs();
        i.input_at_ms = Some(NOW - 9 * 60_000);
        let r = wait_reason(&i, &|_, _| false).unwrap();
        assert!(r.starts_with("Hugh used the test app 9 min ago"), "{r}");
        i.input_at_ms = Some(NOW - 10 * 60_000);
        assert_eq!(wait_reason(&i, &|_, _| false), None);
        i.input_at_ms = Some(NOW + 5000);
        assert!(wait_reason(&i, &|_, _| false).is_some(), "a clock a little ahead still counts");
    }

    #[test]
    fn holds() {
        let set = NOW - 60_000;
        // A Hold for 30 min more.
        assert_eq!(parse_hold(&json!({"until": NOW + 1_800_000, "by": "hugh"}), Some(set), NOW), Some(NOW + 1_800_000));
        // Capped at 2 h from when it was set (the file's time when it has no "at").
        assert_eq!(parse_hold(&json!({"until": NOW + 10 * HOLD_CAP_MS}), Some(set), NOW), Some(set + HOLD_CAP_MS));
        // ... and at now + 2 h, even with a future "at".
        assert_eq!(parse_hold(&json!({"until": NOW + 10 * HOLD_CAP_MS, "at": NOW + HOLD_CAP_MS}), None, NOW), Some(NOW + HOLD_CAP_MS));
        // Set more than 2 h ago: over.
        assert_eq!(parse_hold(&json!({"until": NOW + 3_600_000}), Some(NOW - HOLD_CAP_MS - 1), NOW), None);
        assert_eq!(parse_hold(&json!({"until": NOW - 1}), Some(set), NOW), None, "ended");
        assert_eq!(parse_hold(&json!({"allowUntil": NOW + 60_000}), Some(set), NOW), None);
        assert_eq!(parse_hold(&json!({"by": "hugh"}), Some(set), NOW), None);
        let mut i = inputs();
        i.hold_until_ms = Some(NOW + 90_000);
        let r = wait_reason(&i, &|_, _| false).unwrap();
        assert!(r.starts_with("a Hold is on until") && r.contains("(2 min more)"), "{r}");
    }

    #[test]
    fn restart_notice() {
        let mut i = inputs();
        assert_eq!(restart_at_ms(&i), NOW);
        i.input_at_ms = Some(NOW - 59 * 60_000);
        assert_eq!(restart_at_ms(&i), NOW + 120_000);
        i.input_at_ms = Some(NOW - 60 * 60_000);
        assert_eq!(restart_at_ms(&i), NOW);
    }

    #[test]
    fn the_lock_json() {
        let j: Value = serde_json::from_str(&lock_json(321, 1_762_400_000_000, "router", "wave 3", Some(117), "abc123", "/Users/h/kanban-app-117", "quitting", NOW)).unwrap();
        assert_eq!(
            j,
            json!({"pid": 321, "pidStart": rfc3339(1_762_400_000_000), "owner": "router", "label": "wave 3", "card": 117, "commit": "abc123",
                   "worktree": "/Users/h/kanban-app-117", "phase": "quitting", "restartAt": "2025-11-06T11:53:20.000Z"})
        );
        assert_eq!(serde_json::from_str::<Value>(&lock_json(1, 0, "hugh", "hugh", None, "", "", "quitting", NOW)).unwrap()["card"], Value::Null);
        let moved: Value = serde_json::from_str(&with_phase(&j.to_string(), "installing")).unwrap();
        assert_eq!(moved["phase"], "installing");
        assert_eq!(moved["owner"], "router");
        assert_eq!(with_phase("", "started"), r#"{"phase":"started"}"#);
        assert_eq!(with_phase("[1]", "started"), r#"{"phase":"started"}"#);
    }

    #[test]
    fn owner_and_label() {
        assert_eq!(owner_from_env(Some("router"), Some("117")), "router");
        assert_eq!(owner_from_env(Some(" "), Some("117")), "117");
        assert_eq!(owner_from_env(None, None), "hugh");
        assert_eq!(owner_from_env(Some(""), Some("")), "hugh");
        assert_eq!(label_for(Some("wave 3"), Some(117), "router"), "wave 3");
        assert_eq!(label_for(None, Some(117), "router"), "#117");
        assert_eq!(label_for(Some(""), None, "router"), "router");
    }

    #[test]
    fn arguments() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let got = parse_args(&a(&["--fd", "9", "--lock-dir", "/x/testapp"])).unwrap();
        assert_eq!(
            got,
            LockArgs { fd: 9, lock_dir: "/x/testapp".into(), phase: None, label: None, card: None, commit: String::new(), worktree: String::new(), wait_minutes: 30, ignore_turn: false }
        );
        let got = parse_args(&a(&[
            "--fd", "9", "--lock-dir", "/d", "--phase", "installing", "--label", "wave 3", "--card", "#117", "--commit", "abc", "--worktree", "/w", "--wait-minutes", "5", "--ignore-turn",
        ]))
        .unwrap();
        assert_eq!((got.phase.as_deref(), got.label.as_deref(), got.card, got.wait_minutes, got.ignore_turn), (Some("installing"), Some("wave 3"), Some(117), 5, true));
        assert_eq!((got.commit.as_str(), got.worktree.as_str()), ("abc", "/w"));
        assert!(parse_args(&a(&["--lock-dir", "/d"])).is_err(), "no --fd");
        assert!(parse_args(&a(&["--fd", "9"])).is_err(), "no --lock-dir");
        assert!(parse_args(&a(&["--fd", "x", "--lock-dir", "/d"])).is_err());
        assert!(parse_args(&a(&["--fd", "9", "--lock-dir"])).is_err());
        assert!(parse_args(&a(&["--fd", "9", "--lock-dir", "/d", "--what"])).is_err());
        assert!(parse_args(&a(&["--fd", "9", "--lock-dir", "/d", "--phase", " "])).is_err());
    }

    /// flock through two open files of one lock file, and the descriptor read and write.
    #[cfg(unix)]
    #[test]
    fn flock_and_fd_writes() {
        use std::os::fd::AsRawFd;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("install.lock");
        let a = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&p).unwrap();
        let b = std::fs::OpenOptions::new().append(true).create(true).open(&p).unwrap();
        assert!(flock_nb(a.as_raw_fd()).unwrap());
        assert!(flock_nb(a.as_raw_fd()).unwrap(), "the same open file may lock again");
        assert!(!flock_nb(b.as_raw_fd()).unwrap(), "another open file waits");
        write_fd(a.as_raw_fd(), "{\"phase\":\"quitting\",\"owner\":\"hugh\"}").unwrap();
        write_fd(a.as_raw_fd(), "{\"phase\":\"x\"}").unwrap();
        assert_eq!(read_fd(a.as_raw_fd()).unwrap(), "{\"phase\":\"x\"}", "truncated, not appended");
        assert!(read_fd(b.as_raw_fd()).is_err(), "an append-only descriptor can't be read");
        // Writing through an append-only descriptor still replaces the content.
        unlock(a.as_raw_fd());
        assert!(flock_nb(b.as_raw_fd()).unwrap());
        write_fd(b.as_raw_fd(), "{\"phase\":\"started\"}").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{\"phase\":\"started\"}");
        assert!(!flock_nb(a.as_raw_fd()).unwrap());
        drop(b);
        // A fresh open, as the app's held check and the next install.sh make, gets it now.
        let c = std::fs::File::open(&p).unwrap();
        assert!(flock_nb(c.as_raw_fd()).unwrap(), "closing the holder's last descriptor frees the lock");
    }
}
