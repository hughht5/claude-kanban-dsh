-- Migration 1. Logs, revisions, sessions and session events take inserts only.
CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- Fields the board sorts and filters on are columns; every other card field
-- is kept in `fields` as JSON, in the artifact's own camelCase names.
CREATE TABLE cards (
  id          TEXT PRIMARY KEY,
  number      INTEGER NOT NULL UNIQUE,
  col         TEXT NOT NULL,
  ord         REAL,
  ord_col     TEXT,
  archived    INTEGER NOT NULL DEFAULT 0,
  fields      TEXT NOT NULL,
  created_at  TEXT,
  updated_at  TEXT,
  version     INTEGER NOT NULL
);

CREATE TABLE card_log (
  seq      INTEGER PRIMARY KEY AUTOINCREMENT,
  card_id  TEXT NOT NULL REFERENCES cards(id),
  at       TEXT NOT NULL,
  text     TEXT NOT NULL,
  by       TEXT
);
CREATE INDEX card_log_card ON card_log(card_id, seq);

CREATE TABLE card_revisions (
  seq      INTEGER PRIMARY KEY AUTOINCREMENT,
  card_id  TEXT NOT NULL REFERENCES cards(id),
  version  INTEGER NOT NULL,
  at       TEXT NOT NULL,
  by       TEXT,
  changed  TEXT NOT NULL
);

CREATE TABLE sessions (
  id                 TEXT PRIMARY KEY,
  card_id            TEXT NOT NULL REFERENCES cards(id),
  claude_session_id  TEXT NOT NULL,
  account            TEXT NOT NULL CHECK (account IN ('main', 'account2')),
  cwd                TEXT NOT NULL,
  model              TEXT,
  effort             TEXT,
  started_at         TEXT NOT NULL
);

CREATE TABLE session_events (
  seq         INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id  TEXT NOT NULL REFERENCES sessions(id),
  at          TEXT NOT NULL,
  status      TEXT NOT NULL,
  state       TEXT,
  detail      TEXT
);

CREATE TABLE pending (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  card_id     TEXT NOT NULL REFERENCES cards(id),
  session_id  TEXT,
  kind        TEXT NOT NULL CHECK (kind IN ('move', 'message')),
  payload     TEXT NOT NULL,
  created_at  TEXT NOT NULL,
  done_at     TEXT
);

CREATE TABLE columns (
  name             TEXT PRIMARY KEY,
  position         INTEGER NOT NULL,
  prompt_file      TEXT,
  prompt_text      TEXT,
  mode             TEXT NOT NULL DEFAULT 'none',
  model            TEXT,
  effort           TEXT,
  permission_mode  TEXT,
  auto_spawn       INTEGER NOT NULL DEFAULT 0,
  on_enter         TEXT NOT NULL DEFAULT 'none'
);

CREATE TABLE settings (
  key    TEXT PRIMARY KEY,
  value  TEXT NOT NULL
);

-- The append-only tables refuse changes at the database level too.
CREATE TRIGGER card_log_no_update BEFORE UPDATE ON card_log
  BEGIN SELECT RAISE(ABORT, 'card_log is append-only'); END;
CREATE TRIGGER card_revisions_no_update BEFORE UPDATE ON card_revisions
  BEGIN SELECT RAISE(ABORT, 'card_revisions is append-only'); END;
CREATE TRIGGER sessions_no_update BEFORE UPDATE ON sessions
  BEGIN SELECT RAISE(ABORT, 'sessions is append-only'); END;
CREATE TRIGGER session_events_no_update BEFORE UPDATE ON session_events
  BEGIN SELECT RAISE(ABORT, 'session_events is append-only'); END;

-- Nothing is ever removed: archiving is a flag.
CREATE TRIGGER cards_no_delete BEFORE DELETE ON cards
  BEGIN SELECT RAISE(ABORT, 'cards are archived, never removed'); END;
CREATE TRIGGER card_log_no_delete BEFORE DELETE ON card_log
  BEGIN SELECT RAISE(ABORT, 'card_log is append-only'); END;
CREATE TRIGGER card_revisions_no_delete BEFORE DELETE ON card_revisions
  BEGIN SELECT RAISE(ABORT, 'card_revisions is append-only'); END;
CREATE TRIGGER sessions_no_delete BEFORE DELETE ON sessions
  BEGIN SELECT RAISE(ABORT, 'sessions is append-only'); END;
CREATE TRIGGER session_events_no_delete BEFORE DELETE ON session_events
  BEGIN SELECT RAISE(ABORT, 'session_events is append-only'); END;
CREATE TRIGGER pending_no_delete BEFORE DELETE ON pending
  BEGIN SELECT RAISE(ABORT, 'pending is append-only'); END;

INSERT INTO meta (key, value) VALUES ('next_number', '1');
INSERT INTO columns (name, position) VALUES
  ('Backlog', 0), ('Planning', 1), ('In progress', 2), ('Blocked', 3),
  ('Human test', 4), ('Ship it', 5), ('Done', 6);
