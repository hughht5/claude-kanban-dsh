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
    "kanban_search",
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
        { "name": "kanban_search", "description": "Search every card's session history (the last 90 days, archived cards included): prompts, replies and tool calls. Every word of 3 or more letters must match; \"quote\" a phrase. Secrets and sign-in links are stored masked. For Hugh's own sessions only: card sessions don't get it.",
          "inputSchema": { "type": "object", "properties": { "query": { "type": "string" }, "card": { "type": "integer", "description": "Only this card's sessions." }, "limit": { "type": "integer", "description": "How many of the newest matches to take (default and most 2000)." } }, "required": ["query"] } },
    ])
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
                        // The Router is not a card (card #119, final-test F8): it shows as "router".
                        let n = if id == crate::routersession::KEY { json!("router") } else { json!(store.get(id).map(|c| c.number).unwrap_or(0)) };
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
        "kanban_search" => crate::search::tool(app, args)?,
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

    #[test]
    fn control_tools_are_all_listed() {
        let names: Vec<String> = schemas().as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(names, CONTROL.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    }
}
