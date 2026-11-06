//! Cards that wait on others (card #109). A card's `blockedBy: [12, 40]` names the cards it waits
//! on. The store cleans and checks it on every write: no card waits on itself, on a missing or
//! archived card, on a card that is ready already (outside a stack), or in a circle; a stack's
//! lead and its own child never wait on each other; at most `MAX_WAITS`.
//!
//! The app's 2-second poll (`Store::settle_waits`) acts once every wait of a card is met:
//! - Release: a card in Blocked that Hugh moved there himself, with no blocked reason, outside a
//!   stack (and on the test channel, a dry-run card) is moved to Planning for him, which starts it.
//!   Only his own drag out of Backlog decides that a card starts (router decision, critic #1).
//! - Tell Hugh: any other card in Backlog or Blocked stays where it is, and he is told.
//! - Tell the session: a card in Planning or later stays where it is, and its session is told.
//!
//! The waits are then cleared, as a new revision (nothing is removed). A card that was started
//! or told keeps them in `waitedOn`, so it hears once if one of them goes back. A stack's
//! children are left to the stack (card #113).

use crate::cards::{apply, get_tx, log_tx, write_tx, DONE, HUGH};
use crate::ships::ship_lane;
use crate::stacks::by_number_tx;
use crate::{now, Card, Error, Patch, Result, Store};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};

/// The card field naming the cards it waits on.
pub const BLOCKED_KEY: &str = "blockedBy";
/// The card field the app writes when it clears met waits from a card it started or told: the
/// cards it waited on, watched in case one goes back.
pub const WAITED_KEY: &str = "waitedOn";
/// Who the app's writes for waits are recorded as.
pub const WAIT_BY: &str = "the app (Blocked by)";
/// Most cards one card waits on.
pub const MAX_WAITS: usize = 20;
/// A blocker in the waiting card's own repo is ready from Human test on: built and tested by
/// Hugh's rules, though not on main until it ships (router decision 3).
pub const MET_SAME_REPO: [&str; 4] = ["Human test", "Ship it", "Deploy complete", DONE];
/// A blocker in another repo is ready only once shipped.
pub const MET_OTHER_REPO: [&str; 2] = ["Deploy complete", DONE];

/// The cards the poll looks at: waiting or watching, not archived, not a stack's child (the
/// stack's rules start those). The partial index `cards_waiting` (schema12.sql) has the same
/// condition in brackets, so the poll's empty case reads the index only.
const WAITING: &str = "archived = 0 AND (json_extract(fields,'$.blockedBy') IS NOT NULL OR json_extract(fields,'$.waitedOn') IS NOT NULL) AND json_extract(fields,'$.stackOf') IS NULL";

fn split_into(out: &mut Vec<String>, s: &str) {
    out.extend(s.split(|c: char| c == ',' || c.is_whitespace()).filter(|p| !p.is_empty()).map(str::to_string));
}

fn clean_numbers(key: &str, v: &Value) -> Result<Option<Value>> {
    let bad = |what: &str| Error::Invalid(format!("{key} is a list of card numbers, like [12, 40] (or null for none); {what} isn't a card number"));
    let mut raw = Vec::new();
    match v {
        Value::Null => return Ok(None),
        Value::Number(_) => raw.push(v.to_string()),
        Value::String(s) => split_into(&mut raw, s),
        Value::Array(a) => {
            for x in a {
                match x {
                    Value::Number(_) => raw.push(x.to_string()),
                    Value::String(s) => split_into(&mut raw, s),
                    other => return Err(bad(&other.to_string())),
                }
            }
        }
        other => return Err(bad(&other.to_string())),
    }
    let mut out: Vec<i64> = Vec::new();
    for p in raw {
        let n = p.trim_start_matches('#').parse::<i64>().ok().filter(|n| *n > 0).ok_or_else(|| bad(&format!("\"{p}\"")))?;
        if !out.contains(&n) {
            out.push(n);
        }
    }
    if out.len() > MAX_WAITS {
        return Err(Error::Invalid(format!("a card waits on at most {MAX_WAITS} cards")));
    }
    Ok(if out.is_empty() { None } else { Some(json!(out)) })
}

/// Cleans a card's `blockedBy`: card numbers as a list (`[12, 40]`, `["#12"]`), one number, or
/// one string (`"12, #40"`, `"#12 #40"`, as `kb update --blockedBy` sends). Repeats are kept once.
/// `null`, `""` or `[]` means none (`None`). More than `MAX_WAITS` is refused.
pub fn clean_blocked_by(v: &Value) -> Result<Option<Value>> {
    clean_numbers(BLOCKED_KEY, v)
}

/// The same cleaning for `waitedOn`.
pub(crate) fn clean_waited_on(v: &Value) -> Result<Option<Value>> {
    clean_numbers(WAITED_KEY, v)
}

/// The numbers in a stored list. A value that isn't a clean list of card numbers (written by an
/// older build or by hand) counts as none, so the poll ignores it.
fn numbers(v: Option<&Value>) -> Vec<i64> {
    let Some(a) = v.and_then(Value::as_array) else { return Vec::new() };
    a.iter().map(|x| x.as_i64().filter(|n| *n > 0)).collect::<Option<Vec<_>>>().unwrap_or_default()
}

impl Card {
    /// The cards this card waits on (`blockedBy`).
    pub fn blocked_by(&self) -> Vec<i64> {
        numbers(self.fields.get(BLOCKED_KEY))
    }
    /// The cards it waited on before the app started or told it (`waitedOn`).
    pub fn waited_on(&self) -> Vec<i64> {
        numbers(self.fields.get(WAITED_KEY))
    }
    fn plan_waited_on(&self) -> Vec<i64> {
        numbers(self.fields.get("plan").and_then(|p| p.get(WAITED_KEY)))
    }
}

/// Is `waiter`'s wait on `blocker` met? In the same repo (the `repo` fields match ignoring case,
/// or both are empty), from Human test on; in another repo, only once shipped (Deploy complete).
/// An archived blocker counts only when archived in Deploy complete or Done.
pub fn is_met(waiter: &Card, blocker: &Card) -> bool {
    let col = blocker.column.as_str();
    if blocker.archived {
        return MET_OTHER_REPO.contains(&col);
    }
    if ship_lane(waiter) == ship_lane(blocker) {
        MET_SAME_REPO.contains(&col)
    } else {
        MET_OTHER_REPO.contains(&col)
    }
}

/// The shortest chain of waits from card `from` to card `to` over `edges` (each `(a, b)` is "a
/// waits on b"), both ends included, if there is one.
pub fn wait_path(edges: &[(i64, i64)], from: i64, to: i64) -> Option<Vec<i64>> {
    let mut next: HashMap<i64, Vec<i64>> = HashMap::new();
    for (a, b) in edges {
        next.entry(*a).or_default().push(*b);
    }
    let mut prev: HashMap<i64, i64> = HashMap::new();
    let mut seen = HashSet::from([from]);
    let mut queue = VecDeque::from([from]);
    while let Some(n) = queue.pop_front() {
        if n == to {
            let mut path = vec![n];
            let mut at = n;
            while let Some(p) = prev.get(&at) {
                path.push(*p);
                at = *p;
            }
            path.reverse();
            return Some(path);
        }
        for m in next.get(&n).into_iter().flatten() {
            if seen.insert(*m) {
                prev.insert(*m, n);
                queue.push_back(*m);
            }
        }
    }
    None
}

fn chain(ns: &[i64]) -> String {
    ns.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(" → ")
}

/// The cards `card` watches once its waits `ns` are cleared: its `waitedOn` plus `ns`.
pub(crate) fn watch_with(card: &Card, ns: &[i64]) -> Vec<i64> {
    let mut watch = card.waited_on();
    for n in ns {
        if !watch.contains(n) {
            watch.push(*n);
        }
    }
    watch
}

pub(crate) fn nums(ns: &[i64]) -> String {
    ns.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(", ")
}

/// Every wait on the board, as (waiting card, card it waits on). `archived` includes archived
/// cards' waits, as the write-time check does, so restoring a card never closes a circle.
fn edges(conn: &Connection, archived: bool) -> Result<Vec<(i64, i64)>> {
    let sql = format!(
        "SELECT number, json_extract(fields, '$.blockedBy') FROM cards WHERE json_extract(fields, '$.blockedBy') IS NOT NULL{}",
        if archived { "" } else { " AND archived = 0" }
    );
    let mut st = conn.prepare(&sql)?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (n, list) = row?;
        let v: Option<Value> = list.and_then(|s| serde_json::from_str(&s).ok());
        for b in numbers(v.as_ref()) {
            out.push((n, b));
        }
    }
    Ok(out)
}

/// The checks of a write that changes `card`'s waits from `old` to its own `blockedBy`. Only the
/// newly added numbers are checked, so a wait that was fine when it was set never blocks a later
/// write; taking waits off is always allowed.
pub(crate) fn check_waits(conn: &Connection, card: &Card, old: &[i64]) -> Result<()> {
    let me = card.number;
    let added: Vec<i64> = card.blocked_by().into_iter().filter(|n| !old.contains(n)).collect();
    if added.is_empty() {
        return Ok(());
    }
    let others: Vec<(i64, i64)> = edges(conn, true)?.into_iter().filter(|(a, _)| *a != me).collect();
    for n in added {
        if n == me {
            return Err(Error::Invalid("a card can't wait on itself".into()));
        }
        let b = match by_number_tx(conn, n) {
            Ok(b) => b,
            Err(Error::NotFound(_)) => return Err(Error::Invalid(format!("there's no card #{n}"))),
            Err(e) => return Err(e),
        };
        if b.archived {
            return Err(Error::Invalid(format!("#{n} is archived: restore it first, or leave it out")));
        }
        if card.stack_of() == Some(n) || b.stack_of() == Some(me) || card.stack().contains(&n) || b.stack().contains(&me) {
            return Err(Error::Invalid(format!(
                "#{me} and #{n} are a stack's lead and its own child: the stack's rules order them, so they can't wait on each other"
            )));
        }
        let siblings = card.stack_of().is_some() && card.stack_of() == b.stack_of();
        if !siblings && is_met(card, &b) {
            return Err(Error::Invalid(format!("#{n} is ready already (it's in {}): nothing to wait for", b.column)));
        }
        if let Some(path) = wait_path(&others, n, me) {
            let mut round = vec![me];
            round.extend(path);
            return Err(Error::Invalid(format!(
                "#{me} can't wait on #{n}: that makes a circle ({}, each waiting on the next), so none of them would ever start",
                chain(&round)
            )));
        }
    }
    Ok(())
}

/// Refuses a join that would put a card and a card it waits on (either way) into one stack as
/// its lead and its child (`stacks::join_check`).
pub(crate) fn check_join(lead: &Card, children: &[&Card]) -> Result<()> {
    for c in children {
        let (l, n) = (lead.number, c.number);
        if lead.blocked_by().contains(&n) || c.blocked_by().contains(&l) {
            let (w, on) = if lead.blocked_by().contains(&n) { (l, n) } else { (n, l) };
            return Err(Error::Invalid(format!(
                "#{w} waits on #{on}, and a stack's lead and its own child can't wait on each other: take the wait off #{w} first"
            )));
        }
    }
    Ok(())
}

/// The log line for a change of waits by someone other than the app's poll.
pub(crate) fn change_line(old: &[i64], new: &[i64]) -> Option<String> {
    let added: Vec<i64> = new.iter().copied().filter(|n| !old.contains(n)).collect();
    let gone: Vec<i64> = old.iter().copied().filter(|n| !new.contains(n)).collect();
    let mut parts = Vec::new();
    if !added.is_empty() {
        parts.push(format!("Now waits on {}", nums(&added)));
    }
    if !gone.is_empty() {
        parts.push(format!("No longer waits on {}", nums(&gone)));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

/// One wait, for the monitor (#99) and the stack rules (#113).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Wait {
    /// The waiting card's number and id, and its column.
    pub card: i64,
    pub card_id: String,
    pub column: String,
    /// The card it waits on: its column (`None` when there is no such card) and whether it is archived.
    pub on: i64,
    pub on_column: Option<String>,
    pub on_archived: bool,
    /// By `is_met`. For a stack's child waiting on a sibling, the stack's rules decide its start (#113).
    pub met: bool,
    /// The stack the waiting card is a child of (its lead's number), if any.
    pub stack: Option<i64>,
    /// The wait is part of a circle of waits among cards that aren't archived (one written by
    /// an older build, which didn't check).
    pub in_cycle: bool,
}

/// What the poll did to one card, for the app to pass on once it has let go of the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum WaitKind {
    /// Moved from Blocked to Planning: the app runs its usual after-move (the session starts).
    Released,
    /// Not moved: Hugh gets a notification with `text`.
    TellHugh,
    /// Not moved: its session (if it has one) gets `text`.
    TellSession,
    /// A card it waited on went back: its session (if it has one) gets `text`.
    WentBack,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WaitOutcome {
    pub id: String,
    pub number: i64,
    pub kind: WaitKind,
    pub text: String,
}

/// What a stack's lead started by its waits is told, and logged: only the lead starts (router
/// decision on the whole-build review, N2). Its cards start by the stack's rules.
pub const LEAD_NOTE: &str = "Only this card was started: the cards in its stack start when Hugh moves this card on himself (the stack's rules), not because its waits were met.";

fn branch_of(c: &Card) -> String {
    match c.fields.get("branch").and_then(Value::as_str).map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => format!("branch {b}"),
        None => "no branch named on the card".into(),
    }
}

/// One line per card it waited on: where it is, and for one not shipped yet its branch.
fn blocker_lines(waiter: &Card, blockers: &[Card]) -> Vec<String> {
    blockers
        .iter()
        .map(|b| {
            let shipped = MET_OTHER_REPO.contains(&b.column.as_str());
            let at = if b.archived { format!("archived in {}", b.column) } else { format!("in {}", b.column) };
            if shipped {
                format!("- #{} \"{}\": {at}, shipped.", b.number, b.title())
            } else if ship_lane(waiter) == ship_lane(b) {
                format!("- #{} \"{}\": {at} on {}. It is not on main yet: build on that branch if you need its work, or wait for it to ship.", b.number, b.title(), branch_of(b))
            } else {
                format!("- #{} \"{}\": {at} on {}.", b.number, b.title(), branch_of(b))
            }
        })
        .collect()
}

fn where_short(blockers: &[Card]) -> String {
    blockers.iter().map(|b| format!("#{} in {}", b.number, b.column)).collect::<Vec<_>>().join(", ")
}

impl Store {
    /// Is any card for the poll waiting, or watching cards it waited on? The poll's empty case:
    /// one query on the `cards_waiting` index.
    pub fn any_waits(&self) -> Result<bool> {
        Ok(self.conn.query_row(&format!("SELECT 1 FROM cards WHERE {WAITING} LIMIT 1"), [], |_| Ok(())).optional()?.is_some())
    }

    /// The query plan of `any_waits`, for the test that it reads the index only.
    #[doc(hidden)]
    pub fn any_waits_plan(&self) -> Result<Vec<String>> {
        let mut st = self.conn.prepare(&format!("EXPLAIN QUERY PLAN SELECT 1 FROM cards WHERE {WAITING} LIMIT 1"))?;
        let rows = st.query_map([], |r| r.get::<_, String>(3))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn waiting_cards(&self) -> Result<Vec<Card>> {
        let mut st = self.conn.prepare(&format!("SELECT id FROM cards WHERE {WAITING} ORDER BY number"))?;
        let ids = st.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        drop(st);
        ids.iter().map(|id| self.get(id)).collect()
    }

    /// Every wait of every card that isn't archived (stack children included), in card order.
    pub fn wait_edges(&self) -> Result<Vec<Wait>> {
        let live = edges(&self.conn, false)?;
        let mut out = Vec::new();
        for c in self.list(false)? {
            for on in c.blocked_by() {
                let b = self.get_by_number(on).ok();
                out.push(Wait {
                    card: c.number,
                    card_id: c.id.clone(),
                    column: c.column.clone(),
                    on,
                    on_column: b.as_ref().map(|b| b.column.clone()),
                    on_archived: b.as_ref().map_or(false, |b| b.archived),
                    met: b.as_ref().map_or(false, |b| is_met(&c, b)),
                    stack: c.stack_of(),
                    in_cycle: wait_path(&live, on, c.number).is_some(),
                });
            }
        }
        Ok(out)
    }

    /// Each circle of waits among cards that aren't archived, once, starting at its lowest
    /// number: `[12, 40]` is #12 waits on #40, which waits on #12.
    pub fn wait_cycles(&self) -> Result<Vec<Vec<i64>>> {
        let live = edges(&self.conn, false)?;
        let mut seen: HashSet<Vec<i64>> = HashSet::new();
        let mut out = Vec::new();
        let mut sorted = live.clone();
        sorted.sort();
        for (a, b) in sorted {
            let Some(mut path) = wait_path(&live, b, a) else { continue };
            path.insert(0, a);
            path.pop();
            let low = path.iter().enumerate().min_by_key(|(_, n)| **n).map(|(i, _)| i).unwrap_or(0);
            path.rotate_left(low);
            if seen.insert(path.clone()) {
                out.push(path);
            }
        }
        Ok(out)
    }

    /// The column change before now: who made the card's latest column change (from its
    /// revisions), or None when it never changed column.
    fn last_column_by(&self, id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT by FROM card_revisions WHERE card_id = ?1 AND json_extract(changed, '$.column') IS NOT NULL ORDER BY seq DESC LIMIT 1",
                [id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// What a session moved to Planning by its waits is told about the cards it waited on: each
    /// one's column and, for one not on main yet, its branch. None for any other card.
    pub fn wait_note(&self, card: &Card) -> Option<String> {
        let ns = card.plan_waited_on();
        if ns.is_empty() || card.column != "Planning" {
            return None;
        }
        let blockers: Vec<Card> = ns.iter().filter_map(|n| self.get_by_number(*n).ok()).collect();
        let mut lines = vec![format!(
            "Hugh had moved this card to Blocked to start once the cards it waited on ({}) were ready, and they are now:",
            nums(&ns)
        )];
        lines.extend(blocker_lines(card, &blockers));
        if card.is_lead() {
            lines.push(LEAD_NOTE.to_string());
        }
        Some(lines.join("\n"))
    }

    /// Writes `patch` to the waiting card at the version read, with the app's log line if any.
    fn put_waits(&mut self, w: &Card, patch: &Patch, line: Option<&str>) -> Result<Card> {
        let tx = self.begin()?;
        let mut card = get_tx(&tx, &w.id)?.ok_or_else(|| Error::NotFound(w.id.clone()))?;
        if card.version != w.version {
            return Err(Error::Conflict { id: w.id.clone(), yours: w.version, current: card.version });
        }
        let changed = apply(&mut card, patch)?;
        if !changed.is_empty() {
            write_tx(&tx, &mut card, w.version, &changed, WAIT_BY)?;
        }
        if let Some(line) = line {
            log_tx(&tx, &card.id, &now(), line, Some(WAIT_BY))?;
        }
        tx.commit()?;
        Ok(card)
    }

    /// The app's 2-second poll (`dispatch::pump_waits`): acts on each card whose waits are all
    /// met, on each card a card it waited on went back from, and on waits that can never be met
    /// (see the module's notes). `test` is the test channel, where only dry-run cards are moved.
    /// A card that changed while the poll looked, or that hit a database error, is left for the
    /// next poll; what was done for the other cards is still returned, for the app to pass on.
    pub fn settle_waits(&mut self, test: bool) -> Result<Vec<WaitOutcome>> {
        let mut out = Vec::new();
        if !self.any_waits()? {
            return Ok(out);
        }
        for w in self.waiting_cards()? {
            if w.stack_of().is_some() {
                continue;
            }
            let _ = self.settle_one(w, test, &mut out);
        }
        Ok(out)
    }

    fn settle_one(&mut self, mut w: Card, test: bool, out: &mut Vec<WaitOutcome>) -> Result<()> {
        let not_started = matches!(w.column.as_str(), "Backlog" | "Blocked");
        // The cards it waited on: one no longer ready is said once (a card not started yet waits
        // on it again); one shipped is on main, so there is nothing left to watch.
        let watched = w.waited_on();
        if !watched.is_empty() {
            let (mut back, mut keep, mut done) = (Vec::new(), Vec::new(), false);
            for n in watched {
                match self.get_by_number(n) {
                    Ok(b) if !is_met(&w, &b) => back.push(b),
                    Ok(b) if MET_OTHER_REPO.contains(&b.column.as_str()) => done = true,
                    Ok(_) => keep.push(n),
                    Err(Error::NotFound(_)) => done = true,
                    Err(e) => return Err(e),
                }
            }
            let mut p = Patch::new();
            p.insert(WAITED_KEY.into(), if keep.is_empty() { Value::Null } else { json!(keep) });
            if back.is_empty() {
                if done {
                    w = self.put_waits(&w, &p, None)?;
                }
            } else {
                let names = back
                    .iter()
                    .map(|b| if b.archived { format!("#{} was archived in {} without shipping", b.number, b.column) } else { format!("#{} went back to {}", b.number, b.column) })
                    .collect::<Vec<_>>()
                    .join(", ");
                let again: Vec<i64> = back.iter().filter(|b| !b.archived).map(|b| b.number).collect();
                let mut list = w.blocked_by();
                list.extend(again.iter().filter(|n| !w.blocked_by().contains(n)));
                if not_started && !again.is_empty() {
                    p.insert(BLOCKED_KEY.into(), json!(list));
                    let line = format!("{names}, so it waits on {} again", nums(&again));
                    // A wait that can't be put back (a circle since, say) is only said.
                    w = match self.put_waits(&w, &p, Some(&line)) {
                        Ok(c) => c,
                        Err(Error::Invalid(_)) => {
                            p.remove(BLOCKED_KEY);
                            self.put_waits(&w, &p, Some(&format!("{names}, after the cards it waited on were ready")))?
                        }
                        Err(e) => return Err(e),
                    };
                    out.push(WaitOutcome {
                        id: w.id.clone(),
                        number: w.number,
                        kind: WaitKind::TellHugh,
                        text: format!("#{} {}: {names}. Don't start it yet: it waits on {} again.", w.number, w.title(), nums(&again)),
                    });
                } else {
                    let line = if not_started {
                        format!("{names}, after the cards it waited on were ready")
                    } else {
                        format!("{names}, after this card started on it: its work may change")
                    };
                    w = self.put_waits(&w, &p, Some(&line))?;
                    let detail = back.iter().map(|b| format!("- #{} \"{}\": now in {} ({}).", b.number, b.title(), b.column, branch_of(b))).collect::<Vec<_>>().join("\n");
                    out.push(if not_started {
                        WaitOutcome { id: w.id.clone(), number: w.number, kind: WaitKind::TellHugh, text: format!("#{} {}: {names}. Check whether it still needs that work before you start it.", w.number, w.title()) }
                    } else {
                        WaitOutcome {
                            id: w.id.clone(),
                            number: w.number,
                            kind: WaitKind::WentBack,
                            text: format!("A card this card waited on went back, so its work may change. Check its log and branch before you build on it further:\n{detail}"),
                        }
                    });
                }
            }
        }
        let ns = w.blocked_by();
        if ns.is_empty() {
            return Ok(());
        }
        // Waits that can never be met (archived without shipping, or no such card) come off, and
        // Hugh is told, so the card isn't stuck for good.
        let (mut blockers, mut lost, mut why) = (Vec::new(), Vec::new(), Vec::new());
        for &n in &ns {
            match self.get_by_number(n) {
                Ok(b) if b.archived && !is_met(&w, &b) => {
                    why.push(format!("#{n} was archived in {} without shipping", b.column));
                    lost.push(n);
                }
                Ok(b) => blockers.push(b),
                Err(Error::NotFound(_)) => {
                    why.push(format!("there is no card #{n}"));
                    lost.push(n);
                }
                Err(e) => return Err(e),
            }
        }
        if !lost.is_empty() {
            let rest: Vec<i64> = ns.iter().copied().filter(|n| !lost.contains(n)).collect();
            let mut p = Patch::new();
            p.insert(BLOCKED_KEY.into(), if rest.is_empty() { Value::Null } else { json!(rest) });
            let why = why.join("; ");
            w = self.put_waits(&w, &p, Some(&format!("No longer waits on {}: {why}. Hugh was told", nums(&lost))))?;
            out.push(WaitOutcome {
                id: w.id.clone(),
                number: w.number,
                kind: WaitKind::TellHugh,
                text: format!("#{} {}: {why}, so it no longer waits on it. Check whether it still needs that work.", w.number, w.title()),
            });
            return Ok(());
        }
        if !blockers.iter().all(|b| is_met(&w, b)) {
            return Ok(());
        }
        let ready = where_short(&blockers);
        let watch = watch_with(&w, &ns);
        if not_started {
            let reason = w.fields.get("blockedReason").and_then(Value::as_str).unwrap_or("").trim().to_string();
            let dry = w.fields.get("dryRun").and_then(Value::as_bool) == Some(true);
            let mut why_not = if w.column == "Backlog" {
                Some("it is in Backlog, and only Hugh starts a card from there (drag it to Blocked to have it start by itself)".to_string())
            } else if !reason.is_empty() {
                Some("it has a blocked reason".to_string())
            } else if self.last_column_by(&w.id)?.as_deref() != Some(HUGH) {
                Some("Hugh didn't move it to Blocked himself".to_string())
            } else if test && !dry {
                Some("in the test app only dry-run cards start by themselves".to_string())
            } else {
                None
            };
            if why_not.is_none() {
                match self.move_after_waits(&w.id, w.version, &ns) {
                    Ok(m) => {
                        if m.card.is_lead() {
                            self.add_log(&m.card.id, LEAD_NOTE, WAIT_BY)?;
                        }
                        out.push(WaitOutcome {
                            id: m.card.id.clone(),
                            number: m.card.number,
                            kind: WaitKind::Released,
                            text: format!("The cards it waited on are ready ({ready}): moved to Planning"),
                        });
                        return Ok(());
                    }
                    // Refused for good (not a passing conflict): Hugh is told once, rather than
                    // the poll trying again every 2 seconds.
                    Err(Error::Invalid(e)) => {
                        why_not = Some(format!("the app couldn't move it ({e})"));
                        w = self.get(&w.id)?;
                    }
                    Err(e) => return Err(e),
                }
            }
            let why = why_not.unwrap_or_default();
            let mut p = Patch::new();
            p.insert(BLOCKED_KEY.into(), Value::Null);
            p.insert(WAITED_KEY.into(), json!(watch));
            let w = self.put_waits(&w, &p, Some(&format!("The cards it waited on are ready ({ready}). Not moved: {why}. Hugh was told")))?;
            out.push(WaitOutcome {
                id: w.id.clone(),
                number: w.number,
                kind: WaitKind::TellHugh,
                text: format!("#{} {}: the cards it waited on are ready ({ready}). Drag it to Planning to start it.", w.number, w.title()),
            });
        } else {
            let mut p = Patch::new();
            p.insert(BLOCKED_KEY.into(), Value::Null);
            p.insert(WAITED_KEY.into(), json!(watch));
            let w = self.put_waits(&w, &p, Some(&format!("The cards it waited on are ready ({ready})")))?;
            let mut lines = vec![format!("The cards this card waited on ({}) are ready:", nums(&ns))];
            lines.extend(blocker_lines(&w, &blockers));
            out.push(WaitOutcome { id: w.id.clone(), number: w.number, kind: WaitKind::TellSession, text: lines.join("\n") });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_circles() {
        let e = [(1, 2), (2, 3), (3, 1), (4, 1)];
        assert_eq!(wait_path(&e, 1, 3), Some(vec![1, 2, 3]));
        assert_eq!(wait_path(&e, 4, 3), Some(vec![4, 1, 2, 3]));
        assert_eq!(wait_path(&e, 1, 4), None);
        assert_eq!(wait_path(&e, 5, 5), Some(vec![5]));
    }

    #[test]
    fn change_lines() {
        assert_eq!(change_line(&[], &[12, 40]).as_deref(), Some("Now waits on #12, #40"));
        assert_eq!(change_line(&[12, 40], &[40]).as_deref(), Some("No longer waits on #12"));
        assert_eq!(change_line(&[12], &[40]).as_deref(), Some("Now waits on #40; No longer waits on #12"));
        assert_eq!(change_line(&[12], &[12]), None);
    }
}
