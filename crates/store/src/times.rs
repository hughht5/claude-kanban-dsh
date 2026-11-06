//! Time in each column (card #111): how long a card spent in each column, and the board's
//! median per column, so Hugh sees where cards wait.
//!
//! The source is `card_revisions`, not the log text: every write to a card (Hugh's drag, a
//! session's move, the app's auto-progress, stack moves, archive and restore, the Done rule)
//! goes through `write_tx`, which records the fields it changed, `column` and `archived`
//! included. Read only: nothing here writes.
//!
//! Rules (each one has a test in `tests/times.rs`):
//! - A card's clock starts at its first revision: made on this board, or imported (time before
//!   an import isn't known, so it isn't counted).
//! - The column a card starts in: the `column` its create revision records (since card #111);
//!   for an older card, the "from X" of a log line that says "from X to Y" within
//!   `LOG_WINDOW_MS` of its first column change to Y; if it never changed column, its column
//!   now. Otherwise that first stretch is unknown and isn't counted (`unknown_ms`). An
//!   `import --replace` revision makes the column unknown again in the same way.
//! - Archived time doesn't count. A restore into the same column carries on the same visit;
//!   only a change of column starts a new one.
//! - Done never collects time (cards from before card #83 sat there unarchived).
//! - Revisions are taken in `seq` order; a time earlier than the one before it counts as no
//!   time at all, so a clock that went backwards never makes a negative stretch.
//! - The board's median per column (decided by the router for Hugh, 6 Nov 2026): visits that
//!   ended in the last `MEDIAN_DAYS` days, dry-run cards left out.

use crate::{Card, Result, Store, COLUMNS};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

/// How close a log line's time must be to the first column change it explains.
pub const LOG_WINDOW_MS: i64 = 5_000;
/// The board's medians count visits that ended this many days before now.
pub const MEDIAN_DAYS: i64 = 30;
const DAY_MS: i64 = 24 * 3600 * 1000;
const DONE: &str = "Done";

/// One revision, as the store keeps it, in `seq` order.
#[derive(Debug, Clone)]
pub struct RevIn {
    pub at: String,
    pub changed: Value,
}

/// One log line.
#[derive(Debug, Clone)]
pub struct LogIn {
    pub at: String,
    pub text: String,
}

/// One stay in a column: from the move in to the move out, archived time left out.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Visit {
    pub column: String,
    /// Time in the column while not archived, in milliseconds.
    pub ms: i64,
    /// When the visit's last counted stretch ended (epoch ms): the move out, the archive, or now.
    pub end: i64,
    /// When the visit's first counted stretch began (epoch ms): the move in (card #120).
    pub start: i64,
    /// The card is in this column now and not archived: the visit is still running.
    pub open: bool,
    #[serde(skip)]
    epoch: u64,
}

/// A card's whole history of visits.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Timeline {
    pub visits: Vec<Visit>,
    /// Time counted nowhere because the column then isn't known (older or imported cards).
    pub unknown_ms: i64,
    /// When the card's clock starts (epoch ms): its first revision.
    pub start: Option<i64>,
    /// The clock starts at an import, so anything before it isn't counted.
    pub imported: bool,
}

/// One row of the panel: a card's total in one column.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnTime {
    pub column: String,
    pub ms: i64,
    pub visits: usize,
    /// The card is in this column now: the panel keeps adding to `ms`.
    pub running: bool,
}

/// The board's median for one column.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMedian {
    pub column: String,
    /// None when no visit to it ended in the window.
    pub ms: Option<i64>,
    pub visits: usize,
    /// The window it covers, in days (`MEDIAN_DAYS`), so the page never states its own.
    pub days: i64,
}

/// What the panel shows for one card.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardTimes {
    pub columns: Vec<ColumnTime>,
    pub unknown_ms: i64,
    pub started_at: Option<i64>,
    pub imported: bool,
    /// The time these were worked out at (epoch ms), so the panel can tick from it.
    pub now: i64,
}

fn parse_ms(at: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(at).ok().map(|d| d.timestamp_millis())
}

/// The board's columns, Done left out: the ones time is counted in.
fn timed_columns() -> impl Iterator<Item = &'static str> {
    COLUMNS.iter().copied().filter(|c| *c != DONE)
}

fn move_line() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let cols = COLUMNS.iter().map(|c| regex::escape(c)).collect::<Vec<_>>().join("|");
        Regex::new(&format!(r"\bfrom ({cols}) to ({cols})\b")).expect("move-line pattern")
    })
}

#[derive(PartialEq)]
enum Kind {
    Created,
    Reset,
    Change,
}

fn kind(changed: &Value) -> Kind {
    if changed.get("created").is_some() {
        Kind::Created
    } else if changed.get("imported").is_some() || changed.get("replaced").is_some() {
        Kind::Reset
    } else {
        Kind::Change
    }
}

fn new_column(changed: &Value) -> Option<&str> {
    changed.get("column").and_then(Value::as_str)
}

fn new_archived(changed: &Value) -> Option<bool> {
    changed.get("archived").map(|v| v.as_bool().unwrap_or(false))
}

/// The column a card was in from revision `i` (a create or an import) until its next column
/// change: see the module's rules.
fn resolve_column(revs: &[RevIn], i: usize, log: &[LogIn], final_col: &str) -> Option<String> {
    for r in &revs[i + 1..] {
        if kind(&r.changed) != Kind::Change {
            return None;
        }
        if let Some(to) = new_column(&r.changed) {
            let at = parse_ms(&r.at)?;
            return log.iter().find_map(|l| {
                let t = parse_ms(&l.at)?;
                if (t - at).abs() > LOG_WINDOW_MS {
                    return None;
                }
                move_line()
                    .captures_iter(&l.text)
                    .find(|c| &c[2] == to && &c[1] != to)
                    .map(|c| c[1].to_string())
            });
        }
    }
    Some(final_col.to_string())
}

/// Whether a card was archived from revision `i` (an older import) until its next archive change.
/// A later `archived: true` means it wasn't. A later `archived: false` means it was only when that
/// revision also clears `archivedAt`, as every real restore does (`archive`, `restore_patch`): a
/// patch that just writes `archived: false` onto a card without the field changes nothing.
fn resolve_archived(revs: &[RevIn], i: usize, final_arch: bool) -> bool {
    for r in &revs[i + 1..] {
        if kind(&r.changed) != Kind::Change {
            return false;
        }
        match new_archived(&r.changed) {
            Some(true) => return false,
            Some(false) => return r.changed.get("archivedAt").is_some(),
            None => {}
        }
    }
    final_arch
}

/// Rebuilds a card's visits from its revisions (in `seq` order) and log, up to `now` (epoch ms).
/// `final_col` and `final_arch` are the card as it is now.
pub fn timeline(revs: &[RevIn], log: &[LogIn], final_col: &str, final_arch: bool, now: i64) -> Timeline {
    let mut out = Timeline::default();
    let mut col: Option<String> = None;
    let mut arch = false;
    // Bumped each time the column changes (or becomes known): a new epoch is a new visit.
    let mut epoch: u64 = 0;
    let mut prev: Option<i64> = None;

    let count = |out: &mut Timeline, col: &Option<String>, arch: bool, epoch: u64, from: i64, to: i64| {
        if arch {
            return;
        }
        let ms = (to - from).max(0);
        let Some(c) = col else {
            out.unknown_ms += ms;
            return;
        };
        if c == DONE {
            return;
        }
        match out.visits.last_mut() {
            Some(v) if v.epoch == epoch => {
                v.ms += ms;
                v.end = to.max(v.end);
            }
            _ => out.visits.push(Visit { column: c.clone(), ms, end: to.max(from), start: from, open: false, epoch }),
        }
    };

    for (i, r) in revs.iter().enumerate() {
        let t = match (parse_ms(&r.at), prev) {
            (Some(t), Some(p)) => t.max(p),
            (Some(t), None) => t,
            (None, Some(p)) => p,
            (None, None) => continue,
        };
        if let Some(p) = prev {
            count(&mut out, &col, arch, epoch, p, t);
        } else {
            out.start = Some(t);
        }
        prev = Some(t);
        match kind(&r.changed) {
            Kind::Created => {
                col = match new_column(&r.changed) {
                    Some(c) => Some(c.to_string()),
                    None => resolve_column(revs, i, log, final_col),
                };
                arch = col.as_deref() == Some(DONE);
                epoch += 1;
            }
            Kind::Reset => {
                out.imported = true;
                // Since card #111 an import records the column and archive state it wrote.
                let c = match new_column(&r.changed) {
                    Some(c) => Some(c.to_string()),
                    None => resolve_column(revs, i, log, final_col),
                };
                if c != col || c.is_none() {
                    epoch += 1;
                }
                col = c;
                arch = match new_archived(&r.changed) {
                    Some(a) => a,
                    None => resolve_archived(revs, i, final_arch),
                };
            }
            Kind::Change => {
                if let Some(c) = new_column(&r.changed) {
                    if col.as_deref() != Some(c) {
                        col = Some(c.to_string());
                        epoch += 1;
                    }
                }
                if let Some(a) = new_archived(&r.changed) {
                    arch = a;
                }
            }
        }
    }
    if let Some(p) = prev {
        count(&mut out, &col, arch, epoch, p, now.max(p));
        if let Some(v) = out.visits.last_mut() {
            v.open = !arch && v.epoch == epoch && col.as_deref() == Some(v.column.as_str());
        }
    }
    out
}

/// When the card entered the column it is in now (epoch ms; card #120): the start of its open
/// visit. None when it isn't in a timed column now (archived, or in Done) or that column isn't
/// known (an older card whose first column can't be worked out).
pub fn column_since(t: &Timeline) -> Option<i64> {
    t.visits.last().filter(|v| v.open).map(|v| v.start)
}

fn iso(ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// A card's total per column, every column but Done, in board order.
pub fn card_columns(t: &Timeline) -> Vec<ColumnTime> {
    timed_columns()
        .map(|c| {
            let vs: Vec<&Visit> = t.visits.iter().filter(|v| v.column == c).collect();
            ColumnTime {
                column: c.to_string(),
                ms: vs.iter().map(|v| v.ms).sum(),
                visits: vs.len(),
                running: vs.iter().any(|v| v.open),
            }
        })
        .collect()
}

/// The median of `xs` (sorted here); the mean of the middle two for an even count.
pub fn median(xs: &mut [i64]) -> Option<i64> {
    if xs.is_empty() {
        return None;
    }
    xs.sort_unstable();
    let n = xs.len();
    Some(if n % 2 == 1 { xs[n / 2] } else { (xs[n / 2 - 1] + xs[n / 2]) / 2 })
}

/// The board's median per column: finished visits that ended in the last `MEDIAN_DAYS` days.
/// The caller leaves dry-run cards out.
pub fn medians<'a>(timelines: impl IntoIterator<Item = &'a Timeline>, now: i64) -> Vec<ColumnMedian> {
    let since = now - MEDIAN_DAYS * DAY_MS;
    let mut by_col: HashMap<&str, Vec<i64>> = HashMap::new();
    for t in timelines {
        for v in &t.visits {
            if !v.open && v.end >= since {
                by_col.entry(v.column.as_str()).or_default().push(v.ms);
            }
        }
    }
    timed_columns()
        .map(|c| {
            let mut xs = by_col.remove(c).unwrap_or_default();
            ColumnMedian { column: c.to_string(), visits: xs.len(), ms: median(&mut xs), days: MEDIAN_DAYS }
        })
        .collect()
}

/// Only the revisions that can change a card's column or archive state, or start its clock.
/// The WHERE is bracketed so a card filter added after it applies to every term.
const REVS_SQL: &str = "SELECT card_id, at, changed FROM card_revisions
    WHERE (json_type(changed, '$.column') IS NOT NULL OR json_type(changed, '$.archived') IS NOT NULL
       OR json_type(changed, '$.created') IS NOT NULL OR json_type(changed, '$.imported') IS NOT NULL
       OR json_type(changed, '$.replaced') IS NOT NULL)";
/// Only log lines that can say where a card came from.
const LOG_SQL: &str = "SELECT card_id, at, text FROM card_log WHERE (text LIKE '%from % to %')";

impl Store {
    /// Runs `base` (card id, time, text; a WHERE already in it), for one card or all, in `seq`
    /// order, and groups the rows by card.
    fn time_rows<T>(&self, base: &str, card: Option<&str>, row: impl Fn(String, String) -> T) -> Result<HashMap<String, Vec<T>>> {
        let sql = match card {
            Some(_) => format!("{base} AND card_id = ?1 ORDER BY seq"),
            None => format!("{base} ORDER BY seq"),
        };
        let mut st = self.conn.prepare(&sql)?;
        let map = |r: &rusqlite::Row| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?));
        let rows = match card {
            Some(id) => st.query_map([id], map)?.collect::<rusqlite::Result<Vec<_>>>()?,
            None => st.query_map([], map)?.collect::<rusqlite::Result<Vec<_>>>()?,
        };
        let mut out: HashMap<String, Vec<T>> = HashMap::new();
        for (id, at, text) in rows {
            out.entry(id).or_default().push(row(at, text));
        }
        Ok(out)
    }

    fn time_revs(&self, card: Option<&str>) -> Result<HashMap<String, Vec<RevIn>>> {
        self.time_rows(REVS_SQL, card, |at, changed| RevIn { at, changed: serde_json::from_str(&changed).unwrap_or(Value::Null) })
    }

    fn time_log(&self, card: Option<&str>) -> Result<HashMap<String, Vec<LogIn>>> {
        self.time_rows(LOG_SQL, card, |at, text| LogIn { at, text })
    }

    /// A card's visits, rebuilt from its revisions and log, up to `now` (epoch ms).
    pub fn card_timeline(&self, id: &str, now: i64) -> Result<Timeline> {
        let card = self.get(id)?;
        let revs = self.time_revs(Some(id))?.remove(id).unwrap_or_default();
        let log = self.time_log(Some(id))?.remove(id).unwrap_or_default();
        Ok(timeline(&revs, &log, &card.column, card.archived, now))
    }

    /// What the panel shows for a card: its time in each column (every column but Done).
    pub fn card_times(&self, id: &str, now: i64) -> Result<CardTimes> {
        let t = self.card_timeline(id, now)?;
        Ok(CardTimes { columns: card_columns(&t), unknown_ms: t.unknown_ms, started_at: t.start, imported: t.imported, now })
    }

    /// The board's median per column (every column but Done): finished visits that ended in the
    /// last `MEDIAN_DAYS` days, archived cards included, dry-run cards left out. Reads the
    /// cards, the revisions and the log once each.
    pub fn column_medians(&self, now: i64) -> Result<Vec<ColumnMedian>> {
        let cards = self.list(true)?;
        let mut revs = self.time_revs(None)?;
        let mut log = self.time_log(None)?;
        let timelines: Vec<Timeline> = cards
            .iter()
            .filter(|c| c.fields.get("dryRun").and_then(Value::as_bool) != Some(true))
            .map(|c| {
                let r = revs.remove(&c.id).unwrap_or_default();
                let l = log.remove(&c.id).unwrap_or_default();
                timeline(&r, &l, &c.column, c.archived, now)
            })
            .collect();
        Ok(medians(&timelines, now))
    }

    /// When each card entered the column it is in now (card #120), as an ISO time, or None when
    /// that isn't known (`column_since`), for every card, archived ones included. Reads the
    /// cards, the revisions and the log once each: no query per card.
    pub fn column_since_all(&self, now: i64) -> Result<HashMap<String, Option<String>>> {
        let cards = self.list(true)?;
        self.column_since_of(&cards, now)
    }

    /// `column_since_all` for cards the caller has already listed: reads the revisions and the
    /// log once each. A card not in `cards` is left out.
    pub fn column_since_of(&self, cards: &[Card], now: i64) -> Result<HashMap<String, Option<String>>> {
        let mut revs = self.time_revs(None)?;
        let mut log = self.time_log(None)?;
        Ok(cards
            .iter()
            .map(|c| {
                let r = revs.remove(&c.id).unwrap_or_default();
                let l = log.remove(&c.id).unwrap_or_default();
                let t = timeline(&r, &l, &c.column, c.archived, now);
                (c.id.clone(), column_since(&t).and_then(iso))
            })
            .collect())
    }

    /// A cheap marker of the board's history: the newest revision's seq and the newest log
    /// line's seq, in one small query. Every column move, archive and import writes one of them
    /// (both tables are append-only), so while it stays the same `column_since_all` does too.
    pub fn history_marker(&self) -> Result<(i64, i64)> {
        Ok(self.conn.query_row(
            "SELECT (SELECT COALESCE(MAX(seq), 0) FROM card_revisions), (SELECT COALESCE(MAX(seq), 0) FROM card_log)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }
}
