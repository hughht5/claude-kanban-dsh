//! The app's own messages to a session (card #153): recorded when the app queues them, never for
//! Hugh's words, and split out of a transcript's user message so the chat colours them apart.

use kanban_store::{split_app_parts, MsgPart, Store};
use rusqlite::Connection;
use serde_json::json;

fn board() -> (tempfile::TempDir, Store, String) {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("board.db")).unwrap();
    let c = s.create(None, json!({ "title": "T", "column": "Backlog" }).as_object().unwrap().clone(), "test").unwrap();
    (dir, s, c.id)
}

fn app(t: &str) -> MsgPart {
    MsgPart { auto: true, text: t.into() }
}
fn hugh(t: &str) -> MsgPart {
    MsgPart { auto: false, text: t.into() }
}

#[test]
fn migration_15_adds_an_append_only_table_with_a_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    drop(Store::open(&path).unwrap());
    {
        let c = Connection::open(&path).unwrap();
        c.pragma_update(None, "user_version", 14).unwrap();
        c.execute_batch("DROP TABLE app_messages").unwrap();
    }
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), kanban_store::SCHEMA_VERSION);
    assert!(std::fs::read_dir(s.backups_dir()).unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains("pre-migration")));
    s.note_app_message("x", "hello").unwrap();
    drop(s);
    let c = Connection::open(&path).unwrap();
    assert!(c.execute(&format!("{} FROM app_messages", "DELETE"), []).is_err(), "refuses deletes");
    assert!(c.execute("UPDATE app_messages SET text = 'y'", []).is_err(), "refuses updates");
}

#[test]
fn the_apps_messages_are_recorded_and_hughs_are_not() {
    let (_d, mut s, id) = board();
    s.queue(&id, "message", "Hugh moved this card to In progress.").unwrap();
    s.queue_hugh(&id, "message", "proceed").unwrap();
    s.queue(&id, "move", r#"{"column":"Blocked"}"#).unwrap();
    s.queue(&id, "message", "  \r\n").unwrap();
    // The queue joins them into one message, but each is recorded on its own.
    let waiting = s.waiting(&id, "message").unwrap().unwrap();
    assert_eq!(waiting.payload, "Hugh moved this card to In progress.\n\nproceed\n\n  \r\n");
    assert_eq!(s.app_messages(&id).unwrap(), vec!["Hugh moved this card to In progress.".to_string()]);
    s.note_app_message(&id, "A nudge").unwrap();
    s.note_app_message(&id, "Hugh moved this card to In progress.").unwrap();
    assert_eq!(s.app_messages(&id).unwrap(), vec!["Hugh moved this card to In progress.".to_string(), "A nudge".to_string()]);
    assert!(s.app_messages("other").unwrap().is_empty());
}

#[test]
fn a_message_splits_into_the_apps_parts_and_hughs() {
    let apps = vec!["It is your turn on the test app (card #128).\nUse it now.".to_string(), "From the Router: go on.".to_string()];
    // Hugh's own message: nothing to split.
    assert_eq!(split_app_parts("proceed", &apps), None);
    assert_eq!(split_app_parts("anything", &[]), None);
    // The app's alone, whatever its line endings.
    assert_eq!(split_app_parts("From the Router: go on.\r\n", &apps), Some(vec![app("From the Router: go on.")]));
    // The app's, then Hugh's, typed in together.
    assert_eq!(
        split_app_parts("It is your turn on the test app (card #128).\r\nUse it now.\r\n\r\nproceed", &apps),
        Some(vec![app("It is your turn on the test app (card #128).\nUse it now."), hugh("proceed")])
    );
    // Hugh's paragraphs, then two of the app's.
    assert_eq!(
        split_app_parts("one\n\ntwo\n\nFrom the Router: go on.\n\nIt is your turn on the test app (card #128).\nUse it now.", &apps),
        Some(vec![hugh("one\n\ntwo"), app("From the Router: go on."), app("It is your turn on the test app (card #128).\nUse it now.")])
    );
    // A piece that only starts with an app text is Hugh's.
    assert_eq!(split_app_parts("From the Router: go on. And more", &apps), None);
}
