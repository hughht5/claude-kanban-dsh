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
    // The column prompts' folder (card #96). A live build always points at main's checkout, as
    // the board's own column prompts do, wherever it was built. A test build points at the
    // checkout it was built from, so a worktree's test app reads that worktree's prompts.
    println!("cargo:rerun-if-env-changed=KANBAN_CHANNEL");
    // Main's checkout is the folder that holds the repo's shared .git, whichever worktree builds
    // (for example C:\Users\someone\kanban-app). Paths take the target system's separator, so
    // a Mac build names /Users/<you>/kanban-app/prompts (card #117).
    let windows = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    let native = |p: &str| if windows { p.replace('/', "\\") } else { p.replace('\\', "/") };
    let sep = if windows { "\\" } else { "/" };
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(|p| p.display().to_string()).unwrap_or_default();
    let main_checkout = git(&["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .map(|g| g.trim_end_matches(['/', '\\']).trim_end_matches(".git").trim_end_matches(['/', '\\']).to_string())
        .unwrap_or_else(|| here.clone());
    let main_prompts = format!("{}{sep}prompts", native(&main_checkout));
    // Any test slot ("test", "test2", ...); the store crate refuses a KANBAN_CHANNEL that is not one.
    let prompts = if std::env::var("KANBAN_CHANNEL").is_ok_and(|c| c.starts_with("test")) {
        git(&["rev-parse", "--show-toplevel"]).map(|top| format!("{}{sep}prompts", native(&top))).unwrap_or_else(|| main_prompts.clone())
    } else {
        main_prompts
    };
    println!("cargo:rustc-env=KANBAN_PROMPTS_DIR={prompts}");
    println!("cargo:rustc-env=KANBAN_BUILT_AT={built_at}");

    tauri_build::build()
}
