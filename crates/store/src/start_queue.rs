//! The cap on card sessions running at once, and the starts waiting under it.
//!
//! The cap is the setting `sessions.max_running` (10 when unset). A start (new or resume,
//! by Hugh, after a move, or the restart recovery) that would go over it waits in the queue,
//! kept as JSON in the setting `sessions.start_queue`, and starts oldest-first once a running
//! session ends or is suspended. The queue logic here is pure, so it is tested without a PTY.

use crate::{now, Error, Result, Store};
use serde::{Deserialize, Serialize};

pub const CAP_KEY: &str = "sessions.max_running";
pub const QUEUE_KEY: &str = "sessions.start_queue";
pub const DEFAULT_CAP: usize = 10;
/// The highest cap the app takes.
pub const MAX_CAP: usize = 100;

/// One start waiting for a free place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaitingStart {
    pub card_id: String,
    /// "new" or "resume".
    pub mode: String,
    /// Who asked: "hugh", "move" or "restart" (a session that was mid-turn when the app stopped).
    pub why: String,
    pub queued_at: String,
    /// Increases with each new entry, so the queue keeps its order however it is stored.
    pub seq: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StartQueue {
    pub items: Vec<WaitingStart>,
    pub next_seq: u64,
}

impl StartQueue {
    /// Whether a start can go now: fewer than `cap` sessions running.
    pub fn may_start(running: usize, cap: usize) -> bool {
        running < cap
    }

    /// Whether a start for `card_id` goes now rather than into the queue: there is a place,
    /// and no other card is waiting for one (they are older, so they go first).
    pub fn starts_now(&self, card_id: &str, running: usize, cap: usize) -> bool {
        Self::may_start(running, cap) && self.items.iter().all(|w| w.card_id == card_id)
    }

    /// Adds a start for `card_id`, or updates the card's waiting one in place (it keeps its
    /// place in line; the newest mode wins, and a restart stays a restart). Returns its
    /// place, from 1.
    pub fn push(&mut self, card_id: &str, mode: &str, why: &str, at: &str) -> usize {
        if let Some(i) = self.items.iter().position(|w| w.card_id == card_id) {
            let w = &mut self.items[i];
            w.mode = mode.to_string();
            if w.why != "restart" {
                w.why = why.to_string();
            }
            return i + 1;
        }
        self.next_seq += 1;
        self.items.push(WaitingStart {
            card_id: card_id.to_string(),
            mode: mode.to_string(),
            why: why.to_string(),
            queued_at: at.to_string(),
            seq: self.next_seq,
        });
        self.items.sort_by_key(|w| w.seq);
        self.items.len()
    }

    pub fn remove(&mut self, card_id: &str) -> Option<WaitingStart> {
        let i = self.items.iter().position(|w| w.card_id == card_id)?;
        Some(self.items.remove(i))
    }

    /// The card's place in line, from 1.
    pub fn position(&self, card_id: &str) -> Option<usize> {
        self.items.iter().position(|w| w.card_id == card_id).map(|i| i + 1)
    }

    /// Takes the starts to make now, oldest first: as many as there are free places.
    pub fn take(&mut self, running: usize, cap: usize) -> Vec<WaitingStart> {
        let free = cap.saturating_sub(running).min(self.items.len());
        self.items.sort_by_key(|w| w.seq);
        self.items.drain(..free).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// A stored cap as a number: unset or unreadable is the default, and it is kept within 1..=MAX_CAP.
pub fn parse_cap(v: Option<&str>) -> usize {
    v.and_then(|s| s.trim().parse::<usize>().ok()).map(|n| n.clamp(1, MAX_CAP)).unwrap_or(DEFAULT_CAP)
}

impl Store {
    pub fn session_cap(&self) -> Result<usize> {
        Ok(parse_cap(self.setting(CAP_KEY)?.as_deref()))
    }

    pub fn set_session_cap(&mut self, cap: usize) -> Result<usize> {
        if !(1..=MAX_CAP).contains(&cap) {
            return Err(Error::Invalid(format!("the cap must be between 1 and {MAX_CAP}")));
        }
        self.set_setting(CAP_KEY, &cap.to_string())?;
        Ok(cap)
    }

    /// The waiting starts. A queue that can't be read counts as empty.
    pub fn start_queue(&self) -> Result<StartQueue> {
        Ok(self.setting(QUEUE_KEY)?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default())
    }

    pub fn save_start_queue(&mut self, q: &StartQueue) -> Result<()> {
        self.set_setting(QUEUE_KEY, &serde_json::to_string(q)?)
    }

    /// Queues a start for `card_id` and returns its place in line, from 1.
    pub fn queue_start(&mut self, card_id: &str, mode: &str, why: &str) -> Result<usize> {
        let mut q = self.start_queue()?;
        let pos = q.push(card_id, mode, why, &now());
        self.save_start_queue(&q)?;
        Ok(pos)
    }

    /// Takes the card's waiting start out of the queue, if it has one.
    pub fn unqueue_start(&mut self, card_id: &str) -> Result<Option<WaitingStart>> {
        let mut q = self.start_queue()?;
        let w = q.remove(card_id);
        if w.is_some() {
            self.save_start_queue(&q)?;
        }
        Ok(w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q_with(cards: &[&str]) -> StartQueue {
        let mut q = StartQueue::default();
        for c in cards {
            q.push(c, "new", "hugh", "t");
        }
        q
    }

    #[test]
    fn cap_is_a_strict_upper_bound() {
        assert!(StartQueue::may_start(0, 10));
        assert!(StartQueue::may_start(9, 10));
        assert!(!StartQueue::may_start(10, 10));
        assert!(!StartQueue::may_start(11, 10));
        assert!(!StartQueue::may_start(0, 0));
    }

    #[test]
    fn a_start_waits_behind_older_ones_even_with_a_place_free() {
        let q = q_with(&["a"]);
        assert!(!q.starts_now("b", 5, 10), "b would jump ahead of a");
        assert!(q.starts_now("a", 5, 10), "a is the one waiting");
        assert!(!q.starts_now("a", 10, 10));
        assert!(StartQueue::default().starts_now("x", 9, 10));
    }

    #[test]
    fn push_keeps_one_per_card_and_its_place() {
        let mut q = q_with(&["a", "b", "c"]);
        assert_eq!(q.push("b", "resume", "move", "t2"), 2);
        assert_eq!(q.items.len(), 3);
        assert_eq!(q.items[1].mode, "resume");
        assert_eq!(q.items[1].why, "move");
        assert_eq!(q.items[1].queued_at, "t", "the first request's time is kept");
        assert_eq!(q.push("d", "new", "hugh", "t"), 4);
        assert_eq!(q.position("c"), Some(3));
        assert_eq!(q.position("zz"), None);
    }

    #[test]
    fn a_restart_stays_a_restart() {
        let mut q = StartQueue::default();
        q.push("a", "resume", "restart", "t");
        q.push("a", "resume", "hugh", "t");
        assert_eq!(q.items[0].why, "restart");
        q.push("b", "new", "hugh", "t");
        q.push("b", "resume", "restart", "t");
        assert_eq!(q.items[1].why, "restart");
    }

    #[test]
    fn take_is_oldest_first_and_only_as_many_as_free_places() {
        let mut q = q_with(&["a", "b", "c", "d"]);
        assert!(q.take(10, 10).is_empty());
        assert!(q.take(12, 10).is_empty(), "over the cap (it was lowered) takes nothing");
        let got: Vec<String> = q.take(8, 10).into_iter().map(|w| w.card_id).collect();
        assert_eq!(got, ["a", "b"]);
        assert_eq!(q.position("c"), Some(1));
        let got: Vec<String> = q.take(0, 10).into_iter().map(|w| w.card_id).collect();
        assert_eq!(got, ["c", "d"]);
        assert!(q.is_empty());
    }

    #[test]
    fn removed_cards_leave_the_line_and_a_re_queue_goes_to_the_back() {
        let mut q = q_with(&["a", "b", "c"]);
        assert_eq!(q.remove("a").map(|w| w.card_id), Some("a".into()));
        assert!(q.remove("a").is_none());
        assert_eq!(q.push("a", "new", "hugh", "t"), 3);
        let got: Vec<String> = q.take(0, 2).into_iter().map(|w| w.card_id).collect();
        assert_eq!(got, ["b", "c"]);
    }

    #[test]
    fn order_survives_a_round_trip_through_json() {
        let mut q = q_with(&["a", "b"]);
        q.remove("a");
        q.push("c", "resume", "restart", "t");
        let back: StartQueue = serde_json::from_str(&serde_json::to_string(&q).unwrap()).unwrap();
        assert_eq!(back, q);
        let mut back = back;
        assert_eq!(back.push("d", "new", "hugh", "t"), 3);
        assert!(back.items.windows(2).all(|w| w[0].seq < w[1].seq));
    }

    #[test]
    fn cap_parsing() {
        assert_eq!(parse_cap(None), 10);
        assert_eq!(parse_cap(Some("4")), 4);
        assert_eq!(parse_cap(Some(" 12 ")), 12);
        assert_eq!(parse_cap(Some("0")), 1);
        assert_eq!(parse_cap(Some("9999")), MAX_CAP);
        assert_eq!(parse_cap(Some("lots")), 10);
    }

    #[test]
    fn store_keeps_the_cap_and_queue() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        assert_eq!(s.session_cap().unwrap(), 10);
        assert_eq!(s.set_session_cap(3).unwrap(), 3);
        assert_eq!(s.setting(CAP_KEY).unwrap().as_deref(), Some("3"));
        assert!(s.set_session_cap(0).is_err());
        assert!(s.set_session_cap(MAX_CAP + 1).is_err());
        assert_eq!(s.session_cap().unwrap(), 3);
        assert_eq!(s.queue_start("a", "new", "hugh").unwrap(), 1);
        assert_eq!(s.queue_start("b", "resume", "move").unwrap(), 2);
        assert_eq!(s.queue_start("a", "resume", "hugh").unwrap(), 1);
        drop(s);
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let q = s.start_queue().unwrap();
        assert_eq!(q.items.iter().map(|w| w.card_id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(q.items[0].mode, "resume");
        assert!(s.unqueue_start("a").unwrap().is_some());
        assert!(s.unqueue_start("a").unwrap().is_none());
        assert_eq!(s.start_queue().unwrap().position("b"), Some(1));
        s.set_setting(QUEUE_KEY, "not json").unwrap();
        assert!(s.start_queue().unwrap().is_empty());
    }
}
