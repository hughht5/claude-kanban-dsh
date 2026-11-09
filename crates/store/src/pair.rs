//! The card-reviewer and the card-tester run together (Hugh, card #162): once the build is
//! installed, a card session starts both at once. So a pass counts only if it started after the
//! other one's latest FAIL: a fix for that FAIL came after it, and this pass never saw the fix.
//!
//! Each `test` and `review` record keeps `startedAt` (when that run's Agent call started, or the
//! verdict's own `at` when the app didn't see the start) and `failedAt` (its latest FAIL, carried
//! on to later records until the move into In progress resets the record, as `round` is).

use crate::{Card, REVIEW_KEY, TEST_KEY};
use chrono::{DateTime, Local, Utc};
use serde_json::Value;

/// When the run behind a record started.
pub const STARTED_AT_KEY: &str = "startedAt";
/// When the check last failed since the card last came into In progress.
pub const FAILED_AT_KEY: &str = "failedAt";

/// A pass that started before the other check's latest FAIL.
#[derive(Debug, Clone, PartialEq)]
pub struct StalePass {
    /// The field whose pass doesn't count: `test` or `review`.
    pub key: &'static str,
    /// The other check's latest FAIL.
    pub failed_at: DateTime<Utc>,
    /// When this pass's run started.
    pub started_at: DateTime<Utc>,
}

impl StalePass {
    /// Who failed and who must run again: ("reviewer", "test", "tested", "card-tester") for a
    /// stale test.
    fn words(&self) -> (&'static str, &'static str, &'static str, &'static str) {
        if self.key == TEST_KEY {
            ("reviewer", "test", "tested", "card-tester")
        } else {
            ("tester", "review", "reviewed", "card-reviewer")
        }
    }

    /// The plain reason: "The reviewer failed at 14:05, after this test started (14:02): the
    /// fix wasn't tested. Run the card-tester again."
    pub fn why(&self) -> String {
        let (failed, this, done, agent) = self.words();
        format!(
            "The {failed} failed at {}, after this {this} started ({}): the fix wasn't {done}. Run the {agent} again.",
            hhmm(&self.failed_at),
            hhmm(&self.started_at)
        )
    }

    /// The short form a stack lead's hold puts after the check's name: "started before the
    /// reviewer failed at 14:05".
    pub fn short(&self) -> String {
        format!("started before the {} failed at {}", self.words().0, hhmm(&self.failed_at))
    }
}

/// A time as the board shows it: local hours and minutes.
pub fn hhmm(t: &DateTime<Utc>) -> String {
    t.with_timezone(&Local).format("%H:%M").to_string()
}

fn time(v: Option<&Value>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v?.as_str()?).ok().map(|t| t.with_timezone(&Utc))
}

/// When the run behind `rec` started: its `startedAt`, else its `at` (a record from an older
/// build, or a run whose start the app didn't see).
pub fn started_at(rec: &Value) -> Option<DateTime<Utc>> {
    time(rec.get(STARTED_AT_KEY)).or_else(|| time(rec.get("at")))
}

/// When `rec`'s check last failed, if it has since the reset.
pub fn failed_at(rec: &Value) -> Option<DateTime<Utc>> {
    time(rec.get(FAILED_AT_KEY))
}

/// The passes on `card` that don't count because they started before the other check's latest
/// FAIL: the test's first, then the review's. Callers ask only while both checks are on.
pub fn stale_passes(card: &Card) -> Vec<StalePass> {
    [(TEST_KEY, REVIEW_KEY), (REVIEW_KEY, TEST_KEY)]
        .into_iter()
        .filter_map(|(key, other)| {
            let rec = card.fields.get(key)?;
            if rec.get("state").and_then(Value::as_str) != Some("passed") {
                return None;
            }
            let started = started_at(rec)?;
            let failed = failed_at(card.fields.get(other)?)?;
            (started < failed).then_some(StalePass { key, failed_at: failed, started_at: started })
        })
        .collect()
}

/// Stamps a new verdict record `rec` (at `at`, its run started at `started`, if seen) with
/// `startedAt` and `failedAt`: a FAIL's own `at`, else the previous record's `failedAt`.
pub fn stamp_times(prev: Option<&Value>, rec: &mut Value, started: Option<&str>, at: &str) {
    let Some(obj) = rec.as_object_mut() else { return };
    obj.insert(STARTED_AT_KEY.into(), Value::String(started.unwrap_or(at).to_string()));
    let failed = if obj.get("state").and_then(Value::as_str) == Some("failed") {
        Some(Value::String(at.to_string()))
    } else {
        prev.and_then(|p| p.get(FAILED_AT_KEY)).cloned()
    };
    if let Some(f) = failed {
        obj.insert(FAILED_AT_KEY.into(), f);
    }
}
