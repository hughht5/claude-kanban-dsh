-- The session search index (card #110): its own file, search.db, beside board.db. Its
-- migrations are search1.sql, search2.sql and so on, counted in its own user_version.
-- Every table takes inserts only. A new file is built each month (or when the index
-- version goes up) and the old one is renamed into backups\search, so nothing here is
-- ever changed in place.

-- Written once, when the file is made: its month ("2026-11") and index version.
CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- One transcript file: a card session's main transcript, a desktop conversation, a file a
-- hook named after a /clear, or a subagent's transcript (agent holds its type).
CREATE TABLE files (
  id       INTEGER PRIMARY KEY,
  path     TEXT NOT NULL UNIQUE,
  card_id  TEXT NOT NULL,
  session  TEXT NOT NULL,
  agent    TEXT,
  added_at TEXT NOT NULL
);

-- One searchable entry: a prompt, a reply, a tool call (its name and key input), a task
-- notice. key is the line's uuid and the entry's place in it, or for a line with no uuid the
-- file id, the line's byte offset and the place; INSERT OR IGNORE on it stores a line once.
CREATE TABLE entries (
  id   INTEGER PRIMARY KEY,
  key  TEXT NOT NULL UNIQUE,
  file INTEGER NOT NULL REFERENCES files(id),
  kind TEXT NOT NULL,
  who  TEXT NOT NULL DEFAULT '',
  at   TEXT NOT NULL,
  text TEXT NOT NULL
);

-- How far each file has been read: its latest row wins.
CREATE TABLE progress (
  seq    INTEGER PRIMARY KEY,
  file   INTEGER NOT NULL REFERENCES files(id),
  offset INTEGER NOT NULL,
  at     TEXT NOT NULL
);
CREATE INDEX progress_by_file ON progress(file, seq);

-- The full-text index over entries.text: trigrams, so any run of 3 or more characters
-- matches, ignoring case.
CREATE VIRTUAL TABLE entries_fts USING fts5(text, content='entries', content_rowid='id', tokenize='trigram');

CREATE TRIGGER entries_ai AFTER INSERT ON entries
  BEGIN INSERT INTO entries_fts(rowid, text) VALUES (new.id, new.text); END;

-- INSERT OR REPLACE on a known key would remove the old row without the delete triggers
-- firing (SQLite runs them on REPLACE only with recursive_triggers on): a known key is ignored
-- instead, so the first row stays.
CREATE TRIGGER meta_once BEFORE INSERT ON meta WHEN EXISTS (SELECT 1 FROM meta WHERE key = new.key)
  BEGIN SELECT RAISE(IGNORE); END;
CREATE TRIGGER files_once BEFORE INSERT ON files WHEN EXISTS (SELECT 1 FROM files WHERE path = new.path)
  BEGIN SELECT RAISE(IGNORE); END;
CREATE TRIGGER entries_once BEFORE INSERT ON entries WHEN EXISTS (SELECT 1 FROM entries WHERE key = new.key)
  BEGIN SELECT RAISE(IGNORE); END;

CREATE TRIGGER meta_no_update BEFORE UPDATE ON meta
  BEGIN SELECT RAISE(ABORT, 'meta is written once'); END;
CREATE TRIGGER meta_no_delete BEFORE DELETE ON meta
  BEGIN SELECT RAISE(ABORT, 'meta is written once'); END;
CREATE TRIGGER files_no_update BEFORE UPDATE ON files
  BEGIN SELECT RAISE(ABORT, 'files is append-only'); END;
CREATE TRIGGER files_no_delete BEFORE DELETE ON files
  BEGIN SELECT RAISE(ABORT, 'files is append-only'); END;
CREATE TRIGGER entries_no_update BEFORE UPDATE ON entries
  BEGIN SELECT RAISE(ABORT, 'entries is append-only'); END;
CREATE TRIGGER entries_no_delete BEFORE DELETE ON entries
  BEGIN SELECT RAISE(ABORT, 'entries is append-only'); END;
CREATE TRIGGER progress_no_update BEFORE UPDATE ON progress
  BEGIN SELECT RAISE(ABORT, 'progress is append-only'); END;
CREATE TRIGGER progress_no_delete BEFORE DELETE ON progress
  BEGIN SELECT RAISE(ABORT, 'progress is append-only'); END;
