// Stacked cards (design/stacked-cards/SPEC.md): membership, passed, the stack rules, archive.

use kanban_store::{stack_ready, Error, Patch, Store, STACK_RULE_KEY};
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
    assert!(mv(&mut s, a, "Human test").len() == 1);
    assert_eq!(col(&s, lead), "Human test");

    // The rule itself.
    let kids = s.stack_children(&s.get_by_number(lead).unwrap()).unwrap();
    assert!(stack_ready(&kids, "all") && stack_ready(&kids, "first"));
    assert!(!stack_ready(&[], "all") && !stack_ready(&[], "first"));
}

#[test]
fn rule_first_moves_the_lead_with_the_first_ready_child() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    s.set_setting(STACK_RULE_KEY, "first").unwrap();
    assert_eq!(s.stack_rule(), "first");
    let lead = card(&mut s, "lead", "Planning");
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
    assert_eq!(s.schema_version().unwrap(), 10);
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
    assert_eq!(s.schema_version().unwrap(), 10);
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
