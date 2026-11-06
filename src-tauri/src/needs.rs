//! Cards that need Hugh, and each account's 7-day usage for the board's header.
//!
//! A card needs Hugh when its live session waits at a permission prompt, when its session
//! failed (a StopFailure, or it exited without the app stopping it) since he last opened the
//! card, or when its session ended a turn he hasn't opened the card since. Read and reply
//! marks are kept in the store (migration 5); opening a card's panel records a read.

use crate::{sessions::Sessions, Board};
use kanban_store::Store;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

/// Why a card needs Hugh, most pressing first: `permission`, `failed`, `unread`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Need {
    pub reason: &'static str,
    /// For `failed`: "crashed" (it exited on its own) or "turn failed" (StopFailure).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<&'static str>,
    /// When it started to need him, for the order among pinned cards (newest first).
    pub at: String,
    /// Also has a reply he hasn't read (shown in the tooltip when the reason is another).
    pub unread: bool,
}

/// Every card that needs Hugh, from the live sessions' states (`Sessions::states`) and the store.
pub fn compute(store: &Store, states: &HashMap<String, String>) -> kanban_store::Result<BTreeMap<String, Need>> {
    let marks = store.card_marks()?;
    let latest: HashMap<String, kanban_store::LatestSession> =
        store.latest_sessions()?.into_iter().map(|l| (l.card_id.clone(), l)).collect();
    let mut out = BTreeMap::new();
    for card in store.list(false)? {
        let m = marks.get(&card.id).cloned().unwrap_or_default();
        let unread = m.unread();
        let s = latest.get(&card.id);
        let live = states.get(&card.id).map(String::as_str);
        // When the recorded state became `want`, if the store's latest state is that one.
        let state_at = |want: &str| {
            s.filter(|l| l.state.as_deref() == Some(want)).and_then(|l| l.state_at.clone())
        };
        let need = if live == Some("permission") {
            Some(Need { reason: "permission", detail: None, at: state_at("permission").unwrap_or_else(now), unread })
        } else if live == Some("failed") && state_at("failed").map_or(true, |at| m.after_read(&at)) {
            Some(Need { reason: "failed", detail: Some("turn failed"), at: state_at("failed").unwrap_or_else(now), unread })
        } else if let Some(at) = s
            .filter(|l| live.is_none() && l.status.as_deref() == Some("exited"))
            .and_then(|l| l.status_at.clone())
            .filter(|at| m.after_read(at))
        {
            Some(Need { reason: "failed", detail: Some("crashed"), at, unread })
        } else if unread {
            Some(Need { reason: "unread", detail: None, at: m.reply.clone().unwrap_or_default(), unread })
        } else {
            None
        };
        if let Some(n) = need {
            out.insert(card.id.clone(), n);
        }
    }
    Ok(out)
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// The cards that need Hugh now. Takes the sessions' lock and the board's in turn, never both.
pub fn current(app: &AppHandle) -> BTreeMap<String, Need> {
    let states = app.state::<Arc<Sessions>>().states();
    let board = app.state::<Board>();
    let Ok(store) = board.0.lock() else { return BTreeMap::new() };
    compute(&store, &states).unwrap_or_default()
}

/// Records that Hugh has seen the card's conversation up to now.
pub fn mark_read(app: &AppHandle, card_id: &str) {
    {
        let board = app.state::<Board>();
        let Ok(mut store) = board.0.lock() else { return };
        if store.get(card_id).is_err() {
            return;
        }
        let _ = store.mark_card(card_id, "read");
    }
    let _ = app.emit("needs-changed", card_id.to_string());
}

/// A card's session ended a turn (its Stop hook). Unread, unless Hugh is looking at the card's
/// panel in a focused window. Call it holding no lock: the focus check waits on the main thread.
pub fn turn_ended(app: &AppHandle, card_id: &str) {
    let looking = crate::notify::looking_at(app, card_id);
    {
        let board = app.state::<Board>();
        let Ok(mut store) = board.0.lock() else { return };
        let _ = store.mark_card(card_id, "reply");
        if looking {
            let _ = store.mark_card(card_id, "read");
        }
    }
    let _ = app.emit("needs-changed", card_id.to_string());
}

/// Each card that needs Hugh: `{ cardId: { reason, detail?, at, unread } }`.
#[tauri::command]
pub fn needs_you(app: AppHandle) -> Value {
    json!(current(&app))
}

/// Marks the card read (the page calls it when the window regains focus with a panel open;
/// opening a panel records a read through `panel_opened`).
#[tauri::command]
pub fn mark_card_read(app: AppHandle, card_id: String) {
    mark_read(&app, &card_id);
}

/// Each account's newest 7-day usage, as its sessions' status lines last reported it:
/// `{ main: { sevenDay, at }, account2: { sevenDay, at } }`, with nulls where none came yet.
pub fn usage(store: &Store) -> Value {
    let one = |a: &str| {
        let v: Value = store
            .setting(&format!("usage.{a}"))
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        json!({ "sevenDay": v.get("seven_day").and_then(Value::as_f64), "at": v.get("at").and_then(Value::as_str) })
    };
    json!({ "main": one("main"), "account2": one("account2") })
}

#[tauri::command]
pub fn usage_summary(board: State<Board>) -> Result<Value, String> {
    let store = board.0.lock().map_err(|e| e.to_string())?;
    Ok(usage(&store))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanban_store::SessionRow;

    fn board() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("b.db")).unwrap();
        (dir, s)
    }

    fn card(s: &mut Store, id: &str) {
        s.create(Some(id), json!({ "title": id }).as_object().cloned().unwrap(), "t").unwrap();
    }

    fn session(s: &mut Store, id: &str, card: &str) {
        s.add_session(&SessionRow {
            id: id.into(),
            card_id: card.into(),
            claude_session_id: format!("c-{id}"),
            account: "main".into(),
            cwd: ".".into(),
            model: None,
            effort: None,
            started_at: now(),
            status: None,
        })
        .unwrap();
    }

    fn tick() {
        std::thread::sleep(std::time::Duration::from_millis(3));
    }

    fn live(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn permission_failed_and_unread_need_hugh() {
        let (_d, mut s) = board();
        for id in ["p", "f", "c", "u", "quiet"] {
            card(&mut s, id);
        }
        session(&mut s, "sp", "p");
        s.session_state("sp", "permission").unwrap();
        session(&mut s, "sf", "f");
        s.session_state("sf", "failed").unwrap();
        session(&mut s, "sc", "c");
        s.session_event("sc", "exited", None).unwrap();
        s.mark_card("u", "reply").unwrap();
        session(&mut s, "sq", "quiet");
        s.session_state("sq", "idle").unwrap();
        let states = live(&[("p", "permission"), ("f", "failed"), ("quiet", "idle")]);
        let n = compute(&s, &states).unwrap();
        assert_eq!(n.keys().map(String::as_str).collect::<Vec<_>>(), ["c", "f", "p", "u"]);
        assert_eq!(n["p"].reason, "permission");
        assert_eq!((n["f"].reason, n["f"].detail), ("failed", Some("turn failed")));
        assert_eq!((n["c"].reason, n["c"].detail), ("failed", Some("crashed")));
        assert_eq!(n["u"].reason, "unread");
        assert!(!n["p"].at.is_empty() && !n["u"].at.is_empty());
    }

    #[test]
    fn opening_the_card_clears_unread_and_failures_but_not_a_prompt() {
        let (_d, mut s) = board();
        for id in ["p", "f", "c", "u"] {
            card(&mut s, id);
        }
        session(&mut s, "sp", "p");
        s.session_state("sp", "permission").unwrap();
        s.mark_card("p", "reply").unwrap();
        session(&mut s, "sf", "f");
        s.session_state("sf", "failed").unwrap();
        session(&mut s, "sc", "c");
        s.session_event("sc", "exited", None).unwrap();
        s.mark_card("u", "reply").unwrap();
        tick();
        for id in ["p", "f", "c", "u"] {
            s.mark_card(id, "read").unwrap();
        }
        let states = live(&[("p", "permission"), ("f", "failed")]);
        let n = compute(&s, &states).unwrap();
        assert_eq!(n.keys().collect::<Vec<_>>(), ["p"]);
        assert!(!n["p"].unread);
        // A new turn's end after the read is unread again.
        tick();
        s.mark_card("u", "reply").unwrap();
        assert_eq!(compute(&s, &states).unwrap()["u"].reason, "unread");
    }

    #[test]
    fn a_prompt_outranks_an_unread_reply_and_archived_cards_never_need_hugh() {
        let (_d, mut s) = board();
        card(&mut s, "p");
        card(&mut s, "gone");
        s.mark_card("p", "reply").unwrap();
        s.mark_card("gone", "reply").unwrap();
        let v = s.get("gone").unwrap().version;
        s.archive("gone", v, true, "t").unwrap();
        let n = compute(&s, &live(&[("p", "permission")])).unwrap();
        assert_eq!(n.len(), 1);
        assert_eq!(n["p"].reason, "permission");
        assert!(n["p"].unread);
    }

    #[test]
    fn usage_reads_the_seven_day_figures() {
        let (_d, mut s) = board();
        assert_eq!(usage(&s), json!({ "main": { "sevenDay": null, "at": null }, "account2": { "sevenDay": null, "at": null } }));
        s.set_setting("usage.main", r#"{"seven_day": 42.5, "five_hour": 90, "at": "2026-11-04T10:00:00Z"}"#).unwrap();
        s.set_setting("usage.account2", r#"{"seven_day": 17}"#).unwrap();
        let u = usage(&s);
        assert_eq!(u["main"]["sevenDay"], json!(42.5));
        assert_eq!(u["main"]["at"], json!("2026-11-04T10:00:00Z"));
        assert_eq!(u["account2"]["sevenDay"], json!(17.0));
        assert!(u["main"].get("fiveHour").is_none());
    }
}
