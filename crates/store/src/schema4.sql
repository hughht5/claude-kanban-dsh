-- Migration 4 (Hugh, 3 Nov 2026): the Deploy complete column and the new column order.
UPDATE columns SET position = 0 WHERE name = 'Backlog';
UPDATE columns SET position = 1 WHERE name = 'Blocked';
UPDATE columns SET position = 2 WHERE name = 'Planning';
UPDATE columns SET position = 3 WHERE name = 'In progress';
UPDATE columns SET position = 4 WHERE name = 'Human test';
UPDATE columns SET position = 5 WHERE name = 'Ship it';
UPDATE columns SET position = 7 WHERE name = 'Done';
INSERT INTO columns (name, position, mode, model, effort, permission_mode, auto_spawn, on_enter, prompt_text) VALUES
  ('Deploy complete', 6, 'immediate', 'claude-opus-5-5', 'medium', 'bypassPermissions', 0, 'none',
   'The deploy is proven: follow {prompts}\deploy-complete.md. Leave the card here: Hugh moves it to Done.');
