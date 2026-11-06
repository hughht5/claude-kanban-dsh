//! The plan critique before building (Hugh, card #97): every card session gets a `card-critic`
//! subagent. At the end of Planning the session briefs it with the card; it attacks the plan
//! (the card's summary) and ends PASS or FAIL. The app records its verdict on the card it names
//! (`critique`, by `agents.rs`'s shared code), and a card session's move into In progress is
//! held until that card's critique has passed, while Hugh's rule is on. Hugh's own moves never
//! are. The resets (into Planning, a summary edit, `started`) are the store's (`cards.rs`).

use crate::agents::{not_passed, Agent, Check, OnStop};
use kanban_store::{Card, Store, CRITIQUE_KEY, TEST_KEY};
use serde_json::Value;

/// The subagent's name, as the session calls it (`subagent_type`) and the hook reports it.
pub const CRITIC: &str = "card-critic";

const DESCRIPTION: &str = "Independent critic of one card's plan on Hugh's Claude Kanban DSH board. Run it, and wait for its result, at the end of Planning, before you say the plan is ready or move the card on: brief it with the card number and where the code is, not your conclusions. It checks the plan against the card and the code, changes nothing, and ends with CARD and VERDICT lines that the app records on the card.";

const PROMPT: &str = "You are card-critic, an independent critic of one card's plan on Hugh's Claude Kanban DSH board. The plan is not built yet. Don't take the brief's word for anything: check it yourself.

1. Read the card: kanban_get_card with its number (from the brief). Its summary is the plan: what the card asks for, the deliverables and the tests.
2. Read the code the plan names (the brief says where the checkout is) and the repo's AGENTS.md or CLAUDE.md.
3. Attack the plan. Look for: a deliverable the card asks for that the plan dropped; cases it misses; assumptions about the code that are wrong (check them in the code and give file:line); steps that break a hard rule of the repo; deliverables nobody could test; work the card didn't ask for.
3a. Acceptance checks (card #105): the plan also puts doneWhen (one plain line: the card is done when ...) and 3 to 7 doneWhenChecks on the card (kanban_get_card shows them). Each check is one thing a tester can see pass or fail. A missing, vague or untestable check is a finding.
4. A finding that is Hugh's decision (scope, or a trade-off the card doesn't settle) starts \"HUGH:\". Skip style and wording.
5. Change nothing: don't edit files, commit, or change or move any card.

Report your findings, each with its evidence. Then end your reply with these lines, and nothing after them:
CARD: #<the card's number>
VERDICT: PASS
or, if the plan needs work, VERDICT: FAIL followed by one line per finding, each starting with \"- \". The app records these lines on the card, and only a PASS lets the card's session move it to In progress.";

/// How the app records the critic's verdict (`agents::record_check`).
pub const CHECK: Check = Check {
    agent: CRITIC,
    key: CRITIQUE_KEY,
    by: "the app (card-critic subagent)",
    label: "Plan critique",
    who: "critic",
    verb: "critique",
    reset_in: "Planning",
    cap: Some(3),
    evidence: false,
};

pub const AGENT: Agent = Agent {
    name: CRITIC,
    description: DESCRIPTION,
    prompt: PROMPT,
    effort: Some("high"),
    edits: false,
    also_denied: &[],
    on_stop: OnStop::Check(&CHECK),
};

/// The columns a card's plan is critiqued before leaving, and where to.
const BEFORE_BUILD: [&str; 3] = ["Backlog", "Planning", "Blocked"];
/// Into In progress, or straight on past it (which would skip the build's start, so a stale
/// test or review pass can't carry an unchecked plan through).
const INTO_BUILD: [&str; 3] = ["In progress", "Human test", "Deploy complete"];

/// Why a card's session may not move `card` to `to`, if the plan critique holds it: into In
/// progress (or past it) from Backlog, Planning or Blocked only once the critique passed. From
/// Blocked it is free when the build had already started: `started` on the record (set on every
/// move into In progress), or, with no record at all, a subagent test field (set on every move
/// into In progress since card #76), for a card blocked mid-build before this rule.
pub fn gate(store: &Store, card: &Card, to: &str) -> Option<String> {
    let rec = card.fields.get(CRITIQUE_KEY);
    let built = match rec {
        Some(r) => r.get("started").and_then(Value::as_bool) == Some(true),
        None => card.fields.contains_key(TEST_KEY),
    };
    if card.column == "Blocked" && built {
        return None;
    }
    let latest = not_passed(&CHECK, card, to, &BEFORE_BUILD, &INTO_BUILD, store.critique_required())?;
    Some(format!(
        "card #{n}'s plan hasn't passed a plan critique since it last came into Planning (Hugh's rule: every plan is checked by a subagent before building).{latest} Do these steps:
1. Run the card-critic subagent: the Agent tool, subagent_type \"{CRITIC}\". Wait for its result; if it runs in the background, wait for its notification.
2. Brief it with the card number (#{n}) and where the code is (the checkout and branch). Do not give your conclusions.
3. Fix the plan in the card's summary and run it again. A finding marked HUGH: is his to decide: ask him, and leave the card in Planning.
4. Move the card when its latest verdict is PASS. Any edit to the summary after a pass needs a fresh pass. After its third FAIL, the app moves the card to Blocked.
If your session has no card-critic subagent (it started before this rule), say so on the card and leave the card for Hugh to move on, or ask him to turn the rule off or restart the session.",
        n = card.number
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn card(fields: Value, column: &str) -> Card {
        Card { id: "c4".into(), number: 4, column: column.into(), order: None, order_col: None, archived: false, fields: fields.as_object().cloned().unwrap(), version: 1 }
    }

    #[test]
    fn the_move_into_in_progress_waits_for_a_plan_that_passed() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        // From Planning or Backlog: no record counts as needed.
        for from in ["Planning", "Backlog"] {
            let why = gate(&s, &card(json!({ "title": "T" }), from), "In progress").unwrap();
            assert!(why.contains("card #4's plan hasn't passed a plan critique") && why.contains("subagent_type \"card-critic\"") && why.contains("has no card-critic subagent"), "{why}");
        }
        let failed = card(json!({ "title": "T", "critique": { "state": "failed", "detail": "it deletes log rows", "round": 1 } }), "Planning");
        assert!(gate(&s, &failed, "In progress").unwrap().contains("Its latest critique failed: it deletes log rows."));
        let unclear = card(json!({ "title": "T", "critique": { "state": "unclear", "detail": "no VERDICT line" } }), "Planning");
        assert!(gate(&s, &unclear, "In progress").unwrap().contains("didn't count: no VERDICT line"));
        assert!(gate(&s, &card(json!({ "title": "T", "critique": { "state": "passed" } }), "Planning"), "In progress").is_none());
        // Only the move into In progress is held.
        for to in ["Blocked", "Backlog", "Done"] {
            assert!(gate(&s, &card(json!({ "title": "T" }), "Planning"), to).is_none(), "{to}");
        }
        // Blocked: held before the build started, free once it had (blocked mid-build), and
        // free with no record (blocked before this rule).
        assert!(gate(&s, &card(json!({ "title": "T", "critique": { "state": "failed", "round": 3 } }), "Blocked"), "In progress").is_some());
        assert!(gate(&s, &card(json!({ "title": "T", "critique": { "state": "needed", "round": 0 } }), "Blocked"), "In progress").is_some());
        assert!(gate(&s, &card(json!({ "title": "T", "critique": { "state": "passed" } }), "Blocked"), "In progress").is_none());
        assert!(gate(&s, &card(json!({ "title": "T", "critique": { "state": "needed", "started": true } }), "Blocked"), "In progress").is_none());
        // No record: blocked mid-build before this rule (it has a test field) goes through; a
        // card that never reached In progress (made straight into Planning or Blocked) is held.
        assert!(gate(&s, &card(json!({ "title": "T", "test": { "state": "needed" } }), "Blocked"), "In progress").is_none());
        assert!(gate(&s, &card(json!({ "title": "T" }), "Blocked"), "In progress").is_some());
        // Straight on past In progress is held too: stale test and review passes can't carry
        // an unchecked plan to Human test.
        let stale = card(json!({ "title": "T", "test": { "state": "passed" }, "review": { "state": "passed" }, "critique": { "state": "needed", "round": 0 } }), "Planning");
        assert!(gate(&s, &stale, "Human test").is_some() && gate(&s, &stale, "Deploy complete").is_some());
        assert!(crate::agents::move_refused(&s, &stale, "Human test").is_some());
        // Hugh can turn it off.
        s.set_page_setting(kanban_store::CRITIC_RULE_KEY, "off").unwrap();
        assert!(gate(&s, &failed, "In progress").is_none());
    }

    #[test]
    fn the_gate_is_one_of_the_apps_and_names_the_critic_first() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("b.db")).unwrap();
        // Through agents.rs, as tools.rs and dispatch.rs check it.
        let why = crate::agents::move_refused(&s, &card(json!({ "title": "T" }), "Planning"), "In progress").unwrap();
        assert!(why.contains("plan critique") && !why.contains("subagent review"), "{why}");
        assert!(crate::agents::move_refused(&s, &card(json!({ "title": "T", "critique": { "state": "passed" } }), "Planning"), "In progress").is_none());
    }

    #[test]
    fn the_critic_runs_at_high_and_marks_hughs_calls() {
        assert_eq!(AGENT.effort, Some("high"));
        assert!(!AGENT.edits);
        assert_eq!((CHECK.key, CHECK.reset_in, CHECK.cap), ("critique", "Planning", Some(3)));
        assert!(PROMPT.contains("VERDICT: PASS") && PROMPT.contains("CARD:") && PROMPT.contains("HUGH:"));
        assert!(crate::agents::find("card-critic").is_some_and(|a| a.name == CRITIC));
        let v: Value = serde_json::from_str(&crate::agents::agents_json()).unwrap();
        assert_eq!(v[CRITIC]["effort"], "high");
        assert!(PROMPT.len() < 2_000, "{}", PROMPT.len());
    }

    #[test]
    fn the_third_fail_caps_only_in_planning() {
        let rec = json!({ "state": "failed", "round": 3 });
        assert!(crate::agents::capped(&CHECK, &rec, "Planning"));
        // Already Blocked, or moved on by Hugh: no second trip to Blocked.
        for col in ["Blocked", "In progress", "Backlog"] {
            assert!(!crate::agents::capped(&CHECK, &rec, col), "{col}");
        }
        assert!(!crate::agents::capped(&CHECK, &json!({ "state": "failed", "round": 2 }), "Planning"));
    }
}
