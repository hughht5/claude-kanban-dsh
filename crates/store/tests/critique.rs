// The plan critique (card #97): the `critique` field's resets, which the card-critic gate reads.

use kanban_store::{page_default, Card, Patch, Store, CRITIC_RULE_KEY, CRITIQUE_KEY, PAGE_SETTINGS};
use serde_json::{json, Value};

fn patch(v: Value) -> Patch {
    v.as_object().cloned().unwrap()
}

fn board() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("board.db")).unwrap();
    (dir, s)
}

fn crit(c: &Card) -> Value {
    c.fields.get(CRITIQUE_KEY).cloned().unwrap_or(Value::Null)
}

fn up(s: &mut Store, c: &Card, p: Value) -> Card {
    s.update(&c.id, c.version, &patch(p), "test").unwrap()
}

/// What the app writes when it records a verdict (agents.rs record_check).
fn verdict(s: &mut Store, c: &Card, state: &str, round: u64) -> Card {
    up(s, c, json!({ CRITIQUE_KEY: { "state": state, "at": "t", "agentId": "a1", "detail": "x", "round": round } }))
}

#[test]
fn a_move_into_planning_resets_the_critique() {
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T", "column": "Backlog" })), "test").unwrap();
    assert_eq!(crit(&c), Value::Null);
    let c = up(&mut s, &c, json!({ "column": "Planning" }));
    assert_eq!((crit(&c)["state"].as_str(), crit(&c)["round"].as_u64()), (Some("needed"), Some(0)));
    // A FAIL, then back to Planning from elsewhere: a fresh start, the count with it.
    let c = verdict(&mut s, &c, "failed", 2);
    let c = up(&mut s, &c, json!({ "column": "Blocked" }));
    assert_eq!(crit(&c)["state"], "failed", "a stop in Blocked keeps it");
    let c = up(&mut s, &c, json!({ "column": "Planning" }));
    assert_eq!((crit(&c)["state"].as_str(), crit(&c)["round"].as_u64()), (Some("needed"), Some(0)));
    // Hugh's drop into Planning resets it too (move_by_hugh goes through the same apply).
    let c = verdict(&mut s, &c, "passed", 0);
    let c = up(&mut s, &c, json!({ "column": "Backlog" }));
    let c = s.move_by_hugh(&c.id, c.version, "Planning", Patch::new()).unwrap().card;
    assert_eq!(crit(&c)["state"], "needed");
}

#[test]
fn a_summary_edit_makes_it_needed_but_keeps_the_fail_count() {
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T", "column": "Backlog" })), "test").unwrap();
    let c = up(&mut s, &c, json!({ "column": "Planning" }));
    let c = verdict(&mut s, &c, "failed", 1);
    let c = up(&mut s, &c, json!({ "summary": "the plan, fixed" }));
    assert_eq!((crit(&c)["state"].as_str(), crit(&c)["round"].as_u64()), (Some("needed"), Some(1)));
    assert!(crit(&c).get("detail").is_none(), "the old findings go with the old plan");
    // A pass, then another edit: needed again.
    let c = verdict(&mut s, &c, "passed", 1);
    let c = up(&mut s, &c, json!({ "summary": "the plan, changed after the pass" }));
    assert_eq!(crit(&c)["state"], "needed");
    // Other fields don't count as the plan.
    let c = verdict(&mut s, &c, "passed", 1);
    let c = up(&mut s, &c, json!({ "nextStep": "build it", "labels": ["DSH"] }));
    assert_eq!(crit(&c)["state"], "passed");
    // The same summary again is no change.
    let c = up(&mut s, &c, json!({ "summary": "the plan, changed after the pass" }));
    assert_eq!(crit(&c)["state"], "passed");
    // A patch that sets the critique itself is left as it is.
    let c = up(&mut s, &c, json!({ "summary": "new", CRITIQUE_KEY: { "state": "passed", "round": 1 } }));
    assert_eq!(crit(&c)["state"], "passed");
}

#[test]
fn the_cap_counts_fails_across_plan_fixes() {
    // Planning > FAIL > fix > FAIL > fix > FAIL: the round reaches 3 (agents.rs caps there),
    // because a summary edit keeps it.
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T", "column": "Backlog" })), "test").unwrap();
    let mut c = up(&mut s, &c, json!({ "column": "Planning" }));
    for i in 0..3u64 {
        let before = crit(&c)["round"].as_u64().unwrap();
        assert_eq!(before, i);
        c = verdict(&mut s, &c, "failed", before + 1);
        if i < 2 {
            c = up(&mut s, &c, json!({ "summary": format!("fix {i}") }));
        }
    }
    assert_eq!((crit(&c)["state"].as_str(), crit(&c)["round"].as_u64()), (Some("failed"), Some(3)));
}

#[test]
fn a_move_into_in_progress_marks_the_build_started() {
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T", "column": "Backlog" })), "test").unwrap();
    let c = up(&mut s, &c, json!({ "column": "Planning" }));
    // Hugh's drag with no critique run: not held, and the build has started.
    let c = s.move_by_hugh(&c.id, c.version, "In progress", Patch::new()).unwrap().card;
    assert_eq!(c.column, "In progress");
    assert_eq!((crit(&c)["state"].as_str(), crit(&c)["started"].as_bool()), (Some("needed"), Some(true)));
    // Blocked mid-build: a summary edit doesn't reset it (the gate lets it back).
    let c = up(&mut s, &c, json!({ "column": "Blocked" }));
    let c = up(&mut s, &c, json!({ "summary": "noted Hugh's answer" }));
    assert_eq!(crit(&c)["started"], true);
    // Nor does a summary edit in In progress.
    let c = up(&mut s, &c, json!({ "column": "In progress" }));
    let c = verdict(&mut s, &c, "passed", 0);
    let c = up(&mut s, &c, json!({ "summary": "built" }));
    assert_eq!(crit(&c)["state"], "passed");
    // A card with no record gets one, marked started.
    let d = s.create(None, patch(json!({ "title": "U", "column": "Backlog" })), "test").unwrap();
    let d = up(&mut s, &d, json!({ "column": "In progress" }));
    assert_eq!((crit(&d)["state"].as_str(), crit(&d)["round"].as_u64(), crit(&d)["started"].as_bool()), (Some("needed"), Some(0), Some(true)));
    // A passed critique stays passed into In progress, with its count.
    let e = s.create(None, patch(json!({ "title": "V", "column": "Planning" })), "test").unwrap();
    let e = up(&mut s, &e, json!({ "column": "Backlog" }));
    let e = up(&mut s, &e, json!({ "column": "Planning" }));
    let e = verdict(&mut s, &e, "passed", 2);
    let e = up(&mut s, &e, json!({ "column": "In progress" }));
    assert_eq!((crit(&e)["state"].as_str(), crit(&e)["round"].as_u64(), crit(&e)["started"].as_bool()), (Some("passed"), Some(2), Some(true)));
    // A verdict recorded once the build started keeps started (the record is replaced whole).
    let e = verdict(&mut s, &e, "failed", 3);
    assert_eq!((crit(&e)["state"].as_str(), crit(&e)["started"].as_bool()), (Some("failed"), Some(true)));
    // Back into Planning: started goes with the reset.
    let e = up(&mut s, &e, json!({ "column": "Planning" }));
    assert!(crit(&e).get("started").is_none());
    // And a verdict there doesn't bring it back.
    let e = verdict(&mut s, &e, "passed", 0);
    assert!(crit(&e).get("started").is_none());
}

#[test]
fn a_summary_edit_in_backlog_resets_a_pass() {
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T", "column": "Backlog" })), "test").unwrap();
    let c = up(&mut s, &c, json!({ "column": "Planning" }));
    let c = verdict(&mut s, &c, "passed", 1);
    let c = up(&mut s, &c, json!({ "column": "Backlog" }));
    assert_eq!(crit(&c)["state"], "passed");
    let c = up(&mut s, &c, json!({ "summary": "a different plan" }));
    assert_eq!((crit(&c)["state"].as_str(), crit(&c)["round"].as_u64()), (Some("needed"), Some(1)));
}

#[test]
fn a_blocked_card_before_its_build_is_reset_by_a_summary_edit() {
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T", "column": "Backlog" })), "test").unwrap();
    let c = up(&mut s, &c, json!({ "column": "Planning" }));
    let c = verdict(&mut s, &c, "failed", 3);
    let c = up(&mut s, &c, json!({ "column": "Blocked" }));
    let c = up(&mut s, &c, json!({ "summary": "Hugh's answer, in the plan" }));
    assert_eq!((crit(&c)["state"].as_str(), crit(&c)["round"].as_u64()), (Some("needed"), Some(3)));
    // A Blocked card with no record (blocked before this rule) stays without one.
    let d = s.create(None, patch(json!({ "title": "U", "column": "Blocked" })), "test").unwrap();
    let d = up(&mut s, &d, json!({ "summary": "edited" }));
    assert_eq!(crit(&d), Value::Null);
}

#[test]
fn the_critic_rule_is_on_by_default_and_on_or_off() {
    let (_d, mut s) = board();
    assert!(PAGE_SETTINGS.contains(&CRITIC_RULE_KEY));
    assert_eq!(CRITIC_RULE_KEY, "sessions.critic_before_build");
    assert_eq!(page_default(CRITIC_RULE_KEY).unwrap(), "on");
    assert!(s.critique_required());
    assert_eq!(s.set_page_setting(CRITIC_RULE_KEY, " off ").unwrap(), "off");
    assert!(!s.critique_required());
    assert!(s.set_page_setting(CRITIC_RULE_KEY, "maybe").is_err());
    assert_eq!(s.page_setting(CRITIC_RULE_KEY).unwrap(), "off");
    // The others are its own: turning it off leaves the test and review rules on.
    assert!(s.test_required() && s.review_required());
}

#[test]
fn an_edit_to_the_acceptance_checks_counts_as_a_plan_edit() {
    // Card #105's fields, when they are on the board: doneWhen and doneWhenChecks.
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T", "column": "Backlog" })), "test").unwrap();
    let c = up(&mut s, &c, json!({ "column": "Planning" }));
    let c = verdict(&mut s, &c, "passed", 0);
    let c = up(&mut s, &c, json!({ "doneWhen": "the chip shows" }));
    assert_eq!(crit(&c)["state"], "needed");
    let c = verdict(&mut s, &c, "passed", 0);
    let c = up(&mut s, &c, json!({ "doneWhenChecks": [{ "text": "a check", "state": "open" }] }));
    assert_eq!(crit(&c)["state"], "needed");
}

#[test]
fn a_small_cards_one_critique_survives_a_plan_edit() {
    // Card #144: a card routed small gets one plan critique. Once it passed or failed, a plan
    // edit keeps the record as it is: no fresh critique, and a FAIL's notes stay for the build.
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T", "column": "Backlog" })), "test").unwrap();
    let c = up(&mut s, &c, json!({ "column": "Planning", "route": { "size": "small" } }));
    assert_eq!(crit(&c)["state"], "needed");
    // Before its critique ran, an edit is just an edit.
    let c = up(&mut s, &c, json!({ "summary": "the plan" }));
    assert_eq!((crit(&c)["state"].as_str(), crit(&c)["round"].as_u64()), (Some("needed"), Some(0)));
    // After a pass.
    let c = up(&mut s, &c, json!({ CRITIQUE_KEY: { "state": "passed", "at": "t", "round": 0, "notes": ["a note"] } }));
    let c = up(&mut s, &c, json!({ "summary": "the plan, tidied" }));
    let c = up(&mut s, &c, json!({ "doneWhenChecks": ["a sharper check"] }));
    assert_eq!((crit(&c)["state"].as_str(), crit(&c)["notes"].clone()), (Some("passed"), json!(["a note"])));
    // After a FAIL: kept, with its findings as notes and its round.
    let c = up(&mut s, &c, json!({ CRITIQUE_KEY: { "state": "failed", "at": "t", "round": 1, "detail": "it drops the docs", "notes": ["it drops the docs"] } }));
    let c = up(&mut s, &c, json!({ "summary": "the plan, fixed for the finding" }));
    assert_eq!((crit(&c)["state"].as_str(), crit(&c)["round"].as_u64(), crit(&c)["notes"].clone()), (Some("failed"), Some(1), json!(["it drops the docs"])));
    // In Backlog too.
    let c = up(&mut s, &c, json!({ "column": "Backlog" }));
    let c = up(&mut s, &c, json!({ "summary": "edited in Backlog" }));
    assert_eq!(crit(&c)["state"], "failed");
    // A move into Planning still starts it again.
    let c = up(&mut s, &c, json!({ "column": "Planning" }));
    assert_eq!((crit(&c)["state"].as_str(), crit(&c)["round"].as_u64()), (Some("needed"), Some(0)));
    assert!(crit(&c).get("notes").is_none());
    // A normal card, a split card, and a small card in a stack still reset on an edit.
    for extra in [json!({ "route": { "size": "normal" } }), json!({ "route": { "size": "split" } }), json!({})] {
        let mut f = json!({ "title": "N", "column": "Planning" });
        f.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap());
        let n = s.create(None, patch(f), "test").unwrap();
        let n = verdict(&mut s, &n, "passed", 0);
        let n = up(&mut s, &n, json!({ "summary": "changed" }));
        assert_eq!(crit(&n)["state"], "needed", "{extra}");
    }
    let lead = s.create(None, patch(json!({ "title": "L", "column": "Planning", "route": { "size": "small" } })), "test").unwrap();
    let child = s.create_in_stack(patch(json!({ "title": "C", "column": "Planning", "route": { "size": "small" } })), lead.number, "test").unwrap();
    for id in [lead.id.clone(), child.id.clone()] {
        let k = s.get(&id).unwrap();
        let k = verdict(&mut s, &k, "passed", 0);
        let k = up(&mut s, &k, json!({ "summary": "changed" }));
        assert_eq!(crit(&k)["state"], "needed", "{}", k.number);
    }
}
