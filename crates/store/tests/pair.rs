// Card #162: the card-reviewer and the card-tester run together, so a pass counts only if its run
// started after the other's latest FAIL. The records keep `startedAt` and `failedAt`.

use chrono::{DateTime, Utc};
use kanban_store::{hhmm, lead_missing, stale_passes, stamp_times, Card, Patch, Store, REVIEW_KEY, TEST_KEY};
use serde_json::{json, Value};

fn card(fields: Value) -> Card {
    Card { id: "c1".into(), number: 1, column: "In progress".into(), order: None, order_col: None, archived: false, fields: fields.as_object().cloned().unwrap(), version: 1 }
}

fn t(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}

const T0: &str = "2026-11-09T10:00:00.000Z";
const T1: &str = "2026-11-09T10:05:00.000Z";
const T2: &str = "2026-11-09T10:10:00.000Z";

#[test]
fn a_record_keeps_when_its_run_started_and_when_it_last_failed() {
    // A run whose start was seen keeps it; one whose start wasn't falls back to the verdict's time.
    let mut rec = json!({ "state": "passed", "at": T1 });
    stamp_times(None, &mut rec, Some(T0), T1);
    assert_eq!((rec["startedAt"].as_str(), rec.get("failedAt")), (Some(T0), None));
    let mut rec = json!({ "state": "passed", "at": T1 });
    stamp_times(None, &mut rec, None, T1);
    assert_eq!(rec["startedAt"].as_str(), Some(T1));
    // A FAIL sets failedAt to its own time; a later pass, or an unclear run, carries it on.
    let mut fail = json!({ "state": "failed", "at": T1 });
    stamp_times(None, &mut fail, Some(T0), T1);
    assert_eq!(fail["failedAt"].as_str(), Some(T1));
    let mut pass = json!({ "state": "passed", "at": T2 });
    stamp_times(Some(&fail), &mut pass, Some(T2), T2);
    assert_eq!((pass["failedAt"].as_str(), pass["startedAt"].as_str()), (Some(T1), Some(T2)));
    let mut unclear = json!({ "state": "unclear", "at": T2 });
    stamp_times(Some(&pass), &mut unclear, None, T2);
    assert_eq!(unclear["failedAt"].as_str(), Some(T1));
    // A second FAIL moves it on.
    let mut again = json!({ "state": "failed", "at": T2 });
    stamp_times(Some(&pass), &mut again, Some(T1), T2);
    assert_eq!(again["failedAt"].as_str(), Some(T2));
}

#[test]
fn a_test_pass_that_started_before_the_reviewers_fail_doesnt_count() {
    // Both started at T0; the reviewer failed at T1; the tester passed later, on the old build.
    let c = card(json!({ "test": { "state": "passed", "at": T2, "startedAt": T0 }, "review": { "state": "failed", "at": T1, "failedAt": T1 } }));
    let stale = stale_passes(&c);
    assert_eq!(stale.len(), 1);
    assert_eq!((stale[0].key, stale[0].failed_at, stale[0].started_at), (TEST_KEY, t(T1), t(T0)));
    assert_eq!(stale[0].why(), format!("The reviewer failed at {}, after this test started ({}): the fix wasn't tested. Run the card-tester again.", hhmm(&t(T1)), hhmm(&t(T0))));
    // The review passed after the fix, but the test still didn't see it.
    let c = card(json!({ "test": { "state": "passed", "at": T2, "startedAt": T0 }, "review": { "state": "passed", "at": T2, "startedAt": T2, "failedAt": T1 } }));
    assert_eq!(stale_passes(&c).iter().map(|s| s.key).collect::<Vec<_>>(), vec![TEST_KEY]);
    // A test started after the FAIL counts.
    let c = card(json!({ "test": { "state": "passed", "at": T2, "startedAt": T2 }, "review": { "state": "passed", "at": T2, "startedAt": T2, "failedAt": T1 } }));
    assert!(stale_passes(&c).is_empty());
}

#[test]
fn a_review_pass_that_started_before_the_testers_fail_doesnt_count() {
    let c = card(json!({ "review": { "state": "passed", "at": T2, "startedAt": T0 }, "test": { "state": "failed", "at": T1, "failedAt": T1 } }));
    let stale = stale_passes(&c);
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].key, REVIEW_KEY);
    assert!(stale[0].why().starts_with("The tester failed at ") && stale[0].why().ends_with("the fix wasn't reviewed. Run the card-reviewer again."), "{}", stale[0].why());
    let c = card(json!({ "review": { "state": "passed", "at": T2, "startedAt": T2 }, "test": { "state": "passed", "at": T2, "failedAt": T1 } }));
    assert!(stale_passes(&c).is_empty());
}

#[test]
fn a_record_with_no_start_falls_back_to_its_verdict_time() {
    // An older build's pass (no startedAt): its `at` is used. Before the FAIL: stale.
    let c = card(json!({ "test": { "state": "passed", "at": T0 }, "review": { "state": "passed", "at": T2, "failedAt": T1 } }));
    assert_eq!(stale_passes(&c).len(), 1);
    // After it: counts.
    let c = card(json!({ "test": { "state": "passed", "at": T2 }, "review": { "state": "passed", "at": T2, "failedAt": T1 } }));
    assert!(stale_passes(&c).is_empty());
    // No time at all, no FAIL time, or a record that isn't a pass: nothing to hold.
    for f in [
        json!({ "test": { "state": "passed" }, "review": { "state": "failed", "failedAt": T1 } }),
        json!({ "test": { "state": "passed", "startedAt": T0 }, "review": { "state": "failed" } }),
        json!({ "test": { "state": "failed", "startedAt": T0 }, "review": { "state": "failed", "failedAt": T1 } }),
        json!({ "test": { "state": "passed", "startedAt": "not a time" }, "review": { "state": "failed", "failedAt": T1 } }),
        json!({ "test": { "state": "passed", "startedAt": T0 } }),
    ] {
        assert!(stale_passes(&card(f.clone())).is_empty(), "{f}");
    }
}

#[test]
fn the_move_into_in_progress_resets_failed_at_with_the_round() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("b.db")).unwrap();
    let c = s.create(None, json!({ "title": "T", "column": "In progress" }).as_object().cloned().unwrap(), "test").unwrap();
    let mut p = Patch::new();
    p.insert("review".into(), json!({ "state": "passed", "at": T2, "startedAt": T2, "round": 1, "failedAt": T1 }));
    p.insert("test".into(), json!({ "state": "passed", "at": T2, "startedAt": T0 }));
    let c = s.update(&c.id, c.version, &p, "the app").unwrap();
    assert_eq!(stale_passes(&c).len(), 1);
    let mut p = Patch::new();
    p.insert("column".into(), json!("Blocked"));
    let c = s.update(&c.id, c.version, &p, "test").unwrap();
    assert_eq!(c.fields["review"]["failedAt"].as_str(), Some(T1), "a stop in Blocked keeps it");
    let mut p = Patch::new();
    p.insert("column".into(), json!("In progress"));
    let c = s.update(&c.id, c.version, &p, "test").unwrap();
    for key in [REVIEW_KEY, TEST_KEY] {
        let rec = &c.fields[key];
        assert_eq!(rec["state"], "needed");
        assert!(rec.get("failedAt").is_none() && rec.get("round").is_none(), "{rec}");
    }
    // And the next verdicts start clean.
    let mut rec = json!({ "state": "passed", "at": T2 });
    stamp_times(c.fields.get(REVIEW_KEY), &mut rec, Some(T2), T2);
    assert!(rec.get("failedAt").is_none());
}

#[test]
fn a_leads_own_stale_pass_holds_its_stack_only_while_both_are_on() {
    let lead = card(json!({ "test": { "state": "passed", "at": T2, "startedAt": T0 }, "review": { "state": "passed", "at": T2, "startedAt": T2, "failedAt": T1 } }));
    let both = [(TEST_KEY, "subagent test", true), (REVIEW_KEY, "subagent review", true)];
    assert_eq!(lead_missing(&lead, &both), vec![format!("subagent test (started before the reviewer failed at {})", hhmm(&t(T1)))]);
    // Either rule off (or the review stood in for by the children's, so not in the list): no hold.
    assert!(lead_missing(&lead, &[(TEST_KEY, "subagent test", true), (REVIEW_KEY, "subagent review", false)]).is_empty());
    assert!(lead_missing(&lead, &[(TEST_KEY, "subagent test", true)]).is_empty());
    assert!(lead_missing(&lead, &[(TEST_KEY, "subagent test", false), (REVIEW_KEY, "subagent review", true)]).is_empty());
    // A fresh test: the stack goes on.
    let fresh = card(json!({ "test": { "state": "passed", "at": T2, "startedAt": T2 }, "review": { "state": "passed", "at": T2, "startedAt": T2, "failedAt": T1 } }));
    assert!(lead_missing(&fresh, &both).is_empty());
}
