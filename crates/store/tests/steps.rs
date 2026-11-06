// Step progress on each card (card #106): when a card last moved into a column, and the setting
// that turns Claude Code's to-do tools on for card sessions.

use kanban_store::{mask_secrets, mask_tokens, Patch, Store, TODO_TOOLS_KEY};
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

#[test]
fn entered_at_is_the_latest_move_into_that_column() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let c = s.create(None, patch(json!({ "title": "steps" })), "test").unwrap();
    assert_eq!(s.entered_at(&c.id, "In progress").unwrap(), None, "no move yet");

    let c = s.update(&c.id, c.version, &patch(json!({ "column": "In progress" })), "test").unwrap();
    let first = s.entered_at(&c.id, "In progress").unwrap().expect("the move is recorded");
    // Other fields changing later don't count as a move.
    let c = s.update(&c.id, c.version, &patch(json!({ "nextStep": "build it" })), "test").unwrap();
    assert_eq!(s.entered_at(&c.id, "In progress").unwrap().as_deref(), Some(first.as_str()));

    std::thread::sleep(std::time::Duration::from_millis(5));
    let c = s.update(&c.id, c.version, &patch(json!({ "column": "Human test" })), "test").unwrap();
    let c = s.update(&c.id, c.version, &patch(json!({ "column": "In progress" })), "test").unwrap();
    let second = s.entered_at(&c.id, "In progress").unwrap().unwrap();
    assert!(second > first, "the newest move wins: {second} after {first}");
    assert!(s.entered_at(&c.id, "Human test").unwrap().is_some());
    assert_eq!(s.entered_at(&c.id, "Planning").unwrap(), None);

    // Another card's moves are its own.
    let d = s.create(None, patch(json!({ "title": "other" })), "test").unwrap();
    assert_eq!(s.entered_at(&d.id, "In progress").unwrap(), None);
}

#[test]
fn the_todo_tools_setting_defaults_to_on_and_takes_only_on_or_off() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    // On in both channels (Hugh, 6 Nov 2026: "106 - on please"); he can turn it off.
    let want = "on";
    assert_eq!(kanban_store::page_default(TODO_TOOLS_KEY).unwrap(), want);
    assert_eq!(s.page_setting(TODO_TOOLS_KEY).unwrap(), want);
    assert_eq!(s.todo_tools_on(), want == "on");

    assert_eq!(s.set_page_setting(TODO_TOOLS_KEY, " on ").unwrap(), "on");
    assert!(s.todo_tools_on());
    assert_eq!(s.set_page_setting(TODO_TOOLS_KEY, "off").unwrap(), "off");
    assert!(!s.todo_tools_on());
    for bad in ["yes", "", "true", "ON!"] {
        assert!(s.set_page_setting(TODO_TOOLS_KEY, bad).is_err(), "{bad:?}");
    }
    assert_eq!(s.page_setting(TODO_TOOLS_KEY).unwrap(), "off", "a refused value changes nothing");
    // One of Hugh's on/off rules.
    assert!(kanban_store::RULE_KEYS.contains(&TODO_TOOLS_KEY));
    assert!(kanban_store::PAGE_SETTINGS.contains(&TODO_TOOLS_KEY));
}

#[test]
fn tokens_are_masked_and_ordinary_words_are_not() {
    assert_eq!(mask_tokens("Add the password reset form"), "Add the password reset form");
    assert_eq!(mask_tokens("Run pytest --select=fast"), "Run pytest --select=fast", "sele-ct= is not ct=");
    assert_eq!(mask_tokens("gcloud --project=acme-prod"), "gcloud --project=acme-prod");
    assert_eq!(mask_tokens("Use sk_live_abc123 here"), "Use [hidden] here");
    assert_eq!(mask_tokens("open ?ct=abc.def&x=1"), "open ?[hidden]&x=1");
    assert_eq!(mask_tokens("token eyJhbGciOiJIUzI1NiJ9.e30.sig"), "token [hidden]");
    // mask_secrets still splits the bare word, for text the board stores.
    assert_eq!(mask_secrets("a password"), "a pass word");
}
