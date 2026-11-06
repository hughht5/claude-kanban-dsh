//! Tauri's build step, plus the build's identity for one-click updates (`src/update.rs`):
//! `KANBAN_GIT_COMMIT` and `KANBAN_BUILT_AT` are compiled in. `scripts\install.ps1` sets both
//! before it builds and writes the same two values into the staged update's `update.json`, so
//! the running app can tell whether a staged installer is newer than itself. A build made
//! without them (a plain `cargo build`) takes the checkout's commit and the time of this step.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn main() {
    println!("cargo:rerun-if-env-changed=KANBAN_GIT_COMMIT");
    println!("cargo:rerun-if-env-changed=KANBAN_BUILT_AT");
    // A new commit or checkout in this worktree rewrites its HEAD log, so the commit stays current.
    if let Some(dir) = git(&["rev-parse", "--git-dir"]) {
        println!("cargo:rerun-if-changed={dir}/HEAD");
        println!("cargo:rerun-if-changed={dir}/logs/HEAD");
    }

    let commit = std::env::var("KANBAN_GIT_COMMIT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| git(&["rev-parse", "--short=12", "HEAD"]))
        .unwrap_or_else(|| "unknown".into());
    let built_at = std::env::var("KANBAN_BUILT_AT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    println!("cargo:rustc-env=KANBAN_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=KANBAN_BUILT_AT={built_at}");

    tauri_build::build()
}
