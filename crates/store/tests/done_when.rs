// Card #105: a card's "Done when..." line (doneWhen) and its acceptance checks (doneWhenChecks).
// Plain card fields, so no migration: these tests pin the cleaning, the who-and-when stamp on each
// check, the secret rules, and that code paths which don't name the fields keep them.

use kanban_store::{Error, Patch, Store, DONE_WHEN_CHECKS_KEY as CHECKS, DONE_WHEN_KEY as LINE};
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

fn new_card(s: &mut Store) -> kanban_store::Card {
    s.create(None, patch(json!({ "title": "t", "column": "Planning" })), "test").unwrap()
}

fn set(s: &mut Store, id: &str, v: Value, by: &str) -> kanban_store::Result<kanban_store::Card> {
    let c = s.get(id).unwrap();
    s.update(id, c.version, &patch(v), by)
}

fn checks(c: &kanban_store::Card) -> Vec<Value> {
    c.fields.get(CHECKS).and_then(Value::as_array).cloned().unwrap_or_default()
}

#[test]
fn done_when_is_one_clean_line() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let c = new_card(&mut s);
    let c2 = set(&mut s, &c.id, json!({ LINE: "  Done when:  the line\n shows   on the card " }), "test").unwrap();
    assert_eq!(c2.fields[LINE], json!("the line shows on the card"));
    assert_eq!(set(&mut s, &c.id, json!({ LINE: "done when... it works" }), "test").unwrap().fields[LINE], json!("it works"));
    // Only "Done when" or a leading "..." is dropped: a line's own punctuation stays.
    for (given, kept) in [(".env is ignored by git", ".env is ignored by git"), ("-1px gap is gone", "-1px gap is gone"), ("\u{2026} the board opens", "the board opens"), ("Done whenever", "Done whenever")] {
        assert_eq!(set(&mut s, &c.id, json!({ LINE: given }), "test").unwrap().fields[LINE], json!(kept), "{given}");
    }
    // At most 200 characters: 200 is kept, 201 is refused and the card is left as it was.
    let ok = "a".repeat(200);
    let c4 = set(&mut s, &c.id, json!({ LINE: ok }), "test").unwrap();
    assert_eq!(c4.fields[LINE].as_str().unwrap().chars().count(), 200);
    let err = set(&mut s, &c.id, json!({ LINE: "b".repeat(201) }), "test").unwrap_err();
    assert!(matches!(err, Error::Invalid(ref m) if m.contains("200")), "{err}");
    assert_eq!(s.get(&c.id).unwrap().version, c4.version);
    // Not a string: refused. "" or null removes it.
    assert!(set(&mut s, &c.id, json!({ LINE: 5 }), "test").is_err());
    let c5 = set(&mut s, &c.id, json!({ LINE: "" }), "test").unwrap();
    assert!(!c5.fields.contains_key(LINE));
    set(&mut s, &c.id, json!({ LINE: "back" }), "test").unwrap();
    let c6 = set(&mut s, &c.id, json!({ LINE: null }), "test").unwrap();
    assert!(!c6.fields.contains_key(LINE));
}

#[test]
fn done_when_checks_are_cleaned_and_checked() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let c = new_card(&mut s);
    // One string, a check per line: bullets and blank lines dropped, repeats kept once.
    let c2 = set(&mut s, &c.id, json!({ CHECKS: "- the line shows\n\n2. a tick is kept\n* The line   shows" }), "test").unwrap();
    let l = checks(&c2);
    assert_eq!(l.len(), 2);
    assert_eq!(l[0], json!({ "text": "the line shows", "state": "open" }));
    assert_eq!(l[1], json!({ "text": "a tick is kept", "state": "open" }));
    // A list of strings and objects; the state's case doesn't matter; a note is kept.
    let c3 = set(
        &mut s,
        &c.id,
        json!({ CHECKS: ["one", { "text": "two", "state": " FAILED ", "note": "it wrapped" }, { "text": "three", "state": null }] }),
        "test",
    )
    .unwrap();
    let l = checks(&c3);
    assert_eq!(l[0]["state"], json!("open"));
    assert_eq!(l[1]["state"], json!("failed"));
    assert_eq!(l[1]["note"], json!("it wrapped"));
    assert_eq!(l[2]["state"], json!("open"));
    // The same list again makes no new version.
    let again = set(&mut s, &c.id, json!({ CHECKS: c3.fields[CHECKS].clone() }), "test").unwrap();
    assert_eq!(again.version, c3.version);
    // Refusals leave the card as it was.
    let bad = [
        json!({ CHECKS: (0..11).map(|i| format!("check {i}")).collect::<Vec<_>>() }),
        json!({ CHECKS: ["x".repeat(161)] }),
        json!({ CHECKS: [{ "text": "a", "state": "done" }] }),
        json!({ CHECKS: [{ "text": "a", "colour": "red" }] }),
        json!({ CHECKS: [{ "text": "  " }] }),
        json!({ CHECKS: [{ "text": "a", "note": "n".repeat(301) }] }),
        json!({ CHECKS: [3] }),
        json!({ CHECKS: { "text": "a" } }),
    ];
    for b in bad {
        let err = set(&mut s, &c.id, b.clone(), "test");
        assert!(matches!(err, Err(Error::Invalid(_))), "{b}: {err:?}");
        assert_eq!(s.get(&c.id).unwrap().version, c3.version, "{b}");
    }
    // Ten is the most; an empty list or null removes the field.
    let ten = set(&mut s, &c.id, json!({ CHECKS: (0..10).map(|i| format!("check {i}")).collect::<Vec<_>>() }), "test").unwrap();
    assert_eq!(checks(&ten).len(), 10);
    let gone = set(&mut s, &c.id, json!({ CHECKS: [] }), "test").unwrap();
    assert!(!gone.fields.contains_key(CHECKS));
    // Made with the card: cleaned the same way.
    let made = s
        .create(None, patch(json!({ "title": "m", LINE: "Done when it is made", CHECKS: ["a", { "text": "b", "state": "met" }] })), "a session")
        .unwrap();
    assert_eq!(made.fields[LINE], json!("it is made"));
    let l = checks(&made);
    assert_eq!(l[0], json!({ "text": "a", "state": "open" }));
    assert_eq!(l[1]["by"], json!("a session"));
    assert!(l[1]["at"].is_string());
    assert!(s.create(None, patch(json!({ "title": "m", CHECKS: [{ "text": "a", "state": "maybe" }] })), "t").is_err());
}

#[test]
fn a_filtered_word_in_a_check_is_refused_by_name() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let c = new_card(&mut s);
    set(&mut s, &c.id, json!({ CHECKS: ["one", "two"] }), "test").unwrap();
    let before = s.get(&c.id).unwrap().version;
    // The board's filter (secret.rs) refuses the bare word "password": the refusal names the
    // exact check, so the session knows which one to reword (critic finding 5).
    let err = set(&mut s, &c.id, json!({ CHECKS: ["one", "No password shows in the log"] }), "test").unwrap_err();
    assert!(matches!(err, Error::Secret(ref p) if p == "doneWhenChecks[1]"), "{err:?}");
    assert!(err.to_string().contains("doneWhenChecks[1]"), "{err}");
    let err = set(&mut s, &c.id, json!({ CHECKS: [{ "text": "one", "note": "password" }] }), "test").unwrap_err();
    assert!(matches!(err, Error::Secret(ref p) if p == "doneWhenChecks[0].note"), "{err:?}");
    // Sign-in links and Bearer headers are refused too, in the line, a check or a note.
    for v in [
        json!({ LINE: "open http://127.0.0.1:3006/dev-sign-in.html?as=admin" }),
        json!({ CHECKS: ["see https://x.test/api/dev/sign-in-token?as=user"] }),
        json!({ CHECKS: [{ "text": "a", "note": "Authorization: Bearer abcdefghijklmnopqrstuvwxyz" }] }),
    ] {
        let err = set(&mut s, &c.id, v.clone(), "test").unwrap_err();
        assert!(matches!(err, Error::Secret(_)), "{v}: {err:?}");
    }
    // Words about them are fine: only a link or a header is refused.
    set(&mut s, &c.id, json!({ CHECKS: ["the dev-sign-in page isn't on the card", "no Bearer header is logged"] }), "test").unwrap();
    assert_eq!(s.get(&c.id).unwrap().version, before + 1);
}

#[test]
fn a_check_records_who_changed_its_state_and_when() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let c = new_card(&mut s);
    let c1 = set(&mut s, &c.id, json!({ CHECKS: ["a", "b", "c"] }), "a session").unwrap();
    assert!(checks(&c1).iter().all(|x| x.get("by").is_none() && x.get("at").is_none()));
    // Hugh ticks "b": only "b" gets who and when; a writer's own by and at are ignored.
    let c2 = set(
        &mut s,
        &c.id,
        json!({ CHECKS: ["a", { "text": "b", "state": "met", "by": "someone else", "at": "2020-01-01T00:00:00.000Z" }, "c"] }),
        "Hugh (board)",
    )
    .unwrap();
    let l = checks(&c2);
    assert_eq!(l[1]["by"], json!("Hugh (board)"));
    assert_eq!(l[1]["at"], c2.fields["updatedAt"]);
    assert!(l[0].get("by").is_none() && l[2].get("by").is_none());
    // The tester fails "c" and leaves "b" alone: "b" keeps Hugh's mark, in a new order too.
    let c3 = set(
        &mut s,
        &c.id,
        json!({ CHECKS: [{ "text": "c", "state": "failed", "note": "it wrapped" }, "a", { "text": "B", "state": "met" }] }),
        "the app (card-tester subagent)",
    )
    .unwrap();
    let l = checks(&c3);
    assert_eq!(l[0]["by"], json!("the app (card-tester subagent)"));
    assert_eq!(l[2]["by"], json!("Hugh (board)"));
    assert_eq!(l[2]["at"], checks(&c2)[1]["at"]);
    // Back to open clears who and when.
    let c4 = set(&mut s, &c.id, json!({ CHECKS: ["c", "a", "b"] }), "Hugh (board)").unwrap();
    assert!(checks(&c4).iter().all(|x| x.get("by").is_none()));
    // The revision records the filled-in name, not a placeholder.
    let conn = rusqlite::Connection::open(&path).unwrap();
    let rev: String = conn
        .query_row("SELECT changed FROM card_revisions WHERE card_id = ?1 AND version = ?2", rusqlite::params![c.id, c2.version], |r| r.get(0))
        .unwrap();
    assert!(rev.contains("Hugh (board)") && !rev.contains("pending"), "{rev}");
}

#[test]
fn an_older_build_keeps_the_done_when_fields() {
    // An older build doesn't know these fields. Its writes go through code that only touches the
    // keys a patch names, as this build's do for every other field. This test puts the fields in
    // with raw SQL (json_set) and runs this build's other writes over them; it does not run an
    // older binary (ACCEPTANCE.md says so).
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let c = new_card(&mut s);
    let schema = s.schema_version().unwrap();
    drop(s);
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "UPDATE cards SET fields = json_set(fields, '$.doneWhen', 'it shows', '$.doneWhenChecks', json('[{\"text\":\"a\",\"state\":\"met\",\"by\":\"Hugh (board)\",\"at\":\"2026-11-06T00:00:00.000Z\"}]')) WHERE id = ?1",
            [&c.id],
        )
        .unwrap();
    }
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), schema, "the fields need no migration (#105 adds none)");
    let want_line = json!("it shows");
    let want_checks = json!([{ "text": "a", "state": "met", "by": "Hugh (board)", "at": "2026-11-06T00:00:00.000Z" }]);
    let keep = |s: &Store| {
        let c = s.get(&c.id).unwrap();
        assert_eq!(c.fields[LINE], want_line);
        assert_eq!(c.fields[CHECKS], want_checks);
    };
    set(&mut s, &c.id, json!({ "summary": "s", "nextStep": "n", "labels": ["DSH"] }), "test").unwrap();
    keep(&s);
    let v = s.get(&c.id).unwrap().version;
    s.move_by_hugh(&c.id, v, "In progress", Patch::new()).unwrap();
    keep(&s);
    let v = s.get(&c.id).unwrap().version;
    s.archive(&c.id, v, true, "test").unwrap();
    keep(&s);
    let v = s.get(&c.id).unwrap().version;
    s.archive(&c.id, v, false, "test").unwrap();
    keep(&s);
    s.add_log(&c.id, "a line", "test").unwrap();
    keep(&s);
}
