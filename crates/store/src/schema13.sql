-- Migration 13 (card #114, 6 Nov 2026): the column prompts and the "Kanban app" template name
-- their folder as {prompts}, which the app fills in (dispatch::with_prompts): the private
-- folder's prompts if set, else the prompts the app carries. Before, they named a full path to a
-- checkout's prompts folder. Only the text an earlier migration wrote changes (the same words
-- around any folder): a prompt edited in Settings is left alone. Safe to run twice. Migration 11
-- is card #104's and 12 is card #109's.
UPDATE columns SET prompt_text = 'You are the session for card #{n}, "{title}" ({repo}{issue}). Plan it now: follow {prompts}\planning.md.'
  WHERE name = 'Planning'
    AND prompt_text LIKE 'You are the session for card #{n}, "{title}" ({repo}{issue}). Plan it now: follow %\prompts\planning.md.'
    AND prompt_text NOT LIKE '%{prompts}%';
UPDATE columns SET prompt_text = 'Build it now: follow {prompts}\in-progress.md.'
  WHERE name = 'In progress'
    AND prompt_text LIKE 'Build it now: follow %\prompts\in-progress.md.'
    AND prompt_text NOT LIKE '%{prompts}%';
UPDATE columns SET prompt_text = 'Ship it now: follow {prompts}\ship-it.md.'
  WHERE name = 'Ship it'
    AND prompt_text LIKE 'Ship it now: follow %\prompts\ship-it.md.'
    AND prompt_text NOT LIKE '%{prompts}%';
UPDATE columns SET prompt_text = 'The deploy is proven: follow {prompts}\deploy-complete.md. Leave the card here: Hugh moves it to Done.'
  WHERE name = 'Deploy complete'
    AND prompt_text LIKE 'The deploy is proven: follow %\prompts\deploy-complete.md. Leave the card here: Hugh moves it to Done.'
    AND prompt_text NOT LIKE '%{prompts}%';
-- The template: only its folder changes, from "then follow <folder>\prompts\kanban-app.md".
UPDATE templates SET start_prompt =
    substr(start_prompt, 1, instr(start_prompt, 'then follow ') + length('then follow ') - 1)
    || '{prompts}\kanban-app.md'
    || substr(start_prompt, instr(start_prompt, '\prompts\kanban-app.md') + length('\prompts\kanban-app.md'))
  WHERE id = 'kanban-app'
    AND start_prompt NOT LIKE '%{prompts}%'
    AND (start_prompt LIKE 'This card is a change to the % app itself (%). Read that repo''s AGENTS.md, then follow %\prompts\kanban-app.md: it replaces the column prompts'' % steps (worktrees, WSL dev stacks, sign-in links) with this app''s own. Human test means installed in the test app with steps to try; never restart or install over the live app.'
      OR start_prompt LIKE 'This card is a change to this kanban app itself. Read its repo''s AGENTS.md, then follow %\prompts\kanban-app.md: it replaces the column prompts'' usual steps (worktrees, dev stacks, sign-in links) with this app''s own. Human test means installed in the test app with steps to try; never restart or install over the live app.');
