//! One-click updates. `scripts\install.ps1` (in its default `-Stage` mode for the live app)
//! builds the installer and leaves it in `<data dir>\update\` with an `update.json` beside it,
//! without touching the running app. The app looks at that folder every 30 seconds; when the
//! staged build is newer than itself it tells the page ("update-ready"), which shows a banner.
//! Restart now suspends every session as tray > Quit does, hands the install to a detached
//! helper that waits for this process to exit, runs the installer silently and starts the app
//! again in front (Hugh clicked for it, so it isn't a background start), and exits. The next
//! start resumes the sessions that were mid-turn (restart recovery), as after any quit.
//!
//! Each channel has its own data folder, so the test app only ever sees test updates.

use crate::sessions::Sessions;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

/// This build's identity, compiled in by `build.rs`.
pub const COMMIT: &str = env!("KANBAN_GIT_COMMIT");
pub const BUILT_AT: &str = env!("KANBAN_BUILT_AT");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const POLL: Duration = Duration::from_secs(30);

/// `update.json`, as `install.ps1 -Stage` writes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Staged {
    pub version: String,
    pub commit: String,
    pub built_at: String,
    /// The installer's full path (normally in the update folder itself).
    pub installer: String,
    /// Where the installer goes (`/D=`) and the exe started afterwards. Older files may lack
    /// them; then the running exe's own folder and path are used.
    #[serde(default)]
    pub install_dir: Option<String>,
    #[serde(default)]
    pub app_exe: Option<String>,
}

/// What the page is told: this build, and the staged update if one is ready and not put off.
#[derive(Serialize, Clone)]
pub struct Status {
    running: Running,
    ready: Option<Staged>,
}

#[derive(Serialize, Clone)]
struct Running {
    version: &'static str,
    commit: &'static str,
    built_at: &'static str,
}

#[derive(Default)]
pub struct Updates {
    /// The staged update seen last (newer than this build, installer present).
    ready: Mutex<Option<Staged>>,
    /// The `built_at` Hugh clicked Later on. Kept in memory only, so it lasts until the app
    /// restarts; a newer staged build has another `built_at` and shows again.
    later: Mutex<Option<String>>,
}

pub fn folder() -> PathBuf {
    kanban_store::data_dir().join("update")
}

/// Reads `<dir>\update.json`; `None` if it is missing, unreadable, or its installer is gone.
pub fn read_staged(dir: &Path) -> Option<Staged> {
    let text = std::fs::read_to_string(dir.join("update.json")).ok()?;
    // PowerShell 5.1 writes UTF-8 with a byte-order mark.
    let s: Staged = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    Path::new(&s.installer).is_file().then_some(s)
}

/// Whether a staged build made at `staged` is newer than one made at `running` (both RFC 3339).
/// If either can't be read as a time, any difference counts as newer, so an update is never
/// hidden by an odd timestamp.
pub fn is_newer(staged: &str, running: &str) -> bool {
    use chrono::DateTime;
    match (DateTime::parse_from_rfc3339(staged.trim()), DateTime::parse_from_rfc3339(running.trim())) {
        (Ok(s), Ok(r)) => s > r,
        _ => staged.trim() != running.trim(),
    }
}

fn ready_now(dir: &Path) -> Option<Staged> {
    read_staged(dir).filter(|s| is_newer(&s.built_at, BUILT_AT))
}

impl Updates {
    fn status(&self) -> Status {
        let later = self.later.lock().ok().and_then(|l| l.clone());
        let ready = self
            .ready
            .lock()
            .ok()
            .and_then(|r| r.clone())
            .filter(|s| later.as_deref() != Some(s.built_at.as_str()));
        Status { running: Running { version: VERSION, commit: COMMIT, built_at: BUILT_AT }, ready }
    }

    /// Looks at the folder again; true if what is ready changed.
    fn refresh(&self, dir: &Path) -> bool {
        let now = ready_now(dir);
        let Ok(mut ready) = self.ready.lock() else { return false };
        if *ready == now {
            return false;
        }
        *ready = now;
        true
    }
}

/// Checks the update folder at start and every 30 seconds after, and tells the page when what
/// is ready changes.
pub fn watch(app: AppHandle) {
    std::thread::spawn(move || loop {
        let updates = app.state::<Updates>();
        if updates.refresh(&folder()) {
            let _ = app.emit("update-ready", updates.status());
        }
        std::thread::sleep(POLL);
    });
}

#[tauri::command]
pub fn update_status(updates: State<Updates>) -> Status {
    updates.refresh(&folder());
    updates.status()
}

/// Later: hides this staged build's banner until the app restarts or a newer one is staged.
#[tauri::command]
pub fn update_later(updates: State<Updates>, built_at: String) {
    if let Ok(mut l) = updates.later.lock() {
        *l = Some(built_at);
    }
}

/// Restart now: starts the helper, then suspends every session and exits.
#[tauri::command]
pub fn update_restart(app: AppHandle) -> Result<(), String> {
    let dir = folder();
    let staged = ready_now(&dir).ok_or("No newer update is staged any more.")?;
    let exe = std::env::current_exe().map_err(|e| format!("couldn't find the app's own path: {e}"))?;
    let install_dir = staged
        .install_dir
        .clone()
        .map(PathBuf::from)
        .or_else(|| exe.parent().map(PathBuf::from))
        .ok_or("couldn't tell where the app is installed")?;
    let app_exe = staged.app_exe.clone().map(PathBuf::from).unwrap_or(exe);
    let script = helper_script(
        std::process::id(),
        Path::new(&staged.installer),
        &install_dir,
        &app_exe,
        &dir.join("restart.log"),
    );
    spawn_detached(&script)?;
    // The stops take a moment each, so they run off the command's thread, as tray > Quit's do.
    let a = app.clone();
    std::thread::spawn(move || {
        a.state::<Arc<Sessions>>().stop_all(&a);
        a.exit(0);
    });
    Ok(())
}

/// A PowerShell literal string: single quotes, with any inside doubled.
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The helper's script: wait for the app (pid) to exit, run the installer silently into the
/// install folder, then start the app again whatever the installer did, so Hugh is never left
/// without the board. It starts in front, not with `--background`: Hugh clicked Restart now, so
/// he is looking at the board and expects it back. Each step is logged to `log`.
pub fn helper_script(pid: u32, installer: &Path, install_dir: &Path, app_exe: &Path, log: &Path) -> String {
    let log = ps_quote(&log.display().to_string());
    let setup = ps_quote(&installer.display().to_string());
    // /D must come last and unquoted, even with spaces (NSIS).
    let d_arg = ps_quote(&format!("/D={}", install_dir.display()));
    let dir = ps_quote(&install_dir.display().to_string());
    let exe = ps_quote(&app_exe.display().to_string());
    format!(
        "$ErrorActionPreference = 'Continue'\n\
         $log = {log}\n\
         function Note($t) {{ \"$(Get-Date -Format o) $t\" | Out-File -FilePath $log -Append -Encoding utf8 }}\n\
         Note 'Waiting for the app (pid {pid}) to exit'\n\
         try {{ Wait-Process -Id {pid} -Timeout 300 -ErrorAction Stop }} catch {{}}\n\
         if (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{ Note 'The app is still running after 5 minutes; not installing.'; exit 1 }}\n\
         Note ('Installing ' + {setup})\n\
         try {{\n\
           $p = Start-Process -FilePath {setup} -ArgumentList '/S', {d_arg} -Wait -PassThru\n\
           Note \"Installer exit code $($p.ExitCode)\"\n\
         }} catch {{ Note \"Installer failed to start: $_\" }}\n\
         Note ('Starting ' + {exe})\n\
         Start-Process -FilePath {exe} -WorkingDirectory {dir}\n\
         Note 'Done'\n"
    )
}

/// Starts PowerShell with no window, in its own process group and (when allowed) outside any
/// job this app is in, so it outlives the app.
fn spawn_detached(script: &str) -> Result<(), String> {
    use base64::Engine;
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    // -EncodedCommand takes the script as base64 of UTF-16LE, so no quoting can go wrong.
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
    let run = |flags: u32| {
        std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-EncodedCommand", &encoded])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(flags)
            .spawn()
    };
    run(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB)
        .or_else(|_| run(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP))
        .map(|_| ())
        .map_err(|e| format!("couldn't start the update helper: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, installer: &Path, built_at: &str) {
        let j = serde_json::json!({
            "version": "0.1.0", "commit": "abc123", "built_at": built_at,
            "installer": installer.display().to_string(),
        });
        // With a byte-order mark, as PowerShell 5.1 writes it.
        std::fs::write(dir.join("update.json"), format!("\u{feff}{j}")).unwrap();
    }

    #[test]
    fn newer_compares_times_not_text() {
        assert!(is_newer("2026-11-04T10:00:00Z", "2026-11-04T09:59:59Z"));
        assert!(!is_newer("2026-11-04T09:00:00Z", "2026-11-04T09:00:00Z"));
        assert!(!is_newer("2026-11-04T09:00:00Z", "2026-11-04T10:00:00+00:00"));
        // An hour ahead in another offset is the same instant.
        assert!(!is_newer("2026-11-04T10:00:00+01:00", "2026-11-04T09:00:00Z"));
        assert!(is_newer("garbled", "2026-11-04T09:00:00Z"));
    }

    #[test]
    fn staged_needs_its_installer() {
        let t = tempfile::tempdir().unwrap();
        let setup = t.path().join("Claude Kanban DSH_0.1.0_x64-setup.exe");
        write(t.path(), &setup, "2099-01-01T00:00:00Z");
        assert!(read_staged(t.path()).is_none(), "no installer yet");
        std::fs::write(&setup, b"MZ").unwrap();
        let s = read_staged(t.path()).expect("staged");
        assert_eq!(s.commit, "abc123");
        assert!(s.install_dir.is_none());
        assert!(ready_now(t.path()).is_some(), "2099 is newer than this build");
        write(t.path(), &setup, "2000-01-01T00:00:00Z");
        assert!(ready_now(t.path()).is_none(), "older than this build");
    }

    #[test]
    fn later_hides_only_that_build() {
        let t = tempfile::tempdir().unwrap();
        let setup = t.path().join("setup.exe");
        std::fs::write(&setup, b"MZ").unwrap();
        write(t.path(), &setup, "2099-01-01T00:00:00Z");
        let u = Updates::default();
        assert!(u.refresh(t.path()));
        assert!(!u.refresh(t.path()), "nothing changed");
        assert!(u.status().ready.is_some());
        *u.later.lock().unwrap() = Some("2099-01-01T00:00:00Z".into());
        assert!(u.status().ready.is_none());
        write(t.path(), &setup, "2099-01-02T00:00:00Z");
        assert!(u.refresh(t.path()));
        assert!(u.status().ready.is_some(), "a newer build shows again");
    }

    #[test]
    fn helper_quotes_paths() {
        let s = helper_script(
            42,
            Path::new(r"C:\Users\o'neil\.claude-kanban-dsh\update\Claude Kanban DSH_0.1.0_x64-setup.exe"),
            Path::new(r"C:\Users\o'neil\Programs\Claude Kanban DSH"),
            Path::new(r"C:\Users\o'neil\Programs\Claude Kanban DSH\claude-kanban-dsh.exe"),
            Path::new(r"C:\Users\o'neil\.claude-kanban-dsh\update\restart.log"),
        );
        assert!(s.contains("Wait-Process -Id 42"));
        assert!(s.contains(r"'/D=C:\Users\o''neil\Programs\Claude Kanban DSH'"));
        assert!(s.contains(r"-FilePath 'C:\Users\o''neil\Programs\Claude Kanban DSH\claude-kanban-dsh.exe'"));
        // Hugh clicked for the restart, so the app comes back in front, not in the background.
        assert!(!s.contains("--background"), "the restarted app must not start in the background");
        // The installer runs only after the wait, and the app starts after the installer.
        let (w, i, r) = (s.find("Wait-Process").unwrap(), s.find("'/S'").unwrap(), s.rfind("Start-Process -FilePath 'C:").unwrap());
        assert!(w < i && i < r);
    }

    /// The helper must be valid PowerShell: parse it (never run it) with PowerShell's own parser.
    #[test]
    fn helper_parses_as_powershell() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("helper.ps1");
        let s = helper_script(7, Path::new(r"C:\a b\setup.exe"), Path::new(r"C:\a b\App"), Path::new(r"C:\a b\App\x.exe"), Path::new(r"C:\a b\r.log"));
        std::fs::write(&file, s).unwrap();
        let cmd = format!(
            "$e=$null;$t=$null;[void][System.Management.Automation.Language.Parser]::ParseFile('{}',[ref]$t,[ref]$e);$e.Count",
            file.display()
        );
        let Ok(out) = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", &cmd]).output() else {
            return; // no PowerShell here
        };
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0", "parse errors in:\n{}", std::fs::read_to_string(&file).unwrap());
    }
}
