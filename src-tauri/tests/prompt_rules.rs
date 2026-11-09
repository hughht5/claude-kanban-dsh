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
