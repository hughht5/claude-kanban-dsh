//! The ship queue (card #79): one card ships at a time per repo. A card Hugh moves into Ship it
//! while another card of the same repo is shipping waits, with ship state `queued`, and its
//! session isn't told yet. Once that repo has nothing shipping, the oldest waiting card is
//! released (state `requested` again) and the app hands it to its session.
//!
//! A card holds its repo while it is in Ship it (not archived) and its ship is `shipping`, or
//! `requested` and handed to its session (`handedAt`, or the app's older "Handed" detail), or
//! `requested` within the last `FRESH_MS` (about to be handed). A request nobody picked up
//! (no session started) stops holding it after that, so one stuck card can't stop a repo.
//! `shipped`, `failed`, `needs-hugh` and `cancelled` free the repo, as does leaving Ship it.
//! Cards with no repo don't queue.

use crate::cards::Card;
use crate::{now, Patch, Result, Store};
use serde_json::{json, Value};

pub const QUEUED: &str = "queued";
/// How long an unhanded request holds its repo, for the moment between Hugh's move and the
/// app handing it to a session.
const FRESH_MS: i64 = 60_000;
/// The detail `dispatch::mark_handed` writes; builds before card #79 wrote no `handedAt`.
const HANDED_DETAIL: &str = "Handed to the card's session in the app";

/// The repo a card ships to, its lane in the queue. Empty: the card doesn't queue.
pub fn ship_lane(card: &Card) -> String {
    card.fields.get("repo").and_then(Value::as_str).unwrap_or("").trim().to_lowercase()
}

fn ship(card: &Card) -> Option<&Value> {
    card.fields.get("ship")
}

fn ship_state(card: &Card) -> &str {
    ship(card).and_then(|s| s.get("state")).and_then(Value::as_str).unwrap_or("")
}

/// Milliseconds since the epoch of an RFC 3339 time, whatever its precision or offset.
fn millis(v: Option<&Value>) -> Option<i64> {
    let s = v?.as_str()?;
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|t| t.timestamp_millis())
}

/// When the ship was asked for, then the card's number: the queue's order. A missing or
/// unreadable time sorts last.
fn ship_key(card: &Card) -> (i64, i64) {
    (millis(ship(card).and_then(|s| s.get("requestedAt"))).unwrap_or(i64::MAX), card.number)
}

fn in_ship_it(card: &Card) -> bool {
    card.column == "Ship it" && !card.archived
}

/// Has the card's request been handed to its session?
fn handed(card: &Card) -> bool {
    let s = ship(card);
    s.and_then(|s| s.get("handedAt")).is_some() || s.and_then(|s| s.get("detail")).and_then(Value::as_str) == Some(HANDED_DETAIL)
}

/// Is `card` shipping now: in Ship it and holding its repo?
fn holds(card: &Card, now_ms: i64) -> bool {
    if !in_ship_it(card) {
        return false;
    }
    match ship_state(card) {
        "shipping" => true,
        "requested" => handed(card) || millis(ship(card).and_then(|s| s.get("requestedAt"))).map_or(false, |t| now_ms - t < FRESH_MS),
        _ => false,
    }
}

/// Does `other` go before `me`: shipping now, or asked for earlier and still waiting its turn?
fn blocks(other: &Card, me: &Card, now_ms: i64) -> bool {
    if other.id == me.id || ship_lane(me).is_empty() || ship_lane(other) != ship_lane(me) {
        return false;
    }
    // A fresh request asked for after mine (both moves racing to their dispatch) doesn't.
    if holds(other, now_ms) && (ship_state(other) == "shipping" || handed(other) || ship_key(other) < ship_key(me)) {
        return true;
    }
    in_ship_it(other) && ship_state(other) == QUEUED && ship_key(other) < ship_key(me)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Has `card`'s ship started (card #89): is its session on it, so its merge may be under way
/// or done? Says how, if so. Only a ship still waiting to start (`queued`, or asked for and not
/// handed to a session yet) can take another card into the same push; anything else in Ship
/// it (shipping, shipped, waiting for Hugh, or a ship in no state to start) counts as started.
/// A card not in Ship it has no ship to start.
pub fn ship_started(card: &Card) -> Option<&'static str> {
    if !in_ship_it(card) {
        return None;
    }
    match ship_state(card) {
        QUEUED => None,
        "requested" if !handed(card) => None,
        "requested" => Some("its session has the ship"),
        "shipping" => Some("it is shipping now"),
        "shipped" => Some("it has shipped"),
        "needs-hugh" => Some("its ship is waiting for Hugh"),
        _ => Some("it has no ship waiting to start"),
    }
}

/// Does a card dropped into Ship it now wait for `card`'s ship to be over (card #89)? Yes while
/// `card` holds its repo in the queue; and a ship that is over has nothing left to wait for.
/// Not for a ship waiting for Hugh, which frees the repo.
pub fn ship_goes_first(card: &Card) -> bool {
    holds(card, now_ms()) || (in_ship_it(card) && ship_state(card) == "shipped")
}

/// When `lead` and `child` both wait to ship and the child asked first, the child's ask time:
/// the stack keeps the earlier place in the queue once they ship as one (card #89).
pub(crate) fn earlier_place(lead: &Card, child: &Card) -> Option<Value> {
    if !in_ship_it(lead) || !in_ship_it(child) || ship_state(lead) != QUEUED || ship_started(child).is_some() {
        return None;
    }
    if ship_key(child).0 < ship_key(lead).0 {
        ship(child).and_then(|s| s.get("requestedAt")).cloned()
    } else {
        None
    }
}

impl Store {
    /// The card `card` must wait for before it ships, if any: the one shipping, else the
    /// earliest asked for.
    pub fn ship_blocker(&self, card: &Card) -> Result<Option<Card>> {
        let t = now_ms();
        let mut before: Vec<Card> = self.list(false)?.into_iter().filter(|o| blocks(o, card, t)).collect();
        before.sort_by_key(|o| (!holds(o, t), ship_key(o)));
        Ok(before.into_iter().next())
    }

    /// Puts `id` in the queue behind `blocker`. `from` is the column Hugh moved it from, for
    /// the message its session gets when its turn comes.
    pub fn ship_wait(&mut self, id: &str, blocker: &Card, from: &str) -> Result<Card> {
        let card = self.get(id)?;
        let mut ship = card.fields.get("ship").and_then(Value::as_object).cloned().unwrap_or_default();
        let detail = format!("Waiting for #{} to finish shipping: one ship at a time per repo", blocker.number);
        ship.insert("state".into(), json!(QUEUED));
        ship.insert("queuedAt".into(), json!(now()));
        ship.insert("from".into(), json!(from));
        ship.insert("detail".into(), json!(detail));
        let mut patch = Patch::new();
        patch.insert("ship".into(), Value::Object(ship));
        let card = self.update(id, card.version, &patch, "the app")?;
        self.add_log(id, &format!("Waiting to ship: #{} \"{}\" goes first in {}", blocker.number, blocker.title(), repo_name(&card)), "the app")?;
        Ok(card)
    }

    /// Is any card waiting to ship? A cheap check for the app's loop.
    pub fn ship_any_queued(&self) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM cards WHERE json_extract(fields, '$.ship.state') = ?1",
            [QUEUED],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// The waiting cards whose turn it is: per repo with nothing shipping, the oldest one.
    pub fn ship_due(&self) -> Result<Vec<Card>> {
        let t = now_ms();
        let cards = self.list(false)?;
        let mut due: Vec<Card> = Vec::new();
        for c in cards.iter().filter(|c| in_ship_it(c) && ship_state(c) == QUEUED) {
            let lane = ship_lane(c);
            if cards.iter().any(|o| ship_lane(o) == lane && holds(o, t)) {
                continue;
            }
            match due.iter_mut().find(|d| ship_lane(d) == lane) {
                Some(d) if ship_key(c) < ship_key(d) => *d = c.clone(),
                Some(_) => {}
                None => due.push(c.clone()),
            }
        }
        Ok(due)
    }

    /// Takes `id` out of the queue: its ship is `requested` again, to be handed to its session.
    /// Returns the card and the column Hugh first moved it from.
    pub fn ship_release(&mut self, id: &str) -> Result<(Card, String)> {
        let card = self.get(id)?;
        let mut ship = card.fields.get("ship").and_then(Value::as_object).cloned().unwrap_or_default();
        let from = ship.get("from").and_then(Value::as_str).unwrap_or("Human test").to_string();
        ship.insert("state".into(), json!("requested"));
        ship.insert("detail".into(), json!("Its turn to ship"));
        // Its own turn: it holds the repo from now, as a fresh request would.
        ship.insert("handedAt".into(), json!(now()));
        let mut patch = Patch::new();
        patch.insert("ship".into(), Value::Object(ship));
        let card = self.update(id, card.version, &patch, "the app")?;
        self.add_log(id, &format!("Its turn to ship: nothing else is shipping in {}", repo_name(&card)), "the app")?;
        Ok((card, from))
    }

    /// Cancels the waits of cards that left Ship it some other way than Hugh's move (a
    /// session's or the stack rules' move, or archiving). Returns how many.
    pub fn ship_cancel_strays(&mut self) -> Result<usize> {
        let strays: Vec<Card> = self.list(true)?.into_iter().filter(|c| ship_state(c) == QUEUED && !in_ship_it(c)).collect();
        for c in &strays {
            let mut ship = c.fields.get("ship").and_then(Value::as_object).cloned().unwrap_or_default();
            ship.insert("state".into(), json!("cancelled"));
            ship.insert("cancelledAt".into(), json!(now()));
            ship.insert("detail".into(), json!(format!("Left Ship it (to {}) while waiting to ship", c.column)));
            let mut patch = Patch::new();
            patch.insert("ship".into(), Value::Object(ship));
            self.update(&c.id, c.version, &patch, "the app")?;
            self.add_log(&c.id, "Ship request cancelled: it left Ship it while waiting its turn", "the app")?;
        }
        Ok(strays.len())
    }
}

pub(crate) fn repo_name(card: &Card) -> String {
    card.fields.get("repo").and_then(Value::as_str).unwrap_or("").trim().to_string()
}
