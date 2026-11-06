//! Claude Code's own config, watched while the app's subagents run (Hugh, card #96). Every
//! subagent keeps Bash, so a deny list can't stop one writing `settings.json` (a tester once
//! set Hugh's default effort to low). When a card session starts one of the app's agents (its
//! `Agent` call's `PreToolUse` hook), the app takes a fingerprint of each account's
//! `settings.json` and `.claude.json`; when one stops (`SubagentStop`), it compares, and logs a
//! loud line on the card and tells Hugh if anything changed. It only reports: it never puts a
//! file back.
//!
//! `settings.json` counts whole. `.claude.json` counts only its config keys (`CONFIG_KEYS`, at
//! the top and in each project), because Claude Code rewrites its counters and caches in that
//! file on every session.

use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The keys of `.claude.json` that configure Claude Code, at the top level and in each project.
const CONFIG_KEYS: [&str; 4] = ["mcpServers", "allowedTools", "enabledMcpjsonServers", "disabledMcpjsonServers"];

/// One file's fingerprint: None when it doesn't exist or can't be read.
type Print = Option<u64>;

/// The files watched, for a home folder: each account's `settings.json` and `.claude.json`.
pub fn watched(home: &Path) -> Vec<PathBuf> {
    let main = home.join(".claude");
    let two = home.join(".claude-max2");
    vec![main.join("settings.json"), home.join(".claude.json"), two.join("settings.json"), two.join(".claude.json")]
}

/// The part of `.claude.json` that counts: its config keys, at the top and per project.
fn config_part(v: &Value) -> Value {
    let pick = |o: &Value| -> Value {
        let mut m = serde_json::Map::new();
        for k in CONFIG_KEYS {
            if let Some(x) = o.get(k) {
                m.insert(k.into(), x.clone());
            }
        }
        Value::Object(m)
    };
    let mut out = serde_json::Map::new();
    out.insert("top".into(), pick(v));
    if let Some(projects) = v.get("projects").and_then(Value::as_object) {
        let mut p = serde_json::Map::new();
        for (dir, proj) in projects {
            let part = pick(proj);
            if part.as_object().is_some_and(|m| !m.is_empty()) {
                p.insert(dir.clone(), part);
            }
        }
        out.insert("projects".into(), Value::Object(p));
    }
    Value::Object(out)
}

/// A file's fingerprint: all of a `settings.json`; the config part of a `.claude.json` (a file
/// that isn't valid JSON counts whole).
pub fn print(path: &Path) -> Print {
    let text = std::fs::read_to_string(path).ok()?;
    let mut h = DefaultHasher::new();
    let is_state = path.file_name().is_some_and(|n| n == ".claude.json");
    match serde_json::from_str::<Value>(&text) {
        Ok(v) if is_state => config_part(&v).to_string().hash(&mut h),
        _ => text.hash(&mut h),
    }
    Some(h.finish())
}

/// The fingerprints of every watched file.
pub fn snapshot(home: &Path) -> Vec<(PathBuf, Print)> {
    watched(home).into_iter().map(|p| {
        let f = print(&p);
        (p, f)
    }).collect()
}

/// The files whose fingerprint differs between `before` and now.
pub fn changed(before: &[(PathBuf, Print)]) -> Vec<PathBuf> {
    before.iter().filter(|(p, f)| print(p) != *f).map(|(p, _)| p.clone()).collect()
}

/// A session's watch while any of the app's agents runs in it.
struct Watch {
    before: Vec<(PathBuf, Print)>,
    /// How many of the app's agents it has started that haven't stopped.
    running: usize,
    since: Instant,
}

/// A watch whose agents never reported a stop (the session was killed) is dropped after this.
const WATCH_CAP: Duration = Duration::from_secs(3 * 60 * 60);

/// Each session's watch, by session row id. It exists only while one of the app's agents runs,
/// so a change made between runs is never blamed on the next one.
static WATCHES: Mutex<Option<HashMap<String, Watch>>> = Mutex::new(None);

fn home() -> Option<PathBuf> {
    kanban_store::home_var()
}

/// A `PreToolUse` hook: an `Agent` (or `Task`) call that starts one of the app's agents. The
/// first one running takes the snapshot; any more only add to the count.
pub fn on_pre_tool(row_id: &str, body: &Value) {
    let tool = body.get("tool_name").and_then(Value::as_str).unwrap_or("");
    let kind = body.pointer("/tool_input/subagent_type").and_then(Value::as_str).unwrap_or("");
    if !matches!(tool, "Agent" | "Task") || crate::agents::find(kind).is_none() {
        return;
    }
    let Some(home) = home() else { return };
    let Ok(mut g) = WATCHES.lock() else { return };
    let map = g.get_or_insert_with(HashMap::new);
    map.retain(|_, w| w.since.elapsed() < WATCH_CAP);
    let w = map.entry(row_id.to_string()).or_insert_with(|| Watch { before: snapshot(&home), running: 0, since: Instant::now() });
    w.running += 1;
}

/// A `SubagentStop` hook for one of the app's agents: what changed since the snapshot. The
/// watch ends with the last agent running; otherwise it starts again from now.
pub fn on_stop(row_id: &str, body: &Value) -> Option<(String, Vec<PathBuf>)> {
    let agent = crate::agents::find(body.get("agent_type").and_then(Value::as_str).unwrap_or(""))?;
    let home = home()?;
    let mut g = WATCHES.lock().ok()?;
    let map = g.get_or_insert_with(HashMap::new);
    let mut w = map.remove(row_id)?;
    let diff = changed(&w.before);
    w.running = w.running.saturating_sub(1);
    if w.running > 0 {
        w.before = snapshot(&home);
        map.insert(row_id.to_string(), w);
    }
    (!diff.is_empty()).then(|| (agent.name.to_string(), diff))
}

/// The loud line for the card's log, with the paths as Hugh knows them (`~\.claude.json`).
pub fn warning(agent: &str, files: &[PathBuf], home: &Path) -> String {
    let names: Vec<String> = files
        .iter()
        .map(|p| p.strip_prefix(home).map(|r| format!("~\\{}", r.display())).unwrap_or_else(|_| p.display().to_string()))
        .collect();
    format!(
        "WARNING: Claude Code's config changed while the {agent} subagent ran on this card: {}. It may have been the subagent, or another session or Hugh at the same time: the files are shared. No subagent may change them (Hugh's rule: never write settings.json or ~\\.claude.json). Check the file and put back anything a subagent changed.",
        names.join(", ")
    )
}

/// After a `SubagentStop`: logs and tells Hugh if one of the app's agents changed the config.
pub fn report(app: &tauri::AppHandle, row_id: &str, body: &Value) {
    use tauri::{Emitter, Manager};
    let Some((agent, files)) = on_stop(row_id, body) else { return };
    let Some(home) = home() else { return };
    let line = warning(&agent, &files, &home);
    let board = app.state::<crate::Board>();
    let Ok(mut store) = board.0.lock() else { return };
    let Some(card) = crate::agents::own_card(app, &store, row_id) else { return };
    let _ = store.add_log(&card.id, &line, "the app");
    let text = format!("A subagent changed Claude Code's config on #{}: see its log", card.number);
    let _ = store.add_log(&card.id, &format!("Told Hugh: {text}"), "the app");
    drop(store);
    let _ = app.emit("notify", serde_json::json!({ "card": card.id, "number": card.number, "text": text }));
    let _ = app.emit("cards-changed", ());
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_change_to_config_is_seen_and_a_counter_isnt() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        let state = home.join(".claude.json");
        let settings = home.join(".claude").join("settings.json");
        std::fs::write(&state, json!({ "numStartups": 1, "mcpServers": { "a": {} }, "projects": { "C:/x": { "allowedTools": [], "lastCost": 1 } } }).to_string()).unwrap();
        std::fs::write(&settings, r#"{ "effortLevel": "high" }"#).unwrap();
        let before = snapshot(home);
        assert!(changed(&before).is_empty());
        // Claude Code's own bookkeeping doesn't count.
        std::fs::write(&state, json!({ "numStartups": 2, "mcpServers": { "a": {} }, "projects": { "C:/x": { "allowedTools": [], "lastCost": 5 } } }).to_string()).unwrap();
        assert!(changed(&before).is_empty());
        // A project's allowed tools do, and so does any change to settings.json.
        std::fs::write(&state, json!({ "numStartups": 2, "mcpServers": { "a": {} }, "projects": { "C:/x": { "allowedTools": ["Bash"] } } }).to_string()).unwrap();
        std::fs::write(&settings, r#"{ "effortLevel": "low" }"#).unwrap();
        let diff = changed(&before);
        assert_eq!(diff, vec![settings.clone(), state.clone()]);
        let w = warning("card-tester", &diff, home);
        assert!(w.starts_with("WARNING:") && w.contains("card-tester") && w.contains("~\\.claude\\settings.json") && w.contains("~\\.claude.json"), "{w}");
        // A file made or removed counts too (account2's, here).
        std::fs::create_dir_all(home.join(".claude-max2")).unwrap();
        std::fs::write(home.join(".claude-max2").join("settings.json"), "{}").unwrap();
        assert!(changed(&before).contains(&home.join(".claude-max2").join("settings.json")));
    }

    #[test]
    fn a_watch_lasts_only_while_the_apps_agents_run() {
        let pre = |kind: &str| json!({ "tool_name": "Agent", "tool_input": { "subagent_type": kind } });
        let watching = |row: &str| WATCHES.lock().unwrap().as_ref().and_then(|m| m.get(row).map(|w| w.running));
        on_pre_tool("row-cw-1", &pre("general-purpose"));
        assert_eq!(watching("row-cw-1"), None, "not one of the app's agents");
        on_pre_tool("row-cw-1", &pre("card-reviewer"));
        on_pre_tool("row-cw-1", &pre("card-tester"));
        assert_eq!(watching("row-cw-1"), Some(2));
        // Another agent's stop isn't one of ours: nothing is compared or counted.
        assert!(on_stop("row-cw-1", &json!({ "agent_type": "Explore" })).is_none());
        assert_eq!(watching("row-cw-1"), Some(2));
        on_stop("row-cw-1", &json!({ "agent_type": "card-reviewer" }));
        assert_eq!(watching("row-cw-1"), Some(1), "the tester still runs");
        on_stop("row-cw-1", &json!({ "agent_type": "card-tester" }));
        assert_eq!(watching("row-cw-1"), None, "nothing runs, so nothing is watched until the next start");
        assert!(on_stop("row-cw-1", &json!({ "agent_type": "card-tester" })).is_none());
    }
}
