// Card #129: the reads behind "Running now": the app's own Claude session ids (never listed as
// sessions outside the app) and one card's sessions, newest first.

use kanban_store::{SessionRow, Store};
use serde_json::json;

fn board() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("board.db")).unwrap();
    (dir, s)
}

fn session(s: &mut Store, id: &str, card: &str, claude: &str) {
    s.add_session(&SessionRow {
        id: id.into(),
        card_id: card.into(),
        claude_session_id: claude.into(),
        account: "main".into(),
        cwd: ".".into(),
        model: None,
        effort: None,
        started_at: "2026-11-09T00:00:00Z".into(),
        status: None,
    })
    .unwrap();
}

#[test]
fn own_ids_are_every_card_router_and_lane_conversation() {
    let (_d, mut s) = board();
    s.create(Some("card-1"), json!({ "title": "one" }).as_object().cloned().unwrap(), "test").unwrap();
    session(&mut s, "r1", "card-1", "c-old");
    session(&mut s, "r2", "card-1", "c-new");
    s.add_router_session("rt1", "router-old", "live", "main", ".", "m", "e").unwrap();
    s.add_router_session("rt2", "router-new", "live", "main", ".", "m", "e").unwrap();
    s.add_lane_session("l1", "owner/repo", "lane-old", "live", "main", ".", "m", "e").unwrap();
    s.add_lane_session("l2", "owner/repo", "lane-new", "live", "main", ".", "m", "e").unwrap();
    let ids = s.own_claude_ids().unwrap();
    for id in ["c-old", "c-new", "router-old", "router-new", "lane-old", "lane-new"] {
        assert!(ids.contains(id), "{id} is the app's own");
    }
    assert!(!ids.contains("someone-else"));
}

#[test]
fn a_cards_sessions_come_newest_first_and_only_its_own() {
    let (_d, mut s) = board();
    s.create(Some("card-1"), json!({ "title": "one" }).as_object().cloned().unwrap(), "test").unwrap();
    s.create(Some("card-2"), json!({ "title": "two" }).as_object().cloned().unwrap(), "test").unwrap();
    session(&mut s, "a", "card-1", "c-a");
    session(&mut s, "b", "card-2", "c-b");
    session(&mut s, "c", "card-1", "c-c");
    let rows: Vec<String> = s.card_sessions("card-1").unwrap().into_iter().map(|r| r.claude_session_id).collect();
    assert_eq!(rows, vec!["c-c", "c-a"]);
    assert!(s.card_sessions("card-9").unwrap().is_empty());
}
