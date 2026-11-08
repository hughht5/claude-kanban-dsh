//! The app's own subagents (Hugh, cards #76 and #96). Every card session gets them through
//! `--agents` (`sessions.rs` `build_args`), and the app hears when one finishes (`SubagentStop`,
//! an `http` hook). This module holds the one table of them (`AGENTS`), what none of them may
//! use (`denied`), the verdict code the PASS/FAIL ones share (`Check`), and the moves their
//! verdicts hold (`GATES`). Each agent's own text and rules live in its own module
//! (`tester.rs`, `reviewer.rs`, ...): a new one adds a line to `AGENTS`, and to `GATES` if its
//! verdict holds a move.
//!
//! Every agent keeps its shells (Bash and PowerShell), because it has to run things, so the deny
//! list is a guard rail and not a fence: "change nothing" rests on each prompt, and
//! `configwatch.rs` reports any change to Claude Code's own config while one of them runs. The
//! list names whole tools only. A rule with a specifier, such as `Bash(kb:*)`, takes the whole
//! tool away in an agent's `disallowedTools` (card #100's live trial, Claude Code 2.1.318): it
//! left the agents with PowerShell alone, where `kb` wasn't blocked at all. Blocking commands
//! needs a `PreToolUse` hook that can tell a subagent's call (card #100's guard).

use crate::{sessions::Sessions, Board};
use kanban_store::{Card, Patch, Store};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

/// One of the app's subagents, as `--agents` defines it.
pub struct Agent {
    /// Its name, as the session calls it (`subagent_type`) and the hook reports it.
    pub name: &'static str,
    pub description: &'static str,
    pub prompt: &'static str,
    /// Its own reasoning effort ("low", "high"); None runs at the session's.
    pub effort: Option<&'static str>,
    /// Whether it may edit files. Only an agent whose job is changing them: the card-merger,
    /// which resolves conflicts (card #100).
    pub edits: bool,
    /// What else it may not use, beyond `denied`'s common list.
    pub also_denied: &'static [&'static str],
    /// What the app does when it finishes.
    pub on_stop: OnStop,
}

/// What the app does with a finished agent's report.
pub enum OnStop {
    /// Records its PASS/FAIL verdict on the card it names.
    Check(&'static Check),
    /// Its own handler: (app, the session's row id, the hook's body). For an agent whose report
    /// isn't a PASS/FAIL (the router and the reproducer, cards #98 and #112).
    Custom(fn(&AppHandle, &str, &Value)),
}

/// A subagent whose report ends with CARD and VERDICT PASS|FAIL lines, recorded on the card.
pub struct Check {
    pub agent: &'static str,
    /// The card field holding its latest verdict: `{state, at, agentId, detail, round}`, where
    /// state is "needed", "passed", "failed" or "unclear".
    pub key: &'static str,
    /// Who the verdicts are recorded as.
    pub by: &'static str,
    /// The log line's start: "Subagent test".
    pub label: &'static str,
    /// Who it is and what it does: ("tester", "test") reads "the tester said PASS but ran no
    /// tools, so it didn't test anything".
    pub who: &'static str,
    pub verb: &'static str,
    /// The column whose every entry resets the field to "needed" (`kanban_store` does that).
    pub reset_in: &'static str,
    /// Its FAIL this many times since that reset sends the card to Blocked (Hugh's rule).
    pub cap: Option<u64>,
    /// Whether the app takes the shots its report names in `EVIDENCE:` lines (card #101): copied
    /// in by `evidence.rs` and put on the card's `evidence` field with the verdict.
    pub evidence: bool,
}

/// Every subagent a card session gets. One line each.
pub const AGENTS: &[&Agent] = &[
    &crate::tester::AGENT,
    &crate::reviewer::AGENT,
    &crate::critic::AGENT,
    &crate::router::AGENT,
    &crate::repro::AGENT,
    &crate::merger::AGENT,
];

/// A rule that may refuse a card session's move: (store, the card, the column it would go to).
pub type Gate = fn(&Store, &Card, &str) -> Option<String>;

/// The rules a card session's move is held to, in the order a refusal names them. One line each.
pub const GATES: &[Gate] = &[
    crate::critic::gate,
    crate::repro::gate,
    crate::reviewer::gate,
    crate::tester::gate,
];

/// The `kanban` tools that change cards. No subagent may use them, under either server name:
/// the app's own (`kanban`, from `--mcp-config`) and the repo's (`kanban-app`, from
/// kanban-app's `.mcp.json`, which a card session in that checkout also loads).
const CARD_TOOLS: [&str; 10] = [
    // The Router's ask to archive a card (card #131): never its subagents'.
    "kanban_archive_card",
    "kanban_move_card",
    "kanban_update_card",
    "kanban_create_card",
    "kanban_ship_result",
    "kanban_set_passed",
    "kanban_stack_add",
    "kanban_stack_remove",
    "kanban_stack_join",
    "kanban_stack_detach",
];
const SERVERS: [&str; 2] = ["kanban", "kanban-app"];
const EDIT_TOOLS: [&str; 3] = ["Edit", "Write", "NotebookEdit"];

/// What `agent` may not use: whole tools only (see the module's note on specifiers).
pub fn denied(agent: &Agent) -> Vec<String> {
    let mut d: Vec<String> = if agent.edits { vec![] } else { EDIT_TOOLS.iter().map(|t| t.to_string()).collect() };
    for s in SERVERS {
        d.extend(CARD_TOOLS.iter().map(|t| format!("mcp__{s}__{t}")));
    }
    d.extend(agent.also_denied.iter().map(|t| t.to_string()));
    d
}

/// The `--agents` JSON for a card session: every agent, on the session's own model.
pub fn agents_json() -> String {
    let mut m = serde_json::Map::new();
    for a in AGENTS {
        let mut def = json!({ "description": a.description, "prompt": a.prompt, "disallowedTools": denied(a), "model": "inherit" });
        if let Some(e) = a.effort {
            def["effort"] = json!(e);
        }
        m.insert(a.name.into(), def);
    }
    Value::Object(m).to_string()
}

/// The agent a hook's `agent_type` names: its name, or `<plugin>:<name>`.
pub fn find(agent_type: &str) -> Option<&'static Agent> {
    let name = agent_type.rsplit(':').next().unwrap_or("");
    AGENTS.iter().copied().find(|a| a.name == name)
}

/// Why a card's session may not move `card` to `to`, if any rule in `GATES` refuses it. Only
/// card sessions are held to them: Hugh's own moves, and his own sessions' `kb move`, aren't.
pub fn move_refused(store: &Store, card: &Card, to: &str) -> Option<String> {
    let why: Vec<String> = GATES.iter().filter_map(|g| g(store, card, to)).collect();
    (!why.is_empty()).then(|| why.join("\n\n"))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// The card it names (`CARD: #12`), if any.
    pub card: Option<i64>,
    /// None when there is no VERDICT line.
    pub passed: Option<bool>,
    pub failures: Vec<String>,
    /// The last `SCOPE: full|narrowed` line, lower case (the reviewer's, card #104).
    pub scope: Option<String>,
}

/// Reads an agent's closing lines: the last `CARD: #n` and the last `VERDICT: PASS|FAIL`
/// (any case, markdown bold or backticks around them allowed), and the `- ` lines after a FAIL.
pub fn parse_verdict(text: &str) -> Verdict {
    let clean = |l: &str| l.trim().trim_matches(|c| c == '*' || c == '`' || c == '_').trim().to_string();
    let mut v = Verdict { card: None, passed: None, failures: vec![], scope: None };
    let mut after_fail = false;
    for raw in text.lines() {
        let l = clean(raw);
        let up = l.to_ascii_uppercase();
        if let Some(rest) = up.strip_prefix("CARD:") {
            let digits: String = rest.trim_start_matches(|c: char| c == '*' || c == '`' || c == '_' || c == '#' || c.is_whitespace()).chars().take_while(char::is_ascii_digit).collect();
            if let Ok(n) = digits.parse() {
                v.card = Some(n);
            }
            after_fail = false;
        } else if let Some(rest) = up.strip_prefix("SCOPE:") {
            let word: String = rest.trim().trim_matches(|c| c == '*' || c == '`').trim().chars().take_while(char::is_ascii_alphabetic).collect();
            if word == "FULL" || word == "NARROWED" {
                v.scope = Some(word.to_ascii_lowercase());
            }
            after_fail = false;
        } else if let Some(rest) = up.strip_prefix("VERDICT:") {
            let word: String = rest.trim().trim_matches(|c| c == '*' || c == '`').trim().chars().take_while(char::is_ascii_alphabetic).collect();
            if word == "PASS" {
                v.passed = Some(true);
                v.failures.clear();
                after_fail = false;
            } else if word == "FAIL" || word == "FAILED" {
                v.passed = Some(false);
                v.failures.clear();
                after_fail = true;
            }
        } else if after_fail {
            if let Some(f) = raw.trim().strip_prefix("- ").or_else(|| raw.trim().strip_prefix("* ")) {
                v.failures.push(f.trim().to_string());
            }
        }
    }
    v
}

/// How many tool calls the subagent's transcript records, if it can be read: an agent that ran
/// none didn't look at anything.
pub fn tool_calls(path: &str) -> Option<usize> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(text.lines().map(|l| l.matches("\"type\":\"tool_use\"").count()).sum())
}

/// What the app records for `check`'s verdict: the field's value and the log line. `round` is
/// how many FAILs the field had counted before this one.
pub fn record_of(check: &Check, v: &Verdict, tools: Option<usize>, agent_id: &str, at: &str, round: u64) -> (Value, String) {
    let label = check.label;
    let (state, detail, line) = match (v.passed, tools) {
        (Some(true), Some(0)) => ("unclear", format!("the {} said PASS but ran no tools, so it didn't {} anything", check.who, check.verb), None),
        (Some(true), _) => ("passed", String::new(), Some(format!("{label}: PASS"))),
        (Some(false), _) => {
            let d = if v.failures.is_empty() { format!("see the {}'s report in the session", check.who) } else { v.failures.join("; ") };
            ("failed", d.clone(), Some(format!("{label}: FAIL: {d}")))
        }
        (None, _) => ("unclear", format!("the {} ended with no VERDICT line, so it isn't a pass", check.who), None),
    };
    // A report may name or quote a secret ("a password in config.rs"): the board refuses such
    // text, which would lose the verdict, and must never hold a token, so it is masked.
    let detail: String = kanban_store::mask_secrets(&detail).chars().take(600).collect();
    let line = line.map(|l| kanban_store::mask_secrets(&l)).unwrap_or_else(|| format!("{label} unclear: {detail}"));
    let line: String = line.chars().take(700).collect();
    let round = if state == "failed" { round + 1 } else { round };
    let mut rec = json!({ "state": state, "at": at, "agentId": agent_id, "round": round });
    if !detail.is_empty() {
        rec["detail"] = json!(detail);
    }
    (rec, line)
}

/// Where a verdict was given (card #104): the checkout the app looked at, and its HEAD and status
/// hash then. With the tool count and the reviewer's scope, it lets the app tell later whether
/// the work is still exactly what was checked.
pub fn add_place(rec: &mut Value, tools: Option<usize>, place: Option<(&str, &crate::small::Snapshot)>, scope: Option<&str>) {
    if let Some(n) = tools {
        rec["tools"] = json!(n);
    }
    if let Some((checkout, snap)) = place {
        rec["checkout"] = json!(checkout);
        rec["head"] = json!(snap.head);
        rec["statusHash"] = json!(snap.status_hash);
    }
    if let Some(s) = scope {
        rec["scope"] = json!(s);
    }
}

/// The checkout a card's verdict is placed in: the one Hugh listed for its repo, the one its
/// small-change ship would push. None for any other card, so no git runs while the list is
/// empty (the default).
pub fn checkout_of(store: &Store, card: &Card) -> Option<String> {
    let repo = card.fields.get("repo").and_then(Value::as_str).unwrap_or("");
    store.small_repo_for(repo).map(|e| e.checkout).filter(|p| !p.trim().is_empty())
}

/// The work as it was when each of the app's PASS/FAIL agents started (card #104): per session
/// row and agent, the checkout and its snapshot, oldest first. A verdict counts for the work
/// only when it is still the same when the agent stops, so a commit made while it ran never
/// gets its pass.
static STARTS: std::sync::Mutex<Option<std::collections::HashMap<(String, String), Vec<(String, crate::small::Snapshot)>>>> = std::sync::Mutex::new(None);

/// A `PreToolUse` hook: an `Agent` (or `Task`) call starting one of the app's PASS/FAIL agents,
/// on a card in a listed repo, takes a snapshot of the work.
pub fn on_pre_tool(app: &AppHandle, row_id: &str, body: &Value) {
    let tool = body.get("tool_name").and_then(Value::as_str).unwrap_or("");
    let kind = body.pointer("/tool_input/subagent_type").and_then(Value::as_str).unwrap_or("");
    let Some(agent) = find(kind) else { return };
    if !matches!(tool, "Agent" | "Task") || !matches!(agent.on_stop, OnStop::Check(_)) {
        return;
    }
    // The Router's brief names its card ("CARD: <n>", card #119): the snapshot is of that card's checkout.
    let named = body.pointer("/tool_input/prompt").and_then(Value::as_str).and_then(|p| parse_verdict(p).card);
    let checkout = app.state::<Board>().0.lock().ok().and_then(|s| own_or_named(app, &s, row_id, named).and_then(|c| checkout_of(&s, &c)));
    let Some(checkout) = checkout else { return };
    let Ok(snap) = crate::small::snapshot(&checkout) else { return };
    if let Ok(mut g) = STARTS.lock() {
        g.get_or_insert_with(Default::default).entry((row_id.to_string(), agent.name.to_string())).or_default().push((checkout, snap));
    }
}

/// The oldest start snapshot of `agent` in session `row_id`, taken off the list.
fn take_start(row_id: &str, agent: &str) -> Option<(String, crate::small::Snapshot)> {
    let mut g = STARTS.lock().ok()?;
    let list = g.as_mut()?.get_mut(&(row_id.to_string(), agent.to_string()))?;
    (!list.is_empty()).then(|| list.remove(0))
}

/// Where a verdict counts (card #104): the checkout, when the work was the same when its agent
/// started (`start`) and when it stopped (`stop`). None otherwise: no start seen, a different
/// checkout, or the work moved while it ran.
pub fn place_of<'a>(start: Option<&'a (String, crate::small::Snapshot)>, checkout: Option<&str>, stop: Option<&crate::small::Snapshot>) -> Option<(&'a str, &'a crate::small::Snapshot)> {
    let (c, s) = start?;
    (Some(c.as_str()) == checkout && Some(s) == stop).then_some((c.as_str(), s))
}

/// Whether a record on a card in `column` has reached `check`'s cap: its FAIL rounds since the
/// reset. Only while the card is in the column whose entry resets the count, so a FAIL after
/// Hugh has moved a capped card on elsewhere doesn't send it straight back to Blocked.
pub fn capped(check: &Check, rec: &Value, column: &str) -> bool {
    column == check.reset_in
        && rec.get("state").and_then(Value::as_str) == Some("failed")
        && check.cap.is_some_and(|cap| rec.get("round").and_then(Value::as_u64).unwrap_or(0) >= cap)
}

/// Keeps a cap that has fired (`capped: true`) on the newer record: only the reset, a move into
/// the check's column, clears it. Without this a later PASS would let a capped card through.
pub fn carry_cap(prev: Option<&Value>, rec: &mut Value) {
    if prev.and_then(|p| p.get("capped")).and_then(Value::as_bool) == Some(true) {
        rec["capped"] = json!(true);
    }
}

/// A capped card's rule (card #96): while `check`'s cap has fired and the card is in a column
/// before Human test, a card session may not move it into the check's column (that would reset
/// the cap) or on into Human test or Deploy complete. Hugh moves it on; his move into the check's
/// column resets the record, and with it the cap. Once he has moved it past those columns, the
/// session's own moves are free again (a Ship it session's move to Deploy complete, say).
pub fn capped_hold(check: &Check, card: &Card, to: &str, required: bool) -> Option<String> {
    let capped = card.fields.get(check.key).and_then(|r| r.get("capped")).and_then(Value::as_bool) == Some(true);
    let from_before = crate::tester::BEFORE_TEST.contains(&card.column.as_str());
    let onward = to == check.reset_in || crate::tester::INTO_TEST.contains(&to);
    (required && capped && from_before && onward).then(|| {
        format!(
            "Card #{n}'s {who} failed {cap} times since it last came into {col} (Hugh's rule). It goes to Blocked with the findings as its blocked reason. Do not move it anywhere else. Only Hugh moves it on from Blocked.",
            n = card.number,
            who = check.who,
            cap = check.cap.unwrap_or(0),
            col = check.reset_in
        )
    })
}

/// The card a verdict is for: the one it names, if that is the session's card or a card in
/// the stack the session's card leads; else the session's own card when it names none.
pub fn target_card(store: &Store, own: &Card, named: Option<i64>) -> Result<Card, String> {
    match named {
        None => Ok(own.clone()),
        Some(n) if n == own.number => Ok(own.clone()),
        Some(n) => {
            let c = store.get_by_number(n).map_err(|e| e.to_string())?;
            if c.stack_of() == Some(own.number) {
                Ok(c)
            } else {
                Err(format!("#{n} isn't this card or a card in its stack"))
            }
        }
    }
}

/// The card whose session `row_id` is.
pub fn own_card(app: &AppHandle, store: &Store, row_id: &str) -> Option<Card> {
    let id = app.state::<Arc<Sessions>>().card_of_row(row_id).or_else(|| store.session_card(row_id).ok().flatten())?;
    store.get(&id).ok()
}

/// The card a subagent's report or brief is for, as `own_card` gives it: the session's card; for
/// the Router's session (card #119, finding 13), the card it names (`named`), and that card's
/// app writes for it don't wake the Router.
pub fn own_or_named(app: &AppHandle, store: &Store, row_id: &str, named: Option<i64>) -> Option<Card> {
    if crate::routersession::is_router_row_in(app, store, row_id) {
        let n = named?;
        crate::routersession::note_own_verdict(app, n);
        return store.get_by_number(n).ok();
    }
    own_card(app, store, row_id)
}

/// A `SubagentStop` hook from session `row_id`: hands it to the agent it names, if it is one
/// of the app's.
pub fn on_subagent_stop(app: &AppHandle, row_id: &str, body: &Value) {
    let Some(agent) = find(body.get("agent_type").and_then(Value::as_str).unwrap_or("")) else { return };
    match &agent.on_stop {
        OnStop::Check(check) => record_check(app, row_id, body, check),
        OnStop::Custom(f) => f(app, row_id, body),
    }
}

/// Records a PASS/FAIL agent's verdict on the card it names. At its cap, the card goes to
/// Blocked with the findings as `blockedReason`: the session's own card when its turn ends
/// (a held move), a child of its stack at once. A tester's shots (card #101) are copied in only
/// once that card is known, with the board lock let go, and go on the card in the verdict's write.
fn record_check(app: &AppHandle, row_id: &str, body: &Value, check: &Check) {
    let text = body.get("last_assistant_message").and_then(Value::as_str).unwrap_or("");
    let agent_id = body.get("agent_id").and_then(Value::as_str).unwrap_or("");
    let tools = body.get("agent_transcript_path").and_then(Value::as_str).and_then(tool_calls);
    // Card #105: a tester's CHECK lines (one per acceptance check), read before the board lock.
    let results = if check.agent == crate::tester::TESTER { crate::done_when::check_lines(text) } else { Vec::new() };
    // Taken first, so a report that isn't recorded doesn't leave its start for the next one.
    let start = take_start(row_id, check.agent);
    let board = app.state::<Board>();
    // Where the verdict was given (card #104): found under the lock, read from git without it.
    let checkout = board.0.lock().ok().and_then(|s| {
        let named = parse_verdict(text).card;
        let own = own_or_named(app, &s, row_id, named)?;
        let card = target_card(&s, &own, named).ok()?;
        checkout_of(&s, &card)
    });
    let snap = checkout.as_deref().filter(|_| start.is_some()).and_then(|c| crate::small::snapshot(c).ok());
    let mut v = parse_verdict(text);
    let board = app.state::<Board>();
    // First, under a short lock: the session's card and the card the verdict is for. Nothing is
    // copied until both are known.
    let (own, target_id) = {
        let Ok(mut store) = board.0.lock() else { return };
        let Some(own) = own_or_named(app, &store, row_id, v.card) else { return };
        match target_card(&store, &own, v.card) {
            Ok(c) => (own, c.id),
            Err(why) => {
                let _ = store.add_log(&own.id, &format!("Not recorded: the {} subagent reported on {why}", check.agent), check.by);
                if check.evidence && !crate::evidence::parse(text).is_empty() {
                    let line = format!("Evidence not taken: the verdict wasn't recorded ({why})");
                    let _ = store.add_log(&own.id, &kanban_store::mask_secrets(&line), check.by);
                }
                return;
            }
        }
    };
    // One `at` for the verdict, its run and the run's file names.
    let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    // The shots its report names are read, checked and copied with no lock held.
    let shots = check.evidence.then(|| crate::evidence::prepare(&kanban_store::data_dir(), &std::env::temp_dir(), agent_id, &at, text));
    let Ok(mut store) = board.0.lock() else { return };
    // The card as it is now (its version and fields may have moved on while the files were
    // copied), and still the session's card or one in its stack.
    let card = match store.get(&target_id) {
        Ok(c) if c.id == own.id || c.stack_of() == Some(own.number) => c,
        Ok(c) => {
            let why = format!("#{} isn't this card or a card in its stack any more", c.number);
            let _ = store.add_log(&own.id, &format!("Not recorded: the {} subagent reported on {why}", check.agent), check.by);
            evidence_lost(&mut store, &own.id, shots.as_ref(), &why, check.by);
            return;
        }
        Err(e) => {
            let _ = store.add_log(&own.id, &format!("Couldn't record the {} subagent's verdict: {e}", check.agent), check.by);
            evidence_lost(&mut store, &own.id, shots.as_ref(), &format!("the card couldn't be read: {e}"), check.by);
            return;
        }
    };
    pass_fails_on_checks(&card, &mut v, &results);
    let before = card.fields.get(check.key).and_then(|t| t.get("round")).and_then(Value::as_u64).unwrap_or(0);
    let (mut rec, line) = record_of(check, &v, tools, agent_id, &at, before);
    let place = place_of(start.as_ref(), checkout.as_deref(), snap.as_ref());
    if start.is_some() && place.is_none() {
        // The work changed while it ran: the verdict stands, but it can't let the card go on.
        rec["placeMoved"] = json!(true);
    }
    add_place(&mut rec, tools, place, v.scope.as_deref());
    let prev = card.fields.get(check.key);
    let already = prev.and_then(|p| p.get("capped")).and_then(Value::as_bool) == Some(true);
    carry_cap(prev, &mut rec);
    // The cap fires once; from then on `capped_hold` keeps the card from going anywhere but Blocked.
    let cap = !already && capped(check, &rec, &card.column);
    if cap {
        rec["capped"] = json!(true);
    }
    let mut patch = Patch::new();
    patch.insert(check.key.into(), rec.clone());
    let why = cap.then(|| {
        let d = rec.get("detail").and_then(Value::as_str).unwrap_or("");
        let why: String = format!("The {} failed {} times since #{} last came into {}: {d}", check.who, check.cap.unwrap_or(0), card.number, check.reset_in).chars().take(900).collect();
        why
    });
    if let Some(w) = &why {
        patch.insert("blockedReason".into(), json!(w));
    }
    // The run goes in the same write as the verdict it backs, on the same card, with its `at`.
    let (run, off) = match shots.as_ref().filter(|p| !p.shots.is_empty()) {
        Some(p) => {
            let run = evidence_run(agent_id, &at, &rec, p);
            // Never lose the verdict over its shots: a run that isn't this verdict's (another
            // `at`: `has_tested_evidence` matches them), or one the board would refuse, stays off.
            if !same_at(&run, &rec) {
                (None, Some("its time isn't the verdict's"))
            } else if kanban_store::looks_secret(&run.to_string()) {
                (None, Some("its text looks like it holds a secret"))
            } else {
                patch.insert(kanban_store::EVIDENCE_KEY.into(), kanban_store::add_run(&card, run.clone()));
                (Some(run), None)
            }
        }
        None => (None, None),
    };
    let card = match store.update(&card.id, card.version, &patch, check.by) {
        Ok(c) => c,
        Err(e) => {
            // Both lines on the card the verdict was for.
            let _ = store.add_log(&card.id, &format!("Couldn't record the {} subagent's verdict: {e}", check.agent), check.by);
            evidence_lost(&mut store, &card.id, shots.as_ref(), &format!("the card's write failed: {e}"), check.by);
            return;
        }
    };
    let _ = store.add_log(&card.id, &line, check.by);
    if let Some(run) = &run {
        let n = run.get("shots").and_then(Value::as_array).map_or(0, Vec::len);
        let build = run.get("build").and_then(Value::as_str).map(|b| format!(", build {b}")).unwrap_or_default();
        let _ = store.add_log(&card.id, &format!("Evidence: {n} shot(s) from the {}{build}", check.agent), check.by);
    } else if let Some(off) = off {
        evidence_lost(&mut store, &card.id, shots.as_ref(), off, check.by);
    }
    if let Some(p) = shots.as_ref().filter(|_| off.is_none()) {
        for r in &p.refused {
            let _ = store.add_log(&card.id, &format!("Evidence not taken: {r}"), check.by);
        }
    }
    let card = mark_checks(&mut store, card, &rec, &results, check.by);
    // A stack held back for its lead's own verdict may be ready now.
    if card.is_lead() && rec.get("state").and_then(Value::as_str) == Some("passed") {
        crate::dispatch::after_lead_verdict(app, &mut store, &card.id, check.by, &format!("#{}'s own {} passed", card.number, check.verb));
    }
    // A pass may be the last thing a card routed small waits for (card #98: its plan critique).
    crate::router::go_on_if_ready(&mut store, &card);
    if let Some(w) = why {
        let reason = format!("its {} failed {} times (Hugh's rule)", check.who, check.cap.unwrap_or(0));
        let res = if card.id == own.id {
            store.queue(&card.id, "move", &json!({ "column": "Blocked", "reason": reason }).to_string()).map(|_| ()).map_err(|e| e.to_string())
        } else {
            crate::dispatch::session_move(app, &mut store, &card, "Blocked", &reason, check.by)
        };
        let _ = match res {
            Ok(()) => store.add_log(&card.id, &format!("Going to Blocked: {w}"), check.by),
            Err(e) => store.add_log(&card.id, &format!("Couldn't move it to Blocked after the {}'s FAILs: {e}", check.who), check.by),
        };
        // The session hears it, so it stops rather than fixing and trying again.
        let note = format!(
            "Card #{n}'s {who} failed {cap} times since it last came into {col} (Hugh's rule). The app moves the card to Blocked, with the findings as its blocked reason{when}. Stop work on it. Do not move it anywhere else: only Hugh moves it on from Blocked.",
            n = card.number,
            who = check.who,
            cap = check.cap.unwrap_or(0),
            col = check.reset_in,
            when = if card.id == own.id { ", when your turn ends" } else { "" }
        );
        let _ = store.queue(&own.id, "message", &note);
    }
    // A tester's FAIL line that names a bug a person can see ("- BUG: ...") is a bug report for
    // the card-reproducer (card #112): the tester found it in the installed app, so it can be
    // reproduced there. Not the reviewer's: it reads the diff before the install. The session
    // sees the FAIL itself; the hold tells it the rest.
    let failed = rec.get("state").and_then(Value::as_str) == Some("failed");
    let bugs = (failed && check.agent == crate::tester::TESTER).then(|| crate::repro::bug_lines(&v.failures)).flatten();
    if let (Some(bugs), Ok(now)) = (bugs, store.get(&card.id)) {
        if let Err(e) = crate::repro::mark_needed(&mut store, &now, check.agent, &bugs) {
            let _ = store.add_log(&card.id, &format!("Couldn't set the card's repro to needed after the {}'s FAIL: {e}", check.who), check.by);
        }
    }
    drop(store);
    let _ = app.emit("cards-changed", ());
}

/// The run that goes first in a card's `evidence` (card #101): the agent's id, the verdict's
/// `at` and state, the shots copied in, and the build they show (`run_build`).
fn evidence_run(agent_id: &str, at: &str, rec: &Value, p: &crate::evidence::Prepared) -> Value {
    let mut run = json!({ "run": agent_id, "at": at, "verdict": rec.get("state").cloned().unwrap_or(Value::Null), "shots": p.shots });
    if let Some(b) = run_build(&p.shots) {
        run["build"] = json!(b);
    }
    run
}

/// Whether a run and the verdict record it goes with have the same `at` (a string on both).
fn same_at(run: &Value, rec: &Value) -> bool {
    run.get("at").and_then(Value::as_str).is_some_and(|a| Some(a) == rec.get("at").and_then(Value::as_str))
}

/// The build a run's shots show: their common `build`, "mixed" when they differ (a shot with
/// none counts as another build), None when none has one.
fn run_build(shots: &[Value]) -> Option<String> {
    let mut builds = shots.iter().map(|s| s.get("build").and_then(Value::as_str));
    let first = builds.next()?;
    if builds.all(|b| b == first) {
        first.map(str::to_string)
    } else {
        Some("mixed".into())
    }
}

/// The log lines for shots that went on no card: why, where the copied files stay, and each
/// file that was refused. Masked, so the board takes them.
fn evidence_lost(store: &mut Store, card_id: &str, p: Option<&crate::evidence::Prepared>, why: &str, by: &str) {
    let Some(p) = p else { return };
    if !p.shots.is_empty() {
        let line = format!("Evidence not taken: {why}. The {} shot(s) copied to {} are on no card.", p.shots.len(), p.folder);
        let _ = store.add_log(card_id, &kanban_store::mask_secrets(&line), by);
    }
    for r in &p.refused {
        let _ = store.add_log(card_id, &format!("Evidence not taken: {r}"), by);
    }
}

/// Card #105: a tester's PASS whose CHECK lines fail one of the card's acceptance checks is a FAIL,
/// so the move to Human test is held. The app doesn't take the tester's word for it.
pub fn pass_fails_on_checks(card: &Card, v: &mut Verdict, results: &[crate::done_when::CheckResult]) {
    if v.passed != Some(true) {
        return;
    }
    let failed = crate::done_when::failed_checks(card, results);
    if !failed.is_empty() {
        v.passed = Some(false);
        v.failures = vec![format!("the tester said PASS, but {}", failed.join("; "))];
    }
}

/// Card #105: marks `card`'s acceptance checks from a tester's CHECK lines, for a real passed or
/// failed verdict (`rec`), in a write of its own after the verdict's, so a list the store refuses
/// can't lose the verdict. Returns the card as it now is.
pub fn mark_checks(store: &mut Store, card: Card, rec: &Value, results: &[crate::done_when::CheckResult], by: &str) -> Card {
    let Some((list, said)) = crate::done_when::checks_patch(&card, rec["state"].as_str().unwrap_or(""), results) else { return card };
    let mut p = Patch::new();
    p.insert(kanban_store::DONE_WHEN_CHECKS_KEY.into(), list);
    match store.update(&card.id, card.version, &p, by) {
        Ok(c) => {
            let _ = store.add_log(&c.id, &said, by);
            c
        }
        Err(e) => {
            // Masked: the store's own refusal text has a word its log filter refuses.
            let _ = store.add_log(&card.id, &format!("Couldn't mark the acceptance checks: {}", kanban_store::mask_secrets(&e.to_string())), by);
            card
        }
    }
}

/// A PASS/FAIL agent's rule for a move: `card` may go from any column in `before` into one of
/// `into` only once `check` has passed it, while `required`. None when it may; else what its
/// latest record says ("" when it was never run).
pub fn not_passed(check: &Check, card: &Card, to: &str, before: &[&str], into: &[&str], required: bool) -> Option<String> {
    if !required || !before.contains(&card.column.as_str()) || !into.contains(&to) {
        return None;
    }
    let rec = card.fields.get(check.key);
    let state = rec.and_then(|t| t.get("state")).and_then(Value::as_str).unwrap_or("needed");
    let detail = rec.and_then(|t| t.get("detail")).and_then(Value::as_str);
    match state {
        "passed" => None,
        "failed" => Some(format!(" Its latest {} failed: {}.", check.verb, detail.unwrap_or("see the report"))),
        "unclear" => Some(format!(" Its latest {} didn't count: {}.", check.verb, detail.unwrap_or("no verdict"))),
        _ => Some(String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Check = Check { agent: "x-agent", key: "x", by: "the app", label: "Subagent x", who: "xer", verb: "x", reset_in: "In progress", cap: Some(3), evidence: false };

    #[test]
    fn verdict_lines_are_read_from_the_end_of_the_report() {
        let v = parse_verdict("Tested the form.\n\nCARD: #76\nVERDICT: PASS");
        assert_eq!(v, Verdict { card: Some(76), passed: Some(true), failures: vec![], scope: None });
        let v = parse_verdict("**CARD:** #12\n**VERDICT: FAIL**\n- the chip doesn't show\n- BUG: Esc doesn't close it\n");
        assert_eq!(v.card, Some(12));
        assert_eq!(v.passed, Some(false));
        assert_eq!(v.failures, vec!["the chip doesn't show", "BUG: Esc doesn't close it"]);
        // The last verdict wins (a report that quotes an earlier one).
        let v = parse_verdict("VERDICT: FAIL\n- x\nRe-ran it.\nCARD: 5\nverdict: pass");
        assert_eq!((v.card, v.passed, v.failures.len()), (Some(5), Some(true), 0));
        // No verdict line: unclear, never a pass.
        assert_eq!(parse_verdict("All looks fine to me.").passed, None);
        assert_eq!(parse_verdict("VERDICT: PASSABLE?").passed, None);
        assert_eq!(parse_verdict("VERDICT: PASS (all three)").passed, Some(true));
    }

    #[test]
    fn a_pass_with_no_tools_run_doesnt_count_and_fails_are_counted() {
        let pass = Verdict { card: None, passed: Some(true), failures: vec![], scope: None };
        assert_eq!(record_of(&T, &pass, Some(3), "a1", "t", 0).0["state"], "passed");
        assert_eq!(record_of(&T, &pass, None, "a1", "t", 0).0["state"], "passed"); // transcript unreadable: trust the verdict
        let (rec, line) = record_of(&T, &pass, Some(0), "a1", "t", 0);
        assert_eq!(rec["state"], "unclear");
        assert_eq!(line, "Subagent x unclear: the xer said PASS but ran no tools, so it didn't x anything");
        let fail = Verdict { card: None, passed: Some(false), failures: vec!["a".into(), "b".into()], scope: None };
        let (rec, line) = record_of(&T, &fail, Some(4), "a1", "t", 0);
        assert_eq!((rec["state"].as_str(), rec["detail"].as_str(), rec["round"].as_u64()), (Some("failed"), Some("a; b"), Some(1)));
        assert_eq!(line, "Subagent x: FAIL: a; b");
        assert!(!capped(&T, &rec, "In progress"));
        // The third FAIL since the reset reaches the cap; a pass keeps the count but isn't capped.
        let (rec, _) = record_of(&T, &fail, Some(4), "a1", "t", 2);
        assert_eq!(rec["round"], 3);
        assert!(capped(&T, &rec, "In progress"));
        // Not once the card is elsewhere (Hugh moved a capped card from Blocked to Planning).
        assert!(!capped(&T, &rec, "Planning") && !capped(&T, &rec, "Blocked"));
        let (rec, _) = record_of(&T, &pass, Some(4), "a1", "t", 2);
        assert_eq!(rec["round"], 2);
        assert!(!capped(&T, &rec, "In progress"));
        assert!(!capped(&Check { cap: None, ..T }, &record_of(&T, &fail, Some(1), "a", "t", 9).0, "In progress"));
        assert_eq!(record_of(&T, &Verdict { card: None, passed: None, failures: vec![], scope: None }, Some(2), "a", "t", 0).0["state"], "unclear");
    }

    #[test]
    fn the_scope_line_and_the_place_are_recorded() {
        let v = parse_verdict("Read it all.\nCARD: #104\n**SCOPE: full**\nVERDICT: PASS");
        assert_eq!(v.scope.as_deref(), Some("full"));
        assert_eq!(parse_verdict("SCOPE: Narrowed (README only)\nVERDICT: PASS").scope.as_deref(), Some("narrowed"));
        assert_eq!(parse_verdict("SCOPE: everything\nVERDICT: PASS").scope, None);
        assert_eq!(parse_verdict("VERDICT: PASS").scope, None);
        let (mut rec, _) = record_of(&T, &v, Some(7), "a1", "t", 0);
        let snap = crate::small::Snapshot { head: "abc".into(), status_hash: "e69de".into() };
        add_place(&mut rec, Some(7), Some((r"C:\repo", &snap)), v.scope.as_deref());
        assert_eq!((rec["tools"].as_u64(), rec["checkout"].as_str(), rec["head"].as_str(), rec["statusHash"].as_str(), rec["scope"].as_str()), (Some(7), Some(r"C:\repo"), Some("abc"), Some("e69de"), Some("full")));
        // Nothing known, nothing added; the verdict still counts as it did.
        let (mut rec, _) = record_of(&T, &v, None, "a1", "t", 0);
        add_place(&mut rec, None, None, None);
        assert!(rec.get("tools").is_none() && rec.get("head").is_none() && rec.get("scope").is_none());
        assert_eq!(rec["state"], "passed");
    }

    #[test]
    fn a_verdict_counts_only_for_work_unchanged_while_its_agent_ran() {
        let a = crate::small::Snapshot { head: "abc".into(), status_hash: "h1".into() };
        let b = crate::small::Snapshot { head: "def".into(), status_hash: "h1".into() };
        let dirty = crate::small::Snapshot { head: "abc".into(), status_hash: "h2".into() };
        let start = (r"C:\r".to_string(), a.clone());
        assert_eq!(place_of(Some(&start), Some(r"C:\r"), Some(&a)), Some((r"C:\r", &a)));
        assert_eq!(place_of(Some(&start), Some(r"C:\r"), Some(&b)), None, "a commit while it ran");
        assert_eq!(place_of(Some(&start), Some(r"C:\r"), Some(&dirty)), None, "an edit while it ran");
        assert_eq!(place_of(Some(&start), Some(r"C:\other"), Some(&a)), None);
        assert_eq!(place_of(Some(&start), Some(r"C:\r"), None), None);
        assert_eq!(place_of(None, Some(r"C:\r"), Some(&a)), None, "no start seen");
        // Starts are taken oldest first, per session and agent.
        let s2 = (r"C:\r".to_string(), b.clone());
        STARTS.lock().unwrap().get_or_insert_with(Default::default).insert(("row-t".into(), "card-tester".into()), vec![start.clone(), s2.clone()]);
        assert_eq!(take_start("row-t", "card-tester"), Some(start));
        assert_eq!(take_start("row-t", "card-reviewer"), None);
        assert_eq!(take_start("row-t", "card-tester"), Some(s2));
        assert_eq!(take_start("row-t", "card-tester"), None);
    }

    #[test]
    fn a_verdict_is_placed_only_in_a_listed_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let co = dir.path().join("listed");
        std::fs::create_dir_all(co.join(".git")).unwrap();
        let co = co.display().to_string();
        s.set_page_setting(kanban_store::SMALL_REPOS_KEY, &json!([{ "repo": "a/b", "checkout": co, "branch": "main" }]).to_string()).unwrap();
        let listed = card(json!({ "title": "T", "repo": "A/B", "worktreePath": r"C:\elsewhere" }), "In progress", 1);
        assert_eq!(checkout_of(&s, &listed).as_deref(), Some(co.as_str()));
        // Any other card: no checkout, so no git runs for it.
        let other = card(json!({ "title": "T", "repo": "c/d", "worktreePath": r"C:\wt" }), "In progress", 2);
        assert_eq!(checkout_of(&s, &other), None);
        assert_eq!(checkout_of(&s, &card(json!({ "title": "T" }), "In progress", 3)), None);
    }

    #[test]
    fn a_verdict_that_names_a_secret_is_still_recorded_and_holds_none() {
        let fail = Verdict { card: None, passed: Some(false), failures: vec!["a hard-coded Password in config.rs".into(), "it logs the token eyJhbGciOiJIUzI1NiJ9.e30.abc".into()], scope: None };
        let (rec, line) = record_of(&T, &fail, Some(3), "a1", "t", 0);
        let d = rec["detail"].as_str().unwrap();
        assert_eq!(d, "a hard-coded Pass word in config.rs; it logs the token [hidden]");
        // Sign-in links and Bearer headers too (the router's rule).
        let m = kanban_store::mask_secrets("open http://127.0.0.1:3006/dev-sign-in.html?as=admin&next=%2Fo%2FABC, then GET /api/dev/sign-in-token?as=user with Authorization: Bearer abc.DEF-123_x");
        assert_eq!(m, "open [sign-in link hidden], then GET [sign-in link hidden] with Authorization: Bearer [hidden]");
        assert!(!kanban_store::looks_secret(d) && !kanban_store::looks_secret(&line), "{d} / {line}");
        // So the board takes it.
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let c = s.create(None, json!({ "title": "T", "column": "In progress" }).as_object().cloned().unwrap(), "test").unwrap();
        let mut p = Patch::new();
        p.insert("review".into(), rec);
        assert!(s.update(&c.id, c.version, &p, "the app").is_ok());
        assert!(s.add_log(&c.id, &line, "the app").is_ok());
    }

    #[test]
    fn every_agent_is_defined_and_fenced() {
        let v: Value = serde_json::from_str(&agents_json()).unwrap();
        assert_eq!(v.as_object().unwrap().len(), AGENTS.len());
        for a in AGENTS {
            let d = &v[a.name];
            assert!(d["description"].as_str().is_some_and(|s| !s.is_empty()), "{}", a.name);
            assert!(d["prompt"].as_str().unwrap().contains("CARD:"), "{}", a.name);
            assert_eq!(d["model"], "inherit");
            assert_eq!(d.get("effort").and_then(Value::as_str), a.effort, "{}", a.name);
            let denied: Vec<&str> = d["disallowedTools"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
            for tool in ["mcp__kanban__kanban_move_card", "mcp__kanban-app__kanban_move_card", "mcp__kanban-app__kanban_update_card", "mcp__kanban__kanban_ship_result", "mcp__kanban__kanban_archive_card"] {
                assert!(denied.contains(&tool), "{} may use {tool}", a.name);
            }
            // No rule with a specifier on a shell: it would take the whole shell away (card #100).
            assert!(!denied.iter().any(|t| t.starts_with("Bash(") || t.starts_with("PowerShell(")), "{}: {denied:?}", a.name);
            if !a.edits {
                for tool in ["Edit", "Write", "NotebookEdit"] {
                    assert!(denied.contains(&tool), "{} may use {tool}", a.name);
                }
            }
        }
    }

    #[test]
    fn only_the_tester_takes_evidence_and_its_prompt_says_how() {
        assert!(crate::tester::CHECK.evidence);
        assert!(!crate::reviewer::CHECK.evidence);
        for a in AGENTS {
            if let OnStop::Check(c) = &a.on_stop {
                assert_eq!(c.evidence, a.name == crate::tester::TESTER, "{}", a.name);
            }
        }
        let p = crate::tester::AGENT.prompt;
        for s in ["hughht5/kanban-app", "EVIDENCE: before|after", "5. Evidence", "prints the test app's build commit", "runs another build, take no shots"] {
            assert!(p.contains(s), "{s}");
        }
        // The port is the tested test app's, from the brief (test slots, 9333 + N), 9334 when the
        // brief gives none, and never the live app's.
        for s in ["CDP_PORT=<that port>", "the brief gives the port", "test N 9333+N", "if it gives none, use 9334", "Never 9333"] {
            assert!(p.contains(s), "{s}");
        }
        assert!(!p.contains("CDP_PORT=9334"), "a fixed port in the shot command");
        // The prompt never names the word a secret check looks for.
        assert!(!p.to_ascii_lowercase().contains("password"));
        // Step 5 comes after step 4, and the report still ends with the CARD and VERDICT lines.
        assert!(p.find("4. Judge strictly").unwrap() < p.find("5. Evidence").unwrap());
        assert!(p.find("5. Evidence").unwrap() < p.find("CARD: #<the card's number>").unwrap());
        assert!(p.trim_end().ends_with("only a PASS lets the card go to Human test."));
        assert!(!kanban_store::looks_like_token(p));
    }

    #[test]
    fn a_testers_shots_go_on_the_card_with_its_verdict() {
        let root = tempfile::tempdir().unwrap();
        let (temp, data) = (root.path().join("temp"), root.path().join("data"));
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&[0, 0, 1, 0, 0, 0, 0, 200, 8, 6, 0, 0, 0]);
        let shot = temp.join("after.png");
        std::fs::write(&shot, &png).unwrap();
        let fake = temp.join("fake.png");
        std::fs::write(&fake, b"just text").unwrap();
        let text = format!("Tested.\nEVIDENCE: after {} | the chip\nEVIDENCE: before {} | fake\nCARD: #1\nVERDICT: PASS", shot.display(), fake.display());
        // One `at` for the files, the verdict and its run, as record_check does.
        let at = "2026-11-06T03:15:00.123Z";
        let p = crate::evidence::prepare(&data, &temp, "agent-9", at, &text);
        assert_eq!((p.shots.len(), p.refused.len()), (1, 1), "{:?}", p.refused);
        assert_eq!(p.shots[0]["file"], "evidence/agent-9/20261106031500123-1-after.png");

        let mut s = Store::open(root.path().join("b.db")).unwrap();
        let c = s.create(None, json!({ "title": "T", "column": "In progress" }).as_object().cloned().unwrap(), "test").unwrap();
        let (rec, _) = record_of(&crate::tester::CHECK, &parse_verdict(&text), Some(3), "agent-9", at, 0);
        let run = evidence_run("agent-9", at, &rec, &p);
        assert_eq!((run["run"].as_str(), run["verdict"].as_str(), run["shots"][0]["width"].as_u64()), (Some("agent-9"), Some("passed"), Some(256)));
        assert!(same_at(&run, &rec));
        // The shot has no build chunk, so the run has no build.
        assert!(run.get("build").is_none(), "{run}");
        let mut patch = Patch::new();
        patch.insert(kanban_store::TEST_KEY.into(), rec);
        patch.insert(kanban_store::EVIDENCE_KEY.into(), kanban_store::add_run(&c, run));
        let c = s.update(&c.id, c.version, &patch, crate::tester::BY).unwrap();
        assert!(kanban_store::has_tested_evidence(&c));

        // Shots that go on no card, and each refused file, are logged where Hugh looks.
        evidence_lost(&mut s, &c.id, Some(&p), "#5 isn't this card or a card in its stack", crate::tester::BY);
        let log: Vec<String> = s.log(&c.id).unwrap().into_iter().map(|l| l.text).collect();
        assert!(log.contains(&"Evidence not taken: #5 isn't this card or a card in its stack. The 1 shot(s) copied to evidence/agent-9 are on no card.".to_string()), "{log:?}");
        assert!(log.contains(&"Evidence not taken: fake.png: not a PNG".to_string()), "{log:?}");
        // A why that names a secret is masked, so the line is still taken.
        evidence_lost(&mut s, &c.id, Some(&p), "\"evidence\" looks like it holds a token, key or password", crate::tester::BY);
        assert!(s.log(&c.id).unwrap().iter().any(|l| l.text.contains("key or pass word")));
        // No shots and no refusals: nothing is logged.
        let before = s.log(&c.id).unwrap().len();
        evidence_lost(&mut s, &c.id, Some(&crate::evidence::Prepared::default()), "x", crate::tester::BY);
        evidence_lost(&mut s, &c.id, None, "x", crate::tester::BY);
        assert_eq!(s.log(&c.id).unwrap().len(), before);

        // The tester resumed (the same agent id) with a later `at`: its run goes first, and it,
        // not the older run of the same agent, backs the new pass.
        let later = "2026-11-06T03:40:02.007Z";
        let p2 = crate::evidence::prepare(&data, &temp, "agent-9", later, &text);
        assert_eq!((p2.shots.len(), p2.refused.len()), (1, 1), "{:?}", p2.refused);
        assert_eq!(p2.shots[0]["file"], "evidence/agent-9/20261106034002007-1-after.png");
        let (rec2, _) = record_of(&crate::tester::CHECK, &parse_verdict(&text), Some(3), "agent-9", later, 0);
        let mut patch = Patch::new();
        patch.insert(kanban_store::TEST_KEY.into(), rec2.clone());
        let c = s.get(&c.id).unwrap();
        let c2 = s.update(&c.id, c.version, &patch, crate::tester::BY).unwrap();
        assert!(!kanban_store::has_tested_evidence(&c2), "the older run of the same agent doesn't back the newer pass");
        let mut patch = Patch::new();
        patch.insert(kanban_store::EVIDENCE_KEY.into(), kanban_store::add_run(&c2, evidence_run("agent-9", later, &rec2, &p2)));
        let c2 = s.update(&c2.id, c2.version, &patch, crate::tester::BY).unwrap();
        assert!(kanban_store::has_tested_evidence(&c2));
        assert_eq!(c2.fields[kanban_store::EVIDENCE_KEY].as_array().map(Vec::len), Some(2));
        // A run and a record from two `at`s don't match.
        assert!(!same_at(&evidence_run("agent-9", at, &rec2, &p2), &rec2));
        assert!(!same_at(&json!({ "run": "x" }), &json!({ "at": at })));
    }

    #[test]
    fn a_run_shows_the_build_its_shots_share() {
        let shot = |b: Option<&str>| match b {
            Some(b) => json!({ "n": 1, "kind": "after", "build": b }),
            None => json!({ "n": 1, "kind": "after" }),
        };
        assert_eq!(run_build(&[]), None);
        assert_eq!(run_build(&[shot(None), shot(None)]), None);
        assert_eq!(run_build(&[shot(Some("0321fbe"))]).as_deref(), Some("0321fbe"));
        assert_eq!(run_build(&[shot(Some("0321fbe")), shot(Some("0321fbe"))]).as_deref(), Some("0321fbe"));
        assert_eq!(run_build(&[shot(Some("0321fbe")), shot(Some("f46082b"))]).as_deref(), Some("mixed"));
        // A shot with no build is from a build nobody can name: not the same one.
        assert_eq!(run_build(&[shot(Some("0321fbe")), shot(None)]).as_deref(), Some("mixed"));
        assert_eq!(run_build(&[shot(None), shot(Some("0321fbe"))]).as_deref(), Some("mixed"));
        // On the run: "build" when there is one, no key when there is none.
        let rec = json!({ "state": "passed", "at": "t" });
        let p = crate::evidence::Prepared { shots: vec![shot(Some("abc")), shot(Some("abc"))], ..Default::default() };
        assert_eq!(evidence_run("a", "t", &rec, &p)["build"], "abc");
        let p = crate::evidence::Prepared { shots: vec![shot(None)], ..Default::default() };
        assert!(evidence_run("a", "t", &rec, &p).get("build").is_none());
    }

    #[test]
    fn a_hook_finds_its_agent_by_name_or_plugin_name() {
        assert_eq!(find("card-tester").map(|a| a.name), Some("card-tester"));
        assert_eq!(find("card-reviewer").map(|a| a.name), Some("card-reviewer"));
        assert_eq!(find("someplugin:card-reviewer").map(|a| a.name), Some("card-reviewer"));
        assert_eq!(find("card-reproducer").map(|a| a.name), Some("card-reproducer"));
        assert!(find("general-purpose").is_none());
        assert!(find("").is_none());
        assert!(find("card-reviewer-2").is_none());
    }

    #[test]
    fn a_verdict_goes_on_the_card_it_names_only_within_the_stack() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let mk = |s: &mut Store, t: &str| s.create(None, json!({ "title": t, "column": "In progress" }).as_object().cloned().unwrap(), "test").unwrap();
        let lead = mk(&mut s, "lead");
        let other = mk(&mut s, "other");
        let child = s.create_in_stack(json!({ "title": "child" }).as_object().cloned().unwrap(), lead.number, "test").unwrap();
        let lead = s.get(&lead.id).unwrap();
        assert_eq!(target_card(&s, &lead, None).unwrap().id, lead.id);
        assert_eq!(target_card(&s, &lead, Some(lead.number)).unwrap().id, lead.id);
        assert_eq!(target_card(&s, &lead, Some(child.number)).unwrap().id, child.id);
        assert!(target_card(&s, &lead, Some(other.number)).is_err());
        assert!(target_card(&s, &other, Some(child.number)).is_err());
    }

    fn card(fields: Value, column: &str, number: i64) -> Card {
        Card { id: format!("c{number}"), number, column: column.into(), order: None, order_col: None, archived: false, fields: fields.as_object().cloned().unwrap(), version: 1 }
    }

    #[test]
    fn the_move_to_human_test_needs_both_the_review_and_the_test() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let both = card(json!({ "title": "T", "test": { "state": "passed" }, "review": { "state": "passed" } }), "In progress", 7);
        assert!(move_refused(&s, &both, "Human test").is_none());
        let no_review = card(json!({ "title": "T", "test": { "state": "passed" }, "review": { "state": "needed" } }), "In progress", 7);
        let why = move_refused(&s, &no_review, "Human test").unwrap();
        assert!(why.contains("subagent review") && why.contains("\"card-reviewer\"") && !why.contains("subagent test"), "{why}");
        let no_test = card(json!({ "title": "T", "review": { "state": "passed" } }), "In progress", 7);
        let why = move_refused(&s, &no_test, "Human test").unwrap();
        assert!(why.contains("subagent test") && !why.contains("subagent review"), "{why}");
        // Neither: both are named, the review first.
        let why = move_refused(&s, &card(json!({ "title": "T" }), "In progress", 7), "Deploy complete").unwrap();
        let (r, t) = (why.find("subagent review").unwrap(), why.find("subagent test").unwrap());
        assert!(r < t, "{why}");
        // Each rule is Hugh's to turn off on its own.
        s.set_page_setting(kanban_store::REVIEW_RULE_KEY, "off").unwrap();
        assert!(move_refused(&s, &no_review, "Human test").is_none());
        assert!(move_refused(&s, &no_test, "Human test").is_some());
        s.set_page_setting(kanban_store::REVIEW_RULE_KEY, "on").unwrap();
        s.set_page_setting(kanban_store::TEST_RULE_KEY, "off").unwrap();
        assert!(move_refused(&s, &no_test, "Human test").is_none());
        assert!(move_refused(&s, &no_review, "Human test").is_some());
    }

    // Card #105: the card-tester's CHECK lines and the card's acceptance checks.
    fn new_card(s: &mut Store, fields: Value) -> Card {
        s.create(None, fields.as_object().cloned().unwrap(), "test").unwrap()
    }

    fn record(s: &mut Store, card: &Card, report: &str, tools: usize) -> (Card, Value) {
        let check = &crate::tester::CHECK;
        let results = crate::done_when::check_lines(report);
        let mut v = parse_verdict(report);
        pass_fails_on_checks(card, &mut v, &results);
        let (rec, _) = record_of(check, &v, Some(tools), "a1", "t", 0);
        let mut p = Patch::new();
        p.insert(check.key.into(), rec.clone());
        let saved = s.update(&card.id, card.version, &p, check.by).unwrap();
        (mark_checks(s, saved, &rec, &results, check.by), rec)
    }

    #[test]
    fn check_results_go_to_the_card_the_report_names() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let checks = json!(["the line shows", "a tick is kept"]);
        let lead = new_card(&mut s, json!({ "title": "lead", "column": "In progress", "doneWhenChecks": checks }));
        let child = s.create_in_stack(json!({ "title": "child", "doneWhenChecks": checks }).as_object().cloned().unwrap(), lead.number, "test").unwrap();
        let lead = s.get(&lead.id).unwrap();
        // The lead's session runs the tester on the child: its CARD line names the child.
        let report = format!("CHECK 1: PASS
CHECK 2: FAIL - the tick is lost on reload
CARD: #{}
VERDICT: FAIL
- check 2", child.number);
        let target = target_card(&s, &lead, parse_verdict(&report).card).unwrap();
        assert_eq!(target.id, child.id);
        let (child, rec) = record(&mut s, &target, &report, 3);
        assert_eq!(rec["state"], json!("failed"));
        let l = child.fields["doneWhenChecks"].as_array().unwrap();
        assert_eq!((l[0]["state"].as_str(), l[0]["by"].as_str()), (Some("met"), Some(crate::tester::BY)));
        assert_eq!((l[1]["state"].as_str(), l[1]["note"].as_str()), (Some("failed"), Some("the tick is lost on reload")));
        assert!(s.log(&child.id).unwrap().iter().any(|e| e.text == "Subagent test marked the checks: 1 met (the line shows); 2 failed (a tick is kept)"));
        // The lead's own checks are untouched.
        let lead = s.get(&lead.id).unwrap();
        assert!(lead.fields["doneWhenChecks"].as_array().unwrap().iter().all(|c| c["state"] == json!("open")));
        // A tester that ran no tools is unclear: nothing is marked.
        let (after, rec) = record(&mut s, &lead, "CHECK 1: PASS
CARD: #1
VERDICT: PASS", 0);
        assert_eq!(rec["state"], json!("unclear"));
        assert!(after.fields["doneWhenChecks"].as_array().unwrap().iter().all(|c| c["state"] == json!("open")));
    }

    #[test]
    fn a_pass_with_a_failed_check_is_recorded_as_a_fail() {
        // The prompt asks for a FAIL when a check fails; the app doesn't take the tester's word for it.
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let c = new_card(&mut s, json!({ "title": "c", "column": "In progress", "doneWhenChecks": ["a", "b"] }));
        let (c, rec) = record(&mut s, &c, "CHECK 1: PASS
CHECK 2: FAIL - the password box shows
CHECK 7: FAIL - not a check
CARD: #1
VERDICT: PASS", 4);
        assert_eq!(rec["state"], json!("failed"));
        let d = rec["detail"].as_str().unwrap();
        assert!(d.starts_with("the tester said PASS, but check 2 failed:") && !d.contains("check 7"), "{d}");
        assert!(!kanban_store::looks_secret(d), "{d}");
        assert!(crate::tester::gate(&s, &c, "Human test").is_some(), "the failed check holds the move");
        // A PASS whose checks all pass stays a pass.
        let (_, rec) = record(&mut s, &c, "CHECK 1: PASS
CHECK 2: PASS
CARD: #1
VERDICT: PASS", 4);
        assert_eq!(rec["state"], json!("passed"));
        // Another agent's report isn't read for CHECK lines (record_check reads them for the tester only).
        let mut v = parse_verdict("VERDICT: PASS");
        pass_fails_on_checks(&c, &mut v, &[]);
        assert_eq!(v.passed, Some(true));
    }

    #[test]
    fn a_check_list_the_store_refuses_keeps_the_verdict() {
        // The checks are written after the verdict: a list the store won't take (here, one an older
        // build or an import left with too many checks) loses the marks, never the verdict.
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let c = new_card(&mut s, json!({ "title": "c", "column": "In progress" }));
        let check = &crate::tester::CHECK;
        let (rec, _) = record_of(check, &parse_verdict("CARD: #1
VERDICT: PASS"), Some(2), "a1", "t", 0);
        let mut p = Patch::new();
        p.insert(check.key.into(), rec.clone());
        let mut saved = s.update(&c.id, c.version, &p, check.by).unwrap();
        saved.fields.insert("doneWhenChecks".into(), json!((0..11).map(|i| json!({ "text": format!("c{i}"), "state": "open" })).collect::<Vec<_>>()));
        let after = mark_checks(&mut s, saved, &rec, &crate::done_when::check_lines("CHECK 1: PASS"), check.by);
        assert_eq!(s.get(&c.id).unwrap().fields["test"]["state"], json!("passed"));
        assert_eq!(after.version, s.get(&c.id).unwrap().version);
        assert!(s.log(&c.id).unwrap().iter().any(|e| e.text.starts_with("Couldn't mark the acceptance checks:")), "the refusal is logged");
    }
}
