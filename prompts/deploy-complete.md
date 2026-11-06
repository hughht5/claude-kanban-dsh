<!-- Deploy complete column: an example prompt. The column's prompt points at this file as
     {prompts}\deploy-complete.md. Copy it to your private prompts folder and add your own
     after-deploy steps (a customer change log, release notes, an audit list). -->

# Deploy complete

<!-- toc -->
**Contents**

- [The card](#the-card)
- [1. Leave the worktree clean](#1-leave-the-worktree-clean)
- [2. Your own after-deploy steps](#2-your-own-after-deploy-steps)
- [3. Tell the owner where things stand](#3-tell-the-owner-where-things-stand)
<!-- /toc -->

The card is merged and production serves it. Leave the worktree clean, do any after-deploy steps
your repo has, then say where things stand. Don't move the card: the owner moves it to Done once
satisfied. The dev stack was stopped at Ship it.

## The card

Your card lives in the kanban app. Use its `kanban_*` tools (in Bash, `kb`). There is no other
board. `kanban_get_card` reads the card (issue, pr, branch, worktreePath, ship) and its log, which
is the card's comment thread; `kanban_add_log` adds to it (anything this file says to "comment on
the card" goes there); `kanban_update_card` changes fields.

## 1. Leave the worktree clean

A card can't be archived with uncommitted changes, and this is the last column that can fix that.
Decide yourself rather than leaving it for the owner. Work in the card's `worktreePath`.

1. Run `git status --porcelain`. If it is clean, go to step 2.
2. Otherwise inspect every entry (its size and first lines) before touching it, and never delete
   anything unseen. Then decide file by file:
   - **Shell-redirect junk**: 0-byte files, or file names that are fragments of code or commands
     (`revision=%s`, `--json x`). These come from a `>` inside a quoted string being read as a
     redirection. Delete them: committing them puts permanent garbage in the repo.
   - **Build and tooling output**: `dist/`, `*.log`, `node_modules/`, coverage. Delete them, or add
     them to `.gitignore` if they will recur.
   - **Real work**: source, tests, docs. Commit it. If it is docs only, push it the way the repo
     takes docs changes. If it is code, commit it locally only and don't push: the deploy already
     ran, and a second push to main starts a run this card is no longer watching. Say so clearly in
     the summary, and file an issue so it isn't lost.
   - **Can't tell?** Commit rather than delete: recoverable beats gone.
3. Run `git status --porcelain` again and confirm it is empty. Say in the summary what you removed
   or committed, and why.

## 2. Your own after-deploy steps

Put here whatever your team does once a change is live: a customer change-log entry, release
notes, an update to an audit or to-do list. Write each step so it can run more than once for one
card without doing the work twice (check for an earlier entry first), and log on the card what you
did or why you skipped it. If a step can't run (a tool is missing, a channel doesn't exist), don't
hold the card up: say so in the summary and say what the owner should do.
Put the steps for your own repos in your private prompts folder.

## 3. Tell the owner where things stand

Write a short version into the card's `summary` and `nextStep` (what shipped; what, if anything,
the owner still has to check), then write your message.

Assume the owner has forgotten what this card was about. Open with what it was for and why it
mattered, in plain language, before any detail. Then cover what shipped, anything that turned out to
be wrong underneath it, what was filed rather than fixed, and the result of each step in section 2.

End on numbered how-to-test steps against production: what to click, what a pass looks like at each
step, and any trap that makes a correct build look broken. The owner reads the bottom of the message
and goes and clicks, so don't end on a status table or build summary. The branch's dev stack was
stopped at Ship it, so its links no longer work.
