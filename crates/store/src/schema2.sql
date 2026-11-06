-- Migration 2: each column's prompt, model, effort and session rules (PLAN.md, *Columns*).
-- {n}, {title}, {repo}, {issue}, {from}, {reason} are filled in when a prompt is typed, and
-- {prompts} is the prompts folder: the private folder's prompts\ if set, else the bundled one.
UPDATE columns SET mode = 'immediate', model = 'claude-opus-5-5', effort = 'high', permission_mode = 'bypassPermissions', auto_spawn = 1,
  prompt_text = 'You are the session for card #{n}, "{title}" ({repo}{issue}). Plan it now: follow {prompts}\planning.md.'
  WHERE name = 'Planning';
UPDATE columns SET mode = 'deferred', model = 'claude-opus-5-5', effort = 'high', permission_mode = 'bypassPermissions', auto_spawn = 1,
  prompt_text = 'Build it now: follow {prompts}\in-progress.md.'
  WHERE name = 'In progress';
UPDATE columns SET mode = 'deferred', model = 'claude-sonnet-5-5', effort = 'medium', permission_mode = 'bypassPermissions', auto_spawn = 0,
  prompt_text = 'It is blocked{reason}. Wait for Hugh''s answer before doing more.'
  WHERE name = 'Blocked';
UPDATE columns SET mode = 'none', model = 'claude-opus-5-5', effort = 'high', permission_mode = 'bypassPermissions', auto_spawn = 0
  WHERE name IN ('Backlog', 'Human test');
UPDATE columns SET mode = 'immediate', model = 'claude-opus-5-5', effort = 'medium', permission_mode = 'bypassPermissions', auto_spawn = 1,
  prompt_text = 'Ship it now: follow {prompts}\ship-it.md.'
  WHERE name = 'Ship it';
UPDATE columns SET mode = 'none', auto_spawn = 0, on_enter = 'suspend' WHERE name = 'Done';

CREATE INDEX pending_open ON pending(card_id) WHERE done_at IS NULL;
