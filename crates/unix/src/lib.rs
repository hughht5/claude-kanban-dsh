//! The macOS side of Claude Kanban DSH (card #117), with no Tauri and no SQLite, so it can be
//! checked for macOS on any machine (`cargo check -p kanban-unix --target aarch64-apple-darwin`).
//!
//! - `text`: pure rules and script text, on every system.
//! - `track`: the session tracker's record rules (every system) and its sampler (macOS).
//! - `lock`: the test-app install lock's waiting rules and file parsing (every system), the
//!   descriptor operations (Unix) and the `kb install-lock` loop (macOS).
//! - `sys`, `env`, `reap`: the macOS system calls, the login-shell import and the reaper.

pub mod lock;
pub mod text;
pub mod track;

#[cfg(target_os = "macos")]
pub mod env;
#[cfg(target_os = "macos")]
pub mod reap;
#[cfg(target_os = "macos")]
pub mod sys;
