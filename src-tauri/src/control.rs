//! Session control tools: start, message, stop, switch, read and steer card sessions from
//! outside the app. Offered only to callers outside a card session: the external MCP route
//! (`kb mcp` or `/mcp/<token>/external`) and `kb` run outside a session. A card's own session
//! never gets them, so one session can't start or drive another.

use crate::{sessions::Sessions, Board};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

pub const CONTROL: &[&str] = &[
    "kanban_session_start",
    "kanban_session_send",
    "kanban_session_stop",
    "kanban_session_status",
    "kanban_session_read",
    "kanban_session_switch",
    "kanban_session_keys",
    // Card #209: No to a permission prompt, or an interrupt, then a message at once.
    "kanban_session_deny",
    "kanban_session_interrupt",
    "kanban_search",
    // Card #195: the board's flow defaults (Settings > Sessions), for Hugh's own agents.
    "kanban_flow_defaults",
];

pub fn schemas() -> Value {
    let card = json!({ "type": "integer", "description": "The card's number." });
    json!([
        { "name": "kanban_session_start", "description": "Start the card's Claude Code session in the app (or resume its last one), optionally with a first message. Over the running-sessions cap it waits in the queue and starts when a place frees; the message waits with it. Hugh's cap is never changed.",
          "inputSchema": { "type": "object", "properties": { "card": card, "mode": { "type": "string", "enum": ["new", "resume"], "description": "Default: resume if the card has a conversation in the app, else new." }, "message": { "type": "string" } }, "required": ["card"] } },
        { "name": "kanban_session_send", "description": "Send a message to the card's session, as Hugh would from the panel. It is typed when the session is next free.",
          "inputSchema": { "type": "object", "properties": { "card": card, "text": { "type": "string" } }, "required": ["card", "text"] } },
        { "name": "kanban_session_stop", "description": "Stop the card's running session (it can be resumed later), or take a queued start out of the queue.",
          "inputSchema": { "type": "object", "properties": { "card": card }, "required": ["card"] } },
        { "name": "kanban_session_status", "description": "Session state: for one card (state, model, effort, queue place, what needs Hugh, the latest status line), or for every card with a live or queued session.",
          "inputSchema": { "type": "object", "properties": { "card": card } } },
        { "name": "kanban_session_read", "description": "The card's conversation: the last N messages (user and assistant text; tools as one line each).",
          "inputSchema": { "type": "object", "properties": { "card": card, "last": { "type": "integer", "description": "How many entries (default 10, at most 100)." }, "include_tools": { "type": "boolean" } }, "required": ["card"] } },
        { "name": "kanban_session_switch", "description": "Change the model and/or effort of the card's session. A running session gets /model or /effort typed when it is next free; later starts use it too.",
          "inputSchema": { "type": "object", "properties": { "card": card, "model": { "type": "string" }, "effort": { "type": "string", "enum": ["low", "medium", "high", "xhigh", "max"] } }, "required": ["card"] } },
        { "name": "kanban_session_keys", "description": "Press keys in the card's terminal, to answer a prompt the session is stuck on (a trust or permission question). Keys: enter, up, down, left, right, esc, tab, space, ctrl-c, or {\"text\": \"...\"} to type text.",
          "inputSchema": { "type": "object", "properties": { "card": card, "keys": { "type": "array", "items": {} } }, "required": ["card", "keys"] } },
        { "name": "kanban_session_deny", "description": DENY_DESCRIPTION,
          "inputSchema": { "type": "object", "properties": { "card": card, "reason": { "type": "string", "description": "Typed into the session at once, after the No: why, and what to do instead (for example: redo it a safer way)." } }, "required": ["card", "reason"] } },
        { "name": "kanban_session_interrupt", "description": INTERRUPT_DESCRIPTION,
          "inputSchema": { "type": "object", "properties": { "card": card, "message": { "type": "string", "description": "Typed into the session at once, after the interrupt." } }, "required": ["card"] } },
        { "name": "kanban_search", "description": "Search every card's session history (the last 90 days, archived cards included): prompts, replies and tool calls. Every word of 3 or more letters must match; \"quote\" a phrase. Secrets and sign-in links are stored masked. For Hugh's own sessions only: card sessions don't get it.",
          "inputSchema": { "type": "object", "properties": { "query": { "type": "string" }, "card": { "type": "integer", "description": "Only this card's sessions." }, "limit": { "type": "integer", "description": "How many of the newest matches to take (default and most 2000)." } }, "required": ["query"] } },
        { "name": "kanban_flow_defaults", "description": "The board's flow defaults (Settings > Sessions), for Hugh's own sessions only, when he asked: whether a card skips Planning, Plan review (card #195: a finished plan waits there for Hugh to approve) and Human test, unless the card sets its own (kanban_update_card's skipPlanning, skipPlanReview, skipHumanTest). With no `set` it reads them. `set` changes some, as Hugh's Settings toggle would: true is on (skip it), false is off. Card sessions and the Router don't get it.",
          "inputSchema": { "type": "object", "properties": { "set": { "type": "object", "properties": { "skipPlanning": { "type": "boolean" }, "skipPlanReview": { "type": "boolean" }, "skipHumanTest": { "type": "boolean" } }, "additionalProperties": false } } } },
    ])
}

pub const DENY_DESCRIPTION: &str = "Answer No to the card session's open permission prompt (a tool it asks to run), as Esc does in its terminal, then type `reason` into it at once. It never answers Yes. Refused unless the session is at a permission prompt; a question dialog is Hugh's. Logged on the card.";
pub const INTERRUPT_DESCRIPTION: &str = "Interrupt the card session's running turn, as Esc does in its terminal, then type `message` into it at once (if given). Refused unless it is on a turn; at a permission prompt use kanban_session_deny. Logged on the card.";

/// Who `halt` logs on the card for an outside caller (`kb mcp`).
pub const OUTSIDE_BY: &str = "Hugh's session outside the app";

/// Card #209: `kanban_session_deny` and `kanban_session_interrupt`. One Esc
/// (`Sessions::halt`), a line on the card, then the message, sent as `kanban_session_send`
/// sends one; the session is idle by then, so it is typed at once. `by` is who it is logged as;
/// `as_app`: the words are recorded as the app's (the Router's), so the chat draws them apart.
pub fn halt(app: &AppHandle, args: &Value, kind: crate::sessions::Halt, by: &str, as_app: bool) -> Result<Value, String> {
    use crate::sessions::Halt;
    let (id, n) = card_id(app, args)?;
    let text = match kind {
        Halt::Deny => Some(args.get("reason").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).ok_or("pass `reason`: why, and what to do instead; it is typed into the session")?),
        Halt::Interrupt => args.get("message").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()),
    };
    let live = app.state::<Arc<Sessions>>();
    let asked = live.waits().get(&id).and_then(|w| w.prompt.clone());
    let row = live.halt(&id, kind).map_err(|e| format!("refused: card #{n}: {e}"))?;
    if let Some(r) = &row {
        if let Ok(mut store) = app.state::<Board>().0.lock() {
            let _ = store.session_state(r, "idle");
        }
        let _ = tauri::Emitter::emit(app, "activity", json!({ "card": id.clone(), "state": "idle" }));
        let _ = tauri::Emitter::emit(app, "needs-changed", id.clone());
    }
    let line = halt_log_line(kind, asked.as_deref(), text);
    if let Ok(mut store) = app.state::<Board>().0.lock() {
        let _ = store.add_log(&id, &kanban_store::mask_secrets(&line), by);
        if let (true, Some(t)) = (as_app, text) {
            let _ = store.note_app_message(&id, t);
        }
    }
    if let Some(t) = text {
        crate::session_send(app.clone(), app.state::<Board>(), id.clone(), t.to_string(), None).map_err(err)?;
    }
    let _ = tauri::Emitter::emit(app, "session-changed", id);
    let did = if kind == Halt::Deny { "answered No to its permission prompt" } else { "interrupted its turn" };
    Ok(json!(match (row.is_some(), text.is_some()) {
        (true, true) => format!("Card #{n}: {did}; the message is typed now."),
        (true, false) => format!("Card #{n}: {did}; it is idle at its prompt."),
        (false, true) => format!("Card #{n}: pressed Esc, but its prompt hasn't come back yet; the message waits and is typed once its turn is seen to end. Check with kanban_session_status."),
        (false, false) => format!("Card #{n}: pressed Esc, but its prompt hasn't come back yet. Check with kanban_session_status."),
    }))
}

/// The card's log line for a `halt`: what was turned down, and the message that follows.
pub fn halt_log_line(kind: crate::sessions::Halt, asked: Option<&str>, text: Option<&str>) -> String {
    let cut = |t: &str| crate::transcript::cut_words(t, 200);
    let head = match (kind, asked) {
        (crate::sessions::Halt::Deny, Some(a)) => format!("Answered No to the session's permission prompt ({})", cut(a)),
        (crate::sessions::Halt::Deny, None) => "Answered No to the session's permission prompt".to_string(),
        (crate::sessions::Halt::Interrupt, _) => "Interrupted the session's turn".to_string(),
    };
    match text {
        Some(t) => format!("{head}, then told it: {}", cut(t)),
        None => head,
    }
}

fn err(e: impl serde::Serialize) -> String {
    match serde_json::to_value(&e) {
        Ok(Value::Object(m)) => m.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| Value::Object(m).to_string()),
        Ok(v) => v.to_string(),
        Err(_) => "failed".into(),
    }
}

fn card_id(app: &AppHandle, args: &Value) -> Result<(String, i64), String> {
    let n = args.get("card").and_then(Value::as_i64).ok_or("say which card: pass its number as `card`")?;
    let board = app.state::<Board>();
    let store = board.0.lock().map_err(|e| e.to_string())?;
    let c = store.get_by_number(n).map_err(|e| e.to_string())?;
    if c.archived {
        return Err(format!("card #{n} is archived"));
    }
    Ok((c.id, n))
}

/// The bytes for one named key.
pub fn key_bytes(k: &Value) -> Result<Vec<u8>, String> {
    if let Some(t) = k.get("text").and_then(Value::as_str) {
        return Ok(t.as_bytes().to_vec());
    }
    let name = k.as_str().ok_or("each key is a name or {\"text\": ...}")?;
    Ok(match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => b"\r".to_vec(),
        "up" => b"\x1b[A".to_vec(),
        "down" => b"\x1b[B".to_vec(),
        "right" => b"\x1b[C".to_vec(),
        "left" => b"\x1b[D".to_vec(),
        "esc" | "escape" => b"\x1b".to_vec(),
        "tab" => b"\t".to_vec(),
        "space" => b" ".to_vec(),
        "ctrl-c" => b"\x03".to_vec(),
        other => return Err(format!("unknown key \"{other}\"")),
    })
}

fn status_one(app: &AppHandle, id: &str) -> Result<Value, String> {
    let info = crate::session_info(app.state::<Board>(), app.state::<Arc<Sessions>>(), id.to_string()).map_err(err)?;
    let line = crate::session_status(app.state::<Board>(), app.state::<Arc<Sessions>>(), id.to_string()).ok();
    let needs = crate::needs::needs_you(app.clone()).get(id).cloned();
    Ok(json!({ "session": info, "status": line.and_then(|s| s.get("status").cloned()), "needs": needs }))
}

pub fn call(app: &AppHandle, name: &str, args: &Value) -> Result<Value, String> {
    Ok(match name {
        "kanban_session_start" => {
            let (id, n) = card_id(app, args)?;
            let mode = match args.get("mode").and_then(Value::as_str) {
                Some(m) => m.to_string(),
                None => {
                    let board = app.state::<Board>();
                    let store = board.0.lock().map_err(|e| e.to_string())?;
                    if store.latest_session(&id).ok().flatten().is_some() { "resume".into() } else { "new".into() }
                }
            };
            let r = crate::session_start(app.clone(), app.state::<Board>(), id.clone(), mode.clone()).map_err(err)?;
            if let Some(m) = args.get("message").and_then(Value::as_str).filter(|m| !m.trim().is_empty()) {
                crate::session_send(app.clone(), app.state::<Board>(), id.clone(), m.to_string(), None).map_err(err)?;
            }
            match r.get("queued") {
                Some(q) => json!(format!("Card #{n}: queued ({mode}), number {q} in line; it starts when a place frees.")),
                None => json!(format!("Card #{n}: session started ({mode}, {} {}).", r["model"].as_str().unwrap_or(""), r["effort"].as_str().unwrap_or(""))),
            }
        }
        "kanban_session_send" => {
            let (id, n) = card_id(app, args)?;
            let text = args.get("text").and_then(Value::as_str).ok_or("pass the message as `text`")?;
            crate::session_send(app.clone(), app.state::<Board>(), id, text.to_string(), None).map_err(err)?;
            json!(format!("Queued for card #{n}'s session: it is typed when the session is next free."))
        }
        "kanban_session_stop" => {
            let (id, n) = card_id(app, args)?;
            let live = app.state::<Arc<Sessions>>().inner().clone();
            if live.is_live(&id).is_some() {
                live.stop(app, &id)?;
                json!(format!("Stopped card #{n}'s session."))
            } else if crate::session_unqueue(app.clone(), app.state::<Board>(), id).map_err(err)? {
                json!(format!("Took card #{n} out of the start queue."))
            } else {
                json!(format!("Card #{n} has no running or queued session."))
            }
        }
        "kanban_session_status" => {
            if args.get("card").is_some() {
                let (id, _) = card_id(app, args)?;
                status_one(app, &id)?
            } else {
                let states = crate::session_states(app.state::<Arc<Sessions>>());
                let queue = crate::session_queue(app.state::<Board>(), app.state::<Arc<Sessions>>()).map_err(err)?;
                let needs = crate::needs::needs_you(app.clone());
                let board = app.state::<Board>();
                let store = board.0.lock().map_err(|e| e.to_string())?;
                let mut live = vec![];
                if let Some(m) = states.as_object() {
                    for (id, st) in m {
                        // The Router is not a card (card #119, final-test F8): it shows as "router",
                        // and a merge lane (card #160) as its key, "lane:<repo>".
                        let n = if crate::sessions::is_app_key(id) { json!(id) } else { json!(store.get(id).map(|c| c.number).unwrap_or(0)) };
                        live.push(json!({ "card": n, "state": st, "needs": needs.get(id).and_then(|v| v.get("reason")) }));
                    }
                }
                json!({ "running": live, "queue": queue })
            }
        }
        "kanban_session_read" => {
            let (id, _) = card_id(app, args)?;
            let last = args.get("last").and_then(Value::as_u64).unwrap_or(10).clamp(1, 100) as usize;
            let tools = args.get("include_tools").and_then(Value::as_bool).unwrap_or(false);
            let chunk = crate::transcript_chunk(&app.state::<Board>(), &id, 0, None).map_err(err)?;
            let v = serde_json::to_value(&chunk).map_err(|e| e.to_string())?;
            let entries: Vec<Value> = v["entries"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|e| matches!(e["kind"].as_str(), Some("user" | "assistant")) || (tools && e["kind"] == "tool"))
                .map(|e| {
                    let mut t = e["text"].as_str().unwrap_or("").to_string();
                    if t.len() > 6000 {
                        let mut cut = 6000;
                        while !t.is_char_boundary(cut) {
                            cut -= 1;
                        }
                        t.truncate(cut);
                        t.push_str(" …");
                    }
                    json!({ "kind": e["kind"], "at": e["at"], "text": t.replace('\t', ": ") })
                })
                .collect();
            let start = entries.len().saturating_sub(last);
            Value::Array(entries[start..].to_vec())
        }
        "kanban_session_switch" => {
            let (id, _) = card_id(app, args)?;
            let model = args.get("model").and_then(Value::as_str).map(str::to_string);
            let effort = args.get("effort").and_then(Value::as_str).map(str::to_string);
            let r = crate::set_session_model_effort(app.clone(), app.state::<Board>(), app.state::<Arc<Sessions>>(), id, model, effort).map_err(err)?;
            r.get("message").cloned().unwrap_or(r)
        }
        "kanban_session_keys" => {
            let (id, n) = card_id(app, args)?;
            let keys = args.get("keys").and_then(Value::as_array).ok_or("pass `keys` as a list")?;
            let live = app.state::<Arc<Sessions>>();
            if live.is_live(&id).is_none() {
                return Err(format!("card #{n} has no running session"));
            }
            for k in keys {
                live.input(&id, &key_bytes(k)?)?;
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            json!(format!("Pressed {} key(s) in card #{n}'s terminal.", keys.len()))
        }
        "kanban_session_deny" => halt(app, args, crate::sessions::Halt::Deny, OUTSIDE_BY, false)?,
        "kanban_session_interrupt" => halt(app, args, crate::sessions::Halt::Interrupt, OUTSIDE_BY, false)?,
        "kanban_search" => crate::search::tool(app, args)?,
        "kanban_flow_defaults" => flow_defaults(app, args)?,
        _ => return Err(format!("no tool called {name}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map_to_terminal_bytes() {
        assert_eq!(key_bytes(&json!("down")).unwrap(), b"\x1b[B");
        assert_eq!(key_bytes(&json!("Enter")).unwrap(), b"\r");
        assert_eq!(key_bytes(&json!({ "text": "yes" })).unwrap(), b"yes");
        assert!(key_bytes(&json!("f13")).is_err());
    }

    /// Card #209: what goes on the card's log for a No and for an interrupt.
    #[test]
    fn a_halt_is_logged_with_what_it_turned_down_and_what_followed() {
        use crate::sessions::Halt;
        assert_eq!(halt_log_line(Halt::Deny, Some("Bash: rm -rf target"), Some("redo it a safer way")), "Answered No to the session's permission prompt (Bash: rm -rf target), then told it: redo it a safer way");
        assert_eq!(halt_log_line(Halt::Deny, None, Some("no")), "Answered No to the session's permission prompt, then told it: no");
        assert_eq!(halt_log_line(Halt::Interrupt, Some("ignored"), None), "Interrupted the session's turn");
        assert!(halt_log_line(Halt::Interrupt, None, Some(&"word ".repeat(200))).len() < 300);
    }

    /// Card #209: deny needs its reason; both are offered with their schemas, never a Yes.
    #[test]
    fn deny_and_interrupt_are_offered_and_never_say_yes() {
        let s = schemas();
        let t = |n: &str| s.as_array().unwrap().iter().find(|t| t["name"] == n).cloned().unwrap();
        assert_eq!(t("kanban_session_deny")["inputSchema"]["required"], json!(["card", "reason"]));
        assert_eq!(t("kanban_session_interrupt")["inputSchema"]["required"], json!(["card"]));
        assert!(DENY_DESCRIPTION.contains("never answers Yes"));
        // The one way in is a single Esc: the code presses nothing else.
        let src = include_str!("sessions.rs");
        let body = src.split("pub fn halt(&self").nth(1).unwrap().split("Ok(Some(row_id))").next().unwrap();
        assert_eq!(body.matches("self.input(").count(), 1, "{body}");
        assert!(body.contains(r#"self.input(card_id, b"\x1b")"#), "{body}");
    }

    #[test]
    fn control_tools_are_all_listed() {
        let names: Vec<String> = schemas().as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(names, CONTROL.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    }
}

/// The by of a flow default changed through `kanban_flow_defaults` (card #195).
pub const FLOW_DEFAULTS_BY: &str = "Hugh (his session outside the app)";

/// `kanban_flow_defaults` (card #195): reads the three skip defaults, after setting those in
/// `set` (each on or off), logged as Hugh's, with the same follow-up as his Settings toggle.
fn flow_defaults(app: &AppHandle, args: &Value) -> Result<Value, String> {
    let want = flow_defaults_wanted(args)?;
    let mut changed = Vec::new();
    {
        let board = app.state::<Board>();
        let mut store = board.0.lock().map_err(|e| e.to_string())?;
        for (rule, on) in &want {
            let (_, now) = store.set_page_setting_logged(rule, if *on { "on" } else { "off" }, FLOW_DEFAULTS_BY).map_err(|e| e.to_string())?;
            if now {
                changed.push(*rule);
            }
        }
    }
    for rule in &changed {
        let _ = tauri::Emitter::emit(app, "settings-changed", rule.to_string());
        crate::dispatch::after_default_change(app, rule);
    }
    let board = app.state::<Board>();
    let store = board.0.lock().map_err(|e| e.to_string())?;
    let mut out = serde_json::Map::new();
    for (key, rule) in kanban_store::SKIPS {
        out.insert(key.into(), json!(store.skip_default(rule)));
    }
    out.insert("changed".into(), json!(changed.iter().map(|r| kanban_store::SKIPS.iter().find(|(_, x)| x == r).map(|(k, _)| *k).unwrap_or(r)).collect::<Vec<_>>()));
    Ok(Value::Object(out))
}

/// The defaults `kanban_flow_defaults`'s `set` asks for, as (setting, on), checked first so a bad
/// key changes nothing.
pub fn flow_defaults_wanted(args: &Value) -> Result<Vec<(&'static str, bool)>, String> {
    let Some(set) = args.get("set").filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let set = set.as_object().ok_or("`set` is an object, like {\"skipPlanReview\": true}")?;
    let mut out = Vec::new();
    for (k, v) in set {
        let rule = kanban_store::skip_rule(k).ok_or_else(|| format!("`set` takes skipPlanning, skipPlanReview and skipHumanTest, not {k}"))?;
        let on = v.as_bool().ok_or_else(|| format!("{k} is true (skip it) or false"))?;
        out.push((rule, on));
    }
    Ok(out)
}

#[cfg(test)]
mod flow_default_tests {
    use super::*;

    #[test]
    fn set_takes_the_three_skips_as_booleans_and_nothing_else() {
        assert!(flow_defaults_wanted(&json!({})).unwrap().is_empty());
        assert_eq!(
            flow_defaults_wanted(&json!({ "set": { "skipPlanReview": true, "skipHumanTest": false } })).unwrap(),
            vec![(kanban_store::SKIP_HUMAN_TEST_RULE_KEY, false), (kanban_store::SKIP_PLAN_REVIEW_RULE_KEY, true)]
        );
        assert!(flow_defaults_wanted(&json!({ "set": { "skipPlanReview": "on" } })).is_err());
        assert!(flow_defaults_wanted(&json!({ "set": { "critic": false } })).is_err());
        assert!(flow_defaults_wanted(&json!({ "set": [1] })).is_err());
        assert!(CONTROL.contains(&"kanban_flow_defaults"));
        assert!(schemas().as_array().unwrap().iter().any(|t| t["name"] == "kanban_flow_defaults"));
    }
}
