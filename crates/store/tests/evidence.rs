// The card-tester's screenshots (card #101): the card field `evidence`, newest run first, and
// whether the latest run backs the latest test pass (the same agent and the same `at`).

use kanban_store::{add_run, has_tested_evidence, Card, Patch, Store, EVIDENCE_KEY, TEST_KEY};
use serde_json::{json, Value};

fn patch(v: Value) -> Patch {
    v.as_object().cloned().unwrap()
}

fn card(fields: Value) -> Card {
    Card { id: "c1".into(), number: 1, column: "In progress".into(), order: None, order_col: None, archived: false, fields: patch(fields), version: 1 }
}

fn shot(n: u64, kind: &str) -> Value {
    json!({ "n": n, "kind": kind, "caption": "the chip", "bytes": 1234, "width": 800, "height": 600,
            "file": format!("evidence/a1/{n}-{kind}.png"), "path": format!("C:/Users/x/.claude-kanban-dsh/evidence/a1/{n}-{kind}.png") })
}

/// The `at` the app writes on both the test record and its run.
const AT: &str = "2026-11-06T03:00:00.000Z";

fn run(id: &str, shots: Vec<Value>) -> Value {
    run_at(id, AT, shots)
}

fn run_at(id: &str, at: &str, shots: Vec<Value>) -> Value {
    json!({ "run": id, "at": at, "verdict": "passed", "shots": shots })
}

#[test]
fn add_run_puts_the_new_run_first_and_keeps_every_older_one() {
    let none = card(json!({ "title": "T" }));
    let one = add_run(&none, run("a1", vec![shot(1, "after")]));
    assert_eq!(one, json!([run("a1", vec![shot(1, "after")])]));

    let c = card(json!({ "title": "T", "evidence": one }));
    let two = add_run(&c, run("a2", vec![shot(1, "before"), shot(2, "after")]));
    let list = two.as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["run"], "a2");
    assert_eq!(list[1], run("a1", vec![shot(1, "after")]), "the older run is unchanged");

    let c = card(json!({ "title": "T", "evidence": two.clone() }));
    let three = add_run(&c, run("a3", vec![]));
    let ids: Vec<&str> = three.as_array().unwrap().iter().map(|r| r["run"].as_str().unwrap()).collect();
    assert_eq!(ids, ["a3", "a2", "a1"]);
    assert_eq!(&three.as_array().unwrap()[1..], two.as_array().unwrap().as_slice());

    // A field that isn't a list is never dropped: it stays as the oldest entry.
    let odd = card(json!({ "title": "T", "evidence": { "run": "old" } }));
    let out = add_run(&odd, run("a4", vec![]));
    assert_eq!(out.as_array().unwrap().len(), 2);
    assert_eq!(out[1], json!({ "run": "old" }));
}

#[test]
fn tested_evidence_needs_a_pass_an_after_shot_and_the_same_run() {
    let good = json!({ "title": "T", "test": { "state": "passed", "agentId": "a2", "at": AT },
                       "evidence": [run("a2", vec![shot(1, "before"), shot(2, "after")]), run("a1", vec![shot(1, "after")])] });
    assert!(has_tested_evidence(&card(good.clone())));

    // The test didn't pass.
    for state in ["failed", "unclear", "needed"] {
        let mut f = good.clone();
        f[TEST_KEY]["state"] = json!(state);
        assert!(!has_tested_evidence(&card(f)), "{state}");
    }
    // No "after" shot in the latest run (an older run's "after" doesn't count).
    let mut f = good.clone();
    f[EVIDENCE_KEY][0]["shots"] = json!([shot(1, "before")]);
    assert!(!has_tested_evidence(&card(f)));
    let mut f = good.clone();
    f[EVIDENCE_KEY][0]["shots"] = json!([]);
    assert!(!has_tested_evidence(&card(f)));
    // The latest run is another run's: the shots don't back this pass.
    let mut f = good.clone();
    f[TEST_KEY]["agentId"] = json!("a1");
    assert!(!has_tested_evidence(&card(f)));
    let mut f = good.clone();
    f[EVIDENCE_KEY][0]["run"] = json!("a9");
    assert!(!has_tested_evidence(&card(f)));
    // A resumed tester keeps its agent id: the latest run is an older run of the same agent
    // (another `at`), so its shots don't back this pass.
    let mut f = good.clone();
    f[TEST_KEY]["at"] = json!("2026-11-06T05:00:00.000Z");
    assert!(!has_tested_evidence(&card(f)));
    let mut f = good.clone();
    f[EVIDENCE_KEY][0]["at"] = json!("2026-11-06T02:00:00.000Z");
    assert!(!has_tested_evidence(&card(f)));
    // The newer run of the same agent, put first with the test's `at`, does back it.
    let mut f = good.clone();
    let later = "2026-11-06T05:00:00.000Z";
    f[TEST_KEY]["at"] = json!(later);
    f[EVIDENCE_KEY] = json!([run_at("a2", later, vec![shot(1, "after")]), run("a2", vec![shot(1, "after")])]);
    assert!(has_tested_evidence(&card(f)));
    // A test or a run with no `at`, or an empty one, doesn't count.
    let mut f = good.clone();
    f[TEST_KEY].as_object_mut().unwrap().remove("at");
    assert!(!has_tested_evidence(&card(f)));
    let mut f = good.clone();
    f[EVIDENCE_KEY][0].as_object_mut().unwrap().remove("at");
    assert!(!has_tested_evidence(&card(f)));
    let mut f = good.clone();
    f[TEST_KEY]["at"] = json!("");
    f[EVIDENCE_KEY][0]["at"] = json!("");
    assert!(!has_tested_evidence(&card(f)));
    // The test reset to needed by a move into In progress (no agentId).
    let mut f = good.clone();
    f[TEST_KEY] = json!({ "state": "needed", "at": "2026-11-06T04:00:00.000Z" });
    assert!(!has_tested_evidence(&card(f)));
    // A pass with no agentId, no test at all, or no evidence field.
    let mut f = good.clone();
    f[TEST_KEY] = json!({ "state": "passed" });
    assert!(!has_tested_evidence(&card(f)));
    let mut f = good.clone();
    f.as_object_mut().unwrap().remove(TEST_KEY);
    assert!(!has_tested_evidence(&card(f)));
    let mut f = good.clone();
    f.as_object_mut().unwrap().remove(EVIDENCE_KEY);
    assert!(!has_tested_evidence(&card(f)));
    let mut f = good;
    f[EVIDENCE_KEY] = json!([]);
    assert!(!has_tested_evidence(&card(f)));
}

#[test]
fn the_evidence_field_survives_the_store_and_a_reset() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("board.db")).unwrap();
    let c = s.create(None, patch(json!({ "title": "T", "column": "In progress" })), "test").unwrap();
    // The app's write: the verdict and the run in one patch.
    let ev = add_run(&c, run("a1", vec![shot(1, "after")]));
    let mut p = Patch::new();
    p.insert(TEST_KEY.into(), json!({ "state": "passed", "agentId": "a1", "at": AT, "round": 0 }));
    p.insert(EVIDENCE_KEY.into(), ev.clone());
    let c = s.update(&c.id, c.version, &p, "the app (card-tester subagent)").unwrap();
    let c = s.get(&c.id).unwrap();
    assert_eq!(c.fields[EVIDENCE_KEY], ev);
    assert!(has_tested_evidence(&c));
    // Out of In progress and back in: the test resets, the evidence stays, and no longer counts.
    let c = s.update(&c.id, c.version, &patch(json!({ "column": "Blocked" })), "test").unwrap();
    let c = s.update(&c.id, c.version, &patch(json!({ "column": "In progress" })), "test").unwrap();
    let c = s.get(&c.id).unwrap();
    assert_eq!(c.fields[TEST_KEY]["state"], "needed");
    assert_eq!(c.fields[EVIDENCE_KEY], ev);
    assert!(!has_tested_evidence(&c));
}

#[test]
fn an_unknown_field_is_kept_through_an_update() {
    // So an older build, which doesn't know `evidence`, keeps it as it is.
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("board.db")).unwrap();
    let extra = json!([{ "run": "a1", "shots": [{ "n": 1, "kind": "after" }] }]);
    let c = s.create(None, patch(json!({ "title": "T", "someLaterField": extra.clone() })), "test").unwrap();
    let c = s.update(&c.id, c.version, &patch(json!({ "summary": "changed", "nextStep": "next" })), "test").unwrap();
    let c = s.update(&c.id, c.version, &patch(json!({ "column": "In progress" })), "test").unwrap();
    let c = s.get(&c.id).unwrap();
    assert_eq!(c.fields["someLaterField"], extra);
    assert_eq!(c.fields["summary"], "changed");
}
