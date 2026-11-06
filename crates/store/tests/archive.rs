// Card #83: the board has no Done column. A card that reaches Done is archived at once, and
// Restore brings it back to the column it came from. Migration 10 archives the cards already in Done.

use kanban_store::{Patch, Store};
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

fn card(s: &mut Store, title: &str, col: &str) -> i64 {
    s.create(None, patch(json!({ "title": title, "column": col })), "test").unwrap().number
}

fn logs(s: &Store, n: i64) -> Vec<String> {
    let c = s.get_by_number(n).unwrap();
    s.log(&c.id).unwrap().into_iter().map(|e| e.text).collect()
}

#[test]
fn hughs_move_into_done_archives_the_card() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let n = card(&mut s, "finished", "Deploy complete");
    let c = s.get_by_number(n).unwrap();
    let out = s.move_by_hugh(&c.id, c.version, "Done", Patch::new()).unwrap();
    assert!(out.card.archived);
    assert_eq!(out.card.column, "Done");
    assert_eq!(out.card.fields["archivedFrom"], json!("Deploy complete"));
    assert_eq!(out.card.fields["archivedBy"], json!("Hugh (board)"));
    assert!(out.card.fields["archivedAt"].is_string());
    // It's out of the live list, and the archive is in its log and revisions.
    assert!(s.list(false).unwrap().iter().all(|k| k.number != n));
    assert!(logs(&s, n).iter().any(|t| t == "Archived: it reached Done"));
    assert_eq!(s.revision_count(&c.id).unwrap(), 2);
}

#[test]
fn a_sessions_move_into_done_archives_it_too_and_other_moves_dont() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let n = card(&mut s, "no diff", "In progress");
    let c = s.get_by_number(n).unwrap();
    let c = s.update(&c.id, c.version, &patch(json!({ "column": "Human test" })), "card session").unwrap();
    assert!(!c.archived);
    let c = s.update(&c.id, c.version, &patch(json!({ "column": "Done" })), "card session").unwrap();
    assert!(c.archived);
    assert_eq!(c.fields["archivedFrom"], json!("Human test"));
    // A card made straight into Done is archived from the start.
    let m = card(&mut s, "made done", "Done");
    assert!(s.get_by_number(m).unwrap().archived);
}

#[test]
fn restore_puts_a_done_card_back_where_it_came_from() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let n = card(&mut s, "oops", "Human test");
    let c = s.get_by_number(n).unwrap();
    let c = s.move_by_hugh(&c.id, c.version, "Done", Patch::new()).unwrap().card;
    let r = s.archive(&c.id, c.version, false, "Hugh (board)").unwrap();
    assert!(!r.archived);
    assert_eq!(r.column, "Human test");
    assert!(r.fields.get("archivedFrom").is_none());
    assert!(logs(&s, n).iter().any(|t| t == "Hugh restored it, back to Human test"));
    // Unknown or Ship it origins come back to Deploy complete; a plain archive keeps its column.
    let m = card(&mut s, "was shipping", "Ship it");
    let c = s.get_by_number(m).unwrap();
    let c = s.update(&c.id, c.version, &patch(json!({ "column": "Done" })), "t").unwrap();
    assert_eq!(s.archive(&c.id, c.version, false, "t").unwrap().column, "Deploy complete");
    let k = card(&mut s, "idea", "Backlog");
    let c = s.get_by_number(k).unwrap();
    let c = s.archive(&c.id, c.version, true, "Hugh (board)").unwrap();
    assert_eq!(c.column, "Backlog");
    assert_eq!(s.archive(&c.id, c.version, false, "Hugh (board)").unwrap().column, "Backlog");
}

#[test]
fn a_stack_lead_in_done_takes_its_children_into_the_archive_and_back() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let lead = card(&mut s, "lead", "Deploy complete");
    let a = card(&mut s, "a", "Deploy complete");
    let b = card(&mut s, "b", "Human test");
    s.stack_add(lead, a, "t").unwrap();
    let l = s.stack_add(lead, b, "t").unwrap();
    let l = s.move_by_hugh(&l.id, l.version, "Done", Patch::new()).unwrap().card;
    assert!(s.stack_after_move(&l.id, "t").unwrap().is_empty());
    for k in [a, b] {
        let c = s.get_by_number(k).unwrap();
        assert!(c.archived);
        assert_eq!(c.fields["archivedWith"], json!(lead));
    }
    let l = s.get_by_number(lead).unwrap();
    let r = s.archive(&l.id, l.version, false, "Hugh (board)").unwrap();
    assert_eq!(r.column, "Deploy complete");
    assert_eq!(s.get_by_number(a).unwrap().column, "Deploy complete");
    assert_eq!(s.get_by_number(b).unwrap().column, "Human test");
    assert!(!s.get_by_number(a).unwrap().archived && !s.get_by_number(b).unwrap().archived);
}

#[test]
fn migration_10_archives_cards_already_in_done_and_nothing_else() {
    let (dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let done = s.create(Some("d"), patch(json!({ "title": "done", "column": "Backlog" })), "t").unwrap();
    let live = s.create(Some("l"), patch(json!({ "title": "live", "column": "Deploy complete" })), "t").unwrap();
    let old = s.create(Some("o"), patch(json!({ "title": "old", "column": "Backlog" })), "t").unwrap();
    let old = s.archive(&old.id, old.version, true, "Hugh (board)").unwrap();
    drop(s);
    // Put "d" in Done the way an older build did, without archiving it, and go back to schema 9.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("UPDATE cards SET col = 'Done', fields = json_set(fields, '$.updatedAt', '2026-10-01T09:00:00.000Z') WHERE id = 'd'", []).unwrap();
        conn.pragma_update(None, "user_version", 9).unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), 10);
    let d = s.get("d").unwrap();
    assert!(d.archived);
    assert_eq!(d.column, "Done");
    assert_eq!(d.version, done.version + 1);
    assert_eq!(d.fields["archived"], json!(true));
    assert_eq!(d.fields["archivedAt"], json!("2026-10-01T09:00:00.000Z"));
    assert_eq!(s.revision_count("d").unwrap(), 2);
    assert!(s.log("d").unwrap().iter().any(|e| e.text == "Archived: Done cards are archived now"));
    // Cards elsewhere, and cards archived before, are untouched.
    let l = s.get("l").unwrap();
    assert_eq!((l.archived, l.version), (false, live.version));
    let o = s.get("o").unwrap();
    assert_eq!((o.version, o.fields.get("archivedBy")), (old.version, Some(&json!("Hugh (board)"))));
    drop(s);
    // It took a backup first, and running it again changes nothing.
    assert!(std::fs::read_dir(dir.path().join("backups")).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with("pre-migration.db")));
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 9).unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.get("d").unwrap().version, d.version);
}

#[test]
fn a_card_is_never_left_in_done_unarchived() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let n = card(&mut s, "made done", "Done");
    assert!(logs(&s, n).iter().any(|t| t == "Archived: it was made in Done"));
    let c = s.get_by_number(n).unwrap();
    // Un-archiving it by a plain update doesn't stick while it stays in Done.
    let c = s.update(&c.id, c.version, &patch(json!({ "archived": false })), "kb").unwrap();
    assert!(c.archived);
    // A Done card an older build left unarchived is archived by its next write of any kind.
    drop(s);
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("UPDATE cards SET archived = 0, fields = json_set(fields, '$.archived', json('false')) WHERE number = ?1", [n]).unwrap();
    }
    let mut s = Store::open(&path).unwrap();
    let c = s.get_by_number(n).unwrap();
    assert!(!c.archived);
    let c = s.update(&c.id, c.version, &patch(json!({ "summary": "touched" })), "t").unwrap();
    assert!(c.archived);
}

#[test]
fn hugh_cant_archive_a_card_mid_ship() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let n = card(&mut s, "shipping", "Ship it");
    let c = s.get_by_number(n).unwrap();
    let c = s.update(&c.id, c.version, &patch(json!({ "ship": { "state": "shipping" } })), "t").unwrap();
    match s.move_by_hugh(&c.id, c.version, "Done", Patch::new()) {
        Err(kanban_store::Error::Invalid(m)) => assert!(m.contains("is shipping")),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(!s.get_by_number(n).unwrap().archived);
}

#[test]
fn migration_10_keeps_a_done_stack_together() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let lead = card(&mut s, "lead", "Deploy complete");
    let a = card(&mut s, "a", "Deploy complete");
    s.stack_add(lead, a, "t").unwrap();
    drop(s);
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("UPDATE cards SET col = 'Done'", []).unwrap();
        conn.pragma_update(None, "user_version", 9).unwrap();
    }
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.get_by_number(a).unwrap().fields["archivedWith"], json!(lead));
    let l = s.get_by_number(lead).unwrap();
    assert!(l.archived && l.fields.get("archivedWith").is_none());
    s.archive(&l.id, l.version, false, "Hugh (board)").unwrap();
    let k = s.get_by_number(a).unwrap();
    assert_eq!((k.archived, k.column.as_str()), (false, "Deploy complete"));
}
