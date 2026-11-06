// Auto-progress (card #76): a card Hugh set to go on by itself ("Take it to", `autoTo`).

use kanban_store::{clean_auto_to, Patch, Store, AUTO_BY};
use serde_json::{json, Value};

fn patch(v: Value) -> Patch {
    v.as_object().cloned().unwrap()
}

fn board() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("board.db")).unwrap();
    (dir, s)
}

fn card(s: &mut Store, column: &str, auto: Value) -> String {
    s.create(None, patch(json!({ "title": "T", "column": column, "repo": "acme/kanban", "autoTo": auto })), "test").unwrap().id
}

#[test]
fn auto_to_takes_only_its_two_targets() {
    assert_eq!(clean_auto_to(&json!("Human test")).unwrap(), Some(json!("Human test")));
    assert_eq!(clean_auto_to(&json!("  deploy COMPLETE ")).unwrap(), Some(json!("Deploy complete")));
    assert_eq!(clean_auto_to(&Value::Null).unwrap(), None);
    assert_eq!(clean_auto_to(&json!("")).unwrap(), None);
    for bad in [json!("Ship it"), json!("Planning"), json!("Done"), json!(3), json!(["Human test"])] {
        assert!(clean_auto_to(&bad).is_err(), "{bad}");
    }
    let (_d, mut s) = board();
    let id = card(&mut s, "Backlog", json!("human test"));
    let c = s.get(&id).unwrap();
    assert_eq!(c.auto_to(), Some("Human test"));
    assert!(s.create(None, patch(json!({ "title": "T", "autoTo": "Ship it" })), "test").is_err());
    // An update is checked too, and null clears it.
    assert!(s.update(&id, c.version, &patch(json!({ "autoTo": "Done" })), "test").is_err());
    let c = s.update(&id, c.version, &patch(json!({ "autoTo": "Deploy complete" })), "test").unwrap();
    assert_eq!(c.auto_to(), Some("Deploy complete"));
    let c = s.update(&id, c.version, &patch(json!({ "autoTo": null })), "test").unwrap();
    assert_eq!(c.auto_to(), None);
    assert!(!c.fields.contains_key("autoTo"));
}

#[test]
fn hugh_moving_it_himself_turns_it_off_except_into_planning_or_in_progress() {
    let (_d, mut s) = board();
    let id = card(&mut s, "Backlog", json!("Deploy complete"));
    for to in ["Planning", "In progress"] {
        let c = s.get(&id).unwrap();
        let m = s.move_by_hugh(&id, c.version, to, Patch::new()).unwrap();
        assert_eq!(m.card.auto_to(), Some("Deploy complete"), "kept on a move into {to}");
    }
    let c = s.get(&id).unwrap();
    let m = s.move_by_hugh(&id, c.version, "Human test", Patch::new()).unwrap();
    assert_eq!(m.card.auto_to(), None);
    let log = s.log(&id).unwrap();
    assert!(log.iter().any(|l| l.text.contains("Auto-progress off: Hugh moved it himself")), "{log:?}");

    // Moving it to its own target himself says it's done rather than off.
    let id = card(&mut s, "In progress", json!("Human test"));
    let c = s.get(&id).unwrap();
    s.move_by_hugh(&id, c.version, "Human test", Patch::new()).unwrap();
    assert!(s.log(&id).unwrap().iter().any(|l| l.text.contains("Auto-progress done: Hugh moved it there himself")));

    // Backwards (Blocked, Backlog) turns it off too.
    let id = card(&mut s, "Planning", json!("Human test"));
    let c = s.get(&id).unwrap();
    assert_eq!(s.move_by_hugh(&id, c.version, "Blocked", Patch::new()).unwrap().card.auto_to(), None);
}

#[test]
fn the_apps_move_for_hugh_keeps_it_and_marks_the_ship() {
    let (_d, mut s) = board();
    let id = card(&mut s, "Human test", json!("Deploy complete"));
    let c = s.get(&id).unwrap();
    let m = s.move_for_hugh(&id, c.version, "Ship it", "he set it to go to Deploy complete by itself").unwrap();
    assert!(m.ship);
    assert_eq!(m.card.column, "Ship it");
    assert_eq!(m.card.auto_to(), Some("Deploy complete"));
    let ship = m.card.fields.get("ship").unwrap();
    assert_eq!(ship["state"], "requested");
    assert_eq!(ship["auto"], true);
    // Not a move by hand, and logged as the app's.
    assert!(m.card.fields.get("movedByHand").is_none());
    assert_eq!(m.card.fields.get("updatedBy").and_then(Value::as_str), Some(AUTO_BY));
    let log = s.log(&id).unwrap();
    assert!(log.iter().any(|l| l.text == "The app moved it from Human test to Ship it for Hugh: he set it to go to Deploy complete by itself"), "{log:?}");
    assert!(log.iter().any(|l| l.text == "Ship requested"));
    // Hugh's own move into Ship it has no `auto` mark.
    let id = card(&mut s, "Human test", Value::Null);
    let c = s.get(&id).unwrap();
    let m = s.move_by_hugh(&id, c.version, "Ship it", Patch::new()).unwrap();
    assert!(m.card.fields["ship"].get("auto").is_none());
}

#[test]
fn the_apps_move_into_planning_requests_a_plan() {
    let (_d, mut s) = board();
    let id = card(&mut s, "Backlog", json!("Human test"));
    let c = s.get(&id).unwrap();
    let m = s.move_for_hugh(&id, c.version, "Planning", "Hugh added it to go to Human test by itself").unwrap();
    assert!(m.plan);
    assert_eq!(m.card.fields["plan"]["state"], "requested");
    assert_eq!(m.card.auto_to(), Some("Human test"));
}

// The subagent test gate (card #76): every move into In progress needs a fresh test.
#[test]
fn a_move_into_in_progress_needs_a_new_test() {
    let (_d, mut s) = board();
    let id = card(&mut s, "Planning", Value::Null);
    let c = s.update(&id, s.get(&id).unwrap().version, &patch(json!({ "test": { "state": "passed", "at": "2026-11-05T10:00:00Z" } })), "the app").unwrap();
    let m = s.move_by_hugh(&id, c.version, "In progress", Patch::new()).unwrap();
    assert_eq!(m.card.fields["test"]["state"], "needed");
    // A session's own move (an ordinary update of the column) does it too, from Human test back.
    let c = s.update(&id, m.card.version, &patch(json!({ "test": { "state": "passed" }, "column": "Human test" })), "x").unwrap();
    assert_eq!(c.fields["test"]["state"], "passed");
    let c = s.update(&id, c.version, &patch(json!({ "column": "In progress" })), "card #1's session").unwrap();
    assert_eq!(c.fields["test"]["state"], "needed");
    // Moves elsewhere leave it alone.
    let c = s.update(&id, c.version, &patch(json!({ "test": { "state": "passed" } })), "the app").unwrap();
    let c = s.update(&id, c.version, &patch(json!({ "column": "Human test" })), "x").unwrap();
    assert_eq!(c.fields["test"]["state"], "passed");
}

// Card #96: the subagent review resets with the test, and Hugh's rule for it is its own.
#[test]
fn a_move_into_in_progress_needs_a_new_review_too() {
    let (_d, mut s) = board();
    let id = card(&mut s, "Human test", Value::Null);
    let c = s.update(&id, s.get(&id).unwrap().version, &patch(json!({ "review": { "state": "passed", "round": 3, "capped": true } })), "the app").unwrap();
    let c = s.update(&id, c.version, &patch(json!({ "column": "In progress" })), "Hugh").unwrap();
    assert!(c.fields["review"].get("capped").is_none(), "the move into In progress clears the cap");
    assert_eq!(c.fields["review"]["state"], "needed");
    assert!(c.fields["review"].get("round").is_none(), "the FAIL count starts again");
    assert_eq!(c.fields["test"]["state"], "needed");
    // A patch that sets it keeps its own value.
    let c = s.update(&id, c.version, &patch(json!({ "column": "Human test" })), "x").unwrap();
    let c = s.update(&id, c.version, &patch(json!({ "column": "In progress", "review": { "state": "passed" } })), "x").unwrap();
    assert_eq!((c.fields["review"]["state"].as_str(), c.fields["test"]["state"].as_str()), (Some("passed"), Some("needed")));
}

#[test]
fn the_review_rule_is_on_unless_hugh_turns_it_off() {
    let (_d, mut s) = board();
    assert!(s.review_required());
    assert_eq!(s.page_setting("sessions.review_before_human_test").unwrap(), "on");
    assert_eq!(kanban_store::page_default("sessions.review_before_human_test").unwrap(), "on");
    s.set_page_setting("sessions.review_before_human_test", "off").unwrap();
    assert!(!s.review_required());
    assert!(s.test_required(), "the test rule is its own");
    assert!(s.set_page_setting("sessions.review_before_human_test", "sometimes").is_err());
    s.set_page_setting("sessions.review_before_human_test", " on ").unwrap();
    assert!(s.review_required());
}

#[test]
fn the_test_rule_is_on_unless_hugh_turns_it_off() {
    let (_d, mut s) = board();
    assert!(s.test_required());
    assert_eq!(s.page_setting("sessions.test_before_human_test").unwrap(), "on");
    s.set_page_setting("sessions.test_before_human_test", "off").unwrap();
    assert!(!s.test_required());
    assert!(s.set_page_setting("sessions.test_before_human_test", "maybe").is_err());
    s.set_page_setting("sessions.test_before_human_test", " on ").unwrap();
    assert!(s.test_required());
}

// Card #98: a card routed small goes on by itself only while Hugh's rule is on; its route is
// kept through moves (routed once), and only the app writes it.
#[test]
fn the_small_card_rule_is_on_unless_hugh_turns_it_off() {
    let (_d, mut s) = board();
    assert!(s.small_go_on());
    assert_eq!(s.page_setting("sessions.small_go_on").unwrap(), "on");
    assert_eq!(kanban_store::page_default("sessions.small_go_on").unwrap(), "on");
    assert!(kanban_store::PAGE_SETTINGS.contains(&kanban_store::SMALL_GO_ON_KEY));
    s.set_page_setting("sessions.small_go_on", "off").unwrap();
    assert!(!s.small_go_on());
    assert!(s.test_required() && s.review_required(), "the other rules are their own");
    assert!(s.set_page_setting("sessions.small_go_on", "yes").is_err());
    s.set_page_setting("sessions.small_go_on", "on").unwrap();
    assert!(s.small_go_on());
}

#[test]
fn a_route_stays_on_the_card_through_its_moves() {
    let (_d, mut s) = board();
    let id = card(&mut s, "Planning", Value::Null);
    let route = json!({ "size": "small", "model": "claude-sonnet-5-5", "effort": "medium", "check": "5 min", "reason": "r", "by": "the app (card-router subagent)" });
    let c = s.update(&id, s.get(&id).unwrap().version, &patch(json!({ kanban_store::ROUTE_KEY: route.clone() })), "the app (card-router subagent)").unwrap();
    let c = s.update(&id, c.version, &patch(json!({ "column": "In progress" })), "x").unwrap();
    let c = s.update(&id, c.version, &patch(json!({ "column": "Planning" })), "x").unwrap();
    assert_eq!(c.fields["route"], route, "a move doesn't clear it: a card is routed once");
    // Route again clears it.
    let c = s.update(&id, c.version, &patch(json!({ "route": null })), "Hugh (panel)").unwrap();
    assert!(c.fields.get("route").map_or(true, Value::is_null));
}

// Card #112: the reproducer's rule is its own, on by default; and a move into In progress keeps
// the card's repro (a bug report moves the card there and sets it in the same breath).
#[test]
fn the_reproducer_rule_is_on_unless_hugh_turns_it_off() {
    let (_d, mut s) = board();
    let key = kanban_store::REPRO_RULE_KEY;
    assert_eq!(key, "sessions.repro_before_human_test");
    assert!(s.rule_on(key));
    assert!(kanban_store::PAGE_SETTINGS.contains(&key) && kanban_store::RULE_KEYS.contains(&key));
    assert_eq!(s.page_setting(key).unwrap(), "on");
    assert_eq!(kanban_store::page_default(key).unwrap(), "on");
    s.set_page_setting(key, "off").unwrap();
    assert!(!s.rule_on(key));
    assert!(s.test_required() && s.review_required(), "the other rules are their own");
    assert!(s.set_page_setting(key, "later").is_err());
    s.set_page_setting(key, "on").unwrap();
    assert!(s.rule_on(key));
}

#[test]
fn a_move_into_in_progress_keeps_the_repro() {
    let (_d, mut s) = board();
    let id = card(&mut s, "Human test", Value::Null);
    let repro = json!({ "state": "reproduced", "reports": [{ "note": "the chip is wrong" }], "before": { "steps": "1. x" } });
    let c = s.update(&id, s.get(&id).unwrap().version, &patch(json!({ "repro": repro })), "the app").unwrap();
    let c = s.update(&id, c.version, &patch(json!({ "column": "In progress" })), "Hugh").unwrap();
    assert_eq!(c.fields["repro"], repro);
    assert_eq!(c.fields["test"]["state"], "needed");
}
