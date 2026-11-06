//! A card's dev stack, for its login buttons: whether it is up (`launch::stack_up`), which git
//! branch its checkout is on, and, when it is down, the command that restarts it.
//!
//! Stacks are a repo's `scripts/dev-stack.sh`, run inside WSL from the branch's own checkout
//! (the card's `worktreePath`). **The app never runs WSL or dev-stack commands** (PLAN.md), so
//! the restart is shown for Hugh to paste into a WSL terminal, never run. The branch is read
//! with Windows git, and only from a checkout on a Windows drive; a checkout inside WSL's own
//! file system (`\\wsl$\...`, `/home/...`) is never touched, and the card's `branch` is used.

use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    /// Whether the stack answers its sign-in route (always true for links not on 127.0.0.1).
    pub up: bool,
    /// Whether the link is on a local stack at all.
    pub local: bool,
    pub branch: Option<String>,
    /// "checkout" (read from the worktree with git) or "card" (the card's `branch` field).
    pub branch_from: Option<&'static str>,
    /// The card's `branch`, when the checkout is on a different one.
    pub card_branch: Option<String>,
    /// When the stack is down: what to run, where, and that the app won't run it.
    pub restart: Option<Restart>,
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Restart {
    pub command: String,
    /// Where to run it ("a WSL terminal").
    pub run_in: &'static str,
    pub note: String,
}

fn is_local(url: &str) -> bool {
    url.starts_with("http://127.0.0.1:")
}

/// A checkout path inside WSL's own file system, which Windows tools must not open.
pub(crate) fn in_wsl(path: &str) -> bool {
    let p = path.replace('/', "\\").to_ascii_lowercase();
    // A `/` path is WSL's on Windows; on a Mac it is an ordinary local folder (card #117).
    p.starts_with("\\\\wsl$\\") || p.starts_with("\\\\wsl.localhost\\") || (crate::platform::DEV_STACKS && path.starts_with('/'))
}

/// Dev stacks need WSL, so they run on Windows only in this version (card #117, D6).
const NO_DEV_STACKS: &str = "Dev stacks run on Windows only in this version.";

/// The same folder as WSL sees it: `C:\a\b` -> `/mnt/c/a/b`, `\\wsl$\Ubuntu\home\x` -> `/home/x`.
pub fn wsl_path(path: &str) -> Option<String> {
    if path.starts_with('/') {
        return Some(path.to_string());
    }
    let p = path.replace('\\', "/");
    let lower = p.to_ascii_lowercase();
    for prefix in ["//wsl$/", "//wsl.localhost/"] {
        if lower.starts_with(prefix) {
            // Skip the distro name.
            let rest = &p[prefix.len()..];
            let after = rest.find('/').map(|i| &rest[i..]).unwrap_or("/");
            return Some(after.to_string());
        }
    }
    let b = p.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        let rest = p[2..].trim_end_matches('/');
        return Some(format!("/mnt/{}{}", (b[0] as char).to_ascii_lowercase(), rest));
    }
    None
}

/// A bash word: bare when it is plainly safe, otherwise in single quotes.
pub fn sh_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "._/-+:@".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// The restart command, as the kanban skill gives it: `scripts/dev-stack.sh restart <branch>`
/// from the branch's own checkout.
pub fn restart_command(worktree: Option<&str>, branch: Option<&str>) -> String {
    let branch = branch.map(sh_quote).unwrap_or_else(|| "<branch>".into());
    let cmd = format!("scripts/dev-stack.sh restart {branch}");
    match worktree.and_then(wsl_path) {
        Some(dir) => format!("cd {} && {cmd}", sh_quote(&dir)),
        None => cmd,
    }
}

/// `git rev-parse --abbrev-ref HEAD` in a checkout on a Windows drive, with no console window.
fn checkout_branch(worktree: &str) -> Option<String> {
    use crate::platform::NoWindow;
    if in_wsl(worktree) || !Path::new(worktree).is_dir() {
        return None;
    }
    let out = std::process::Command::new("git")
        .args(["-C", worktree, "rev-parse", "--abbrev-ref", "HEAD"])
        .stdin(std::process::Stdio::null())
        .no_window()
        .output()
        .ok()?;
    let b = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !b.is_empty()).then_some(b)
}

pub fn health(url: &str, worktree: Option<&str>, card_branch: Option<&str>) -> Health {
    let worktree = worktree.map(str::trim).filter(|w| !w.is_empty());
    let card_branch = card_branch.map(str::trim).filter(|b| !b.is_empty());
    let local = is_local(url);
    let up = crate::launch::stack_up(url);
    let (branch, branch_from) = match worktree.and_then(checkout_branch) {
        Some(b) => (Some(b), Some("checkout")),
        None => (card_branch.map(String::from), card_branch.map(|_| "card")),
    };
    let differs = matches!((branch_from, &branch, card_branch), (Some("checkout"), Some(b), Some(c)) if b != c);
    let restart = (local && !up).then(|| Restart {
        command: restart_command(worktree, branch.as_deref()),
        run_in: "a WSL terminal",
        note: if !crate::platform::DEV_STACKS {
            NO_DEV_STACKS.into()
        } else if worktree.is_some() {
            "Paste it into a WSL terminal. The app never runs WSL commands itself.".into()
        } else {
            "The card has no worktreePath, so run it from the branch's own checkout, in a WSL terminal. The app never runs WSL commands itself.".into()
        },
    });
    Health { up, local, branch, branch_from, card_branch: differs.then(|| card_branch.unwrap_or_default().to_string()), restart }
}

#[tauri::command]
pub async fn stack_health(url: String, worktree: Option<String>, branch: Option<String>) -> Health {
    let fallback = Health { up: true, local: false, branch: None, branch_from: None, card_branch: None, restart: None };
    tauri::async_runtime::spawn_blocking(move || health(&url, worktree.as_deref(), branch.as_deref()))
        .await
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    // WSL is Windows only: these three tests run on Windows (card #114).
    #[cfg(windows)]
    #[test]
    fn wsl_paths() {
        assert_eq!(wsl_path(r"C:\Users\me\web\wt-123").as_deref(), Some("/mnt/c/Users/me/web/wt-123"));
        assert_eq!(wsl_path(r"D:\").as_deref(), Some("/mnt/d"));
        assert_eq!(wsl_path(r"\\wsl$\Ubuntu\home\me\web").as_deref(), Some("/home/me/web"));
        assert_eq!(wsl_path(r"\\wsl.localhost\Ubuntu-22.04\home\h").as_deref(), Some("/home/h"));
        assert_eq!(wsl_path("/home/h/x").as_deref(), Some("/home/h/x"));
        assert_eq!(wsl_path("relative"), None);
        assert!(in_wsl(r"\\wsl$\Ubuntu\home") && in_wsl("//wsl.localhost/U/x") && in_wsl("/home/x"));
        assert!(!in_wsl(r"C:\Users\x"));
    }

    #[cfg(windows)]
    #[test]
    fn commands_in_wsl() {
        assert_eq!(
            restart_command(Some(r"C:\Users\me\web\my wt"), Some("feat/login-v2")),
            "cd '/mnt/c/Users/me/web/my wt' && scripts/dev-stack.sh restart feat/login-v2"
        );
    }

    #[test]
    fn commands() {
        assert_eq!(restart_command(None, Some("x;rm -rf ~")), "scripts/dev-stack.sh restart 'x;rm -rf ~'");
        assert_eq!(restart_command(None, None), "scripts/dev-stack.sh restart <branch>");
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn non_local_links_need_no_restart() {
        let h = health("https://app.example.com", None, Some("main"));
        assert!(h.up && !h.local && h.restart.is_none());
        assert_eq!((h.branch.as_deref(), h.branch_from), (Some("main"), Some("card")));
    }

    #[cfg(windows)]
    #[test]
    fn a_down_stack_gets_a_command() {
        // Port 1 on 127.0.0.1 never answers here.
        let h = health("http://127.0.0.1:1/login.html?as=admin", Some(r"\\wsl$\Ubuntu\home\h\wt"), Some("b1"));
        assert!(!h.up && h.local);
        assert_eq!(h.branch_from, Some("card"), "a WSL checkout is never opened from Windows");
        assert_eq!(h.restart.unwrap().command, "cd /home/h/wt && scripts/dev-stack.sh restart b1");
    }

    #[test]
    fn reads_the_branch_of_a_windows_checkout() {
        let here = env!("CARGO_MANIFEST_DIR");
        if let Some(b) = checkout_branch(here) {
            let h = health("https://example.com", Some(here), Some("definitely-not-this-branch"));
            assert_eq!(h.branch.as_deref(), Some(b.as_str()));
            assert_eq!(h.branch_from, Some("checkout"));
            assert_eq!(h.card_branch.as_deref(), Some("definitely-not-this-branch"));
        }
    }
}
