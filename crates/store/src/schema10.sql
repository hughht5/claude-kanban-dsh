-- Migration 10 (Hugh, 5 Nov 2026, card #83): the board has no Done column any more. A card in
-- Done is archived and shows in the board's Archive; from now on the store archives a card the
-- moment it reaches Done. This archives the cards already there, keeping when each was last
-- touched as its archivedAt (so the Archive lists them in the order they were finished), with a
-- revision and a log line for each. Nothing is removed. An older build opens the board as before:
-- it just shows these cards as archived.
CREATE TEMP TABLE m10_done AS
  SELECT id FROM cards WHERE col = 'Done' AND archived = 0;

UPDATE cards
   SET archived = 1,
       fields = json_set(fields,
         '$.archived', json('true'),
         '$.archivedAt', COALESCE(json_extract(fields, '$.updatedAt'), updated_at),
         '$.archivedBy', 'the app (Done is archived)'),
       version = version + 1
 WHERE id IN (SELECT id FROM m10_done);

-- A stack's children carried into Done with their lead are archived with it, so restoring the
-- lead brings them back (as the store does for a lead archived from now on).
UPDATE cards
   SET fields = json_set(fields, '$.archivedWith', json_extract(fields, '$.stackOf'))
 WHERE id IN (SELECT id FROM m10_done)
   AND json_extract(fields, '$.stackOf') IN
       (SELECT c.number FROM cards c WHERE c.id IN (SELECT id FROM m10_done));

INSERT INTO card_revisions (card_id, version, at, by, changed)
  SELECT c.id, c.version, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), 'the app (Done is archived)',
         json_object('archived', json('true'), 'archivedAt', json_extract(c.fields, '$.archivedAt'),
                     'archivedBy', 'the app (Done is archived)')
    FROM cards c WHERE c.id IN (SELECT id FROM m10_done);

INSERT INTO card_log (card_id, at, text, by)
  SELECT id, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), 'Archived: Done cards are archived now', 'the app'
    FROM m10_done;

DROP TABLE m10_done;
