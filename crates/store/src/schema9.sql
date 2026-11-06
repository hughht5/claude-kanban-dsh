-- Migration 9 (Hugh, 4 Nov 2026): the "Kanban app" card template, for changes to this app itself,
-- made by the kanban-agent skill ("Kanban agent, add …"). Its starting prompt points the card's
-- session at {prompts}\kanban-app.md, which swaps the column prompts' usual steps (worktrees,
-- dev stacks, sign-in links) for this app's own: the test app for Human test, a staged update for
-- Ship it. A board that already has the id keeps its own.
INSERT OR IGNORE INTO templates (id, name, title_prefix, body, start_prompt, position, created_at, updated_at) VALUES
  ('kanban-app', 'Kanban app', 'Kanban: ',
   'What to add or change:

Why:

How you will know it works:',
   'This card is a change to this kanban app itself. Read its repo''s AGENTS.md, then follow {prompts}\kanban-app.md: it replaces the column prompts'' usual steps (worktrees, dev stacks, sign-in links) with this app''s own. Human test means installed in the test app with steps to try; never restart or install over the live app.',
   3, '2026-11-04T00:00:00.000Z', '2026-11-04T00:00:00.000Z');
