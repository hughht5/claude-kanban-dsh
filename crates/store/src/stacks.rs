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

/// Is the stack ready for Hugh, by `rule`? Archived children don't count.
pub fn stack_ready(children: &[Card], rule: &str) -> bool {
    let live: Vec<&Card> = children.iter().filter(|c| !c.archived).collect();
    if live.is_empty() {
        return false;
    }
    let ready = |c: &&Card| READY.contains(&c.column.as_str());
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
        let repo = fields.get("repo").and_then(Value::as_str).unwrap_or("").trim().to_lowercase();
        if !repo.is_empty() && !ship_lane(&l).is_empty() && repo != ship_lane(&l) {
            return Err(Error::Invalid(format!("#{lead} is for {}: a stack is one branch in one repo", repo_name(&l))));
        }
        let card = self.create(None, fields, by)?;
        self.stack_add(lead, card.number, by)?;
        self.get(&card.id)
    }

    /// Hugh's Passed (or taking it back).
    pub fn set_passed(&mut self, id: &str, on: bool, by: &str) -> Result<Card> {
        let tx = self.begin()?;
        let mut c = get_tx(&tx, id)?.ok_or_else(|| Error::NotFound(id.to_string()))?;
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
        let ready = stack_ready(&kids, &rule);
        let from = lead.column.clone();
        let to = if ready && BUILDING.contains(&from.as_str()) {
            Some("Human test")
        } else if !ready && from == "Human test" {
            Some("In progress")
        } else {
            None
        };
        if let Some(to) = to {
            let mut p = Patch::new();
            p.insert("column".into(), json!(to));
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
