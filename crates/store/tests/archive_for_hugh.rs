// Card #223: an agent archives a card because Hugh asked it to (kanban_archive_card, `kb archive
// <n> --asked-by-hugh "<his words>"`). The same rules as his Archive all; his words on the card.

use kanban_store::{Patch, Store, ARCHIVED_FOR_KEY, ROUTER_BY};
use serde_json::{json, Value};

fn patch(v: Value) -> Patch {
    v.as_object().cloned().unwrap()
}

fn board() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("board.db")).unwrap();
    (dir, s)
}

fn card(s: &mut Store, title: &str, col: &str) -> i64 {
    s.create(None, patch(json!({ "title": title, "column": col })), "test").unwrap().number
}

fn get(s: &Store, n: i64) -> kanban_store::Card {
    s.get_by_number(n).unwrap()
}

fn err<T: std::fmt::Debug>(r: kanban_store::Result<T>) -> String {
    r.unwrap_err().to_string()
}

const BY: &str = "a session outside the app";

#[test]
fn it_archives_into_done_with_hughs_words_on_the_card_and_its_log() {
    let (_d, mut s) = board();
    let n = card(&mut s, "old work", "Human test");
    let r = s.archive_for_hugh(n, None, BY, "  archive 12, it's\nalready live ").unwrap();
    assert!(r.done);
    assert_eq!(r.from.as_deref(), Some("Human test"));
    assert!(r.children.is_empty());
    let c = get(&s, n);
    assert!(c.archived);
    assert_eq!(c.column, "Done");
    assert_eq!(c.fields["archivedFrom"], json!("Human test"));
    assert_eq!(c.fields["archivedBy"], json!("a session outside the app, for Hugh"));
    let f = &c.fields[ARCHIVED_FOR_KEY];
    assert_eq!(f["by"], json!(BY));
    assert_eq!(f["words"], json!("archive 12, it's already live"));
    let log = s.log(&c.id).unwrap();
    let line = log.iter().find(|l| l.text.starts_with("Archived by")).expect("a log line");
    assert_eq!(line.text, "Archived by a session outside the app for Hugh (it was in Human test): he asked \"archive 12, it's already live\"");
    assert_eq!(line.by.as_deref(), Some("a session outside the app, for Hugh"));
    // Hugh's Restore brings it back where it was, and the record of the archive goes.
    let v = get(&s, n).version;
    let back = s.archive(&c.id, v, false, "Hugh (board)").unwrap();
    assert!(!back.archived);
    assert_eq!(back.column, "Human test");
    assert!(!back.fields.contains_key(ARCHIVED_FOR_KEY));
}

#[test]
fn it_needs_hughs_words_the_version_and_a_card_not_archived() {
    let (_d, mut s) = board();
    let n = card(&mut s, "x", "Backlog");
    assert!(err(s.archive_for_hugh(n, None, BY, " \n ")).contains("hugh_words"));
    let v = get(&s, n).version;
    assert!(err(s.archive_for_hugh(n, Some(v + 1), BY, "archive it")).contains("changed"));
    assert!(!get(&s, n).archived);
    s.archive_for_hugh(n, Some(v), BY, "archive it").unwrap();
    assert!(err(s.archive_for_hugh(n, None, BY, "archive it")).contains("archived already"));
    assert!(s.archive_for_hugh(9999, None, BY, "archive it").is_err());
}

#[test]
fn the_router_never_archives_this_way() {
    let (_d, mut s) = board();
    let n = card(&mut s, "x", "Deploy complete");
    assert!(err(s.archive_for_hugh(n, None, ROUTER_BY, "archive it")).contains("never archives"));
    assert!(!get(&s, n).archived);
}

#[test]
fn ship_it_and_a_stacks_child_are_refused_and_a_lead_takes_its_children() {
    let (_d, mut s) = board();
    let ship = card(&mut s, "shipping", "Ship it");
    assert!(err(s.archive_for_hugh(ship, None, BY, "archive it")).contains("Ship it"));
    assert!(!get(&s, ship).archived);

    let lead = card(&mut s, "lead", "Human test");
    let a = card(&mut s, "a", "Human test");
    let b = card(&mut s, "b", "In progress");
    s.stack_add(lead, a, "t").unwrap();
    s.stack_add(lead, b, "t").unwrap();
    assert!(err(s.archive_for_hugh(a, None, BY, "archive it")).contains(&format!("stack #{lead}; ask for the lead")));

    let r = s.archive_for_hugh(lead, None, "card #7's session", "drop the whole stack").unwrap();
    assert!(get(&s, lead).archived);
    let mut kids = r.children.clone();
    kids.sort();
    let mut want = vec![get(&s, a).id, get(&s, b).id];
    want.sort();
    assert_eq!(kids, want);
    for k in [a, b] {
        let c = get(&s, k);
        assert!(c.archived, "#{k}");
        assert_eq!(c.fields["archivedWith"], json!(lead));
    }
    assert!(s.log(&get(&s, lead).id).unwrap().iter().any(|l| l.text.contains("Archived by card #7's session for Hugh") && l.text.contains("drop the whole stack")));
}

#[test]
fn a_lead_with_a_child_in_ship_it_is_refused() {
    let (_d, mut s) = board();
    let lead = card(&mut s, "lead", "Human test");
    let kid = card(&mut s, "kid", "Human test");
    s.stack_add(lead, kid, "t").unwrap();
    let k = get(&s, kid);
    s.update(&k.id, k.version, &patch(json!({ "column": "Ship it" })), "test").unwrap();
    assert!(err(s.archive_for_hugh(lead, None, BY, "archive it")).contains("in Ship it"));
    assert!(!get(&s, lead).archived);
}

#[test]
fn a_token_in_his_words_is_refused() {
    let (_d, mut s) = board();
    let n = card(&mut s, "x", "Backlog");
    let r = s.archive_for_hugh(n, None, BY, "archive it, key sk_live_abcdef123456");
    assert!(r.is_err(), "{r:?}");
    assert!(!get(&s, n).archived);
}
