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

use crate::{Error, Result, Store};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

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

    /// Asks for a turn, with every card in line taken as live and `blocked` the same for
    /// everyone: `take_now` with fixed answers.
    pub fn take(&mut self, ask: Ask, now: DateTime<Utc>, blocked: bool) -> Take {
        self.take_now(ask, now, |_| true, |_| blocked)
    }

    /// Asks for a turn. The asker gets it now when nobody holds it, `blocked(owner)` is false
    /// for it (nobody else has the test app outside this queue), and nobody in line ahead of
    /// it could be given it now: the same rule as `next`, so an install ahead always counts,
    /// a card ahead only while its session is live (`live`), and a waiter that `blocked` stops
    /// doesn't. That holds when it asks again from its place in line too (card #149), so a
    /// card first in line, or behind only paused cards, gets the turn on its own take.
    /// Otherwise it goes in line: an install in front of every tester (behind earlier
    /// installs), a tester at the back. Asking again keeps the place, or renews a held turn.
    pub fn take_now(&mut self, ask: Ask, now: DateTime<Utc>, live: impl Fn(&str) -> bool, blocked: impl Fn(&str) -> bool) -> Take {
        let at = stamp(now);
        if let Some(h) = self.holder.as_mut().filter(|h| h.who.same(&ask.kind, ask.card_id.as_deref(), &ask.owner)) {
            h.renewed_at = at;
            return Take::Renewed;
        }
        let place = self.place(ask.card_id.as_deref(), &ask.owner, &ask.kind);
        // Where it stands, or would stand: an install behind the installs, a tester at the back.
        let i = place.map(|p| p - 1).unwrap_or_else(|| if ask.kind == INSTALL { self.line.iter().take_while(|x| x.who.kind == INSTALL).count() } else { self.line.len() });
        let ahead_can = self.line[..i].iter().any(|w| w.who.card_id.as_deref().map_or(true, &live) && !blocked(&w.who.owner));
        if self.holder.is_none() && !ahead_can && !blocked(&ask.owner) {
            if place.is_some() {
                self.line.remove(i);
            }
            self.holder = Some(Turn { who: ask, given_at: at.clone(), renewed_at: at, install_seen: false });
            return Take::Given;
        }
        if let Some(p) = place {
            self.line[p - 1].seen_at = Some(at);
            return Take::Waiting(p);
        }
        self.next_seq += 1;
        let w = Waiting { who: ask, asked_at: at.clone(), seq: self.next_seq, seen_at: Some(at) };
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

    /// Card #149: #131 sat first in line with nobody holding the turn, and asking again only
    /// kept its place. Now its own take gives it the turn.
    #[test]
    fn a_card_first_in_line_with_no_holder_gets_it_on_its_own_take() {
        let mut q = TestAppQueue::default();
        assert_eq!(q.take(card(131), t(0), true), Take::Waiting(1), "blocked when it first asked");
        assert_eq!(q.take(card(131), t(1), true), Take::Waiting(1), "still blocked: keeps its place");
        assert_eq!(q.take(card(131), t(2), false), Take::Given);
        assert_eq!(holder(&q).as_deref(), Some("#131"));
        assert!(q.line.is_empty(), "it left the line for the turn");
        assert_eq!(q.holder.as_ref().unwrap().given_at, stamp(t(2)));
    }

    /// Card #149's second form: #126 sat first in line with no session for days, and #131
    /// stayed number 2 behind it. A paused card ahead is passed over, as `next` does, and
    /// keeps its place.
    #[test]
    fn a_paused_card_ahead_doesn_t_hold_up_a_live_card_s_take() {
        let mut q = TestAppQueue::default();
        q.take(card(126), t(0), true);
        let live = |id: &str| id != "card-126";
        assert_eq!(q.take_now(card(131), t(1), live, FREE), Take::Given);
        assert_eq!(line(&q), ["#126"], "the paused card keeps its place");
        // Asking again from second place, with the paused card still ahead.
        let mut q = TestAppQueue::default();
        q.take(card(126), t(0), true);
        q.take(card(131), t(0), true);
        assert_eq!(q.take_now(card(131), t(1), live, FREE), Take::Given);
        assert_eq!(line(&q), ["#126"]);
        // Once the paused card is live again, it is ahead and a new take waits behind it.
        let mut q = TestAppQueue::default();
        q.take(card(126), t(0), true);
        assert_eq!(q.take_now(card(131), t(1), ALL_LIVE, FREE), Take::Waiting(2));
    }

    #[test]
    fn a_take_still_waits_for_a_holder_a_live_card_ahead_an_install_or_a_block() {
        // A holder.
        let mut q = TestAppQueue::default();
        q.take(card(1), t(0), false);
        assert_eq!(q.take_now(card(2), t(1), ALL_LIVE, FREE), Take::Waiting(1));
        assert_eq!(q.take_now(card(2), t(2), ALL_LIVE, FREE), Take::Waiting(1));
        // An install in line goes first, live or not (it has no session), even paused cards'.
        let mut q = TestAppQueue::default();
        q.take(Ask::install("router", "wave 3"), t(0), true);
        assert_eq!(q.take_now(card(2), t(1), |_| false, FREE), Take::Waiting(2));
        // Someone outside the queue (an install holding install.lock, a fresh DSH folder).
        let mut q = TestAppQueue::default();
        assert_eq!(q.take_now(card(2), t(1), ALL_LIVE, |o| o != "hugh"), Take::Waiting(1));
        assert_eq!(q.take_now(card(2), t(2), ALL_LIVE, |o| o != "hugh"), Take::Waiting(1));
        assert!(q.holder.is_none());
        // A waiter ahead that the block stops (it isn't the block's owner) doesn't count: the
        // block's own owner gets it, as `next` would give it.
        let mut q = TestAppQueue::default();
        q.take(card(1), t(0), true);
        assert_eq!(q.take_now(card(105), t(1), ALL_LIVE, |o| o != "105"), Take::Given);
        assert_eq!(line(&q), ["#1"]);
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
}
