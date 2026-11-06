//! The subagent test before Human test (Hugh, card #76): every card session gets a
//! `card-tester` subagent (`--agents`, from `agents.rs`'s table), the app hears when one
//! finishes (`SubagentStop`, an `http` hook), records its verdict on the card it names (`test`),
//! and a session's move of a card from In progress to Human test is refused until that card's
//! latest verdict is a pass. The verdict code it shares with the card-reviewer is in `agents.rs`.

use crate::agents::{not_passed, Agent, Check, OnStop};
use kanban_store::{Card, Store, TEST_KEY};

/// The subagent's name, as the session calls it (`subagent_type`) and the hook reports it.
pub const TESTER: &str = "card-tester";
/// Who the verdicts are recorded as.
pub const BY: &str = "the app (card-tester subagent)";

const DESCRIPTION: &str = "Independent tester for one card on Hugh's Claude Kanban DSH board. Run it, and wait for its result, before moving a card from In progress to Human test: brief it with the card number, the deliverables and how to try the change, not your conclusions. It tests the real thing, fixes nothing, and ends with CARD and VERDICT lines that the app records on the card.";

const PROMPT: &str = "You are card-tester, an independent tester for one card on Hugh's Claude Kanban DSH board. The session that built the change has briefed you. You haven't seen its work, so don't take its word for anything: find out for yourself.

1. Read the card: kanban_get_card with its number (from the brief). Its summary lists the deliverables; the brief says where the change is and how to try it.
2. Test each deliverable by using the real thing: run the commands, the tests and the app or page the brief names, and look at what happens. Reading the code alone is not a test. Try the obvious failure case for each as well.
3. Fix nothing. Don't edit files, commit, push, merge or deploy, and don't change or move any card. Don't run kb or claude config, and don't change Claude Code's settings. Leave anything you started (test cards, processes) as you found it, or say what is left.
4. Judge strictly. A deliverable that is missing, only partly built, or that you couldn't try is a FAIL: say why. Skip style and wording nits.
5. Evidence, only when the card's repo is hughht5/kanban-app (other repos: skip this step). Shoot only the test app the brief names, on its DevTools port: the brief gives the port (test 9334, test N 9333+N); if it gives none, use 9334. Never 9333, which is the live app. Run CDP_PORT=<that port> node tools/cdp.mjs shot <a .png path in your scratchpad> <css selector of the changed element>. Crop every shot to the changed element, so it shows nothing else (no terminal, no chat, no other card). Take a \"before\" shot of the new build just before the action that shows the change, and an \"after\" shot once it shows; a change you only look at needs only an \"after\". If that port does not answer, or cdp.mjs says the shot is blank, take no shots and say so. cdp.mjs prints the test app's build commit; if the brief names the commit or branch under test and the test app runs another build, take no shots and say so. Never start, stop or restart either app. Name each shot on its own line before the CARD line: EVIDENCE: before|after <full path> | <short caption>. The app copies the files in; only PNGs from your scratchpad are taken.
6. Acceptance checks (card #105): if the card has doneWhenChecks, test each check as well as each deliverable. Before the CARD line, write one line per check, numbered from 1 in the list's order: \"CHECK <n>: PASS\", or \"CHECK <n>: FAIL - <why, in a few words>\". A check you couldn't try is a FAIL, and any failed check makes the VERDICT a FAIL. The app marks each check on the card from these lines.

Report, for each deliverable: what you did, what you saw (the command and its output, or what the screen showed), and PASS or FAIL. Then end your reply with these lines, and nothing after them:
CARD: #<the card's number>
VERDICT: PASS
or, if anything failed, VERDICT: FAIL followed by one line per failure, each starting with \"- \". If the failure is a bug a person can see when they use the app or the page, start its line with \"- BUG: \". The app reads these lines to record the test on the card, and only a PASS lets the card go to Human test.";

pub const CHECK: Check = Check {
    agent: TESTER,
    key: TEST_KEY,
    by: BY,
    label: "Subagent test",
    who: "tester",
    verb: "test",
    reset_in: "In progress",
    // The tester's three-round rule stays in the In progress prompt, as before card #96.
    cap: None,
    // Its EVIDENCE lines' shots go on the card with its verdict (card #101).
    evidence: true,
};

pub const AGENT: Agent = Agent {
    name: TESTER,
    description: DESCRIPTION,
    prompt: PROMPT,
    effort: None,
    edits: false,
    also_denied: &[],
    on_stop: OnStop::Check(&CHECK),
};

/// Columns a card is tested before leaving for Human test (or past it): every column before it.
pub const BEFORE_TEST: [&str; 4] = ["Backlog", "Blocked", "Planning", "In progress"];
/// The columns that need the test first.
pub const INTO_TEST: [&str; 2] = ["Human test", "Deploy complete"];

/// Why a card's session may not move `card` to `to`, if it may not: into Human test (or on past
/// it, to Deploy complete) from any column before it, only once the card's latest subagent test
/// passed, unless Hugh turned the rule off. Every move into In progress makes it needed again.
pub fn gate(store: &Store, card: &Card, to: &str) -> Option<String> {
    let latest = not_passed(&CHECK, card, to, &BEFORE_TEST, &INTO_TEST, store.test_required())?;
    Some(format!(
        "card #{n} hasn't passed a subagent test since it last came into In progress (Hugh's rule: every card is tested by a subagent before Human test).{latest} Run the card-tester subagent (the Agent tool, subagent_type \"{TESTER}\") and wait for its result: if it runs in the background, wait for its notification before you move the card. Brief it like a stranger: the card number (#{n}), the deliverables, where the change is and how to try it (the test app, the dev stack and its logins), not your conclusions. It ends with CARD and VERDICT lines, and the app records its verdict on the card. Fix what it finds and run it again; move the card once its latest verdict is PASS.",
        n = card.number
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn card(fields: Value, column: &str, number: i64) -> Card {
        Card { id: format!("c{number}"), number, column: column.into(), order: None, order_col: None, archived: false, fields: fields.as_object().cloned().unwrap(), version: 1 }
    }

    #[test]
    fn the_move_to_human_test_waits_for_a_pass() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let needed = card(json!({ "title": "T", "test": { "state": "needed" } }), "In progress", 7);
        let why = gate(&s, &needed, "Human test").unwrap();
        assert!(why.contains("card #7 hasn't passed a subagent test") && why.contains("subagent_type \"card-tester\""), "{why}");
        assert!(gate(&s, &needed, "Deploy complete").is_some());
        // No record at all (in In progress before this rule) counts as needed.
        assert!(gate(&s, &card(json!({ "title": "T" }), "In progress", 7), "Human test").is_some());
        let failed = card(json!({ "title": "T", "test": { "state": "failed", "detail": "the chip is missing" } }), "In progress", 7);
        assert!(gate(&s, &failed, "Human test").unwrap().contains("Its latest test failed: the chip is missing."));
        let passed = card(json!({ "title": "T", "test": { "state": "passed" } }), "In progress", 7);
        assert!(gate(&s, &passed, "Human test").is_none());
        // Other moves are free: Done (no diff), Blocked, back to Planning, and from Human test on.
        for to in ["Done", "Blocked", "Planning"] {
            assert!(gate(&s, &needed, to).is_none(), "{to}");
        }
        assert!(gate(&s, &card(json!({ "title": "T" }), "Human test", 7), "Deploy complete").is_none());
        // No way round it through Blocked or straight from Planning.
        assert!(gate(&s, &card(json!({ "title": "T", "test": { "state": "needed" } }), "Blocked", 7), "Human test").is_some());
        assert!(gate(&s, &card(json!({ "title": "T" }), "Planning", 7), "Human test").is_some());
        // A pass still counts after a stop in Blocked (only a move into In progress resets it).
        assert!(gate(&s, &card(json!({ "title": "T", "test": { "state": "passed" } }), "Blocked", 7), "Human test").is_none());
        // Hugh can turn the rule off.
        s.set_page_setting(kanban_store::TEST_RULE_KEY, "off").unwrap();
        assert!(gate(&s, &needed, "Human test").is_none());
    }

    #[test]
    fn the_tester_inherits_the_sessions_effort_and_marks_bugs() {
        assert_eq!(AGENT.effort, None);
        assert!(PROMPT.contains("VERDICT: PASS") && PROMPT.contains("- BUG: "));
        assert!(DESCRIPTION.contains("Human test"));
    }
}
