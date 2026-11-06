// Card #108: what changed on each card. Two ordinary JSON fields, no migration:
// `changesOutputs` (files a session hands over to Hugh) and `changesSaved` (the app's figures).

use kanban_store::{Error, Patch, Store};
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

fn set(s: &mut Store, id: &str, v: Value) -> kanban_store::Result<kanban_store::Card> {
    let c = s.get(id).unwrap();
    s.update(id, c.version, &patch(v), "test")
}

fn refused(s: &mut Store, id: &str, outputs: Value) -> String {
    match set(s, id, json!({ "changesOutputs": outputs })) {
        Err(Error::Invalid(m)) => m,
        other => panic!("expected a refusal for {outputs}, got {other:?}"),
    }
}

#[test]
fn outputs_take_full_local_paths_with_optional_labels() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    s.create(Some("x"), patch(json!({ "title": "t" })), "test").unwrap();
    let c = set(
        &mut s,
        "x",
        json!({ "changesOutputs": [
            { "path": " C:\\Users\\someone\\report.md ", "label": "  The report  " },
            { "path": "D:/out/chart.png", "label": "" },
            { "path": "C:\\x\\notes.txt", "label": null },
            { "path": "C:\\x\\y.pdf" }
        ] }),
    )
    .unwrap();
    assert_eq!(
        c.fields["changesOutputs"],
        json!([
            { "path": "C:\\Users\\someone\\report.md", "label": "The report" },
            { "path": "D:/out/chart.png" },
            { "path": "C:\\x\\notes.txt" },
            { "path": "C:\\x\\y.pdf" }
        ])
    );
    // It reads back as written.
    assert_eq!(s.get("x").unwrap().fields["changesOutputs"], c.fields["changesOutputs"]);
    // A label of exactly 80 characters is fine.
    let long = "a".repeat(80);
    let c = set(&mut s, "x", json!({ "changesOutputs": [{ "path": "C:\\a.md", "label": long }] })).unwrap();
    assert_eq!(c.fields["changesOutputs"][0]["label"], json!("a".repeat(80)));
    // On create too.
    let c = s.create(Some("y"), patch(json!({ "title": "t", "changesOutputs": [{ "path": "C:\\b.md", "label": " b " }] })), "test").unwrap();
    assert_eq!(c.fields["changesOutputs"], json!([{ "path": "C:\\b.md", "label": "b" }]));
    assert!(s.create(Some("z"), patch(json!({ "title": "t", "changesOutputs": [{ "path": "b.md" }] })), "test").is_err());
}

#[test]
fn outputs_refuse_urls_network_and_relative_paths_and_other_shapes() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    s.create(Some("x"), patch(json!({ "title": "t" })), "test").unwrap();
    let one = |p: &str| json!([{ "path": p }]);
    assert!(refused(&mut s, "x", one("https://example.com/a.md")).contains("URL"));
    assert!(refused(&mut s, "x", one("file:///C:/a.md")).contains("URL"));
    assert!(refused(&mut s, "x", one("\\\\server\\share\\a.md")).contains("network"));
    assert!(refused(&mut s, "x", one("//server/share/a.md")).contains("network"));
    assert!(refused(&mut s, "x", one("\\\\wsl$\\Ubuntu\\home\\a.md")).contains("network"));
    assert!(refused(&mut s, "x", one("docs\\a.md")).contains("full local path"));
    assert!(refused(&mut s, "x", one("/home/h/a.md")).contains("full local path"));
    assert!(refused(&mut s, "x", one("C:a.md")).contains("full local path"));
    assert!(refused(&mut s, "x", one("")).contains("empty"));
    assert!(refused(&mut s, "x", one("   ")).contains("empty"));
    assert!(refused(&mut s, "x", json!([{ "path": "C:\\a.md", "label": "b".repeat(81) }])).contains("80"));
    assert!(refused(&mut s, "x", json!([{ "path": "C:\\a.md", "label": 5 }])).contains("label"));
    assert!(refused(&mut s, "x", json!([{ "path": "C:\\a.md", "kind": "doc" }])).contains("\"kind\""));
    assert!(refused(&mut s, "x", json!([{ "label": "no path" }])).contains("list of"));
    assert!(refused(&mut s, "x", json!(["C:\\a.md"])).contains("list of"));
    assert!(refused(&mut s, "x", json!("C:\\a.md")).contains("list of"));
    assert!(refused(&mut s, "x", json!({ "path": "C:\\a.md" })).contains("list of"));
    let many: Vec<Value> = (0..21).map(|i| json!({ "path": format!("C:\\f{i}.md") })).collect();
    assert!(refused(&mut s, "x", json!(many)).contains("at most 20"));
    let twenty: Vec<Value> = (0..20).map(|i| json!({ "path": format!("C:\\f{i}.md") })).collect();
    assert!(set(&mut s, "x", json!({ "changesOutputs": twenty })).is_ok());
    // The secret check still applies.
    let c = s.get("x").unwrap();
    let e = s.update("x", c.version, &patch(json!({ "changesOutputs": [{ "path": "C:\\a.md", "label": "my password" }] })), "test");
    assert!(matches!(e, Err(Error::Secret(_))), "{e:?}");
    // Nothing refused was written.
    assert_eq!(s.get("x").unwrap().fields["changesOutputs"].as_array().unwrap().len(), 20);
}

#[test]
fn null_or_an_empty_list_clears_outputs() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    s.create(Some("x"), patch(json!({ "title": "t" })), "test").unwrap();
    set(&mut s, "x", json!({ "changesOutputs": [{ "path": "C:\\a.md" }] })).unwrap();
    let c = set(&mut s, "x", json!({ "changesOutputs": [] })).unwrap();
    assert!(c.fields.get("changesOutputs").is_none());
    set(&mut s, "x", json!({ "changesOutputs": [{ "path": "C:\\a.md" }] })).unwrap();
    let c = set(&mut s, "x", json!({ "changesOutputs": null })).unwrap();
    assert!(c.fields.get("changesOutputs").is_none());
    // Clearing what isn't there changes nothing.
    let v = c.version;
    assert_eq!(set(&mut s, "x", json!({ "changesOutputs": null })).unwrap().version, v);
}

#[test]
fn saved_figures_are_an_object_or_null() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    s.create(Some("x"), patch(json!({ "title": "t" })), "test").unwrap();
    for bad in [json!("x"), json!(5), json!([1]), json!(true)] {
        assert!(matches!(set(&mut s, "x", json!({ "changesSaved": bad })), Err(Error::Invalid(_))));
    }
    let saved = json!({ "at": "2026-11-06T10:00:00Z", "source": "branch", "branch": "b", "base": "abc",
                        "totals": { "files": 1, "added": 2, "removed": 0 },
                        "files": [{ "path": "a b/ü.txt", "old": null, "status": "A", "added": 2, "removed": 0 }],
                        "more": 0, "commits": [{ "sha": "abcdef1", "subject": "card #108" }], "commitCount": 1 });
    let c = set(&mut s, "x", json!({ "changesSaved": saved.clone() })).unwrap();
    assert_eq!(c.fields["changesSaved"], saved);
    let c = set(&mut s, "x", json!({ "changesSaved": null })).unwrap();
    assert!(c.fields.get("changesSaved").is_none());
    // The same check on create.
    for bad in [json!("x"), json!(5), json!([1]), json!(true)] {
        let e = s.create(None, patch(json!({ "title": "t", "changesSaved": bad })), "test");
        assert!(matches!(e, Err(Error::Invalid(_))), "{e:?}");
    }
    let c = s.create(Some("y"), patch(json!({ "title": "t", "changesSaved": saved.clone() })), "test").unwrap();
    assert_eq!(c.fields["changesSaved"], saved);
    let c = s.create(Some("z"), patch(json!({ "title": "t", "changesSaved": null })), "test").unwrap();
    assert!(c.fields.get("changesSaved").is_none());
}

/// A card carrying both fields reads back exactly as written, the way an older build that
/// doesn't know them carries them (ordinary fields, no migration).
#[test]
fn a_card_with_both_fields_reads_back_unchanged() {
    let (_d, path) = temp_board();
    let outputs = json!([{ "path": "C:\\Users\\someone\\out\\report.md", "label": "Report" }, { "path": "C:\\x.png" }]);
    let saved = json!({ "at": "2026-11-06T10:00:00.000Z", "source": "commits", "branch": null, "base": null,
                        "totals": { "files": 2, "added": 10, "removed": 3 },
                        "files": [{ "path": "bin.dat", "old": null, "status": "M", "added": null, "removed": null },
                                  { "path": "new name.txt", "old": "old name.txt", "status": "R", "added": 10, "removed": 3 }],
                        "more": 0, "commits": [], "commitCount": 0, "somethingNewer": { "kept": true } });
    {
        let mut s = Store::open(&path).unwrap();
        s.create(Some("x"), patch(json!({ "title": "t", "column": "Human test" })), "test").unwrap();
        set(&mut s, "x", json!({ "changesOutputs": outputs.clone(), "changesSaved": saved.clone() })).unwrap();
        // Other writes leave them alone.
        set(&mut s, "x", json!({ "summary": "s" })).unwrap();
    }
    let s = Store::open(&path).unwrap();
    let c = s.get("x").unwrap();
    assert_eq!(c.fields["changesOutputs"], outputs);
    assert_eq!(c.fields["changesSaved"], saved);
    let doc = c.to_document(&[]);
    assert_eq!(doc["changesOutputs"], outputs);
    assert_eq!(doc["changesSaved"], saved);
}
