<!-- Working notes for a card that changes this kanban app itself (the "kanban-app" template). The
     template's starting prompt points at this file as {prompts}\kanban-app.md. Where this file and
     a column prompt ({prompts}\planning.md, {prompts}\in-progress.md, {prompts}\ship-it.md,
     {prompts}\deploy-complete.md) disagree, this file wins. -->

# A kanban-app card

<!-- toc -->
**Contents**

- [What this card is](#what-this-card-is)
- [A stack card](#a-stack-card)
- [Planning](#planning)
- [In progress](#in-progress)
- [Human test](#human-test)
- [Ship it](#ship-it)
- [Deploy complete](#deploy-complete)
<!-- /toc -->

## Where changes go

Every change to the app is pushed to GitHub, to one of two repos:

- **The app's code, tests and docs** go to the public repo `hughht5/claude-kanban-dsh`, in `~\claude-kanban-dsh` (a card for `hughht5/kanban-app` or `hughht5/claude-kanban-dsh` builds there; `~\kanban-app` is the old archive, never build in it).
- **Owner-only material** (column prompts, skills, `repos.json`) goes to the owner's private repo (the private folder in Settings).
- In progress pushes the card's own branch, never `main`. Ship it merges to `main`, pushes `main`, then stages the live update (never `-Now`).
- Before a commit to the public repo, its pre-commit check (`scripts/check-public.sh`) must pass: no employer names, work emails, home paths or private repo names.

## What this card is

This card changes the kanban app itself (this repo). The column prompts are written for a web
product with a dev stack and a deploy; follow them for the card tools, the board's rules, the scope
gate, the card-tester, stacks and hand-over, but **not** their dev-stack and deploy parts. Follow
the repo's `AGENTS.md` throughout: it holds the build, the test version and the hard rules.

Not for this card, whatever the column prompt says:

- no worktree, no dev stack, no `logins`, no `devStack`, no browser profiles;
- no deploy-run proofs, no production checks, no after-deploy steps.

You are probably running inside the live app. **Never quit, restart or `-Now` the live app**: it
would kill this very session and every other card's.

## A stack card

If your card leads a stack or is in one, and `kanban_stack_list` shows a `stage` for each child,
the stack builds in parallel (planning.md and in-progress.md, their stack sections). For the
stack's cards this section replaces "work on `main`" and "one kanban-app card builds at a time"
below, because each card has its own worktree:

- **The lead** makes the stack's worktree `<checkout>-<n>` (its own number) on a
  branch of its own, for example `stack-<n>`, from `main`
  (`git -C <checkout> worktree add -b stack-<n> <checkout>-<n> main`),
  and records `worktreePath` and `branch`. The merger fast-forwards that branch, and the lead pushes it
  (`git push origin stack-<n>`); nothing reaches `main` until the stack ships.
- **A child** works in `<checkout>-<n>` (its own number) on the branch
  `<stack branch>-<n>`, made from the stack's branch
  (`git -C <checkout> worktree add -b <stack branch>-<n> <checkout>-<n> <stack branch>`),
  and records both. It never commits to the
  stack's branch or to `main`. It pushes its own branch (`git push origin <its branch>`), never
  `main`.
- **Children don't install the test app for the owner.** A child builds, runs
  `cargo test --workspace`, `/code-review`, its review and its test on its own branch, and commits
  there. Its `nextStep` starts "Built on `<branch>`, commit `<short sha>`; the lead merges it and
  installs the stack", then its numbered steps for the stack's test app and "Broken if: ...". If a
  change can only be checked in the running app, any install still follows In progress step 5; say
  in the hand-over that the owner tests it on the lead's install. A bug report on a child doesn't
  hold it for the reproducer: the child puts the steps that show the bug in its hand-over, and the
  lead's `card-reproducer` proves the fix on the lead's install.
- **The lead installs once.** When the stack is ready, the lead installs the stack's branch in the
  test app (In progress step 5), checks it (step 6) and runs its own `card-tester` on that install
  (step 6a). Its `nextStep` starts "Installed in the test app (Claude Kanban DSH Test), stack branch
  `<branch>`, commit `<short sha>`".
- **Ship it** runs on the lead, once, for the whole stack (the Ship it section below).

Otherwise, as before: the stack's lead works on `main` in `<checkout>`, as below.

## Planning

0. If your session has the card-router subagent, route the card first, as planning.md's *Route it
   first* says (not for a stack's child, and not if the card already has a `route`). If it hasn't,
   say so on the card and go on.
1. Work in the repo's own checkout on `main` (`<checkout>` in this file, for example
   `~\claude-kanban-dsh`, where `~` is your home folder). No worktree, no branch. Record
   `worktreePath` (the checkout's full path) and `branch` = `main` on the card.
2. **One kanban-app card builds at a time**, because they share the checkout. Check
   `kanban_list_cards`: if another card for this repo is In progress (or Human test with
   uncommitted work in the checkout), say so on the card and in your message, and wait in Planning.
3. Read `AGENTS.md`, `HANDOVER.md`, and the code the change touches. Run `git status`: if the
   checkout has someone else's uncommitted work, don't touch it; say so and wait.
4. A GitHub issue is welcome but optional; put it in `issue` if you make one.
5. Write the approach into `summary` (every deliverable listed) and the first build step into
   `nextStep`. Make your own decisions; ask the owner only what is genuinely theirs.
5a. If `kanban_update_card` lists `doneWhen` among its allowed fields, also write `doneWhen` and
   3 to 7 `doneWhenChecks`, as planning.md step 5a says. If it doesn't (an older build), skip this.
6. If your session has the card-critic subagent (the Agent tool lists it), check the plan with it
   last, as planning.md's *Check the plan with a subagent* says: brief it with the card number and
   the checkout (`<checkout>`, or the card's worktree), fix what it finds, and run
   a fresh one until it passes. If your session doesn't have it, say so on the card and go on.

## In progress

1. Build on `main` in `<checkout>`. Never push `main` here (step 7 pushes the card's branch).
1a. If the card's `repro` is needed (a bug report) and your session has the `card-reproducer`
   subagent, prove the bug before you fix it, and the fix after it (in-progress.md, *Prove a
   reported bug first*). Its second run needs the fix in the test app first (step 5). If your
   session has no `card-reproducer`, the move to Human test is still held: say so on the card and
   ask the owner to turn the rule off or restart the session.
2. Tests for every data rule and migration; `cargo test --workspace` must pass (see AGENTS.md).
3. Run `/code-review` on your diff and fix what it finds.
4. Write the undo into `UNDO.md`; update `README.md`/`HANDOVER.md` if behaviour changed.
4a. If your session has the `card-reviewer` subagent, have it review your diff now, before the
   install (in-progress.md, *Review and test it with subagents*). Put `(card #<n>)` in your commit
   messages so it finds them. Fix what it finds and run a fresh one. If your session has no
   `card-reviewer`, say so on the card and go on, but leave the move to Human test to the owner
   (yours would be refused while the review rule is on), or ask them to turn the rule off or restart
   the session.
4b. **Turns on the test app**. If `kanban_test_app` is in your tools: the test app is
   shared, so use it only during your card's turn. Call `kanban_test_app` with action `take`
   before step 5. You get the turn now, or a place in line: then end your turn; the app sends you
   a message when it is yours. Release it with action `release` after step 6a. If your card or
   the owner says the test app already has your build installed, skip step 5 and use it as it is.
   Run `install.ps1` in the background (`run_in_background`): it waits for other installs, other
   cards' turns, the owner's Hold and their use of the test app, and prints why; a wait is not a
   failure. If `kanban_test_app` isn't in your tools, skip this step and follow your card's
   instructions on the test app.
5. Install it in the **test app**:
   `powershell -ExecutionPolicy Bypass -File scripts/install.ps1 -Channel test` (add `-FreshCopy`
   only if the test board needs to be current). Never the live channel here. The install restarts
   the test app: before it, if any card of a stack is in Human test in the test app, log on each of
   those cards that you are reinstalling it. On a Mac only: install it in the test app with
   `sh scripts/install.sh --channel test`.
6. Verify it yourself in the test app, driving it with `tools/cdp.mjs` on port 9334 (always
   `--background`). Use dry-run cards on the test board. When the change shows on screen, save a
   shot (`CDP_PORT=<the test app's port> node tools/cdp.mjs shot <png>`; 9334 for the test app,
   9333+N for test N) and put it in your message as
   `![what it shows](C:/path/to/shot.png)`, so the owner sees it in the card's chat.
   On a Mac only: there is no tools/cdp.mjs and no screenshot. Check the change through `kb` and
   the app's API, and tell the owner what to look at.
6a. Then have the `card-tester` subagent test it (in-progress.md, *Review and test it with
   subagents*): brief it with the card number, the deliverables, the commit and the test app it
   tests with that app's DevTools port (9334 for the test app, 9333+N for test N), and fix what it
   finds. The move to Human test is refused until the latest review and
   test have both passed. A fix after a pass means running both again. On a Mac only: brief
   the card-tester to check through `kb` and the API, not cdp.mjs.
6b. If the card has `doneWhenChecks` (and `kanban_update_card` lists `doneWhen`), mark each check as
   you verify it in step 6, and say in the tester's brief that the card has them: its `CHECK <n>`
   lines mark them on the card.
7. Commit on `main` locally (a clear message, the Co-Authored-By line). Then push the card's work
   to GitHub as its own branch, never as `main`: `git push origin HEAD:refs/heads/card-<n>`
   (`--force-with-lease` when you push it again). `main` is pushed only in Ship it.
8. Route: no diff goes to Done; otherwise hand over (below) and move to Human test.

## Human test

Before the move, write into `nextStep`:

- one line: "Installed in the test app (Claude Kanban DSH Test), commit `<short sha>`";
- if the card had a bug report, the reproducer's proof next: the before and after steps, both
  screenshots and the test's name;
- **numbered steps of what to click in the test app** (where to start, what to do, what a pass looks
  like);
- a last line **"Broken if: ..."**.

If `kanban_update_card` lists `changesOutputs` among its allowed fields and the work made a finished
file for the owner (a report, a CSV, a PDF), list it there too, by its full Windows path, as
`[{"path": "C:\\...", "label": "..."}]`.
Skip this on an older app.

End your message with the same steps. No logins and no dev stack. If the owner reports a bug, log it
and ask them to move the card back to In progress (their Fail, or a message, does that), where the
reproducer proves it before and after the fix.

## Ship it

Only after the owner moves the card there themselves, or the app moves it there for them because
they set the card to go on by itself to Deploy complete, or because the change was small and fully
checked and they turned on small-change auto-ship for this repo (the app's message says which).
Never move a card into Ship it yourself. When the app's message says "push
exactly <sha>", push that commit and nothing else, to the branch the message names
(`git push origin <sha>:<branch>`), instead of step 3's push; the app checks it.

1. `kanban_ship_result` `shipping`.
1a. If your session has the `card-merger` subagent (the Agent tool lists it), run it now (card
   #100). If not, say so on the card and go on with step 2.
   - Brief it with the card's number, the checkout (`worktreePath`) and the branch: `main`, or the
     stack's branch. It rebases that branch onto `origin/main`, checks
     that every commit in `origin/main..HEAD` carries `(card #<n>)` for a card of this ship, and
     runs `cargo test --workspace`. It never pushes.
   - On **PASS**, check its `HEAD:` line is the branch's HEAD and its `ORIGIN:` SHAs match (if not,
     something pushed: report `needs-hugh` and stop). For a branch other than `main`, fast-forward
     main to it in the main checkout: `git -C <checkout> merge --ff-only <branch>`.
     Then go on with step 2.
   - On **FAIL** (or "Merge unclear" on the card): `kanban_ship_result` `needs-hugh` with its
     reason, and stop. A commit with no `(card #<n>)` of this ship is someone else's unshipped
     work: never push it.
2. Make sure the tested commit is what's on `main` (`git status` clean for your files, tests pass).
3. `git push origin main`. Then delete the card's branch on GitHub if you pushed one:
   `git push origin --delete card-<n>` (each card of a stack: its own branch).
4. Stage the live update: `powershell -ExecutionPolicy Bypass -File scripts/install.ps1` (no
   `-Channel`; its default is `-Stage`). **Never `-Now`, never restart or quit the live app
   yourself.** Check `%USERPROFILE%\.claude-kanban-dsh\update\update.json` names your commit.
   On a Mac only: `sh scripts/install.sh` (it stages by default). Never --now.
5. Log the commit and the staged installer on the card. `kanban_ship_result` `shipped`, with the
   short sha as detail; `failed` with what failed if the push or the build failed.
6. Move to Deploy complete, and tell the owner to **click Restart now** on the "Update ready" bar
   when it suits them (it suspends and resumes every session, this one included). Then numbered
   steps to check it in the live app.

## Deploy complete

Leave the checkout clean for your files (commit real work, delete junk you made, never someone
else's), log what you did, and say where it stands. No after-deploy steps. The owner moves it to
Done.
