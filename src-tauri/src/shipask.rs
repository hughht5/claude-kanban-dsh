//! An agent's request to ship a card for Hugh (card #184).
//!
//! No agent moves a card into Ship it. When Hugh asks one to ship a card for him, it asks the
//! app (`kanban_move_card` to Ship it with `for_hugh`, `kb move <n> "Ship it" --for-hugh "<why>"`
//! or `kb ship <n> --asked-by-hugh`). The app keeps the request here, shows Hugh "Ship #n?" in
//! its own window (a banner) and a notification, and only his click on Ship moves the card, as
//! his drag does. Nothing a session writes (a chat line, a log, a message) can stand in for the
//! click, so the request itself proves nothing and is safe to accept from any agent.
//!
//! One request per card, each with its own id. It covers only that card and only Ship it, is
//! used once, and expires after `TTL_MS`. A new request for the same card replaces the old one.
//! Kept in memory only: a restart forgets them, and the agent asks again.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;

/// How long Hugh has to click Ship: 10 minutes.
pub const TTL_MS: i64 = 10 * 60 * 1000;

#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub id: String,
    pub card: String,
    pub number: i64,
    pub title: String,
    /// Who asked: "card #12's session", "a session outside the app".
    pub by: String,
    pub reason: String,
    pub branch: String,
    pub pr: String,
    pub at: i64,
}

impl Ask {
    pub fn expires(&self) -> i64 {
        self.at + TTL_MS
    }
    pub fn to_json(&self) -> Value {
        json!({ "ask": self.id, "card": self.card, "number": self.number, "title": self.title, "by": self.by,
                "reason": self.reason, "branch": self.branch, "pr": self.pr, "at": self.at, "expires": self.expires() })
    }
}

#[derive(Default)]
pub struct ShipAsks(Mutex<HashMap<String, Ask>>);

impl ShipAsks {
    /// Records `ask`, replacing any earlier request for the same card.
    pub fn put(&self, ask: Ask) {
        if let Ok(mut m) = self.0.lock() {
            m.insert(ask.card.clone(), ask);
        }
    }

    /// The requests still open at `now`, oldest first.
    pub fn open(&self, now: i64) -> Vec<Ask> {
        let Ok(mut m) = self.0.lock() else { return vec![] };
        m.retain(|_, a| a.expires() > now);
        let mut v: Vec<Ask> = m.values().cloned().collect();
        v.sort_by_key(|a| a.at);
        v
    }

    /// Hugh's click on Ship: takes request `id` for card `card`, once. Refused when there is no
    /// such request, it is for another card, it was used or replaced, or it expired.
    pub fn take(&self, card: &str, id: &str, now: i64) -> Result<Ask, String> {
        let mut m = self.0.lock().map_err(|e| e.to_string())?;
        let a = m.get(card).ok_or("there is no open request to ship that card (it was used, dismissed, or the app restarted): ask the agent to ask again")?;
        if a.id != id {
            return Err("that request was replaced by a newer one for the same card: use the newest banner".into());
        }
        if a.expires() <= now {
            m.remove(card);
            return Err("that request expired (they last 10 minutes): ask the agent to ask again".into());
        }
        Ok(m.remove(card).expect("checked above"))
    }

    /// Puts back a request taken for a move that then failed, unless a newer one replaced it.
    pub fn restore(&self, ask: Ask) {
        if let Ok(mut m) = self.0.lock() {
            m.entry(ask.card.clone()).or_insert(ask);
        }
    }

    /// Hugh's Dismiss: drops request `id` for `card`. True if it was open.
    pub fn dismiss(&self, card: &str, id: &str) -> Option<Ask> {
        let mut m = self.0.lock().ok()?;
        if m.get(card).map_or(false, |a| a.id == id) {
            m.remove(card)
        } else {
            None
        }
    }
}

/// A new request's id: unique enough within one run of the app.
pub fn new_id(now: i64) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(1);
    format!("ship-{now}-{}", N.fetch_add(1, Ordering::Relaxed))
}

/// What a refused move into Ship it tells the agent: how to ask Hugh.
pub const HOW_TO_ASK: &str = "only Hugh moves a card into Ship it (or the app for him: Take it to, or small-change auto-ship). If Hugh asked you to ship this card for him, ask him to confirm: call kanban_move_card again with column \"Ship it\", `for_hugh: true` and his words as `reason` (in Bash: kb move <n> \"Ship it\" --for-hugh \"<his words>\"). The app shows him \"Ship #n?\" and only his click on Ship moves it. Otherwise say in nextStep that it is ready";

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(card: &str, id: &str, at: i64) -> Ask {
        Ask { id: id.into(), card: card.into(), number: 7, title: "t".into(), by: "card #7's session".into(), reason: "Hugh said ship it".into(), branch: "card-7".into(), pr: String::new(), at }
    }

    #[test]
    fn without_a_request_nothing_is_approved() {
        let s = ShipAsks::default();
        assert!(s.take("card-7", "ship-1", 0).is_err());
    }

    #[test]
    fn a_request_is_approved_once() {
        let s = ShipAsks::default();
        s.put(ask("card-7", "ship-1", 1_000));
        assert_eq!(s.take("card-7", "ship-1", 2_000).unwrap().id, "ship-1");
        let again = s.take("card-7", "ship-1", 2_001).unwrap_err();
        assert!(again.contains("no open request"), "{again}");
    }

    #[test]
    fn an_expired_request_is_refused() {
        let s = ShipAsks::default();
        s.put(ask("card-7", "ship-1", 1_000));
        let e = s.take("card-7", "ship-1", 1_000 + TTL_MS).unwrap_err();
        assert!(e.contains("expired"), "{e}");
        assert!(s.take("card-7", "ship-1", 1_001).is_err(), "gone once expired");
        assert!(s.open(1_000 + TTL_MS).is_empty());
    }

    #[test]
    fn a_request_covers_only_its_own_card() {
        let s = ShipAsks::default();
        s.put(ask("card-7", "ship-1", 1_000));
        assert!(s.take("card-8", "ship-1", 1_001).is_err(), "another card");
        assert!(s.take("card-7", "ship-1", 1_002).is_ok(), "still there for its own card");
    }

    #[test]
    fn a_newer_request_replaces_the_old_one() {
        let s = ShipAsks::default();
        s.put(ask("card-7", "ship-1", 1_000));
        s.put(ask("card-7", "ship-2", 1_500));
        assert!(s.take("card-7", "ship-1", 1_600).unwrap_err().contains("replaced"));
        assert!(s.take("card-7", "ship-2", 1_600).is_ok());
    }

    #[test]
    fn a_failed_move_puts_the_request_back_and_dismiss_drops_it() {
        let s = ShipAsks::default();
        s.put(ask("card-7", "ship-1", 1_000));
        let a = s.take("card-7", "ship-1", 1_100).unwrap();
        s.restore(a);
        assert_eq!(s.open(1_200).len(), 1);
        assert!(s.dismiss("card-7", "other").is_none());
        assert!(s.dismiss("card-7", "ship-1").is_some());
        assert!(s.open(1_300).is_empty());
    }

    #[test]
    fn the_refusal_says_how_to_ask() {
        assert!(HOW_TO_ASK.contains("for_hugh") && HOW_TO_ASK.contains("--for-hugh") && HOW_TO_ASK.contains("only his click"));
    }
}
