//! Card sessions: one row per `claude` process the app starts, and an event per
//! change of status or state. Both tables take inserts only; a session's status is its
//! latest event.

use crate::{now, Error, Result, Store};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

/// The setting holding the last session event the resume offer has been answered up to.
pub const RESUME_OFFER_KEY: &str = "app.resume_offer_after";

/// The hook states that mean a session was in the middle of a turn.
pub const MID_TURN: &[&str] = &["working", "permission"];

#[derive(Debug, Clone, Serialize)]
pub struct SessionRow {
    pub id: String,
    pub card_id: String,
    pub claude_session_id: String,
    pub account: String,
    pub cwd: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub started_at: String,
    pub status: Option<String>,
}

fn session_row(r: &rusqlite::Row) -> rusqlite::Result<SessionRow> {
    Ok(SessionRow {
        id: r.get("id")?,
        card_id: r.get("card_id")?,
        claude_session_id: r.get("claude_session_id")?,
        account: r.get("account")?,
        cwd: r.get("cwd")?,
        model: r.get("model")?,
        effort: r.get("effort")?,
        started_at: r.get("started_at")?,
        status: r.get("status")?,
    })
}

impl Store {
    pub fn add_session(&mut self, row: &SessionRow) -> Result<()> {
        if row.account != "main" && row.account != "account2" {
            return Err(Error::Invalid(format!("unknown account {}", row.account)));
        }
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO sessions (id, card_id, claude_session_id, account, cwd, model, effort, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![row.id, row.card_id, row.claude_session_id, row.account, row.cwd, row.model, row.effort, row.started_at],
        )?;
        tx.execute(
            "INSERT INTO session_events (session_id, at, status) VALUES (?1, ?2, 'running')",
            params![row.id, now()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn session_event(&mut self, session_id: &str, status: &str, detail: Option<&str>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO session_events (session_id, at, status, detail) VALUES (?1, ?2, ?3, ?4)",
            params![session_id, now(), status, detail],
        )?;
        Ok(())
    }

    /// The card's most recent session, with its latest status.
    pub fn latest_session(&self, card_id: &str) -> Result<Option<SessionRow>> {
        Ok(self
            .conn
            .query_row(
                "SELECT s.*, (SELECT status FROM session_events e WHERE e.session_id = s.id ORDER BY seq DESC LIMIT 1) AS status
                 FROM sessions s WHERE s.card_id = ?1 ORDER BY s.rowid DESC LIMIT 1",
                [card_id],
                session_row,
            )
            .optional()?)
    }

    /// One session row by its id (status left out).
    pub fn session(&self, id: &str) -> Result<Option<SessionRow>> {
        Ok(self.conn.query_row("SELECT s.*, NULL AS status FROM sessions s WHERE s.id = ?1", [id], session_row).optional()?)
    }

    /// Every session row, oldest first (archived cards' included), for the search indexer.
    /// Read-only; `status` is left out.
    pub fn all_sessions(&self) -> Result<Vec<SessionRow>> {
        let mut st = self.conn.prepare("SELECT s.*, NULL AS status FROM sessions s ORDER BY s.rowid")?;
        let rows = st.query_map([], session_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Sessions whose latest status is still `running`, for marking orphans after a crash.
    pub fn running_sessions(&self) -> Result<Vec<String>> {
        let mut st = self.conn.prepare(
            "SELECT s.id FROM sessions s WHERE (SELECT status FROM session_events e WHERE e.session_id = s.id ORDER BY seq DESC LIMIT 1) = 'running'",
        )?;
        let ids = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(ids)
    }
}

impl Store {
    /// The card a session row belongs to.
    pub fn session_card(&self, session_id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT card_id FROM sessions WHERE id = ?1", [session_id], |r| r.get(0))
            .optional()?)
    }

    pub fn session_account(&self, session_id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT account FROM sessions WHERE id = ?1", [session_id], |r| r.get(0))
            .optional()?)
    }
}

impl Store {
    /// The state a session last recorded from its hooks.
    pub fn last_state(&self, session_id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT state FROM session_events WHERE session_id = ?1 AND state IS NOT NULL ORDER BY seq DESC LIMIT 1",
                [session_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Records a running session's state (idle, working, permission, ...), but only when it
    /// differs from the state recorded last, so a session adds a row per change, not per hook.
    /// Returns whether a row was written.
    pub fn session_state(&mut self, session_id: &str, state: &str) -> Result<bool> {
        let status: Option<String> = self
            .conn
            .query_row(
                "SELECT status FROM session_events WHERE session_id = ?1 ORDER BY seq DESC LIMIT 1",
                [session_id],
                |r| r.get(0),
            )
            .optional()?;
        // A hook that arrives after the app stopped the session changes nothing.
        if status.as_deref() != Some("running") || self.last_state(session_id)?.as_deref() == Some(state) {
            return Ok(false);
        }
        self.conn.execute(
            "INSERT INTO session_events (session_id, at, status, state) VALUES (?1, ?2, 'running', ?3)",
            params![session_id, now(), state],
        )?;
        Ok(true)
    }

    /// At the app's start: sessions still marked running were ended by the Job Object when
    /// the app last stopped, so each gets an `orphaned` event saying whether it was mid-turn
    /// (its last recorded state was working or permission). Then returns the sessions that
    /// were mid-turn at the last stop, whether that was a clean Quit or a crash.
    pub fn begin_run(&mut self) -> Result<Vec<SessionRow>> {
        // The last event of the run before the one that just stopped: everything after it
        // belongs to that run, or is an orphan written now.
        let since: i64 = self.setting("app.run_started_seq")?.and_then(|v| v.parse().ok()).unwrap_or(0);
        for id in self.running_sessions()? {
            let detail = match self.last_state(&id)? {
                Some(s) if MID_TURN.contains(&s.as_str()) => format!("mid-turn ({s}): the app stopped while it ran"),
                _ => "idle: the app stopped while it ran".to_string(),
            };
            self.session_event(&id, "orphaned", Some(&detail))?;
        }
        // A board first opened by a build with the resume offer offers only what ran at the
        // last stop, not every idle stop in its history.
        if self.setting(RESUME_OFFER_KEY)?.is_none() {
            self.set_setting(RESUME_OFFER_KEY, &since.to_string())?;
        }
        let mid = self.mid_turn_after(since)?;
        let last: i64 = self.conn.query_row("SELECT COALESCE(MAX(seq), 0) FROM session_events", [], |r| r.get(0))?;
        self.set_setting("app.run_started_seq", &last.to_string())?;
        Ok(mid)
    }

    /// Each card's latest session whose latest event is a stop (suspended or orphaned) marked
    /// mid-turn, with an event number above `after`. A session stopped mid-turn in an earlier
    /// run and never resumed is left alone.
    pub fn mid_turn_after(&self, after: i64) -> Result<Vec<SessionRow>> {
        let mut st = self.conn.prepare(
            "SELECT s.*, e.status AS status FROM sessions s
             JOIN session_events e ON e.seq = (SELECT MAX(seq) FROM session_events x WHERE x.session_id = s.id)
             WHERE s.rowid = (SELECT MAX(rowid) FROM sessions y WHERE y.card_id = s.card_id)
               AND e.status IN ('suspended', 'orphaned') AND e.detail LIKE 'mid-turn%' AND e.seq > ?1
             ORDER BY s.rowid",
        )?;
        let rows = st.query_map([after], session_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
    /// The resume offer: each card's latest session whose latest event is a stop made while
    /// it was running and idle when the app quit (`suspended`, "idle") or crashed (`orphaned`,
    /// "idle: ..."), after the offer was last answered. Mid-turn stops resume by themselves,
    /// and stops by Hugh, Done, the idle pause or a board copy carry other details.
    pub fn resume_offer(&self) -> Result<Vec<SessionRow>> {
        let after: i64 = self.setting(RESUME_OFFER_KEY)?.and_then(|v| v.parse().ok()).unwrap_or(0);
        let mut st = self.conn.prepare(
            "SELECT s.*, e.status AS status FROM sessions s
             JOIN session_events e ON e.seq = (SELECT MAX(seq) FROM session_events x WHERE x.session_id = s.id)
             WHERE s.rowid = (SELECT MAX(rowid) FROM sessions y WHERE y.card_id = s.card_id)
               AND ((e.status = 'suspended' AND e.detail = 'idle')
                 OR (e.status = 'orphaned' AND e.detail LIKE 'idle:%'))
               AND e.seq > ?1
             ORDER BY s.rowid",
        )?;
        let rows = st.query_map([after], session_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Hugh answered the resume offer (Resume all or Not now): nothing stopped so far is offered
    /// again. A later quit offers its own sessions.
    pub fn answer_resume_offer(&mut self) -> Result<()> {
        let last: i64 = self.conn.query_row("SELECT COALESCE(MAX(seq), 0) FROM session_events", [], |r| r.get(0))?;
        self.set_setting(RESUME_OFFER_KEY, &last.to_string())
    }
}
