//! What the Settings screen edits: each column's prompt, model and effort, the map from a
//! login's browser to its Chrome profile, the session cap, and the allow-list of settings the
//! page may read and write. Config rows are updated in place (upserts); nothing is removed.

use crate::{secret::looks_like_token, ColumnCfg, Error, Result, Store, MIGRATIONS};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// The efforts a column (or a card's override) may ask for.
pub const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// The settings key holding the browser-profile map, as JSON (a list of `Profile`).
pub const PROFILES_KEY: &str = "launch.profiles";
/// The settings key for the most card sessions running at once.
pub const MAX_RUNNING_KEY: &str = "sessions.max_running";
pub const MAX_RUNNING_DEFAULT: i64 = 10;
/// The settings key for how many idle minutes pause a session (0: never).
pub const IDLE_PAUSE_KEY: &str = "sessions.idle_pause_minutes";
pub const IDLE_PAUSE_DEFAULT: i64 = 30;

/// Whether a card's session may move it from In progress to Human test only after a
/// card-tester subagent passed it (Hugh, card #76): "on" (the default) or "off".
pub const TEST_RULE_KEY: &str = "sessions.test_before_human_test";

/// The settings key for the private folder: a folder outside this repo with the owner's own
/// column prompts (`prompts\`) and repo list (`repos.json`). Empty: the bundled examples.
pub const PRIVATE_DIR_KEY: &str = "private.dir";

/// The keys the page may read and write with `get_setting` / `set_setting`.
pub const PAGE_SETTINGS: [&str; 6] = [PROFILES_KEY, MAX_RUNNING_KEY, IDLE_PAUSE_KEY, crate::STACK_RULE_KEY, TEST_RULE_KEY, PRIVATE_DIR_KEY];

/// One row of the browser-profile map: a login whose `browser` label is `name`, or whose link
/// matches `url_pattern` (`*` matches anything, case-insensitive; empty matches nothing), opens
/// in Chrome with `--profile-directory=<profile_dir>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub url_pattern: String,
    pub profile_dir: String,
}

/// The map as it was written in launch.rs before it moved into settings.
pub fn default_profiles() -> Vec<Profile> {
    vec![
        Profile { name: "Chrome · work (Profile 1)".into(), url_pattern: String::new(), profile_dir: "Profile 1".into() },
        Profile { name: "Chrome · Default (personal)".into(), url_pattern: String::new(), profile_dir: "Default".into() },
    ]
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    // Iterative wildcard match with backtracking to the last `*`.
    let (mut pi, mut ti, mut star, mut mark) = (0usize, 0usize, None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && p[pi] != '*' && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// The Chrome profile folder for a login: the row named by its `browser` label, else the first
/// row whose URL pattern matches its link. None opens the link in the default browser.
pub fn profile_for<'a>(profiles: &'a [Profile], browser: &str, url: &str) -> Option<&'a str> {
    profiles
        .iter()
        .find(|p| !browser.is_empty() && p.name == browser)
        .or_else(|| profiles.iter().find(|p| !p.url_pattern.trim().is_empty() && glob_match(p.url_pattern.trim(), url)))
        .map(|p| p.profile_dir.as_str())
}

fn check_profiles(json: &str) -> Result<Vec<Profile>> {
    let rows: Vec<Profile> = serde_json::from_str(json)?;
    for (i, p) in rows.iter().enumerate() {
        let n = i + 1;
        if p.name.trim().is_empty() {
            return Err(Error::Invalid(format!("profile row {n} has no name")));
        }
        let dir = p.profile_dir.trim();
        if dir.is_empty() {
            return Err(Error::Invalid(format!("profile row {n} ({}) has no profile folder", p.name)));
        }
        // A folder name inside Chrome's user data folder, such as "Default" or "Profile 3".
        if dir.contains(['/', '\\', '"', ':']) || dir.contains("..") {
            return Err(Error::Invalid(format!("profile row {n}: \"{dir}\" should be a folder name like Default or Profile 1, not a path")));
        }
        if looks_like_token(&p.name) || looks_like_token(&p.url_pattern) {
            return Err(Error::Secret(format!("profile row {n}")));
        }
    }
    Ok(rows)
}

/// A column's editable settings as they ship: every migration replayed on an empty board.
fn shipped_columns() -> Result<Vec<ColumnCfg>> {
    let mut conn = Connection::open_in_memory()?;
    for sql in MIGRATIONS.iter() {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.commit()?;
    }
    read_columns(&conn)
}

pub(crate) fn read_columns(conn: &Connection) -> Result<Vec<ColumnCfg>> {
    let mut st = conn.prepare(
        "SELECT name, prompt_text, mode, model, effort, permission_mode, auto_spawn, on_enter FROM columns ORDER BY position, name",
    )?;
    let rows = st
        .query_map([], |r| {
            Ok(ColumnCfg {
                name: r.get(0)?,
                prompt_text: r.get(1)?,
                mode: r.get(2)?,
                model: r.get(3)?,
                effort: r.get(4)?,
                permission_mode: r.get(5)?,
                auto_spawn: r.get::<_, i64>(6)? != 0,
                on_enter: r.get(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn blank_to_none(s: Option<&str>) -> Option<String> {
    s.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

impl Store {
    /// Every column's settings, in board order.
    pub fn columns(&self) -> Result<Vec<ColumnCfg>> {
        read_columns(&self.conn)
    }

    /// A column's settings as they ship (the migrations' values), for "Reset to default".
    pub fn column_default(&self, name: &str) -> Result<ColumnCfg> {
        shipped_columns()?
            .into_iter()
            .find(|c| c.name == name)
            .ok_or_else(|| Error::Invalid(format!("no column {name}")))
    }

    /// Sets a column's prompt, model and effort. An empty prompt or model clears it (the
    /// column then sends no prompt, or the app's default model is used).
    pub fn update_column(&mut self, name: &str, prompt_text: Option<&str>, model: Option<&str>, effort: Option<&str>) -> Result<ColumnCfg> {
        let prompt = prompt_text.map(str::to_string).filter(|p| !p.trim().is_empty());
        let model = blank_to_none(model);
        let effort = blank_to_none(effort);
        if let Some(e) = &effort {
            if !EFFORTS.contains(&e.as_str()) {
                return Err(Error::Invalid(format!("effort must be one of {}", EFFORTS.join(", "))));
            }
        }
        if let Some(m) = &model {
            if m.contains(char::is_whitespace) || m.len() > 100 {
                return Err(Error::Invalid(format!("\"{m}\" isn't a model name")));
            }
        }
        if prompt.as_deref().map_or(false, looks_like_token) {
            return Err(Error::Secret("prompt".into()));
        }
        let n = self.conn.execute(
            "UPDATE columns SET prompt_text = ?1, model = ?2, effort = ?3 WHERE name = ?4",
            params![prompt, model, effort, name],
        )?;
        if n == 0 {
            return Err(Error::Invalid(format!("no column {name}")));
        }
        self.column(name)?.ok_or_else(|| Error::Invalid(format!("no column {name}")))
    }

    /// Puts a column's prompt, model and effort back to the shipped values.
    pub fn reset_column(&mut self, name: &str) -> Result<ColumnCfg> {
        let d = self.column_default(name)?;
        self.conn.execute(
            "UPDATE columns SET prompt_text = ?1, model = ?2, effort = ?3 WHERE name = ?4",
            params![d.prompt_text, d.model, d.effort, name],
        )?;
        self.column(name)?.ok_or_else(|| Error::Invalid(format!("no column {name}")))
    }

    /// The browser-profile map: the saved one, else the default. A saved value that no longer
    /// parses falls back to the default rather than breaking every login.
    pub fn launch_profiles(&self) -> Result<Vec<Profile>> {
        Ok(match self.setting(PROFILES_KEY)? {
            Some(v) => serde_json::from_str(&v).unwrap_or_else(|_| default_profiles()),
            None => default_profiles(),
        })
    }

    /// The most card sessions that run at once (`sessions.max_running`, default 10).
    pub fn max_running(&self) -> Result<i64> {
        Ok(self
            .setting(MAX_RUNNING_KEY)?
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(MAX_RUNNING_DEFAULT))
    }

    /// Minutes a session may sit idle before the app pauses it (`sessions.idle_pause_minutes`,
    /// default 30; 0 never pauses).
    pub fn idle_pause_minutes(&self) -> Result<i64> {
        Ok(self
            .setting(IDLE_PAUSE_KEY)?
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(IDLE_PAUSE_DEFAULT))
    }

    /// Must a card pass its subagent test before its session moves it to Human test? (Hugh's
    /// setting; on unless he turned it off.)
    pub fn test_required(&self) -> bool {
        self.setting(TEST_RULE_KEY).ok().flatten().as_deref() != Some("off")
    }

    /// A setting the page may read: its saved value, else its default.
    pub fn page_setting(&self, key: &str) -> Result<String> {
        page_key(key)?;
        Ok(match self.setting(key)? {
            Some(v) => v,
            None => page_default(key)?,
        })
    }

    /// A setting the page may write, checked for its key. Returns the value as stored.
    pub fn set_page_setting(&mut self, key: &str, value: &str) -> Result<String> {
        page_key(key)?;
        let stored = match key {
            MAX_RUNNING_KEY => {
                let n: i64 = value
                    .trim()
                    .parse()
                    .map_err(|_| Error::Invalid("max running sessions must be a whole number".into()))?;
                if !(1..=100).contains(&n) {
                    return Err(Error::Invalid("max running sessions must be between 1 and 100".into()));
                }
                n.to_string()
            }
            PROFILES_KEY => serde_json::to_string(&check_profiles(value)?)?,
            IDLE_PAUSE_KEY => {
                let n: i64 = value
                    .trim()
                    .parse()
                    .map_err(|_| Error::Invalid("idle minutes must be a whole number".into()))?;
                if !(0..=1440).contains(&n) {
                    return Err(Error::Invalid("idle minutes must be between 0 (never) and 1440".into()));
                }
                n.to_string()
            }
            crate::STACK_RULE_KEY => match value.trim() {
                v @ ("all" | "first") => v.to_string(),
                _ => return Err(Error::Invalid("the stack rule is \"all\" or \"first\"".into())),
            },
            TEST_RULE_KEY => match value.trim() {
                v @ ("on" | "off") => v.to_string(),
                _ => return Err(Error::Invalid("the subagent test rule is \"on\" or \"off\"".into())),
            },
            PRIVATE_DIR_KEY => check_private_dir(value)?,
            _ => unreachable!(),
        };
        self.set_setting(key, &stored)?;
        Ok(stored)
    }
}

/// A private folder is empty (none), a full path, or a path under `~`.
fn check_private_dir(value: &str) -> Result<String> {
    let v = value.trim().trim_matches('"').trim_end_matches(['\\', '/']);
    let b = v.as_bytes();
    let full = v.starts_with('~') || v.starts_with(r"\\") || (b.len() > 2 && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'));
    if !v.is_empty() && !full {
        return Err(Error::Invalid(r"the private folder must be a full path (C:\...) or start with ~".into()));
    }
    Ok(v.to_string())
}

fn page_key(key: &str) -> Result<()> {
    if PAGE_SETTINGS.contains(&key) {
        Ok(())
    } else {
        Err(Error::Invalid(format!("the setting {key} can't be read or changed from the page")))
    }
}

/// The default of a setting the page may read.
pub fn page_default(key: &str) -> Result<String> {
    page_key(key)?;
    Ok(match key {
        MAX_RUNNING_KEY => MAX_RUNNING_DEFAULT.to_string(),
        PROFILES_KEY => serde_json::to_string(&default_profiles())?,
        IDLE_PAUSE_KEY => IDLE_PAUSE_DEFAULT.to_string(),
        crate::STACK_RULE_KEY => "all".to_string(),
        TEST_RULE_KEY => "on".to_string(),
        PRIVATE_DIR_KEY => String::new(),
        _ => unreachable!(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_dir_is_a_full_path_or_none() {
        assert_eq!(check_private_dir("").unwrap(), "");
        assert_eq!(check_private_dir(r" ~\kanban-private\ ").unwrap(), r"~\kanban-private");
        assert_eq!(check_private_dir(r"D:\x").unwrap(), r"D:\x");
        assert!(check_private_dir("relative").is_err());
        assert!(check_private_dir("C:").is_err());
    }

    #[test]
    fn glob() {
        assert!(glob_match("http://127.0.0.1:*/login.html?as=admin*", "http://127.0.0.1:5173/login.html?as=admin&next=/o/x"));
        assert!(!glob_match("http://127.0.0.1:*/login.html?as=admin*", "http://127.0.0.1:5173/login.html?as=user"));
        assert!(glob_match("*EXAMPLE.com*", "https://app.example.com/x"));
        assert!(glob_match("*", ""));
        assert!(!glob_match("a", ""));
    }
}
