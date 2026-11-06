//! The card-tester's screenshots (card #101): the card field `evidence`. It is the app's own
//! field, so no session or `kb update` can set it (it isn't in the tools' UPDATABLE list), and
//! the app only ever adds a run to it, in the same write as the tester's verdict. The board's
//! revisions keep every older version, and an older build keeps the field as it is, because
//! card fields are a JSON map.
//!
//! Its shape, newest run first:
//! `[{run, at, verdict, build?, shots: [{n, kind, caption, bytes, width, height, file, path, build?}]}]`,
//! where `run` is the tester's agent id, `at` is the verdict's own `at` (the test record's), so a
//! resumed tester, which keeps its agent id, still has runs that differ, `verdict` is the test's
//! state ("passed", "failed", "unclear"), `kind` is "before" or "after", `file` is relative to
//! the data folder and `path` is absolute, both with forward slashes. `build` is the test app's
//! build commit that `tools/cdp.mjs` wrote into the shot (the run's: the shots' common one, or
//! "mixed"); it is left out when there is none.

use crate::{Card, TEST_KEY};
use serde_json::Value;

/// The card field that holds the runs.
pub const EVIDENCE_KEY: &str = "evidence";

/// The card's new evidence list: `run` first, then every older run as it was. It never drops
/// a run; an old value that isn't a list is kept as the oldest entry.
pub fn add_run(card: &Card, run: Value) -> Value {
    let mut out = vec![run];
    match card.fields.get(EVIDENCE_KEY) {
        Some(Value::Array(old)) => out.extend(old.iter().cloned()),
        None | Some(Value::Null) => {}
        Some(other) => out.push(other.clone()),
    }
    Value::Array(out)
}

/// Whether the card's latest subagent test passed and its latest evidence run backs that pass:
/// the test's state is "passed", the latest run (`evidence[0]`) has an "after" shot, and that
/// run is the one the test records: the same agent (`test.agentId`) and the same `at`
/// (`test.at`). A resumed tester keeps its agent id, so the agent alone doesn't tell an older
/// run from this one; the `at` does, as the app writes one `at` on both.
pub fn has_tested_evidence(card: &Card) -> bool {
    let Some(test) = card.fields.get(TEST_KEY) else { return false };
    if test.get("state").and_then(Value::as_str) != Some("passed") {
        return false;
    }
    let Some(agent) = test.get("agentId").and_then(Value::as_str).filter(|a| !a.is_empty()) else { return false };
    let Some(at) = test.get("at").and_then(Value::as_str).filter(|a| !a.is_empty()) else { return false };
    let Some(latest) = card.fields.get(EVIDENCE_KEY).and_then(|e| e.get(0)) else { return false };
    latest.get("run").and_then(Value::as_str) == Some(agent)
        && latest.get("at").and_then(Value::as_str) == Some(at)
        && latest
            .get("shots")
            .and_then(Value::as_array)
            .is_some_and(|shots| shots.iter().any(|s| s.get("kind").and_then(Value::as_str) == Some("after")))
}
