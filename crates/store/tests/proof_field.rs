// Card #125: `proof`, the screenshots and reports that prove a card's change. An ordinary JSON
// field, no migration, cleaned by the same rules as `changesOutputs` (shared code).

use kanban_store::{Error, Patch, Store};
use serde_json::{json, Value};

fn patch(v: Value) -> Patch {
    v.as_object().cloned().unwrap()
}

fn board() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(&dir.path().join("board.db")).unwrap();
    (dir, s)
}

fn set(s: &mut Store, id: &str, v: Value) -> kanban_store::Result<kanban_store::Card> {
    let c = s.get(id).unwrap();
    s.update(id, c.version, &patch(v), "test")
}

fn refused(s: &mut Store, id: &str, proof: Value) -> String {
    match set(s, id, json!({ "proof": proof })) {
        Err(Error::Invalid(m)) => m,
        other => panic!("expected a refusal for {proof}, got {other:?}"),
    }
}

#[test]
fn proof_takes_full_local_paths_with_optional_labels() {
    let (_d, mut s) = board();
    s.create(Some("x"), patch(json!({ "title": "t" })), "test").unwrap();
    let c = set(
        &mut s,
        "x",
        json!({ "proof": [
            { "path": " C:\\Users\\me\\shots\\after.png ", "label": "  After the fix  " },
            { "path": "C:/Users/me/report.md", "label": "" },
        ] }),
    )
    .unwrap();
    assert_eq!(
        c.fields.get("proof"),
        Some(&json!([
            { "path": "C:\\Users\\me\\shots\\after.png", "label": "After the fix" },
            { "path": "C:/Users/me/report.md" },
        ]))
    );
    // null and [] clear it.
    let c = set(&mut s, "x", json!({ "proof": null })).unwrap();
    assert!(c.fields.get("proof").is_none());
    set(&mut s, "x", json!({ "proof": [{ "path": "C:\\a.png" }] })).unwrap();
    let c = set(&mut s, "x", json!({ "proof": [] })).unwrap();
    assert!(c.fields.get("proof").is_none());
}

#[test]
fn proof_refuses_what_changes_outputs_refuses_and_names_its_own_field() {
    let (_d, mut s) = board();
    s.create(Some("x"), patch(json!({ "title": "t" })), "test").unwrap();
    for (bad, says) in [
        (json!("C:\\a.png"), "proof is a list"),
        (json!([{ "path": "https://example.com/a.png" }]), "proof[0].path"),
        (json!([{ "path": "\\\\host\\share\\a.png" }]), "network path"),
        (json!([{ "path": "a.png" }]), "isn't a full local path"),
        (json!([{ "path": "C:\\a.png", "by": "me" }]), "proof[0] has \"by\""),
        (json!([{ "path": "C:\\a.png", "label": 3 }]), "proof[0].label is a string"),
        (json!([{ "path": "C:\\a.png", "label": "x".repeat(81) }]), "longer than 80"),
    ] {
        let m = refused(&mut s, "x", bad.clone());
        assert!(m.contains(says), "{bad}: {m}");
    }
    let many: Vec<Value> = (0..21).map(|i| json!({ "path": format!("C:\\s\\{i}.png") })).collect();
    assert!(refused(&mut s, "x", json!(many)).contains("proof takes at most 20 files"));
    // changesOutputs still names itself.
    match set(&mut s, "x", json!({ "changesOutputs": [{ "path": "a.md" }] })) {
        Err(Error::Invalid(m)) => assert!(m.starts_with("changesOutputs[0].path"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_new_card_cleans_proof_too() {
    let (_d, mut s) = board();
    let c = s
        .create(Some("y"), patch(json!({ "title": "t", "proof": [{ "path": " D:\\out\\shot.png " }] })), "test")
        .unwrap();
    assert_eq!(c.fields.get("proof"), Some(&json!([{ "path": "D:\\out\\shot.png" }])));
    assert!(matches!(
        s.create(Some("z"), patch(json!({ "title": "t", "proof": [{ "path": "shot.png" }] })), "test"),
        Err(Error::Invalid(_))
    ));
}
