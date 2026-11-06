// The ship queue (card #79): one card ships at a time per repo.

use kanban_store::{Patch, Store, SHIP_QUEUED};
use serde_json::{json, Value};

fn patch(v: Value) -> Patch {
    v.as_object().cloned().unwrap()
}

fn board() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("board.db")).unwrap();
    (dir, s)
}

/// A card in Human test for `repo`; returns its number.
fn card(s: &mut Store, title: &str, repo: &str) -> i64 {
    s.create(None, patch(json!({ "title": title, "column": "Human test", "repo": repo })), "test").unwrap().number
}

/// Hugh drops card `n` into Ship it. Its requestedAt is the move's time, so a short sleep keeps
/// the order of the moves.
fn ship(s: &mut Store, n: i64) {
    let c = s.get_by_number(n).unwrap();
    s.move_by_hugh(&c.id, c.version, "Ship it", Patch::new()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
}

/// What the app does after Hugh's move: wait behind a blocker, or hand it to its session.
fn dispatch(s: &mut Store, n: i64) -> Option<i64> {
    let c = s.get_by_number(n).unwrap();
    match s.ship_blocker(&c).unwrap() {
        Some(b) => {
            s.ship_wait(&c.id, &b, "Human test").unwrap();
            Some(b.number)
        }
        None => {
            set_ship(s, n, json!({ "handedAt": "2026-11-05T10:00:00.000Z" }));
            None
        }
    }
}

/// Merges `v` into card `n`'s ship.
fn set_ship(s: &mut Store, n: i64, v: Value) {
    let c = s.get_by_number(n).unwrap();
    let mut ship = c.fields["ship"].as_object().cloned().unwrap();
    for (k, x) in v.as_object().unwrap() {
        ship.insert(k.clone(), x.clone());
    }
    s.update(&c.id, c.version, &patch(json!({ "ship": ship })), "test").unwrap();
}

fn state(s: &Store, n: i64) -> String {
    s.get_by_number(n).unwrap().fields["ship"]["state"].as_str().unwrap_or("").to_string()
}

fn report(s: &mut Store, n: i64, st: &str) {
    let c = s.get_by_number(n).unwrap();
    let mut ship = c.fields["ship"].as_object().cloned().unwrap();
    ship.insert("state".into(), json!(st));
    s.update(&c.id, c.version, &patch(json!({ "ship": ship })), "card session").unwrap();
}

fn due(s: &Store) -> Vec<i64> {
    s.ship_due().unwrap().iter().map(|c| c.number).collect()
}

#[test]
fn second_ship_in_a_repo_waits_and_goes_when_the_first_is_done() {
    let (_d, mut s) = board();
    let a = card(&mut s, "A", "acme/kanban");
    let b = card(&mut s, "B", "acme/kanban");
    let c = card(&mut s, "C", "Acme/Kanban");
    ship(&mut s, a);
    ship(&mut s, b);
    ship(&mut s, c);
    assert_eq!(dispatch(&mut s, a), None, "the first goes at once");
    assert_eq!(dispatch(&mut s, b), Some(a));
    assert_eq!(dispatch(&mut s, c), Some(a), "repo names match whatever their case");
    assert_eq!(state(&s, b), SHIP_QUEUED);
    let card_b = s.get_by_number(b).unwrap();
    assert!(card_b.fields["ship"]["detail"].as_str().unwrap().contains(&format!("#{a}")));
    assert!(s.log(&card_b.id).unwrap().iter().any(|l| l.text.starts_with("Waiting to ship")));
    assert!(due(&s).is_empty(), "nothing is due while A is still requested");

    report(&mut s, a, "shipping");
    assert!(due(&s).is_empty(), "nor while it ships");
    report(&mut s, a, "shipped");
    assert_eq!(due(&s), vec![b], "the oldest waiting card goes next, and only one");

    let (rel, from) = s.ship_release(&s.get_by_number(b).unwrap().id).unwrap();
    assert_eq!(from, "Human test");
    assert_eq!(rel.fields["ship"]["state"], "requested");
    assert_eq!(s.ship_blocker(&rel).unwrap().map(|c| c.number), None, "a released card isn't blocked by the one behind it");
    assert!(due(&s).is_empty(), "C waits for B");
    report(&mut s, b, "needs-hugh");
    assert_eq!(due(&s), vec![c], "needs-hugh frees the repo too");
}

#[test]
fn different_repos_ship_side_by_side() {
    let (_d, mut s) = board();
    let a = card(&mut s, "A", "acme/kanban");
    let b = card(&mut s, "B", "acme/web");
    ship(&mut s, a);
    ship(&mut s, b);
    assert_eq!(dispatch(&mut s, a), None);
    assert_eq!(dispatch(&mut s, b), None);
}

#[test]
fn the_earlier_request_wins_whichever_is_dispatched_first() {
    let (_d, mut s) = board();
    let a = card(&mut s, "A", "r");
    let b = card(&mut s, "B", "r");
    ship(&mut s, a);
    ship(&mut s, b);
    // B's dispatch thread runs first: A, asked for earlier, still holds the lane.
    assert_eq!(dispatch(&mut s, b), Some(a));
    assert_eq!(dispatch(&mut s, a), None);
}

#[test]
fn leaving_ship_it_frees_the_repo_and_cancels_a_waiting_request() {
    let (_d, mut s) = board();
    let a = card(&mut s, "A", "r");
    let b = card(&mut s, "B", "r");
    let c = card(&mut s, "C", "r");
    ship(&mut s, a);
    ship(&mut s, b);
    ship(&mut s, c);
    dispatch(&mut s, a);
    dispatch(&mut s, b);
    dispatch(&mut s, c);
    // Hugh takes waiting B back out: its request is cancelled.
    let cb = s.get_by_number(b).unwrap();
    s.move_by_hugh(&cb.id, cb.version, "Human test", Patch::new()).unwrap();
    assert_eq!(state(&s, b), "cancelled");
    // A's ship failed and the app sent it back to In progress.
    report(&mut s, a, "failed");
    let ca = s.get_by_number(a).unwrap();
    s.update(&ca.id, ca.version, &patch(json!({ "column": "In progress" })), "the app").unwrap();
    assert_eq!(due(&s), vec![c]);
}

#[test]
fn an_archived_shipping_card_frees_the_repo() {
    let (_d, mut s) = board();
    let a = card(&mut s, "A", "r");
    let b = card(&mut s, "B", "r");
    ship(&mut s, a);
    ship(&mut s, b);
    dispatch(&mut s, b);
    report(&mut s, a, "shipping");
    let ca = s.get_by_number(a).unwrap();
    s.archive(&ca.id, ca.version, true, "Hugh (board)").unwrap();
    assert_eq!(due(&s), vec![b]);
}

#[test]
fn cards_with_no_repo_dont_queue() {
    let (_d, mut s) = board();
    let a = card(&mut s, "A", "");
    let b = card(&mut s, "B", " ");
    ship(&mut s, a);
    ship(&mut s, b);
    assert_eq!(dispatch(&mut s, a), None);
    assert_eq!(dispatch(&mut s, b), None);
}

#[test]
fn a_new_request_doesnt_jump_a_waiting_one() {
    let (_d, mut s) = board();
    let a = card(&mut s, "A", "r");
    let b = card(&mut s, "B", "r");
    let c = card(&mut s, "C", "r");
    ship(&mut s, a);
    ship(&mut s, b);
    dispatch(&mut s, a);
    dispatch(&mut s, b);
    report(&mut s, a, "shipped");
    // C arrives before the pump has released B: B asked first, so C waits for B.
    ship(&mut s, c);
    assert_eq!(dispatch(&mut s, c), Some(b));
    assert_eq!(due(&s), vec![b]);
}

#[test]
fn a_request_no_session_picked_up_stops_holding_the_repo() {
    let (_d, mut s) = board();
    let a = card(&mut s, "A", "r");
    let b = card(&mut s, "B", "r");
    ship(&mut s, a);
    // A's session never started: no handedAt, and its request is two minutes old.
    set_ship(&mut s, a, json!({ "requestedAt": "2020-01-01T00:00:00+00:00" }));
    ship(&mut s, b);
    assert_eq!(dispatch(&mut s, b), None, "a stale unhanded request doesn't block");
    // But one an older build handed (its detail, no handedAt) still does, however old.
    let c = card(&mut s, "C", "r");
    set_ship(&mut s, a, json!({ "detail": "Handed to the card's session in the app" }));
    ship(&mut s, c);
    assert_eq!(dispatch(&mut s, c), Some(a), "the earliest card holding the repo is named");
}

#[test]
fn request_times_compare_as_times_not_strings() {
    let (_d, mut s) = board();
    let a = card(&mut s, "A", "r");
    let b = card(&mut s, "B", "r");
    ship(&mut s, a);
    ship(&mut s, b);
    // The same instants written differently: A at 11:00 in +01:00 is 10:00Z, before B's 10:30Z.
    set_ship(&mut s, a, json!({ "requestedAt": "2099-01-01T11:00:00+01:00" }));
    set_ship(&mut s, b, json!({ "requestedAt": "2099-01-01T10:30:00Z" }));
    assert_eq!(dispatch(&mut s, b), Some(a));
}

#[test]
fn a_waiting_card_moved_out_any_other_way_is_cancelled() {
    let (_d, mut s) = board();
    let a = card(&mut s, "A", "r");
    let b = card(&mut s, "B", "r");
    ship(&mut s, a);
    ship(&mut s, b);
    dispatch(&mut s, a);
    dispatch(&mut s, b);
    assert!(s.ship_any_queued().unwrap());
    let cb = s.get_by_number(b).unwrap();
    s.update(&cb.id, cb.version, &patch(json!({ "column": "In progress" })), "card session").unwrap();
    assert_eq!(s.ship_cancel_strays().unwrap(), 1);
    assert_eq!(state(&s, b), "cancelled");
    assert!(!s.ship_any_queued().unwrap());
    assert_eq!(s.ship_cancel_strays().unwrap(), 0);
}
