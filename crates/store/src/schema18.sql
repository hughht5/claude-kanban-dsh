-- Migration 18 (card #195, 12 Nov 2026): the Plan review column, between Planning and In
-- progress. A card whose plan is finished waits there for Hugh: no session runs (mode none, no
-- auto-spawn), and the card's planning session stops when it arrives (on_enter suspend; the
-- conversation is kept and resumes in In progress). Hugh approves the plan by moving the card to
-- In progress, or sends it back to Planning with his notes. The columns after it move up one.
-- Nothing is removed, and it is safe to run twice. An older build opens a board at this version as it is: it doesn't know the
-- column, so it draws a card there in Backlog until the newer build is back.
UPDATE columns SET position = position + 1
  WHERE position >= 3 AND NOT EXISTS (SELECT 1 FROM columns WHERE name = 'Plan review');
INSERT INTO columns (name, position, mode, model, effort, permission_mode, auto_spawn, on_enter, prompt_text)
  SELECT 'Plan review', 3, 'none', 'claude-opus-5-5', 'high', 'bypassPermissions', 0, 'suspend', NULL
  WHERE NOT EXISTS (SELECT 1 FROM columns WHERE name = 'Plan review');
