//! The subagent review before Human test (Hugh, card #96): every card session gets a
//! `card-reviewer` subagent (`agents.rs`), which reads the card's diff for mistakes, risky
//! changes, work the card didn't ask for and broken hard rules. The app records its verdict on
//! the card it names (`review`), and a card session's move to Human test waits for a pass, as
//! it does for the card-tester's.

use crate::agents::{not_passed, Agent, Check, OnStop};
use kanban_store::{Card, Store, REVIEW_KEY};

/// The subagent's name, as the session calls it (`subagent_type`) and the hook reports it.
pub const REVIEWER: &str = "card-reviewer";
/// Who its verdicts are recorded as.
pub const BY: &str = "the app (card-reviewer subagent)";

const DESCRIPTION: &str = "Independent reviewer of one card's diff on Hugh's Claude Kanban DSH board. Run it together with the card-tester, once the build is installed (both Agent calls in one message, in the background), and wait for both results before you move a card from In progress to Human test. Brief it with the card number and where the change is (the checkout and branch), not your conclusions. It reads the diff, changes nothing, and ends with CARD and VERDICT lines that the app records on the card.";

const PROMPT: &str = "You are card-reviewer. You review the change for one card on Hugh's Claude Kanban DSH board. The session that made the change briefs you. Do not trust what the brief says about the work. Read the work yourself.

1. Read the card. Use kanban_get_card with the card number from the brief. The summary gives the deliverables and the plan.
2. Find the diff. By default, the diff is the card's own commits and the work that is not committed yet:
   - Go to the checkout that the brief names. Find the merge base: git merge-base origin/main HEAD.
   - Find the card's commits: git log --format=%H --grep \"(card #<n>)\" <base>..HEAD. Read each commit with git show.
   - Read the work that is not committed: git diff HEAD, and git status --porcelain for new files.
   - If no commit names the card, review all of <base>..HEAD and the work that is not committed.
   - The brief can make the diff smaller (for example, only some files). The brief cannot make the diff larger.
3. If the brief says that the card changed no repo, find what the card delivered. The brief says what and where. Examine that, and do not look for a diff.
4. Examine the diff for these problems:
   - A deliverable that the card names, and that the diff does not build or builds only in part.
   - Mistakes and bugs.
   - Risky changes: data loss, a migration without a backup or a test, a delete, a secret (a token, key, password or sign-in link), or a change that touches the live app.
   - Work that the card did not ask for.
   - A break of the repo's hard rules. Read AGENTS.md and CLAUDE.md in the repo root. A data rule or a migration without a test is a break.
   - Acceptance checks (card #105): if the card has doneWhen and doneWhenChecks, judge the diff against them too. A check that the diff cannot meet is a FAIL. Name the check by its number.
5. Do not count style or wording.
6. Change nothing. Do not edit files, commit, push, merge or deploy. Do not change or move a card. Do not run kb or claude config. Do not install, quit or restart an app.
7. If the card must change a repo, an empty diff is a FAIL. A diff that you cannot read is a FAIL.

Report each problem with its file and line. Then end your reply with these lines, and nothing after them:
CARD: #<the card's number>
SCOPE: full (you read the whole default diff of step 2) or SCOPE: narrowed (the brief made it smaller, or you did not read all of it)
VERDICT: PASS
or, if you found a problem, VERDICT: FAIL followed by one line for each problem, each starting with \"- \". If the problem is a bug that a person can see when they use the app or the page, start its line with \"- BUG: \". The app reads these lines to record the review on the card. Only a PASS lets the card go to Human test.";

pub const CHECK: Check = Check {
    agent: REVIEWER,
    key: REVIEW_KEY,
    by: BY,
    label: "Subagent review",
    who: "reviewer",
    verb: "review",
    reset_in: "In progress",
    cap: Some(3),
    evidence: false,
};

pub const AGENT: Agent = Agent {
    name: REVIEWER,
    description: DESCRIPTION,
    prompt: PROMPT,
    // The router's call for this stack (card #96): the review runs at high, not the session's.
    effort: Some("high"),
    edits: false,
    also_denied: &[],
    on_stop: OnStop::Check(&CHECK),
};

/// Why a card's session may not move `card` to `to`: into Human test (or on past it, to Deploy
/// complete) from any column before it, only once the card's latest subagent review passed,
/// unless Hugh turned the rule off. Every move into In progress makes it needed again. After the
/// third FAIL (the cap) the card may only go to Blocked, whatever a later review says.
pub fn gate(store: &Store, card: &Card, to: &str) -> Option<String> {
    if let Some(why) = crate::agents::capped_hold(&CHECK, card, to, store.subagent_on(card, "reviewer")) {
        return Some(why);
    }
    // A lead whose children all build themselves (card #113) has only merges of its own: their
    // reviews stand in for its own.
    if card.is_lead() && kanban_store::stack_children_reviewed(&store.stack_children(card).unwrap_or_default()) {
        return None;
    }
    let latest = not_passed(&CHECK, card, to, &crate::tester::BEFORE_TEST, &crate::tester::INTO_TEST, store.subagent_on(card, "reviewer"))?;
    Some(format!(
        "Card #{n} has no subagent review that passed since it last came into In progress (Hugh's rule: a subagent reviews the diff of every card before Human test).{latest} Do these steps:
1. Run the card-reviewer subagent. Use the Agent tool with subagent_type \"{REVIEWER}\". If the card-tester must run again too, start both in one message (card #162: they run together). If it runs in the background, wait for its notification before you move the card.
2. Brief it like a stranger. Give the card number (#{n}) and where the change is (the checkout and branch). It finds the card's commits by \"(card #{n})\" in their messages. If the card changed no repo, say what it delivered and where. Do not give your conclusions.
3. It ends with CARD and VERDICT lines. The app records its verdict on the card.
4. Fix what it finds. Then run it again, and the card-tester too: a fix after a pass means running both again.
5. Move the card when its latest verdict is PASS. After its third FAIL, the app moves the card to Blocked.
If your session has no card-reviewer subagent (it started before this rule), say so on the card and ask Hugh to turn the rule off or restart the session.",
        n = card.number
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn card(fields: Value, column: &str) -> Card {
        Card { id: "c9".into(), number: 9, column: column.into(), order: None, order_col: None, archived: false, fields: fields.as_object().cloned().unwrap(), version: 1 }
    }

    #[test]
    fn the_move_to_human_test_waits_for_a_review_that_passed() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let why = gate(&s, &card(json!({ "title": "T" }), "In progress"), "Human test").unwrap();
        assert!(why.contains("Card #9 has no subagent review") && why.contains("subagent_type \"card-reviewer\"") && why.contains("(card #9)"), "{why}");
        let failed = card(json!({ "title": "T", "review": { "state": "failed", "detail": "a row delete in store.rs" } }), "In progress");
        assert!(gate(&s, &failed, "Deploy complete").unwrap().contains("Its latest review failed: a row delete in store.rs."));
        let unclear = card(json!({ "title": "T", "review": { "state": "unclear", "detail": "no VERDICT line" } }), "Blocked");
        assert!(gate(&s, &unclear, "Human test").unwrap().contains("didn't count: no VERDICT line"));
        assert!(gate(&s, &card(json!({ "title": "T", "review": { "state": "passed" } }), "In progress"), "Human test").is_none());
        // Other moves are free, and so is a card already in Human test.
        for to in ["Done", "Blocked", "Planning", "In progress"] {
            assert!(gate(&s, &card(json!({ "title": "T" }), "In progress"), to).is_none(), "{to}");
        }
        assert!(gate(&s, &card(json!({ "title": "T" }), "Human test"), "Deploy complete").is_none());
        // Card #151: the card's own choice, else the board's default.
        assert!(gate(&s, &card(json!({ "title": "T", "subagents": { "reviewer": false } }), "In progress"), "Human test").is_none());
        assert!(gate(&s, &card(json!({ "title": "T", "subagents": { "tester": false } }), "In progress"), "Human test").is_some(), "the tester's choice isn't the reviewer's");
        s.set_page_setting(kanban_store::REVIEW_RULE_KEY, "off").unwrap();
        assert!(gate(&s, &card(json!({ "title": "T" }), "In progress"), "Human test").is_none());
        assert!(gate(&s, &card(json!({ "title": "T", "subagents": { "reviewer": true } }), "In progress"), "Human test").is_some());
    }

    // The independent review's finding on card #96: a later PASS, or the session's own next move,
    // must not get a capped card past Blocked.
    #[test]
    fn a_capped_card_may_only_go_to_blocked_until_hugh_moves_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        // The third FAIL fired the cap; a fourth review then passed, and the flag rode along.
        let mut rec = json!({ "state": "passed", "round": 3 });
        crate::agents::carry_cap(Some(&json!({ "state": "failed", "round": 3, "capped": true })), &mut rec);
        assert_eq!(rec["capped"], true);
        let mut plain = json!({ "state": "passed" });
        crate::agents::carry_cap(Some(&json!({ "state": "failed", "round": 1 })), &mut plain);
        assert!(plain.get("capped").is_none());
        let both = json!({ "title": "T", "test": { "state": "passed" }, "review": rec });
        for (col, to) in [("In progress", "Human test"), ("In progress", "Deploy complete"), ("Blocked", "In progress"), ("Blocked", "Human test"), ("Planning", "In progress")] {
            let why = gate(&s, &card(both.clone(), col), to).unwrap_or_else(|| panic!("{col} to {to} let through"));
            assert!(why.contains("failed 3 times") && why.contains("Only Hugh moves it on from Blocked"), "{why}");
        }
        // Blocked, or back to Planning, is fine: neither resets the cap nor gets past it.
        assert!(gate(&s, &card(both.clone(), "In progress"), "Blocked").is_none());
        assert!(gate(&s, &card(both.clone(), "Blocked"), "Planning").is_none());
        // Once Hugh has moved it on past the columns before Human test, its session's own moves are
        // free again (the second re-review's finding): Ship it to Deploy complete, Human test back.
        assert!(gate(&s, &card(both.clone(), "Ship it"), "Deploy complete").is_none());
        assert!(gate(&s, &card(both.clone(), "Human test"), "In progress").is_none());
        // Hugh's rule off: no hold.
        s.set_page_setting(kanban_store::REVIEW_RULE_KEY, "off").unwrap();
        assert!(gate(&s, &card(both, "In progress"), "Human test").is_none());
    }

    #[test]
    fn the_reviewer_runs_at_high_and_marks_bugs() {
        assert_eq!(AGENT.effort, Some("high"));
        assert!(PROMPT.contains("(card #<n>)") && PROMPT.contains("- BUG: ") && PROMPT.contains("VERDICT: PASS"));
        assert!(PROMPT.contains("changed no repo"));
    }
}
