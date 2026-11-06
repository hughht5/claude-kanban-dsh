-- Migration 6 (Hugh, 4 Nov 2026): card templates. A template fills the new-card form (a title
-- prefix and a description) and gives the card's first session a starting prompt, sent with the
-- column prompt. Templates are archived, never removed. Written so it is safe to run twice
-- (IF NOT EXISTS, OR IGNORE), so the order beside migration 5 can be settled at merge.
CREATE TABLE IF NOT EXISTS templates (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  title_prefix  TEXT NOT NULL DEFAULT '',
  body          TEXT NOT NULL DEFAULT '',
  start_prompt  TEXT NOT NULL DEFAULT '',
  position      INTEGER NOT NULL DEFAULT 0,
  archived      INTEGER NOT NULL DEFAULT 0,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);

CREATE TRIGGER IF NOT EXISTS templates_no_delete BEFORE DELETE ON templates
  BEGIN SELECT RAISE(ABORT, 'templates are archived, never removed'); END;

INSERT OR IGNORE INTO templates (id, name, title_prefix, body, start_prompt, position, created_at, updated_at) VALUES
  ('bug', 'Bug', 'Bug: ',
   'What happens:

What should happen:

Steps to reproduce:
1.

Where (page, org, account):',
   'This card is a bug. Reproduce it first and write the steps you used into the card''s summary. Find the cause before you change anything, add a test that fails without the fix, then fix it. Say in nextStep what Hugh should try to see it fixed.',
   0, '2026-11-04T00:00:00.000Z', '2026-11-04T00:00:00.000Z'),
  ('customer-request', 'Customer request', 'Customer: ',
   'Customer (organisation and person):

What they asked for, in their words:

Why it matters to them:

Deadline or promise made:',
   'This card is a customer request. Restate what the customer needs in one or two sentences in the summary, and list anything unclear as questions for Hugh before you build; if the ask is ambiguous, move the card to Blocked with the questions in blockedReason. Keep the change to what they asked for.',
   1, '2026-11-04T00:00:00.000Z', '2026-11-04T00:00:00.000Z'),
  ('small-fix', 'Small fix', 'Fix: ',
   'What to change:

Where:',
   'This card is a small fix. Keep it small: one focused change and the tests around it, no refactoring beyond what the fix needs. If it turns out bigger than a small fix, say so in nextStep and stop for Hugh.',
   2, '2026-11-04T00:00:00.000Z', '2026-11-04T00:00:00.000Z');
