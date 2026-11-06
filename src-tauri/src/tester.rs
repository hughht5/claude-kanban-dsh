//! The subagent test before Human test (Hugh, card #76): every card session gets a
//! `card-tester` subagent (`--agents`), the app hears when one finishes (`SubagentStop`, an
//! `http` hook), records its verdict on the card it names (`test`), and a session's move of a
//! card from In progress to Human test is refused until that card's latest verdict is a pass.

use crate::{sessions::Sessions, Board};
use kanban_store::{Card, Patch, Store, TEST_KEY};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

/// The subagent's name, as the session calls it (`subagent_type`) and the hook reports it.
pub const TESTER: &str = "card-tester";
/// Who the verdicts are recorded as.
pub const BY: &str = "the app (card-tester subagent)";

const DESCRIPTION: &str = "Independent tester for one card on Hugh's Claude Kanban DSH board. Run it, and wait for its result, before moving a card from In progress to Human test: brief it with the card number, the deliverables and how to try the change, not your conclusions. It tests the real thing, fixes nothing, and ends with CARD and VERDICT lines that the app records on the card.";

const PROMPT: &str = "You are card-tester, an independent tester for one card on Hugh's Claude Kanban DSH board. The session that built the change has briefed you. You haven't seen its work, so don't take its word for anything: find out for yourself.

1. Read the card: kanban_get_card with its number (from the brief). Its summary lists the deliverables; the brief says where the change is and how to try it.
2. Test each deliverable by using the real thing: run the commands, the tests and the app or page the brief names, and look at what happens. Reading the code alone is not a test. Try the obvious failure case for each as well.
3. Fix nothing. Don't edit files, commit, push, merge or deploy, and don't change or move any card. Leave anything you started (test cards, processes) as you found it, or say what is left.
4. Judge strictly. A deliverable that is missing, only partly built, or that you couldn't try is a FAIL: say why. Skip style and wording nits.

Report, for each deliverable: what you did, what you saw (the command and its output, or what the screen showed), and PASS or FAIL. Then end your reply with these lines, and nothing after them:
CARD: #<the card's number>
VERDICT: PASS
or, if anything failed, VERDICT: FAIL followed by one line per failure, each starting with \"- \". The app reads these lines to record the test on the card, and only a PASS lets the card go to Human test.";

/// Tools the tester may not use: it reports, it doesn't change anything.
const DENIED: [&str; 12] = [
    "Edit",
    "Write",
    "NotebookEdit",
    "mcp__kanban__kanban_move_card",
    "mcp__kanban__kanban_update_card",
    "mcp__kanban__kanban_create_card",
    "mcp__kanban__kanban_ship_result",
    "mcp__kanban__kanban_set_passed",
    "mcp__kanban__kanban_stack_add",
    "mcp__kanban__kanban_stack_remove",
    "mcp__kanban__kanban_stack_join",
    "mcp__kanban__kanban_stack_detach",
];

/// The `--agents` JSON for a card session: the tester, on the session's own model.
pub fn agents_json() -> String {
    json!({ TESTER: { "description": DESCRIPTION, "prompt": PROMPT, "disallowedTools": DENIED, "model": "inherit" } }).to_string()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// The card it names (`CARD: #12`), if any.
    pub card: Option<i64>,
    /// None when there is no VERDICT line.
    pub passed: Option<bool>,
    pub failures: Vec<String>,
}

/// Reads the tester's closing lines: the last `CARD: #n` and the last `VERDICT: PASS|FAIL`
/// (any case, markdown bold or backticks around them allowed), and the `- ` lines after a FAIL.
pub fn parse_verdict(text: &str) -> Verdict {
    let clean = |l: &str| l.trim().trim_matches(|c| c == '*' || c == '`' || c == '_').trim().to_string();
    let mut v = Verdict { card: None, passed: None, failures: vec![] };
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

/// How many tool calls the subagent's transcript records, if it can be read: a tester that ran
/// none didn't test anything.
fn tool_calls(path: &str) -> Option<usize> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(text.lines().map(|l| l.matches("\"type\":\"tool_use\"").count()).sum())
}

/// What the app records for a verdict: the `test` field's value and the log line.
pub fn record_of(v: &Verdict, tools: Option<usize>, agent_id: &str, at: &str) -> (Value, String) {
    let (state, detail, line) = match (v.passed, tools) {
        (Some(true), Some(0)) => ("unclear", "the tester said PASS but ran no tools, so it didn't test anything".to_string(), None),
        (Some(true), _) => ("passed", String::new(), Some("Subagent test: PASS".to_string())),
        (Some(false), _) => {
            let d = if v.failures.is_empty() { "see the tester's report in the session".to_string() } else { v.failures.join("; ") };
            ("failed", d.clone(), Some(format!("Subagent test: FAIL: {d}")))
        }
        (None, _) => ("unclear", "the tester ended with no VERDICT line, so it isn't a pass".to_string(), None),
    };
    let detail: String = detail.chars().take(600).collect();
    let line = line.unwrap_or_else(|| format!("Subagent test unclear: {detail}"));
    let line: String = line.chars().take(700).collect();
    let mut rec = json!({ "state": state, "at": at, "agentId": agent_id });
    if !detail.is_empty() {
        rec["detail"] = json!(detail);
    }
    (rec, line)
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

/// A `SubagentStop` hook from session `row_id`: records a card-tester's verdict.
pub fn on_subagent_stop(app: &AppHandle, row_id: &str, body: &Value) {
    let kind = body.get("agent_type").and_then(Value::as_str).unwrap_or("");
    if kind != TESTER && !kind.ends_with(&format!(":{TESTER}")) {
        return;
    }
    let text = body.get("last_assistant_message").and_then(Value::as_str).unwrap_or("");
    let agent_id = body.get("agent_id").and_then(Value::as_str).unwrap_or("");
    let tools = body.get("agent_transcript_path").and_then(Value::as_str).and_then(tool_calls);
    let board = app.state::<Board>();
    let Ok(mut store) = board.0.lock() else { return };
    let own_id = app.state::<Arc<Sessions>>().card_of_row(row_id).or_else(|| store.session_card(row_id).ok().flatten());
    let Some(own) = own_id.and_then(|id| store.get(&id).ok()) else { return };
    let v = parse_verdict(text);
    let card = match target_card(&store, &own, v.card) {
        Ok(c) => c,
        Err(why) => {
            let _ = store.add_log(&own.id, &format!("Not recorded: the card-tester subagent reported on {why}"), BY);
            return;
        }
    };
    let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let (rec, line) = record_of(&v, tools, agent_id, &at);
    let mut patch = Patch::new();
    patch.insert(TEST_KEY.into(), rec);
    if let Err(e) = store.update(&card.id, card.version, &patch, BY) {
        let _ = store.add_log(&card.id, &format!("Couldn't record the subagent test: {e}"), BY);
        return;
    }
    let _ = store.add_log(&card.id, &line, BY);
    drop(store);
    let _ = app.emit("cards-changed", ());
}

/// Columns a card is tested before leaving for Human test (or past it): every column before it.
const BEFORE_TEST: [&str; 4] = ["Backlog", "Blocked", "Planning", "In progress"];

/// Why a card's session may not move `card` to `to`, if it may not: into Human test (or on past
/// it, to Deploy complete) from any column before it, only once the card's latest subagent test
/// passed, unless Hugh turned the rule off. Every move into In progress makes it needed again.
/// Only card sessions are held to it: Hugh's own moves, and his own sessions' `kb move`, aren't.
pub fn move_refused(store: &Store, card: &Card, to: &str) -> Option<String> {
    if !BEFORE_TEST.contains(&card.column.as_str()) || !["Human test", "Deploy complete"].contains(&to) || !store.test_required() {
        return None;
    }
    let test = card.fields.get(TEST_KEY);
    let state = test.and_then(|t| t.get("state")).and_then(Value::as_str).unwrap_or("needed");
    if state == "passed" {
        return None;
    }
    let latest = match state {
        "failed" => format!(" Its latest test failed: {}.", test.and_then(|t| t.get("detail")).and_then(Value::as_str).unwrap_or("see the tester's report")),
        "unclear" => format!(" Its latest test didn't count: {}.", test.and_then(|t| t.get("detail")).and_then(Value::as_str).unwrap_or("no verdict")),
        _ => String::new(),
    };
    Some(format!(
        "card #{n} hasn't passed a subagent test since it last came into In progress (Hugh's rule: every card is tested by a subagent before Human test).{latest} Run the card-tester subagent (the Agent tool, subagent_type \"{TESTER}\") and wait for its result: if it runs in the background, wait for its notification before you move the card. Brief it like a stranger: the card number (#{n}), the deliverables, where the change is and how to try it (the test app, the dev stack and its logins), not your conclusions. It ends with CARD and VERDICT lines, and the app records its verdict on the card. Fix what it finds and run it again; move the card once its latest verdict is PASS.",
        n = card.number
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_lines_are_read_from_the_end_of_the_report() {
        let v = parse_verdict("Tested the form.\n\nCARD: #76\nVERDICT: PASS");
        assert_eq!(v, Verdict { card: Some(76), passed: Some(true), failures: vec![] });
        let v = parse_verdict("**CARD:** #12\n**VERDICT: FAIL**\n- the chip doesn't show\n- Esc doesn't close it\n");
        assert_eq!(v.card, Some(12));
        assert_eq!(v.passed, Some(false));
        assert_eq!(v.failures, vec!["the chip doesn't show", "Esc doesn't close it"]);
        // The last verdict wins (a report that quotes an earlier one).
        let v = parse_verdict("VERDICT: FAIL\n- x\nRe-ran it.\nCARD: 5\nverdict: pass");
        assert_eq!((v.card, v.passed, v.failures.len()), (Some(5), Some(true), 0));
        // No verdict line: unclear, never a pass.
        let v = parse_verdict("All looks fine to me.");
        assert_eq!(v.passed, None);
        assert_eq!(parse_verdict("VERDICT: PASSABLE?").passed, None);
        assert_eq!(parse_verdict("VERDICT: PASS (all three)").passed, Some(true));
    }

    #[test]
    fn a_pass_with_no_tools_run_doesnt_count() {
        let pass = Verdict { card: None, passed: Some(true), failures: vec![] };
        assert_eq!(record_of(&pass, Some(3), "a1", "t").0["state"], "passed");
        assert_eq!(record_of(&pass, None, "a1", "t").0["state"], "passed"); // transcript unreadable: trust the verdict
        let (rec, line) = record_of(&pass, Some(0), "a1", "t");
        assert_eq!(rec["state"], "unclear");
        assert!(line.starts_with("Subagent test unclear:"));
        let fail = Verdict { card: None, passed: Some(false), failures: vec!["a".into(), "b".into()] };
        let (rec, line) = record_of(&fail, Some(4), "a1", "t");
        assert_eq!((rec["state"].as_str(), rec["detail"].as_str()), (Some("failed"), Some("a; b")));
        assert_eq!(line, "Subagent test: FAIL: a; b");
        assert_eq!(record_of(&Verdict { card: None, passed: None, failures: vec![] }, Some(2), "a", "t").0["state"], "unclear");
    }

    #[test]
    fn the_agents_flag_defines_the_tester() {
        let v: Value = serde_json::from_str(&agents_json()).unwrap();
        let t = &v[TESTER];
        assert!(t["description"].as_str().unwrap().contains("Human test"));
        assert!(t["prompt"].as_str().unwrap().contains("VERDICT: PASS"));
        let denied: Vec<&str> = t["disallowedTools"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        for tool in ["Edit", "Write", "mcp__kanban__kanban_move_card"] {
            assert!(denied.contains(&tool), "{tool}");
        }
    }

    fn card(fields: Value, column: &str, number: i64) -> Card {
        Card { id: format!("c{number}"), number, column: column.into(), order: None, order_col: None, archived: false, fields: fields.as_object().cloned().unwrap(), version: 1 }
    }

    #[test]
    fn the_move_to_human_test_waits_for_a_pass() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let needed = card(json!({ "title": "T", "test": { "state": "needed" } }), "In progress", 7);
        let why = move_refused(&s, &needed, "Human test").unwrap();
        assert!(why.contains("card #7 hasn't passed a subagent test") && why.contains("subagent_type \"card-tester\""), "{why}");
        assert!(move_refused(&s, &needed, "Deploy complete").is_some());
        // No record at all (in In progress before this rule) counts as needed.
        assert!(move_refused(&s, &card(json!({ "title": "T" }), "In progress", 7), "Human test").is_some());
        let failed = card(json!({ "title": "T", "test": { "state": "failed", "detail": "the chip is missing" } }), "In progress", 7);
        assert!(move_refused(&s, &failed, "Human test").unwrap().contains("Its latest test failed: the chip is missing."));
        let passed = card(json!({ "title": "T", "test": { "state": "passed" } }), "In progress", 7);
        assert!(move_refused(&s, &passed, "Human test").is_none());
        // Other moves are free: Done (no diff), Blocked, back to Planning, and from Human test on.
        for to in ["Done", "Blocked", "Planning"] {
            assert!(move_refused(&s, &needed, to).is_none(), "{to}");
        }
        assert!(move_refused(&s, &card(json!({ "title": "T" }), "Human test", 7), "Deploy complete").is_none());
        // No way round it through Blocked or straight from Planning.
        assert!(move_refused(&s, &card(json!({ "title": "T", "test": { "state": "needed" } }), "Blocked", 7), "Human test").is_some());
        assert!(move_refused(&s, &card(json!({ "title": "T" }), "Planning", 7), "Human test").is_some());
        // A pass still counts after a stop in Blocked (only a move into In progress resets it).
        assert!(move_refused(&s, &card(json!({ "title": "T", "test": { "state": "passed" } }), "Blocked", 7), "Human test").is_none());
        // Hugh can turn the rule off.
        s.set_page_setting(kanban_store::TEST_RULE_KEY, "off").unwrap();
        assert!(move_refused(&s, &needed, "Human test").is_none());
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
}
