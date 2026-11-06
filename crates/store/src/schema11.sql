-- Migration 11 (Hugh, 6 Nov 2026, card #104): a log of changes to the page's settings, so a
-- change to the small-change repo list (which lets cards go on to Ship it with no human test)
-- is always seen. Append-only like the card log. An older build opens the board as before: it
-- never reads this table. Safe to run twice (a board put back to an older schema number).
CREATE TABLE IF NOT EXISTS settings_log (
  id    INTEGER PRIMARY KEY AUTOINCREMENT,
  at    TEXT NOT NULL,
  key   TEXT NOT NULL,
  value TEXT NOT NULL,
  by    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS settings_log_key ON settings_log (key, id);
CREATE TRIGGER IF NOT EXISTS settings_log_no_update BEFORE UPDATE ON settings_log
  BEGIN SELECT RAISE(ABORT, 'settings_log is append-only'); END;
CREATE TRIGGER IF NOT EXISTS settings_log_no_delete BEFORE DELETE ON settings_log
  BEGIN SELECT RAISE(ABORT, 'settings_log is append-only'); END;
