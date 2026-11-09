//! The kanban_* tools, for card sessions (over MCP) and `kb` (over the REST route).

use crate::{dispatch, sessions::Sessions, Board};
use kanban_store::{Card, Patch, Store};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

/// Fields a session may set with kanban_update_card. The column, ship and plan
/// states and the number belong to moves, ship results and the board.
pub(crate) const UPDATABLE: &[&str] = &[
    "title", "summary", "nextStep", "blockedReason", "repo", "issue", "pr", "branch", "worktreePath",
    "devStack", "persona", "logins", "links", "dryRun", "choices", "labels",
    "changesOutputs",
    "doneWhen", // card #105: one line, the card is done when ...
    "doneWhenChecks", // card #105: its acceptance checks
    "blockedBy",
];

/// Fields kanban_create_card copies from its arguments. Not `subagents` (card #151): a card made
/// by a session or the Router gets the board's defaults; only Hugh's Add card chooses.
pub(crate) const CREATE_FIELDS: &[&str] = &["column", "repo", "summary", "issue", "labels", "dryRun"];

/// How to write the card #105 fields, for kanban_update_card's description.
const DONE_WHEN_HINT: &str = "doneWhen is one plain line (the card is done when ...); doneWhenChecks is a list of up to 10 acceptance checks, each a string or {text, state: open|met|failed, note}, and the app records who changed each state and when.";

/// May this caller set `blockedBy` (card #109) on a card whose lead is `lead`? Hugh (in the app)
/// and sessions outside the app may. A card's own session may only order the children of the
/// stack its card leads: on such a child, with every wait inside that stack.
fn waits_refused(store: &Store, own: Option<&str>, lead: Option<i64>, v: &Value) -> Option<String> {
    let me = store.get(own?).ok()?;
    let list: Vec<i64> = kanban_store::clean_blocked_by(v)
        .ok()
        .flatten()
        .and_then(|l| l.as_array().map(|a| a.iter().filter_map(Value::as_i64).collect()))
        .unwrap_or_default();
    if lead == Some(me.number) && list.iter().all(|n| me.stack().contains(n)) {
        return None;
    }
    Some(format!(
        "a card's session sets blockedBy only on the children of the stack its own card leads, each wait inside that stack (card #{} {}). Say in nextStep which cards this one waits on, and Hugh sets it",
        me.number,
        if me.is_lead() { "leads a stack" } else { "leads no stack" }
    ))
}

pub fn schemas() -> Value {
    let mut list = base_schemas();
    if let Some(a) = list.as_array_mut() {
        a.push(crate::testapp::schema());
    }
    list
}

fn base_schemas() -> Value {
    let card = json!({ "type": "integer", "description": "The card's number. Leave it out for your own card." });
    json!([
        { "name": "kanban_get_card", "description": "Read a card: your own, or one by number.",
          "inputSchema": { "type": "object", "properties": { "card": card } } },
        { "name": "kanban_list_cards", "description": "List the board's cards (number, title, column, labels, summary, nextStep, blockedBy and waitedOn, and for stacks: stack, stackOf, passed).",
          "inputSchema": { "type": "object", "properties": { "column": { "type": "string" }, "include_archived": { "type": "boolean" } } } },
        { "name": "kanban_update_card", "description": format!("Change fields on a card. Allowed: {}. {DONE_WHEN_HINT} blockedBy is the card numbers it waits on, like [12, 40] (null for none): a card waiting in Blocked, put there by Hugh, starts by itself once they are ready. Pass if_version to refuse the write if the card changed since you read it.", UPDATABLE.join(", ")),
          "inputSchema": { "type": "object", "properties": { "card": card, "if_version": { "type": "integer" }, "fields": { "type": "object" } }, "required": ["fields"] } },
        { "name": "kanban_add_log", "description": "Add a line to a card's log.",
          "inputSchema": { "type": "object", "properties": { "card": card, "text": { "type": "string" } }, "required": ["text"] } },
        { "name": "kanban_move_card", "description": "Move a card to another column, with a one-line reason. A move of your own card waits until your turn ends. Never Ship it. A stack's lead goes to Human test only when its children are ready; the app moves it then. A card that waits on cards not ready yet (blockedBy) goes into Planning or later only once they are.",
          "inputSchema": { "type": "object", "properties": { "card": card, "column": { "type": "string", "enum": kanban_store::COLUMNS }, "reason": { "type": "string" } }, "required": ["column"] } },
        { "name": "kanban_create_card", "description": "Add a card. It gets the next number. Pass `stack` (a lead card's number) to create it straight into that stack.",
          "inputSchema": { "type": "object", "properties": { "id": { "type": "string" }, "title": { "type": "string" }, "column": { "type": "string" }, "repo": { "type": "string" }, "summary": { "type": "string" }, "issue": { "type": "object" }, "labels": { "type": "array", "items": { "type": "string" }, "description": "Short labels, like [\"BUG\", \"RCS\"]." }, "dryRun": { "type": "boolean", "description": "A test card: its sessions change nothing real." }, "stack": { "type": "integer", "description": "The lead card's number: the new card joins its stack, at the end." }, "template": { "type": "string", "description": "A card template's id (e.g. kanban-app, bug): its starting prompt opens the card's first session." }, "to": { "type": "string", "enum": ["Planning", "Human test"], "description": "Only for Hugh's own sessions, when he asked: start the card now (it goes into Planning and its session starts). \"Human test\" also sets it to go on by itself to Human test. A card's own session can't use it." } }, "required": ["title"] } },
        { "name": "kanban_stack_list", "description": "A stack: its lead card (number, title, column, branch, pr, devStack, logins) and its children in order (number, title, column, passed, nextStep, blockedReason, choices), plus the board's rule for when the stack goes to Human test (\"all\" or \"first\") and whether it is ready.",
          "inputSchema": { "type": "object", "properties": { "stack": { "type": "integer", "description": "The lead card's number. Leave it out for your own card's stack (the one it leads or is in)." } } } },
        { "name": "kanban_stack_add", "description": "Put a card into a stack (at the end), making the lead a stack if it isn't one. A card is in at most one stack, and a stack is one repo; a lead can't be a child; a card in Ship it or archived can't join, and nothing joins a stack in Ship it (only Hugh does that, by dropping a card on it in the app).",
          "inputSchema": { "type": "object", "properties": { "stack": { "type": "integer", "description": "The lead card's number." }, "card": { "type": "integer", "description": "The card to add." } }, "required": ["stack", "card"] } },
        { "name": "kanban_stack_remove", "description": "Take a card out of a stack: it goes back on the board as an ordinary card in its own column.",
          "inputSchema": { "type": "object", "properties": { "stack": { "type": "integer", "description": "The lead card's number." }, "card": { "type": "integer", "description": "The card to take out." } }, "required": ["stack", "card"] } },
        { "name": "kanban_stack_join", "description": "Join a card to a stack the way Hugh's drag does (making the lead a stack if it isn't one), and brief the stack's session (started if need be): a card not built yet is to be planned and built on the stack's branch; a built one (Human test or later, or with a PR) has its branch merged into the stack's and its PR closed in favour of the stack's. Its own session, if any, is told to stand down. The same limits as kanban_stack_add: joining a stack in Ship it, taking a card out of Ship it or putting a whole stack into another is Hugh's drop in the app only.",
          "inputSchema": { "type": "object", "properties": { "stack": { "type": "integer", "description": "The lead card's number (any card: it becomes a stack)." }, "card": { "type": "integer", "description": "The card to join." } }, "required": ["stack", "card"] } },
        { "name": "kanban_stack_detach", "description": "Detach a card from its stack the way Hugh's Detach button does: the stack's session is told to stop building it and log what it already did; the card becomes its own card and goes to Planning, where its own session starts (a card already shipping or shipped just leaves the stack).",
          "inputSchema": { "type": "object", "properties": { "stack": { "type": "integer", "description": "The lead card's number." }, "card": { "type": "integer", "description": "The card to detach." } }, "required": ["stack", "card"] } },
        { "name": "kanban_set_passed", "description": "Mark a stack's child as passed (Hugh tested it), or take that back with passed: false. A passed child stays in Human test until the stack ships.",
          "inputSchema": { "type": "object", "properties": { "card": { "type": "integer", "description": "The child card's number." }, "passed": { "type": "boolean", "default": true } }, "required": ["card"] } },
        { "name": "kanban_ship_result", "description": "Report a ship's progress: shipping, shipped, failed or needs-hugh, with detail.",
          "inputSchema": { "type": "object", "properties": { "card": card, "state": { "type": "string", "enum": ["shipping", "shipped", "failed", "needs-hugh"] }, "detail": { "type": "string" } }, "required": ["state"] } },
        { "name": "kanban_notify", "description": "Tell Hugh something about a card (a Windows notification and a log line).",
          "inputSchema": { "type": "object", "properties": { "card": card, "text": { "type": "string" } }, "required": ["text"] } },
        { "name": "kanban_merge", "description": "Only in Ship it, for a card with a branch: ask your repo's merge lane for your ship's merge, after you committed your work. Every ship's merge goes through its repo's lane. The app finds the lane's checkout (your worktree's main checkout, else the repo's own-worktree checkout, else its repos.json folder). The lane rebases your branch onto the latest origin/main in its own worktree and runs the checks; your branch doesn't move. End your turn after it: the app sends you the verdict and what to do next. Refused (saying why) outside Ship it, before the ship started, with no branch or worktreePath, with uncommitted work, while a merge waits or runs, or when the app finds no checkout for the repo (then report needs-hugh with its reason).",
          "inputSchema": { "type": "object", "properties": { "branches": { "type": "array", "items": { "type": "string" }, "description": "Other branches of this ship, in order (a card Hugh dropped on this one)." }, "rules": { "type": "string", "description": "The repo's merge rules (how to resolve a version, a changelog, files that always conflict), in its own words." }, "checks": { "type": "string", "description": "The check commands to run (lint, typecheck, tests, build)." } } } },
        { "name": "kanban_router", "description": "Talk to the Router, the long-running session that routes and checks the board's cards (card #119). Pass text to send it a message (data for it, about your card), and/or replies: n to read its last n replies. Refused while the Router isn't running; nothing but Hugh starts it.",
          "inputSchema": { "type": "object", "properties": { "text": { "type": "string" }, "replies": { "type": "integer", "description": "How many of its latest replies to read (1 to 20)." } } } },
    ])
}

/// The first field in `fields` that kanban_update_card may not set.
fn not_updatable(fields: &serde_json::Map<String, Value>) -> Option<&String> {
    fields.keys().find(|k| !UPDATABLE.contains(&k.as_str()))
}

/// Tools only `kb` reaches (over /tool), never listed for or callable by a card session's MCP.
pub const CLI_ONLY: &[&str] = &["kanban_request_ship"];

/// Tools only a card's own session is offered (card #160): not the Router, a lane, `kb` or a
/// session outside the app.
pub const CARD_SESSION_ONLY: &[&str] = &["kanban_merge"];

/// `kb ship <n> --asked-by-hugh`: refused without the flag, and from a card's own session.
pub fn ship_request_allowed(own: Option<&str>, args: &Value) -> Result<(), String> {
    if own.is_some() {
        return Err("a card's session can't ask for a ship; only Hugh moves a card into Ship it, in the app".into());
    }
    if args.get("asked_by_hugh").and_then(Value::as_bool) != Some(true) {
        return Err("refused: a ship needs Hugh. Run `kb ship <n> --asked-by-hugh` only when Hugh asked for it".into());
    }
    if args.get("card").and_then(Value::as_i64).is_none() {
        return Err("say which card: pass its number as `card`".into());
    }
    Ok(())
}

pub(crate) fn caller_card(app: &AppHandle, store: &Store, caller: Option<&str>) -> Option<String> {
    let row = caller?;
    app.state::<Arc<Sessions>>().card_of_row(row).or_else(|| store.session_card(row).ok().flatten())
}

fn target(store: &Store, own: Option<&str>, args: &Value) -> Result<Card, String> {
    match args.get("card").and_then(Value::as_i64) {
        Some(n) => store.get_by_number(n).map_err(|e| e.to_string()),
        None => match own {
            Some(id) => store.get(id).map_err(|e| e.to_string()),
            None => Err("say which card: pass its number as `card`".into()),
        },
    }
}

fn doc(store: &Store, c: &Card) -> Value {
    let mut log = store.log(&c.id).unwrap_or_default();
    log.truncate(20);
    let mut d = c.to_document(&log);
    d["id"] = json!(c.id);
    d["version"] = json!(c.version);
    // Card #151: the subagents on the card as they hold now: its own choice, else the board's
    // default (a card made before it). Hugh's only: kanban_update_card refuses the field.
    d[kanban_store::SUBAGENTS_KEY] = Value::Object(store.subagents_of(c));
    d
}

/// A stack as kanban_stack_list shows it. Card #113 adds, per child, its stage, session,
/// branch, waits and the state of its test, review and merge, and the merged count.
fn stack_json(app: &AppHandle, store: &Store, lead: &Card) -> Result<Value, String> {
    let f = |c: &Card, k: &str| c.fields.get(k).cloned().unwrap_or(Value::Null);
    let st = |c: &Card, k: &str| c.fields.get(k).and_then(|v| v.get("state")).cloned().unwrap_or(Value::Null);
    let kids = store.stack_children(lead).map_err(|e| e.to_string())?;
    let live = app.state::<std::sync::Arc<crate::sessions::Sessions>>();
    let queue = store.start_queue().unwrap_or_default();
    let session = |c: &Card| {
        if live.is_live(&c.id).is_some() {
            "running"
        } else if queue.position(&c.id).is_some() {
            "queued"
        } else if store.latest_session(&c.id).ok().flatten().is_some() {
            "paused"
        } else {
            "none"
        }
    };
    let children: Vec<Value> = kids
        .iter()
        .map(|k| json!({ "number": k.number, "title": k.title(), "column": k.column, "archived": k.archived, "passed": k.passed(),
                         "nextStep": f(k, "nextStep"), "blockedReason": f(k, "blockedReason"), "choices": f(k, "choices"),
                         "stage": crate::stackrun::stage(store, k, lead), "session": session(k), "branch": f(k, "branch"),
                         "waitsOn": crate::stackrun::waits_on(k), "test": st(k, kanban_store::TEST_KEY), "review": st(k, kanban_store::REVIEW_KEY),
                         "merge": st(k, "merge"),
                         "blockedBy": f(k, kanban_store::BLOCKED_KEY) }))
        .collect();
    let merged = kids.iter().filter(|k| !k.archived && kanban_store::STACK_READY.contains(&k.column.as_str()) && kanban_store::stack_merged(k, lead)).count();
    let rule = store.stack_rule();
    Ok(json!({
        "lead": { "number": lead.number, "title": lead.title(), "column": lead.column, "branch": f(lead, "branch"), "pr": f(lead, "pr"),
                  "worktreePath": f(lead, "worktreePath"), "devStack": f(lead, "devStack"), "logins": f(lead, "logins") },
        "children": children,
        "merged": merged,
        "rule": rule,
        "ready": kanban_store::stack_ready(lead, &kids, &rule),
    }))
}

pub fn call(app: &AppHandle, caller: Option<&str>, name: &str, args: &Value) -> Result<Value, String> {
    // The Router's calls (card #119) are held to its own rules first.
    if let Some(row) = caller.filter(|r| crate::routersession::is_router_row(app, r)) {
        return crate::routertools::call(app, row, name, args);
    }
    // A merge lane's (card #160): its four tools only.
    if let Some((row, key)) = caller.and_then(|r| crate::lane::key_of_row(app, r).map(|k| (r, k))) {
        return crate::lanetools::call(app, row, &key, name, args);
    }
    // A card session's request for its merge lane (card #160). Git reads its worktree, so it
    // takes the board's lock itself.
    if name == "kanban_merge" {
        return crate::lane::request(app, caller, args);
    }
    // A message to the Router, or its replies, from any session or kb (card #119).
    if name == "kanban_router" {
        return router_tool(app, caller, args);
    }
    // Test-app turns read files, so they take the board's lock themselves, after (card #115).
    if name == "kanban_test_app" {
        return crate::testapp::tool(app, caller, args);
    }
    call_inner(app, caller, name, args, false)
}

/// A card tool for the Router, after `routertools` checked its rules: no own card, its writes by
/// "the router", and held to the subagent gates and waits as a card session is.
pub fn call_as_router(app: &AppHandle, _row: &str, name: &str, args: &Value) -> Result<Value, String> {
    call_inner(app, None, name, args, true)
}

/// A card tool for a merge lane, after `lanetools` checked it (reads only): no own card.
pub fn call_as_lane(app: &AppHandle, name: &str, args: &Value) -> Result<Value, String> {
    call_inner(app, None, name, args, false)
}

/// `kanban_router`: send the Router a message (`text`), or read its last replies (`replies`).
fn router_tool(app: &AppHandle, caller: Option<&str>, args: &Value) -> Result<Value, String> {
    // A card session's words are data for the Router; Hugh's own (kb, or his sessions outside the
    // app) are his instructions, as in the panel.
    let (from, card_from) = {
        let board = app.state::<Board>();
        let store = board.0.lock().map_err(|e| e.to_string())?;
        match caller_card(app, &store, caller).and_then(|id| store.get(&id).ok()) {
            Some(c) => (format!("card #{}", c.number), Some(c.number)),
            None if caller.is_some() => ("Hugh's session outside the app".to_string(), None),
            None => ("Hugh (kb)".to_string(), None),
        }
    };
    let mut out = serde_json::Map::new();
    if let Some(t) = args.get("text").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
        match card_from {
            Some(n) => crate::routersession::send_by(app, t, Some(&from), Some(n), &format!("card #{n}'s session"))?,
            None => crate::routersession::send_by(app, &format!("From {from}: {t}"), None, None, if caller.is_some() { "Hugh (session)" } else { "Hugh (kb)" })?,
        }
        out.insert("sent".into(), json!(true));
    }
    let n = args.get("replies").and_then(Value::as_u64).map(|n| n.clamp(1, 20) as usize);
    if let Some(n) = n.or(if out.is_empty() { Some(1) } else { None }) {
        let board = app.state::<Board>();
        let store = board.0.lock().map_err(|e| e.to_string())?;
        out.insert("replies".into(), json!(crate::routersession::replies(&store, n)));
    }
    out.insert("running".into(), json!(app.state::<Arc<Sessions>>().is_live(crate::routersession::KEY).is_some()));
    Ok(Value::Object(out))
}

fn call_inner(app: &AppHandle, caller: Option<&str>, name: &str, args: &Value, as_router: bool) -> Result<Value, String> {
    let board = app.state::<Board>();
    let mut store = board.0.lock().map_err(|e| e.to_string())?;
    let own = caller_card(app, &store, caller);
    let by = match own.as_ref().and_then(|id| store.get(id).ok()) {
        Some(c) => format!("card #{}'s session", c.number),
        None if as_router => kanban_store::ROUTER_BY.to_string(),
        None => "a session outside the app".to_string(),
    };
    // Held to Hugh's rules as a card session is: its own session, or the Router.
    let held = own.is_some() || as_router;
    let out = match name {
        "kanban_get_card" => {
            let c = target(&store, own.as_deref(), args)?;
            let mut d = doc(&store, &c);
            // The card's step progress (card #106), when its session keeps a to-do list. Read
            // under the board lock is allowed: the steps worker never takes the board lock while
            // it holds the summaries' lock.
            if let Some(s) = app.try_state::<Arc<crate::steps::Steps>>().and_then(|st| st.get(&c.id)) {
                d["steps"] = json!({ "done": s.done, "total": s.total, "current": s.current, "line": s.line });
            }
            d
        }
        "kanban_list_cards" => {
            let all = args.get("include_archived").and_then(Value::as_bool).unwrap_or(false);
            let col = args.get("column").and_then(Value::as_str);
            let cards = store.list(all).map_err(|e| e.to_string())?;
            Value::Array(
                cards
                    .iter()
                    .filter(|c| col.map_or(true, |x| c.column == x))
                    .map(|c| json!({ "card": c.number, "id": c.id, "title": c.title(), "column": c.column, "archived": c.archived,
                                     "summary": c.fields.get("summary"), "nextStep": c.fields.get("nextStep"), "labels": c.fields.get("labels"),
                                     "stack": c.fields.get("stack"), "stackOf": c.fields.get("stackOf"), "passed": c.passed(),
                                     "blockedBy": c.fields.get(kanban_store::BLOCKED_KEY), "waitedOn": c.fields.get(kanban_store::WAITED_KEY) }))
                    .collect(),
            )
        }
        "kanban_update_card" => {
            let c = target(&store, own.as_deref(), args)?;
            let fields = args.get("fields").and_then(Value::as_object).ok_or("pass the fields to change as `fields`")?;
            if let Some(bad) = not_updatable(fields) {
                return Err(format!("\"{bad}\" can't be set this way (allowed: {})", UPDATABLE.join(", ")));
            }
            if let Some(ch) = fields.get("choices") {
                let ok = ch.is_null() || ch.as_array().map_or(false, |a| a.iter().all(|v| v.as_str().map_or(false, |t| !t.trim().is_empty())));
                if !ok {
                    return Err("choices is a list of answer strings (or null to clear it)".into());
                }
            }
            if let Some(why) = fields.get(kanban_store::BLOCKED_KEY).and_then(|v| waits_refused(&store, own.as_deref(), c.stack_of(), v)) {
                return Err(format!("refused: {why}"));
            }
            let version = args.get("if_version").and_then(Value::as_i64).unwrap_or(c.version);
            let card = store.update(&c.id, version, &fields.clone(), &by).map_err(|e| e.to_string())?;
            doc(&store, &card)
        }
        "kanban_add_log" => {
            let c = target(&store, own.as_deref(), args)?;
            let text = args.get("text").and_then(Value::as_str).ok_or("pass the line as `text`")?;
            store.add_log(&c.id, text, &by).map_err(|e| e.to_string())?;
            json!("Logged.")
        }
        "kanban_move_card" => {
            let c = target(&store, own.as_deref(), args)?;
            let to = args.get("column").and_then(Value::as_str).ok_or("pass the column")?;
            let reason = args.get("reason").and_then(Value::as_str).unwrap_or("");
            if to == "Ship it" {
                return Err("only Hugh moves a card into Ship it (or the app for him: Take it to, or small-change auto-ship); say in nextStep that it is ready".into());
            }
            if c.column == to {
                return Ok(json!(format!("Card #{} is already in {to}.", c.number)));
            }
            if let Some(why) = dispatch::stack_move_refused(&store, &c, to) {
                return Err(format!("refused: {why}"));
            }
            // Hugh's rules hold card sessions to the subagent review and test; his own sessions'
            // moves aren't.
            if let Some(why) = held.then(|| crate::agents::move_refused(&store, &c, to)).flatten() {
                let _ = store.add_log(&c.id, &format!("Refused a move to {to} by {by}: its subagent checks haven't passed"), "the app");
                return Err(format!("refused: {why}"));
            }
            if let Some(why) = held.then(|| dispatch::wait_move_refused(&store, &c, to)).flatten() {
                let _ = store.add_log(&c.id, &format!("Refused a move to {to} by {by}: it waits on cards that aren't ready"), "the app");
                return Err(format!("refused: {why}"));
            }
            if own.as_deref() == Some(c.id.as_str()) {
                store
                    .queue(&c.id, "move", &json!({ "column": to, "reason": reason }).to_string())
                    .map_err(|e| e.to_string())?;
                json!(format!("Queued: card #{} moves to {to} when your turn ends. Finish your turn as normal.", c.number))
            } else {
                dispatch::session_move(app, &mut store, &c, to, reason, &by)?;
                json!(format!("Moved card #{} to {to}.", c.number))
            }
        }
        "kanban_create_card" => {
            let title = args.get("title").and_then(Value::as_str).ok_or("pass a title")?;
            let mut fields = Patch::new();
            fields.insert("title".into(), json!(title));
            for &k in CREATE_FIELDS {
                if let Some(v) = args.get(k) {
                    fields.insert(k.into(), v.clone());
                }
            }
            if fields.get("column").and_then(Value::as_str) == Some("Ship it") {
                return Err("only Hugh moves a card into Ship it (or the app for him: Take it to, or small-change auto-ship)".into());
            }
            // Starting it (card #76): `to`, or a column that starts a session, from Hugh's own
            // session only; a card's session only places cards.
            let to = args.get("to").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty());
            let start = match to {
                Some(_) if own.is_some() => return Err("a card's session can't start a card; leave out `to`".into()),
                Some("Deploy complete") => return Err("only Hugh sets a card to ship by itself, in the app (Add card's Take it to)".into()),
                Some("Planning") => Some(("Planning".to_string(), None)),
                Some("Human test") => Some(("Planning".to_string(), Some("Human test"))),
                Some(t) => return Err(format!("`to` is Planning or Human test, not {t}")),
                None if own.is_none() && args.get("stack").map_or(true, Value::is_null) => fields
                    .get("column")
                    .and_then(Value::as_str)
                    .filter(|c| ["Planning", "In progress"].contains(c))
                    .map(|c| (c.to_string(), None)),
                None => None,
            };
            if let Some((_, auto)) = &start {
                fields.remove("column");
                if let Some(t) = auto {
                    fields.insert(kanban_store::AUTO_KEY.into(), json!(t));
                }
            }
            if let Some(t) = args.get("template").and_then(Value::as_str) {
                match store.template(t) {
                    Ok(Some(_)) => {
                        fields.insert("templateId".into(), json!(t));
                    }
                    _ => return Err(format!("no card template called \"{t}\"")),
                }
            }
            let id = args.get("id").and_then(Value::as_str);
            let card = match args.get("stack").filter(|v| !v.is_null()) {
                Some(v) => {
                    if id.is_some() {
                        return Err("a card made into a stack gets its id from the board; leave out `id`".into());
                    }
                    let lead = v.as_i64().ok_or("stack is the lead card's number")?;
                    let c = store.create_in_stack(fields, lead, &by).map_err(|e| e.to_string())?;
                    // Into a stack Hugh has started, it starts as its siblings did (card #113).
                    crate::stackrun::joined_started_stack(app, &mut store, &c.id);
                    crate::stackrun::lead_effort(app, &mut store, lead);
                    store.get(&c.id).map_err(|e| e.to_string())?
                }
                None => store.create(id, fields, &by).map_err(|e| e.to_string())?,
            };
            store.add_log(&card.id, &format!("Created by {by}"), &by).map_err(|e| e.to_string())?;
            let mut card = store.get(&card.id).map_err(|e| e.to_string())?;
            if let Some((col, auto)) = start {
                let why = match auto {
                    Some(t) => format!("he asked {by} to add it to go on by itself to {t}"),
                    None => format!("he asked {by} to add it to start in {col}"),
                };
                match dispatch::start_new(app, &mut store, &card, &col, &why) {
                    Ok(c) => card = c,
                    Err(e) => {
                        let _ = store.add_log(&card.id, &format!("Made, but couldn't start it in {col}: {e}. It waits in Backlog."), "the app");
                    }
                }
            }
            doc(&store, &card)
        }
        "kanban_stack_list" => {
            let c = target(&store, own.as_deref(), &json!({ "card": args.get("stack") }))?;
            let lead = match c.stack_of() {
                Some(n) if args.get("stack").and_then(Value::as_i64).is_none() => store.get_by_number(n).map_err(|e| e.to_string())?,
                Some(n) => return Err(format!("#{} is in stack #{n}; it doesn't lead one", c.number)),
                None => c,
            };
            if !lead.is_lead() {
                return Err(format!("#{} isn't a stack (it has no cards in it)", lead.number));
            }
            stack_json(app, &store, &lead)?
        }
        "kanban_stack_add" | "kanban_stack_remove" => {
            let lead = args.get("stack").and_then(Value::as_i64).ok_or("pass the lead card's number as `stack`")?;
            let child = args.get("card").and_then(Value::as_i64).ok_or("pass the card's number as `card`")?;
            let l = if name == "kanban_stack_add" {
                store.stack_add(lead, child, &by)
            } else {
                store.stack_remove(lead, child, &by)
            }
            .map_err(|e| e.to_string())?;
            if name == "kanban_stack_add" {
                if let Ok(c) = store.get_by_number(child) {
                    crate::stackrun::joined_started_stack(app, &mut store, &c.id);
                }
                crate::stackrun::lead_effort(app, &mut store, lead);
            }
            let l = store.get_by_number(l.number).map_err(|e| e.to_string())?;
            stack_json(app, &store, &l)?
        }
        "kanban_stack_join" | "kanban_stack_detach" => {
            let lead = args.get("stack").and_then(Value::as_i64).ok_or("pass the lead card's number as `stack`")?;
            let child = args.get("card").and_then(Value::as_i64).ok_or("pass the card's number as `card`")?;
            if name == "kanban_stack_join" {
                let l = crate::stack_join_op(app, &mut store, lead, child, &by, false)?;
                stack_json(app, &store, &l)?
            } else {
                let (c, from) = crate::stack_detach_op(app, &mut store, lead, child, &by)?;
                if let Some(from) = from {
                    // Starts its own session once this call lets go of the board.
                    let (a, id) = (app.clone(), c.id.clone());
                    std::thread::spawn(move || dispatch::after_move(&a, &id, &from));
                }
                json!(format!("Detached #{child} from stack #{lead}; it is its own card in {} now.", c.column))
            }
        }
        "kanban_set_passed" => {
            let c = target(&store, own.as_deref(), args)?;
            let on = args.get("passed").and_then(Value::as_bool).unwrap_or(true);
            let card = store.set_passed(&c.id, on, &by).map_err(|e| e.to_string())?;
            doc(&store, &card)
        }
        "kanban_ship_result" => {
            let c = target(&store, own.as_deref(), args)?;
            let state = args.get("state").and_then(Value::as_str).ok_or("pass the state")?;
            let detail = args.get("detail").and_then(Value::as_str).unwrap_or("");
            let out = dispatch::ship_result(app, &mut store, &c, state, detail, &by)?;
            // A card that went on by itself: is what was pushed the commit the app checked?
            if state == "shipped" {
                crate::goes_on::after_shipped(app, &c.id);
            }
            out
        }
        "kanban_request_ship" => {
            ship_request_allowed(own.as_deref(), args)?;
            let c = target(&store, None, args)?;
            if c.archived {
                return Err(format!("card #{} is archived", c.number));
            }
            if c.column == "Ship it" {
                return Ok(json!(format!("Card #{} is already in Ship it.", c.number)));
            }
            store
                .add_log(&c.id, "A session outside the app asked to ship this card: waiting for Hugh to confirm in the app", &by)
                .map_err(|e| e.to_string())?;
            // Never moved here: the app shows a banner, and only Hugh's click ships it.
            let _ = app.emit("ship-request", json!({ "card": c.id, "number": c.number, "title": c.title() }));
            json!(format!("Asked Hugh to confirm \"Ship #{}?\" in the app. It ships only when he clicks Ship.", c.number))
        }
        "kanban_notify" => {
            let c = target(&store, own.as_deref(), args)?;
            let text = args.get("text").and_then(Value::as_str).ok_or("pass the text")?;
            store.add_log(&c.id, &format!("Told Hugh: {text}"), &by).map_err(|e| e.to_string())?;
            let _ = app.emit("notify", json!({ "card": c.id, "number": c.number, "text": text }));
            json!("Hugh has been told.")
        }
        _ => return Err(format!("no tool called {name}")),
    };
    drop(store);
    let _ = app.emit("cards-changed", ());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Card #131: no session, `kb` or the Router can write the Done pile's fields through
    /// kanban_update_card; only kanban_archive_card asks, and only Hugh's page answers.
    #[test]
    fn no_mcp_caller_sets_the_done_piles_fields() {
        for k in [kanban_store::ARCHIVE_ASK_KEY, kanban_store::ARCHIVE_DECLINED_KEY] {
            let f = json!({ k: { "reason": "x" } });
            assert_eq!(not_updatable(f.as_object().unwrap()).map(String::as_str), Some(k), "{k}");
        }
    }

    #[test]
    fn t4_6_ship_request_needs_the_flag_and_an_outside_session() {
        assert!(ship_request_allowed(None, &json!({ "card": 34 })).unwrap_err().starts_with("refused"));
        assert!(ship_request_allowed(None, &json!({ "card": 34, "asked_by_hugh": false })).is_err());
        assert!(ship_request_allowed(Some("card-id"), &json!({ "card": 34, "asked_by_hugh": true })).is_err());
        assert!(ship_request_allowed(None, &json!({ "asked_by_hugh": true })).is_err());
        assert!(ship_request_allowed(None, &json!({ "card": 34, "asked_by_hugh": true })).is_ok());
    }

    #[test]
    fn stack_tools_are_listed() {
        let names: Vec<String> = schemas().as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        for t in ["kanban_stack_list", "kanban_stack_add", "kanban_stack_remove", "kanban_stack_join", "kanban_stack_detach", "kanban_set_passed"] {
            assert!(names.iter().any(|n| n == t), "{t}");
        }
        assert!(UPDATABLE.contains(&"choices"));
        assert!(UPDATABLE.contains(&"labels"));
        // What the app's checks rest on is never a session's to write (cards #76, #96, #104, #112).
        for k in ["test", "review", "repro", "smallChange", "smallOptOut", "autoTo", "ship", "column"] {
            assert!(!UPDATABLE.contains(&k), "{k}");
        }
        // Card #151: a card's subagents are Hugh's alone, not a session's or the Router's.
        assert!(!UPDATABLE.contains(&kanban_store::SUBAGENTS_KEY) && !CREATE_FIELDS.contains(&kanban_store::SUBAGENTS_KEY));
        assert_eq!(not_updatable(&json!({ "title": "T", "subagents": { "tester": false } }).as_object().cloned().unwrap()).map(String::as_str), Some(kanban_store::SUBAGENTS_KEY));
        assert!(UPDATABLE.contains(&"blockedBy"));
        assert!(!UPDATABLE.contains(&"waitedOn"), "the app writes waitedOn");
        let create = schemas().as_array().unwrap().iter().find(|t| t["name"] == "kanban_create_card").cloned().unwrap();
        assert!(create["inputSchema"]["properties"].get("stack").is_some());
    }

    #[test]
    fn update_card_takes_changes_outputs_but_not_saved_figures() {
        let ok = json!({ "changesOutputs": [{ "path": "C:\\x\\report.md", "label": "Report" }], "summary": "s" });
        assert_eq!(not_updatable(ok.as_object().unwrap()), None);
        let saved = json!({ "changesOutputs": null, "changesSaved": {} });
        assert_eq!(not_updatable(saved.as_object().unwrap()).map(String::as_str), Some("changesSaved"));
        let unknown = json!({ "noSuchField": 1 });
        assert_eq!(not_updatable(unknown.as_object().unwrap()).map(String::as_str), Some("noSuchField"));
        assert!(not_updatable(json!({ "column": "Done" }).as_object().unwrap()).is_some());
    }

    #[test]
    fn a_session_cant_set_the_evidence() {
        // The card-tester's shots (card #101) are the app's own field: only the tester's verdict
        // write adds a run, so kanban_update_card and `kb update` refuse "evidence".
        assert!(!UPDATABLE.contains(&kanban_store::EVIDENCE_KEY));
        assert!(!UPDATABLE.contains(&"evidence"));
        let update = schemas().as_array().unwrap().iter().find(|t| t["name"] == "kanban_update_card").cloned().unwrap();
        assert!(!update["description"].as_str().unwrap().contains("evidence"));
    }

    #[test]
    fn done_when_fields_are_updatable() {
        // Card #105. The prompts' guard reads the description: "If kanban_update_card lists doneWhen ...".
        assert!(UPDATABLE.contains(&"doneWhen") && UPDATABLE.contains(&"doneWhenChecks"));
        let upd = schemas().as_array().unwrap().iter().find(|t| t["name"] == "kanban_update_card").cloned().unwrap();
        let d = upd["description"].as_str().unwrap();
        assert!(d.contains("Allowed: title,") && d.contains(", doneWhen, doneWhenChecks"), "{d}");
        assert!(!kanban_store::looks_like_token(d));
    }

    /// Card #160: kanban_merge is a card session's tool, and asks for its own card's merge only.
    #[test]
    fn kanban_merge_is_listed_for_card_sessions() {
        let t = schemas().as_array().unwrap().iter().find(|t| t["name"] == "kanban_merge").cloned().expect("listed");
        let d = t["description"].as_str().unwrap();
        for must in ["Ship it", "merge lane", "End your turn", "uncommitted"] {
            assert!(d.contains(must), "{must}");
        }
        assert!(t["inputSchema"]["properties"].get("card").is_none(), "only its own card");
        assert!(CARD_SESSION_ONLY.contains(&"kanban_merge"));
        assert!(!UPDATABLE.contains(&kanban_store::MERGE_KEY), "the merge record is the app's");
    }

    #[test]
    fn t4_6_card_sessions_are_not_offered_the_ship_request() {
        let names: Vec<String> = schemas().as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        for t in CLI_ONLY {
            assert!(!names.iter().any(|n| n == t));
        }
    }
}
