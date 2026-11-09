//! The card-router subagent (Hugh, card #98): a quick look at a card at the start of Planning,
//! on low effort ("router should be on low effort"). It sizes the card (small, normal or split),
//! suggests the model and effort for its session and how long Hugh's check should take, and the
//! app records that on the card (`route`). It changes nothing itself.
//!
//! What the route does:
//! - Its model and effort are their own value, not Hugh's overrides. They apply only while the
//!   card is in Planning or In progress (`settings_for`). A running session is moved onto them,
//!   and back again, by one step in the dispatcher's loop (`restart_if_due`), so every way a card
//!   moves is covered: it is stopped and its conversation resumed with `--model` and `--effort`,
//!   because typing `/model` or `/effort` would save them as Hugh's default in his settings.json.
//!   A card with no route behaves as before.
//! - A card routed small goes on to In progress by itself once its plan critique (card #97)
//!   passed, while Hugh's rule is on (`go_on_if_ready`): the app's own held move.
//! - A card is routed once; Hugh's Route again (the panel) clears it and asks again.

use crate::agents::{target_card, Agent, OnStop};
use crate::sessions::{self, Sessions};
use crate::Board;
use kanban_store::{Card, Patch, Store, ROUTE_KEY};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

/// The subagent's name, as the session calls it (`subagent_type`) and the hook reports it.
pub const ROUTER: &str = "card-router";
/// Who its routes, and the app's moves and switches for them, are recorded as.
pub const BY: &str = "the app (card-router subagent)";
/// Who Hugh's Route again is recorded as.
pub const HUGH: &str = "Hugh (panel)";

/// The sizes a route can have.
pub const SIZES: [&str; 3] = ["small", "normal", "split"];
/// The models a route can apply (the panel's list). Anything else is recorded, not applied.
pub const MODELS: [&str; 4] = ["claude-opus-5-5", "claude-fable-5-1", "claude-sonnet-5-5", "claude-haiku-5-5"];
/// How long Hugh's check of the result should take.
pub const CHECKS: [&str; 3] = ["5 min", "15 min", "30 min+"];
/// The columns where a route's model and effort apply.
pub const ROUTED_COLUMNS: [&str; 2] = ["Planning", "In progress"];
/// The card field holding the card-critic's verdict on the plan (card #97's record:
/// `{state, ...}`, state "passed" once the critic passed it).
pub const CRITIQUE_KEY: &str = "critique";
/// The log's words for the small-card move.
pub const GO_ON_REASON: &str = "routed small and its plan critique passed";

const DESCRIPTION: &str = "Sizes one card on Hugh's Claude Kanban DSH board at the start of Planning, on low effort. Run it first in Planning, once per card, and never for a stack's child: brief it with the card number and the checkout. It reads the card, skims the code, changes nothing, and ends with CARD, ROUTE, MODEL, EFFORT, CHECK and REASON lines that the app records on the card.";

const PROMPT: &str = "You are card-router. You size one card on Hugh's Claude Kanban DSH board before its plan is written. Work fast. This is a quick look, not a plan.

1. Read the card. Use kanban_get_card with the card number from the brief.
2. Look quickly at the code that the card names, in the checkout that the brief names. Read only what you need to size the card.
3. Choose the route:
   - small: one clear change in a few files. No data rule, migration, sign-in or live change. A short plan is enough.
   - normal: a usual card. It needs a full plan.
   - split: two or more parts that can be built and tested one at a time.
4. Choose the model and effort for the card's session. For a small card, use claude-sonnet-5-5 with low or medium. For a normal or split card, use claude-opus-5-5 with high. Use xhigh or max only for a hard or risky card.
5. Choose how long Hugh needs to check the result: 5 min, 15 min or 30 min+.
6. Change nothing. Do not edit files, commit or push. Do not change, move or create a card. Do not run kb or claude config.

End your reply with these lines, and nothing after them:
CARD: #<the card's number>
ROUTE: <small|normal|split>
MODEL: <claude-opus-5-5|claude-sonnet-5-5|claude-haiku-5-5>
EFFORT: <low|medium|high|xhigh|max>
CHECK: <5 min|15 min|30 min+>
REASON: <one line>
For a split only, add one line after REASON for each child card. Start each line with \"- \", then a short title, a colon, and its scope in one sentence. The app reads these lines to record the route on the card.";

pub const AGENT: Agent = Agent {
    name: ROUTER,
    description: DESCRIPTION,
    prompt: PROMPT,
    // Hugh (card #98): "router should be on low effort".
    effort: Some("low"),
    edits: false,
    also_denied: &[],
    on_stop: OnStop::Custom(on_stop),
};

/// What a router's report says, from its closing lines. Each is None when its line is missing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Route {
    pub card: Option<i64>,
    pub size: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub check: Option<String>,
    pub reason: Option<String>,
    /// The `- ` lines after the last ROUTE line.
    pub children: Vec<String>,
}

/// A line's value after its `KEY:`, without markdown around it.
fn value(rest: &str) -> String {
    rest.trim_matches(|c: char| c == '*' || c == '`' || c == '_' || c.is_whitespace()).to_string()
}

/// The first word of a value, lower case, without punctuation after it.
fn word(v: &str) -> String {
    v.split_whitespace().next().unwrap_or("").trim_end_matches(|c: char| !c.is_ascii_alphanumeric()).to_ascii_lowercase()
}

/// Reads the router's closing lines (any case, markdown bold or backticks allowed). The last of
/// each line wins; the `- ` lines count only after the last ROUTE line.
pub fn parse_route(text: &str) -> Route {
    let mut r = Route::default();
    let mut after_route = false;
    for raw in text.lines() {
        // A key line may be written as a list item ("- ROUTE: small").
        let body = raw.trim();
        let body = body.strip_prefix("- ").or_else(|| body.strip_prefix("* ")).unwrap_or(body);
        let l = body.trim().trim_matches(|c| c == '*' || c == '`' || c == '_').trim();
        let up = l.to_ascii_uppercase();
        let key = ["CARD:", "ROUTE:", "MODEL:", "EFFORT:", "CHECK:", "REASON:"].into_iter().find(|k| up.starts_with(k));
        let Some(key) = key else {
            if after_route {
                if let Some(c) = raw.trim().strip_prefix("- ").or_else(|| raw.trim().strip_prefix("* ")) {
                    let c: String = kanban_store::mask_secrets(c.trim()).chars().take(300).collect();
                    if !c.is_empty() {
                        r.children.push(c);
                    }
                }
            }
            continue;
        };
        let v = value(&l[key.len()..]);
        match key {
            "CARD:" => {
                let digits: String = v.trim_start_matches(|c: char| c == '#' || c.is_whitespace()).chars().take_while(char::is_ascii_digit).collect();
                if let Ok(n) = digits.parse() {
                    r.card = Some(n);
                }
            }
            "ROUTE:" => {
                let w = word(&v);
                if SIZES.contains(&w.as_str()) {
                    r.size = Some(w);
                    r.children.clear();
                    after_route = true;
                }
            }
            "MODEL:" => r.model = Some(word(&v)).filter(|m| !m.is_empty()),
            "EFFORT:" => r.effort = Some(word(&v)).filter(|e| !e.is_empty()),
            "CHECK:" => {
                let n: String = v.chars().take_while(char::is_ascii_digit).collect();
                r.check = CHECKS.iter().find(|c| c.split(' ').next() == Some(n.as_str())).map(|c| c.to_string()).or(r.check);
            }
            _ => r.reason = Some(kanban_store::mask_secrets(&v).chars().take(300).collect()).filter(|s: &String| !s.is_empty()),
        }
    }
    r
}

pub fn valid_model(m: &str) -> bool {
    MODELS.contains(&m)
}

pub fn valid_effort(e: &str) -> bool {
    sessions::EFFORTS.contains(&e)
}

/// What the app records for a route: the `route` field's value, its log line and any notes
/// (each a log line of its own). `child`: the card is a stack's child, which can't be split.
/// Err with why when there is nothing to record.
pub fn record_of(r: &Route, child: bool, agent_id: &str, at: &str) -> Result<(Value, String, Vec<String>), String> {
    let Some(mut size) = r.size.clone() else {
        return Err("the card-router ended with no ROUTE line (small, normal or split), so nothing was recorded".into());
    };
    let mut notes = vec![];
    if child && size == "split" {
        size = "normal".into();
        notes.push("A stack's child can't be split (its lead plans the stack), so its route is recorded as normal.".to_string());
    }
    let mut rec = json!({ "size": size, "at": at, "agentId": agent_id, "by": BY });
    for (k, v) in [("model", &r.model), ("effort", &r.effort), ("check", &r.check), ("reason", &r.reason)] {
        if let Some(v) = v {
            rec[k] = json!(v);
        }
    }
    if size == "split" && !r.children.is_empty() {
        rec["children"] = json!(r.children);
    }
    if let Some(m) = r.model.as_deref().filter(|m| !valid_model(m)) {
        notes.push(format!("The route's model \"{m}\" isn't one the app knows ({}): recorded, not applied.", MODELS.join(", ")));
    }
    if let Some(e) = r.effort.as_deref().filter(|e| !valid_effort(e)) {
        notes.push(format!("The route's effort \"{e}\" isn't one of {}: recorded, not applied.", sessions::EFFORTS.join(", ")));
    }
    let what = [r.model.clone(), r.effort.clone()].into_iter().flatten().collect::<Vec<_>>().join(" ");
    let mut line = format!("Routed: {size}");
    if !what.is_empty() {
        line.push_str(&format!(", {what}"));
    }
    if let Some(c) = &r.check {
        line.push_str(&format!(", check ~{c}"));
    }
    if let Some(why) = &r.reason {
        line.push_str(&format!(": {why}"));
    }
    if size == "split" {
        line.push_str(&format!(" ({} child card{} named)", r.children.len(), if r.children.len() == 1 { "" } else { "s" }));
    }
    Ok((rec, line.chars().take(700).collect(), notes))
}

/// The card's route, if it has one (not cleared).
pub fn route_of(card: &Card) -> Option<&Value> {
    card.fields.get(ROUTE_KEY).filter(|v| v.is_object())
}

/// A `SubagentStop` from a card-router: records its route on the card it names (the session's
/// own card, or a child of its stack), once per card.
pub fn on_stop(app: &AppHandle, row_id: &str, body: &Value) {
    let text = body.get("last_assistant_message").and_then(Value::as_str).unwrap_or("");
    let agent_id = body.get("agent_id").and_then(Value::as_str).unwrap_or("");
    let board = app.state::<Board>();
    let Ok(mut store) = board.0.lock() else { return };
    let Some(own) = crate::agents::own_or_named(app, &store, row_id, parse_route(text).card) else { return };
    record(&mut store, &own, text, agent_id);
    drop(store);
    let _ = app.emit("cards-changed", ());
}

/// `on_stop`'s work, on the store: what was recorded, if anything.
pub fn record(store: &mut Store, own: &Card, text: &str, agent_id: &str) -> Option<Card> {
    let r = parse_route(text);
    let card = match target_card(store, own, r.card) {
        Ok(c) => c,
        Err(why) => {
            let _ = store.add_log(&own.id, &format!("Not recorded: the card-router subagent reported on {why}"), BY);
            return None;
        }
    };
    if let Some(old) = route_of(&card) {
        let size = old.get("size").and_then(Value::as_str).unwrap_or("?");
        let _ = store.add_log(&card.id, &format!("Not recorded: card #{} is already routed ({size}). A card is routed once; Route again in its panel clears the route.", card.number), BY);
        return None;
    }
    let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let (rec, line, notes) = match record_of(&r, card.stack_of().is_some(), agent_id, &at) {
        Ok(x) => x,
        Err(why) => {
            let _ = store.add_log(&card.id, &format!("Route unclear: {why}"), BY);
            return None;
        }
    };
    let mut patch = Patch::new();
    patch.insert(ROUTE_KEY.into(), rec);
    if card.fields.get(CLEARED_KEY).is_some_and(|v| !v.is_null()) {
        patch.insert(CLEARED_KEY.into(), Value::Null);
    }
    let card = match store.update(&card.id, card.version, &patch, BY) {
        Ok(c) => c,
        Err(e) => {
            let _ = store.add_log(&card.id, &format!("Couldn't record the card-router's route: {e}"), BY);
            return None;
        }
    };
    let _ = store.add_log(&card.id, &line, BY);
    for n in notes {
        let _ = store.add_log(&card.id, &n, BY);
    }
    go_on_if_ready(store, &card);
    store.get(&card.id).ok()
}

/// The model, effort and permission mode a card's session uses, and where the model and effort
/// come from: "Hugh" (his overrides in the panel), "route" (the card-router's, only in Planning
/// and In progress, only values the app knows) or "column" (the column's settings, else its
/// defaults).
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub model: String,
    pub model_from: &'static str,
    pub effort: String,
    pub effort_from: &'static str,
    pub permission_mode: String,
}

/// The route's model and effort that apply to `card` now: in a routed column, and known.
pub fn route_values(card: &Card) -> (Option<String>, Option<String>) {
    let Some(r) = route_of(card).filter(|_| ROUTED_COLUMNS.contains(&card.column.as_str())) else { return (None, None) };
    let get = |k: &str| r.get(k).and_then(Value::as_str).map(str::to_string);
    (get("model").filter(|m| valid_model(m)), get("effort").filter(|e| valid_effort(e)))
}

pub fn settings_for(store: &Store, card: &Card) -> Settings {
    let cfg = store.column(&card.column).ok().flatten();
    let (dm, de) = sessions::column_defaults(&card.column);
    let (rm, re) = route_values(card);
    let over = |k: &str| card.fields.get(k).and_then(Value::as_str).map(str::to_string);
    let pick = |hugh: Option<String>, route: Option<String>, col: Option<String>, default: &str| -> (String, &'static str) {
        if let Some(v) = hugh {
            (v, "Hugh")
        } else if let Some(v) = route {
            (v, "route")
        } else {
            (col.unwrap_or_else(|| default.to_string()), "column")
        }
    };
    let (model, model_from) = pick(over("modelOverride"), rm, cfg.as_ref().and_then(|c| c.model.clone()), dm);
    // A stack's lead integrates in Planning and In progress (card #113): low effort there in place
    // of the column's. Merged in wave 2: Hugh's override first, then the route, then this.
    let col_effort = if crate::stackrun::lead_effort_applies(card) { Some(crate::stackrun::LEAD_EFFORT.to_string()) } else { cfg.as_ref().and_then(|c| c.effort.clone()) };
    let (effort, effort_from) = pick(over("effortOverride"), re, col_effort, de);
    let permission_mode = over("permissionOverride").or(cfg.as_ref().and_then(|c| c.permission_mode.clone())).unwrap_or_else(|| "bypassPermissions".to_string());
    Settings { model, model_from, effort, effort_from, permission_mode }
}

/// The card field Route again leaves behind (card #98): the cleared route's model and effort,
/// so a session still on them goes back to the card's own values. A new route removes it.
pub const CLEARED_KEY: &str = "routeCleared";

/// The route values a running session may be on: the route's, else the cleared route's.
fn route_marks(card: &Card) -> Option<&Value> {
    route_of(card).or_else(|| card.fields.get(CLEARED_KEY).filter(|v| v.is_object()))
}

/// What a routed card's running session should change to: for each of model and effort, the
/// value `settings_for` wants, when the session runs on another and the route is involved (the
/// wanted value is the route's, or the session's value is the route's or the cleared route's, so
/// it goes back when the card leaves Planning and In progress, or Route again clears the route).
/// Nothing for a card with no route, or a session whose value isn't known.
pub fn reconcile(card: &Card, s: &Settings, live: (Option<&str>, Option<&str>)) -> (Option<String>, Option<String>) {
    let Some(r) = route_marks(card) else { return (None, None) };
    let field = |want: &str, from: &str, live: Option<&str>, key: &str| -> Option<String> {
        let live = live?;
        if live == want {
            return None;
        }
        let routed = r.get(key).and_then(Value::as_str) == Some(live);
        (from == "route" || routed).then(|| want.to_string())
    };
    (field(&s.model, s.model_from, live.0, "model"), field(&s.effort, s.effort_from, live.1, "effort"))
}

/// The log line for a restart onto new values.
pub fn switch_line(card: &Card, s: &Settings, model: Option<&str>, effort: Option<&str>) -> String {
    let what = [model.map(|m| format!("model {m}")), effort.map(|e| format!("effort {e}"))].into_iter().flatten().collect::<Vec<_>>().join(" and ");
    let to_route = (model.is_some() && s.model_from == "route") || (effort.is_some() && s.effort_from == "route");
    let why = if to_route {
        "the card-router subagent's route (it applies in Planning and In progress)".to_string()
    } else if route_of(card).is_none() {
        "back to the card's own values: Route again cleared the route".to_string()
    } else {
        let whose = if (model.is_some() && s.model_from == "Hugh") || (effort.is_some() && s.effort_from == "Hugh") { "Hugh's" } else { "the column's" };
        format!("back to {whose} values: the route applies only in Planning and In progress, and the card is in {}", card.column)
    };
    format!("Restarting the session on {what}: {why}. Its conversation is resumed. The app restarts it rather than typing /model or /effort, which Claude Code would save as Hugh's default.")
}

/// Cards whose session the app is restarting onto new values now, so one restart at a time.
static RESTARTING: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
/// Marks `card_id`'s restart as started. False if one is already under way.
pub fn begin_restart(card_id: &str) -> bool {
    let Ok(mut r) = RESTARTING.lock() else { return false };
    if r.iter().any(|c| c == card_id) {
        return false;
    }
    r.push(card_id.to_string());
    true
}

/// Marks `card_id`'s restart as over (done, or put off for a running subagent).
pub fn end_restart(card_id: &str) {
    if let Ok(mut r) = RESTARTING.lock() {
        r.retain(|c| c != card_id);
    }
}

/// Is the app restarting `card_id`'s session now (from the decision until the resumed session
/// starts)? Nothing may be typed into it meanwhile: a message typed into the old session is lost
/// when it is stopped, which is how a card routed small that went on to In progress could miss
/// its In progress prompt and sit idle (the ping-pong test's #187 and #188). It stays queued
/// and is typed into the resumed session.
pub fn restarting(card_id: &str) -> bool {
    RESTARTING.lock().map_or(false, |r| r.iter().any(|c| c == card_id))
}

/// Cards whose restart waits for a subagent, and until when it isn't tried again.
static WAITING: std::sync::Mutex<Vec<(String, std::time::Instant)>> = std::sync::Mutex::new(Vec::new());
/// How long a restart that found a subagent running waits before it looks again.
const SUBAGENT_RETRY: std::time::Duration = std::time::Duration::from_secs(30);

/// Whether a session has a subagent that hasn't finished and still writes (within the monitor's
/// quiet time), so a restart would kill it: a background card-tester, card-reviewer or
/// card-critic the session waits for looks idle. One that has gone quiet for longer is dead
/// (the monitor nudges it), so it doesn't hold the restart for ever.
pub fn subagent_running(subs: &[crate::transcript::Subagent], now: chrono::DateTime<chrono::Utc>) -> bool {
    crate::monitor::subagent_state(subs, None, crate::monitor::QUIET, now).1
}

/// The dispatcher's step for a free session (card #98): if a routed card's session runs on the
/// wrong model or effort, it is stopped and its conversation resumed with the right ones
/// (`--model`, `--effort`). Typing `/model` or `/effort` would also save them as Hugh's default in
/// his own settings.json, which the app must never write. Not while Hugh is looking at the card
/// (he may be typing in it), not while one of its subagents still runs (a session waiting on a
/// background card-tester, card-reviewer or card-critic looks idle; it is tried again every 30 s),
/// and one restart at a time. Returns whether it started one (or a look for a running subagent).
pub fn restart_if_due(app: &AppHandle, live: &Arc<Sessions>, store: &mut Store, card_id: &str) -> bool {
    let Ok(card) = store.get(card_id) else { return false };
    if route_marks(&card).is_none() || crate::notify::looking_at(app, card_id) {
        return false;
    }
    let Some(row) = live.is_live(card_id) else { return false };
    let s = settings_for(store, &card);
    let (m, e) = reconcile(&card, &s, (row.model.as_deref(), row.effort.as_deref()));
    if m.is_none() && e.is_none() {
        return false;
    }
    let now = std::time::Instant::now();
    if let Ok(mut w) = WAITING.lock() {
        w.retain(|(_, until)| *until > now);
        if w.iter().any(|(c, _)| c == card_id) {
            return false;
        }
    }
    if !begin_restart(card_id) {
        return false;
    }
    let line = switch_line(&card, &s, m.as_deref(), e.as_deref());
    let (a, l, id) = (app.clone(), live.clone(), card_id.to_string());
    std::thread::spawn(move || {
        let done = |id: &str| end_restart(id);
        // A subagent still running would die with the session: wait for it. The transcript is
        // read here, with no lock held.
        let subs = crate::transcript::find(&row.account, &row.claude_session_id).map(|p| crate::transcript::subagents(&p)).unwrap_or_default();
        if subagent_running(&subs, chrono::Utc::now()) {
            if let Ok(mut w) = WAITING.lock() {
                w.push((id.clone(), std::time::Instant::now() + SUBAGENT_RETRY));
            }
            done(&id);
            return;
        }
        {
            let board = a.state::<Board>();
            let lock = board.0.lock();
            if let Ok(mut store) = lock {
                let _ = store.add_log(&id, &line, BY);
            };
        }
        let _ = l.stop(&a, &id);
        {
            let board = a.state::<Board>();
            let lock = board.0.lock();
            if let Ok(mut store) = lock {
                if let Ok(card) = store.get(&id) {
                    if let Err(e) = crate::begin_session(&a, &mut store, &card, "resume", "route") {
                        let _ = store.add_log(&id, &format!("Couldn't resume the session after the restart: {e}. Send it a message to start it again."), BY);
                    }
                }
            };
        }
        done(&id);
        let _ = a.emit("session-changed", id);
        let _ = a.emit("cards-changed", ());
    });
    true
}

/// Why the small-card move (card #98) isn't due, or None when it is: a card in Planning, routed
/// small, whose plan critique passed after the route was recorded (an older pass was for an older
/// plan), that isn't a stack's child or lead, hasn't gone on for this route already, while Hugh's
/// rule is on. `queued`: checked when the waiting move is applied, so a waiting move is its own.
pub fn go_on_blocker(store: &Store, card: &Card, queued: bool) -> Option<&'static str> {
    let Some(r) = route_of(card) else { return Some("it has no route (Route again cleared it)") };
    let critique = card.fields.get(CRITIQUE_KEY);
    let state = critique.and_then(|c| c.get("state")).and_then(Value::as_str);
    let critique_at = critique.and_then(|c| c.get("at")).and_then(Value::as_str).unwrap_or("");
    let route_at = r.get("at").and_then(Value::as_str).unwrap_or("");
    if card.column != "Planning" {
        return Some("it isn't in Planning any more");
    }
    if card.archived {
        return Some("it is archived");
    }
    if card.stack_of().is_some() || card.is_lead() {
        return Some("it is in a stack");
    }
    if r.get("size").and_then(Value::as_str) != Some("small") {
        return Some("it isn't routed small");
    }
    if r.get("wentOnAt").is_some() {
        return Some("it already went on once for this route");
    }
    if state != Some("passed") {
        return Some("its plan critique hasn't passed");
    }
    // Card #144: a question for Hugh, or a FAIL since it came into Planning (its one critique
    // failed, so a later pass doesn't count), means Hugh moves it on.
    if critique.and_then(|c| c.get("hugh")).and_then(Value::as_bool) == Some(true) {
        return Some("its plan critic asked Hugh something");
    }
    if crate::critic::one_failed(card) {
        return Some("its one plan critique failed, so Hugh moves it on");
    }
    // Both are the app's own RFC 3339 times in UTC with milliseconds, so they sort as text.
    if route_at.is_empty() || critique_at <= route_at {
        return Some("its plan critique passed before the route, so the plan needs a fresh critique");
    }
    if !store.small_go_on() {
        return Some("Hugh's rule for small cards is off");
    }
    if !queued && store.waiting(&card.id, "move").ok().flatten().is_some() {
        return Some("a move is already waiting");
    }
    None
}

pub fn go_on_due(store: &Store, card: &Card) -> bool {
    go_on_blocker(store, card, false).is_none()
}

/// Queues the small-card move when it is due: the app's held move, applied when the card's
/// session ends its turn (the dispatcher reads its `by`, checks `go_on_blocker` again, and then
/// marks the route with `went_on`, so it happens once per route). Called when a route is recorded
/// and when any PASS/FAIL verdict is (so the critique's pass sets it off). Returns whether it
/// queued the move.
pub fn go_on_if_ready(store: &mut Store, card: &Card) -> bool {
    let Ok(card) = store.get(&card.id) else { return false };
    if !go_on_due(store, &card) {
        return false;
    }
    let payload = json!({ "column": "In progress", "reason": GO_ON_REASON, "by": BY });
    if store.queue(&card.id, "move", &payload.to_string()).is_err() {
        return false;
    }
    let _ = store.add_log(&card.id, &format!("Going on to In progress when this turn ends: {GO_ON_REASON} (Hugh's rule: a card routed small starts building by itself)"), BY);
    true
}

/// The first line of the session's message when the app's small-card move is applied.
pub const GO_ON_LEAD: &str = "it was routed small and its plan critique passed, and Hugh's rule lets a card routed small start building by itself.";

/// Marks the route once its small-card move was applied, so it goes on once per route.
pub fn went_on(store: &mut Store, card_id: &str) {
    let Ok(card) = store.get(card_id) else { return };
    let Some(mut route) = route_of(&card).cloned() else { return };
    route["wentOnAt"] = json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    let mut patch = Patch::new();
    patch.insert(ROUTE_KEY.into(), route);
    let _ = store.update(&card.id, card.version, &patch, BY);
}


/// What Route again sends the card's session.
pub fn again_message(card: &Card) -> String {
    format!(
        "Hugh pressed Route again on card #{n}, so its route is cleared. Run the card-router subagent for card #{n} again (the Agent tool, subagent_type \"{ROUTER}\"). Brief it with the card number and the checkout, and wait for its result: the app records the new route on the card. Then check that your plan still fits the route. If your session has no card-router subagent, say so on the card.",
        n = card.number
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(fields: Value, column: &str, number: i64) -> Card {
        Card { id: format!("c{number}"), number, column: column.into(), order: None, order_col: None, archived: false, fields: fields.as_object().cloned().unwrap(), version: 1 }
    }

    fn board() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("b.db")).unwrap();
        (dir, s)
    }

    fn mk(s: &mut Store, column: &str) -> Card {
        s.create(None, json!({ "title": "T", "column": column }).as_object().cloned().unwrap(), "test").unwrap()
    }

    const REPORT: &str = "Looked at board.js.\n\n**CARD:** #42\n**ROUTE:** `small`\nMODEL: claude-sonnet-5-5\nEFFORT: Medium.\nCHECK: 5 minutes\nREASON: one chip in board.js, no data change";

    #[test]
    fn the_route_lines_are_read_from_the_report() {
        let r = parse_route(REPORT);
        assert_eq!(r.card, Some(42));
        assert_eq!(r.size.as_deref(), Some("small"));
        assert_eq!(r.model.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(r.effort.as_deref(), Some("medium"));
        assert_eq!(r.check.as_deref(), Some("5 min"));
        assert_eq!(r.reason.as_deref(), Some("one chip in board.js, no data change"));
        assert!(r.children.is_empty());
        // A split names its children after the ROUTE line; dashes before it don't count.
        let r = parse_route("- a note\nCARD: 7\nROUTE: split\nMODEL: claude-opus-5-5\nEFFORT: high\nCHECK: 30 min+\nREASON: two parts\n- Store: the field and its test\n- UI: the chip");
        assert_eq!(r.size.as_deref(), Some("split"));
        assert_eq!(r.check.as_deref(), Some("30 min+"));
        assert_eq!(r.children, vec!["Store: the field and its test", "UI: the chip"]);
        // The last ROUTE wins, and starts the children again.
        let r = parse_route("ROUTE: split\n- a\nROUTE: normal\nCHECK: 15 min");
        assert_eq!((r.size.as_deref(), r.children.len(), r.check.as_deref()), (Some("normal"), 0, Some("15 min")));
        // Closing lines written as a list are read too, and aren't taken for children.
        let r = parse_route("- CARD: #3
- ROUTE: split
- CHECK: 15 min
- REASON: two
- UI: the chip");
        assert_eq!((r.card, r.size.as_deref(), r.check.as_deref(), r.reason.as_deref()), (Some(3), Some("split"), Some("15 min"), Some("two")));
        assert_eq!(r.children, vec!["UI: the chip"]);
        // A secret in the reason or a child line is masked before it is stored.
        let r = parse_route("ROUTE: split\nREASON: see http://127.0.0.1:3006/dev-sign-in.html?as=admin\n- Login: Authorization: Bearer abc.DEF-123_x");
        assert!(!r.reason.as_deref().unwrap().contains("dev-sign-in.html?as=admin"), "{:?}", r.reason);
        assert!(!r.children[0].contains("abc.DEF-123_x"), "{:?}", r.children);
        // Missing or unknown lines are None.
        let r = parse_route("ROUTE: tiny\nMODEL:\nCHECK: an hour");
        assert_eq!(r, Route::default());
        assert_eq!(parse_route("All fine.").size, None);
    }

    #[test]
    fn a_route_is_recorded_with_who_set_it() {
        let r = parse_route(REPORT);
        let (rec, line, notes) = record_of(&r, false, "a1", "t").unwrap();
        assert_eq!(rec["size"], "small");
        assert_eq!(rec["model"], "claude-sonnet-5-5");
        assert_eq!(rec["effort"], "medium");
        assert_eq!(rec["check"], "5 min");
        assert_eq!(rec["by"], BY);
        assert_eq!((rec["agentId"].as_str(), rec["at"].as_str()), (Some("a1"), Some("t")));
        assert!(rec.get("children").is_none());
        assert_eq!(line, "Routed: small, claude-sonnet-5-5 medium, check ~5 min: one chip in board.js, no data change");
        assert!(notes.is_empty());
        // No ROUTE line: nothing to record.
        assert!(record_of(&parse_route("CARD: #4\nREASON: x"), false, "a", "t").unwrap_err().contains("no ROUTE line"));
    }

    #[test]
    fn a_split_on_a_stack_child_is_recorded_as_normal() {
        let r = parse_route("ROUTE: split\nREASON: big\n- a: x\n- b: y");
        let (rec, line, _) = record_of(&r, false, "a", "t").unwrap();
        assert_eq!((rec["size"].as_str(), rec["children"].as_array().map(Vec::len)), (Some("split"), Some(2)));
        assert!(line.ends_with("(2 child cards named)"), "{line}");
        let (rec, line, notes) = record_of(&r, true, "a", "t").unwrap();
        assert_eq!(rec["size"], "normal");
        assert!(rec.get("children").is_none());
        assert!(line.starts_with("Routed: normal"));
        assert!(notes[0].contains("can't be split"), "{notes:?}");
    }

    #[test]
    fn an_unknown_model_or_effort_is_recorded_but_not_applied() {
        let r = parse_route("ROUTE: normal\nMODEL: gpt-9\nEFFORT: extreme");
        let (rec, _, notes) = record_of(&r, false, "a", "t").unwrap();
        assert_eq!((rec["model"].as_str(), rec["effort"].as_str()), (Some("gpt-9"), Some("extreme")));
        assert_eq!(notes.len(), 2);
        assert!(notes[0].contains("\"gpt-9\"") && notes[0].contains("not applied"));
        let c = card(json!({ "title": "T", "route": rec }), "Planning", 3);
        assert_eq!(route_values(&c), (None, None));
        let (_d, s) = board();
        let set = settings_for(&s, &c);
        assert_eq!((set.model_from, set.effort_from), ("column", "column"));
    }

    #[test]
    fn the_route_applies_only_in_planning_and_in_progress_and_hugh_comes_first() {
        let (_d, s) = board();
        let route = json!({ "size": "small", "model": "claude-sonnet-5-5", "effort": "low" });
        for col in ["Planning", "In progress"] {
            let set = settings_for(&s, &card(json!({ "title": "T", "route": route }), col, 1));
            assert_eq!((set.model.as_str(), set.model_from, set.effort.as_str(), set.effort_from), ("claude-sonnet-5-5", "route", "low", "route"), "{col}");
        }
        for col in ["Backlog", "Blocked", "Human test", "Ship it", "Deploy complete", "Done"] {
            let set = settings_for(&s, &card(json!({ "title": "T", "route": route }), col, 1));
            let (dm, de) = sessions::column_defaults(col);
            assert_eq!((set.model.as_str(), set.model_from, set.effort.as_str(), set.effort_from), (dm, "column", de, "column"), "{col}");
        }
        // Hugh's override beats the route, field by field.
        let set = settings_for(&s, &card(json!({ "title": "T", "route": route, "effortOverride": "max" }), "Planning", 1));
        assert_eq!((set.model_from, set.effort.as_str(), set.effort_from), ("route", "max", "Hugh"));
        // No route: as before.
        let set = settings_for(&s, &card(json!({ "title": "T" }), "In progress", 1));
        assert_eq!((set.model.as_str(), set.effort.as_str(), set.model_from), ("claude-opus-5-5", "high", "column"));
        assert_eq!(set.permission_mode, "bypassPermissions");
        // A cleared route (Route again) is no route.
        assert_eq!(route_values(&card(json!({ "title": "T", "route": null }), "Planning", 1)), (None, None));
    }

    #[test]
    fn a_running_session_is_switched_to_the_route_and_back() {
        let (_d, s) = board();
        let route = json!({ "size": "small", "model": "claude-sonnet-5-5", "effort": "low" });
        // The route lands in Planning: the session (on the column's values) switches to it.
        let c = card(json!({ "title": "T", "route": route }), "Planning", 1);
        let set = settings_for(&s, &c);
        assert_eq!(reconcile(&c, &set, (Some("claude-opus-5-5"), Some("high"))), (Some("claude-sonnet-5-5".into()), Some("low".into())));
        let line = switch_line(&c, &set, Some("claude-sonnet-5-5"), Some("low"));
        assert!(line.starts_with("Restarting the session on model claude-sonnet-5-5 and effort low: the card-router subagent's route"), "{line}");
        assert!(line.contains("rather than typing /model"), "{line}");
        // Already on it: nothing more.
        assert_eq!(reconcile(&c, &set, (Some("claude-sonnet-5-5"), Some("low"))), (None, None));
        // Unknown session values: nothing.
        assert_eq!(reconcile(&c, &set, (None, None)), (None, None));
        // It leaves In progress for Human test: back to the column's values.
        let c = card(json!({ "title": "T", "route": route }), "Human test", 1);
        let set = settings_for(&s, &c);
        assert_eq!(reconcile(&c, &set, (Some("claude-sonnet-5-5"), Some("low"))), (Some("claude-opus-5-5".into()), Some("high".into())));
        let line = switch_line(&c, &set, Some("claude-opus-5-5"), Some("high"));
        assert!(line.contains("back to the column's values") && line.contains("in Human test"), "{line}");
        // A session the route never set (started elsewhere on other values) is left alone there.
        assert_eq!(reconcile(&c, &set, (Some("claude-opus-5-5"), Some("medium"))), (None, None));
        // No route: never switched, whatever it runs on.
        let plain = card(json!({ "title": "T" }), "Human test", 1);
        assert_eq!(reconcile(&plain, &settings_for(&s, &plain), (Some("claude-haiku-5-5"), Some("low"))), (None, None));
    }

    #[test]
    fn the_route_doesnt_fight_hughs_override() {
        let (_d, s) = board();
        let route = json!({ "size": "small", "model": "claude-sonnet-5-5", "effort": "low" });
        let c = card(json!({ "title": "T", "route": route, "modelOverride": "claude-opus-5-5", "effortOverride": "xhigh" }), "Planning", 1);
        let set = settings_for(&s, &c);
        // On Hugh's values: nothing to do.
        assert_eq!(reconcile(&c, &set, (Some("claude-opus-5-5"), Some("xhigh"))), (None, None));
        // On another value the route didn't set (Hugh typed /model himself): left alone.
        assert_eq!(reconcile(&c, &set, (Some("claude-haiku-5-5"), Some("medium"))), (None, None));
        // Still on the route's values from before he set his: his win.
        assert_eq!(reconcile(&c, &set, (Some("claude-sonnet-5-5"), Some("low"))), (Some("claude-opus-5-5".into()), Some("xhigh".into())));
        assert!(switch_line(&c, &set, Some("claude-opus-5-5"), None).contains("back to Hugh's values"));
    }

    const ROUTED_AT: &str = "2026-11-06T01:00:00.000Z";
    const CRITIQUED_AT: &str = "2026-11-06T02:00:00.000Z";

    // A card routed `size`, whose plan critique (after the route) is `critique`.
    fn routed(s: &mut Store, size: &str, critique: &str) -> Card {
        let c = mk(s, "Planning");
        let p = json!({ "route": { "size": size, "at": ROUTED_AT }, "critique": { "state": critique, "at": CRITIQUED_AT } }).as_object().cloned().unwrap();
        s.update(&c.id, c.version, &p, "test").unwrap()
    }

    #[test]
    fn a_small_card_goes_on_only_after_its_critique_passed() {
        let (_d, mut s) = board();
        let c = routed(&mut s, "small", "needed");
        assert!(!go_on_due(&s, &c), "no critique pass yet");
        assert!(!go_on_if_ready(&mut s, &c));
        let c = routed(&mut s, "small", "passed");
        assert!(go_on_due(&s, &c));
        assert!(go_on_if_ready(&mut s, &c));
        // Queued as the app's move, applied when the turn ends, and marked on the route.
        let m = s.waiting(&c.id, "move").unwrap().unwrap();
        let p: Value = serde_json::from_str(&m.payload).unwrap();
        assert_eq!((p["column"].as_str(), p["by"].as_str(), p["reason"].as_str()), (Some("In progress"), Some(BY), Some(GO_ON_REASON)));
        let c = s.get(&c.id).unwrap();
        assert_eq!(c.column, "Planning", "it moves when the turn ends, not now");
        assert!(s.log(&c.id).unwrap().iter().any(|l| l.text.starts_with("Going on to In progress when this turn ends")));
        // Not queued twice while it waits; still due when it is applied.
        assert!(!go_on_if_ready(&mut s, &c));
        assert_eq!(go_on_blocker(&s, &c, true), None);
        // Once applied, the route is marked: never twice for one route.
        s.done(m.id, "applied").unwrap();
        went_on(&mut s, &c.id);
        let c = s.get(&c.id).unwrap();
        assert!(c.fields["route"]["wentOnAt"].is_string());
        assert!(!go_on_if_ready(&mut s, &c));
        // Normal and split cards wait for Hugh or the router.
        for size in ["normal", "split"] {
            let c = routed(&mut s, size, "passed");
            assert!(!go_on_due(&s, &c), "{size}");
        }
        // No route: no move.
        let plain = mk(&mut s, "Planning");
        assert!(!go_on_due(&s, &plain));
    }

    // Card #144: one critique for a small card; a FAIL or a question for Hugh leaves it to him.
    #[test]
    fn a_small_card_waits_for_hugh_after_a_fail_or_a_question() {
        let (_d, mut s) = board();
        let with = |s: &mut Store, extra: Value| {
            let c = routed(s, "small", "passed");
            let mut rec = c.fields["critique"].clone();
            rec.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap());
            s.update(&c.id, c.version, &json!({ "critique": rec }).as_object().cloned().unwrap(), "test").unwrap()
        };
        // A pass after its one FAIL (marked) doesn't move it; a FAIL from before the route doesn't count.
        let c = with(&mut s, json!({ "round": 1 }));
        assert!(go_on_due(&s, &c));
        let c = with(&mut s, json!({ "round": 1, "oneFailed": true }));
        assert_eq!(go_on_blocker(&s, &c, false), Some("its one plan critique failed, so Hugh moves it on"));
        assert!(!go_on_if_ready(&mut s, &c));
        // A pass with a HUGH: line doesn't either.
        let c = with(&mut s, json!({ "round": 0, "hugh": true }));
        assert_eq!(go_on_blocker(&s, &c, false), Some("its plan critic asked Hugh something"));
        // A pass with plain notes does.
        let c = with(&mut s, json!({ "round": 0, "notes": ["a note"] }));
        assert!(go_on_due(&s, &c));
        // A plan edit after the pass keeps it (the store keeps a small card's record), so the
        // waiting move still applies when the turn ends.
        assert!(go_on_if_ready(&mut s, &c));
        let c = s.get(&c.id).unwrap();
        let c = s.update(&c.id, c.version, &json!({ "summary": "the plan, tidied after the pass" }).as_object().cloned().unwrap(), "test").unwrap();
        assert_eq!(c.fields["critique"]["state"], "passed");
        assert_eq!(go_on_blocker(&s, &c, true), None);
    }

    #[test]
    fn a_small_card_doesnt_go_on_as_a_child_a_lead_or_with_the_rule_off() {
        let (_d, mut s) = board();
        let lead = routed(&mut s, "small", "passed");
        let child = s.create_in_stack(json!({ "title": "child", "column": "Planning" }).as_object().cloned().unwrap(), lead.number, "test").unwrap();
        let p = json!({ "route": { "size": "small" }, "critique": { "state": "passed" }, "column": "Planning" }).as_object().cloned().unwrap();
        let child = s.update(&child.id, child.version, &p, "test").unwrap();
        let lead = s.get(&lead.id).unwrap();
        assert!(lead.is_lead() && child.stack_of().is_some());
        assert!(!go_on_due(&s, &lead), "a lead");
        assert!(!go_on_due(&s, &child), "a child");
        // Not from another column, and not with a move already waiting.
        let ip = routed(&mut s, "small", "passed");
        let ip = s.update(&ip.id, ip.version, &json!({ "column": "In progress" }).as_object().cloned().unwrap(), "test").unwrap();
        assert!(!go_on_due(&s, &ip));
        let waiting = routed(&mut s, "small", "passed");
        s.queue(&waiting.id, "move", &json!({ "column": "Blocked", "reason": "x" }).to_string()).unwrap();
        assert!(!go_on_due(&s, &waiting));
        // Hugh's rule off.
        let c = routed(&mut s, "small", "passed");
        s.set_page_setting(kanban_store::SMALL_GO_ON_KEY, "off").unwrap();
        assert!(!go_on_due(&s, &c));
        s.set_page_setting(kanban_store::SMALL_GO_ON_KEY, "on").unwrap();
        assert!(go_on_due(&s, &c));
    }

    #[test]
    fn a_card_is_routed_once_and_route_again_clears_it() {
        let (_d, mut s) = board();
        let c = mk(&mut s, "Planning");
        let rec = record(&mut s, &c, REPORT.replace("#42", &format!("#{}", c.number)).as_str(), "a1").unwrap();
        assert_eq!(rec.fields["route"]["size"], "small");
        assert_eq!(rec.fields["route"]["by"], BY);
        let log = s.log(&c.id).unwrap();
        assert!(log.iter().any(|l| l.text.starts_with("Routed: small, claude-sonnet-5-5 medium")), "{log:?}");
        // A second verdict over the route isn't recorded.
        assert!(record(&mut s, &rec, "ROUTE: normal", "a2").is_none());
        let again = s.get(&c.id).unwrap();
        assert_eq!(again.fields["route"]["size"], "small");
        assert!(s.log(&c.id).unwrap().iter().any(|l| l.text.contains("is already routed (small)")));
        // Route again clears it, and the next route is recorded.
        let cleared = s.update(&c.id, again.version, &json!({ "route": null }).as_object().cloned().unwrap(), HUGH).unwrap();
        let rec = record(&mut s, &cleared, "ROUTE: normal\nREASON: bigger than it looked", "a3").unwrap();
        assert_eq!(rec.fields["route"]["size"], "normal");
        // A report on a card outside the session's stack isn't recorded.
        let other = mk(&mut s, "Planning");
        assert!(record(&mut s, &other, &format!("CARD: #{}\nROUTE: small", c.number), "a4").is_none());
        // No ROUTE line: logged as unclear.
        let blank = mk(&mut s, "Planning");
        assert!(record(&mut s, &blank, "I couldn't tell.", "a5").is_none());
        assert!(s.log(&blank.id).unwrap().iter().any(|l| l.text.starts_with("Route unclear:")));
        assert!(again_message(&c).contains("subagent_type \"card-router\""));
    }

    #[test]
    fn a_critique_from_before_the_route_doesnt_count() {
        let (_d, mut s) = board();
        let c = mk(&mut s, "Planning");
        // A pass from before the route (Route again, or a plan from before the router): no move.
        let c = s.update(&c.id, c.version, &json!({ "critique": { "state": "passed", "at": "2000-01-01T00:00:00.000Z" } }).as_object().cloned().unwrap(), "test").unwrap();
        let c = record(&mut s, &c, "ROUTE: small\nREASON: a typo", "a1").unwrap();
        assert!(s.waiting(&c.id, "move").unwrap().is_none());
        assert_eq!(go_on_blocker(&s, &c, false), Some("its plan critique passed before the route, so the plan needs a fresh critique"));
        // A fresh pass after the route: it goes on.
        let c = s.update(&c.id, c.version, &json!({ "critique": { "state": "passed", "at": "2999-01-01T00:00:00.000Z" } }).as_object().cloned().unwrap(), "test").unwrap();
        assert!(go_on_if_ready(&mut s, &c));
    }

    #[test]
    fn the_apps_waiting_move_is_dropped_if_things_changed() {
        let (_d, mut s) = board();
        let c = routed(&mut s, "small", "passed");
        assert!(go_on_if_ready(&mut s, &c));
        // Hugh dragged it away before the turn ended.
        let moved = s.update(&c.id, c.version, &json!({ "column": "Blocked" }).as_object().cloned().unwrap(), "Hugh (board)").unwrap();
        assert_eq!(go_on_blocker(&s, &moved, true), Some("it isn't in Planning any more"));
        let back = s.update(&c.id, moved.version, &json!({ "column": "Planning" }).as_object().cloned().unwrap(), "Hugh (board)").unwrap();
        // Route again cleared it.
        let cleared = s.update(&c.id, back.version, &json!({ "route": null }).as_object().cloned().unwrap(), HUGH).unwrap();
        assert!(go_on_blocker(&s, &cleared, true).unwrap().contains("no route"));
        // Hugh turned the rule off.
        let c2 = routed(&mut s, "small", "passed");
        s.set_page_setting(kanban_store::SMALL_GO_ON_KEY, "off").unwrap();
        assert_eq!(go_on_blocker(&s, &c2, true), Some("Hugh's rule for small cards is off"));
    }

    #[test]
    fn a_session_left_on_a_cleared_routes_values_goes_back() {
        // Route again leaves the old values in routeCleared: the session goes back to the column's.
        let (_d, s) = board();
        let now = card(json!({ "title": "T", "route": null, "routeCleared": { "model": "claude-sonnet-5-5", "effort": "low" } }), "Planning", 1);
        let set = settings_for(&s, &now);
        assert_eq!(set.model_from, "column");
        assert_eq!(reconcile(&now, &set, (Some("claude-sonnet-5-5"), Some("low"))), (Some("claude-opus-5-5".into()), Some("high".into())));
        assert!(switch_line(&now, &set, Some("claude-opus-5-5"), None).contains("Route again cleared the route"));
        // A session on other values is left alone.
        assert_eq!(reconcile(&now, &set, (Some("claude-haiku-5-5"), Some("medium"))), (None, None));
        // No route and no cleared route: never touched.
        let plain = card(json!({ "title": "T", "route": null }), "Planning", 1);
        assert_eq!(reconcile(&plain, &settings_for(&s, &plain), (Some("claude-sonnet-5-5"), Some("low"))), (None, None));
    }

    fn sub(id: &str, finished: bool, stopped: bool, updated: Option<&str>) -> crate::transcript::Subagent {
        crate::transcript::Subagent { id: id.into(), agent_type: "card-tester".into(), description: "Test".into(), tool_id: None, finished, stopped, started: Some("2026-11-06T05:00:00Z".into()), updated: updated.map(str::to_string) }
    }

    #[test]
    fn a_restart_waits_for_a_running_subagent() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-06T05:10:00Z").unwrap().with_timezone(&chrono::Utc);
        // A background card-tester still writing: the session isn't restarted.
        assert!(subagent_running(&[sub("a1", false, false, Some("2026-11-06T05:09:30Z"))], now));
        // Finished, stopped, or none at all: the restart goes ahead.
        assert!(!subagent_running(&[sub("a1", true, false, Some("2026-11-06T05:09:30Z"))], now));
        assert!(!subagent_running(&[sub("a1", false, true, Some("2026-11-06T05:09:30Z"))], now));
        assert!(!subagent_running(&[], now));
        // Quiet past the monitor's quiet time: dead, so it doesn't hold the restart for ever.
        assert!(!subagent_running(&[sub("a1", false, false, Some("2026-11-06T04:40:00Z"))], now));
    }

    #[test]
    fn nothing_is_typed_into_a_session_while_it_restarts() {
        // The ping-pong bug (#187, #188): the In progress prompt was typed while the restart was
        // under way, then lost with the old session. While restarting, the dispatcher skips the
        // card and the chat leaves its message queued for the resumed session.
        let id = "card-pingpong-test";
        assert!(!restarting(id));
        assert!(begin_restart(id));
        assert!(restarting(id), "typing is held from the decision on");
        assert!(!begin_restart(id), "one restart at a time");
        end_restart(id);
        assert!(!restarting(id), "the resumed session gets its queued message");
        assert!(begin_restart(id));
        end_restart(id);
    }

    #[test]
    fn a_new_route_removes_the_cleared_marker() {
        let (_d, mut s) = board();
        let c = mk(&mut s, "Planning");
        let c = s.update(&c.id, c.version, &json!({ "routeCleared": { "model": "claude-sonnet-5-5" } }).as_object().cloned().unwrap(), HUGH).unwrap();
        let c = record(&mut s, &c, "ROUTE: normal\nMODEL: claude-opus-5-5", "a1").unwrap();
        assert!(c.fields.get(CLEARED_KEY).map_or(true, Value::is_null));
        assert_eq!(c.fields["route"]["size"], "normal");
    }

    #[test]
    fn the_router_runs_at_low_effort_and_its_prompt_is_short() {
        assert_eq!(AGENT.effort, Some("low"));
        assert!(PROMPT.len() < 2_500, "the prompt is {} characters", PROMPT.len());
        for line in ["CARD:", "ROUTE:", "MODEL:", "EFFORT:", "CHECK:", "REASON:"] {
            assert!(PROMPT.contains(line), "{line}");
        }
        let v: Value = serde_json::from_str(&crate::agents::agents_json()).unwrap();
        assert_eq!(v[ROUTER]["effort"], "low");
        assert_eq!(v[ROUTER]["model"], "inherit");
        // The prompt's suggestions are models the app can apply.
        for m in ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"] {
            assert!(valid_model(m), "{m}");
        }
    }
}
