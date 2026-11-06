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
    /// Why the last Restart now didn't install, once (macOS's helper records it; card #117).
    failed: Option<String>,
}

/// The macOS update helper's result, `restart-result.json` beside its log: the error of a
/// failed install, read once at the first status and then renamed, so the board says it once.
/// Windows' helper writes no such file.
fn last_failure(dir: &Path) -> Option<String> {
    let file = dir.join("restart-result.json");
    let text = std::fs::read_to_string(&file).ok()?;
    let _ = std::fs::rename(&file, dir.join("restart-result.seen.json"));
    let (ok, error, _) = kanban_unix::text::restart_result(&text)?;
    (!ok).then(|| error.unwrap_or_else(|| "the helper gave no reason".into()))
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
    /// Why the last Restart now didn't install (macOS), until Hugh clicks OK on it.
    failed: Mutex<Option<String>>,
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
        Status { running: Running { version: VERSION, commit: COMMIT, built_at: BUILT_AT }, ready, failed: self.failed.lock().ok().and_then(|f| f.clone()) }
    }

    /// Looks at the folder again; true if what is ready changed.
    fn refresh(&self, dir: &Path) -> bool {
        // A failed install's result is picked up at start and at any later look, so a helper
        // that gave up while this copy still ran is told too.
        let mut changed = false;
        if let Some(why) = last_failure(dir) {
            if let Ok(mut f) = self.failed.lock() {
                *f = Some(why);
                changed = true;
            }
        }
        let now = ready_now(dir);
        let Ok(mut ready) = self.ready.lock() else { return changed };
        if *ready == now {
            return changed;
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

/// What runs and what is ready. A change it finds is announced as the watcher would, so a caller
/// outside the page (`tools/cdp.mjs shot` reading the build, card #101) can't swallow the banner.
#[tauri::command]
pub fn update_status(app: AppHandle, updates: State<Updates>) -> Status {
    if updates.refresh(&folder()) {
        let _ = app.emit("update-ready", updates.status());
    }
    updates.status()
}

/// Later: hides this staged build's banner until the app restarts or a newer one is staged.
#[tauri::command]
pub fn update_later(updates: State<Updates>, built_at: String) {
    if let Ok(mut l) = updates.later.lock() {
        *l = Some(built_at);
    }
}

/// OK on "The last update didn't install": the notice goes until another install fails.
#[tauri::command]
pub fn update_failure_seen(updates: State<Updates>) {
    if let Ok(mut f) = updates.failed.lock() {
        *f = None;
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
    let script = crate::platform::update_helper(
        std::process::id(),
        Path::new(&staged.installer),
        &install_dir,
        &app_exe,
        &dir.join("restart.log"),
    );
    crate::platform::spawn_detached(&script)?;
    // The stops take a moment each, so they run off the command's thread, as tray > Quit's do.
    let a = app.clone();
    std::thread::spawn(move || {
        a.state::<Arc<Sessions>>().stop_all(&a);
        a.exit(0);
    });
    Ok(())
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
    fn a_failed_restart_is_told_once() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(last_failure(t.path()), None, "no result file: nothing to say");
        let file = t.path().join("restart-result.json");
        std::fs::write(&file, r#"{"ok":false,"error":"mv: rename failed: Operation not permitted","at":"2026-11-06T10:00:00Z"}"#).unwrap();
        assert_eq!(last_failure(t.path()).as_deref(), Some("mv: rename failed: Operation not permitted"));
        assert!(!file.exists() && t.path().join("restart-result.seen.json").is_file(), "kept, renamed");
        assert_eq!(last_failure(t.path()), None, "said once");
        std::fs::write(&file, r#"{"ok":true,"error":null,"at":"2026-11-06T10:00:00Z"}"#).unwrap();
        assert_eq!(last_failure(t.path()), None, "a good install says nothing");
    }

    #[test]
    fn the_failure_stays_until_ok() {
        let t = tempfile::tempdir().unwrap();
        let u = Updates::default();
        assert!(!u.refresh(t.path()));
        std::fs::write(t.path().join("restart-result.json"), r#"{"ok":false,"error":"ditto: no space","at":"2026-11-06T10:00:00Z"}"#).unwrap();
        assert!(u.refresh(t.path()), "a new failure is a change");
        assert!(!u.refresh(t.path()), "told once per failure");
        assert_eq!(u.status().failed.as_deref(), Some("ditto: no space"));
        assert_eq!(u.status().failed.as_deref(), Some("ditto: no space"), "kept across looks and reloads");
        *u.failed.lock().unwrap() = None;
        assert_eq!(u.status().failed, None);
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
}
