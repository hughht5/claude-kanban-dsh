//! Stacked cards (design/stacked-cards/SPEC.md): one lead card that manages a set of child cards.
//!
//! - The lead lists its children in order (`stack: [n, …]`); each child names its lead
//!   (`stackOf: n`). A card is in at most one stack, and a lead is never itself a child.
//! - `passed` on a child is Hugh's "Passed" in its row; it is still in Human test until it ships.
//! - The lead's column is the stack's column. It moves to Human test once its children are
//!   ready (Human test or later), by the rule in `stacks.human_test_when`: "all" (the default) or
//!   "first". It goes back to In progress when a child goes back and the rule no longer holds.
//! - From Deploy complete on, the children follow the lead (they shipped with it).
//! - Archiving the lead archives its children (`archivedWith`); restoring it restores them.
//! - Hugh's drop can join a built card or stack to a stack in Ship it, or take a card out of Ship
//!   it into a stack, while neither ship has started, so they ship as one (card #89,
//!   `stack_join`). A stack is one repo.
//! - Each child builds on its own branch, and the lead merges it into the stack's branch (card
//!   #113). A child in Human test counts as ready, and can be passed, only once it is merged
//!   (`merged`). A child with no branch of its own was built the old way, by the lead, so it
//!   counts as merged.
//!
//! The fields live in `fields` like any other; only these functions write `stack` and `stackOf`.

use crate::cards::{apply, get_tx, log_tx, write_tx};
use crate::ships::{repo_name, ship_lane, ship_started};
use crate::{now, Card, Error, Patch, Result, Store};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};

/// Setting: when the stack moves to Human test. "all" or "first".
pub const STACK_RULE_KEY: &str = "stacks.human_test_when";
/// Fields only the stack functions may write.
pub const STACK_FIELDS: [&str; 3] = ["stack", "stackOf", "archivedWith"];
/// A child in one of these columns is ready for Hugh (or past him).
pub const READY: [&str; 4] = ["Human test", "Ship it", "Deploy complete", "Done"];
/// Lead columns the rule may move a stack out of, into Human test.
const BUILDING: [&str; 3] = ["Planning", "In progress", "Blocked"];
/// The lead's own field naming what holds the stack back from Human test (card #96), so the
/// reason is logged once, not on every recheck.
pub const STACK_HELD_KEY: &str = "stackHeld";

/// The state of a card's verdict field (`test`, `review`, `merge`): `{state, ...}`.
fn state_of<'a>(card: &'a Card, key: &str) -> Option<&'a str> {
    card.fields.get(key).and_then(|t| t.get("state")).and_then(Value::as_str)
}

/// What still holds a lead back from Human test (card #96): its own subagent test or review,
/// each while Hugh's rule for it is on, until it has passed. A capped one (`capped: true`, its
/// third FAIL) holds it whatever a later verdict says, until Hugh moves the lead into In progress.
/// `rules` is (field, what it is, on). While both the test and the review are on, a pass that
/// started before the other's latest FAIL doesn't count either (card #162, `pair.rs`).
pub fn lead_missing(lead: &Card, rules: &[(&str, &str, bool)]) -> Vec<String> {
    let on = |k: &str| rules.iter().any(|(key, _, on)| *key == k && *on);
    let stale = if on(crate::TEST_KEY) && on(crate::REVIEW_KEY) { crate::stale_passes(lead) } else { vec![] };
    rules
        .iter()
        .filter(|(_, _, on)| *on)
        .filter_map(|(key, what, _)| {
            let rec = lead.fields.get(*key);
            if rec.and_then(|t| t.get("capped")).and_then(Value::as_bool) == Some(true) {
                Some(format!("{what} (failed 3 times; Hugh moves it on)"))
            } else if rec.and_then(|t| t.get("state")).and_then(Value::as_str) != Some("passed") {
                Some(what.to_string())
            } else {
                stale.iter().find(|s| s.key == *key).map(|s| format!("{what} ({})", s.short()))
            }
        })
        .collect()
}

/// `lead_missing` for a lead with its children `kids`. The review counts as passed when every
/// live child's review has passed (card #113): a lead's own commits are only the merges of
/// reviewed work. Old-way children, with no review of their own, still need the lead's.
pub fn lead_missing_in_stack(lead: &Card, kids: &[Card], rules: &[(&str, &str, bool)]) -> Vec<String> {
    let kids_reviewed = children_reviewed(kids);
    let rules: Vec<(&str, &str, bool)> = rules.iter().copied().filter(|(key, _, _)| !(kids_reviewed && *key == crate::REVIEW_KEY)).collect();
    lead_missing(lead, &rules)
}

/// Do a lead's children's reviews stand in for its own (card #113)? Only in a stack whose
/// children all build themselves (a lead that builds work of its own still has it reviewed,
/// card #96): every live child builds itself and its review passed.
pub fn children_reviewed(kids: &[Card]) -> bool {
    let live: Vec<&Card> = kids.iter().filter(|k| !k.archived).collect();
    !live.is_empty() && live.iter().all(|k| own_build(k) && state_of(k, crate::REVIEW_KEY) == Some("passed"))
}

/// A card's own branch, trimmed ("" when it has none).
fn branch_of(card: &Card) -> &str {
    card.fields.get("branch").and_then(Value::as_str).map(str::trim).unwrap_or("")
}

/// The app's mark on a stack child that builds itself in its own session (card #113). Set
/// when the child's own session starts; sessions can't write it.
pub const OWN_BUILD_KEY: &str = "stackOwnBuild";

/// Does `child` build itself (its own session started while in a stack), rather than being
/// built by its lead the old way?
pub fn own_build(child: &Card) -> bool {
    child.fields.get(OWN_BUILD_KEY).and_then(Value::as_bool) == Some(true)
}

/// Is `child` in the stack's branch (card #113)? A child that builds itself is merged only once
/// the merger's record on it says passed, whatever its `branch` says. A child its lead built
/// the old way (no session of its own) with no branch of its own (none, or the lead's) is in
/// the stack's branch already.
pub fn merged(child: &Card, lead: &Card) -> bool {
    if state_of(child, "merge") == Some("passed") {
        return true;
    }
    if own_build(child) {
        return false;
    }
    let b = branch_of(child);
    b.is_empty() || b == branch_of(lead)
}

/// Where a child is in its stack (card #113), for the board and the rules: "waiting" (on a card
/// not yet merged, as `waiting` says), "building", "to-merge" (in Human test, not merged yet),
/// "merged", "passed" (Hugh's Passed, once merged) or "shipped".
pub fn stack_stage(child: &Card, lead: &Card, waiting: bool) -> &'static str {
    match child.column.as_str() {
        "Ship it" | "Deploy complete" | "Done" => "shipped",
        "Human test" if !merged(child, lead) => "to-merge",
        "Human test" if child.passed() => "passed",
        "Human test" => "merged",
        "Backlog" | "Planning" | "Blocked" if waiting => "waiting",
        _ => "building",
    }
}

/// A move the stack rules made, for the app to pass on (a message to the lead's session, or
/// stopping a child's session when it went to Done).
#[derive(Debug, Clone, PartialEq)]
pub struct StackMove {
    pub id: String,
    pub number: i64,
    pub from: String,
    pub to: String,
    pub lead: bool,
}

impl Card {
    /// The lead's number, if this card is in a stack.
    pub fn stack_of(&self) -> Option<i64> {
        self.fields.get("stackOf").and_then(Value::as_i64)
    }
    /// The children's numbers, in order (empty for a card that isn't a lead).
    pub fn stack(&self) -> Vec<i64> {
        self.fields
            .get("stack")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_i64).collect())
            .unwrap_or_default()
    }
    pub fn is_lead(&self) -> bool {
        !self.stack().is_empty()
    }
    pub fn passed(&self) -> bool {
        self.fields.get("passed").and_then(Value::as_bool) == Some(true)
    }
}

pub(crate) fn by_number_tx(c: &Connection, n: i64) -> Result<Card> {
    let id: Option<String> = c.query_row("SELECT id FROM cards WHERE number = ?1", [n], |r| r.get(0)).optional()?;
    let id = id.ok_or_else(|| Error::NotFound(format!("#{n}")))?;
    get_tx(c, &id)?.ok_or_else(|| Error::NotFound(format!("#{n}")))
}

/// What `Store::stack_join` did besides the join itself.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Joined {
    /// Cards taken out of Ship it, their own ship request cancelled: they ship with the stack.
    pub unshipped: Vec<i64>,
    /// The cards of a joining stack that came with its lead, in order.
    pub came_with: Vec<i64>,
}

/// A stack can join another from these columns, and a card can leave them into a stack in
/// Ship it: built and tested, not yet shipped.
const BUILT: [&str; 2] = ["Human test", "Ship it"];

fn nums(ns: &[i64]) -> String {
    ns.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(", ")
}

/// The ship of a card that left Ship it to ship with stack `lead`. `with` names the stack, for
/// the app (`dispatch::left_ship_for_stack`); `detail` says it for Hugh.
fn cancelled(card: &Card, at: &str, lead: i64) -> Value {
    let retries = card.fields.get("ship").and_then(|s| s.get("retries")).and_then(Value::as_i64).unwrap_or(0);
    json!({ "state": "cancelled", "cancelledAt": at, "retries": retries, "with": lead, "detail": format!("Ships with stack #{lead}") })
}

/// The checks of a join (`Store::stack_join`), with nothing written: the lead, the child, and
/// the cards that come with the child when it leads a stack of its own.
fn join_check(conn: &Connection, lead: i64, child: i64, hugh: bool) -> Result<(Card, Card, Vec<Card>)> {
    if lead == child {
        return Err(Error::Invalid("a card can't be in its own stack".into()));
    }
    let l = by_number_tx(conn, lead)?;
    let c = by_number_tx(conn, child)?;
    if l.archived || c.archived {
        return Err(Error::Invalid("an archived card can't join a stack; restore it first".into()));
    }
    if let Some(n) = l.stack_of() {
        return Err(Error::Invalid(format!("#{lead} is in stack #{n}, so it can't lead one")));
    }
    if let Some(n) = c.stack_of() {
        return Err(Error::Invalid(if n == lead {
            format!("#{child} is already in stack #{lead}")
        } else {
            format!("#{child} is already in stack #{n}; a card is in at most one stack")
        }));
    }
    let (lr, cr) = (ship_lane(&l), ship_lane(&c));
    if !lr.is_empty() && !cr.is_empty() && lr != cr {
        return Err(Error::Invalid(format!(
            "#{child} is for {} and #{lead} is for {}: a stack is one branch in one repo",
            repo_name(&c),
            repo_name(&l)
        )));
    }
    // A card that waits on the lead, or that the lead waits on, can't become its child (card #109).
    crate::waits::check_join(&l, &[&c])?;
    let mut kids = Vec::new();
    if c.is_lead() {
        if !hugh {
            return Err(Error::Invalid(format!("#{child} leads a stack of its own, so it can't join another")));
        }
        if !BUILT.contains(&c.column.as_str()) {
            return Err(Error::Invalid(format!(
                "#{child} leads a stack that is still being built (it's in {}): a whole stack can join another once it is in Human test or Ship it",
                c.column
            )));
        }
        for n in c.stack() {
            if let Ok(k) = by_number_tx(conn, n) {
                kids.push(k);
            }
        }
        crate::waits::check_join(&l, &kids.iter().collect::<Vec<_>>())?;
    }
    if c.column == "Ship it" {
        if !hugh {
            return Err(Error::Invalid(format!("#{child} is in Ship it; add it once its ship is over")));
        }
        if let Some(why) = ship_started(&c) {
            return Err(Error::Invalid(format!(
                "#{child} can't leave Ship it for a stack now ({why}): its merge may be done already. Let its ship finish first"
            )));
        }
    }
    if l.column == "Ship it" {
        if !hugh {
            return Err(Error::Invalid(format!(
                "#{lead} is in Ship it, and a card that joins it ships with it: only Hugh adds one, by dropping it on #{lead} in the app"
            )));
        }
        if let Some(why) = ship_started(&l) {
            return Err(Error::Invalid(format!(
                "#{lead}'s ship has started ({why}), so #{child} can't join that push: move #{child} to Ship it to ship it straight after"
            )));
        }
        // Only built, unshipped cards: in Human test (or waiting in Ship it themselves).
        for k in std::iter::once(&c).chain(kids.iter()).filter(|k| !k.archived) {
            if !BUILT.contains(&k.column.as_str()) {
                return Err(Error::Invalid(if READY.contains(&k.column.as_str()) {
                    format!("#{} has shipped already (it's in {}), so it can't join stack #{lead}'s ship", k.number, k.column)
                } else {
                    format!(
                        "#{} isn't built yet (it's in {}), and stack #{lead} is in Ship it, so it ships now: build it first, then drop it on #{lead} once it is in Human test",
                        k.number, k.column
                    )
                }));
            }
        }
    }
    Ok((l, c, kids))
}

/// Marks a card Hugh approved to ship (it was in Ship it, or joins a stack there) as passed.
fn approve(p: &mut Patch, card: &Card, on: bool, at: &str, lines: &mut Vec<String>, lead: i64, into_ship: bool) {
    if !on || card.passed() {
        return;
    }
    p.insert("passed".into(), json!(true));
    p.insert("passedAt".into(), json!(at));
    lines.push(if into_ship {
        format!("Passed: Hugh dropped it on #{lead} in Ship it, which is his approval to ship it with the stack")
    } else {
        "Passed: Hugh had moved it to Ship it, which is his approval to ship it".to_string()
    });
}

/// `put` with several log lines.
fn put_lines(tx: &rusqlite::Transaction, card: &mut Card, patch: &Patch, by: &str, lines: &[String], at: &str) -> Result<()> {
    put(tx, card, patch, by, None)?;
    for t in lines {
        log_tx(tx, &card.id, at, t, Some(by))?;
    }
    Ok(())
}

/// Writes `patch` to `card` inside `tx` (no version check beyond the row's own), with a log line.
fn put(tx: &rusqlite::Transaction, card: &mut Card, patch: &Patch, by: &str, log: Option<&str>) -> Result<()> {
    let v = card.version;
    let changed = apply(card, patch)?;
    if !changed.is_empty() {
        write_tx(tx, card, v, &changed, by)?;
    }
    if let Some(t) = log {
        log_tx(tx, &card.id, &now(), t, Some(by))?;
    }
    Ok(())
}

/// Is `lead`'s stack ready for Hugh, by `rule`? Archived children don't count. A child in
/// Human test is ready only once it is merged into the stack's branch (card #113).
pub fn stack_ready(lead: &Card, children: &[Card], rule: &str) -> bool {
    let live: Vec<&Card> = children.iter().filter(|c| !c.archived).collect();
    if live.is_empty() {
        return false;
    }
    let ready = |c: &&Card| READY.contains(&c.column.as_str()) && (c.column != "Human test" || merged(c, lead));
    if rule == "first" {
        live.iter().any(ready)
    } else {
        live.iter().all(ready)
    }
}

/// Refuses a direct write of the stack's own fields (`update`, `create`).
pub(crate) fn check_no_stack_fields(patch: &Patch) -> Result<()> {
    if let Some(k) = patch.keys().find(|k| STACK_FIELDS.contains(&k.as_str())) {
        return Err(Error::Invalid(format!(
            "\"{k}\" is set by the stack commands (stack add / stack remove), not by an update"
        )));
    }
    Ok(())
}

impl Store {
    /// "all" or "first".
    pub fn stack_rule(&self) -> String {
        match self.setting(STACK_RULE_KEY).ok().flatten().as_deref() {
            Some("first") => "first".into(),
            _ => "all".into(),
        }
    }

    /// The lead's children, in its order. A number no longer on the board is skipped.
    pub fn stack_children(&self, lead: &Card) -> Result<Vec<Card>> {
        let mut out = Vec::new();
        for n in lead.stack() {
            match self.get_by_number(n) {
                Ok(c) => out.push(c),
                Err(Error::NotFound(_)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }

    /// The lead of the stack `card` is in, if any.
    pub fn stack_lead(&self, card: &Card) -> Result<Option<Card>> {
        match card.stack_of() {
            Some(n) => Ok(Some(self.get_by_number(n)?)),
            None => Ok(None),
        }
    }

    /// Would `stack_join` take `child` into `lead`'s stack? Its checks, with nothing written: the
    /// board asks before it shows Hugh what his drop will do (card #89).
    pub fn stack_join_check(&self, lead: i64, child: i64, hugh: bool) -> Result<()> {
        join_check(&self.conn, lead, child, hugh).map(|_| ())
    }

    /// Puts card `child` into the stack led by `lead` (at the end), making `lead` a stack if it
    /// isn't one yet: a session's add, by the rules before card #89 (no card in Ship it, no
    /// stack into a stack, and nothing into a stack in Ship it). Returns the lead.
    pub fn stack_add(&mut self, lead: i64, child: i64, by: &str) -> Result<Card> {
        self.stack_join(lead, child, by, false).map(|(l, _)| l)
    }

    /// Puts card `child` into the stack led by `lead` (at the end), making `lead` a stack if it
    /// isn't one yet, in one transaction. Returns the lead and what else it did.
    ///
    /// `hugh` is Hugh's own drop on the board (card #89), which may also:
    /// - add a built card (Human test, or Ship it before its ship started) to a stack in Ship it
    ///   whose ship hasn't started: it ships with the stack, as one merge;
    /// - take a card out of Ship it (before its ship started) into a stack: its own ship request
    ///   is cancelled and it ships with the stack;
    /// - put a whole stack (in Human test, or Ship it before its ship started) into another:
    ///   its lead, then its cards, join in order.
    ///
    /// A card that leaves Ship it this way, or joins a stack in Ship it, waits in Human test
    /// marked Passed: Hugh approved it to ship. A stack that takes a waiting card keeps the
    /// earlier of the two places in the ship queue. A card for another repo never joins.
    pub fn stack_join(&mut self, lead: i64, child: i64, by: &str, hugh: bool) -> Result<(Card, Joined)> {
        let tx = self.begin()?;
        let (mut l, mut c, mut kids) = join_check(&tx, lead, child, hugh)?;
        let into_ship = l.column == "Ship it";
        let from_ship = c.column == "Ship it";
        let approved = into_ship || from_ship;
        let mut joined = Joined::default();
        let at = now();

        // The lead: its new list, and the earlier place in the queue.
        let mut list = l.stack();
        list.push(child);
        for k in &kids {
            if !list.contains(&k.number) {
                list.push(k.number);
                joined.came_with.push(k.number);
            }
        }
        let mut p = Patch::new();
        p.insert("stack".into(), json!(list));
        let mut place = None;
        if let Some(t) = crate::ships::earlier_place(&l, &c) {
            let mut ship = l.fields.get("ship").and_then(Value::as_object).cloned().unwrap_or_default();
            ship.insert("requestedAt".into(), t);
            p.insert("ship".into(), Value::Object(ship));
            place = Some(format!("The stack takes #{child}'s earlier place in the ship queue"));
        }
        let text = if joined.came_with.is_empty() {
            format!("Added #{child} to this stack")
        } else {
            format!("Added stack #{child} to this stack, with its cards {}", nums(&joined.came_with))
        };
        put(&tx, &mut l, &p, by, Some(&text))?;
        if let Some(t) = place {
            log_tx(&tx, &l.id, &at, &t, Some(by))?;
        }

        // The child, and the cards that come with it.
        let mut p = Patch::new();
        p.insert("stackOf".into(), json!(lead));
        if c.is_lead() {
            p.insert("stack".into(), Value::Null);
        }
        let mut lines = vec![if c.is_lead() {
            format!("Joined stack #{lead}, with the cards of its own stack: it no longer leads one")
        } else {
            format!("Joined stack #{lead}")
        }];
        if from_ship {
            p.insert("column".into(), json!("Human test"));
            p.insert("shipRequestedAt".into(), Value::Null);
            p.insert("ship".into(), cancelled(&c, &at, lead));
            joined.unshipped.push(child);
            lines.push(format!("Ship request cancelled: it ships with stack #{lead}. Moved from Ship it to Human test, where a stack's cards wait until it ships"));
        }
        approve(&mut p, &c, approved, &at, &mut lines, lead, into_ship);
        put_lines(&tx, &mut c, &p, by, &lines, &at)?;
        for k in kids.iter_mut() {
            let mut p = Patch::new();
            p.insert("stackOf".into(), json!(lead));
            // A card archived with its old stack isn't the new lead's to restore.
            if k.fields.get("archivedWith").and_then(Value::as_i64) == Some(child) {
                p.insert("archivedWith".into(), Value::Null);
            }
            // A card that left Ship it for its old stack ships with this one now.
            if let Some(ship) = k.fields.get("ship").and_then(Value::as_object) {
                if ship.get("with").and_then(Value::as_i64) == Some(child) {
                    let mut ship = ship.clone();
                    ship.insert("with".into(), json!(lead));
                    ship.insert("detail".into(), json!(format!("Ships with stack #{lead}")));
                    p.insert("ship".into(), Value::Object(ship));
                }
            }
            let mut lines = vec![format!("Moved with stack #{child} into stack #{lead}")];
            approve(&mut p, k, approved && k.column == "Human test", &at, &mut lines, lead, into_ship);
            put_lines(&tx, k, &p, by, &lines, &at)?;
        }
        tx.commit()?;
        Ok((l, joined))
    }

    /// Takes `child` out of `lead`'s stack: it goes back on the board as an ordinary card in its
    /// own column. A lead left with no children is an ordinary card again. Returns the lead.
    pub fn stack_remove(&mut self, lead: i64, child: i64, by: &str) -> Result<Card> {
        let tx = self.begin()?;
        let mut l = by_number_tx(&tx, lead)?;
        let mut c = by_number_tx(&tx, child)?;
        let list = l.stack();
        if !list.contains(&child) && c.stack_of() != Some(lead) {
            return Err(Error::Invalid(format!("#{child} isn't in stack #{lead}")));
        }
        let rest: Vec<i64> = list.into_iter().filter(|n| *n != child).collect();
        let mut p = Patch::new();
        p.insert("stack".into(), if rest.is_empty() { Value::Null } else { json!(rest) });
        put(&tx, &mut l, &p, by, Some(&format!("Took #{child} out of this stack")))?;
        let mut p = Patch::new();
        p.insert("stackOf".into(), Value::Null);
        p.insert("archivedWith".into(), Value::Null);
        put(&tx, &mut c, &p, by, Some(&format!("Left stack #{lead}: it is an ordinary card again")))?;
        tx.commit()?;
        Ok(l)
    }

    /// Creates a card straight into `lead`'s stack.
    pub fn create_in_stack(&mut self, fields: Patch, lead: i64, by: &str) -> Result<Card> {
        let l = self.get_by_number(lead)?;
        if l.archived {
            return Err(Error::Invalid(format!("stack #{lead} is archived")));
        }
        if let Some(n) = l.stack_of() {
            return Err(Error::Invalid(format!("#{lead} is in stack #{n}, so it can't lead one")));
        }
        // What stack_add would refuse, checked first: a refused add mustn't leave the new card behind.
        if l.column == "Ship it" {
            return Err(Error::Invalid(format!(
                "#{lead} is in Ship it, and a card that joins it ships with it: only Hugh adds one, by dropping it on #{lead} in the app"
            )));
        }
        // Its waits (card #109) are checked as the stack's card it will be, so a sibling can be
        // waited on whatever its column, and are set once it has joined.
        let mut fields = fields;
        let waits = match fields.remove(crate::BLOCKED_KEY) {
            Some(v) => crate::clean_blocked_by(&v)?,
            None => None,
        };
        if let Some(w) = &waits {
            let mut probe_fields = fields.clone();
            probe_fields.insert("stackOf".into(), json!(lead));
            probe_fields.insert(crate::BLOCKED_KEY.into(), w.clone());
            let probe = Card {
                id: String::new(),
                number: self.next_number()?,
                column: fields.get("column").and_then(Value::as_str).unwrap_or("Backlog").to_string(),
                order: None,
                order_col: None,
                archived: false,
                fields: probe_fields,
                version: 0,
            };
            crate::waits::check_waits(&self.conn, &probe, &[])?;
        }
        let repo = fields.get("repo").and_then(Value::as_str).unwrap_or("").trim().to_lowercase();
        if !repo.is_empty() && !ship_lane(&l).is_empty() && repo != ship_lane(&l) {
            return Err(Error::Invalid(format!("#{lead} is for {}: a stack is one branch in one repo", repo_name(&l))));
        }
        let card = self.create(None, fields, by)?;
        self.stack_add(lead, card.number, by)?;
        if let Some(w) = waits {
            let c = self.get(&card.id)?;
            let mut p = Patch::new();
            p.insert(crate::BLOCKED_KEY.into(), w);
            self.update(&card.id, c.version, &p, by)?;
        }
        self.get(&card.id)
    }

    /// Hugh's Passed (or taking it back). A stack's child is passed only once it is merged into
    /// the stack's branch (card #113): what Hugh tests is the stack's build. Taking it back is
    /// always allowed.
    pub fn set_passed(&mut self, id: &str, on: bool, by: &str) -> Result<Card> {
        let tx = self.begin()?;
        let mut c = get_tx(&tx, id)?.ok_or_else(|| Error::NotFound(id.to_string()))?;
        if let (true, Some(n)) = (on, c.stack_of()) {
            if let Ok(lead) = by_number_tx(&tx, n) {
                if !merged(&c, &lead) {
                    return Err(Error::Invalid(format!(
                        "#{} isn't merged into the stack's branch yet, so it can't be passed: the stack's lead merges it first",
                        c.number
                    )));
                }
            }
        }
        if c.passed() == on {
            return Ok(c);
        }
        let mut p = Patch::new();
        p.insert("passed".into(), if on { json!(true) } else { Value::Null });
        if on {
            p.insert("passedAt".into(), json!(now()));
        } else {
            p.insert("passedAt".into(), Value::Null);
        }
        let text = if on { format!("Passed: {by} tested it") } else { format!("Passed taken back by {by}") };
        put(&tx, &mut c, &p, by, Some(&text))?;
        tx.commit()?;
        Ok(c)
    }

    /// After `card_id` moved (by anyone): applies the stack rules and returns the moves they made.
    /// A child's move can move its lead (to Human test, or back to In progress). A lead that
    /// reached Deploy complete or Done takes its children with it.
    pub fn stack_after_move(&mut self, card_id: &str, by: &str) -> Result<Vec<StackMove>> {
        let card = self.get(card_id)?;
        let mut moves = Vec::new();
        if let Some(lead_n) = card.stack_of() {
            // A child that went back has work the stack's branch doesn't hold yet (card #113):
            // its merge record is cleared before the rules read it, and the lead merges it again.
            if !READY.contains(&card.column.as_str()) && card.fields.get("merge").map_or(false, |m| !m.is_null()) {
                let tx = self.begin()?;
                let mut c = get_tx(&tx, card_id)?.ok_or_else(|| Error::NotFound(card_id.to_string()))?;
                let mut p = Patch::new();
                p.insert("merge".into(), Value::Null);
                let line = format!("Merge record cleared: it went back to {}, so the stack's lead merges it again once it passes", c.column);
                put(&tx, &mut c, &p, by, Some(&line))?;
                tx.commit()?;
            }
            moves = self.stack_recheck(lead_n, &format!("#{} moved", card.number), by)?;
        } else if card.is_lead() && (card.column == "Deploy complete" || card.column == "Done") {
            let tx = self.begin()?;
            for n in card.stack() {
                let Ok(mut k) = by_number_tx(&tx, n) else { continue };
                if k.archived || k.column == card.column || (k.column == "Done" && card.column == "Deploy complete") {
                    continue;
                }
                let from = k.column.clone();
                let mut p = Patch::new();
                p.insert("column".into(), json!(card.column));
                put(&tx, &mut k, &p, by, Some(&format!("Moved from {from} to {} with its stack #{}", card.column, card.number)))?;
                moves.push(StackMove { id: k.id.clone(), number: k.number, from, to: card.column.clone(), lead: false });
            }
            tx.commit()?;
        }
        Ok(moves)
    }

    /// Applies the column rule to stack `lead_n` (after a child moved, joined or left): the
    /// stack moves to Human test once ready, or back to In progress when it no longer is.
    /// `cause` goes in the log line ("#45 moved").
    pub fn stack_recheck(&mut self, lead_n: i64, cause: &str, by: &str) -> Result<Vec<StackMove>> {
        let mut moves = Vec::new();
        let rule = self.stack_rule();
        // The lead's subagents (card #151): its own choice, else the board's default, read before
        // the transaction with the one rule every gate uses.
        let on = self.get_by_number(lead_n).ok().map(|l| (self.subagent_on(&l, "tester"), self.subagent_on(&l, "reviewer"), self.subagent_on(&l, "reproducer")));
        let (test_on, review_on, repro_on) = on.unwrap_or((true, true, true));
        let tx = self.begin()?;
        let Ok(mut lead) = by_number_tx(&tx, lead_n) else { return Ok(moves) };
        if lead.archived {
            return Ok(moves);
        }
        let mut kids = Vec::new();
        for n in lead.stack() {
            if let Ok(k) = by_number_tx(&tx, n) {
                kids.push(k);
            }
        }
        let ready = stack_ready(&lead, &kids, &rule);
        let from = lead.column.clone();
        let lead_rules = [(crate::TEST_KEY, "subagent test", test_on), (crate::REVIEW_KEY, "subagent review", review_on)];
        // The lead's own work is held to its test and review too (card #96): the stack waits,
        // and says why once. Its children's reviews stand for its own (card #113).
        let mut missing = if ready && BUILDING.contains(&from.as_str()) { lead_missing_in_stack(&lead, &kids, &lead_rules) } else { vec![] };
        // And a bug reported on the lead itself waits for the card-reproducer's proof (card #112).
        if ready && BUILDING.contains(&from.as_str()) && repro_on && crate::repro_open(&lead) {
            missing.push("reproducer proof".to_string());
        }
        let held = (!missing.is_empty()).then(|| format!("#{}'s own {} {} passed", lead.number, missing.join(" and "), if missing.len() > 1 { "haven't" } else { "hasn't" }));
        let was_held = lead.fields.get(STACK_HELD_KEY).and_then(Value::as_str).map(str::to_string);
        if held != was_held {
            let mut p = Patch::new();
            p.insert(STACK_HELD_KEY.into(), held.as_ref().map_or(Value::Null, |h| json!(h)));
            let line = held.as_ref().map(|h| format!("The stack is ready to test, but {h}: it stays in {from} until it has ({cause})"));
            put(&tx, &mut lead, &p, by, line.as_deref())?;
        }
        let to = if ready && BUILDING.contains(&from.as_str()) && missing.is_empty() {
            Some("Human test")
        } else if !ready && from == "Human test" {
            Some("In progress")
        } else {
            None
        };
        if let Some(to) = to {
            let mut p = Patch::new();
            p.insert("column".into(), json!(to));
            // A child's work sent the stack back, not the lead's: the lead keeps its own verdicts
            // (card #96), where a move into In progress would otherwise reset them.
            if to == "In progress" {
                for key in crate::RESET_ON_IN_PROGRESS {
                    if let Some(v) = lead.fields.get(*key) {
                        p.insert((*key).into(), v.clone());
                    }
                }
            }
            let why = if to == "Human test" {
                if rule == "first" { "a card in the stack is ready to test" } else { "every card in the stack is ready to test" }
            } else {
                "a card in the stack went back to In progress"
            };
            put(&tx, &mut lead, &p, by, Some(&format!("The app moved the stack from {from} to {to}: {why} ({cause})")))?;
            moves.push(StackMove { id: lead.id.clone(), number: lead.number, from, to: to.into(), lead: true });
        }
        tx.commit()?;
        Ok(moves)
    }

    /// Archives (`on`) or restores the children of a lead that was just archived or restored.
    /// Restoring brings back only the children archived with it.
    pub(crate) fn stack_archive_children(tx: &rusqlite::Transaction, lead: &Card, on: bool, by: &str) -> Result<()> {
        let at = now();
        for n in lead.stack() {
            let Ok(mut k) = by_number_tx(tx, n) else { continue };
            let mut p = Patch::new();
            if on {
                if k.archived {
                    continue;
                }
                p.insert("archived".into(), json!(true));
                p.insert("archivedAt".into(), json!(at));
                p.insert("archivedBy".into(), json!(by));
                p.insert("archivedWith".into(), json!(lead.number));
                put(tx, &mut k, &p, by, Some(&format!("Archived with its stack #{}", lead.number)))?;
            } else {
                if !k.archived || k.fields.get("archivedWith").and_then(Value::as_i64) != Some(lead.number) {
                    continue;
                }
                p = crate::cards::restore_patch(&k);
                p.insert("archivedWith".into(), Value::Null);
                put(tx, &mut k, &p, by, Some(&format!("Restored with its stack #{}", lead.number)))?;
            }
        }
        Ok(())
    }
}
