//! The rules for this app's own cards (card #148, stack #145): every card builds in its own
//! worktree and branch, tests in its own test app, and ships alone (one at a time per repo) or
//! as one stack. The old rule, one kanban-app card at a time on `main` in the shared checkout,
//! must not come back in the bundled prompts or AGENTS.md.

const KANBAN_APP: &str = include_str!("../../prompts/kanban-app.md");
const PLANNING: &str = include_str!("../../prompts/planning.md");
const IN_PROGRESS: &str = include_str!("../../prompts/in-progress.md");
const ROUTER: &str = include_str!("../../prompts/router.md");
const AGENTS: &str = include_str!("../../AGENTS.md");

/// Lower case, with Markdown's `*` and backticks gone and every run of white space one space,
/// so a rule is found whatever its line breaks and formatting.
fn plain(text: &str) -> String {
    text.replace(['*', '`'], "").to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

const OLD_RULE: [&str; 6] = [
    "builds at a time",
    "because they share the checkout",
    "no worktree, no branch",
    "commit on main locally",
    "head:refs/heads/card-",
    "the test app is shared",
];

#[test]
fn the_old_one_card_at_a_time_rule_is_gone() {
    for (name, text) in [("kanban-app.md", KANBAN_APP), ("planning.md", PLANNING), ("in-progress.md", IN_PROGRESS), ("router.md", ROUTER), ("AGENTS.md", AGENTS)] {
        let t = plain(text);
        for old in OLD_RULE {
            assert!(!t.contains(old), "{name} still says \"{old}\"");
        }
    }
}

#[test]
fn a_card_builds_in_its_own_worktree_and_never_in_the_checkout() {
    let t = plain(KANBAN_APP);
    assert!(t.contains("<checkout>/.claude/worktrees/card-<n> on the branch card-<n>"), "kanban-app.md names the card's worktree and branch");
    assert!(t.contains("sessions.own_worktree_repos"), "kanban-app.md says when the app makes it");
    assert!(t.contains("worktree add --no-track -b card-<n>"), "kanban-app.md says how to make it when the app hasn't");
    assert!(t.contains("never in <checkout> itself"), "kanban-app.md keeps cards out of the main checkout");
    let a = plain(AGENTS);
    assert!(a.contains("## parallel builds"), "AGENTS.md has the parallel-build rule");
    assert!(a.contains(".claude\\worktrees\\card-<n>"), "AGENTS.md names the card's worktree");
    // A stack child's worktree is in the same place as every other card's (card #146).
    for (name, text) in [("planning.md", PLANNING), ("in-progress.md", IN_PROGRESS)] {
        assert!(!text.contains("`<checkout>-<n>`"), "{name} still puts a child's worktree beside the checkout");
        assert!(plain(text).contains("<checkout>/.claude/worktrees/card-<n>"), "{name} names a child's worktree");
    }
}

#[test]
fn each_card_tests_in_its_own_test_app() {
    let t = plain(KANBAN_APP);
    assert!(t.contains("its own test app"), "kanban-app.md gives each card its own test app");
    assert!(t.contains("testapp"), "kanban-app.md names the card's testApp field");
    assert!(t.contains("-channel <its channel>"), "kanban-app.md installs into the card's own test app");
    assert!(!t.contains("install.ps1 -channel test (add"), "kanban-app.md still installs every card into Test");
    assert!(plain(IN_PROGRESS).contains("the card's own test app"), "in-progress.md briefs the tester with the card's own test app");
    assert!(plain(AGENTS).contains("testapp"), "AGENTS.md names the card's testApp field");
}

#[test]
fn cards_ship_one_at_a_time_rebased_or_as_one_stack() {
    let t = plain(KANBAN_APP);
    assert!(t.contains("ship one at a time"), "kanban-app.md ships one card at a time");
    assert!(t.contains("waiting to ship"), "kanban-app.md names the ship queue");
    assert!(t.contains("rebases that branch onto the latest origin/main"), "kanban-app.md rebases at ship");
    // Card #162: in the repo's merge lane, never the card-merger.
    assert!(t.contains("merge lane") && !t.contains("card-merger"), "kanban-app.md ships through the merge lane only");
    assert!(t.contains("merge --ff-only <branch>"), "kanban-app.md fast-forwards main to the card's branch");
    assert!(t.contains("ship together, once, on the lead"), "kanban-app.md ships a stack as one");
    // Ship it can't remove the folder its own session runs in; it logs how to.
    assert!(t.contains("worktree remove <worktreepath>"), "kanban-app.md says how a card's worktree is removed");
}

#[test]
fn the_router_no_longer_integrates_single_cards() {
    let r = plain(ROUTER);
    assert!(!r.contains("mode: integrate"), "router.md still briefs the merger to integrate");
    assert!(!r.contains("after a wave"), "router.md still installs the test app after a wave");
    assert!(r.contains("do not integrate cards onto an integration branch"), "router.md says not to");
    assert!(r.contains("its own test app"), "router.md says each card tests in its own test app");
}

// Card #162: the card-reviewer and the card-tester run together, on the installed build, and a
// FAIL from either means running both again. The old order (review first, before the install)
// must not come back.
#[test]
fn the_reviewer_and_the_tester_run_together_after_the_install() {
    let ip = plain(IN_PROGRESS);
    for old in ["review before you test", "review it first", "once the review has passed", "and then your card-tester"] {
        assert!(!ip.contains(old), "in-progress.md still says \"{old}\"");
    }
    for new in ["### review and test it together", "two agent calls in one message", "run_in_background: true", "wait for both notifications", "on any fail, from either one", "started before the other's latest fail"] {
        assert!(ip.contains(new), "in-progress.md doesn't say \"{new}\"");
    }
    // Install first, then the pair; the reproducer's flow stays before the pair.
    assert!(ip.find("install it first").unwrap() < ip.find("start both at once").unwrap());
    assert!(ip.find("### prove a reported bug first").unwrap() < ip.find("### review and test it together").unwrap());
    // The stack child's section says they run together too.
    assert!(ip.contains("run your card-reviewer and your card-tester together"), "in-progress.md's stack child section");
    let k = plain(KANBAN_APP);
    for old in ["review your diff now, before the install", "have the card-tester subagent test it"] {
        assert!(!k.contains(old), "kanban-app.md still says \"{old}\"");
    }
    assert!(k.contains("don't run the card-reviewer yet"), "kanban-app.md 4a defers the review");
    assert!(k.contains("6a. then run the card-reviewer and the card-tester together"), "kanban-app.md 6a runs both");
    assert!(k.find("5. install it in your test app").unwrap() < k.find("6a. then run the card-reviewer").unwrap());
}

// Card #162: no agent text says the review comes first.
#[test]
fn no_agent_text_says_the_review_comes_first() {
    for (name, text) in [("reviewer.rs", include_str!("../src/reviewer.rs")), ("tester.rs", include_str!("../src/tester.rs")), ("agents.rs", include_str!("../src/agents.rs"))] {
        let t = plain(text);
        for old in ["before the card-tester", "review before you test", "before the install"] {
            assert!(!t.contains(old), "{name} still says \"{old}\"");
        }
    }
}

/// Hugh (card #172): this repo's cards test with no computer use and no browser; cdp.mjs into
/// the card's own test app stays.
#[test]
fn this_repos_cards_test_without_computer_use_or_a_browser() {
    for (name, text) in [("kanban-app.md", KANBAN_APP), ("AGENTS.md", AGENTS)] {
        let t = plain(text);
        assert!(t.contains("no computer use and no browser"), "{name} states the rule");
        for want in ["claude in chrome", "built-in browser", "chrome-devtools", "tools/cdp.mjs", "card #172"] {
            assert!(t.contains(want), "{name} says \"{want}\"");
        }
    }
}
