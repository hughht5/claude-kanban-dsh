// How long a Human test card takes to check (card #103): `checkSize` is removed on every move
// into Human test, and `check_size_due` lists the cards that need one.

use kanban_store::{Patch, Store, CHECK_SIZE_KEY, RESET_ON_HUMAN_TEST};
use serde_json::{json, Value};

fn patch(v: Value) -> Patch {
    v.as_object().cloned().unwrap()
}

fn board() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("board.db")).unwrap();
    (dir, s)
}

fn make(s: &mut Store, column: &str) -> String {
    s.create(None, patch(json!({ "title": "T", "column": column, "repo": "hughht5/kanban-app" })), "test").unwrap().id
}

fn set(s: &mut Store, id: &str, v: Value) {
    let c = s.get(id).unwrap();
    s.update(id, c.version, &patch(v), "test").unwrap();
}

fn has_check(s: &Store, id: &str) -> bool {
    s.get(id).unwrap().fields.contains_key(CHECK_SIZE_KEY)
}

#[test]
fn check_size_is_in_the_reset_list() {
    assert_eq!(RESET_ON_HUMAN_TEST, &["checkSize"]);
}

#[test]
fn every_move_into_human_test_removes_the_old_check() {
    let (_d, mut s) = board();
    let id = make(&mut s, "In progress");
    set(&mut s, &id, json!({ "checkSize": { "class": "quick" } }));
    // A session's move (an ordinary update of column).
    set(&mut s, &id, json!({ "column": "Human test" }));
    assert!(!has_check(&s, &id));

    // Hugh's move, back and forth: the check written meanwhile goes on the way in again.
    set(&mut s, &id, json!({ "checkSize": { "class": "real" } }));
    let c = s.get(&id).unwrap();
    s.move_by_hugh(&id, c.version, "In progress", Patch::new()).unwrap();
    assert!(has_check(&s, &id), "leaving Human test keeps it (it only shows in Human test)");
    let c = s.get(&id).unwrap();
    s.move_by_hugh(&id, c.version, "Human test", Patch::new()).unwrap();
    assert!(!has_check(&s, &id));

    // The app's own move for Hugh (Take it to) too.
    set(&mut s, &id, json!({ "column": "In progress" }));
    set(&mut s, &id, json!({ "checkSize": { "class": "real" } }));
    let c = s.get(&id).unwrap();
    s.move_for_hugh(&id, c.version, "Human test", "a test").unwrap();
    assert!(!has_check(&s, &id));
}

#[test]
fn a_patch_that_sets_it_keeps_it_and_other_writes_leave_it() {
    let (_d, mut s) = board();
    let id = make(&mut s, "In progress");
    set(&mut s, &id, json!({ "column": "Human test", "checkSize": { "class": "quick" } }));
    assert_eq!(s.get(&id).unwrap().fields[CHECK_SIZE_KEY]["class"], "quick");
    // A write in Human test that doesn't move it leaves it alone.
    set(&mut s, &id, json!({ "summary": "new words" }));
    assert!(has_check(&s, &id));
    // No change is recorded for a card that had none.
    let c = make(&mut s, "In progress");
    let before = s.revision_count(&c).unwrap();
    set(&mut s, &c, json!({ "column": "Human test" }));
    assert_eq!(s.revision_count(&c).unwrap(), before + 1);
    assert!(!has_check(&s, &c));
}

#[test]
fn due_lists_only_human_test_cards_without_one() {
    let (_d, mut s) = board();
    let due = make(&mut s, "Human test");
    let done = make(&mut s, "Human test");
    set(&mut s, &done, json!({ "checkSize": { "class": "real" } }));
    let _other = make(&mut s, "In progress");
    let archived = make(&mut s, "Human test");
    let c = s.get(&archived).unwrap();
    s.archive(&archived, c.version, true, "test").unwrap();
    // A stack's child: its lead's check covers it. (The lead is in Human test too, so it is due.)
    let lead = make(&mut s, "Human test");
    let child = make(&mut s, "Human test");
    let (ln, cn) = (s.get(&lead).unwrap().number, s.get(&child).unwrap().number);
    s.stack_add(ln, cn, "test").unwrap();
    let c = s.get(&lead).unwrap();
    s.update(&lead, c.version, &patch(json!({ "checkSize": { "class": "real" } })), "test").unwrap();
    let ids: Vec<String> = s.check_size_due(10).unwrap().into_iter().map(|c| c.id).collect();
    assert_eq!(ids, vec![due.clone()]);
    // The limit holds, lowest number first.
    let second = make(&mut s, "Human test");
    let ids: Vec<String> = s.check_size_due(1).unwrap().into_iter().map(|c| c.id).collect();
    assert_eq!(ids, vec![due]);
    assert_eq!(s.check_size_due(10).unwrap().len(), 2);
    let _ = second;
    // The lead with a record is listed for the stack-changed check; a plain card isn't.
    let leads: Vec<String> = s.check_size_leads().unwrap().into_iter().map(|c| c.id).collect();
    assert_eq!(leads, vec![lead]);
}
