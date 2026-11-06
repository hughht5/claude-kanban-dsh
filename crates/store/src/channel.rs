//! Which channel this build is: "live" (the app Hugh works in) or "test" (a side-by-side copy
//! for trying changes). It is fixed when the build is made, from `KANBAN_CHANNEL=test`; any
//! other value, or none, is live. The two channels never share a data folder, so never a
//! board, a port or a token.

use std::path::{Path, PathBuf};

/// "test" when built with `KANBAN_CHANNEL=test`, otherwise "live".
pub fn channel() -> &'static str {
    match option_env!("KANBAN_CHANNEL") {
        Some("test") => "test",
        _ => "live",
    }
}

pub fn is_test() -> bool {
    channel() == "test"
}

/// The name shown in the window title, the tray tooltip and toasts.
pub fn app_name() -> &'static str {
    if is_test() { "Claude Kanban DSH (test)" } else { "Claude Kanban DSH" }
}

/// The named event the app waits on so `scripts\install.ps1` can ask it to quit cleanly.
pub fn quit_event() -> &'static str {
    if is_test() { r"Local\ClaudeKanbanDshQuitTest" } else { r"Local\ClaudeKanbanDshQuit" }
}

/// The data folder for `channel` under `home`: `.claude-kanban-dsh`, or `.claude-kanban-dsh-test`.
pub fn data_dir_for(channel: &str, home: &Path) -> PathBuf {
    home.join(if channel == "test" { ".claude-kanban-dsh-test" } else { ".claude-kanban-dsh" })
}

/// The live board, `%USERPROFILE%\.claude-kanban-dsh\board.db`, whichever channel this build is.
/// `kb copy-board` reads it by default.
pub fn live_board() -> PathBuf {
    data_dir_for("live", &home()).join("board.db")
}

pub(crate) fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_paths() {
        let home = Path::new(r"C:\Users\x");
        assert_eq!(data_dir_for("live", home), home.join(".claude-kanban-dsh"));
        assert_eq!(data_dir_for("test", home), home.join(".claude-kanban-dsh-test"));
        assert_eq!(data_dir_for("anything else", home), home.join(".claude-kanban-dsh"));
    }

    #[test]
    fn this_build_matches_its_names() {
        assert!(channel() == "live" || channel() == "test");
        assert_eq!(quit_event().ends_with("Test"), is_test());
        assert_eq!(app_name().contains("(test)"), is_test());
    }
}
