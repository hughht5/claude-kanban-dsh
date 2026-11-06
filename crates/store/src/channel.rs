//! Which channel this build is: "live" (the app Hugh works in) or a test slot (a side-by-side
//! copy for trying changes): "test", "test2", "test3", ... It is fixed when the build is made,
//! from `KANBAN_CHANNEL`. Unset, empty or "live" is live; any other value that is not a test
//! slot stops the build (see the `assert!` below), so a typo never makes a build that opens the live
//! board. No two channels share a data folder, so never a board, a port or a token.
//!
//! Every name a slot needs is derived here from its number (test is slot 1, testN is slot N):
//! `scripts\install.ps1` derives the same names (install folder, exe, identifier, quit event)
//! by the same rules, and a test in `crates/store/tests` holds the two to each other.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The DevTools port of the live app; test slot N uses `LIVE_DEVTOOLS_PORT + N`.
pub const LIVE_DEVTOOLS_PORT: u16 = 9333;

/// The slot number of a channel name: "live" 0, "test" 1, "testN" N (2 to 10). None for
/// anything else, including "test1" and "test02", so each slot has one name only.
/// A `const fn`, so the compile-time check below uses this same rule.
pub const fn slot_of(channel: &str) -> Option<u32> {
    let b = channel.as_bytes();
    if b.len() == 4 && b[0] == b'l' && b[1] == b'i' && b[2] == b'v' && b[3] == b'e' {
        return Some(0);
    }
    if b.len() < 4 || b[0] != b't' || b[1] != b'e' || b[2] != b's' || b[3] != b't' {
        return None;
    }
    match b.len() {
        4 => Some(1),
        5 => match digit(b[4]) {
            Some(n) if n >= 2 => Some(n),
            _ => None,
        },
        6 if b[4] == b'1' && b[5] == b'0' => Some(10),
        _ => None,
    }
}

const fn digit(c: u8) -> Option<u32> {
    if c.is_ascii_digit() { Some((c - b'0') as u32) } else { None }
}

/// Unset or empty is live; anything else must be a channel name.
const fn valid(raw: Option<&str>) -> bool {
    match raw {
        None => true,
        Some(s) => s.is_empty() || slot_of(s).is_some(),
    }
}

const _: () = assert!(
    valid(option_env!("KANBAN_CHANNEL")),
    "KANBAN_CHANNEL must be unset, live, test or testN (N 2 to 10)"
);

/// "live", "test", "test2", ... as built (`KANBAN_CHANNEL`).
pub fn channel() -> &'static str {
    match option_env!("KANBAN_CHANNEL") {
        None | Some("") | Some("live") => "live",
        Some(c) => c,
    }
}

/// This build's slot: 0 for live, 1 for test, N for testN.
pub fn slot() -> u32 {
    slot_of(channel()).unwrap_or(0)
}

pub fn is_test() -> bool {
    slot() > 0
}

/// How a slot is named to Hugh: "test", "test 2", "test 3". Empty for live.
pub fn label_for(slot: u32) -> String {
    match slot {
        0 => String::new(),
        1 => "test".into(),
        n => format!("test {n}"),
    }
}

/// The name shown in the window title, the tray tooltip and toasts: "Claude Kanban DSH",
/// "Claude Kanban DSH (test)", "Claude Kanban DSH (test 2)".
pub fn app_name_for(slot: u32) -> String {
    if slot == 0 { "Claude Kanban DSH".into() } else { format!("Claude Kanban DSH ({})", label_for(slot)) }
}

pub fn app_name() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| app_name_for(slot()))
}

/// The named event the app waits on so `scripts\install.ps1` can ask it to quit cleanly:
/// `Local\ClaudeKanbanDshQuit`, `Local\ClaudeKanbanDshQuitTest`, `Local\ClaudeKanbanDshQuitTest2`.
pub fn quit_event_for(slot: u32) -> String {
    match slot {
        0 => r"Local\ClaudeKanbanDshQuit".into(),
        1 => r"Local\ClaudeKanbanDshQuitTest".into(),
        n => format!(r"Local\ClaudeKanbanDshQuitTest{n}"),
    }
}

pub fn quit_event() -> &'static str {
    static EVENT: OnceLock<String> = OnceLock::new();
    EVENT.get_or_init(|| quit_event_for(slot()))
}

/// The suffix the install names take: "", " Test", " Test 2" (install folder and product name),
/// and "", "-test", "-test2" (exe, data folder, identifier).
fn suffixes(slot: u32) -> (String, String) {
    match slot {
        0 => (String::new(), String::new()),
        1 => (" Test".into(), "-test".into()),
        n => (format!(" Test {n}"), format!("-test{n}")),
    }
}

/// The product name and install folder under `Programs`: "Claude Kanban DSH", "Claude Kanban DSH Test",
/// "Claude Kanban DSH Test 2".
pub fn product_name_for(slot: u32) -> String {
    format!("Claude Kanban DSH{}", suffixes(slot).0)
}

/// The exe's name without `.exe`: "claude-kanban-dsh", "claude-kanban-dsh-test", "claude-kanban-dsh-test2".
pub fn exe_name_for(slot: u32) -> String {
    format!("claude-kanban-dsh{}", suffixes(slot).1)
}

/// The Tauri identifier, which also keys the single-instance lock and the WebView profile:
/// "io.github.hughht5.claude-kanban-dsh", "io.github.hughht5.claude-kanban-dsh.test", "io.github.hughht5.claude-kanban-dsh.test2".
pub fn identifier_for(slot: u32) -> String {
    match slot {
        0 => "io.github.hughht5.claude-kanban-dsh".into(),
        1 => "io.github.hughht5.claude-kanban-dsh.test".into(),
        n => format!("io.github.hughht5.claude-kanban-dsh.test{n}"),
    }
}

/// The DevTools port to launch the slot with: live 9333, test 9334, test 2 9335, test 3 9336.
pub fn devtools_port_for(slot: u32) -> u16 {
    LIVE_DEVTOOLS_PORT + slot as u16
}

/// The data folder for `channel` under `home`: `.claude-kanban-dsh`, `.claude-kanban-dsh-test`,
/// `.claude-kanban-dsh-test2`. Anything that is not a channel is live's.
pub fn data_dir_for(channel: &str, home: &Path) -> PathBuf {
    home.join(format!(".claude-kanban-dsh{}", suffixes(slot_of(channel).unwrap_or(0)).1))
}

/// The live board, `%USERPROFILE%\.claude-kanban-dsh\board.db`, whichever channel this build is.
/// `kb copy-board` reads it by default.
pub fn live_board() -> PathBuf {
    data_dir_for("live", &home()).join("board.db")
}

/// The user's home folder as the system names it: `USERPROFILE` on Windows, `HOME` elsewhere.
/// None when it isn't set. The one place the home folder is read (card #114): the store, `kb`
/// and the app all use it, each with its own fallback.
pub fn home_var() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// The variable a test app sets for its card sessions (finding F1 of the final test, card #118):
/// the data folder of the board they belong to. `kb` and `kb mcp` read it first, so a test
/// session's `kb`, even the live one on PATH, reaches the test board and never the live one.
/// The live app never sets it, and strips it from what its sessions inherit.
pub const DATA_DIR_ENV: &str = "KANBAN_DATA_DIR";

/// The board folder that `kb` uses: `KANBAN_DATA_DIR` when it is set and not empty, else
/// `own` (this build's channel's folder).
pub fn kb_data_dir(var: Option<std::ffi::OsString>, own: impl FnOnce() -> PathBuf) -> PathBuf {
    match var {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => own(),
    }
}

pub(crate) fn home() -> PathBuf {
    home_var().unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_paths() {
        let home = Path::new(r"C:\Users\x");
        assert_eq!(data_dir_for("live", home), home.join(".claude-kanban-dsh"));
        assert_eq!(data_dir_for("test", home), home.join(".claude-kanban-dsh-test"));
        assert_eq!(data_dir_for("test2", home), home.join(".claude-kanban-dsh-test2"));
        assert_eq!(data_dir_for("test3", home), home.join(".claude-kanban-dsh-test3"));
        assert_eq!(data_dir_for("anything else", home), home.join(".claude-kanban-dsh"));
        assert_eq!(data_dir_for("test1", home), home.join(".claude-kanban-dsh"));
    }

    #[test]
    fn slots_have_one_name_each() {
        assert_eq!(slot_of("live"), Some(0));
        assert_eq!(slot_of("test"), Some(1));
        assert_eq!(slot_of("test2"), Some(2));
        assert_eq!(slot_of("test3"), Some(3));
        assert_eq!(slot_of("test10"), Some(10));
        assert_eq!(devtools_port_for(10), 9343);
        for bad in ["", "test1", "test0", "test02", "test11", "test99", "test100", "test-2", "Test2", "testx", "tes", "live2"] {
            assert_eq!(slot_of(bad), None, "{bad}");
        }
        // The compile-time check agrees with slot_of.
        for s in ["live", "test", "test2", "test3", "test10"] {
            assert!(valid(Some(s)), "{s}");
        }
        for s in ["test1", "test0", "test02", "test11", "test99", "test100", "test-2", "Test2", "testx", "tes", "live2"] {
            assert!(!valid(Some(s)), "{s}");
        }
        assert!(valid(None) && valid(Some("")));
    }

    #[test]
    fn test_keeps_its_names_and_slots_derive_theirs() {
        // Live and test are exactly as they were before slots.
        assert_eq!((app_name_for(0), app_name_for(1)), ("Claude Kanban DSH".into(), "Claude Kanban DSH (test)".into()));
        assert_eq!((quit_event_for(0), quit_event_for(1)), (r"Local\ClaudeKanbanDshQuit".into(), r"Local\ClaudeKanbanDshQuitTest".into()));
        assert_eq!((product_name_for(0), product_name_for(1)), ("Claude Kanban DSH".into(), "Claude Kanban DSH Test".into()));
        assert_eq!((exe_name_for(0), exe_name_for(1)), ("claude-kanban-dsh".into(), "claude-kanban-dsh-test".into()));
        assert_eq!((identifier_for(0), identifier_for(1)), ("io.github.hughht5.claude-kanban-dsh".into(), "io.github.hughht5.claude-kanban-dsh.test".into()));
        assert_eq!((devtools_port_for(0), devtools_port_for(1)), (9333, 9334));
        // Test 2 and test 3.
        assert_eq!(app_name_for(2), "Claude Kanban DSH (test 2)");
        assert_eq!(quit_event_for(3), r"Local\ClaudeKanbanDshQuitTest3");
        assert_eq!(product_name_for(2), "Claude Kanban DSH Test 2");
        assert_eq!(exe_name_for(3), "claude-kanban-dsh-test3");
        assert_eq!(identifier_for(2), "io.github.hughht5.claude-kanban-dsh.test2");
        assert_eq!((devtools_port_for(2), devtools_port_for(3)), (9335, 9336));
    }

    #[test]
    fn kb_follows_the_session_board() {
        let own = || PathBuf::from("/x/.claude-kanban-dsh");
        assert_eq!(kb_data_dir(None, own), own());
        assert_eq!(kb_data_dir(Some("".into()), own), own());
        let test2 = "/x/.claude-kanban-dsh-test2";
        assert_eq!(kb_data_dir(Some(test2.into()), own), PathBuf::from(test2));
    }

    #[test]
    fn this_build_matches_its_names() {
        assert!(slot_of(channel()).is_some());
        assert_eq!(quit_event(), quit_event_for(slot()));
        assert_eq!(app_name(), app_name_for(slot()));
        assert_eq!(app_name().contains("(test"), is_test());
    }
}
