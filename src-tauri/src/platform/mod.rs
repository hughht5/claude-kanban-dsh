//! Everything that differs between operating systems, behind one interface (card #114). Code
//! outside this folder never calls an operating-system API itself: `tests/platform_guard.rs`
//! fails the build's tests if it does.
//!
//! - Windows: `windows.rs`, the code that used to sit in sessions.rs, tray.rs, place.rs,
//!   notify.rs, update.rs, launch.rs, slash.rs, stack.rs and session_cmds.rs, moved as it was.
//! - macOS: `macos.rs` (card #117), with the same names. Its logic that needs no Tauri or
//!   AppKit is in the `kanban-unix` crate, which can be checked for macOS on any machine.
//!
//! What each system's module provides:
//!
//! - `Job` (`Job::new()`, `job.assign(pid)`) and `exit_waiter(pid)`: every card session ends
//!   when the app's process ends, and the app hears when one exits.
//! - `NoWindow` (`command.no_window()`): a helper process starts without a console window.
//! - `claude_candidates(home)`: where Claude Code may be installed (the newest one is used).
//! - `no_home()`: the folder used when the system names no home folder.
//! - `chrome_paths()`: where Chrome may be installed, for logins in a Chrome profile.
//! - `toast(app, card_id, head, body)`: a notification; clicking it opens the card.
//! - `work_area(w)`, `virtual_screen(w)`, `place_window(w, rect, background)`, `hide_window(w)`:
//!   where the board window goes, and hiding it to the tray.
//! - `on_quit_signal(app, quit)`: the install script asks the running app to quit cleanly.
//! - `update_helper(pid, installer, install_dir, app_exe, log)` and `spawn_detached(script)`:
//!   Restart now's helper, which installs the staged update once the app has exited.
//! - `file_url_path(s)`: the local path in a `file://` URL.
//! - `at_start()`, `builder(b)` and `on_run_event(app, event)`: hooks main.rs calls at start-up,
//!   on the app's builder before its plugins, and on every run event, so a system that needs
//!   one doesn't edit main.rs. Nothing on Windows.
//! - Card #117, nothing new on Windows: `tray_style(b)`, `tray_count(tray, n)` and
//!   `TRAY_LEFT_CLICK_SHOWS`, the tray or menu-bar icon; `SECOND_ACCOUNT`, whether account 2 can
//!   run here; `DEV_STACKS`, whether WSL dev stacks can; `write_private(path, bytes)`,
//!   a file only this user can read (the API token).
//! - The test-app lock files (testapp.rs, wave 5 merge): `share_all(options)`, a file opened
//!   with every kind of sharing; `process_start(pid)` and `PID_LOCKS`, whether a lock's writer
//!   is checked by its pid and start time (Windows) or the file's presence is enough (macOS).
//! - `install_command(channel)` (card #147): the command a card session runs to install its
//!   build into a test app.
//! - `in_settings_folder(home, path)` (card #125): the system's settings folders a card's proof
//!   never reads (AppData outside its Temp on Windows, ~/Library on macOS), and `plain_path(p)`, a
//!   resolved path without Windows' `\\?\` prefix.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::*;

#[cfg(not(any(windows, target_os = "macos")))]
compile_error!("Claude Kanban DSH runs on Windows and macOS only (src-tauri/src/platform/).");
