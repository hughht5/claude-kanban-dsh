// Stacked cards (design/stacked-cards/SPEC.md): membership, passed, the stack rules, archive.

use kanban_store::{lead_missing_in_stack, stack_merged, stack_ready, stack_stage, Card, Error, Patch, Store, REVIEW_KEY, STACK_RULE_KEY, TEST_KEY};
use serde_json::{json, Value};
use std::path::PathBuf;

fn patch(v: Value) -> Patch {
    v.as_object().cloned().unwrap()
}

fn temp_board() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    (dir, path)
}

/// A card in `col`; returns its number.
fn card(s: &mut Store, title: &str, col: &str) -> i64 {
    s.create(None, patch(json!({ "title": title, "column": col })), "test").unwrap().number
}

/// Gives card `n` its own subagent test and review, passed (a lead's own work, card #96).
fn pass(s: &mut Store, n: i64) {
    let c = s.get_by_number(n).unwrap();
    s.update(&c.id, c.version, &patch(json!({ "test": { "state": "passed" }, "review": { "state": "passed" } })), "the app").unwrap();
}

fn col(s: &Store, n: i64) -> String {
    s.get_by_number(n).unwrap().column
}

/// A session's move (a plain column update), then the stack rules.
fn mv(s: &mut Store, n: i64, to: &str) -> Vec<kanban_store::StackMove> {
    let c = s.get_by_number(n).unwrap();
    s.update(&c.id, c.version, &patch(json!({ "column": to })), "card session").unwrap();
    s.stack_after_move(&c.id, "card session").unwrap()
}

fn invalid<T: std::fmt::Debug>(r: Result<T, Error>) -> String {
    match r {
        Err(Error::Invalid(m)) => m,
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn add_and_remove_keep_order_and_one_stack_per_card() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let lead = card(&mut s, "lead", "In progress");
    let a = card(&mut s, "a", "Backlog");
    let b = card(&mut s, "b", "Planning");
    let c = card(&mut s, "c", "In progress");
    let other = card(&mut s, "other lead", "In progress");
    let d = card(&mut s, "d", "Backlog");

    s.stack_add(lead, b, "t").unwrap();
    s.stack_add(lead, a, "t").unwrap();
    let l = s.stack_add(lead, c, "t").unwrap();
    assert_eq!(l.stack(), vec![b, a, c]);
    assert!(l.is_lead());
    assert_eq!(s.get_by_number(a).unwrap().stack_of(), Some(lead));
    let kids: Vec<i64> = s.stack_children(&l).unwrap().iter().map(|k| k.number).collect();
    assert_eq!(kids, vec![b, a, c]);
    assert_eq!(s.stack_lead(&s.get_by_number(c).unwrap()).unwrap().unwrap().number, lead);

    // One stack per card; a lead can't be a child; a child can't lead; not itself.
    assert!(invalid(s.stack_add(lead, a, "t")).contains("already in stack"));
    assert!(invalid(s.stack_add(other, a, "t")).contains("at most one stack"));
    assert!(invalid(s.stack_add(other, lead, "t")).contains("leads a stack"));
    assert!(invalid(s.stack_add(a, d, "t")).contains("can't lead one"));
    assert!(invalid(s.stack_add(d, d, "t")).contains("its own stack"));
    assert!(matches!(s.stack_add(lead, 999, "t"), Err(Error::NotFound(_))));

    // Archived and Ship it cards are refused.
    let e = s.get_by_number(d).unwrap();
    s.archive(&e.id, e.version, true, "t").unwrap();
    assert!(invalid(s.stack_add(lead, d, "t")).contains("archived"));
    let shipping = card(&mut s, "shipping", "Ship it");
    assert!(invalid(s.stack_add(lead, shipping, "t")).contains("Ship it"));

    // Remove keeps the order of the rest and makes the child an ordinary card in its column.
    let l = s.stack_remove(lead, a, "t").unwrap();
    assert_eq!(l.stack(), vec![b, c]);
    let ac = s.get_by_number(a).unwrap();
    assert_eq!((ac.stack_of(), ac.column.as_str()), (None, "Backlog"));
    assert!(invalid(s.stack_remove(lead, a, "t")).contains("isn't in stack"));
    s.stack_remove(lead, b, "t").unwrap();
    let l = s.stack_remove(lead, c, "t").unwrap();
    assert!(!l.is_lead() && !l.fields.contains_key("stack"));
    // A former lead can join a stack once it has no children.
    s.stack_add(other, lead, "t").unwrap();
    let log: Vec<String> = s.log(&ac.id).unwrap().into_iter().map(|e| e.text).collect();
    assert!(log.iter().any(|t| t.starts_with("Left stack")) && log.iter().any(|t| t == &format!("Joined stack #{lead}")));
}

#[test]
fn create_in_stack_and_update_refuses_stack_fields() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let lead = card(&mut s, "lead", "In progress");
    let k = s.create_in_stack(patch(json!({ "title": "child", "column": "Planning" })), lead, "t").unwrap();
    assert_eq!((k.stack_of(), k.column.as_str()), (Some(lead), "Planning"));
    assert_eq!(s.get_by_number(lead).unwrap().stack(), vec![k.number]);
    // Not into a child's stack.
    assert!(invalid(s.create_in_stack(patch(json!({ "title": "x" })), k.number, "t")).contains("can't lead one"));

    for f in ["stack", "stackOf", "archivedWith"] {
        let c = s.get(&k.id).unwrap();
        assert!(invalid(s.update(&c.id, c.version, &patch(json!({ (f): 1 })), "t")).contains("stack commands"));
        assert!(invalid(s.create(None, patch(json!({ "title": "y", (f): 1 })), "t")).contains("stack commands"));
    }
    // Choices and passed fields are ordinary writes.
    let c = s.get(&k.id).unwrap();
    let c = s.update(&c.id, c.version, &patch(json!({ "blockedReason": "Which?", "choices": ["A", "B"] })), "t").unwrap();
    assert_eq!(c.fields["choices"], json!(["A", "B"]));
}

#[test]
fn set_passed_on_and_off() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let lead = card(&mut s, "lead", "Human test");
    let k = s.create_in_stack(patch(json!({ "title": "child", "column": "Human test" })), lead, "t").unwrap();
    let c = s.set_passed(&k.id, true, "Hugh (board)").unwrap();
    assert!(c.passed() && c.fields.contains_key("passedAt"));
    assert_eq!(c.column, "Human test");
    // Again is a no-op.
    let v = c.version;
    assert_eq!(s.set_passed(&k.id, true, "Hugh (board)").unwrap().version, v);
    let c = s.set_passed(&k.id, false, "Hugh (board)").unwrap();
    assert!(!c.passed() && !c.fields.contains_key("passed") && !c.fields.contains_key("passedAt"));
    let log: Vec<String> = s.log(&k.id).unwrap().into_iter().map(|e| e.text).collect();
    assert!(log[0].starts_with("Passed taken back") && log[1].starts_with("Passed:"));
}

#[test]
fn rule_all_moves_the_lead_once_every_child_is_ready() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.stack_rule(), "all");
    let lead = card(&mut s, "lead", "In progress");
    pass(&mut s, lead);
    let a = card(&mut s, "a", "In progress");
    let b = card(&mut s, "b", "In progress");
    let c = card(&mut s, "c", "Planning");
    for n in [a, b, c] {
        s.stack_add(lead, n, "t").unwrap();
    }
    assert!(mv(&mut s, a, "Human test").is_empty());
    assert!(mv(&mut s, b, "Done").is_empty());
    assert_eq!(col(&s, lead), "In progress");
    // An archived child doesn't count.
    let cc = s.get_by_number(c).unwrap();
    s.archive(&cc.id, cc.version, true, "t").unwrap();
    let moves = s.stack_after_move(&s.get_by_number(a).unwrap().id, "t").unwrap();
    assert_eq!(moves.len(), 1);
    assert!(moves[0].lead && moves[0].from == "In progress" && moves[0].to == "Human test" && moves[0].number == lead);
    assert_eq!(col(&s, lead), "Human test");
    let cc = s.get_by_number(c).unwrap();
    s.archive(&cc.id, cc.version, false, "t").unwrap();

    // Ready again with c in Human test: nothing more to move.
    assert!(mv(&mut s, c, "Human test").is_empty());
    // A child back to In progress takes the lead back.
    let moves = mv(&mut s, a, "In progress");
    assert_eq!(moves.len(), 1);
    assert_eq!((moves[0].from.as_str(), moves[0].to.as_str()), ("Human test", "In progress"));
    assert_eq!(col(&s, lead), "In progress");
    let log: Vec<String> = s.log(&s.get_by_number(lead).unwrap().id).unwrap().into_iter().map(|e| e.text).collect();
    assert!(log[0].starts_with("The app moved the stack from Human test to In progress"), "{log:?}");
    // A child sent it back, not the lead's own work: the lead keeps its passes (card #96), so the
    // stack goes on again as soon as the child is ready.
    let l = s.get_by_number(lead).unwrap();
    assert_eq!((l.fields["review"]["state"].as_str(), l.fields["test"]["state"].as_str()), (Some("passed"), Some("passed")));
    assert!(mv(&mut s, a, "Human test").len() == 1);
    assert_eq!(col(&s, lead), "Human test");

    // The rule itself.
    let l = s.get_by_number(lead).unwrap();
    let kids = s.stack_children(&l).unwrap();
    assert!(stack_ready(&l, &kids, "all") && stack_ready(&l, &kids, "first"));
    assert!(!stack_ready(&l, &[], "all") && !stack_ready(&l, &[], "first"));
}

#[test]
fn rule_first_moves_the_lead_with_the_first_ready_child() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    s.set_setting(STACK_RULE_KEY, "first").unwrap();
    assert_eq!(s.stack_rule(), "first");
    let lead = card(&mut s, "lead", "Planning");
    pass(&mut s, lead);
    let a = card(&mut s, "a", "In progress");
    let b = card(&mut s, "b", "In progress");
    s.stack_add(lead, a, "t").unwrap();
    s.stack_add(lead, b, "t").unwrap();
    let moves = mv(&mut s, a, "Human test");
    assert_eq!((moves.len(), moves[0].from.as_str()), (1, "Planning"));
    assert_eq!(col(&s, lead), "Human test");
    assert!(mv(&mut s, b, "Blocked").is_empty());
    assert_eq!(col(&s, lead), "Human test");
    assert_eq!(mv(&mut s, a, "In progress").len(), 1);
    assert_eq!(col(&s, lead), "In progress");
    // The page can read and write the rule; anything else is refused.
    assert_eq!(s.page_setting(STACK_RULE_KEY).unwrap(), "first");
    assert_eq!(s.set_page_setting(STACK_RULE_KEY, " all ").unwrap(), "all");
    assert!(matches!(s.set_page_setting(STACK_RULE_KEY, "some"), Err(Error::Invalid(_))));
    assert_eq!(kanban_store::page_default(STACK_RULE_KEY).unwrap(), "all");
}

// Card #96: the lead's own work is held to its subagent test and review too, so a ready stack
// waits for them, says why once, and goes on when they pass.
#[test]
fn a_ready_stack_waits_for_the_leads_own_test_and_review() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let lead = card(&mut s, "lead", "In progress");
    let a = card(&mut s, "a", "In progress");
    s.stack_add(lead, a, "t").unwrap();
    assert!(mv(&mut s, a, "Human test").is_empty());
    assert_eq!(col(&s, lead), "In progress");
    let held = |s: &Store| s.log(&s.get_by_number(lead).unwrap().id).unwrap().into_iter().filter(|e| e.text.starts_with("The stack is ready to test, but")).map(|e| e.text).collect::<Vec<_>>();
    assert_eq!(held(&s), vec![format!("The stack is ready to test, but #{lead}'s own subagent test and subagent review haven't passed: it stays in In progress until it has (#{a} moved)")]);
    // Asked again with nothing new: no second line.
    assert!(s.stack_recheck(lead, "again", "t").unwrap().is_empty());
    assert_eq!(held(&s).len(), 1);
    // The test passes: the reason changes, so it is said again.
    let c = s.get_by_number(lead).unwrap();
    s.update(&c.id, c.version, &patch(json!({ "test": { "state": "passed" } })), "the app").unwrap();
    assert!(s.stack_recheck(lead, "test passed", "t").unwrap().is_empty());
    assert!(held(&s)[0].contains("own subagent review hasn't passed"), "{:?}", held(&s));
    // And the review: the stack goes on, and the hold is cleared.
    pass(&mut s, lead);
    let moves = s.stack_recheck(lead, "review passed", "t").unwrap();
    assert_eq!((moves.len(), col(&s, lead).as_str()), (1, "Human test"));
    assert!(s.get_by_number(lead).unwrap().fields.get("stackHeld").map_or(true, Value::is_null));
    // Hugh's rules off: a lead with neither isn't held.
    let lead2 = card(&mut s, "lead2", "In progress");
    let b = card(&mut s, "b", "In progress");
    s.stack_add(lead2, b, "t").unwrap();
    s.set_page_setting("sessions.test_before_human_test", "off").unwrap();
    s.set_page_setting("sessions.review_before_human_test", "off").unwrap();
    assert_eq!(mv(&mut s, b, "Human test").len(), 1);
    assert_eq!(col(&s, lead2), "Human test");
}

// Card #112: a bug reported on the lead itself holds the stack too, until the card-reproducer
// proves it fixed (or can't reproduce it), while Hugh's reproducer rule is on.
#[test]
fn a_ready_stack_waits_for_the_leads_bug_to_be_proved() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let lead = card(&mut s, "lead", "In progress");
    let a = card(&mut s, "a", "In progress");
    s.stack_add(lead, a, "t").unwrap();
    pass(&mut s, lead);
    let c = s.get_by_number(lead).unwrap();
    s.update(&c.id, c.version, &patch(json!({ "repro": { "state": "reproduced", "reports": [{ "note": "x" }] } })), "the app").unwrap();
    assert!(mv(&mut s, a, "Human test").is_empty());
    assert_eq!(col(&s, lead), "In progress");
    let c = s.get_by_number(lead).unwrap();
    assert!(c.fields["stackHeld"].as_str().unwrap().contains("own reproducer proof hasn't passed"), "{:?}", c.fields["stackHeld"]);
    // Proved fixed: the stack goes on.
    s.update(&c.id, c.version, &patch(json!({ "repro": { "state": "fixed" } })), "the app").unwrap();
    assert_eq!(s.stack_recheck(lead, "fix proved", "t").unwrap().len(), 1);
    assert_eq!(col(&s, lead), "Human test");
    // The rule off: an open bug report doesn't hold a stack.
    let lead2 = card(&mut s, "lead2", "In progress");
    let b = card(&mut s, "b", "In progress");
    s.stack_add(lead2, b, "t").unwrap();
    pass(&mut s, lead2);
    let c = s.get_by_number(lead2).unwrap();
    s.update(&c.id, c.version, &patch(json!({ "repro": { "state": "needed" } })), "the app").unwrap();
    s.set_page_setting("sessions.repro_before_human_test", "off").unwrap();
    assert_eq!(mv(&mut s, b, "Human test").len(), 1);
    assert_eq!(col(&s, lead2), "Human test");
}

// The second re-review of #96: a later PASS must not let a capped lead's stack through on the
// app's own move.
#[test]
fn a_capped_lead_holds_its_stack_even_after_a_later_pass() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let lead = card(&mut s, "lead", "In progress");
    let c = s.get_by_number(lead).unwrap();
    s.update(&c.id, c.version, &patch(json!({ "test": { "state": "passed" }, "review": { "state": "passed", "round": 3, "capped": true } })), "the app").unwrap();
    let a = card(&mut s, "a", "In progress");
    s.stack_add(lead, a, "t").unwrap();
    assert!(mv(&mut s, a, "Human test").is_empty());
    assert_eq!(col(&s, lead), "In progress");
    let log: Vec<String> = s.log(&s.get_by_number(lead).unwrap().id).unwrap().into_iter().map(|e| e.text).collect();
    assert!(log[0].contains("own subagent review (failed 3 times; Hugh moves it on) hasn't passed"), "{log:?}");
}

#[test]
fn lead_to_deploy_complete_carries_children_and_done_archives_them() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let lead = card(&mut s, "lead", "Human test");
    let a = card(&mut s, "a", "Human test");
    let b = card(&mut s, "b", "Deploy complete");
    let c = card(&mut s, "c", "Human test");
    for n in [a, b, c] {
        s.stack_add(lead, n, "t").unwrap();
    }
    // Then it ships (a session can't add to a stack already in Ship it: card #89).
    let lc = s.get_by_number(lead).unwrap();
    s.update(&lc.id, lc.version, &patch(json!({ "column": "Ship it" })), "Hugh (board)").unwrap();
    let cc = s.get_by_number(c).unwrap();
    s.archive(&cc.id, cc.version, true, "t").unwrap();
    let moves = mv(&mut s, lead, "Deploy complete");
    assert_eq!(moves.len(), 1);
    assert!(!moves[0].lead && moves[0].number == a && moves[0].to == "Deploy complete");
    assert_eq!((col(&s, a), col(&s, b), col(&s, c)), ("Deploy complete".into(), "Deploy complete".into(), "Human test".into()));
    // Done archives the lead (card #83), and the lead takes its children into the archive with it.
    let moves = mv(&mut s, lead, "Done");
    assert!(moves.is_empty());
    for k in [a, b] {
        let kc = s.get_by_number(k).unwrap();
        assert!(kc.archived);
        assert_eq!(kc.column, "Deploy complete");
        let log: Vec<String> = s.log(&kc.id).unwrap().into_iter().map(|e| e.text).collect();
        assert_eq!(log[0], format!("Archived with its stack #{lead}"));
    }
    // c was archived on its own before, so it isn't the stack's to restore.
    assert!(s.get_by_number(c).unwrap().fields.get("archivedWith").is_none());
}

#[test]
fn archive_lead_archives_children_and_restore_brings_back_only_those() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let lead = card(&mut s, "lead", "In progress");
    let a = card(&mut s, "a", "In progress");
    let b = card(&mut s, "b", "In progress");
    s.stack_add(lead, a, "t").unwrap();
    s.stack_add(lead, b, "t").unwrap();
    // b was archived on its own first.
    let bc = s.get_by_number(b).unwrap();
    s.archive(&bc.id, bc.version, true, "t").unwrap();
    let l = s.get_by_number(lead).unwrap();
    s.archive(&l.id, l.version, true, "Hugh (board)").unwrap();
    let ac = s.get_by_number(a).unwrap();
    assert!(ac.archived);
    assert_eq!(ac.fields["archivedWith"], json!(lead));
    assert!(!s.get_by_number(b).unwrap().fields.contains_key("archivedWith"));
    // The rules leave an archived stack alone.
    assert!(s.stack_after_move(&ac.id, "t").unwrap().is_empty());

    let l = s.get_by_number(lead).unwrap();
    s.archive(&l.id, l.version, false, "Hugh (board)").unwrap();
    let ac = s.get_by_number(a).unwrap();
    assert!(!ac.archived && !ac.fields.contains_key("archivedWith"));
    assert!(s.get_by_number(b).unwrap().archived);
    let log: Vec<String> = s.log(&ac.id).unwrap().into_iter().map(|e| e.text).collect();
    assert_eq!(log[0], format!("Restored with its stack #{lead}"));
    assert_eq!(log[1], format!("Archived with its stack #{lead}"));
}

#[test]
fn hugh_cant_move_a_child_into_ship_it() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let lead = card(&mut s, "lead", "Human test");
    let a = card(&mut s, "a", "Human test");
    s.stack_add(lead, a, "t").unwrap();
    let ac = s.get_by_number(a).unwrap();
    assert!(invalid(s.move_by_hugh(&ac.id, ac.version, "Ship it", Patch::new())).contains("ships with its stack"));
    // The lead can go.
    let l = s.get_by_number(lead).unwrap();
    assert!(s.move_by_hugh(&l.id, l.version, "Ship it", Patch::new()).unwrap().ship);
}

#[test]
fn migration_8_adds_the_rule_and_leaves_cards_alone() {
    let (dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), kanban_store::SCHEMA_VERSION);
    assert_eq!(s.setting(STACK_RULE_KEY).unwrap().as_deref(), Some("all"));
    let c = s.create(Some("x"), patch(json!({ "title": "loose", "column": "In progress", "summary": "s", "links": [{ "label": "PR", "url": "https://example.com" }] })), "t").unwrap();
    drop(s);
    let read = |p: &PathBuf| -> (String, i64, String) {
        let conn = rusqlite::Connection::open(p).unwrap();
        conn.query_row("SELECT fields, version, col FROM cards WHERE id = 'x'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap()
    };
    let before = read(&path);
    // Back to a schema-7 board without the setting (renamed aside, since nothing is removed).
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("UPDATE settings SET key = 'old.stacks' WHERE key = ?1", [STACK_RULE_KEY]).unwrap();
        conn.pragma_update(None, "user_version", 7).unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), kanban_store::SCHEMA_VERSION);
    assert_eq!(s.setting(STACK_RULE_KEY).unwrap().as_deref(), Some("all"));
    let after = s.get("x").unwrap();
    assert_eq!((after.version, after.stack_of(), after.is_lead()), (c.version, None, false));
    drop(s);
    assert_eq!(read(&path), before);
    // The migration took a backup first.
    assert!(std::fs::read_dir(dir.path().join("backups")).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with("pre-migration.db")));
    // A board that already had a rule keeps it.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("UPDATE settings SET value = 'first' WHERE key = ?1", [STACK_RULE_KEY]).unwrap();
        conn.pragma_update(None, "user_version", 7).unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.stack_rule(), "first");
}

// Card #113: each child builds on its own branch and the lead merges it. The rules below read
// the merge record (`merge: {state, ...}`, card #100's field) through `stack_merged`.

/// A card in memory, for the pure rules.
fn mem(n: i64, col: &str, fields: Value) -> Card {
    Card { id: format!("card-{n}"), number: n, column: col.into(), order: None, order_col: None, archived: false, fields: patch(fields), version: 1 }
}

/// A card on its own branch, in `col`; returns its number.
fn branched(s: &mut Store, title: &str, col: &str, branch: &str) -> i64 {
    s.create(None, patch(json!({ "title": title, "column": col, "branch": branch })), "test").unwrap().number
}

/// The app's write of fields on card `n`.
fn set(s: &mut Store, n: i64, v: Value) {
    let c = s.get_by_number(n).unwrap();
    s.update(&c.id, c.version, &patch(v), "the app").unwrap();
}

fn merge_passed() -> Value {
    json!({ "merge": { "state": "passed", "at": "t", "detail": "merged" } })
}

fn log_of(s: &Store, n: i64) -> Vec<String> {
    s.log(&s.get_by_number(n).unwrap().id).unwrap().into_iter().map(|e| e.text).collect()
}

fn has_merge(s: &Store, n: i64) -> bool {
    s.get_by_number(n).unwrap().fields.get("merge").map_or(false, |m| !m.is_null())
}

#[test]
fn merged_takes_the_merge_record_or_a_child_with_no_branch_of_its_own() {
    let lead = mem(1, "In progress", json!({ "branch": "stk" }));
    // The old way (critic finding 16): no branch, a blank one, or the lead's.
    assert!(stack_merged(&mem(2, "Human test", json!({})), &lead));
    assert!(stack_merged(&mem(2, "Human test", json!({ "branch": null })), &lead));
    assert!(stack_merged(&mem(2, "Human test", json!({ "branch": "  " })), &lead));
    assert!(stack_merged(&mem(2, "Human test", json!({ "branch": " stk " })), &lead));
    // Its own branch: only a passed merge record counts.
    assert!(!stack_merged(&mem(2, "Human test", json!({ "branch": "stk-2" })), &lead));
    assert!(!stack_merged(&mem(2, "Human test", json!({ "branch": "stk-2", "merge": { "state": "failed" } })), &lead));
    assert!(!stack_merged(&mem(2, "Human test", json!({ "branch": "stk-2", "merge": null })), &lead));
    assert!(stack_merged(&mem(2, "Human test", json!({ "branch": "stk-2", "merge": { "state": "passed" } })), &lead));
    // A lead with no branch doesn't make every child's branch its own.
    assert!(!stack_merged(&mem(2, "Human test", json!({ "branch": "stk-2" })), &mem(1, "In progress", json!({}))));
    // A child that builds itself (its own session started in the stack) is merged only by its
    // record: no branch, or the lead's branch, doesn't skip the merge (code review, card #113).
    assert!(!stack_merged(&mem(2, "Human test", json!({ "stackOwnBuild": true })), &lead));
    assert!(!stack_merged(&mem(2, "Human test", json!({ "stackOwnBuild": true, "branch": "stk" })), &lead));
    assert!(stack_merged(&mem(2, "Human test", json!({ "stackOwnBuild": true, "merge": { "state": "passed" } })), &lead));
}

#[test]
fn stack_stage_gives_each_stage() {
    let lead = mem(1, "In progress", json!({ "branch": "stk" }));
    let own = json!({ "branch": "stk-2" });
    let merged = json!({ "branch": "stk-2", "merge": { "state": "passed" } });
    for c in ["Ship it", "Deploy complete", "Done"] {
        assert_eq!(stack_stage(&mem(2, c, own.clone()), &lead, true), "shipped");
    }
    assert_eq!(stack_stage(&mem(2, "Human test", own.clone()), &lead, false), "to-merge");
    assert_eq!(stack_stage(&mem(2, "Human test", json!({ "branch": "stk-2", "passed": true })), &lead, false), "to-merge", "Passed before the merge doesn't count");
    assert_eq!(stack_stage(&mem(2, "Human test", merged.clone()), &lead, false), "merged");
    assert_eq!(stack_stage(&mem(2, "Human test", json!({ "branch": "stk-2", "merge": { "state": "passed" }, "passed": true })), &lead, false), "passed");
    assert_eq!(stack_stage(&mem(2, "Human test", json!({ "passed": true })), &lead, false), "passed", "the old way: merged already");
    for c in ["Backlog", "Planning", "Blocked"] {
        assert_eq!(stack_stage(&mem(2, c, own.clone()), &lead, true), "waiting");
        assert_eq!(stack_stage(&mem(2, c, own.clone()), &lead, false), "building");
    }
    assert_eq!(stack_stage(&mem(2, "In progress", own.clone()), &lead, true), "building", "a child in In progress has started");
    assert_eq!(stack_stage(&mem(2, "In progress", own), &lead, false), "building");
}

#[test]
fn stack_ready_counts_a_child_in_human_test_once_merged() {
    let lead = mem(1, "In progress", json!({ "branch": "stk" }));
    let a = mem(2, "Human test", json!({ "branch": "stk-a" }));
    let b = mem(3, "Done", json!({ "branch": "stk-b" }));
    assert!(!stack_ready(&lead, &[a.clone(), b.clone()], "all"));
    assert!(stack_ready(&lead, &[a.clone(), b.clone()], "first"), "b is past Human test");
    assert!(!stack_ready(&lead, &[a.clone()], "first"), "a isn't merged yet");
    let a = mem(2, "Human test", json!({ "branch": "stk-a", "merge": { "state": "passed" } }));
    assert!(stack_ready(&lead, &[a.clone(), b.clone()], "all") && stack_ready(&lead, &[a.clone()], "first"));
    // An archived child doesn't count, merged or not.
    let mut c = mem(4, "Human test", json!({ "branch": "stk-c" }));
    c.archived = true;
    assert!(stack_ready(&lead, &[a.clone(), b.clone(), c.clone()], "all"));
    assert!(!stack_ready(&lead, &[c.clone()], "all") && !stack_ready(&lead, &[c], "first"), "no live child");
    // The old way: a child with no branch of its own is ready in Human test.
    assert!(stack_ready(&lead, &[mem(5, "Human test", json!({}))], "all"));
    assert!(!stack_ready(&lead, &[], "all") && !stack_ready(&lead, &[], "first"));
}

#[test]
fn lead_missing_in_stack_takes_the_childrens_reviews_for_the_leads() {
    let rules = [(TEST_KEY, "subagent test", true), (REVIEW_KEY, "subagent review", true)];
    let lead = mem(1, "In progress", json!({}));
    let reviewed = |n| mem(n, "Human test", json!({ "review": { "state": "passed" }, "stackOwnBuild": true }));
    let kids = [reviewed(2), reviewed(3)];
    assert_eq!(lead_missing_in_stack(&lead, &kids, &rules), ["subagent test"], "the lead's own test is still needed");
    // One child's review hasn't passed: the lead's own is needed.
    let not = mem(4, "In progress", json!({ "review": { "state": "needed" }, "stackOwnBuild": true }));
    assert_eq!(lead_missing_in_stack(&lead, &[reviewed(2), not.clone()], &rules), ["subagent test", "subagent review"]);
    // An archived child doesn't count.
    let mut gone = not;
    gone.archived = true;
    assert_eq!(lead_missing_in_stack(&lead, &[reviewed(2), gone.clone()], &rules), ["subagent test"]);
    // No live child: the lead's own, as card #96 had it.
    assert_eq!(lead_missing_in_stack(&lead, &[gone], &rules), ["subagent test", "subagent review"]);
    assert_eq!(lead_missing_in_stack(&lead, &[], &rules), ["subagent test", "subagent review"]);
    // The lead's own review still counts, and a rule that is off holds nothing.
    let own = mem(1, "In progress", json!({ "review": { "state": "passed" } }));
    assert_eq!(lead_missing_in_stack(&own, &[mem(2, "Human test", json!({}))], &rules), ["subagent test"]);
    let off = [(TEST_KEY, "subagent test", false), (REVIEW_KEY, "subagent review", true)];
    assert!(lead_missing_in_stack(&lead, &kids, &off).is_empty());
    // Children its lead built the old way: the lead's own review is still needed (card #96).
    let old_way = [mem(2, "Human test", json!({ "review": { "state": "passed" } }))];
    assert_eq!(lead_missing_in_stack(&lead, &old_way, &rules), ["subagent test", "subagent review"]);
}

#[test]
fn a_lead_goes_to_human_test_only_once_its_children_are_merged() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let lead = branched(&mut s, "lead", "In progress", "stk");
    // The lead's own test of the integrated build passed; its own review never ran.
    set(&mut s, lead, json!({ "test": { "state": "passed" } }));
    let a = branched(&mut s, "a", "In progress", "stk-a");
    let b = branched(&mut s, "b", "In progress", "stk-b");
    for n in [a, b] {
        s.stack_add(lead, n, "t").unwrap();
    }
    // Each child passed its own tester and reviewer, and moved to Human test.
    for n in [a, b] {
        set(&mut s, n, json!({ "test": { "state": "passed" }, "review": { "state": "passed" }, "stackOwnBuild": true }));
        assert!(mv(&mut s, n, "Human test").is_empty());
    }
    assert_eq!(col(&s, lead), "In progress", "neither child is merged");
    set(&mut s, a, merge_passed());
    assert!(s.stack_recheck(lead, &format!("#{a} merged"), "the app").unwrap().is_empty());
    assert_eq!(col(&s, lead), "In progress", "b isn't merged yet");
    set(&mut s, b, merge_passed());
    let moves = s.stack_recheck(lead, &format!("#{b} merged"), "the app").unwrap();
    assert_eq!((moves.len(), col(&s, lead).as_str()), (1, "Human test"), "the children's reviews stand for the lead's");
    assert!(s.get_by_number(lead).unwrap().fields.get("stackHeld").map_or(true, Value::is_null));

    // A merged child goes back (Found a problem): its merge is cleared and the stack goes back.
    let moves = mv(&mut s, a, "In progress");
    assert_eq!((moves.len(), col(&s, lead).as_str()), (1, "In progress"));
    assert!(!has_merge(&s, a) && has_merge(&s, b));
    assert!(log_of(&s, a).contains(&"Merge record cleared: it went back to In progress, so the stack's lead merges it again once it passes".to_string()), "{:?}", log_of(&s, a));
    assert_eq!(s.get_by_number(lead).unwrap().fields["test"]["state"], "passed", "the lead keeps its own test");
    // Fixed and back in Human test: not ready until it is merged again.
    set(&mut s, a, json!({ "test": { "state": "passed" }, "review": { "state": "passed" } }));
    assert!(mv(&mut s, a, "Human test").is_empty());
    assert_eq!(col(&s, lead), "In progress");
    set(&mut s, a, merge_passed());
    assert_eq!(s.stack_recheck(lead, &format!("#{a} merged"), "the app").unwrap().len(), 1);
    assert_eq!(col(&s, lead), "Human test");
}

#[test]
fn a_child_in_the_ready_columns_keeps_its_merge_record() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let lead = branched(&mut s, "lead", "Human test", "stk");
    let a = branched(&mut s, "a", "Human test", "stk-a");
    let b = branched(&mut s, "b", "Human test", "stk-b");
    s.stack_add(lead, a, "t").unwrap();
    s.stack_add(lead, b, "t").unwrap();
    set(&mut s, a, merge_passed());
    set(&mut s, b, merge_passed());
    s.stack_after_move(&s.get_by_number(a).unwrap().id, "t").unwrap();
    assert!(has_merge(&s, a));
    mv(&mut s, a, "Deploy complete");
    assert!(has_merge(&s, a));
    // Back to Blocked is going back too.
    mv(&mut s, b, "Blocked");
    assert!(!has_merge(&s, b));
    assert!(log_of(&s, b).iter().any(|t| t == "Merge record cleared: it went back to Blocked, so the stack's lead merges it again once it passes"));
    // A card in no stack keeps its own merge record (card #100's ship merge) wherever it goes.
    let loose = branched(&mut s, "loose", "Human test", "x");
    set(&mut s, loose, merge_passed());
    mv(&mut s, loose, "In progress");
    assert!(has_merge(&s, loose));
    // A child with no merge record logs nothing.
    let c = branched(&mut s, "c", "Human test", "stk-c");
    s.stack_add(lead, c, "t").unwrap();
    mv(&mut s, c, "In progress");
    assert!(!log_of(&s, c).iter().any(|t| t.starts_with("Merge record cleared")));
}

#[test]
fn passed_waits_for_the_merge_and_again_after_found_a_problem() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let lead = branched(&mut s, "lead", "Human test", "stk");
    let a = branched(&mut s, "a", "Human test", "stk-a");
    s.stack_add(lead, a, "t").unwrap();
    let id = s.get_by_number(a).unwrap().id;
    let stage = |s: &Store| stack_stage(&s.get_by_number(a).unwrap(), &s.get_by_number(lead).unwrap(), false);
    let refused = format!("#{a} isn't merged into the stack's branch yet, so it can't be passed: the stack's lead merges it first");
    assert_eq!(stage(&s), "to-merge");
    assert_eq!(invalid(s.set_passed(&id, true, "Hugh (board)")), refused);
    assert!(!s.get_by_number(a).unwrap().passed());
    // Taking it back is always allowed.
    assert!(!s.set_passed(&id, false, "Hugh (board)").unwrap().passed());

    // Merged > passed (finding 20).
    set(&mut s, a, merge_passed());
    assert_eq!(stage(&s), "merged");
    assert!(s.set_passed(&id, true, "Hugh (board)").unwrap().passed());
    assert_eq!(stage(&s), "passed");
    // > Found a problem (as the app's stack_problem does it): back to In progress, Passed
    // taken back, the stack rules; the merge is cleared.
    let c = s.get(&id).unwrap();
    s.update(&id, c.version, &patch(json!({ "column": "In progress" })), "Hugh (board)").unwrap();
    s.set_passed(&id, false, "Hugh (board)").unwrap();
    s.stack_after_move(&id, "Hugh (board)").unwrap();
    assert!(!has_merge(&s, a));
    assert_eq!(stage(&s), "building");
    // > back in Human test: not testable until it is merged again.
    mv(&mut s, a, "Human test");
    assert_eq!(stage(&s), "to-merge");
    assert_eq!(invalid(s.set_passed(&id, true, "Hugh (board)")), refused);
    set(&mut s, a, merge_passed());
    assert!(s.set_passed(&id, true, "Hugh (board)").unwrap().passed());

    // A Passed left on a card whose merge went away can still be taken back.
    set(&mut s, a, json!({ "merge": null }));
    assert!(!s.set_passed(&id, false, "Hugh (board)").unwrap().passed());
    // The old way: a child on the lead's branch, or with none, is passed as before.
    let b = branched(&mut s, "b", "Human test", "stk");
    s.stack_add(lead, b, "t").unwrap();
    assert!(s.set_passed(&s.get_by_number(b).unwrap().id, true, "Hugh (board)").unwrap().passed());
    // A card in no stack isn't held to a merge.
    let loose = branched(&mut s, "loose", "Human test", "x");
    assert!(s.set_passed(&s.get_by_number(loose).unwrap().id, true, "Hugh (board)").unwrap().passed());
}
