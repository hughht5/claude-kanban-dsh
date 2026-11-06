-- Migration 14 (card #119, 6 Nov 2026): the Router, one long-running Claude Code session the
-- app owns for the whole board. It is not a card: it has its own session record and its own
-- append-only log, so board lists, search, archive, import and the card tools never see it.
-- An older build opens a board at this version as it is and never reads these tables.
CREATE TABLE IF NOT EXISTS router_sessions (
  seq                INTEGER PRIMARY KEY AUTOINCREMENT,
  id                 TEXT NOT NULL UNIQUE,
  claude_session_id  TEXT NOT NULL,
  channel            TEXT NOT NULL,
  account            TEXT NOT NULL,
  cwd                TEXT NOT NULL,
  model              TEXT,
  effort             TEXT,
  started_at         TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS router_sessions_no_update BEFORE UPDATE ON router_sessions
  BEGIN SELECT RAISE(ABORT, 'router_sessions is append-only'); END;
CREATE TRIGGER IF NOT EXISTS router_sessions_no_delete BEFORE DELETE ON router_sessions
  BEGIN SELECT RAISE(ABORT, 'router_sessions is append-only'); END;

-- kind: decision (the Router's own), event (what the app told it), session (start, stop,
-- resume), note (anything else). card: the number of the card it is about, if any.
CREATE TABLE IF NOT EXISTS router_log (
  seq   INTEGER PRIMARY KEY AUTOINCREMENT,
  at    TEXT NOT NULL,
  kind  TEXT NOT NULL,
  card  INTEGER,
  text  TEXT NOT NULL,
  by    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS router_log_card ON router_log (card, seq);
CREATE TRIGGER IF NOT EXISTS router_log_no_update BEFORE UPDATE ON router_log
  BEGIN SELECT RAISE(ABORT, 'router_log is append-only'); END;
CREATE TRIGGER IF NOT EXISTS router_log_no_delete BEFORE DELETE ON router_log
  BEGIN SELECT RAISE(ABORT, 'router_log is append-only'); END;
