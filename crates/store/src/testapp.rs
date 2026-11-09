//! Turns on the shared test app (card #115).
//!
//! The test app is one installed copy that every kanban-app card tests in, so its use is
//! given out in turns. Two kinds:
//!
//! - a **tester** turn: one card session uses the installed test app (its own checks, its
//!   card-tester). Only a card in In progress or Human test takes one.
//! - an **install** turn: a session outside the app (the router's wave install, Hugh's own)
//!   reinstalls it. It goes to the front of the line, behind other installs, and while it is
//!   held no tester turn is given, so the install's restart never lands under a tester.
//!
//! The queue is JSON in the setting `testapp.queue`, like the start queue. The logic here is
//! pure (the clock and the facts about cards and sessions are passed in), so it is tested
//! without the app. `scripts\install.ps1` never reads this: the app writes the holder to a
//! file it does read (src-tauri/src/testapp.rs).

use crate::{Card, Error, Patch, Result, Store};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const QUEUE_KEY: &str = "testapp.queue";
pub const LIMIT_KEY: &str = "testapp.turn_minutes";
pub const DEFAULT_LIMIT: i64 = 60;
pub const MIN_LIMIT: i64 = 10;
pub const MAX_LIMIT: i64 = 240;
/// A turn held this long with someone waiting shows amber on the board, with Release.
pub const OVERDUE_MINUTES: i64 = 45;
/// The columns a card takes a tester turn from.
pub const TESTER_COLUMNS: [&str; 2] = ["In progress", "Human test"];
/// An install waiting in line that hasn't asked again for this long has given up (kb --wait
/// asks every 10 s): it leaves the line, so an abandoned install never holds the test app.
pub const INSTALL_WAIT_MINUTES: i64 = 10;
/// An install turn whose install hasn't been seen within this long ends.
pub const INSTALL_START_MINUTES: i64 = 15;

pub const TESTER: &str = "tester";
pub const INSTALL: &str = "install";

/// Who asks for a turn: a card's session (`card_id` set) or a session outside the app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ask {
    pub kind: String,
    pub card_id: Option<String>,
    pub card: Option<i64>,
    /// The name install.ps1 knows the run by: the card's number, or KANBAN_LOCK_OWNER.
    pub owner: String,
    /// What the board shows: "#115", "wave 3".
    pub label: String,
}

impl Ask {
    pub fn tester(card_id: &str, number: i64) -> Ask {
        Ask { kind: TESTER.into(), card_id: Some(card_id.into()), card: Some(number), owner: number.to_string(), label: format!("#{number}") }
    }

    pub fn install(owner: &str, label: &str) -> Ask {
        let label = if label.trim().is_empty() { owner } else { label.trim() };
        Ask { kind: INSTALL.into(), card_id: None, card: None, owner: owner.trim().into(), label: label.into() }
    }

    /// The same asker: the same card, or (outside) the same owner and kind.
    fn same(&self, kind: &str, card_id: Option<&str>, owner: &str) -> bool {
        match (&self.card_id, card_id) {
            (Some(a), Some(b)) => a == b,
            (None, None) => self.owner == owner && self.kind == kind,
            _ => false,
        }
    }
}

/// The turn being used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    #[serde(flatten)]
    pub who: Ask,
    pub given_at: String,
    /// Given, taken again, or seen in use (the session working, its tester running, its install).
    pub renewed_at: String,
    /// An install turn: an install by its owner has been seen holding install.lock.
    #[serde(default)]
    pub install_seen: bool,
}

/// A place in line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Waiting {
    #[serde(flatten)]
    pub who: Ask,
    pub asked_at: String,
    pub seq: u64,
    /// The last time the asker asked again (installs only; a card waits for its message).
    #[serde(default)]
    pub seen_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestAppQueue {
    pub holder: Option<Turn>,
    pub line: Vec<Waiting>,
    pub next_seq: u64,
}

/// What a take did.
#[derive(Debug, Clone, PartialEq)]
pub enum Take {
    /// The turn is the asker's now.
    Given,
    /// It was the asker's already; its clock starts again.
    Renewed,
    /// In line at this place, from 1.
    Waiting(usize),
}

/// Why a holder's turn ended without its release.
#[derive(Debug, Clone, PartialEq)]
pub enum Ended {
    Archived,
    /// The card is in this column, outside TESTER_COLUMNS.
    Left(String),
    Gone,
    TimedOut(i64),
    /// An install turn whose install has started the test app again.
    Installed,
    /// An install turn whose install never started.
    NotStarted,
    /// An install that stopped asking while it waited.
    GaveUp,
}

impl Ended {
    pub fn text(&self) -> String {
        match self {
            Ended::Archived => "the card was archived".into(),
            Ended::Left(col) => format!("the card moved to {col}"),
            Ended::Gone => "the card is no longer on the board".into(),
            Ended::TimedOut(m) => format!("it was not used for {m} min"),
            Ended::Installed => "the install has started the test app again".into(),
            Ended::NotStarted => format!("its install didn't start within {INSTALL_START_MINUTES} min"),
            Ended::GaveUp => format!("it stopped asking for {INSTALL_WAIT_MINUTES} min"),
        }
    }
}

pub fn parse(t: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(t).ok().map(|d| d.with_timezone(&Utc))
}

pub fn stamp(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// A card's column and whether it is archived; None when it isn't on the board.
pub type CardState = Option<(String, bool)>;

impl TestAppQueue {
    /// The asker's place in line, from 1.
    pub fn place(&self, card_id: Option<&str>, owner: &str, kind: &str) -> Option<usize> {
        self.line.iter().position(|w| w.who.same(kind, card_id, owner)).map(|i| i + 1)
    }

    pub fn holds(&self, card_id: Option<&str>, owner: &str, kind: &str) -> bool {
        self.holder.as_ref().map_or(false, |h| h.who.same(kind, card_id, owner))
    }

    /// Asks for a turn. The asker gets it now when nobody holds it, nobody is in line ahead
    /// of it, and `blocked` (the old DSH lock folder belongs to someone else) is false.
    /// Otherwise it goes in line: an install in front of every tester (behind earlier
    /// installs), a tester at the back. Asking again keeps the place, or renews a held turn.
    pub fn take(&mut self, ask: Ask, now: DateTime<Utc>, blocked: bool) -> Take {
        let at = stamp(now);
        if let Some(h) = self.holder.as_mut().filter(|h| h.who.same(&ask.kind, ask.card_id.as_deref(), &ask.owner)) {
            h.renewed_at = at;
            return Take::Renewed;
        }
        if let Some(p) = self.place(ask.card_id.as_deref(), &ask.owner, &ask.kind) {
            self.line[p - 1].seen_at = Some(at);
            return Take::Waiting(p);
        }
        if self.holder.is_none() && self.line.is_empty() && !blocked {
            self.holder = Some(Turn { who: ask, given_at: at.clone(), renewed_at: at, install_seen: false });
            return Take::Given;
        }
        self.next_seq += 1;
        let install = ask.kind == INSTALL;
        let w = Waiting { who: ask, asked_at: at.clone(), seq: self.next_seq, seen_at: Some(at) };
        let i = if install { self.line.iter().take_while(|x| x.who.kind == INSTALL).count() } else { self.line.len() };
        self.line.insert(i, w);
        Take::Waiting(i + 1)
    }

    /// The asker lets go: of its turn (returned as true) or its place in line.
    pub fn release(&mut self, card_id: Option<&str>, owner: &str, kind: &str) -> Option<bool> {
        if self.holds(card_id, owner, kind) {
            self.holder = None;
            return Some(true);
        }
        let i = self.line.iter().position(|w| w.who.same(kind, card_id, owner))?;
        self.line.remove(i);
        Some(false)
    }

    /// Hugh's (or an outside session's) Release: whoever holds the turn loses it.
    pub fn force(&mut self) -> Option<Turn> {
        self.holder.take()
    }

    /// Takes out of the line the cards that can't use a turn (archived, gone, or outside
    /// TESTER_COLUMNS) and the installs that stopped asking. Returns them with the reason.
    pub fn prune(&mut self, now: DateTime<Utc>, card: impl Fn(&str) -> CardState) -> Vec<(Waiting, Ended)> {
        let mut out = vec![];
        self.line.retain(|w| {
            let Some(id) = w.who.card_id.as_deref() else {
                let seen = w.seen_at.as_deref().or(Some(w.asked_at.as_str())).and_then(parse).unwrap_or(now);
                if now - seen > Duration::minutes(INSTALL_WAIT_MINUTES) {
                    out.push((w.clone(), Ended::GaveUp));
                    return false;
                }
                return true;
            };
            match ended_for(card(id)) {
                Some(e) => {
                    out.push((w.clone(), e));
                    false
                }
                None => true,
            }
        });
        out
    }

    /// Whether the holder's turn has ended by itself, and why. `install_done`: the install
    /// lock was seen held by this install turn's owner and is gone now.
    pub fn stale(&self, now: DateTime<Utc>, limit_minutes: i64, card: impl Fn(&str) -> CardState, install_done: bool) -> Option<Ended> {
        let h = self.holder.as_ref()?;
        if let Some(id) = h.who.card_id.as_deref() {
            if let Some(e) = ended_for(card(id)) {
                return Some(e);
            }
        }
        if h.who.kind == INSTALL && h.install_seen && install_done {
            return Some(Ended::Installed);
        }
        if h.who.kind == INSTALL && !h.install_seen && parse(&h.given_at).map_or(false, |g| now - g > Duration::minutes(INSTALL_START_MINUTES)) {
            return Some(Ended::NotStarted);
        }
        let last = parse(&h.renewed_at).unwrap_or(now);
        (now - last > Duration::minutes(limit_minutes)).then_some(Ended::TimedOut(limit_minutes))
    }

    /// The holder is in use now: its clock starts again.
    pub fn renew(&mut self, now: DateTime<Utc>) {
        if let Some(h) = self.holder.as_mut() {
            h.renewed_at = stamp(now);
        }
    }

    /// With nobody holding the turn, gives it to the first in line who can use it: an
    /// install always can; a card only if its session is live (`live`). A paused card keeps
    /// its place and is passed over until its session runs again. `blocked(owner)`: someone
    /// else has the test app outside this queue (an install holding install.lock, the other
    /// board's turn, the old DSH lock folder), so only that owner, if it waits here, gets it.
    pub fn next(&mut self, now: DateTime<Utc>, live: impl Fn(&str) -> bool, blocked: impl Fn(&str) -> bool) -> Option<Turn> {
        if self.holder.is_some() {
            return None;
        }
        let i = self.line.iter().position(|w| w.who.card_id.as_deref().map_or(true, &live) && !blocked(&w.who.owner))?;
        let w = self.line.remove(i);
        let at = stamp(now);
        let t = Turn { who: w.who, given_at: at.clone(), renewed_at: at, install_seen: false };
        self.holder = Some(t.clone());
        Some(t)
    }

    /// When the holder's turn ends by itself: `renewed_at` plus the limit.
    pub fn expires_at(&self, limit_minutes: i64) -> Option<DateTime<Utc>> {
        let h = self.holder.as_ref()?;
        Some(parse(&h.renewed_at)? + Duration::minutes(limit_minutes))
    }

    /// Held for OVERDUE_MINUTES or more while someone waits: the board shows Release in amber.
    pub fn overdue(&self, now: DateTime<Utc>) -> bool {
        let Some(h) = self.holder.as_ref() else { return false };
        !self.line.is_empty() && parse(&h.given_at).map_or(false, |g| now - g >= Duration::minutes(OVERDUE_MINUTES))
    }

    pub fn is_empty(&self) -> bool {
        self.holder.is_none() && self.line.is_empty()
    }
}

/// Why a card can't hold or wait for a tester turn, if it can't.
fn ended_for(state: CardState) -> Option<Ended> {
    match state {
        None => Some(Ended::Gone),
        Some((_, true)) => Some(Ended::Archived),
        Some((col, false)) if !TESTER_COLUMNS.contains(&col.as_str()) => Some(Ended::Left(col)),
        _ => None,
    }
}

/// Why a card in `column` can't take a tester turn, if it can't.
pub fn take_refused(column: &str, archived: bool) -> Option<String> {
    if archived {
        return Some("the card is archived".into());
    }
    (!TESTER_COLUMNS.contains(&column)).then(|| format!("a card takes a test-app turn only in In progress or Human test; this one is in {column}"))
}

/// A stored limit in minutes: unset or unreadable is the default, kept within MIN..=MAX.
pub fn parse_limit(v: Option<&str>) -> i64 {
    v.and_then(|s| s.trim().parse::<i64>().ok()).map(|n| n.clamp(MIN_LIMIT, MAX_LIMIT)).unwrap_or(DEFAULT_LIMIT)
}

impl Store {
    /// The test-app turns. A queue that can't be read counts as empty.
    pub fn test_app_queue(&self) -> Result<TestAppQueue> {
        Ok(self.setting(QUEUE_KEY)?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default())
    }

    pub fn save_test_app_queue(&mut self, q: &TestAppQueue) -> Result<()> {
        self.set_setting(QUEUE_KEY, &serde_json::to_string(q)?)
    }

    pub fn test_app_limit(&self) -> Result<i64> {
        Ok(parse_limit(self.setting(LIMIT_KEY)?.as_deref()))
    }

    pub fn set_test_app_limit(&mut self, minutes: i64) -> Result<i64> {
        if !(MIN_LIMIT..=MAX_LIMIT).contains(&minutes) {
            return Err(Error::Invalid(format!("a test-app turn lasts {MIN_LIMIT} to {MAX_LIMIT} minutes")));
        }
        self.set_setting(LIMIT_KEY, &minutes.to_string())?;
        Ok(minutes)
    }
}

// --- A test app per card (card #147) --------------------------------------------------------
//
// A card's session that takes a turn first gets a test app of its own: the lowest slot no card
// holds, from 1 up to a ceiling. Hugh (9 Nov 2026): when every test app holds a card's build,
// make a new one, so a card gets Test 4 when Test to Test 3 are taken, and its own install
// creates it. The card keeps it while it is In progress or Human test, so Hugh tests its build
// there and no other card installs over it. Turns stay as above, one queue per slot.

/// The card field holding its own test app: `{slot, channel, name, label, port, since}`. Only
/// the app writes it (it isn't in kanban_update_card's list).
pub const TEST_APP_KEY: &str = "testApp";
/// The highest slot the board gives out, 1 to MAX_SLOTS.
pub const SLOTS_KEY: &str = "testapp.slots";
pub const DEFAULT_SLOTS: u32 = 10;
/// channel.rs and install.ps1 know test to test10.
pub const MAX_SLOTS: u32 = 10;
/// The cards waiting for a test app because every slot up to the ceiling is taken.
pub const SLOT_LINE_KEY: &str = "testapp.slot_line";
/// Who the app's own writes are by.
const APP_BY: &str = "the app";

/// The setting holding slot `slot`'s turns: `testapp.queue` for Test, so an older build still
/// reads it, and `testapp.queue.N` for Test N.
pub fn queue_key(slot: u32) -> String {
    if slot <= 1 { QUEUE_KEY.to_string() } else { format!("{QUEUE_KEY}.{slot}") }
}

/// The slot's channel: "test", "test2", ...
pub fn slot_channel(slot: u32) -> String {
    if slot <= 1 { "test".into() } else { format!("test{slot}") }
}

/// The slot as Hugh sees it: "Test", "Test 2", ...
pub fn slot_label(slot: u32) -> String {
    if slot <= 1 { "Test".into() } else { format!("Test {slot}") }
}

/// The slot of a test channel's name; None for live and anything else.
pub fn channel_slot(channel: &str) -> Option<u32> {
    crate::slot_of(channel).filter(|s| (1..=MAX_SLOTS).contains(s))
}

/// The `testApp` value for `slot`.
pub fn test_app_value(slot: u32, now: DateTime<Utc>) -> Value {
    json!({ "slot": slot, "channel": slot_channel(slot), "name": crate::product_name_for(slot), "label": slot_label(slot),
            "port": crate::devtools_port_for(slot), "since": stamp(now) })
}

/// The card's own test app, if it names one.
pub fn card_slot(card: &Card) -> Option<u32> {
    let s = card.fields.get(TEST_APP_KEY)?.get("slot")?.as_u64()?;
    u32::try_from(s).ok().filter(|s| (1..=MAX_SLOTS).contains(s))
}

/// When the card was given its test app (`testApp.since`), or "".
pub fn test_app_since(card: &Card) -> String {
    card.fields.get(TEST_APP_KEY).and_then(|v| v.get("since")).and_then(Value::as_str).unwrap_or("").to_string()
}

/// Why a card can't keep (or be given) a test app, if it can't: it is archived, outside In
/// progress and Human test, or a stack's card in Human test (Hugh tests a stack on its lead's
/// install).
pub fn slot_refused(card: &Card) -> Option<String> {
    // The column first: a move into Done archives the card too, and "moved to Done" says more.
    if !TESTER_COLUMNS.contains(&card.column.as_str()) {
        return Some(format!("the card moved to {}", card.column));
    }
    if card.archived {
        return Some("the card was archived".into());
    }
    if card.stack_of().is_some() && card.column == "Human test" {
        return Some("a stack's card is tested on its lead's test app".into());
    }
    None
}

/// The lowest slot from 1 to `ceiling` that isn't `own` (a test app never installs over itself;
/// live is 0) and isn't taken.
pub fn lowest_free(ceiling: u32, own: u32, taken: &BTreeSet<u32>) -> Option<u32> {
    (1..=ceiling.min(MAX_SLOTS)).find(|s| *s != own && !taken.contains(s))
}

/// The stored ceiling: unset or unreadable is the default, kept within 1..=MAX_SLOTS.
pub fn parse_slots(v: Option<&str>) -> u32 {
    v.and_then(|s| s.trim().parse::<u32>().ok()).map(|n| n.clamp(1, MAX_SLOTS)).unwrap_or(DEFAULT_SLOTS)
}

/// A card waiting for a test app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotWait {
    pub card_id: String,
    pub card: i64,
    pub asked_at: String,
    pub seq: u64,
}

/// The cards waiting for a test app, first come first served.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotLine {
    pub line: Vec<SlotWait>,
    pub next_seq: u64,
}

impl SlotLine {
    /// The card's place, from 1.
    pub fn place(&self, card_id: &str) -> Option<usize> {
        self.line.iter().position(|w| w.card_id == card_id).map(|i| i + 1)
    }

    /// Joins at the back, or keeps its place. Returns the place, from 1.
    pub fn join(&mut self, card_id: &str, number: i64, now: DateTime<Utc>) -> usize {
        if let Some(p) = self.place(card_id) {
            return p;
        }
        self.next_seq += 1;
        self.line.push(SlotWait { card_id: card_id.into(), card: number, asked_at: stamp(now), seq: self.next_seq });
        self.line.len()
    }

    pub fn leave(&mut self, card_id: &str) -> bool {
        let n = self.line.len();
        self.line.retain(|w| w.card_id != card_id);
        self.line.len() != n
    }
}

/// What a card's take found about its test app.
#[derive(Debug, Clone, PartialEq)]
pub enum SlotClaim {
    /// The card had it already.
    Kept(u32),
    /// Given to it now.
    Given(u32),
    /// Every slot up to the ceiling is taken: its place in the slot line, from 1.
    Waiting(usize),
}

/// What one pass over the slot line did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LineStep {
    /// (card id, number, slot) given a test app.
    pub given: Vec<(String, i64, u32)>,
    /// (card id, why) taken out of the line: it can't use a test app now.
    pub left: Vec<(String, String)>,
}

impl Store {
    /// The highest slot the board gives out.
    pub fn test_slots_ceiling(&self) -> Result<u32> {
        Ok(parse_slots(self.setting(SLOTS_KEY)?.as_deref()))
    }

    /// Slot `slot`'s turns (slot 1 is `test_app_queue`). A queue that can't be read counts as empty.
    pub fn test_app_queue_for(&self, slot: u32) -> Result<TestAppQueue> {
        Ok(self.setting(&queue_key(slot))?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default())
    }

    pub fn save_test_app_queue_for(&mut self, slot: u32, q: &TestAppQueue) -> Result<()> {
        self.set_setting(&queue_key(slot), &serde_json::to_string(q)?)
    }

    /// The cards waiting for a test app. A line that can't be read counts as empty.
    pub fn slot_line(&self) -> Result<SlotLine> {
        Ok(self.setting(SLOT_LINE_KEY)?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default())
    }

    pub fn save_slot_line(&mut self, l: &SlotLine) -> Result<()> {
        self.set_setting(SLOT_LINE_KEY, &serde_json::to_string(l)?)
    }

    /// Every card with a `testApp`, archived ones too.
    pub fn cards_with_test_apps(&self) -> Result<Vec<Card>> {
        let mut st = self
            .conn
            .prepare("SELECT * FROM cards WHERE json_valid(fields) AND json_extract(fields, '$.testApp') IS NOT NULL ORDER BY number")?;
        let cards = st.query_map([], crate::cards::row_to_card)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(cards)
    }

    /// The test apps cards hold now, slot to card. When two cards name one slot, the one given
    /// it first keeps it.
    pub fn slot_owners(&self) -> Result<BTreeMap<u32, Card>> {
        let mut out: BTreeMap<u32, Card> = BTreeMap::new();
        for c in self.cards_with_test_apps()? {
            let Some(s) = card_slot(&c) else { continue };
            if slot_refused(&c).is_some() {
                continue;
            }
            let keep = out.get(&s).map_or(false, |o| test_app_since(o) <= test_app_since(&c));
            if !keep {
                out.insert(s, c);
            }
        }
        Ok(out)
    }

    /// The cards whose `testApp` should go, with the slot and why: the card can't keep it, it
    /// names no slot, or another card was given the same slot first.
    pub fn stale_test_apps(&self) -> Result<Vec<(Card, Option<u32>, String)>> {
        let owners = self.slot_owners()?;
        let mut out = vec![];
        for c in self.cards_with_test_apps()? {
            let slot = card_slot(&c);
            let why = match slot {
                None => Some("it named no test app".to_string()),
                Some(s) => slot_refused(&c).or_else(|| {
                    owners.get(&s).filter(|o| o.id != c.id).map(|o| format!("{} was given to #{} first", slot_label(s), o.number))
                }),
            };
            if let Some(why) = why {
                out.push((c, slot, why));
            }
        }
        Ok(out)
    }

    /// The slots that can't be given now: held by a card here, with a turn or a line of its own
    /// (an outside install) other than `except`'s, or `elsewhere` (another running app's board
    /// has it).
    pub fn taken_slots(&self, elsewhere: &BTreeSet<u32>, except: Option<&str>) -> Result<BTreeSet<u32>> {
        let mut taken: BTreeSet<u32> = self.slot_owners()?.into_keys().collect();
        taken.extend(elsewhere.iter().copied());
        for s in 1..=MAX_SLOTS {
            let q = self.test_app_queue_for(s)?;
            let others = q.holder.iter().map(|h| &h.who).chain(q.line.iter().map(|w| &w.who)).any(|w| w.card_id.as_deref() != except || except.is_none());
            if others {
                taken.insert(s);
            }
        }
        Ok(taken)
    }

    /// The test app whose turn (or line for it) card `id` already has, from before it had a test
    /// app of its own (#115's one queue): it keeps that one if it is free, so no old turn is left
    /// behind on another.
    fn turn_slot(&self, id: &str) -> Result<Option<u32>> {
        for s in 1..=MAX_SLOTS {
            let q = self.test_app_queue_for(s)?;
            if q.holder.as_ref().map_or(false, |h| h.who.card_id.as_deref() == Some(id)) || q.line.iter().any(|w| w.who.card_id.as_deref() == Some(id)) {
                return Ok(Some(s));
            }
        }
        Ok(None)
    }

    /// Gives card `id` test app `slot`, with a log line.
    pub fn give_test_app(&mut self, id: &str, slot: u32, now: DateTime<Utc>) -> Result<Card> {
        let c = self.get(id)?;
        let mut p = Patch::new();
        p.insert(TEST_APP_KEY.into(), test_app_value(slot, now));
        let c = self.update(id, c.version, &p, APP_BY)?;
        self.add_log(
            id,
            &format!(
                "Its test app is {} ({}, channel {}, CDP port {}); it stays this card's while the card is In progress or Human test",
                slot_label(slot),
                crate::product_name_for(slot),
                slot_channel(slot),
                crate::devtools_port_for(slot)
            ),
            APP_BY,
        )?;
        Ok(c)
    }

    /// Takes card `id`'s test app away, with its turn and its place in line on that slot, and
    /// logs why. Returns the slot it named (None when it had no test app).
    pub fn free_test_app(&mut self, id: &str, why: &str, by: &str) -> Result<Option<u32>> {
        let c = self.get(id)?;
        if !c.fields.contains_key(TEST_APP_KEY) {
            return Ok(None);
        }
        let slot = card_slot(&c);
        let mut p = Patch::new();
        p.insert(TEST_APP_KEY.into(), Value::Null);
        self.update(id, c.version, &p, by)?;
        if let Some(s) = slot {
            let mut q = self.test_app_queue_for(s)?;
            if q.release(Some(id), &c.number.to_string(), TESTER).is_some() {
                self.save_test_app_queue_for(s, &q)?;
            }
            self.add_log(id, &format!("Its test app ({}) is free again: {why}", slot_label(s)), by)?;
        }
        Ok(slot)
    }

    /// Card `id`'s test app: the one it holds, else the lowest free one (given now, unless
    /// cards wait ahead of it), else a place in the slot line. `own`: this app's own slot (0 for
    /// live). `elsewhere`: slots another running app's board has given out or has a turn on.
    pub fn claim_test_app(&mut self, id: &str, now: DateTime<Utc>, own: u32, elsewhere: &BTreeSet<u32>) -> Result<SlotClaim> {
        let c = self.get(id)?;
        if let Some(why) = slot_refused(&c) {
            return Err(Error::Invalid(format!("no test app for card #{}: {why}", c.number)));
        }
        if let Some(s) = card_slot(&c) {
            if self.slot_owners()?.get(&s).map_or(false, |o| o.id == c.id) {
                return Ok(SlotClaim::Kept(s));
            }
        }
        let mut line = self.slot_line()?;
        // Cards ahead that can't use a test app now don't hold this one back.
        let before = line.line.len();
        line.line.retain(|w| w.card_id == id || self.get(&w.card_id).map_or(false, |c| slot_refused(&c).is_none()));
        if line.line.len() != before {
            self.save_slot_line(&line)?;
        }
        let first = line.place(id).map_or(line.line.is_empty(), |p| p == 1);
        if first {
            let ceiling = self.test_slots_ceiling()?;
            let taken = self.taken_slots(elsewhere, Some(id))?;
            let kept = self.turn_slot(id)?.filter(|s| *s <= ceiling && *s != own && !taken.contains(s));
            if let Some(s) = kept.or_else(|| lowest_free(ceiling, own, &taken)) {
                if line.leave(id) {
                    self.save_slot_line(&line)?;
                }
                self.give_test_app(id, s, now)?;
                return Ok(SlotClaim::Given(s));
            }
        }
        let p = line.join(id, c.number, now);
        self.save_slot_line(&line)?;
        Ok(SlotClaim::Waiting(p))
    }

    /// One pass over the slot line: takes out the cards that can't use a test app now, then
    /// gives free slots to the rest in order, until none is free.
    pub fn step_slot_line(&mut self, now: DateTime<Utc>, own: u32, elsewhere: &BTreeSet<u32>) -> Result<LineStep> {
        let mut line = self.slot_line()?;
        let mut out = LineStep::default();
        if line.line.is_empty() {
            return Ok(out);
        }
        let before = line.clone();
        line.line.retain(|w| {
            let why = match self.get(&w.card_id) {
                Ok(c) => slot_refused(&c),
                Err(_) => Some("the card is no longer on the board".into()),
            };
            match why {
                Some(why) => {
                    out.left.push((w.card_id.clone(), why));
                    false
                }
                None => true,
            }
        });
        let ceiling = self.test_slots_ceiling()?;
        while let Some(w) = line.line.first().cloned() {
            let Some(s) = lowest_free(ceiling, own, &self.taken_slots(elsewhere, Some(&w.card_id))?) else { break };
            line.line.remove(0);
            self.give_test_app(&w.card_id, s, now)?;
            out.given.push((w.card_id, w.card, s));
        }
        if line != before {
            self.save_slot_line(&line)?;
        }
        Ok(out)
    }

    /// The copy of a board starts with no test apps: no card's `testApp`, no slot line and no
    /// turns on any slot (they belong to the board it was copied from).
    pub fn clear_test_apps(&mut self) -> Result<()> {
        for c in self.cards_with_test_apps()? {
            let mut p = Patch::new();
            p.insert(TEST_APP_KEY.into(), Value::Null);
            self.update(&c.id, c.version, &p, APP_BY)?;
        }
        self.save_slot_line(&SlotLine::default())?;
        for s in 1..=MAX_SLOTS {
            self.save_test_app_queue_for(s, &TestAppQueue::default())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(min: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-06T03:00:00Z").unwrap().with_timezone(&Utc) + Duration::minutes(min)
    }
    fn card(n: i64) -> Ask {
        Ask::tester(&format!("card-{n}"), n)
    }
    fn holder(q: &TestAppQueue) -> Option<String> {
        q.holder.as_ref().map(|h| h.who.label.clone())
    }
    fn line(q: &TestAppQueue) -> Vec<String> {
        q.line.iter().map(|w| w.who.label.clone()).collect()
    }
    fn in_progress(_: &str) -> CardState {
        Some(("In progress".into(), false))
    }
    const ALL_LIVE: fn(&str) -> bool = |_| true;
    const FREE: fn(&str) -> bool = |_| false;

    #[test]
    fn the_first_take_gets_it_and_the_rest_wait_in_order() {
        let mut q = TestAppQueue::default();
        assert_eq!(q.take(card(1), t(0), false), Take::Given);
        assert_eq!(q.take(card(2), t(1), false), Take::Waiting(1));
        assert_eq!(q.take(card(3), t(2), false), Take::Waiting(2));
        assert_eq!(q.take(card(2), t(3), false), Take::Waiting(1), "asking again keeps the place");
        assert_eq!(holder(&q).as_deref(), Some("#1"));
        assert_eq!(line(&q), ["#2", "#3"]);
    }

    #[test]
    fn taking_again_renews_the_turn() {
        let mut q = TestAppQueue::default();
        q.take(card(1), t(0), false);
        assert_eq!(q.take(card(1), t(50), false), Take::Renewed);
        assert_eq!(q.holder.as_ref().unwrap().renewed_at, stamp(t(50)));
        assert_eq!(q.holder.as_ref().unwrap().given_at, stamp(t(0)));
        assert_eq!(q.expires_at(60), Some(t(110)));
        assert!(q.stale(t(100), 60, in_progress, false).is_none());
    }

    #[test]
    fn a_take_never_jumps_the_line_even_when_nobody_holds_it() {
        let mut q = TestAppQueue::default();
        q.take(card(1), t(0), false);
        q.take(card(2), t(0), false);
        q.release(Some("card-1"), "1", TESTER);
        assert!(q.holder.is_none());
        assert_eq!(q.take(card(3), t(1), false), Take::Waiting(2));
    }

    #[test]
    fn an_install_goes_to_the_front_behind_other_installs() {
        let mut q = TestAppQueue::default();
        q.take(card(1), t(0), false);
        q.take(card(2), t(0), false);
        assert_eq!(q.take(Ask::install("router", "wave 3"), t(1), false), Take::Waiting(1));
        assert_eq!(q.take(Ask::install("hugh", ""), t(2), false), Take::Waiting(2));
        assert_eq!(line(&q), ["wave 3", "hugh", "#2"]);
        // It waits only for the current tester.
        q.release(Some("card-1"), "1", TESTER);
        assert_eq!(q.next(t(3), ALL_LIVE, FREE).map(|h| h.who.label), Some("wave 3".into()));
        // While the install holds it, no tester gets a turn, not even by taking.
        assert_eq!(q.take(card(4), t(4), false), Take::Waiting(3));
        assert!(q.next(t(4), ALL_LIVE, FREE).is_none());
    }

    #[test]
    fn an_install_turn_ends_once_its_install_has_started_the_app() {
        let mut q = TestAppQueue::default();
        q.take(Ask::install("router", "wave 3"), t(0), false);
        assert!(q.stale(t(5), 60, in_progress, true).is_none(), "not seen installing yet");
        q.holder.as_mut().unwrap().install_seen = true;
        assert!(q.stale(t(6), 60, in_progress, false).is_none(), "still installing");
        assert_eq!(q.stale(t(7), 60, in_progress, true), Some(Ended::Installed));
    }

    #[test]
    fn an_install_that_stops_asking_or_never_starts_lets_go() {
        let mut q = TestAppQueue::default();
        q.take(card(1), t(0), false);
        q.take(Ask::install("router", "wave 3"), t(0), false);
        assert_eq!(q.take(Ask::install("router", "wave 3"), t(9), false), Take::Waiting(1), "asking again keeps it in line");
        assert!(q.prune(t(18), in_progress).is_empty());
        let gone = q.prune(t(20), in_progress);
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].1, Ended::GaveUp);
        let mut q = TestAppQueue::default();
        q.take(Ask::install("router", "wave 3"), t(0), false);
        assert!(q.stale(t(15), 60, in_progress, true).is_none());
        assert_eq!(q.stale(t(16), 60, in_progress, true), Some(Ended::NotStarted));
    }

    #[test]
    fn release_lets_go_of_the_turn_or_the_place() {
        let mut q = TestAppQueue::default();
        q.take(card(1), t(0), false);
        q.take(card(2), t(0), false);
        q.take(card(3), t(0), false);
        assert_eq!(q.release(Some("card-2"), "2", TESTER), Some(false));
        assert_eq!(q.release(Some("card-1"), "1", TESTER), Some(true));
        assert_eq!(q.release(Some("card-1"), "1", TESTER), None);
        assert_eq!(line(&q), ["#3"]);
        assert_eq!(q.next(t(1), ALL_LIVE, FREE).map(|h| h.who.label), Some("#3".into()));
        // An outside install is released by its owner only.
        let mut q = TestAppQueue::default();
        q.take(Ask::install("router", "wave 3"), t(0), false);
        assert_eq!(q.release(None, "hugh", INSTALL), None);
        assert_eq!(q.release(Some("card-1"), "router", TESTER), None, "a card is never the outside owner");
        assert_eq!(q.release(None, "router", INSTALL), Some(true));
    }

    #[test]
    fn force_takes_it_from_whoever_holds_it() {
        let mut q = TestAppQueue::default();
        q.take(card(1), t(0), false);
        q.take(card(2), t(0), false);
        assert_eq!(q.force().map(|h| h.who.label), Some("#1".into()));
        assert!(q.force().is_none());
        assert_eq!(line(&q), ["#2"]);
    }

    #[test]
    fn next_passes_over_paused_cards_who_keep_their_place() {
        let mut q = TestAppQueue::default();
        q.take(card(1), t(0), false);
        q.take(card(2), t(0), false);
        q.take(card(3), t(0), false);
        q.force();
        let live = |id: &str| id != "card-2";
        assert_eq!(q.next(t(1), live, FREE).map(|h| h.who.label), Some("#3".into()));
        assert_eq!(line(&q), ["#2"]);
        q.force();
        assert!(q.next(t(2), live, FREE).is_none(), "the only waiting card is paused");
        assert_eq!(q.next(t(3), ALL_LIVE, FREE).map(|h| h.who.label), Some("#2".into()));
    }

    #[test]
    fn the_old_dsh_lock_folder_blocks_every_turn_but_its_owner_s() {
        let mut q = TestAppQueue::default();
        assert_eq!(q.take(card(1), t(0), true), Take::Waiting(1));
        assert!(q.next(t(1), ALL_LIVE, |owner| owner != "105").is_none());
        q.take(card(105), t(1), false);
        assert_eq!(line(&q), ["#1", "#105"]);
        assert_eq!(q.next(t(2), ALL_LIVE, |owner| owner != "105").map(|h| h.who.label), Some("#105".into()));
        q.force();
        assert_eq!(q.next(t(3), ALL_LIVE, FREE).map(|h| h.who.label), Some("#1".into()));
    }

    #[test]
    fn a_card_turn_ends_when_the_card_leaves_or_is_archived() {
        let mut q = TestAppQueue::default();
        q.take(card(1), t(0), false);
        assert!(q.stale(t(1), 60, in_progress, false).is_none());
        assert!(q.stale(t(1), 60, |_| Some(("Human test".into(), false)), false).is_none(), "Human test keeps it");
        assert_eq!(q.stale(t(1), 60, |_| Some(("Blocked".into(), false)), false), Some(Ended::Left("Blocked".into())));
        assert_eq!(q.stale(t(1), 60, |_| Some(("In progress".into(), true)), false), Some(Ended::Archived));
        assert_eq!(q.stale(t(1), 60, |_| None, false), Some(Ended::Gone));
    }

    #[test]
    fn any_turn_times_out_after_the_limit_without_use() {
        let mut q = TestAppQueue::default();
        q.take(card(1), t(0), false);
        assert!(q.stale(t(60), 60, in_progress, false).is_none());
        assert_eq!(q.stale(t(61), 60, in_progress, false), Some(Ended::TimedOut(60)));
        q.renew(t(61));
        assert!(q.stale(t(100), 60, in_progress, false).is_none(), "use renews it");
        assert_eq!(q.stale(t(100), 30, in_progress, false), Some(Ended::TimedOut(30)), "the limit is a setting");
    }

    #[test]
    fn prune_drops_cards_that_can_t_use_a_turn_and_keeps_installs() {
        let mut q = TestAppQueue::default();
        q.take(card(1), t(0), false);
        q.take(card(2), t(0), false);
        q.take(card(3), t(0), false);
        q.take(Ask::install("router", "wave 3"), t(0), false);
        let gone = q.prune(t(0), |id| match id {
            "card-2" => Some(("Blocked".into(), false)),
            "card-3" => Some(("In progress".into(), true)),
            _ => Some(("In progress".into(), false)),
        });
        assert_eq!(gone.iter().map(|(w, _)| w.who.label.clone()).collect::<Vec<_>>(), ["#2", "#3"]);
        assert_eq!(line(&q), ["wave 3"]);
        assert_eq!(holder(&q).as_deref(), Some("#1"), "prune leaves the holder to stale");
    }

    #[test]
    fn overdue_needs_45_minutes_and_someone_waiting() {
        let mut q = TestAppQueue::default();
        q.take(card(1), t(0), false);
        assert!(!q.overdue(t(50)), "nobody waits");
        q.take(card(2), t(1), false);
        assert!(!q.overdue(t(44)));
        assert!(q.overdue(t(45)));
        q.renew(t(45));
        assert!(q.overdue(t(46)), "counted from when it was given, not renewed");
    }

    #[test]
    fn refusals_name_the_columns() {
        assert!(take_refused("In progress", false).is_none());
        assert!(take_refused("Human test", false).is_none());
        let why = take_refused("Blocked", false).unwrap();
        assert!(why.contains("In progress or Human test") && why.contains("Blocked"), "{why}");
        assert!(take_refused("In progress", true).is_some());
    }

    #[test]
    fn limit_parsing() {
        assert_eq!(parse_limit(None), 60);
        assert_eq!(parse_limit(Some(" 90 ")), 90);
        assert_eq!(parse_limit(Some("1")), MIN_LIMIT);
        assert_eq!(parse_limit(Some("9999")), MAX_LIMIT);
        assert_eq!(parse_limit(Some("soon")), 60);
    }

    #[test]
    fn order_and_turns_survive_json_and_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.db");
        let mut s = Store::open(&path).unwrap();
        assert!(s.test_app_queue().unwrap().is_empty());
        let mut q = TestAppQueue::default();
        q.take(card(1), t(0), false);
        q.take(card(2), t(0), false);
        q.take(Ask::install("router", "wave 3"), t(0), false);
        q.holder.as_mut().unwrap().install_seen = true;
        s.save_test_app_queue(&q).unwrap();
        assert_eq!(s.test_app_limit().unwrap(), 60);
        assert_eq!(s.set_test_app_limit(90).unwrap(), 90);
        assert!(s.set_test_app_limit(5).is_err());
        drop(s);
        let mut s = Store::open(&path).unwrap();
        assert_eq!(s.test_app_queue().unwrap(), q);
        assert_eq!(s.test_app_limit().unwrap(), 90);
        let json = s.setting(QUEUE_KEY).unwrap().unwrap();
        assert!(json.contains("\"cardId\"") && json.contains("\"renewedAt\""), "{json}");
        s.set_setting(QUEUE_KEY, "not json").unwrap();
        assert!(s.test_app_queue().unwrap().is_empty());
    }

    // --- card #147 ---

    #[test]
    fn slot_names_and_keys() {
        assert_eq!(queue_key(1), "testapp.queue", "Test keeps #115's key");
        assert_eq!(queue_key(4), "testapp.queue.4");
        assert_eq!((slot_channel(1), slot_label(1)), ("test".into(), "Test".into()));
        assert_eq!((slot_channel(10), slot_label(10)), ("test10".into(), "Test 10".into()));
        assert_eq!(channel_slot("test"), Some(1));
        assert_eq!(channel_slot("test7"), Some(7));
        assert_eq!(channel_slot("live"), None);
        assert_eq!(channel_slot("test1"), None);
        let v = test_app_value(3, t(0));
        assert_eq!(v["port"], 9336);
        assert_eq!(v["since"], "2026-11-06T03:00:00Z");
    }

    #[test]
    fn the_lowest_free_slot_skips_taken_and_own_and_stops_at_the_ceiling() {
        let taken: BTreeSet<u32> = [1, 2, 3].into_iter().collect();
        assert_eq!(lowest_free(10, 0, &taken), Some(4), "a new test app when the lower ones are taken");
        assert_eq!(lowest_free(10, 4, &taken), Some(5), "never this app's own slot");
        assert_eq!(lowest_free(3, 0, &taken), None, "at the ceiling");
        assert_eq!(lowest_free(10, 0, &BTreeSet::new()), Some(1));
        assert_eq!(lowest_free(99, 0, &(1..=10).collect()), None, "never past test10");
    }

    #[test]
    fn slots_setting_parsing() {
        assert_eq!(parse_slots(None), 10);
        assert_eq!(parse_slots(Some(" 3 ")), 3);
        assert_eq!(parse_slots(Some("0")), 1);
        assert_eq!(parse_slots(Some("50")), 10);
        assert_eq!(parse_slots(Some("lots")), 10);
    }

    #[test]
    fn the_slot_line_keeps_places_and_survives_json() {
        let mut l = SlotLine::default();
        assert_eq!(l.join("a", 1, t(0)), 1);
        assert_eq!(l.join("b", 2, t(1)), 2);
        assert_eq!(l.join("a", 1, t(2)), 1, "joining again keeps the place");
        assert!(l.leave("a"));
        assert!(!l.leave("a"));
        assert_eq!(l.place("b"), Some(1));
        let back: SlotLine = serde_json::from_str(&serde_json::to_string(&l).unwrap()).unwrap();
        assert_eq!(back, l);
    }
}
