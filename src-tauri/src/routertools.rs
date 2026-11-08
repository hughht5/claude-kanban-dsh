//! The Router's tools (card #119): the card tools, held to the Router's own rules, plus its
//! own (`kanban_start_card`, `kanban_router_log`) and three session tools. Every rule is checked
//! here, then the call goes to `tools::call_as_router`, so the card tools' own checks (the
//! subagent gates, the waits, never Ship it) still apply.
//!
//! - Writes and moves need `if_version` (finding 16): an event can be some minutes old.
//! - Moves: `routersession::move_refused` (between Planning, In progress, Human test and Blocked;
//!   Backlog to Planning only with "Router may start cards"; never Ship it).
//! - Hands off a card Hugh has open or changed in the last 2 minutes (decision 4).
//! - Anything that starts a session needs "Router may start cards".
//! - Never a ship: `kanban_set_passed`, `kanban_ship_result` and `kanban_request_ship` are
//!   refused; a test-channel Router never installs.

use crate::routersession;
use crate::Board;
use kanban_store::{Card, Store, ROUTER_BY};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

/// The card tools the Router is not offered.
pub const NOT_FOR_ROUTER: [&str; 3] = ["kanban_set_passed", "kanban_ship_result", "kanban_router"];
/// The session tools it is offered (of `control::CONTROL`).
pub const SESSION_TOOLS: [&str; 3] = ["kanban_session_send", "kanban_session_status", "kanban_session_read"];

/// The Router's own tools.
pub fn own_schemas() -> Value {
    let card = json!({ "type": "integer", "description": "The card's number." });
    json!([
        { "name": "kanban_start_card", "description": "Start (or resume) a card's session, as Hugh's start does: in its worktree (worktreePath) when it has one, else in its repo's folder. Only for a card in Planning or In progress, and only while Hugh's \"Router may start cards\" setting is on; refused otherwise. The running-sessions cap and queue still apply.",
          "inputSchema": { "type": "object", "properties": { "card": card, "if_version": { "type": "integer" }, "message": { "type": "string", "description": "A first message for the session." } }, "required": ["card", "if_version"] } },
        { "name": "kanban_archive_card", "description": "Ask Hugh to archive a card whose work is live (merged, shipped, in Deploy complete) or that he asked for. It does not archive: the card goes into the board's Done pile, and Hugh's Archive all moves it into Done. Refused in Ship it, for a stack's child (ask for the lead), and after Hugh said Not now until he or the card's own session changes it.",
          "inputSchema": { "type": "object", "properties": { "card": card, "if_version": { "type": "integer" }, "reason": { "type": "string", "description": "Why it can go, for Hugh's Done pile." }, "evidence": { "type": "string", "description": "What shows the work is live (a merged branch, a ship, Hugh's words)." } }, "required": ["card", "if_version", "reason"] } },
        { "name": "kanban_router_log", "description": "Your own log. With text: add a line (kind decision or note; card: the card it is about). Without text: read your last lines, newest first (limit, default 20).",
          "inputSchema": { "type": "object", "properties": { "text": { "type": "string" }, "kind": { "type": "string", "enum": ["decision", "note"] }, "card": { "type": "integer" }, "limit": { "type": "integer" } } } },
    ])
}

/// What the Router is offered over MCP.
pub fn tool_list() -> Value {
    let mut out: Vec<Value> = crate::tools::schemas()
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|t| !NOT_FOR_ROUTER.contains(&t["name"].as_str().unwrap_or("")))
        .map(|mut t| {
            // Its writes and moves need the card's version.
            let name = t["name"].as_str().unwrap_or("").to_string();
            if name == "kanban_move_card" || name == "kanban_update_card" {
                t["inputSchema"]["properties"]["if_version"] = json!({ "type": "integer", "description": "The card's version from the event or your last read. Required." });
                t["inputSchema"]["required"] = json!(if name == "kanban_move_card" { json!(["card", "column", "reason", "if_version"]) } else { json!(["card", "fields", "if_version"]) });
            }
            if name == "kanban_list_cards" {
                t["inputSchema"]["properties"]["compact"] = json!({ "type": "boolean", "description": "Leave out summaries: number, title, column, labels, version, nextStep (short), blockedBy, stack." });
            }
            t
        })
        .collect();
    out.extend(own_schemas().as_array().cloned().unwrap_or_default());
    out.extend(crate::control::schemas().as_array().cloned().unwrap_or_default().into_iter().filter(|t| SESSION_TOOLS.contains(&t["name"].as_str().unwrap_or(""))));
    Value::Array(out)
}

fn number(args: &Value) -> Result<i64, String> {
    args.get("card").and_then(Value::as_i64).ok_or_else(|| "say which card: pass its number as `card`".into())
}

fn version(args: &Value) -> Result<i64, String> {
    args.get("if_version").and_then(Value::as_i64).ok_or_else(|| "pass if_version: the card's version from the event or your last read".into())
}

/// The card, if it is still at `if_version` and the Router may act on it now.
fn card_for_write(app: &AppHandle, store: &Store, args: &Value) -> Result<Card, String> {
    let c = store.get_by_number(number(args)?).map_err(|e| e.to_string())?;
    let v = version(args)?;
    if c.version != v {
        return Err(format!("card #{} changed: you had version {v}, it is now at {}. Read it again and decide again.", c.number, c.version));
    }
    if c.archived {
        return Err(format!("card #{} is archived", c.number));
    }
    if let Some(why) = routersession::hands_off(app, store, &c) {
        return Err(format!("refused: {why}"));
    }
    Ok(c)
}

/// Whether a create with these args would start a session (`to`, or a column that starts one).
pub fn create_starts(args: &Value) -> bool {
    args.get("to").and_then(Value::as_str).is_some_and(|t| !t.trim().is_empty())
        || (args.get("stack").map_or(true, Value::is_null) && args.get("column").and_then(Value::as_str).is_some_and(|c| ["Planning", "In progress"].contains(&c)))
}

fn log_decision(app: &AppHandle, card: Option<i64>, text: &str) {
    if let Ok(mut s) = app.state::<Board>().0.lock() {
        let _ = s.add_router_log("decision", card, &crate::transcript::cut_words(text, 400), ROUTER_BY);
    }
    let _ = tauri::Emitter::emit(app, "router-changed", ());
}

pub fn call(app: &AppHandle, row: &str, name: &str, args: &Value) -> Result<Value, String> {
    let may_start = app.state::<Board>().0.lock().map(|s| s.router_may_start()).unwrap_or(false);
    match name {
        n if NOT_FOR_ROUTER.contains(&n) || n == "kanban_request_ship" => Err("the Router doesn't mark tests passed or report ships: only Hugh tests and ships".into()),
        "kanban_get_card" | "kanban_stack_list" | "kanban_notify" => {
            if name != "kanban_stack_list" {
                number(args)?;
            }
            crate::tools::call_as_router(app, row, name, args)
        }
        "kanban_list_cards" => {
            if args.get("compact").and_then(Value::as_bool) == Some(true) {
                let board = app.state::<Board>();
                let store = board.0.lock().map_err(|e| e.to_string())?;
                return Ok(compact_list(&store, args.get("column").and_then(Value::as_str)));
            }
            crate::tools::call_as_router(app, row, name, args)
        }
        "kanban_add_log" => {
            number(args)?;
            let out = crate::tools::call_as_router(app, row, name, args)?;
            Ok(out)
        }
        "kanban_update_card" => {
            {
                let board = app.state::<Board>();
                let store = board.0.lock().map_err(|e| e.to_string())?;
                card_for_write(app, &store, args)?;
            }
            crate::tools::call_as_router(app, row, name, args)
        }
        "kanban_move_card" => {
            let (c, to) = {
                let board = app.state::<Board>();
                let store = board.0.lock().map_err(|e| e.to_string())?;
                let c = card_for_write(app, &store, args)?;
                let to = args.get("column").and_then(Value::as_str).ok_or("pass the column")?.to_string();
                if args.get("reason").and_then(Value::as_str).map_or(true, |r| r.trim().is_empty()) {
                    return Err("pass a reason: it goes on the card".into());
                }
                // Already there: nothing moves, so nothing follows (no after-move, no decision line).
                if c.column == to {
                    return Ok(json!(format!("Card #{} is already in {to}.", c.number)));
                }
                if let Some(why) = routersession::move_refused(&c.column, &to, may_start) {
                    return Err(format!("refused: {why}"));
                }
                (c, to)
            };
            let out = crate::tools::call_as_router(app, row, name, args)?;
            follow_up_move(app, &c.id, &c.column);
            log_decision(app, Some(c.number), &format!("Moved #{} {} -> {to}: {}", c.number, c.column, args.get("reason").and_then(Value::as_str).unwrap_or("")));
            Ok(out)
        }
        "kanban_create_card" => {
            if let Some(col) = args.get("column").and_then(Value::as_str) {
                if !CREATE_COLUMNS.contains(&col) {
                    return Err(format!("refused: the Router makes cards in {} only, not {col}", CREATE_COLUMNS.join(", ")));
                }
            }
            if create_starts(args) && !may_start {
                return Err("refused: making a card that starts at once needs Hugh's \"Router may start cards\" setting, which is off. Make it in Backlog.".into());
            }
            if let Some(lead) = args.get("stack").and_then(Value::as_i64) {
                stack_start_refused(app, lead, may_start)?;
            }
            let out = crate::tools::call_as_router(app, row, name, args)?;
            let n = out.get("number").and_then(Value::as_i64);
            log_decision(app, n, &format!("Made card #{}: {}", n.unwrap_or(0), args.get("title").and_then(Value::as_str).unwrap_or("")));
            Ok(out)
        }
        "kanban_stack_add" => {
            stack_start_refused(app, args.get("stack").and_then(Value::as_i64).ok_or("pass the lead card's number as `stack`")?, may_start)?;
            crate::tools::call_as_router(app, row, name, args)
        }
        "kanban_stack_remove" => crate::tools::call_as_router(app, row, name, args),
        "kanban_stack_join" | "kanban_stack_detach" => {
            if !may_start {
                return Err("refused: a join or detach can start a session, which needs Hugh's \"Router may start cards\" setting".into());
            }
            crate::tools::call_as_router(app, row, name, args)
        }
        "kanban_test_app" => {
            test_app_refused(args, kanban_store::is_test())?;
            crate::testapp::tool(app, None, args)
        }
        "kanban_start_card" => start_card(app, args, may_start),
        "kanban_archive_card" => ask_archive(app, args),
        "kanban_router_log" => router_log(app, args),
        "kanban_session_status" | "kanban_session_read" => crate::control::call(app, name, args),
        "kanban_session_send" => {
            let (id, live) = {
                let board = app.state::<Board>();
                let store = board.0.lock().map_err(|e| e.to_string())?;
                let c = store.get_by_number(number(args)?).map_err(|e| e.to_string())?;
                if let Some(why) = routersession::hands_off(app, &store, &c) {
                    return Err(format!("refused: {why}"));
                }
                let live = app.state::<Arc<crate::sessions::Sessions>>().is_live(&c.id).is_some();
                (c.id, live)
            };
            // A message to a stopped session resumes it: that is a start.
            if !live && !may_start {
                return Err("refused: that card's session isn't running, and a message would start it; that needs Hugh's \"Router may start cards\" setting".into());
            }
            let _ = id;
            let out = crate::control::call(app, name, args)?;
            log_decision(app, args.get("card").and_then(Value::as_i64), &format!("Sent #{} a message", args.get("card").and_then(Value::as_i64).unwrap_or(0)));
            Ok(out)
        }
        _ => Err(format!("no tool called {name}")),
    }
}

/// The owner the Router's test-app turns go by (`install.ps1` runs with KANBAN_LOCK_OWNER=router).
pub const TEST_APP_OWNER: &str = "router";

/// The columns the Router may make a card in.
pub const CREATE_COLUMNS: [&str; 4] = ["Backlog", "Blocked", "Planning", "In progress"];
/// The columns whose card the Router may start: the building ones, never a ship's or Done.
pub const START_COLUMNS: [&str; 2] = ["Planning", "In progress"];

/// Why the Router may not add a card to stack `lead` now: a stack that is running starts the
/// new card's session (`stackrun::joined_started_stack`), which needs "Router may start cards".
fn stack_start_refused(app: &AppHandle, lead: i64, may_start: bool) -> Result<(), String> {
    if may_start {
        return Ok(());
    }
    let board = app.state::<Board>();
    let store = board.0.lock().map_err(|e| e.to_string())?;
    let l = store.get_by_number(lead).map_err(|e| e.to_string())?;
    let running = app.state::<Arc<crate::sessions::Sessions>>().is_live(&l.id).is_some() || START_COLUMNS.contains(&l.column.as_str());
    if running {
        return Err(format!("refused: stack #{lead} is under way, so a card added to it starts at once; that needs Hugh's \"Router may start cards\" setting"));
    }
    Ok(())
}

/// Why the Router may not make this test-app call (finding: it is no outside caller): only the
/// status, an install turn and its own release, never a forced release; a test-channel Router
/// only reads the status.
pub fn test_app_refused(args: &Value, test_channel: bool) -> Result<(), String> {
    let action = args.get("action").and_then(Value::as_str).unwrap_or("status");
    if test_channel && action != "status" {
        return Err("refused: a test-channel Router never installs or takes test-app turns".into());
    }
    if args.get("force").and_then(Value::as_bool) == Some(true) {
        return Err("refused: only Hugh forces a release of the test app".into());
    }
    if !["status", "take", "release"].contains(&action) {
        return Err(format!("refused: the Router may only read the status, take an install turn or release its own, not {action}"));
    }
    if action == "take" && args.get("install").and_then(Value::as_bool) != Some(true) {
        return Err("refused: the Router takes only install turns (install: true); testers take their own".into());
    }
    if action != "status" && args.get("owner").and_then(Value::as_str) != Some(TEST_APP_OWNER) {
        return Err(format!("refused: the Router takes and releases turns only as owner \"{TEST_APP_OWNER}\" (pass owner: \"{TEST_APP_OWNER}\")"));
    }
    Ok(())
}

/// After a Router move that went through (final-test F1): the card's after-move, as for Hugh's drag,
/// on its own thread once the board is let go. Its session starts in a column that starts one,
/// or gets the column's message (a card sent back to In progress hears why from the move).
pub fn follow_up_move(app: &AppHandle, card_id: &str, from: &str) {
    let (a, id, from) = (app.clone(), card_id.to_string(), from.to_string());
    std::thread::spawn(move || crate::dispatch::after_router_move(&a, &id, &from));
}

/// The folder a started card runs in, as Hugh's start picks it (`start_session`): its worktree
/// if it is a folder here, else its repo's folder.
pub fn start_folder(worktree: Option<&str>, repo: Option<&str>) -> String {
    match worktree.filter(|p| std::path::Path::new(p).is_dir()) {
        Some(p) => p.to_string(),
        None => crate::sessions::repo_folder(repo).display().to_string(),
    }
}

fn start_card(app: &AppHandle, args: &Value, may_start: bool) -> Result<Value, String> {
    if !may_start {
        return Err("refused: starting a card needs Hugh's \"Router may start cards\" setting, which is off".into());
    }
    let board = app.state::<Board>();
    let mut store = board.0.lock().map_err(|e| e.to_string())?;
    let c = card_for_write(app, &store, args)?;
    if !START_COLUMNS.contains(&c.column.as_str()) {
        return Err(format!("refused: card #{} is in {}; the Router starts only cards in {}", c.number, c.column, START_COLUMNS.join(" or ")));
    }
    // Where it runs is Hugh's start's rule (final-test F1): its worktree when it has one on this
    // machine, else its repo's folder.
    let wt = start_folder(c.fields.get("worktreePath").and_then(Value::as_str), c.fields.get("repo").and_then(Value::as_str));
    // Never the home folder: a card whose repo has no folder here has nowhere to run.
    if kanban_store::home_var().is_some_and(|h| std::path::Path::new(&wt) == h.as_path()) {
        return Err(format!("refused: card #{} has no worktree and no repo folder on this machine; set its worktreePath or repo first", c.number));
    }
    let mode = if crate::conversation(&store, &c).is_some() { "resume" } else { "new" };
    let begun = crate::begin_session(app, &mut store, &c, mode, "router")?;
    let text = match begun {
        crate::Begun::Started(_) => format!("Started card #{}'s session ({mode}) in {wt}.", c.number),
        crate::Begun::Queued(pos) => format!("Card #{}'s start waits under Hugh's cap: number {pos} in line.", c.number),
    };
    let _ = store.add_log(&c.id, &format!("The Router started its session ({mode})"), ROUTER_BY);
    if let Some(m) = args.get("message").and_then(Value::as_str).filter(|m| !m.trim().is_empty()) {
        let _ = store.queue(&c.id, "message", m);
    }
    drop(store);
    log_decision(app, Some(c.number), &text);
    let _ = tauri::Emitter::emit(app, "cards-changed", ());
    Ok(json!(text))
}

/// The Router's ask to archive a card (card #131): the store's rules, after the Router's own
/// checks (its version, not archived, hands off a card Hugh has open or just changed).
fn ask_archive(app: &AppHandle, args: &Value) -> Result<Value, String> {
    let board = app.state::<Board>();
    let mut store = board.0.lock().map_err(|e| e.to_string())?;
    let c = card_for_write(app, &store, args)?;
    let reason = args.get("reason").and_then(Value::as_str).unwrap_or("");
    let evidence = args.get("evidence").and_then(Value::as_str);
    store.ask_archive(c.number, c.version, reason, evidence).map_err(|e| format!("refused: {e}"))?;
    let pile = store.list(false).map(|l| l.iter().filter(|k| k.fields.contains_key(kanban_store::ARCHIVE_ASK_KEY)).count()).unwrap_or(0);
    drop(store);
    let _ = tauri::Emitter::emit(app, "cards-changed", ());
    let _ = tauri::Emitter::emit(app, "router-changed", ());
    Ok(json!(format!("Card #{} is in Hugh's Done pile ({pile} there now). He archives the pile with one click.", c.number)))
}

fn router_log(app: &AppHandle, args: &Value) -> Result<Value, String> {
    let board = app.state::<Board>();
    let mut store = board.0.lock().map_err(|e| e.to_string())?;
    match args.get("text").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
        Some(t) => {
            let kind = args.get("kind").and_then(Value::as_str).unwrap_or("decision");
            if !["decision", "note"].contains(&kind) {
                return Err("kind is decision or note".into());
            }
            store.add_router_log(kind, args.get("card").and_then(Value::as_i64), t, ROUTER_BY).map_err(|e| e.to_string())?;
            drop(store);
            let _ = tauri::Emitter::emit(app, "router-changed", ());
            Ok(json!("Logged."))
        }
        None => {
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(20).min(200) as usize;
            Ok(serde_json::to_value(store.router_log(0, limit).map_err(|e| e.to_string())?).unwrap_or(json!([])))
        }
    }
}

/// The board as the Router reads it most of the time (finding 10): no summaries, no logs.
pub fn compact_list(store: &Store, column: Option<&str>) -> Value {
    let cards = store.list(false).unwrap_or_default();
    Value::Array(
        cards
            .iter()
            .filter(|c| column.map_or(true, |x| c.column == x))
            .map(|c| {
                let next = c.fields.get("nextStep").and_then(Value::as_str).map(|s| crate::transcript::cut_words(s, 120));
                let state = |k: &str| c.fields.get(k).and_then(|v| v.get("state")).cloned().unwrap_or(Value::Null);
                json!({ "card": c.number, "title": c.title(), "column": c.column, "version": c.version, "labels": c.fields.get("labels"),
                        "repo": c.fields.get("repo"), "branch": c.fields.get("branch"), "dryRun": c.fields.get("dryRun"),
                        "nextStep": next, "blockedBy": c.fields.get(kanban_store::BLOCKED_KEY), "stack": c.fields.get("stack"), "stackOf": c.fields.get("stackOf"),
                        "route": c.fields.get("route").and_then(|r| r.get("size")).cloned(), "test": state("test"), "review": state("review"),
                        "stuck": c.fields.get("stuck").and_then(|s| s.get("kind")).cloned() })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_create_that_starts_a_session_is_seen() {
        assert!(create_starts(&json!({ "title": "x", "to": "Planning" })));
        assert!(create_starts(&json!({ "title": "x", "column": "In progress" })));
        assert!(!create_starts(&json!({ "title": "x", "column": "Backlog" })));
        assert!(!create_starts(&json!({ "title": "x" })));
        assert!(!create_starts(&json!({ "title": "x", "column": "Planning", "stack": 4 })));
    }

    #[test]
    fn the_router_is_offered_its_tools_and_never_a_ship() {
        let names: Vec<String> = tool_list().as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        for t in ["kanban_get_card", "kanban_list_cards", "kanban_move_card", "kanban_update_card", "kanban_create_card", "kanban_start_card", "kanban_router_log", "kanban_session_send", "kanban_notify", "kanban_archive_card"] {
            assert!(names.iter().any(|n| n == t), "{t}");
        }
        for t in ["kanban_set_passed", "kanban_ship_result", "kanban_router", "kanban_session_start", "kanban_session_stop", "kanban_session_keys", "kanban_search"] {
            assert!(!names.iter().any(|n| n == t), "{t}");
        }
        let mv = tool_list().as_array().unwrap().iter().find(|t| t["name"] == "kanban_move_card").cloned().unwrap();
        assert!(mv["inputSchema"]["required"].as_array().unwrap().iter().any(|v| v == "if_version"));
        // Card #131: the archive ask needs the card's version and a reason.
        let ar = tool_list().as_array().unwrap().iter().find(|t| t["name"] == "kanban_archive_card").cloned().unwrap();
        let req: Vec<&str> = ar["inputSchema"]["required"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        for k in ["card", "if_version", "reason"] {
            assert!(req.contains(&k), "{k}");
        }
    }

    #[test]
    fn the_compact_list_has_no_summaries() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("board.db")).unwrap();
        let mut p = kanban_store::Patch::new();
        p.insert("title".into(), json!("A"));
        p.insert("summary".into(), json!("x".repeat(5000)));
        p.insert("nextStep".into(), json!("y ".repeat(200)));
        s.create(None, p, "Hugh").unwrap();
        let v = compact_list(&s, None);
        let text = v.to_string();
        assert!(!text.contains("xxxx"), "no summary");
        assert!(text.len() < 1000, "{}", text.len());
        assert_eq!(v[0]["version"], 1);
    }

    #[test]
    fn the_routers_test_app_calls_are_its_own_never_forced() {
        assert!(test_app_refused(&json!({ "action": "status" }), false).is_ok());
        assert!(test_app_refused(&json!({ "action": "take", "install": true, "owner": "router", "label": "wave 2" }), false).is_ok());
        assert!(test_app_refused(&json!({ "action": "release", "owner": "router" }), false).is_ok());
        assert!(test_app_refused(&json!({ "action": "release", "owner": "router", "force": true }), false).is_err());
        assert!(test_app_refused(&json!({ "action": "release", "owner": "hugh" }), false).is_err());
        assert!(test_app_refused(&json!({ "action": "take", "owner": "router" }), false).is_err(), "no tester turns");
        assert!(test_app_refused(&json!({ "action": "release-mkdir" }), false).is_err());
        assert!(test_app_refused(&json!({ "action": "take", "install": true, "owner": "router" }), true).is_err(), "a test-channel Router");
    }

    #[test]
    fn the_router_starts_and_makes_cards_only_in_its_columns() {
        for c in ["Ship it", "Deploy complete", "Done", "Backlog", "Human test", "Blocked"] {
            assert!(!START_COLUMNS.contains(&c), "{c}");
        }
        for c in ["Ship it", "Deploy complete", "Done", "Human test"] {
            assert!(!CREATE_COLUMNS.contains(&c), "{c}");
        }
    }

    /// Final-test F1: a Router move runs the card's after-move (as Hugh's drag), so the session
    /// starts or hears the move; a lead's children are not started by it.
    #[test]
    fn a_router_move_runs_the_cards_after_move() {
        let src = include_str!("routertools.rs").replace("\r\n", "\n");
        let arm = src.split("\"kanban_move_card\" => {").nth(1).unwrap().split("\"kanban_create_card\"").next().unwrap();
        let call = arm.find("call_as_router").expect("the move goes through the card tool");
        let follow = arm.find("follow_up_move(app, &c.id, &c.column)").expect("then its after-move, from the column it left");
        assert!(follow > call, "after the move went through, not before");
        let same = arm.find("if c.column == to").expect("a move to its own column returns early");
        assert!(same < call, "before anything moves or follows");
        let d = include_str!("dispatch.rs").replace("\r\n", "\n");
        let body = d.split("pub fn after_router_move").nth(1).unwrap().split("\n}\n").next().unwrap();
        assert!(body.contains("after_move_card(app, card_id, from)"), "the card's own after-move");
        assert!(!body.contains("start_stack"), "never a stack's children");
        assert!(body.contains("ship_start_refused"), "a card that goes on by itself is checked again");
    }

    /// Final-test F1: a card with no worktree starts where Hugh's start would.
    #[test]
    fn a_card_with_no_worktree_starts_in_its_repo_folder() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path().display().to_string();
        assert_eq!(start_folder(Some(&wt), Some("hughht5/kanban-app")), wt);
        let repo = crate::sessions::repo_folder(Some("hughht5/kanban-app")).display().to_string();
        assert_eq!(start_folder(None, Some("hughht5/kanban-app")), repo);
        assert_eq!(start_folder(Some("C:/no/such/folder/119"), Some("hughht5/kanban-app")), repo, "a worktree not on this machine");
    }
}
