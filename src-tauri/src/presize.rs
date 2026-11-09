//! Sizing a card before its session starts (card #159). Hugh's setting
//! `sessions.size_before_start` (on by default): just before a card's session starts in Planning
//! or In progress, the app sizes the card itself with one quick `claude -p` call that runs the
//! card-router's own rules (`router::RULES`), records the route on the card, and only then starts
//! the session, on the route's model and effort. The session is never restarted onto its route.
//! Off: the card's session routes it with its card-router subagent, as before (card #98).
//!
//! - Only for a move (a column's auto-spawn), the board Router's start and Hugh's Start a new
//!   session (`blocker`); a card with no route, not a stack's child, with its card-router on, and
//!   no live session.
//! - While it sizes, the card carries `sizing` (`{at, mode, why}`) and every start of it waits
//!   (`Begun::Sizing`), as a start waits under the cap: its messages are typed once it starts.
//! - A call that fails never stops the start: the session starts with no route, and its planning
//!   prompt routes it as before.
//! - After an app restart, a card still marked `sizing` is sized again and started
//!   (`resume_at_start`).

use crate::router;
use crate::sessions::{self, Sessions};
use crate::Board;
use kanban_store::{Card, Patch, Store};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// The card field marking a sizing under way: `{at, mode, why}`.
pub const SIZING_KEY: &str = "sizing";
/// Who the sizing's log lines are by.
pub const BY: &str = "the app (sizing before the session starts)";
/// The route's `agentId` for a route from the app's own call.
pub const AGENT_ID: &str = "before-start";
/// How long the call may take before it is given up.
pub const TIMEOUT: Duration = Duration::from_secs(180);
/// The most turns the call takes (each a model reply, most with a tool call).
pub const MAX_TURNS: &str = "20";
/// The only tools the call has: it reads, it changes nothing.
pub const TOOLS: &str = "Read,Grep,Glob";
/// The effort it runs at, as the card-router subagent does.
pub const EFFORT: &str = "low";
/// The most of one card field written into the brief, and of all of them.
const FIELD_MAX: usize = 6_000;
const CARD_MAX: usize = 20_000;

/// Whether a start of this kind (`mode`, `why`, as `begin_session` takes them) is one the card
/// would be sized for: a move into the column (its auto-spawn), the board Router's start, and
/// Hugh's Start a new session. Not a resume from a chat message, an app restart, a route switch
/// or a start leaving the cap's queue.
pub fn counts(mode: &str, why: &str) -> bool {
    matches!(why, "move" | "router") || (why == "hugh" && mode == "new")
}

/// Why the app doesn't size `card` before this start, or None when it does. `live`: a session of
/// the card runs now.
pub fn blocker(store: &Store, card: &Card, mode: &str, why: &str, live: bool) -> Option<&'static str> {
    if !store.size_first() {
        return Some("Hugh's setting to size cards before their session starts is off");
    }
    if !counts(mode, why) {
        return Some("this kind of start isn't sized first");
    }
    if card.archived {
        return Some("it is archived");
    }
    if !router::ROUTED_COLUMNS.contains(&card.column.as_str()) {
        return Some("it isn't in Planning or In progress");
    }
    if router::route_of(card).is_some() {
        return Some("it already has a route");
    }
    if card.stack_of().is_some() {
        return Some("it is a stack's child, which its lead plans");
    }
    if !store.subagent_on(card, "router") {
        return Some("its card-router is off");
    }
    if live {
        return Some("its session is already running");
    }
    None
}

/// The card's sizing mark, if it has one.
pub fn sizing_of(card: &Card) -> Option<&Value> {
    card.fields.get(SIZING_KEY).filter(|v| v.is_object())
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max).collect();
    t.push_str(" [cut]");
    t
}

/// The card as the brief gives it: one block per field it has, in reading order.
pub fn card_text(card: &Card) -> String {
    let f = |k: &str| card.fields.get(k).filter(|v| !v.is_null());
    let text = |v: &Value| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
    let mut out = vec![format!("Number: #{}", card.number), format!("Column: {}", card.column)];
    for (k, label) in [("title", "Title"), ("repo", "Repo"), ("labels", "Labels"), ("issue", "Issue"), ("summary", "Summary"), ("nextStep", "Next step"), ("doneWhen", "Done when")] {
        if let Some(v) = f(k) {
            out.push(format!("{label}: {}", clip(&text(v), FIELD_MAX)));
        }
    }
    if let Some(checks) = f("doneWhenChecks").and_then(Value::as_array) {
        let lines: Vec<String> = checks.iter().filter_map(|c| c.as_str().or_else(|| c.get("text").and_then(Value::as_str))).map(|c| format!("- {c}")).collect();
        if !lines.is_empty() {
            out.push(format!("Acceptance checks:\n{}", clip(&lines.join("\n"), FIELD_MAX)));
        }
    }
    if let Some(v) = f("links") {
        out.push(format!("Links: {}", clip(&text(v), FIELD_MAX)));
    }
    if card.is_lead() {
        out.push("Stack: it leads a stack of cards.".into());
    }
    kanban_store::mask_secrets(&clip(&out.join("\n\n"), CARD_MAX))
}

/// The brief for the call: the card-router's own rules with the card written in, since the call
/// has no board tools. `checkout`: the folder it runs in, or None when the card's repo has no
/// folder on this machine.
pub fn brief(card: &Card, checkout: Option<&str>) -> String {
    let place = match checkout {
        Some(p) => format!("The checkout is your working folder, {p}."),
        None => "There is no checkout of its repo on this machine: size the card from its text alone.".to_string(),
    };
    format!(
        "You are card-router. You size one card on Hugh's Claude Kanban DSH board before its session starts. Work fast. This is a quick look, not a plan.\n\n1. The card is below, between the CARD START and CARD END lines. You have no board tools. {place}\n{}\n\nCARD START\n{}\nCARD END\n",
        router::RULES,
        card_text(card)
    )
}

/// The call's arguments: print mode with the brief on stdin, JSON out, read-only tools, no MCP
/// servers, no hooks, nothing saved.
pub fn args(model: &str) -> Vec<String> {
    [
        "-p",
        "--output-format",
        "json",
        "--model",
        model,
        "--effort",
        EFFORT,
        "--tools",
        TOOLS,
        "--strict-mcp-config",
        "--settings",
        r#"{"disableAllHooks":true}"#,
        "--no-session-persistence",
        "--max-turns",
        MAX_TURNS,
        "--permission-mode",
        "default",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// The reply's text, from what `--output-format json` prints: one `result` object. Err with why
/// for an error result or anything else.
pub fn read_output(out: &str) -> Result<String, String> {
    let v = out
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
        .find(|v| v.get("type").and_then(Value::as_str) == Some("result"))
        .or_else(|| serde_json::from_str::<Value>(out.trim()).ok().filter(|v| v.get("type").and_then(Value::as_str) == Some("result")))
        .ok_or_else(|| {
            let first: String = out.trim().chars().take(200).collect();
            if first.is_empty() { "it printed nothing".to_string() } else { format!("it printed no result: {first}") }
        })?;
    let text = v.get("result").and_then(Value::as_str).unwrap_or("").to_string();
    let subtype = v.get("subtype").and_then(Value::as_str).unwrap_or("?");
    if v.get("is_error").and_then(Value::as_bool) == Some(true) || subtype != "success" {
        let why: String = kanban_store::mask_secrets(&text).chars().take(200).collect();
        return Err(format!("it ended with {subtype}{}", if why.is_empty() { String::new() } else { format!(": {why}") }));
    }
    Ok(text)
}

/// The message a session in Planning gets after the app sized its card.
pub fn note(card: &Card) -> String {
    let r = router::route_of(card);
    let get = |k: &str| r.and_then(|r| r.get(k)).and_then(Value::as_str).unwrap_or("");
    let what = [get("model"), get("effort")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    format!(
        "The app sized card #{n} before this session started (Hugh's setting, card #159): routed {size}{what}{why}. It is on the card as its route, and this session already runs on its model and effort. Don't run the card-router: go on from the planning prompt's *Route it first* step 3 (small: keep the plan short; split: create the child cards it names).",
        n = card.number,
        size = get("size"),
        what = if what.is_empty() { String::new() } else { format!(", {what}") },
        why = if get("reason").is_empty() { String::new() } else { format!(": {}", get("reason")) },
    )
}

/// Marks the card as being sized (one write).
pub fn mark(store: &mut Store, card: &Card, mode: &str, why: &str) -> Result<Card, String> {
    let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let mut patch = Patch::new();
    patch.insert(SIZING_KEY.into(), json!({ "at": at, "mode": mode, "why": why }));
    let fresh = store.get(&card.id).map_err(|e| e.to_string())?;
    store.update(&card.id, fresh.version, &patch, BY).map_err(|e| e.to_string())
}

/// Clears the card's sizing mark, if it has one.
pub fn clear(store: &mut Store, card_id: &str) {
    let Ok(card) = store.get(card_id) else { return };
    if sizing_of(&card).is_none() {
        return;
    }
    let mut patch = Patch::new();
    patch.insert(SIZING_KEY.into(), Value::Null);
    let _ = store.update(card_id, card.version, &patch, BY);
}

/// Records the call's reply on the card (`router::record_with`, the route marked `before`,
/// `sizing` cleared in the same write). Err with why when nothing was recorded; the mark is then
/// cleared too.
pub fn record(store: &mut Store, card_id: &str, text: &str) -> Result<Card, String> {
    let card = store.get(card_id).map_err(|e| e.to_string())?;
    let done = router::record_with(store, &card, text, AGENT_ID, true).filter(|c| c.id == card.id && router::route_of(c).is_some());
    match done {
        Some(c) => {
            clear(store, card_id);
            store.get(card_id).map_err(|e| e.to_string()).or(Ok(c))
        }
        None => {
            clear(store, card_id);
            Err("its reply had no route the app could record (see the line above)".into())
        }
    }
}

/// The log line for a call that failed: why, and what happens instead.
pub fn failed_line(why: &str) -> String {
    format!("Couldn't size it before its session started: {why}. Its session starts without a route, and routes it with the card-router, as before.")
}

/// The cards still marked as being sized, with the start each waited for (`mode`, `why`).
pub fn pending(store: &Store) -> Vec<(Card, String, String)> {
    store
        .list(false)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|c| {
            let s = sizing_of(&c)?;
            let mode = s.get("mode").and_then(Value::as_str).filter(|m| *m == "new" || *m == "resume").unwrap_or("new").to_string();
            let why = s.get("why").and_then(Value::as_str).unwrap_or("move").to_string();
            Some((c, mode, why))
        })
        .collect()
}

// --- The call, run by the app ----------------------------------------------------------------

/// Cards whose sizing call runs now.
static IN_FLIGHT: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Is the app sizing `card_id` now?
pub fn in_flight(card_id: &str) -> bool {
    IN_FLIGHT.lock().map_or(false, |v| v.iter().any(|c| c == card_id))
}

fn take_flight(card_id: &str) -> bool {
    let Ok(mut v) = IN_FLIGHT.lock() else { return false };
    if v.iter().any(|c| c == card_id) {
        return false;
    }
    v.push(card_id.to_string());
    true
}

fn end_flight(card_id: &str) {
    if let Ok(mut v) = IN_FLIGHT.lock() {
        v.retain(|c| c != card_id);
    }
}

/// `begin_session`'s first step: true when this start waits for the card's sizing (one already
/// under way, or one it starts now), false when the session starts as before.
pub fn hold(app: &AppHandle, store: &mut Store, card: &Card, mode: &str, why: &str) -> bool {
    if in_flight(&card.id) {
        return true;
    }
    let live = app.state::<Arc<Sessions>>().is_live(&card.id).is_some();
    if blocker(store, card, mode, why, live).is_some() {
        return false;
    }
    if !take_flight(&card.id) {
        return true;
    }
    if let Err(e) = mark(store, card, mode, why) {
        end_flight(&card.id);
        let _ = store.add_log(&card.id, &failed_line(&format!("couldn't mark it on the card ({e})")), BY);
        return false;
    }
    let _ = store.add_log(&card.id, "Sizing it before its session starts (one quick read-only call with the card-router's rules); its session starts once it is routed", BY);
    let (a, id, m, w) = (app.clone(), card.id.clone(), mode.to_string(), why.to_string());
    std::thread::spawn(move || run_and_start(&a, &id, &m, &w));
    let _ = app.emit("cards-changed", ());
    true
}

/// What the call needs, read under the board's lock.
struct Inputs {
    exe: PathBuf,
    env: Vec<(String, String)>,
    cwd: PathBuf,
    model: String,
    brief: String,
}

fn inputs(store: &Store, card: &Card) -> Result<Inputs, String> {
    let exe = sessions::claude_exe().ok_or("claude.exe wasn't found on this machine")?;
    let account = crate::conversation(store, card).map(|c| c.1).unwrap_or_else(|| crate::dispatch::pick_account(store, card).0);
    let env = sessions::build_env(std::env::vars(), &account, card.number, false);
    let folder = card
        .fields
        .get("worktreePath")
        .and_then(Value::as_str)
        .filter(|p| Path::new(p).is_dir())
        .map(PathBuf::from)
        .unwrap_or_else(|| sessions::repo_folder(card.fields.get("repo").and_then(Value::as_str)));
    let home = kanban_store::home_var();
    let checkout = (folder.is_dir() && home.as_deref() != Some(folder.as_path())).then_some(folder);
    let cwd = match &checkout {
        Some(f) => f.clone(),
        None => {
            let d = kanban_store::data_dir().join("sizing");
            std::fs::create_dir_all(&d).map_err(|e| format!("couldn't make {}: {e}", d.display()))?;
            d
        }
    };
    let model = router::settings_for(store, card).model;
    let brief = brief(card, checkout.as_ref().map(|p| p.display().to_string()).as_deref());
    Ok(Inputs { exe, env, cwd, model, brief })
}

/// Runs the call: the brief on stdin, the reply read from stdout. Killed after `TIMEOUT`.
fn call(live: &Sessions, i: &Inputs) -> Result<String, String> {
    use crate::platform::NoWindow;
    use std::io::{Read, Write};
    let mut cmd = std::process::Command::new(&i.exe);
    cmd.args(args(&i.model))
        .env_clear()
        .envs(i.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .current_dir(&i.cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .no_window();
    let mut child = cmd.spawn().map_err(|e| format!("couldn't start claude: {e}"))?;
    let _ = live.adopt(child.id());
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(i.brief.as_bytes());
    }
    let mut out = child.stdout.take().ok_or("no output from claude")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        let _ = tx.send(s);
    });
    let got = rx.recv_timeout(TIMEOUT);
    let _ = child.kill();
    let _ = child.wait();
    match got {
        Ok(s) => read_output(&s),
        Err(_) => Err(format!("no answer after {} minutes", TIMEOUT.as_secs() / 60)),
    }
}

/// The sizing thread: the call (no lock held), the route on the card, then the start.
fn run_and_start(app: &AppHandle, card_id: &str, mode: &str, why: &str) {
    let live = app.state::<Arc<Sessions>>().inner().clone();
    let started = Instant::now();
    let prep = {
        let board = app.state::<Board>();
        let Ok(store) = board.0.lock() else {
            end_flight(card_id);
            return;
        };
        store.get(card_id).map_err(|e| e.to_string()).and_then(|c| inputs(&store, &c))
    };
    let reply = prep.and_then(|i| call(&live, &i));
    let board = app.state::<Board>();
    let Ok(mut store) = board.0.lock() else {
        end_flight(card_id);
        return;
    };
    let secs = started.elapsed().as_secs();
    let routed = match reply.and_then(|text| record(&mut store, card_id, &text)) {
        Ok(c) => {
            let _ = store.add_log(card_id, &format!("Sized before its session started, in {secs} s: the session starts on the route's model and effort"), BY);
            Some(c)
        }
        Err(why) => {
            clear(&mut store, card_id);
            let _ = store.add_log(card_id, &failed_line(&why), BY);
            None
        }
    };
    end_flight(card_id);
    start_after(app, &live, &mut store, card_id, mode, why, routed.is_some());
    drop(store);
    let _ = app.emit("session-changed", card_id.to_string());
    let _ = app.emit("cards-changed", ());
}

/// The start the sizing held, once it is done: the normal way (now, or in the cap's queue),
/// unless the card was archived, moved to a column that stops sessions, or has a session now.
fn start_after(app: &AppHandle, live: &Sessions, store: &mut Store, card_id: &str, mode: &str, why: &str, routed: bool) {
    let Ok(card) = store.get(card_id) else { return };
    let suspends = store.column(&card.column).ok().flatten().map_or(false, |c| c.on_enter == "suspend");
    let skip = if card.archived {
        Some("it was archived")
    } else if suspends {
        Some("it moved to a column that stops sessions")
    } else if live.is_live(card_id).is_some() {
        Some("it already has a session")
    } else {
        None
    };
    if let Some(s) = skip {
        let _ = store.add_log(card_id, &format!("Didn't start its session after the sizing: {s}"), BY);
        return;
    }
    if let Err(e) = crate::begin_session_now(app, store, &card, mode, why) {
        let _ = store.add_log(card_id, &format!("Couldn't start a session: {e}"), "the app");
        return;
    }
    if routed && card.column == "Planning" {
        let _ = store.queue(card_id, "message", &note(&card));
    }
}

/// At start-up: a card still marked as being sized (the app quit during its call) is sized
/// again, then started, by the start it waited for.
pub fn resume_at_start(app: &AppHandle) {
    let board = app.state::<Board>();
    let Ok(mut store) = board.0.lock() else { return };
    for (card, mode, why) in pending(&store) {
        clear(&mut store, &card.id);
        let Ok(card) = store.get(&card.id) else { continue };
        let _ = store.add_log(&card.id, "The app restarted while it was sizing this card: sizing it again", BY);
        if let Err(e) = crate::begin_session(app, &mut store, &card, &mode, &why) {
            let _ = store.add_log(&card.id, &format!("Couldn't start a session: {e}"), "the app");
        }
    }
    drop(store);
    let _ = app.emit("cards-changed", ());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("b.db")).unwrap();
        (dir, s)
    }

    fn mk(s: &mut Store, fields: Value) -> Card {
        s.create(None, fields.as_object().cloned().unwrap(), "test").unwrap()
    }

    #[test]
    fn only_moves_the_routers_start_and_hughs_new_start_are_sized() {
        for (mode, why) in [("new", "move"), ("resume", "move"), ("new", "router"), ("resume", "router"), ("new", "hugh")] {
            assert!(counts(mode, why), "{mode} {why}");
        }
        for (mode, why) in [("resume", "hugh"), ("resume", "message"), ("new", "message"), ("resume", "restart"), ("resume", "route")] {
            assert!(!counts(mode, why), "{mode} {why}");
        }
    }

    #[test]
    fn a_card_is_sized_first_only_when_every_rule_holds() {
        let (_d, mut s) = board();
        let c = mk(&mut s, json!({ "title": "T", "column": "Planning" }));
        assert_eq!(blocker(&s, &c, "new", "move", false), None);
        let ip = mk(&mut s, json!({ "title": "T", "column": "In progress" }));
        assert_eq!(blocker(&s, &ip, "resume", "move", false), None);
        assert_eq!(blocker(&s, &c, "resume", "message", false), Some("this kind of start isn't sized first"));
        assert_eq!(blocker(&s, &c, "new", "move", true), Some("its session is already running"));
        let bl = mk(&mut s, json!({ "title": "T", "column": "Backlog" }));
        assert_eq!(blocker(&s, &bl, "new", "hugh", false), Some("it isn't in Planning or In progress"));
        let routed = mk(&mut s, json!({ "title": "T", "column": "Planning", "route": { "size": "small" } }));
        assert_eq!(blocker(&s, &routed, "new", "move", false), Some("it already has a route"));
        let off = mk(&mut s, json!({ "title": "T", "column": "Planning", "subagents": { "router": false } }));
        assert_eq!(blocker(&s, &off, "new", "move", false), Some("its card-router is off"));
        let lead = mk(&mut s, json!({ "title": "lead", "column": "Planning" }));
        let child = s.create_in_stack(json!({ "title": "child", "column": "Planning" }).as_object().cloned().unwrap(), lead.number, "test").unwrap();
        assert_eq!(blocker(&s, &child, "new", "move", false), Some("it is a stack's child, which its lead plans"));
        let lead = s.get(&lead.id).unwrap();
        assert_eq!(blocker(&s, &lead, "new", "move", false), None, "a lead is sized, as the session routes it today");
        s.set_page_setting(kanban_store::SIZE_FIRST_KEY, "off").unwrap();
        assert!(blocker(&s, &c, "new", "move", false).unwrap().contains("setting"));
    }

    #[test]
    fn the_brief_is_the_routers_rules_with_the_card_written_in() {
        let (_d, mut s) = board();
        let c = mk(&mut s, json!({ "title": "Add a chip", "column": "Planning", "repo": "o/r", "summary": "One chip in board.js.", "doneWhenChecks": ["the chip shows", { "text": "it survives a reload", "state": "open" }] }));
        let b = brief(&c, Some("C:\\x\\repo"));
        assert!(b.contains(router::RULES), "the shared rules");
        for want in ["Title: Add a chip", "Repo: o/r", "Summary: One chip in board.js.", "- the chip shows", "- it survives a reload", "Column: Planning", &format!("Number: #{}", c.number), "C:\\x\\repo", "You have no board tools"] {
            assert!(b.contains(want), "{want}");
        }
        for line in ["CARD:", "ROUTE:", "MODEL:", "EFFORT:", "CHECK:", "REASON:"] {
            assert!(b.contains(line), "{line}");
        }
        assert!(!b.contains("kanban_get_card"));
        assert!(brief(&c, None).contains("size the card from its text alone"));
        // A long field is cut, and the whole card too.
        let long = mk(&mut s, json!({ "title": "T", "column": "Planning", "summary": "x".repeat(50_000) }));
        assert!(card_text(&long).chars().count() < CARD_MAX + 100);
    }

    #[test]
    fn the_subagents_prompt_still_reads_the_card_itself_and_shares_the_rules() {
        let v: Value = serde_json::from_str(&crate::agents::agents_json()).unwrap();
        let p = v[router::ROUTER]["prompt"].as_str().unwrap();
        assert!(p.starts_with("You are card-router. You size one card on Hugh's Claude Kanban DSH board before its plan is written."));
        assert!(p.contains("1. Read the card. Use kanban_get_card with the card number from the brief.\n2. Look quickly"));
        assert!(p.ends_with(router::RULES));
    }

    #[test]
    fn the_call_reads_only_and_loads_nothing_else() {
        let a = args("claude-opus-5-5");
        let has = |pair: [&str; 2]| a.windows(2).any(|w| w[0] == pair[0] && w[1] == pair[1]);
        assert_eq!(a[0], "-p");
        assert!(has(["--model", "claude-opus-5-5"]) && has(["--effort", "low"]) && has(["--tools", "Read,Grep,Glob"]));
        assert!(has(["--output-format", "json"]) && has(["--settings", r#"{"disableAllHooks":true}"#]) && has(["--max-turns", "20"]));
        assert!(a.contains(&"--strict-mcp-config".to_string()) && a.contains(&"--no-session-persistence".to_string()));
        assert!(!a.iter().any(|x| x == "--mcp-config" || x == "--agents" || x.contains("bypass")), "{a:?}");
    }

    #[test]
    fn the_reply_is_read_from_the_json_result() {
        let ok = r#"{"type":"result","subtype":"success","is_error":false,"result":"Looked.\nCARD: #4\nROUTE: small","duration_ms":9000}"#;
        assert_eq!(read_output(ok).unwrap(), "Looked.\nCARD: #4\nROUTE: small");
        assert_eq!(read_output(&format!("some noise\n{ok}\n")).unwrap(), "Looked.\nCARD: #4\nROUTE: small");
        let err = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"result":""}"#;
        assert!(read_output(err).unwrap_err().contains("error_max_turns"));
        let api = r#"{"type":"result","subtype":"success","is_error":true,"result":"Credit balance is too low"}"#;
        assert!(read_output(api).unwrap_err().contains("Credit balance"));
        assert_eq!(read_output("").unwrap_err(), "it printed nothing");
        assert!(read_output("Error: unknown option").unwrap_err().starts_with("it printed no result"));
    }

    #[test]
    fn a_route_from_the_call_is_recorded_once_and_clears_the_mark() {
        let (_d, mut s) = board();
        let c = mk(&mut s, json!({ "title": "T", "column": "Planning" }));
        let c = mark(&mut s, &c, "new", "move").unwrap();
        assert_eq!(sizing_of(&c).unwrap()["why"], "move");
        assert_eq!(pending(&s).len(), 1);
        let text = format!("CARD: #{}\nROUTE: small\nMODEL: claude-sonnet-5-5\nEFFORT: low\nCHECK: 5 min\nREASON: one chip", c.number);
        let got = record(&mut s, &c.id, &text).unwrap();
        assert!(sizing_of(&got).is_none(), "cleared with the route");
        let r = router::route_of(&got).unwrap();
        assert_eq!((r["size"].as_str(), r["before"].as_bool(), r["agentId"].as_str(), r["by"].as_str()), (Some("small"), Some(true), Some(AGENT_ID), Some(router::BY)));
        assert!(s.log(&c.id).unwrap().iter().any(|l| l.text.starts_with("Routed: small, claude-sonnet-5-5 low")));
        assert!(pending(&s).is_empty());
        // The session starts on the route's values: no restart.
        let set = router::settings_for(&s, &got);
        assert_eq!((set.model.as_str(), set.effort.as_str()), ("claude-sonnet-5-5", "low"));
        let n = note(&got);
        assert!(n.contains("routed small, claude-sonnet-5-5 low: one chip") && n.contains("Don't run the card-router") && n.contains("step 3"), "{n}");
    }

    #[test]
    fn a_reply_with_no_route_clears_the_mark_and_leaves_routing_to_the_session() {
        let (_d, mut s) = board();
        let c = mk(&mut s, json!({ "title": "T", "column": "Planning" }));
        mark(&mut s, &c, "new", "move").unwrap();
        assert!(record(&mut s, &c.id, "I couldn't tell.").is_err());
        let c = s.get(&c.id).unwrap();
        assert!(sizing_of(&c).is_none() && router::route_of(&c).is_none());
        assert!(s.log(&c.id).unwrap().iter().any(|l| l.text.starts_with("Route unclear:")));
        assert!(failed_line("no answer after 3 minutes").contains("routes it with the card-router, as before"));
        // A reply naming another card isn't recorded on it.
        let other = mk(&mut s, json!({ "title": "O", "column": "Planning" }));
        mark(&mut s, &c, "new", "move").unwrap();
        assert!(record(&mut s, &c.id, &format!("CARD: #{}\nROUTE: small", other.number)).is_err());
        assert!(router::route_of(&s.get(&other.id).unwrap()).is_none());
        assert!(sizing_of(&s.get(&c.id).unwrap()).is_none());
    }

    #[test]
    fn a_card_left_marked_by_a_restart_is_found_with_its_start() {
        let (_d, mut s) = board();
        let c = mk(&mut s, json!({ "title": "T", "column": "In progress" }));
        mark(&mut s, &c, "resume", "router").unwrap();
        let p = pending(&s);
        assert_eq!((p.len(), p[0].1.as_str(), p[0].2.as_str()), (1, "resume", "router"));
        clear(&mut s, &c.id);
        assert!(pending(&s).is_empty());
    }

    #[test]
    fn one_call_per_card_at_a_time() {
        let id = "card-presize-test";
        assert!(!in_flight(id));
        assert!(take_flight(id));
        assert!(in_flight(id) && !take_flight(id));
        end_flight(id);
        assert!(!in_flight(id));
    }
}
