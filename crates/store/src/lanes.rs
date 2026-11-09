//! The merge lanes (card #160): one Claude Code session the app owns per listed repo, which
//! merges that repo's ships one at a time and keeps what it learnt across them. Not a card: its
//! sessions live in their own append-only table (migration 16), its switches are settings, and
//! its jobs are the cards' own `merge` records (`queued`, then `running`, then a verdict), so an
//! app restart loses nothing.

use crate::{cards::row_to_card, Card, Patch, Result, Store, MERGE_KEY};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The repos whose ships go through their merge lane: a JSON list of `MergeLaneRepo`, empty (the
/// default) for none, so nothing changes until Hugh lists a repo.
pub const MERGE_LANE_REPOS_KEY: &str = "sessions.merge_lane_repos";
/// The lanes' model and effort (default: card sessions' default model, effort high, as the
/// card-merger runs).
pub const LANE_MODEL_KEY: &str = "lanes.model";
pub const LANE_EFFORT_KEY: &str = "lanes.effort";
pub const LANE_MODEL_DEFAULT: &str = "claude-opus-5-5";
pub const LANE_EFFORT_DEFAULT: &str = "high";
/// The start of a lane's key in the app's live session map: `lane:<repo>`, repo lower case.
pub const LANE_KEY_PREFIX: &str = "lane:";

/// A `merge` record's states: a job waiting for its lane, one the lane works on, a verdict
/// (as the card-merger's), or a job dropped because its card left Ship it.
pub const MERGE_STATES: [&str; 6] = ["queued", "running", "passed", "failed", "unclear", "cancelled"];
/// The states of a job its lane still owes a verdict on.
pub const MERGE_WAITING: [&str; 2] = ["queued", "running"];

/// A repo whose ships go through its merge lane: its name as cards hold it (`owner/name`), and
/// the checkout its lane's worktree is made in (under `.claude\worktrees`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeLaneRepo {
    pub repo: String,
    pub checkout: String,
}

/// One start (new or resumed) of a repo's lane.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaneSession {
    pub seq: i64,
    pub id: String,
    pub repo: String,
    pub claude_session_id: String,
    pub channel: String,
    pub account: String,
    pub cwd: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub started_at: String,
}

/// The live-map key of `repo`'s lane.
pub fn lane_key(repo: &str) -> String {
    format!("{LANE_KEY_PREFIX}{}", repo.trim().to_ascii_lowercase())
}

/// The repo of a lane key, if it is one.
pub fn lane_repo(key: &str) -> Option<&str> {
    key.strip_prefix(LANE_KEY_PREFIX).filter(|r| !r.is_empty())
}

/// Checks the merge-lane repo list as the own-worktree list is checked: each repo `owner/name`
/// and listed once, each checkout an absolute path to a folder holding a git checkout.
pub fn check_merge_lane_repos(json: &str) -> Result<Vec<MergeLaneRepo>> {
    let rows: Vec<MergeLaneRepo> = serde_json::from_str(json)
        .map_err(|e| crate::Error::Invalid(format!("the merge-lane repo list should be a list of {{repo, checkout}}: {e}")))?;
    let mut out: Vec<MergeLaneRepo> = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        let (repo, checkout) = crate::config::check_repo_checkout(i + 1, &r.repo, &r.checkout, &out.iter().map(|o| o.repo.as_str()).collect::<Vec<_>>())?;
        out.push(MergeLaneRepo { repo, checkout });
    }
    Ok(out)
}

fn session_of(r: &rusqlite::Row) -> rusqlite::Result<LaneSession> {
    Ok(LaneSession {
        seq: r.get(0)?,
        id: r.get(1)?,
        repo: r.get(2)?,
        claude_session_id: r.get(3)?,
        channel: r.get(4)?,
        account: r.get(5)?,
        cwd: r.get(6)?,
        model: r.get(7)?,
        effort: r.get(8)?,
        started_at: r.get(9)?,
    })
}

const COLS: &str = "seq, id, repo, claude_session_id, channel, account, cwd, model, effort, started_at";

impl Store {
    /// Records a lane session start (new or resumed) for `repo` (kept lower case).
    #[allow(clippy::too_many_arguments)]
    pub fn add_lane_session(&mut self, id: &str, repo: &str, claude_session_id: &str, channel: &str, account: &str, cwd: &str, model: &str, effort: &str) -> Result<LaneSession> {
        let at = crate::now();
        let repo = repo.trim().to_ascii_lowercase();
        self.conn.execute(
            "INSERT INTO lane_sessions (id, repo, claude_session_id, channel, account, cwd, model, effort, started_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, repo, claude_session_id, channel, account, cwd, model, effort, at],
        )?;
        Ok(LaneSession {
            seq: self.conn.last_insert_rowid(),
            id: id.into(),
            repo,
            claude_session_id: claude_session_id.into(),
            channel: channel.into(),
            account: account.into(),
            cwd: cwd.into(),
            model: Some(model.into()),
            effort: Some(effort.into()),
            started_at: at,
        })
    }

    /// The newest session of `repo`'s lane on `channel` (any case): the conversation the next
    /// merge resumes. Rows carry their channel, so a board copy never resumes another channel's.
    pub fn latest_lane_session(&self, repo: &str, channel: &str) -> Result<Option<LaneSession>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLS} FROM lane_sessions WHERE repo = ?1 AND channel = ?2 ORDER BY seq DESC LIMIT 1"),
                params![repo.trim().to_ascii_lowercase(), channel],
                session_of,
            )
            .optional()?)
    }

    /// Whether `row_id` is one of the lanes' sessions (any repo, any channel).
    pub fn is_lane_row(&self, row_id: &str) -> Result<bool> {
        Ok(self.lane_of_row(row_id)?.is_some())
    }

    /// The repo (lower case) of the lane whose session `row_id` is.
    pub fn lane_of_row(&self, row_id: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT repo FROM lane_sessions WHERE id = ?1", [row_id], |r| r.get(0)).optional()?)
    }

    /// The repos whose ships go through their lane. A saved value that no longer parses counts
    /// as none: their ships then use the card-merger, as before.
    pub fn merge_lane_repos(&self) -> Vec<MergeLaneRepo> {
        self.setting(MERGE_LANE_REPOS_KEY).ok().flatten().and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
    }

    /// The merge-lane entry for `repo` (a card's `owner/name`, any case), if Hugh listed it.
    pub fn merge_lane_repo_for(&self, repo: &str) -> Option<MergeLaneRepo> {
        let repo = repo.trim();
        if repo.is_empty() {
            return None;
        }
        self.merge_lane_repos().into_iter().find(|r| r.repo.eq_ignore_ascii_case(repo))
    }

    /// The lanes' model and effort, from Hugh's settings.
    pub fn lane_model_effort(&self) -> (String, String) {
        let m = self.setting(LANE_MODEL_KEY).ok().flatten().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| LANE_MODEL_DEFAULT.into());
        let e = self.setting(LANE_EFFORT_KEY).ok().flatten().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| LANE_EFFORT_DEFAULT.into());
        (m, e)
    }

    /// How many jobs any lane owes a verdict on (`merge.state` queued or running): one cheap
    /// count, so a dispatcher pass with nothing waiting costs nothing more.
    pub fn lane_jobs_waiting(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM cards WHERE json_extract(fields, '$.merge.state') IN ('queued', 'running')",
            [],
            |r| r.get(0),
        )?)
    }

    /// The jobs (cards, archived ones too) that lane `key` owes a verdict on, oldest request
    /// first: the running one, then the queue.
    pub fn lane_jobs(&self, key: &str) -> Result<Vec<Card>> {
        let mut st = self.conn.prepare(
            "SELECT * FROM cards WHERE json_extract(fields, '$.merge.state') IN ('queued', 'running')
             AND json_extract(fields, '$.merge.lane') = ?1
             ORDER BY json_extract(fields, '$.merge.state') = 'running' DESC, json_extract(fields, '$.merge.at'), number",
        )?;
        let cards = st.query_map([key], row_to_card)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(cards)
    }

    /// Every lane's jobs at once (cards, archived ones too), grouped by lane, each lane's running
    /// job first, then its queue, oldest request first: one query for the dispatcher's pass.
    pub fn lane_jobs_all(&self) -> Result<Vec<Card>> {
        let mut st = self.conn.prepare(
            "SELECT * FROM cards WHERE json_extract(fields, '$.merge.state') IN ('queued', 'running')
             ORDER BY json_extract(fields, '$.merge.lane'), json_extract(fields, '$.merge.state') = 'running' DESC,
             json_extract(fields, '$.merge.at'), number",
        )?;
        let cards = st.query_map([], row_to_card)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(cards)
    }

    /// The last `n` finished merges (passed, failed or unclear) of `repo`'s cards, newest first,
    /// from the cards' `merge` records: the lane's history, given in every brief so it survives
    /// a compaction or a new conversation. `except` (a card id) is left out.
    pub fn recent_merges(&self, repo: &str, n: usize, except: Option<&str>) -> Result<Vec<Card>> {
        let mut st = self.conn.prepare(
            "SELECT * FROM cards WHERE lower(json_extract(fields, '$.repo')) = ?1
             AND json_extract(fields, '$.merge.state') IN ('passed', 'failed', 'unclear') AND id <> ?2
             ORDER BY json_extract(fields, '$.merge.at') DESC LIMIT ?3",
        )?;
        let cards = st
            .query_map(params![repo.trim().to_ascii_lowercase(), except.unwrap_or(""), n as i64], row_to_card)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(cards)
    }

    /// In a board copy: no lane job of the board it came from runs here. Each one waiting or
    /// running is cancelled, saying why (a lane on the copy would merge another board's card).
    pub(crate) fn detach_lane_jobs(&mut self) -> Result<usize> {
        let mut st = self.conn.prepare("SELECT * FROM cards WHERE json_extract(fields, '$.merge.state') IN ('queued', 'running')")?;
        let cards = st.query_map([], row_to_card)?.collect::<rusqlite::Result<Vec<_>>>()?;
        drop(st);
        for c in &cards {
            let mut rec = c.fields.get(MERGE_KEY).cloned().unwrap_or(json!({}));
            rec["state"] = json!("cancelled");
            rec["detail"] = json!("board copied: a merge-lane job of the board this came from never runs here");
            rec["at"] = json!(crate::now());
            let mut p = Patch::new();
            p.insert(MERGE_KEY.into(), rec);
            self.update(&c.id, c.version, &p, "the app")?;
        }
        Ok(cards.len())
    }
}

/// The state of a card's `merge` record, if it has one.
pub fn merge_state(card: &Card) -> Option<&str> {
    card.fields.get(MERGE_KEY).and_then(|m| m.get("state")).and_then(Value::as_str)
}
