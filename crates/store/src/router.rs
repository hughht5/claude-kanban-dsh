//! The Router (card #119): one long-running Claude Code session the app owns for the whole
//! board. It is not a card. Its sessions and its log live in their own append-only tables
//! (migration 14), and its switches are settings, so no board list, search, archive, import,
//! board copy or card tool ever shows or writes it.

use crate::{Result, Store};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

/// Hugh switched the Router on ("1"); his Stop clears it. At start-up the app resumes the
/// Router only while it is on.
pub const ROUTER_ON_KEY: &str = "router.on";
/// The Router's model (default Sonnet) and effort (default low).
pub const ROUTER_MODEL_KEY: &str = "router.model";
pub const ROUTER_EFFORT_KEY: &str = "router.effort";
/// May the Router start cards, which is a move from Backlog to Planning or `kanban_start_card`
/// (off unless Hugh turns it on)?
pub const ROUTER_MAY_START_KEY: &str = "router.may_start_cards";
/// Router sessions at or below this seq are not resumed here (a board copy sets it).
pub const ROUTER_RESUME_AFTER_KEY: &str = "router.resume_after";
/// When Hugh last opened the Router panel (for the digest). A setting, not a card field, so an
/// open writes no card revision.
pub const ROUTER_LAST_OPENED_KEY: &str = "router.last_opened";

pub const ROUTER_MODEL_DEFAULT: &str = "claude-sonnet-5-5";
pub const ROUTER_EFFORT_DEFAULT: &str = "low";
/// Who the Router's own writes are by, on cards and in its log.
pub const ROUTER_BY: &str = "the router";
/// The kinds of a Router log line.
pub const ROUTER_LOG_KINDS: [&str; 4] = ["decision", "event", "session", "note"];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterSession {
    pub seq: i64,
    pub id: String,
    pub claude_session_id: String,
    pub channel: String,
    pub account: String,
    pub cwd: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub started_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RouterLogLine {
    pub seq: i64,
    pub at: String,
    pub kind: String,
    pub card: Option<i64>,
    pub text: String,
    pub by: String,
}

/// One card write, as the Router's events read it from `card_revisions`.
#[derive(Debug, Clone, PartialEq)]
pub struct Revision {
    pub seq: i64,
    pub card_id: String,
    pub number: i64,
    pub version: i64,
    pub at: String,
    pub by: String,
    pub changed: Value,
}

impl Store {
    /// Records a Router session start (new or resumed).
    #[allow(clippy::too_many_arguments)]
    pub fn add_router_session(&mut self, id: &str, claude_session_id: &str, channel: &str, account: &str, cwd: &str, model: &str, effort: &str) -> Result<RouterSession> {
        let at = crate::now();
        self.conn.execute(
            "INSERT INTO router_sessions (id, claude_session_id, channel, account, cwd, model, effort, started_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, claude_session_id, channel, account, cwd, model, effort, at],
        )?;
        Ok(RouterSession {
            seq: self.conn.last_insert_rowid(),
            id: id.into(),
            claude_session_id: claude_session_id.into(),
            channel: channel.into(),
            account: account.into(),
            cwd: cwd.into(),
            model: Some(model.into()),
            effort: Some(effort.into()),
            started_at: at,
        })
    }

    /// The newest Router session of `channel` that may be resumed on this board: later than
    /// `router.resume_after` (a board copy sets it, so a copy never resumes the source's
    /// conversation).
    pub fn latest_router_session(&self, channel: &str) -> Result<Option<RouterSession>> {
        let after: i64 = self.setting(ROUTER_RESUME_AFTER_KEY)?.and_then(|v| v.parse().ok()).unwrap_or(0);
        Ok(self
            .conn
            .query_row(
                "SELECT seq, id, claude_session_id, channel, account, cwd, model, effort, started_at FROM router_sessions
                 WHERE channel = ?1 AND seq > ?2 ORDER BY seq DESC LIMIT 1",
                params![channel, after],
                |r| {
                    Ok(RouterSession {
                        seq: r.get(0)?,
                        id: r.get(1)?,
                        claude_session_id: r.get(2)?,
                        channel: r.get(3)?,
                        account: r.get(4)?,
                        cwd: r.get(5)?,
                        model: r.get(6)?,
                        effort: r.get(7)?,
                        started_at: r.get(8)?,
                    })
                },
            )
            .optional()?)
    }

    /// Whether `row_id` is one of the Router's sessions (any channel).
    pub fn is_router_row(&self, row_id: &str) -> Result<bool> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM router_sessions WHERE id = ?1", [row_id], |r| r.get::<_, i64>(0))? > 0)
    }

    /// Adds a line to the Router's log. Text that looks secret is refused, as on cards.
    pub fn add_router_log(&mut self, kind: &str, card: Option<i64>, text: &str, by: &str) -> Result<i64> {
        if !ROUTER_LOG_KINDS.contains(&kind) {
            return Err(crate::Error::Invalid(format!("a Router log line is one of {}, not {kind}", ROUTER_LOG_KINDS.join(", "))));
        }
        if crate::looks_secret(text) || crate::looks_like_token(text) {
            return Err(crate::Error::Secret("router log".into()));
        }
        self.conn.execute(
            "INSERT INTO router_log (at, kind, card, text, by) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![crate::now(), kind, card, text, by],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// The Router's log after `since` (a seq), newest first, at most `limit` lines.
    pub fn router_log(&self, since: i64, limit: usize) -> Result<Vec<RouterLogLine>> {
        let mut st = self.conn.prepare("SELECT seq, at, kind, card, text, by FROM router_log WHERE seq > ?1 ORDER BY seq DESC LIMIT ?2")?;
        let rows = st.query_map(params![since, limit as i64], |r| {
            Ok(RouterLogLine { seq: r.get(0)?, at: r.get(1)?, kind: r.get(2)?, card: r.get(3)?, text: r.get(4)?, by: r.get(5)? })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// The Router's log lines written at or after `at` (an RFC 3339 time), oldest first.
    pub fn router_log_since_time(&self, at: &str, limit: usize) -> Result<Vec<RouterLogLine>> {
        let mut st = self.conn.prepare("SELECT seq, at, kind, card, text, by FROM router_log WHERE at >= ?1 ORDER BY seq LIMIT ?2")?;
        let rows = st.query_map(params![at, limit as i64], |r| {
            Ok(RouterLogLine { seq: r.get(0)?, at: r.get(1)?, kind: r.get(2)?, card: r.get(3)?, text: r.get(4)?, by: r.get(5)? })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// The newest card revision's seq (0 for none): where the Router's events start.
    pub fn last_revision_seq(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COALESCE(MAX(seq), 0) FROM card_revisions", [], |r| r.get(0))?)
    }

    /// Card writes after `seq`, oldest first, at most `limit`: the one place the Router's
    /// events come from, so every move path (a drag, a tool, a held move, a stack rule, an
    /// import) is seen.
    pub fn revisions_since(&self, seq: i64, limit: usize) -> Result<Vec<Revision>> {
        let mut st = self.conn.prepare(
            "SELECT r.seq, r.card_id, c.number, r.version, r.at, COALESCE(r.by, ''), r.changed
             FROM card_revisions r JOIN cards c ON c.id = r.card_id WHERE r.seq > ?1 ORDER BY r.seq LIMIT ?2",
        )?;
        let rows = st.query_map(params![seq, limit as i64], |r| {
            let changed: String = r.get(6)?;
            Ok(Revision {
                seq: r.get(0)?,
                card_id: r.get(1)?,
                number: r.get(2)?,
                version: r.get(3)?,
                at: r.get(4)?,
                by: r.get(5)?,
                changed: serde_json::from_str(&changed).unwrap_or(Value::Null),
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// The column a card was in before revision `seq`, from its last earlier write of a column.
    pub fn column_before(&self, card_id: &str, seq: i64) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT json_extract(changed, '$.column') FROM card_revisions
                 WHERE card_id = ?1 AND seq < ?2 AND json_extract(changed, '$.column') IS NOT NULL ORDER BY seq DESC LIMIT 1",
                params![card_id, seq],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    /// The Router's model and effort, from Hugh's settings.
    pub fn router_model_effort(&self) -> (String, String) {
        let m = self.setting(ROUTER_MODEL_KEY).ok().flatten().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| ROUTER_MODEL_DEFAULT.into());
        let e = self.setting(ROUTER_EFFORT_KEY).ok().flatten().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| ROUTER_EFFORT_DEFAULT.into());
        (m, e)
    }

    /// Whether Hugh has the Router switched on.
    pub fn router_on(&self) -> bool {
        self.setting(ROUTER_ON_KEY).ok().flatten().as_deref() == Some("1")
    }

    /// Switches the Router on or off (Hugh's Start and Stop), logged in `settings_log`.
    pub fn set_router_on(&mut self, on: bool, by: &str) -> Result<()> {
        let v = if on { "1" } else { "0" };
        self.set_setting(ROUTER_ON_KEY, v)?;
        self.log_setting(ROUTER_ON_KEY, v, by)
    }

    /// Whether the Router may start cards (off unless Hugh turned it on).
    pub fn router_may_start(&self) -> bool {
        self.setting(ROUTER_MAY_START_KEY).ok().flatten().as_deref() == Some("on")
    }

    /// In a board copy: the source's Router is not resumed here, and is off.
    pub(crate) fn detach_router(&mut self) -> Result<()> {
        let last: i64 = self.conn.query_row("SELECT COALESCE(MAX(seq), 0) FROM router_sessions", [], |r| r.get(0))?;
        self.set_setting(ROUTER_RESUME_AFTER_KEY, &last.to_string())?;
        self.set_setting(ROUTER_ON_KEY, "0")?;
        if last > 0 {
            self.add_router_log("session", None, "Board copied: the Router of the board this came from is not resumed here. Start a new one with the Router button.", "the app")?;
        }
        Ok(())
    }
}

impl Store {
    /// A card's latest `n` writes as (by, at), newest first.
    pub fn revisions_by(&self, card_id: &str, n: usize) -> Result<Vec<(String, String)>> {
        let mut st = self.conn.prepare("SELECT COALESCE(by, ''), at FROM card_revisions WHERE card_id = ?1 ORDER BY seq DESC LIMIT ?2")?;
        let rows = st.query_map(params![card_id, n as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Card writes at or after `at` (an RFC 3339 time), oldest first, at most `limit`.
    pub fn revisions_since_time(&self, at: &str, limit: usize) -> Result<Vec<Revision>> {
        let seq: i64 = self.conn.query_row("SELECT COALESCE(MIN(seq), 0) - 1 FROM card_revisions WHERE at >= ?1", [at], |r| r.get(0))?;
        if seq < 0 {
            return Ok(vec![]);
        }
        self.revisions_since(seq, limit)
    }
}
