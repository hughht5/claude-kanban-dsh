// The Claude Kanban DSH app: the board, a Claude Code session per card, and the app as the dispatcher.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod btw;
mod control;
mod dispatch;
mod launch;
mod needs;
mod notify;
mod place;
mod private;
mod sessions;
mod stack;
mod settings;
mod slash;
mod tester;
mod testing;
mod tools;
mod transcript;
mod tray;
mod update;

use kanban_store::{Card, Error, Patch, SessionRow, Store};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

pub struct Board(pub Mutex<Store>);

/// What a refused command sends back to the page.
#[derive(Serialize)]
struct CmdError {
    kind: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    current: Option<Value>,
}

impl CmdError {
    fn other(message: impl ToString) -> CmdError {
        CmdError { kind: "error", message: message.to_string(), current: None }
    }
}

type CmdResult<T> = Result<T, CmdError>;

/// The card as the page expects it: the artifact's document, plus id, version and the newest 20 log lines.
fn doc(store: &Store, card: &Card) -> Value {
    let mut log = store.log(&card.id).unwrap_or_default();
    log.truncate(20);
    let mut d = card.to_document(&log);
    d["id"] = json!(card.id);
    d["version"] = json!(card.version);
    d
}

fn refused(store: &Store, e: Error) -> CmdError {
    match e {
        Error::Conflict { ref id, .. } => {
            let current = store.get(id).ok().map(|c| doc(store, &c));
            CmdError { kind: "conflict", message: e.to_string(), current }
        }
        Error::Secret(_) => CmdError { kind: "secret", message: e.to_string(), current: None },
        other => CmdError::other(other),
    }
}

fn changed(app: &AppHandle) {
    let _ = app.emit("cards-changed", ());
}

#[tauri::command]
fn list_cards(board: State<Board>) -> CmdResult<Vec<Value>> {
    let store = board.0.lock().map_err(CmdError::other)?;
    let cards = store.list(true).map_err(CmdError::other)?;
    Ok(cards.iter().map(|c| doc(&store, c)).collect())
}

/// Hugh's Add card. `to` is his "Take it to" (card #76): Backlog (or none) leaves it there;
/// Planning starts it now; Human test or Deploy complete also sets it to go on by itself
/// (`autoTo`).
#[tauri::command]
fn create_card(app: AppHandle, board: State<Board>, id: String, mut fields: Patch, to: Option<String>) -> CmdResult<Value> {
    let to = to.map(|t| t.trim().to_string()).filter(|t| !t.is_empty() && t != "Backlog");
    if let Some(t) = &to {
        if t != "Planning" {
            if !kanban_store::AUTO_TARGETS.contains(&t.as_str()) {
                return Err(CmdError::other(format!("a new card can be taken to Backlog, Planning, Human test or Deploy complete, not {t}")));
            }
            fields.insert(kanban_store::AUTO_KEY.into(), json!(t));
        }
        fields.remove("column");
    }
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let mut card = store.create(Some(&id), fields, "Hugh (board)").map_err(|e| refused(&store, e))?;
    store.add_log(&card.id, "Added on the board", "Hugh (board)").map_err(CmdError::other)?;
    if let Some(t) = &to {
        let why = if t == "Planning" { "he added it to be planned now".to_string() } else { format!("he added it to go on by itself to {t}") };
        // The card is made either way; a start that fails leaves it in Backlog, saying why.
        match dispatch::start_new(&app, &mut store, &card, "Planning", &why) {
            Ok(c) => card = c,
            Err(e) => {
                let _ = store.add_log(&card.id, &format!("Made, but couldn't start it in Planning: {e}. It waits in Backlog."), "the app");
            }
        }
    }
    let out = doc(&store, &card);
    drop(store);
    changed(&app);
    Ok(out)
}

#[tauri::command]
fn update_card(app: AppHandle, board: State<Board>, id: String, version: i64, patch: Patch, by: Option<String>) -> CmdResult<Value> {
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let by = by.unwrap_or_else(|| "Hugh (board)".into());
    let card = store.update(&id, version, &patch, &by).map_err(|e| refused(&store, e))?;
    let out = doc(&store, &card);
    drop(store);
    changed(&app);
    let keys: Vec<String> = patch.keys().cloned().collect();
    let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    dispatch::after_edit(&app, &id, &refs);
    Ok(out)
}

#[tauri::command]
fn move_card(app: AppHandle, board: State<Board>, id: String, version: i64, column: String, extra: Patch) -> CmdResult<Value> {
    let store = board.0.lock().map_err(CmdError::other)?;
    hugh_move(&app, store, id, version, &column, extra)
}

/// Hugh's move of card `id` (his drag, or his drop that ships a card straight after another,
/// card #89): the stack rules, the ship queue, and the session told or started once the board
/// is let go.
fn hugh_move(app: &AppHandle, mut store: std::sync::MutexGuard<'_, Store>, id: String, version: i64, column: &str, extra: Patch) -> CmdResult<Value> {
    let app = app.clone();
    let from = store.get(&id).map(|c| c.column).unwrap_or_default();
    let m = store.move_by_hugh(&id, version, column, extra).map_err(|e| refused(&store, e))?;
    dispatch::after_stack_move(&app, &mut store, &id, "Hugh (board)");
    // The card as it is after the stack rules (they never move the card itself, but re-read).
    let mut card = store.get(&id).unwrap_or(m.card.clone());
    // A ship behind another of the same repo waits its turn (card #79): queued here, so the
    // page gets the card's new version, and after_move leaves its session alone.
    let mut wait_for = None;
    if m.ship {
        if let Ok(Some(b)) = store.ship_blocker(&card) {
            if let Ok(c) = store.ship_wait(&id, &b, &from) {
                wait_for = Some(b.number);
                card = c;
            }
        }
    }
    let out = json!({ "card": doc(&store, &card), "version": card.version, "ship": m.ship, "plan": m.plan, "shipWaitFor": wait_for });
    // A stack's child has no session of its own: its stack's session owns it, and hears of the move.
    let lead = store.stack_lead(&card).ok().flatten();
    if let Some(l) = &lead {
        if dispatch::has_session(&app, &store, &l.id) {
            let text = format!("Hugh moved #{} \"{}\" in this stack from {from} to {}.", card.number, card.title(), card.column);
            let _ = store.queue(&l.id, "message", &text);
        }
    }
    drop(store);
    changed(&app);
    if lead.is_none() {
        // Starting a session can take a moment, so it happens off the command's thread.
        let a = app.clone();
        std::thread::spawn(move || dispatch::after_move(&a, &id, &from));
    }
    Ok(out)
}

const HUGH: &str = "Hugh (board)";

/// Puts card `child` into the stack led by `lead`. Returns the lead.
#[tauri::command]
fn stack_add(app: AppHandle, board: State<Board>, lead: i64, child: i64) -> CmdResult<Value> {
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let l = store.stack_add(lead, child, HUGH).map_err(|e| refused(&store, e))?;
    let out = doc(&store, &l);
    drop(store);
    changed(&app);
    Ok(out)
}

/// Takes card `child` out of `lead`'s stack: an ordinary card again. Returns the lead.
#[tauri::command]
fn stack_remove(app: AppHandle, board: State<Board>, lead: i64, child: i64) -> CmdResult<Value> {
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let l = store.stack_remove(lead, child, HUGH).map_err(|e| refused(&store, e))?;
    let out = doc(&store, &l);
    drop(store);
    changed(&app);
    Ok(out)
}

/// Detach: the stack's session stops building `child`, which becomes a card of its own and goes
/// to Planning, where its own session starts. A child already shipping or shipped just leaves the
/// stack where it is. Shared by the board's Detach button and the kanban_stack_detach tool.
/// Returns the card and, when it moved, the column it came from (the caller then runs
/// `dispatch::after_move` off the lock, which starts its session).
pub(crate) fn stack_detach_op(app: &AppHandle, store: &mut Store, lead: i64, child: i64, by: &str) -> Result<(Card, Option<String>), String> {
    let l = store.get_by_number(lead).map_err(|e| e.to_string())?;
    store.stack_remove(lead, child, by).map_err(|e| e.to_string())?;
    let c = store.get_by_number(child).map_err(|e| e.to_string())?;
    let branch = l.fields.get("branch").and_then(Value::as_str).unwrap_or("");
    let on = if branch.is_empty() { String::new() } else { format!(" on branch {branch}") };
    store
        .add_log(&c.id, &format!("Detached from stack #{lead} by {by}, to be built on its own. Anything already done for it is{on}; stack #{lead}'s session will log what."), by)
        .map_err(|e| e.to_string())?;
    let _ = store.stack_recheck(lead, &format!("#{child} was detached"), by);
    let text = format!(
        "#{child} \"{}\" was detached from this stack ({by}): it is now its own card with its own session, so stop working on it here. If you already changed anything for it{on}, log what with kanban_add_log on card {child} so its new session knows, and tell Hugh whether it should come out of this branch.",
        c.title()
    );
    if dispatch::has_session(app, store, &l.id) {
        let _ = store.queue(&l.id, "message", &dispatch::dry(&l, text));
    } else {
        let _ = store.add_log(&l.id, &format!("#{child} was detached from this stack by {by}"), by);
    }
    let from = c.column.clone();
    if ["Backlog", "Blocked", "In progress", "Human test"].contains(&from.as_str()) {
        let moved = store.move_by_hugh(&c.id, c.version, "Planning", Patch::new()).map_err(|e| e.to_string())?.card;
        return Ok((moved, Some(from)));
    }
    Ok((c, None))
}

/// Join: `child` goes into `lead`'s stack (making `lead` a stack if it isn't one). The stack's
/// session is told to plan and build it if it isn't built yet, or, if it is (Human test or later,
/// or it has a PR), to merge its branch into the stack's; for a stack in Ship it, before its
/// merge, so they ship as one (card #89). Its own session, if any, stands down. Shared by
/// Hugh's drop on a card or stack (`hugh`: the Ship it joins and a stack into a stack, which
/// sessions can't do) and by the kanban_stack_join tool.
pub(crate) fn stack_join_op(app: &AppHandle, store: &mut Store, lead: i64, child: i64, by: &str, hugh: bool) -> Result<Card, String> {
    let l = store.get_by_number(lead).map_err(|e| e.to_string())?;
    let c = store.get_by_number(child).map_err(|e| e.to_string())?;
    let kids = if c.is_lead() { store.stack_children(&c).unwrap_or_default() } else { Vec::new() };
    let (_, j) = store.stack_join(lead, child, by, hugh).map_err(|e| e.to_string())?;
    let text = dispatch::join_message(&l, &c, &kids, &j, by);
    let _ = store.add_log(&c.id, &format!("Joined stack #{lead} ({by}); {}", if dispatch::built(&c) { "already built: its branch is to be merged into the stack's" } else { "the stack's session plans and builds it" }), by);
    let _ = store.stack_recheck(lead, &format!("#{child} joined"), by);
    let l = store.get_by_number(lead).map_err(|e| e.to_string())?;
    send_to_session(app, store, &l.id, &dispatch::dry(&l, text))?;
    if dispatch::has_session(app, store, &c.id) {
        let _ = store.queue(&c.id, "message", &dispatch::stand_down_message(&c, lead, &j));
    }
    let _ = app.emit("session-changed", l.id.clone());
    store.get_by_number(lead).map_err(|e| e.to_string())
}

/// Hugh's drop of `c` on `l`, a card in Ship it whose ship has started (`why`): that push is
/// under way, so `c` can't join it, and goes into Ship it on its own to ship straight after
/// (card #89). `first`: `l` goes first in the ship queue (`ship_goes_first`), so `c` really
/// waits for it. Err when it can't do that; Ok(false) when `c` is in Ship it already.
pub(crate) fn ship_after_check(l: &Card, c: &Card, why: &str, first: bool) -> Result<bool, String> {
    let (ln, cn) = (l.number, c.number);
    if ln == cn {
        return Err("a card can't be in its own stack".into());
    }
    if c.archived {
        return Err(format!("#{cn} is archived; restore it first"));
    }
    if let Some(n) = c.stack_of() {
        return Err(format!("#{cn} is in stack #{n}: it ships with that stack"));
    }
    if !first {
        return Err(format!(
            "#{ln} can't take #{cn} into its ship, or ship it next ({why}). Move #{cn} to Ship it yourself if it should ship on its own"
        ));
    }
    let (lr, cr) = (kanban_store::ship_lane(l), kanban_store::ship_lane(c));
    if lr.is_empty() || cr.is_empty() {
        return Err(format!("#{ln}'s ship has started ({why}), and #{cn} can only wait its turn behind it if both cards name their repo"));
    }
    if lr != cr {
        return Err(format!("#{cn} and #{ln} are for different repos, so they can't ship as one"));
    }
    if c.column == "Ship it" {
        // Waiting its turn, it already ships after; any other ship of its own doesn't wait.
        return match kanban_store::ship_started(c) {
            None => Ok(false),
            Some(w) => Err(format!("#{cn} is in Ship it already, and {w}: it can't join #{ln}'s ship or wait behind it")),
        };
    }
    if kanban_store::STACK_READY.contains(&c.column.as_str()) && c.column != "Human test" {
        return Err(format!("#{cn} has shipped already (it's in {}), so it can't ship after #{ln}", c.column));
    }
    if c.column != "Human test" {
        return Err(format!(
            "#{ln}'s ship has started ({why}), so #{cn} can't join that push, and #{cn} is in {}, not Human test, so it can't ship straight after it either: build it first",
            c.column
        ));
    }
    Ok(true)
}

/// Hugh's Detach on a stack's child (the board's button).
#[tauri::command]
fn stack_detach(app: AppHandle, board: State<Board>, lead: i64, child: i64) -> CmdResult<Value> {
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let (c, from) = stack_detach_op(&app, &mut store, lead, child, HUGH).map_err(CmdError::other)?;
    let out = doc(&store, &c);
    drop(store);
    changed(&app);
    if let Some(from) = from {
        let (a, id) = (app.clone(), c.id.clone());
        std::thread::spawn(move || dispatch::after_move(&a, &id, &from));
    }
    Ok(out)
}

/// What Hugh's drop of `child` on `lead` would do, asked before the board shows its dialog
/// (card #89): `join`, `after` (the lead's ship has started: it ships straight after),
/// `already` (it is in Ship it already, behind it), or `refuse` with why. The same checks as
/// `stack_join`, with nothing written.
#[tauri::command]
fn stack_join_preview(board: State<Board>, lead: i64, child: i64) -> CmdResult<Value> {
    let store = board.0.lock().map_err(CmdError::other)?;
    let l = store.get_by_number(lead).map_err(CmdError::other)?;
    let c = store.get_by_number(child).map_err(CmdError::other)?;
    Ok(match kanban_store::ship_started(&l) {
        Some(why) => match ship_after_check(&l, &c, why, kanban_store::ship_goes_first(&l)) {
            Ok(true) => json!({ "kind": "after", "why": why }),
            Ok(false) => json!({ "kind": "already", "why": why }),
            Err(e) => json!({ "kind": "refuse", "why": e }),
        },
        None => match store.stack_join_check(lead, child, true) {
            Ok(()) => json!({ "kind": "join" }),
            Err(e) => json!({ "kind": "refuse", "why": e.to_string() }),
        },
    })
}

/// Hugh dropped card `child` on card or stack `lead` and confirmed. On a card whose ship has
/// started, the card ships straight after it instead (card #89): `shipAfter` in the reply.
#[tauri::command]
fn stack_join(app: AppHandle, board: State<Board>, lead: i64, child: i64) -> CmdResult<Value> {
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let l = store.get_by_number(lead).map_err(CmdError::other)?;
    if let Some(why) = kanban_store::ship_started(&l) {
        let c = store.get_by_number(child).map_err(CmdError::other)?;
        if !ship_after_check(&l, &c, why, kanban_store::ship_goes_first(&l)).map_err(CmdError::other)? {
            return Ok(json!({ "joined": false, "shipAfter": lead, "already": true, "why": why }));
        }
        store
            .add_log(&c.id, &format!("Hugh dropped it on #{lead}, whose ship has started ({why}): it ships straight after, on its own"), HUGH)
            .map_err(CmdError::other)?;
        let c = store.get(&c.id).map_err(CmdError::other)?;
        let mut out = hugh_move(&app, store, c.id.clone(), c.version, "Ship it", Patch::new())?;
        out["joined"] = json!(false);
        out["shipAfter"] = json!(lead);
        out["why"] = json!(why);
        return Ok(out);
    }
    let l = stack_join_op(&app, &mut store, lead, child, HUGH, true).map_err(CmdError::other)?;
    let out = json!({ "joined": true, "lead": doc(&store, &l) });
    drop(store);
    changed(&app);
    Ok(out)
}

/// Hugh's Passed on a stack's child (or taking it back). The stack's session hears of it.
#[tauri::command]
fn set_passed(app: AppHandle, board: State<Board>, id: String, on: bool) -> CmdResult<Value> {
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let card = store.set_passed(&id, on, HUGH).map_err(|e| refused(&store, e))?;
    if let Some(l) = store.stack_lead(&card).ok().flatten() {
        if dispatch::has_session(&app, &store, &l.id) {
            let text = if on {
                format!("FYI: Hugh passed #{} \"{}\" in this stack (it stays in Human test until the stack ships).", card.number, card.title())
            } else {
                format!("FYI: Hugh took back Passed on #{} \"{}\" in this stack.", card.number, card.title())
            };
            let _ = store.queue(&l.id, "message", &text);
        }
    }
    let out = doc(&store, &card);
    drop(store);
    changed(&app);
    Ok(out)
}

/// The session a stack child's message goes to: its lead's, or the card's own if it isn't in a stack.
fn stack_session_card(store: &Store, card: &Card) -> String {
    store.stack_lead(card).ok().flatten().map(|l| l.id).unwrap_or_else(|| card.id.clone())
}

/// Hugh's "Found a problem" on a stack's child: it goes back to In progress (Passed cleared),
/// the stack rules follow, and the note goes to the stack's session (started if need be).
#[tauri::command]
fn stack_problem(app: AppHandle, board: State<Board>, id: String, note: String) -> CmdResult<Value> {
    let note = note.trim().to_string();
    if note.is_empty() {
        return Err(CmdError::other("say what the problem is"));
    }
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let card = store.get(&id).map_err(CmdError::other)?;
    let from = card.column.clone();
    let mut patch = Patch::new();
    if from != "In progress" {
        patch.insert("column".into(), json!("In progress"));
    }
    let card = if patch.is_empty() { card } else { store.update(&id, card.version, &patch, HUGH).map_err(|e| refused(&store, e))? };
    store.add_log(&id, &format!("Hugh found a problem: {note}"), HUGH).map_err(|e| refused(&store, e))?;
    if from != "In progress" {
        store.add_log(&id, &format!("Hugh moved it from {from} to In progress"), HUGH).map_err(CmdError::other)?;
    }
    let card = store.set_passed(&card.id, false, HUGH).map_err(CmdError::other)?;
    dispatch::after_stack_move(&app, &mut store, &id, HUGH);
    let to = stack_session_card(&store, &card);
    let text = format!(
        "Hugh tested #{n} \"{t}\" in this stack and found a problem: {note}\n#{n} is back in In progress. Fix it, then move it back to Human test.",
        n = card.number,
        t = card.title()
    );
    // A dry-run stack (the test board's copies) only says what it would do.
    let text = match store.get(&to) { Ok(l) => dispatch::dry(&l, text), Err(_) => text };
    let sent = send_to_session(&app, &mut store, &to, &text);
    if let Err(e) = &sent {
        let _ = store.add_log(&to, &format!("Couldn't send Hugh's problem note on #{} to the session: {e}", card.number), "the app");
    }
    let card = store.get(&id).map_err(CmdError::other)?;
    let out = doc(&store, &card);
    drop(store);
    changed(&app);
    let _ = app.emit("session-changed", to);
    sent.map_err(CmdError::other)?;
    Ok(out)
}

/// Hugh's answer to the question on a stack's child (its blockedReason): logged on the child,
/// the question and its choices cleared, and the answer sent to the stack's session.
#[tauri::command]
fn stack_answer(app: AppHandle, board: State<Board>, id: String, answer: String) -> CmdResult<Value> {
    let answer = answer.trim().to_string();
    if answer.is_empty() {
        return Err(CmdError::other("there's no answer to send"));
    }
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let card = store.get(&id).map_err(CmdError::other)?;
    let question = card.fields.get("blockedReason").and_then(Value::as_str).unwrap_or("").to_string();
    store.add_log(&id, &format!("Hugh answered: {answer}"), HUGH).map_err(|e| refused(&store, e))?;
    let mut patch = Patch::new();
    patch.insert("blockedReason".into(), Value::Null);
    patch.insert("choices".into(), Value::Null);
    let card = store.update(&id, card.version, &patch, HUGH).map_err(|e| refused(&store, e))?;
    let to = stack_session_card(&store, &card);
    let asked = if question.is_empty() { String::new() } else { format!(" ({question})") };
    let text = format!("Hugh answered the question on #{} \"{}\"{asked}: {answer}", card.number, card.title());
    // A dry-run stack (the test board's copies) only says what it would do.
    let text = match store.get(&to) { Ok(l) => dispatch::dry(&l, text), Err(_) => text };
    let sent = send_to_session(&app, &mut store, &to, &text);
    if let Err(e) = &sent {
        let _ = store.add_log(&to, &format!("Couldn't send Hugh's answer on #{} to the session: {e}", card.number), "the app");
    }
    let out = doc(&store, &card);
    drop(store);
    changed(&app);
    let _ = app.emit("session-changed", to);
    sent.map_err(CmdError::other)?;
    Ok(out)
}

#[tauri::command]
fn archive_card(app: AppHandle, board: State<Board>, id: String, version: i64, on: bool) -> CmdResult<Value> {
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let from = store.get(&id).map(|c| c.column).unwrap_or_default();
    let card = store.archive(&id, version, on, "Hugh (board)").map_err(|e| refused(&store, e))?;
    let out = doc(&store, &card);
    drop(store);
    // A Done card restored goes back to the column it came from (card #83): treat it as a move
    // there, so its session hears about it or the column starts one, as Hugh's move would.
    if !on && card.column != from {
        dispatch::after_move(&app, &card.id, &from);
    }
    changed(&app);
    Ok(out)
}

#[tauri::command]
fn open_external(url: String) -> CmdResult<()> {
    launch::open_external(&url).map_err(CmdError::other)
}

#[tauri::command]
fn open_login(board: State<Board>, url: String, browser: String) -> CmdResult<()> {
    let profiles = board.0.lock().map_err(CmdError::other)?.launch_profiles().map_err(CmdError::other)?;
    launch::open_login(&url, &browser, &profiles).map_err(CmdError::other)
}

#[tauri::command]
async fn stack_up(url: String) -> bool {
    tauri::async_runtime::spawn_blocking(move || launch::stack_up(&url))
        .await
        .unwrap_or(true)
}

include!("session_cmds.rs");
include!("live_cmds.rs");

/// The window starts hidden (`visible: false`) and is shown here. With `--background` it is
/// shown behind other windows without taking focus, so an update or a test launch never pulls
/// Hugh out of what he has in front (a full-screen game, for one). A later launch of the app
/// brings it forward as usual.
fn show_window(app: &AppHandle) {
    let background = std::env::args().any(|a| a == "--background");
    // A moment after start-up, once the window framework has applied its own settings, put the
    // window where Hugh last left it (the first time, filling the screen) and show it. With
    // --background it goes behind everything without activating. Not ShowWindow: Windows swaps
    // the first ShowWindow of a process for the show mode its launcher asked for, which
    // activates the window. SetWindowPos isn't swapped.
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(if background { 800 } else { 150 }));
        let a2 = app.clone();
        let _ = app.run_on_main_thread(move || {
            let Some(w) = a2.get_webview_window("main") else { return };
            place::place(&w, place::initial(), background);
            if !background {
                let _ = w.set_focus();
            }
        });
    });
}

/// Where the board is, for the status line, so Hugh can tell which board a window shows.
#[tauri::command]
fn board_location() -> String {
    board_path().display().to_string()
}

/// "live" or "test", fixed when the app was built (`KANBAN_CHANNEL=test`). The page shows a
/// warning bar across the top in the test build.
#[tauri::command]
fn app_channel() -> &'static str {
    kanban_store::channel()
}

fn board_path() -> PathBuf {
    kanban_store::data_dir().join("board.db")
}

fn main() {
    // A board asked for in import-from.txt (a renamed app's old data folder) comes in first.
    match kanban_store::import_if_asked(&kanban_store::data_dir(), &private::home()) {
        Ok(Some(from)) => eprintln!("imported the board from {}", from.display()),
        Ok(None) => {}
        // Never go on to an empty board in its place: the import would then never run.
        Err(e) => panic!("import the board asked for in {}: {e}", kanban_store::IMPORT_REQUEST),
    }
    let mut store = Store::open(board_path()).expect("open the board database");
    // Sessions still marked running were ended by the Job Object when the app last stopped:
    // they are marked orphaned. Those mid-turn at the last stop (a Quit or a crash) resume below.
    let mid_turn = store.begin_run().unwrap_or_else(|e| {
        eprintln!("restart recovery: {e}");
        vec![]
    });
    if let Err(e) = store.backup_if_none_today(kanban_store::KEEP_BACKUPS) {
        eprintln!("backup at start: {e}");
    }
    private::set(&store.page_setting(kanban_store::PRIVATE_DIR_KEY).unwrap_or_default());
    let sessions = Arc::new(sessions::Sessions::new().expect("create the job object"));
    let app = tauri::Builder::default()
        // One copy only: a second launch brings the running window forward and exits.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if args.iter().any(|a| a == "--background") {
                return;
            }
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .manage(Board(Mutex::new(store)))
        .manage(sessions)
        .manage(update::Updates::default())
        .setup(move |app| {
            show_window(app.handle());
            let dir = board_path().parent().map(PathBuf::from).unwrap_or_default();
            let info = api::start(app.handle().clone(), dir).map_err(|e| eprintln!("local API: {e}")).ok();
            app.manage(Api(info));
            dispatch::run(app.handle().clone());
            let handle = app.handle().clone();
            std::thread::spawn(move || dispatch::resume_after_restart(&handle, &mid_turn));
            nightly_backups(app.handle().clone());
            update::watch(app.handle().clone());
            notify::setup(app);
            tray::setup(app)?;
            // tauri.test.conf.json sets this too; this covers a test build made without it.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_title(kanban_store::app_name());
            }
            Ok(())
        })
        .on_window_event(|w, e| {
            place::on_event(w, e);
            tray::on_window_event(w, e);
        })
        .invoke_handler(tauri::generate_handler![
            list_cards,
            create_card,
            update_card,
            move_card,
            archive_card,
            stack_add,
            stack_remove,
            stack_detach,
            stack_join,
            stack_join_preview,
            set_passed,
            stack_problem,
            stack_answer,
            open_external,
            open_login,
            stack_up,
            session_info,
            board_location,
            app_channel,
            session_states,
            session_testing,
            session_start,
            session_input,
            session_resize,
            session_snapshot,
            session_screen,
            session_btw,
            slash_commands,
            session_send,
            session_send_now,
            session_dismiss_untaken,
            session_answer,
            session_stop,
            transcript_read,
            subagent_list,
            notify::panel_opened,
            needs::needs_you,
            needs::mark_card_read,
            needs::usage_summary,
            stack::stack_health,
            update::update_status,
            update::update_later,
            update::update_restart,
            set_session_model_effort,
            get_session_cap,
            set_session_cap,
            session_queue,
            session_unqueue,
            resume_offer,
            resume_offer_answer,
            session_status,
            attachment_save,
            image_read,
            settings::list_columns,
            settings::update_column,
            settings::reset_column,
            settings::list_templates,
            settings::save_template,
            settings::archive_template,
            settings::get_setting,
            settings::set_setting,
            settings::list_repos
        ])
        .build(tauri::generate_context!())
        .expect("build the app");
    app.run(|app, event| match event {
        // An exit asked for in code (the tray's Quit, app.exit) or the app's last moment:
        // every live session is suspended, recording whether it was mid-turn. The last
        // window closing (code None) is left to the tray, which may keep the app running.
        RunEvent::ExitRequested { code: Some(_), .. } | RunEvent::Exit => {
            app.state::<Arc<sessions::Sessions>>().stop_all(app);
        }
        _ => {}
    });
}

/// A backup of the board every 24 hours while the app runs, keeping the newest 30.
fn nightly_backups(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(24 * 60 * 60));
        let board = app.state::<Board>();
        let Ok(store) = board.0.lock() else { continue };
        if let Err(e) = store.backup_and_prune("daily", kanban_store::KEEP_BACKUPS) {
            eprintln!("daily backup: {e}");
        }
    });
}

#[cfg(test)]
mod join_tests {
    use super::*;

    fn card(n: i64, fields: Value, column: &str) -> Card {
        Card { id: format!("card-{n}"), number: n, column: column.into(), order: None, order_col: None, archived: false, fields: fields.as_object().cloned().unwrap(), version: 1 }
    }

    /// Hugh's drop on a card whose ship has started (card #89): ships straight after, or says why not.
    #[test]
    fn a_drop_on_a_started_ship_ships_straight_after() {
        let l = card(73, json!({ "title": "L", "repo": "acme/web" }), "Ship it");
        let why = "it is shipping now";
        let repo = json!({ "title": "x", "repo": "ACME/web" });
        assert_eq!(ship_after_check(&l, &card(81, repo.clone(), "Human test"), why, true), Ok(true));
        assert!(ship_after_check(&l, &card(81, repo.clone(), "Ship it"), why, true).is_err(), "in Ship it with no ship waiting");
        let e = ship_after_check(&l, &card(81, repo.clone(), "In progress"), why, true).unwrap_err();
        assert!(e.contains("#73's ship has started (it is shipping now)") && e.contains("build it first"), "{e}");
        assert!(ship_after_check(&l, &card(81, json!({ "title": "x", "repo": "acme/kanban" }), "Human test"), why, true).unwrap_err().contains("different repos"));
        assert!(ship_after_check(&l, &card(81, json!({ "title": "x", "stackOf": 9 }), "Human test"), why, true).unwrap_err().contains("in stack #9"));
        let mut gone = card(81, repo.clone(), "Human test");
        gone.archived = true;
        assert!(ship_after_check(&l, &gone, why, true).unwrap_err().contains("archived"));
        assert!(ship_after_check(&l, &card(73, repo.clone(), "Ship it"), why, true).is_err());
        // With no repo the card can't queue behind it, so it would ship at the same time.
        let e = ship_after_check(&l, &card(81, json!({ "title": "x" }), "Human test"), why, true).unwrap_err();
        assert!(e.contains("both cards name their repo"), "{e}");
        // A card in Ship it whose own ship isn't waiting won't ship after it by itself.
        let stuck = json!({ "title": "x", "repo": "ACME/web", "ship": { "state": "needs-hugh" } });
        let e = ship_after_check(&l, &card(81, stuck, "Ship it"), why, true).unwrap_err();
        assert!(e.contains("#81 is in Ship it already, and its ship is waiting for Hugh"), "{e}");
        let queued = json!({ "title": "x", "repo": "ACME/web", "ship": { "state": "queued" } });
        assert_eq!(ship_after_check(&l, &card(81, queued, "Ship it"), why, true), Ok(false));
        // A shipped card isn't told to build it first.
        let e = ship_after_check(&l, &card(81, json!({ "title": "x", "repo": "ACME/web" }), "Deploy complete"), why, true).unwrap_err();
        assert_eq!(e, "#81 has shipped already (it's in Deploy complete), so it can't ship after #73");
        // A ship waiting for Hugh frees the repo: nothing would make the card wait for it.
        let e = ship_after_check(&l, &card(81, repo, "Human test"), "its ship is waiting for Hugh", false).unwrap_err();
        assert!(e.starts_with("#73 can't take #81 into its ship, or ship it next (its ship is waiting for Hugh)"), "{e}");
    }
}
