-- Migration 5 (Hugh, 4 Nov 2026): unread replies. A 'reply' mark is written when a card's
-- session ends a turn (its Stop hook); a 'read' mark when Hugh opens (or is looking at) the
-- card's panel. A card is unread while its newest reply is newer than its newest read.
-- Insert-only, like the logs: a card's state is its newest mark of each kind.
CREATE TABLE card_marks (
  seq      INTEGER PRIMARY KEY AUTOINCREMENT,
  card_id  TEXT NOT NULL REFERENCES cards(id),
  kind     TEXT NOT NULL CHECK (kind IN ('read', 'reply')),
  at       TEXT NOT NULL
);
CREATE INDEX card_marks_card ON card_marks(card_id, kind, seq);
CREATE INDEX IF NOT EXISTS session_events_session ON session_events(session_id, seq);

CREATE TRIGGER card_marks_no_update BEFORE UPDATE ON card_marks
  BEGIN SELECT RAISE(ABORT, 'card_marks is append-only'); END;
CREATE TRIGGER card_marks_no_delete BEFORE DELETE ON card_marks
  BEGIN SELECT RAISE(ABORT, 'card_marks is append-only'); END;
