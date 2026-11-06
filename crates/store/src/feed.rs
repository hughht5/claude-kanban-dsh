//! The agents' feed (card #120): what the app's agents said and decided, across the board,
//! newest first. Read only: nothing here writes.
//!
//! Two sources, merged in time order:
//! - `card_log` rows whose `by` is an agent's (kind "log");
//! - `card_revisions` rows whose `changed` sets one of `VERDICT_FIELDS` (kind "verdict"). A
//!   revision records each changed field's new value, so the verdict is read from it: route's
//!   `size` and `reason`, stuck's `kind` and `reason`, the others' `state` and `detail`. A field
//!   set to null is "cleared".
//!
//! The agent comes from `by`, never from the text (`agent_of_by`); a verdict written as plain
//! "the app" goes by its field (`agent_of_field`). A row with any other `by` is left out.
//! Every text goes through `mask_secrets`.
//!
//! Order: newest first by (at, kind, seq, sub), where log sorts before verdict at the same time
//! and `sub` numbers the verdicts one revision makes. The cursor (`next`, passed back as
//! `before`) is that key as "at|kind|seq|sub", so paging has no duplicates and no gaps.

use crate::{mask_secrets, Error, Result, Store};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;

pub const FEED_DEFAULT_LIMIT: usize = 50;
pub const FEED_MAX_LIMIT: usize = 200;
/// The longest a verdict's text runs, in characters.
pub const VERDICT_TEXT_MAX: usize = 300;
/// The card fields whose changes are verdicts, in the order a revision's verdicts are numbered.
pub const VERDICT_FIELDS: [&str; 7] = ["route", "critique", "test", "review", "repro", "merge", "stuck"];
/// The plain `by` whose verdicts go by their field.
const APP: &str = "the app";

/// Each `by` an agent writes as, and the agent. The app's own writers are the
/// "the app (<name> subagent)" and "the app (monitor)" strings in `src-tauri` (a test there
/// checks they still map).
const BYS: [(&str, &str); 18] = [
    ("card-tester", "tester"),
    ("the tester", "tester"),
    ("the app (card-tester subagent)", "tester"),
    ("card-reviewer", "reviewer"),
    ("the reviewer", "reviewer"),
    ("the app (card-reviewer subagent)", "reviewer"),
    ("card-critic", "critic"),
    ("the app (card-critic subagent)", "critic"),
    ("card-reproducer", "reproducer"),
    ("the app (card-reproducer subagent)", "reproducer"),
    ("card-merger", "merger"),
    ("the app (card-merger subagent)", "merger"),
    ("card-router", "router"),
    ("the router", "router"),
    ("router", "router"),
    ("the app (card-router subagent)", "router"),
    ("the app (monitor)", "monitor"),
    ("monitor", "monitor"),
];

/// The agent a `by` belongs to, if any.
pub fn agent_of_by(by: &str) -> Option<&'static str> {
    BYS.iter().find(|(b, _)| *b == by).map(|(_, a)| *a)
}

/// The agent that owns a verdict field, for a verdict written as plain "the app".
pub fn agent_of_field(field: &str) -> Option<&'static str> {
    Some(match field {
        "test" => "tester",
        "review" => "reviewer",
        "critique" => "critic",
        "repro" => "reproducer",
        "merge" => "merger",
        "route" => "router",
        "stuck" => "monitor",
        _ => return None,
    })
}

/// What the page asks for. Every filter is optional.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FeedQuery {
    /// Card numbers (0 is the router's system card).
    pub cards: Option<Vec<i64>>,
    pub agents: Option<Vec<String>>,
    /// "log" and/or "verdict".
    pub kinds: Option<Vec<String>>,
    /// An ISO time: only items at or after it.
    pub since: Option<String>,
    /// A `next` cursor from an earlier page: only items older than it.
    pub before: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FeedItem {
    /// The row's seq in its table (`card_log` or `card_revisions`).
    pub seq: i64,
    pub at: String,
    pub card: i64,
    pub agent: String,
    /// "log" or "verdict".
    pub kind: String,
    /// The new state (verdicts only).
    pub verdict: Option<String>,
    /// The card field the verdict is in (verdicts only).
    pub field: Option<String>,
    pub text: String,
    #[serde(skip)]
    sub: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FeedPage {
    pub items: Vec<FeedItem>,
    /// Pass as `before` for the next, older page; None when there is none.
    pub next: Option<String>,
}

/// The sort key: (at, kind rank, seq, sub). Larger is newer.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    at: String,
    rank: i64,
    seq: i64,
    sub: i64,
}

fn rank(kind: &str) -> i64 {
    if kind == "log" { 0 } else { 1 }
}

fn key_of(i: &FeedItem) -> Key {
    Key { at: i.at.clone(), rank: rank(&i.kind), seq: i.seq, sub: i.sub }
}

fn cursor_of(i: &FeedItem) -> String {
    format!("{}|{}|{}|{}", i.at, i.kind, i.seq, i.sub)
}

fn parse_cursor(s: &str) -> Result<Key> {
    let bad = || Error::Invalid(format!("\"{s}\" isn't a feed cursor"));
    let parts: Vec<&str> = s.split('|').collect();
    if parts.len() != 4 || !matches!(parts[1], "log" | "verdict") {
        return Err(bad());
    }
    Ok(Key {
        at: parts[0].to_string(),
        rank: rank(parts[1]),
        seq: parts[2].parse().map_err(|_| bad())?,
        sub: parts[3].parse().map_err(|_| bad())?,
    })
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

/// A verdict's state and text from a field's new value.
fn verdict_of(field: &str, v: &Value) -> (String, String) {
    if v.is_null() {
        return ("cleared".into(), String::new());
    }
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let (state, text) = match field {
        "route" => (s("size"), s("reason")),
        "stuck" => (s("kind"), s("reason")),
        _ => (s("state"), s("detail")),
    };
    (if state.is_empty() { "unknown".into() } else { state }, clip(&mask_secrets(&text), VERDICT_TEXT_MAX))
}

/// One SQL row: seq, at, card number, by, and the text or the changed JSON.
struct Row {
    seq: i64,
    at: String,
    card: i64,
    by: Option<String>,
    body: String,
}

/// A string as an SQL literal.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// How many rows each batch reads.
const BATCH: usize = 256;

impl Store {
    /// The agents' feed: see the module.
    pub fn agents_feed(&self, q: &FeedQuery) -> Result<FeedPage> {
        let limit = q.limit.unwrap_or(FEED_DEFAULT_LIMIT).clamp(1, FEED_MAX_LIMIT);
        let since = match q.since.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => Some(
                chrono::DateTime::parse_from_rfc3339(s)
                    .map_err(|_| Error::Invalid(format!("since \"{s}\" isn't an ISO time")))?
                    .with_timezone(&chrono::Utc)
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            ),
            None => None,
        };
        let before = q.before.as_deref().filter(|s| !s.is_empty()).map(parse_cursor).transpose()?;
        let wants = |k: &str| q.kinds.as_ref().map_or(true, |ks| ks.iter().any(|x| x == k));
        let keep = |i: &FeedItem| q.agents.as_ref().map_or(true, |a| a.iter().any(|x| x == &i.agent));
        let mut items = Vec::new();
        if wants("log") {
            items.extend(self.feed_source(false, q, since.as_deref(), before.as_ref(), limit + 1, &keep)?);
        }
        if wants("verdict") {
            items.extend(self.feed_source(true, q, since.as_deref(), before.as_ref(), limit + 1, &keep)?);
        }
        // The Router's own log (card #119, final-test F4): its decisions, the events it was given
        // (with their card), its starts and stops, and Hugh's messages to it.
        if wants("log") {
            items.extend(self.router_feed_source(q, since.as_deref(), before.as_ref(), limit + 1, &keep)?);
        }
        items.sort_by(|a, b| key_of(b).cmp(&key_of(a)));
        dedupe_router_lines(&mut items);
        dedupe_verdict_echoes(&mut items);
        let next = if items.len() > limit {
            items.truncate(limit);
            items.last().map(cursor_of)
        } else {
            None
        };
        Ok(FeedPage { items, next })
    }

    /// Up to `want` items from one source, newest first, older than `before`.
    fn feed_source(&self, revs: bool, q: &FeedQuery, since: Option<&str>, before: Option<&Key>, want: usize, keep: &dyn Fn(&FeedItem) -> bool) -> Result<Vec<FeedItem>> {
        // The agents asked for, as the `by` strings they write as: the filter is in SQL, so a rare
        // agent's page reads only its own rows. A verdict written as plain "the app" goes by its
        // field, so those rows are narrowed to the asked-for agents' fields here, and `keep`
        // checks each item exactly (one revision can set several fields).
        let asked = |a: &str| q.agents.as_ref().map_or(true, |ws| ws.iter().any(|w| w == a));
        let mut bys: Vec<&str> = BYS.iter().filter(|(_, a)| asked(a)).map(|(b, _)| *b).collect();
        let app_fields: Vec<&str> = if revs { VERDICT_FIELDS.iter().copied().filter(|f| agent_of_field(f).map_or(false, |a| asked(a))).collect() } else { vec![] };
        if !app_fields.is_empty() {
            bys.push(APP);
        }
        if bys.is_empty() {
            return Ok(vec![]);
        }
        let has = |fs: &[&str]| fs.iter().map(|f| format!("json_type(t.changed, '$.{f}') IS NOT NULL")).collect::<Vec<_>>().join(" OR ");
        let (table, body, extra) = if revs {
            let any = has(&VERDICT_FIELDS);
            let app = if app_fields.is_empty() { "0".to_string() } else { has(&app_fields) };
            let q_app = quote(APP);
            ("card_revisions", "t.changed", format!("CASE WHEN json_valid(t.changed) THEN (CASE WHEN t.by = {q_app} THEN ({app}) ELSE ({any}) END) ELSE 0 END"))
        } else {
            // A verdict's own log line (the same write: same card, same `at`, same `by`) is left
            // out, so each verdict shows once, as its "verdict" item (final test, card #120).
            let any = VERDICT_FIELDS.iter().map(|f| format!("json_type(r.changed, '$.{f}') IS NOT NULL")).collect::<Vec<_>>().join(" OR ");
            ("card_log", "t.text", format!("NOT EXISTS (SELECT 1 FROM card_revisions r WHERE r.card_id = t.card_id AND r.at = t.at AND r.by IS t.by AND json_valid(r.changed) AND ({any}))"))
        };
        let by_list = bys.iter().map(|b| quote(b)).collect::<Vec<_>>().join(", ");
        let cards = match &q.cards {
            Some(ns) if ns.is_empty() => return Ok(vec![]),
            Some(ns) => format!(" AND c.number IN ({})", ns.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ")),
            None => String::new(),
        };
        let sql = format!(
            "SELECT t.seq, t.at, c.number, t.by, {body} FROM {table} t JOIN cards c ON c.id = t.card_id
             WHERE t.by IN ({by_list}) AND ({extra}){cards}
               AND (?1 IS NULL OR t.at >= ?1)
               AND (t.at, t.seq) <= (?2, ?3)
             ORDER BY t.at DESC, t.seq DESC LIMIT {BATCH}"
        );
        let mut st = self.conn.prepare(&sql)?;
        // The SQL bound is inclusive and loose; `before` is applied exactly below.
        // At the cursor's own time, a log cursor leaves out every verdict and a verdict cursor
        // takes every log row (log sorts first); rows of its own kind go by seq.
        let own = rank(if revs { "verdict" } else { "log" });
        let (mut at, mut seq) = match before {
            Some(k) if k.rank == own => (k.at.clone(), k.seq),
            Some(k) if own < k.rank => (k.at.clone(), i64::MAX),
            Some(k) => (k.at.clone(), i64::MIN),
            None => ("\u{10FFFF}".to_string(), i64::MAX),
        };
        let mut out = Vec::new();
        loop {
            let rows: Vec<Row> = st
                .query_map(rusqlite::params![since, at, seq], |r| {
                    Ok(Row { seq: r.get(0)?, at: r.get(1)?, card: r.get(2)?, by: r.get(3)?, body: r.get(4)? })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let n = rows.len();
            for row in &rows {
                for item in feed_items(revs, row) {
                    if before.map_or(true, |b| key_of(&item).cmp(b) == Ordering::Less) && keep(&item) {
                        out.push(item);
                    }
                }
            }
            if out.len() >= want || n < BATCH {
                break;
            }
            let last = rows.last().expect("a full batch");
            at = last.at.clone();
            seq = last.seq - 1;
        }
        Ok(out)
    }
}

/// The feed's items from one row (none when its `by` maps to no agent).
fn feed_items(revs: bool, row: &Row) -> Vec<FeedItem> {
    let by = row.by.as_deref().unwrap_or("");
    let by_agent = agent_of_by(by);
    if !revs {
        return by_agent
            .map(|a| FeedItem { seq: row.seq, at: row.at.clone(), card: row.card, agent: a.into(), kind: "log".into(), verdict: None, field: None, text: mask_secrets(&row.body), sub: 0 })
            .into_iter()
            .collect();
    }
    let Ok(changed) = serde_json::from_str::<Value>(&row.body) else { return vec![] };
    VERDICT_FIELDS
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            let v = changed.get(*f)?;
            let agent = by_agent.or_else(|| if by == APP { agent_of_field(f) } else { None })?;
            let (verdict, text) = verdict_of(f, v);
            Some(FeedItem { seq: row.seq, at: row.at.clone(), card: row.card, agent: agent.into(), kind: "verdict".into(), verdict: Some(verdict), field: Some((*f).into()), text, sub: i as i64 })
        })
        .collect()
}

/// The agent of a Router log line: Hugh's own messages are "hugh", a card session's message is
/// that session's (its `by`, "card #n's session"), the rest the Router's.
pub fn router_log_agent(by: &str) -> String {
    if by.starts_with("Hugh") {
        "hugh".into()
    } else if by.starts_with("card #") {
        by.into()
    } else {
        "router".into()
    }
}

/// A Router log item's `sub`: apart from a `card_log` row with the same seq and time.
const ROUTER_SUB: i64 = 1_000;

impl Store {
    /// Up to `want` items of the Router's log, newest first, older than `before` (card #119).
    /// `router_log` is written in time order, so its seq (the primary key) orders it: each batch
    /// reads the next `BATCH` rows below the last seq, and filters them, until `want` are found
    /// or the log runs out.
    fn router_feed_source(&self, q: &FeedQuery, since: Option<&str>, before: Option<&Key>, want: usize, keep: &dyn Fn(&FeedItem) -> bool) -> Result<Vec<FeedItem>> {
        let cards: Option<&Vec<i64>> = q.cards.as_ref();
        if cards.is_some_and(|c| c.is_empty()) {
            return Ok(vec![]);
        }
        let mut st = self.conn.prepare("SELECT seq, at, COALESCE(card, 0), by, text FROM router_log WHERE seq < ?1 ORDER BY seq DESC LIMIT ?2")?;
        let mut below = i64::MAX;
        let mut out = Vec::new();
        'batches: loop {
            let rows: Vec<(i64, String, i64, String, String)> = st
                .query_map(rusqlite::params![below, BATCH as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let n = rows.len();
            for (seq, at, card, by, text) in rows {
                below = seq;
                if since.is_some_and(|s| at.as_str() < s) {
                    break 'batches;
                }
                if cards.is_some_and(|c| !c.contains(&card)) {
                    continue;
                }
                let item = FeedItem { seq, at, card, agent: router_log_agent(&by), kind: "log".into(), verdict: None, field: None, text: mask_secrets(&text), sub: ROUTER_SUB };
                if before.map_or(true, |b| key_of(&item).cmp(b) == Ordering::Less) && keep(&item) {
                    out.push(item);
                    if out.len() >= want {
                        break 'batches;
                    }
                }
            }
            if n < BATCH {
                break;
            }
        }
        Ok(out)
    }
}

/// How close in time a Router log line and the same card log line count as one write.
const SAME_LINE_SECS: i64 = 120;

/// A line the Router wrote both on the card and in its own log shows once, as the card's
/// (re-test note, card #119): a Router log item is dropped when a Router card-log item on the
/// same card has the same text within `SAME_LINE_SECS`.
pub fn dedupe_router_lines(items: &mut Vec<FeedItem>) {
    let card_lines: Vec<(i64, String, String)> = items
        .iter()
        .filter(|i| i.sub != ROUTER_SUB && i.kind == "log" && i.agent == "router")
        .map(|i| (i.card, i.text.trim().to_string(), i.at.clone()))
        .collect();
    let secs = |a: &str, b: &str| {
        let p = |s: &str| chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp());
        match (p(a), p(b)) {
            (Some(x), Some(y)) => (x - y).abs(),
            _ => i64::MAX,
        }
    };
    items.retain(|i| {
        i.sub != ROUTER_SUB || !card_lines.iter().any(|(c, t, at)| *c == i.card && *t == i.text.trim() && secs(at, &i.at) <= SAME_LINE_SECS)
    });
}

/// How close the agent's own log line is to its verdict when it is the same write (a moment
/// apart), and how long after a route the Router's own echo of it may come.
const SAME_WRITE_SECS: i64 = 5;
const ECHO_SECS: i64 = 120;

/// One routing decision shows once, as its route verdict row (re-test 2, card #119): a
/// "router" log line on the same card that echoes it ("Routed: …" from the card-router, written a
/// moment apart, or the Router's own "route recorded …") within `ECHO_SECS` is dropped.
pub fn dedupe_verdict_echoes(items: &mut Vec<FeedItem>) {
    let ts = |s: &str| chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp());
    let verdicts: Vec<(i64, String, Option<String>, i64)> = items
        .iter()
        .filter(|i| i.kind == "verdict")
        .filter_map(|i| Some((i.card, i.agent.clone(), i.field.clone(), ts(&i.at)?)))
        .collect();
    items.retain(|i| {
        if i.kind != "log" {
            return true;
        }
        let Some(at) = ts(&i.at) else { return true };
        let low = i.text.to_ascii_lowercase();
        !verdicts.iter().any(|(card, agent, field, vat)| {
            let route = field.as_deref() == Some("route");
            // The card-router and the Router both show as "router": for a route only its echoes
            // go, by their words.
            let echo = low.starts_with("routed") || low.contains("route recorded") || low.contains("route:");
            *card == i.card
                && if route {
                    i.agent == "router" && echo && at - vat >= -SAME_WRITE_SECS && at - vat <= ECHO_SECS
                } else {
                    // Other verdicts keep their agents' lines (a nudge, a test started): the SQL
                    // above already drops a verdict's own line of the same write.
                    let _ = agent;
                    false
                }
        })
    });
}
