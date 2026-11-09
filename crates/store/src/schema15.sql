-- Migration 15 (card #153, 9 Nov 2026): the text of every message the app sends to a session on
-- its own (a column prompt, the Router's events, a test-app turn, a nudge), so the chat can draw
-- them apart from Hugh's own words. card_id is the card's id, or the Router's key. Append-only.
-- An older build opens a board at this version as it is and never reads this table.
CREATE TABLE IF NOT EXISTS app_messages (
  seq      INTEGER PRIMARY KEY AUTOINCREMENT,
  card_id  TEXT NOT NULL,
  at       TEXT NOT NULL,
  text     TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS app_messages_card ON app_messages (card_id, seq);
CREATE TRIGGER IF NOT EXISTS app_messages_no_update BEFORE UPDATE ON app_messages
  BEGIN SELECT RAISE(ABORT, 'app_messages is append-only'); END;
CREATE TRIGGER IF NOT EXISTS app_messages_no_delete BEFORE DELETE ON app_messages
  BEGIN SELECT RAISE(ABORT, 'app_messages is append-only'); END;
