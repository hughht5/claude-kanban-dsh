// Card #120: the agents' feed, from the agents' log lines and the verdicts in the revisions.

use kanban_store::{FeedItem, FeedQuery, Patch, Store, FEED_MAX_LIMIT};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const T0: i64 = 1_793_923_200_000; // 6 Nov 2026, 00:00 UTC
const MIN: i64 = 60_000;

fn at(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
fn patch(v: Value) -> Patch {
    v.as_object().cloned().unwrap()
}
fn temp_board() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    (dir, path)
}
/// A board with cards #1 and #2 (and #0 when `zero`), made through the store.
fn board(path: &Path, zero: bool) {
    let mut s = Store::open(path).unwrap();
    s.create(None, patch(json!({ "title": "one" })), "test").unwrap();
    s.create(None, patch(json!({ "title": "two" })), "test").unwrap();
    drop(s);
    if zero {
        // The router's system card (card #0) isn't made by the store yet: written straight in.
        let db = rusqlite::Connection::open(path).unwrap();
        db.execute(
            "INSERT INTO cards (id, number, col, archived, fields, created_at, updated_at, version) VALUES ('card-0', 0, 'Backlog', 0, ?1, ?2, ?2, 1)",
            rusqlite::params![json!({ "title": "router", "system": "router" }).to_string(), at(T0)],
        )
        .unwrap();
    }
}
fn log(db: &rusqlite::Connection, card: &str, ms: i64, text: &str, by: Option<&str>) {
    db.execute("INSERT INTO card_log (card_id, at, text, by) VALUES (?1, ?2, ?3, ?4)", rusqlite::params![card, at(ms), text, by]).unwrap();
}
fn rev(db: &rusqlite::Connection, card: &str, ms: i64, by: &str, changed: Value) {
    db.execute(
        "INSERT INTO card_revisions (card_id, version, at, by, changed) VALUES (?1, 99, ?2, ?3, ?4)",
        rusqlite::params![card, at(ms), by, changed.to_string()],
    )
    .unwrap();
}
fn q(v: Value) -> FeedQuery {
    serde_json::from_value(v).unwrap()
}
/// Only the rows this test wrote (the cards' own create revisions are left out by their by).
fn texts(items: &[FeedItem]) -> Vec<String> {
    items.iter().map(|i| format!("{}:{}:{}", i.kind, i.agent, i.verdict.clone().unwrap_or_else(|| i.text.clone()))).collect()
}

#[test]
fn both_sources_merge_newest_first_and_map_agents_by_by() {
    let (_d, path) = temp_board();
    board(&path, false);
    let db = rusqlite::Connection::open(&path).unwrap();
    log(&db, "card-1", T0, "Test passed", Some("the app (card-tester subagent)"));
    rev(&db, "card-1", T0 + MIN, "the app (card-reviewer subagent)", json!({ "review": { "state": "failed", "detail": "a; b" } }));
    log(&db, "card-2", T0 + 2 * MIN, "Routed: small", Some("the app (card-router subagent)"));
    rev(&db, "card-2", T0 + 3 * MIN, "the app", json!({ "route": { "size": "small", "reason": "one file" } }));
    log(&db, "card-1", T0 + 4 * MIN, "the card-tester said hi", Some("card #1's session")); // unknown by: left out
    log(&db, "card-1", T0 + 5 * MIN, "no by", None);
    rev(&db, "card-1", T0 + 6 * MIN, "Hugh (board)", json!({ "test": { "state": "needed" } })); // Hugh: left out
    rev(&db, "card-1", T0 + 7 * MIN, "the app (monitor)", json!({ "stuck": { "kind": "quiet", "reason": "May be stuck (quiet)" } }));
    rev(&db, "card-1", T0 + 8 * MIN, "the app (monitor)", json!({ "stuck": null }));
    log(&db, "card-2", T0 + 9 * MIN, "Typed a nudge", Some("the app (monitor)"));
    rev(&db, "card-2", T0 + 10 * MIN, "the app", json!({ "merge": { "state": "passed", "detail": "rebased" }, "test": { "state": "passed" }, "summary": "x" }));
    let s = Store::open(&path).unwrap();
    let page = s.agents_feed(&FeedQuery::default()).unwrap();
    assert_eq!(page.next, None);
    assert_eq!(
        texts(&page.items),
        vec![
            "verdict:merger:passed",
            "verdict:tester:passed",
            "log:monitor:Typed a nudge",
            "verdict:monitor:cleared",
            "verdict:monitor:quiet",
            "verdict:router:small",
            "log:router:Routed: small",
            "verdict:reviewer:failed",
            "log:tester:Test passed",
        ]
    );
    let first = &page.items[0];
    assert_eq!((first.card, first.field.as_deref(), first.text.as_str()), (2, Some("merge"), "rebased"));
    let route = page.items.iter().find(|i| i.agent == "router" && i.kind == "verdict").unwrap();
    assert_eq!(route.text, "one file");
    let rev_item = page.items.iter().find(|i| i.agent == "reviewer").unwrap();
    assert_eq!((rev_item.card, rev_item.text.as_str()), (1, "a; b"));
    // The JSON the page reads.
    let v = serde_json::to_value(&page).unwrap();
    assert_eq!(v["next"], Value::Null);
    let keys: HashSet<String> = v["items"][0].as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, ["seq", "at", "card", "agent", "kind", "verdict", "field", "text"].iter().map(|s| s.to_string()).collect());
    assert_eq!(v["items"][2]["verdict"], Value::Null);
}

#[test]
fn filters_by_agent_card_kind_and_since() {
    let (_d, path) = temp_board();
    board(&path, true);
    let db = rusqlite::Connection::open(&path).unwrap();
    log(&db, "card-0", T0, "router card line", Some("the router"));
    log(&db, "card-1", T0 + MIN, "tester line", Some("card-tester"));
    log(&db, "card-2", T0 + 2 * MIN, "reviewer line", Some("the reviewer"));
    rev(&db, "card-2", T0 + 3 * MIN, "card-critic", json!({ "critique": { "state": "passed" } }));
    rev(&db, "card-1", T0 + 4 * MIN, "the app (card-reproducer subagent)", json!({ "repro": { "state": "reproduced", "detail": "seen" } }));
    let s = Store::open(&path).unwrap();
    let got = s.agents_feed(&q(json!({ "agents": ["tester", "critic"] }))).unwrap();
    assert_eq!(texts(&got.items), vec!["verdict:critic:passed", "log:tester:tester line"]);
    let got = s.agents_feed(&q(json!({ "cards": [2] }))).unwrap();
    assert_eq!(texts(&got.items), vec!["verdict:critic:passed", "log:reviewer:reviewer line"]);
    let got = s.agents_feed(&q(json!({ "cards": [0] }))).unwrap();
    assert_eq!(got.items.len(), 1);
    assert_eq!((got.items[0].card, got.items[0].agent.as_str()), (0, "router"));
    let got = s.agents_feed(&q(json!({ "kinds": ["verdict"] }))).unwrap();
    assert_eq!(texts(&got.items), vec!["verdict:reproducer:reproduced", "verdict:critic:passed"]);
    let got = s.agents_feed(&q(json!({ "kinds": ["log"], "since": at(T0 + MIN) }))).unwrap();
    assert_eq!(texts(&got.items), vec!["log:reviewer:reviewer line", "log:tester:tester line"]);
    // A since without milliseconds or in another zone means the same moment.
    let got = s.agents_feed(&q(json!({ "since": "2026-11-06T01:03:00+01:00" }))).unwrap();
    assert_eq!(texts(&got.items), vec!["verdict:reproducer:reproduced", "verdict:critic:passed"]);
    assert!(s.agents_feed(&q(json!({ "cards": [] }))).unwrap().items.is_empty());
    assert!(s.agents_feed(&q(json!({ "since": "yesterday" }))).is_err());
    assert!(s.agents_feed(&q(json!({ "before": "nope" }))).is_err());
}

#[test]
fn paging_has_no_duplicates_or_gaps_and_the_limit_is_capped() {
    let (_d, path) = temp_board();
    board(&path, false);
    let db = rusqlite::Connection::open(&path).unwrap();
    // 700 rows of both kinds, many at the same moment, and revisions with two verdicts each.
    let mut want = 0;
    for i in 0..350i64 {
        let t = T0 + (i / 4) * MIN; // four rows share each minute
        log(&db, if i % 2 == 0 { "card-1" } else { "card-2" }, t, &format!("line {i}"), Some("card-merger"));
        rev(&db, "card-1", t, "the app", json!({ "test": { "state": "passed" }, "review": { "state": "passed" } }));
        want += 3;
    }
    let s = Store::open(&path).unwrap();
    let all = s.agents_feed(&q(json!({ "limit": 5000 }))).unwrap();
    assert_eq!(all.items.len(), FEED_MAX_LIMIT);
    assert!(all.next.is_some());
    assert_eq!(s.agents_feed(&FeedQuery::default()).unwrap().items.len(), 50);
    for limit in [7usize, 50, 200] {
        let mut seen: Vec<(String, i64, String, String)> = vec![];
        let mut before: Option<String> = None;
        loop {
            let page = s.agents_feed(&q(json!({ "limit": limit, "before": before }))).unwrap();
            assert!(page.items.len() <= limit);
            seen.extend(page.items.iter().map(|i| (i.at.clone(), i.seq, i.kind.clone(), i.field.clone().unwrap_or_default())));
            match page.next {
                Some(n) => before = Some(n),
                None => break,
            }
        }
        assert_eq!(seen.len(), want, "limit {limit}");
        let unique: HashSet<_> = seen.iter().cloned().collect();
        assert_eq!(unique.len(), want, "no duplicates at limit {limit}");
        assert!(seen.windows(2).all(|w| w[0].0 >= w[1].0), "newest first at limit {limit}");
    }
    // A filtered page fills up past rows it leaves out.
    let only = s.agents_feed(&q(json!({ "agents": ["merger"], "limit": 200 }))).unwrap();
    assert_eq!(only.items.len(), 200);
    assert!(only.items.iter().all(|i| i.kind == "log"));
}

#[test]
fn verdict_text_is_clipped_and_secrets_masked() {
    let (_d, path) = temp_board();
    board(&path, false);
    let db = rusqlite::Connection::open(&path).unwrap();
    rev(&db, "card-1", T0, "the app", json!({ "test": { "state": "failed", "detail": "x".repeat(1000) } }));
    rev(&db, "card-1", T0 + MIN, "the app", json!({ "review": { "state": "failed", "detail": "leaked sk_live_abc123 here" } }));
    log(&db, "card-1", T0 + 2 * MIN, "Bearer abc.def is in the log", Some("card-tester"));
    db.execute("INSERT INTO card_revisions (card_id, version, at, by, changed) VALUES ('card-1', 99, ?1, 'the app', 'not json')", [at(T0 + 3 * MIN)]).unwrap();
    let s = Store::open(&path).unwrap();
    let items = s.agents_feed(&FeedQuery::default()).unwrap().items;
    assert_eq!(items.len(), 3, "the bad JSON row is skipped, not an error");
    assert_eq!(items[0].text, "Bearer [hidden] is in the log");
    assert!(!items[1].text.contains("sk_live"));
    assert_eq!(items[2].text.chars().count(), 300);
}

#[test]
fn a_rare_agents_filter_still_returns_the_right_page() {
    let (_d, path) = temp_board();
    board(&path, false);
    let db = rusqlite::Connection::open(&path).unwrap();
    // Three merger items, oldest, under far more than a batch of other agents' rows.
    log(&db, "card-1", T0, "merger one", Some("card-merger"));
    rev(&db, "card-2", T0 + MIN, "the app", json!({ "merge": { "state": "merged", "detail": "two" }, "test": { "state": "passed" } }));
    log(&db, "card-1", T0 + 2 * MIN, "merger three", Some("the app (card-merger subagent)"));
    for i in 0..700 {
        log(&db, "card-1", T0 + 3 * MIN + i, &format!("tester {i}"), Some("card-tester"));
        rev(&db, "card-2", T0 + 3 * MIN + i, "the app", json!({ "test": { "state": "passed" } }));
    }
    let s = Store::open(&path).unwrap();
    let first = s.agents_feed(&q(json!({ "agents": ["merger"], "limit": 2 }))).unwrap();
    assert_eq!(texts(&first.items), vec!["log:merger:merger three", "verdict:merger:merged"]);
    assert_eq!(first.items[1].field.as_deref(), Some("merge"), "the same revision's test verdict is the tester's");
    let next = first.next.clone().expect("a third item");
    let second = s.agents_feed(&q(json!({ "agents": ["merger"], "limit": 2, "before": next }))).unwrap();
    assert_eq!(texts(&second.items), vec!["log:merger:merger one"]);
    assert_eq!(second.next, None);
    // The plain "the app" revision's test verdict still goes to the tester.
    let tests = s.agents_feed(&q(json!({ "agents": ["tester"], "kinds": ["verdict"], "limit": 200 }))).unwrap();
    assert!(tests.items.iter().all(|i| i.agent == "tester" && i.field.as_deref() == Some("test")));
    // An agent nobody writes as gives an empty page, not an error.
    assert!(s.agents_feed(&q(json!({ "agents": ["nobody"] }))).unwrap().items.is_empty());
}

#[test]
fn a_verdict_shows_once_not_also_as_its_own_log_line() {
    let (_d, path) = temp_board();
    board(&path, false);
    let db = rusqlite::Connection::open(&path).unwrap();
    let by = "the app (card-tester subagent)";
    // One write: the verdict and its log line, same card, same at, same by.
    rev(&db, "card-1", T0 + MIN, by, json!({ "test": { "state": "passed", "detail": "ran it" } }));
    log(&db, "card-1", T0 + MIN, "Subagent test: PASS", Some(by));
    // A log line of the same agent at another time stays.
    log(&db, "card-1", T0 + 2 * MIN, "Subagent test: started", Some(by));
    // A log line at the same time on another card stays.
    log(&db, "card-2", T0 + MIN, "Subagent test: PASS", Some(by));
    drop(db);
    let s = Store::open(&path).unwrap();
    let page = s.agents_feed(&q(json!({ "agents": ["tester"] }))).unwrap();
    assert_eq!(texts(&page.items), vec!["log:tester:Subagent test: started", "verdict:tester:passed", "log:tester:Subagent test: PASS"]);
    assert_eq!(page.items.iter().filter(|i| i.card == 1).count(), 2);
}
