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
