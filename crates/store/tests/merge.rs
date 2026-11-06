// The card-merger's report at ship (card #100): the `merge` field, and its clear on every move
// into Ship it, so a report from an earlier ship never shows on a new one.

use kanban_store::{Patch, Store, MERGE_KEY};
use serde_json::{json, Value};

fn patch(v: Value) -> Patch {
    v.as_object().cloned().unwrap()
}

fn board() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("board.db")).unwrap();
    (dir, s)
}

fn merged() -> Value {
    json!({ "state": "passed", "at": "2026-11-06T02:00:00.000Z", "agentId": "a1", "head": "abc1234", "origin": { "before": "def5678", "after": "def5678" } })
}

#[test]
fn every_move_into_ship_it_clears_the_last_merge() {
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T", "column": "Human test", "repo": "hughht5/kanban-app", "merge": merged() })), "test").unwrap();
    assert_eq!(c.fields.get(MERGE_KEY), Some(&merged()));
    // Hugh's move into Ship it: a new ship, so no merge yet.
    let m = s.move_by_hugh(&c.id, c.version, "Ship it", Patch::new()).unwrap();
    let c = s.get(&c.id).unwrap();
    assert!(!c.fields.contains_key(MERGE_KEY), "{m:?}");
    // The app records the merger's report; it stays through Deploy complete.
    let c = s.update(&c.id, c.version, &patch(json!({ "merge": merged() })), "the app (card-merger subagent)").unwrap();
    let c = s.update(&c.id, c.version, &patch(json!({ "column": "Deploy complete" })), "card #1's session").unwrap();
    assert_eq!(c.fields.get(MERGE_KEY), Some(&merged()));
    // Back for another ship (the app's move after a failed ship, say): cleared again.
    let c = s.update(&c.id, c.version, &patch(json!({ "column": "In progress" })), "the app").unwrap();
    assert_eq!(c.fields.get(MERGE_KEY), Some(&merged()), "only Ship it clears it");
    let c = s.update(&c.id, c.version, &patch(json!({ "column": "Ship it" })), "the app").unwrap();
    assert!(!c.fields.contains_key(MERGE_KEY));
    // A move into Ship it that sets the merge itself keeps what it sets.
    let c = s.update(&c.id, c.version, &patch(json!({ "column": "Human test" })), "the app").unwrap();
    let c = s.update(&c.id, c.version, &patch(json!({ "column": "Ship it", "merge": merged() })), "the app").unwrap();
    assert_eq!(c.fields.get(MERGE_KEY), Some(&merged()));
    // A card with no merge moves into Ship it with nothing to clear.
    let d = s.create(None, patch(json!({ "title": "U", "column": "Human test" })), "test").unwrap();
    s.move_by_hugh(&d.id, d.version, "Ship it", Patch::new()).unwrap();
    assert!(!s.get(&d.id).unwrap().fields.contains_key(MERGE_KEY));
}
