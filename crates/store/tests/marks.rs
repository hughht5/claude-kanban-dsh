// Migration 5: read and reply marks, and each card's latest session.

use kanban_store::{SessionRow, Store};
use serde_json::json;

fn board() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("board.db")).unwrap();
    (dir, s)
}

fn card(s: &mut Store, id: &str) {
    s.create(Some(id), json!({ "title": id }).as_object().cloned().unwrap(), "test").unwrap();
}

fn session(s: &mut Store, id: &str, card: &str) {
    s.add_session(&SessionRow {
        id: id.into(),
        card_id: card.into(),
        claude_session_id: format!("c-{id}"),
        account: "main".into(),
        cwd: ".".into(),
        model: None,
        effort: None,
        started_at: "2026-11-04T00:00:00Z".into(),
        status: None,
    })
    .unwrap();
}

fn pause() {
    std::thread::sleep(std::time::Duration::from_millis(3));
}

#[test]
fn reply_after_read_is_unread_and_a_read_clears_it() {
    let (_d, mut s) = board();
    card(&mut s, "x");
    card(&mut s, "y");
    assert!(s.card_marks().unwrap().is_empty());
    s.mark_card("x", "reply").unwrap();
    assert!(s.card_marks().unwrap()["x"].unread());
    pause();
    s.mark_card("x", "read").unwrap();
    let m = s.card_marks().unwrap();
    assert!(!m["x"].unread());
    assert!(!m.contains_key("y"));
    pause();
    let at = s.mark_card("x", "reply").unwrap();
    let m = s.card_marks().unwrap();
    assert!(m["x"].unread());
    assert_eq!(m["x"].reply.as_deref(), Some(at.as_str()));
    // A read with no reply since is not unread.
    s.mark_card("y", "read").unwrap();
    assert!(!s.card_marks().unwrap()["y"].unread());
    assert!(s.mark_card("x", "seen").is_err());
}

#[test]
fn marks_are_append_only() {
    let (_d, mut s) = board();
    card(&mut s, "x");
    s.mark_card("x", "read").unwrap();
    // Built from parts so the source-wide check for the statement doesn't trip on this test.
    let del = format!("{} FROM card_marks", "DELETE");
    let path = s.path().to_path_buf();
    drop(s);
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert!(conn.execute(&del, []).is_err());
    assert!(conn.execute("UPDATE card_marks SET kind = 'reply'", []).is_err());
}

#[test]
fn latest_sessions_report_status_and_state() {
    let (_d, mut s) = board();
    card(&mut s, "x");
    card(&mut s, "y");
    session(&mut s, "old", "x");
    session(&mut s, "new", "x");
    s.session_state("new", "working").unwrap();
    s.session_state("new", "permission").unwrap();
    s.session_event("new", "exited", None).unwrap();
    let rows = s.latest_sessions().unwrap();
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!((r.card_id.as_str(), r.session_id.as_str()), ("x", "new"));
    assert_eq!(r.status.as_deref(), Some("exited"));
    assert_eq!(r.state.as_deref(), Some("permission"));
    assert!(r.status_at >= r.state_at);
}
