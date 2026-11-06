//! Read and reply marks on cards (migration 5), and each card's latest session as the
//! "needs you" view reads it. Inserts only: a card's state is its newest mark of each kind.

use crate::{now, Error, Result, Store};
use rusqlite::params;
use std::collections::HashMap;

/// A card's newest reply (a turn its session ended) and newest read (Hugh opened its panel).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CardMarks {
    pub reply: Option<String>,
    pub read: Option<String>,
}

impl CardMarks {
    /// A reply Hugh hasn't opened the card since.
    pub fn unread(&self) -> bool {
        match (&self.reply, &self.read) {
            (Some(reply), Some(read)) => reply > read,
            (Some(_), None) => true,
            _ => false,
        }
    }

    /// Whether something that happened at `at` is newer than Hugh's last look at the card.
    pub fn after_read(&self, at: &str) -> bool {
        self.read.as_deref().map_or(true, |r| at > r)
    }
}

/// A card's most recent session: its latest event's status and time, and its latest
/// recorded hook state and when that was recorded.
#[derive(Debug, Clone)]
pub struct LatestSession {
    pub card_id: String,
    pub session_id: String,
    pub status: Option<String>,
    pub status_at: Option<String>,
    pub state: Option<String>,
    pub state_at: Option<String>,
}

impl Store {
    /// Records a `read` or `reply` mark on the card, stamped now. Returns the stamp.
    pub fn mark_card(&mut self, card_id: &str, kind: &str) -> Result<String> {
        if kind != "read" && kind != "reply" {
            return Err(Error::Invalid(format!("unknown mark {kind}")));
        }
        let at = now();
        self.conn.execute(
            "INSERT INTO card_marks (card_id, kind, at) VALUES (?1, ?2, ?3)",
            params![card_id, kind, at],
        )?;
        Ok(at)
    }

    /// Every card's newest reply and read.
    pub fn card_marks(&self) -> Result<HashMap<String, CardMarks>> {
        let mut st = self.conn.prepare(
            "SELECT card_id, kind, MAX(at) FROM card_marks GROUP BY card_id, kind",
        )?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
        let mut out: HashMap<String, CardMarks> = HashMap::new();
        for row in rows {
            let (card, kind, at) = row?;
            let m = out.entry(card).or_default();
            if kind == "reply" {
                m.reply = Some(at);
            } else {
                m.read = Some(at);
            }
        }
        Ok(out)
    }

    /// Each card's most recent session, with its latest status and state.
    pub fn latest_sessions(&self) -> Result<Vec<LatestSession>> {
        let mut st = self.conn.prepare(
            "SELECT s.card_id, s.id,
               (SELECT status FROM session_events e WHERE e.session_id = s.id ORDER BY seq DESC LIMIT 1),
               (SELECT at FROM session_events e WHERE e.session_id = s.id ORDER BY seq DESC LIMIT 1),
               (SELECT state FROM session_events e WHERE e.session_id = s.id AND state IS NOT NULL ORDER BY seq DESC LIMIT 1),
               (SELECT at FROM session_events e WHERE e.session_id = s.id AND state IS NOT NULL ORDER BY seq DESC LIMIT 1)
             FROM sessions s WHERE s.rowid = (SELECT MAX(rowid) FROM sessions y WHERE y.card_id = s.card_id)",
        )?;
        let rows = st
            .query_map([], |r| {
                Ok(LatestSession {
                    card_id: r.get(0)?,
                    session_id: r.get(1)?,
                    status: r.get(2)?,
                    status_at: r.get(3)?,
                    state: r.get(4)?,
                    state_at: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}
