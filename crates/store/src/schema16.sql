-- Migration 16 (card #160, 9 Nov 2026): the merge lanes, one long-lived Claude Code session the
-- app owns per listed repo (Settings › Sessions › "Ship through one merge lane per repo"), which
-- merges that repo's ships one at a time. Like the Router (migration 14) it is not a card: its
-- sessions have their own append-only table, one row per start or resume, so the next merge
-- resumes the repo's last conversation on this channel. repo is the cards' owner/name, lower case.
-- An older build opens a board at this version as it is and never reads this table.
CREATE TABLE IF NOT EXISTS lane_sessions (
  seq                INTEGER PRIMARY KEY AUTOINCREMENT,
  id                 TEXT NOT NULL UNIQUE,
  repo               TEXT NOT NULL,
  claude_session_id  TEXT NOT NULL,
  channel            TEXT NOT NULL,
  account            TEXT NOT NULL,
  cwd                TEXT NOT NULL,
  model              TEXT,
  effort             TEXT,
  started_at         TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS lane_sessions_repo ON lane_sessions (repo, channel, seq);
CREATE TRIGGER IF NOT EXISTS lane_sessions_no_update BEFORE UPDATE ON lane_sessions
  BEGIN SELECT RAISE(ABORT, 'lane_sessions is append-only'); END;
CREATE TRIGGER IF NOT EXISTS lane_sessions_no_delete BEFORE DELETE ON lane_sessions
  BEGIN SELECT RAISE(ABORT, 'lane_sessions is append-only'); END;
