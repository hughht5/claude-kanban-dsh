//! Column settings, and the queue of messages and held moves for card sessions.

use crate::{now, Result, Store};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ColumnCfg {
    pub name: String,
    pub prompt_text: Option<String>,
    /// "immediate", "deferred" or "none".
    pub mode: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permission_mode: Option<String>,
    pub auto_spawn: bool,
    /// "suspend" stops the card's session when the card enters.
    pub on_enter: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Pending {
    pub id: i64,
    pub card_id: String,
    pub kind: String,
    pub payload: String,
}

impl Store {
    pub fn column(&self, name: &str) -> Result<Option<ColumnCfg>> {
        Ok(self
            .conn
            .query_row(
                "SELECT name, prompt_text, mode, model, effort, permission_mode, auto_spawn, on_enter FROM columns WHERE name = ?1",
                [name],
                |r| {
                    Ok(ColumnCfg {
                        name: r.get(0)?,
                        prompt_text: r.get(1)?,
                        mode: r.get(2)?,
                        model: r.get(3)?,
                        effort: r.get(4)?,
                        permission_mode: r.get(5)?,
                        auto_spawn: r.get::<_, i64>(6)? != 0,
                        on_enter: r.get(7)?,
                    })
                },
            )
            .optional()?)
    }

    /// Queues `payload` for a card. A card holds at most one of each kind: a waiting
    /// `message` has the new text added after it, so everything goes in one message, and a
    /// waiting `move` (a held move) is replaced by the newer one.
    ///
    /// A `message` queued here is the app's own text (a column prompt, a notice): it is also
    /// recorded in `app_messages`, so the chat draws it apart from Hugh's words (card #153).
    /// Hugh's own words go through `queue_hugh`.
    pub fn queue(&mut self, card_id: &str, kind: &str, payload: &str) -> Result<i64> {
        self.queue_from(card_id, kind, payload, true)
    }

    /// `queue` for Hugh's own words (his message from the chat, his Fail note, his answer):
    /// not recorded as the app's. Also for a message put back in the queue, which was recorded
    /// (or not) when it was first queued.
    pub fn queue_hugh(&mut self, card_id: &str, kind: &str, payload: &str) -> Result<i64> {
        self.queue_from(card_id, kind, payload, false)
    }

    fn queue_from(&mut self, card_id: &str, kind: &str, payload: &str, app: bool) -> Result<i64> {
        let tx = self.conn.transaction()?;
        if app && kind == "message" {
            crate::app_messages::record(&tx, card_id, payload)?;
        }
        let earlier: Option<String> = if kind == "message" {
            tx.query_row(
                "SELECT payload FROM pending WHERE card_id = ?1 AND kind = ?2 AND done_at IS NULL ORDER BY id DESC LIMIT 1",
                params![card_id, kind],
                |r| r.get(0),
            )
            .optional()?
        } else {
            None
        };
        let payload = match earlier {
            Some(e) => format!("{e}\n\n{payload}"),
            None => payload.to_string(),
        };
        tx.execute(
            "UPDATE pending SET done_at = ?1 WHERE card_id = ?2 AND kind = ?3 AND done_at IS NULL",
            params![format!("superseded {}", now()), card_id, kind],
        )?;
        tx.execute(
            "INSERT INTO pending (card_id, kind, payload, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![card_id, kind, payload, now()],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    pub fn waiting(&self, card_id: &str, kind: &str) -> Result<Option<Pending>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, card_id, kind, payload FROM pending WHERE card_id = ?1 AND kind = ?2 AND done_at IS NULL ORDER BY id DESC LIMIT 1",
                params![card_id, kind],
                |r| Ok(Pending { id: r.get(0)?, card_id: r.get(1)?, kind: r.get(2)?, payload: r.get(3)? }),
            )
            .optional()?)
    }

    pub fn done(&mut self, id: i64, how: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE pending SET done_at = ?1 WHERE id = ?2 AND done_at IS NULL",
            params![format!("{how} {}", now()), id],
        )?;
        Ok(())
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}
