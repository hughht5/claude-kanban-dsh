//! A merge lane's tools (card #160): it reads cards and stacks, logs on the card it merges now,
//! and reports that merge. Nothing else: it moves no card, changes no field, starts nothing and
//! talks to no other session.

use crate::Board;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

/// The card tools a lane is offered (of `tools::schemas`).
pub const CARD_TOOLS: [&str; 3] = ["kanban_get_card", "kanban_stack_list", "kanban_add_log"];

/// What a lane is told when its MCP connection starts.
pub const INSTRUCTIONS: &str = "You are a merge lane of Hugh's board, the Claude Kanban DSH app (card #160). Your rules are in your system prompt. \
Read a card with kanban_get_card (pass its number) and a stack with kanban_stack_list. Log your steps on your job's card with kanban_add_log. \
End each job with one kanban_merge_result call. Never push, never move or change a card.";

/// The lane's own tool.
pub fn result_schema() -> Value {
    let sha = |what: &str| json!({ "type": "string", "description": what });
    json!({
        "name": "kanban_merge_result",
        "description": "Report your merge job's result, once, at its end. The app records it on the card (its merge chip and log) and sends the card's session the verdict and your lines. A PASS needs head (a commit on top of onto) and onto; the app checks it.",
        "inputSchema": { "type": "object", "properties": {
            "card": { "type": "integer", "description": "Your job's card number." },
            "verdict": { "type": "string", "enum": ["PASS", "FAIL"] },
            "head": sha("The SHA of the merged commit in your worktree (git rev-parse HEAD)."),
            "onto": sha("The SHA of origin/main you rebased onto (step 4)."),
            "originBefore": sha("origin/<the card's branch> at step 4, or \"none\"."),
            "originAfter": sha("origin/<the card's branch> at step 10, or \"none\"."),
            "lines": { "type": "array", "items": { "type": "string" }, "description": "One line for each thing you changed (a rebase, each conflict and how you resolved it, a version change, each check and its result); on a FAIL, each thing that failed." }
        }, "required": ["card", "verdict", "lines"] }
    })
}

/// What a lane is offered over MCP.
pub fn tool_list() -> Value {
    let mut out: Vec<Value> = crate::tools::schemas()
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|t| CARD_TOOLS.contains(&t["name"].as_str().unwrap_or("")))
        .map(|mut t| {
            let name = t["name"].as_str().unwrap_or("").to_string();
            let card = json!({ "type": "integer", "description": "The card's number." });
            match name.as_str() {
                "kanban_get_card" => {
                    t["inputSchema"]["properties"]["card"] = card;
                    t["inputSchema"]["required"] = json!(["card"]);
                }
                "kanban_add_log" => {
                    t["description"] = json!("Add a line to your merge job's card's log (only that card), so Hugh sees each step.");
                    t["inputSchema"]["properties"]["card"] = card;
                    t["inputSchema"]["required"] = json!(["card", "text"]);
                }
                _ => {
                    t["inputSchema"]["properties"]["stack"]["description"] = json!("The lead card's number.");
                    t["inputSchema"]["required"] = json!(["stack"]);
                }
            }
            t
        })
        .collect();
    out.push(result_schema());
    Value::Array(out)
}

/// Why lane `key`'s call `name` is refused before it runs, if it is: a tool it isn't offered, a
/// read with no card named, or a log line on a card other than its job's (`job`).
pub fn refused(name: &str, args: &Value, job: Option<i64>) -> Option<String> {
    match name {
        "kanban_get_card" if args.get("card").and_then(Value::as_i64).is_none() => Some("pass the card's number as `card` (a lane has no card of its own)".into()),
        "kanban_stack_list" if args.get("stack").and_then(Value::as_i64).is_none() => Some("pass the lead card's number as `stack`".into()),
        "kanban_add_log" => {
            let n = args.get("card").and_then(Value::as_i64);
            match (n, job) {
                (None, _) => Some("pass your job's card number as `card`".into()),
                (_, None) => Some("you have no merge job now, so no card to log on".into()),
                (Some(n), Some(j)) if n != j => Some(format!("you log only on your job's card (#{j}), not #{n}")),
                _ => None,
            }
        }
        "kanban_get_card" | "kanban_stack_list" | "kanban_merge_result" => None,
        other => Some(format!("a merge lane has no tool {other}: it reads cards, logs on its job's card and reports with kanban_merge_result")),
    }
}

/// A lane's call (`row`, lane `key`).
pub fn call(app: &AppHandle, row: &str, key: &str, name: &str, args: &Value) -> Result<Value, String> {
    let job = if name == "kanban_add_log" {
        let board = app.state::<Board>();
        let store = board.0.lock().map_err(|e| e.to_string())?;
        crate::lane::running_job(&store, key).map(|c| c.number)
    } else {
        None
    };
    if let Some(why) = refused(name, args, job) {
        return Err(why);
    }
    match name {
        "kanban_merge_result" => crate::lane::record_result(app, key, row, args),
        "kanban_add_log" => {
            let text = args.get("text").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).ok_or("pass the line as `text`")?;
            let n = args.get("card").and_then(Value::as_i64).unwrap_or(0);
            let board = app.state::<Board>();
            let mut store = board.0.lock().map_err(|e| e.to_string())?;
            let c = store.get_by_number(n).map_err(|e| e.to_string())?;
            store.add_log(&c.id, &crate::transcript::cut_words(text, 600), crate::lane::LOG_BY).map_err(|e| e.to_string())?;
            drop(store);
            let _ = app.emit("cards-changed", ());
            Ok(json!("Logged."))
        }
        _ => crate::tools::call_as_lane(app, name, args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &Value) -> Vec<String> {
        v.as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn a_lane_is_offered_its_four_tools_only() {
        let n = names(&tool_list());
        assert_eq!(n, vec!["kanban_get_card", "kanban_add_log", "kanban_stack_list", "kanban_merge_result"]);
        for never in ["kanban_move_card", "kanban_update_card", "kanban_ship_result", "kanban_merge", "kanban_router", "kanban_test_app", "kanban_create_card", "kanban_notify"] {
            assert!(!n.iter().any(|x| x == never), "{never}");
        }
        let r = result_schema();
        assert_eq!(r["inputSchema"]["required"], json!(["card", "verdict", "lines"]));
        assert!(INSTRUCTIONS.contains("Never push"));
    }

    #[test]
    fn a_lane_logs_only_on_its_jobs_card_and_names_the_card_it_reads() {
        assert!(refused("kanban_add_log", &json!({ "card": 7, "text": "x" }), Some(7)).is_none());
        assert!(refused("kanban_add_log", &json!({ "card": 8, "text": "x" }), Some(7)).unwrap().contains("only on your job's card (#7)"));
        assert!(refused("kanban_add_log", &json!({ "card": 7, "text": "x" }), None).unwrap().contains("no merge job"));
        assert!(refused("kanban_add_log", &json!({ "text": "x" }), Some(7)).is_some());
        assert!(refused("kanban_get_card", &json!({}), None).is_some());
        assert!(refused("kanban_get_card", &json!({ "card": 3 }), None).is_none());
        assert!(refused("kanban_stack_list", &json!({ "stack": 3 }), None).is_none());
        assert!(refused("kanban_stack_list", &json!({}), None).is_some());
        assert!(refused("kanban_merge_result", &json!({}), None).is_none(), "checked by the lane itself");
        for other in ["kanban_move_card", "kanban_update_card", "kanban_ship_result", "kanban_merge", "kanban_session_send", "kanban_create_card"] {
            assert!(refused(other, &json!({ "card": 7 }), Some(7)).is_some(), "{other}");
        }
    }
}
