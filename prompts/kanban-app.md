<!-- Working notes for a card that changes this kanban app itself (the "kanban-app" template). The
     template's starting prompt points at this file as {prompts}\kanban-app.md. Where this file and
     a column prompt ({prompts}\planning.md, {prompts}\in-progress.md, {prompts}\ship-it.md,
     {prompts}\deploy-complete.md) disagree, this file wins. -->

# A kanban-app card

<!-- toc -->
**Contents**

- [What this card is](#what-this-card-is)
- [Cards build in parallel](#cards-build-in-parallel)
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

- no dev stack, no `logins`, no `devStack`, no browser profiles;
- no deploy-run proofs, no production checks, no after-deploy steps.

You are probably running inside the live app. **Never quit, restart or `-Now` the live app**: it
would kill this very session and every other card's.

## Cards build in parallel

Several cards for this repo build at once (card #145). Each has its own git worktree and branch,
and its own test app. Ships still go one at a time.

- **Its own worktree.** `<checkout>/.claude/worktrees/card-<n>` on the branch `card-<n>`, made
  from `origin/main`. If the repo is listed in Settings › Sessions › "Build each card in its own
  worktree" (`sessions.own_worktree_repos`), the app makes it when the card's first session starts,
  records `worktreePath` and `branch`, and its note names both (card #146): don't change them.
  Otherwise you make it (Planning step 1). On a test app's board the name is `<channel>-card-<n>`
  (`test-card-<n>`), so it never takes a live card's branch.
- **Work only there.** Build, test and commit in the card's worktree: never in `<checkout>` itself,
  never in another card's worktree, never on `main`.
- **Its own test app.** `kanban_test_app` with action `take` gives the card a test app of its own
  (Test, Test 2, ...; Settings › Sessions › "Test apps, at most"), recorded on the card as
  `testApp` (card #147). Its reply names the test app, its install command and its CDP port. It
  stays the card's while the card is In progress or Human test (a stack's child: In progress only),
  so the owner tests each card's build in its own test app.
- **Ships go one at a time.** Cards of this repo ship one at a time (the rest show "Waiting to
  ship", card #79). At ship the repo's merge lane rebases the card's branch onto the latest
  `origin/main` and reruns the tests (cards #160, #162); then `main` is fast-forwarded to it and
  pushed. A stack's cards
  ship together, once, on the lead (card #113).

## A stack card

If your card leads a stack or is in one, and `kanban_stack_list` shows a `stage` for each child,
the stack builds in parallel (planning.md and in-progress.md, their stack sections). The app
doesn't make a stack card's worktree (card #146): the lead and each child make their own, as
below, in place of Planning step 1:

- **The lead** makes the stack's worktree `<checkout>/.claude/worktrees/card-<n>` (its own number) on a
  branch of its own, for example `stack-<n>`, from `main`
  (`git -C <checkout> worktree add -b stack-<n> <checkout>/.claude/worktrees/card-<n> main`),
  and records `worktreePath` and `branch`. The merger fast-forwards that branch, and the lead pushes it
  (`git push origin stack-<n>`); nothing reaches `main` until the stack ships.
- **A child** works in `<checkout>/.claude/worktrees/card-<n>` (its own number) on the branch
  `<stack branch>-<n>`, made from the stack's branch
  (`git -C <checkout> worktree add -b <stack branch>-<n> <checkout>/.claude/worktrees/card-<n> <stack branch>`),
  and records both. It never commits to the
  stack's branch or to `main`. It pushes its own branch (`git push origin <its branch>`), never
  `main`.
- **Children don't install a test app for the owner.** A child builds, runs
  `cargo test --workspace`, `/code-review`, then its review and its test together on its own
  branch, and commits there. Its `nextStep` starts "Built on `<branch>`, commit `<short sha>`; the
  lead merges it and installs the stack", then its numbered steps for the stack's test app and
  "Broken if: ...". If a change can only be checked in the running app, the child takes a test app
  of its own for its own checks and its review and test (In progress steps 4b to 6a); it keeps it
  only while In progress, so say in the hand-over that the owner tests it on the lead's install. A bug report on a child
  doesn't hold it for the reproducer: the child puts the steps that show the bug in its hand-over,
  and the lead's `card-reproducer` proves the fix on the lead's install.
- **The lead installs once.** When the stack is ready, the lead installs the stack's branch in its
  own test app (In progress steps 4b and 5), checks it (step 6) and runs its own `card-tester` on
  that install (step 6a; its review counts as passed once every child's has). Its `nextStep` starts "Installed in <its test app> (Claude Kanban DSH
  Test N), stack branch `<branch>`, commit `<short sha>`".
- **Ship it** runs on the lead, once, for the whole stack (the Ship it section below).

Otherwise, as before: the stack's lead builds every child itself, in its own worktree on the
stack's branch, as below.

## Planning

`<checkout>` in this file is the repo's own checkout, for example `~\claude-kanban-dsh`, where `~`
is your home folder.

0. If your session has the card-router subagent, route the card first, as planning.md's *Route it
   first* says (not for a stack's child, and not if the card already has a `route`). If it hasn't,
   say so on the card and go on.
1. **Your worktree.** If the app's note names this card's worktree and branch (the app made them),
   use them as they are. If not, make them (a stack card: as *A stack card* says):
   `git -C <checkout> fetch origin`, then
   `git -C <checkout> worktree add --no-track -b card-<n> <checkout>/.claude/worktrees/card-<n> origin/main`,
   and record `worktreePath` and `branch` on the card (on a test app's board, `<channel>-card-<n>`
   in place of `card-<n>`, both times). Work only there from now on.
2. **Other cards of this repo build at the same time**, each in its own worktree. Never touch
   `<checkout>` itself or another card's worktree. If your plan changes the same files as another
   card In progress (`kanban_list_cards`), log it on both cards: whichever ships second is rebased
   onto the first at its ship.
3. Read `AGENTS.md`, `HANDOVER.md`, and the code the change touches, in your worktree. Run
   `git status` there: it should be clean, on your branch.
4. A GitHub issue is welcome but optional; put it in `issue` if you make one.
5. Write the approach into `summary` (every deliverable listed) and the first build step into
   `nextStep`. Make your own decisions; ask the owner only what is genuinely theirs.
5a. If `kanban_update_card` lists `doneWhen` among its allowed fields, also write `doneWhen` and
   3 to 7 `doneWhenChecks`, as planning.md step 5a says. If it doesn't (an older build), skip this.
6. If your session has the card-critic subagent (the Agent tool lists it), check the plan with it
   last, as planning.md's *Check the plan with a subagent* says: brief it with the card number and
   the card's worktree and branch, fix what it finds, and run
   a fresh one until it passes (a card routed small has one critique: after a FAIL, fix the plan
   and leave the card for the owner). Don't edit the plan for a PASS's notes: they go to the build. If
   your session doesn't have it, say so on the card and go on.

## In progress

1. Build in the card's worktree, on its own branch: never in `<checkout>` itself and never on
   `main`. Never push `main` here (step 7 pushes the card's branch).
1a. If the card's `repro` is needed (a bug report) and your session has the `card-reproducer`
   subagent, prove the bug before you fix it, and the fix after it (in-progress.md, *Prove a
   reported bug first*). Its second run needs the fix in the test app first (step 5). If your
   session has no `card-reproducer`, the move to Human test is still held: say so on the card and
   ask the owner to turn the rule off or restart the session.
2. Tests for every data rule and migration; `cargo test --workspace` must pass (see AGENTS.md).
3. Run `/code-review` on your diff and fix what it finds.
4. Write the undo into `UNDO.md`; update `README.md`/`HANDOVER.md` if behaviour changed.
4a. Don't run the `card-reviewer` yet: it runs together with the `card-tester` in step 6a, once
   the build is installed (card #162). Put `(card #<n>)` in your commit messages so it finds them.
4b. **Your test app** (cards #115 and #147). If `kanban_test_app` is in your tools, call it with
   action `take` before step 5. The first take gives this card a test app of its own (Test,
   Test 2, ...) and a turn on it, or a place in line: then end your turn; the app sends you a
   message when it is yours. Its reply names the test app, its install command and its CDP port:
   use those in steps 5 to 6a, and never another card's test app. Release the turn with action
   `release` after step 6a: the test app stays this card's, so the owner tests your build there.
   If your card or the owner says your test app already has your build installed, skip step 5 and
   use it as it is. Run `install.ps1` in the background (`run_in_background`): it waits for other
   installs, other cards' turns on that test app, the owner's Hold and their use of it, and prints
   why; a wait is not a failure. If `kanban_test_app` isn't in your tools, skip this step and use
   the one shared test app (`-Channel test`, port 9334) as your card says.
5. Install it in **your test app**, from your worktree, with the command step 4b's reply gave:
   `powershell -ExecutionPolicy Bypass -File scripts/install.ps1 -Channel <its channel>` (`test`
   for Test, `test2` for Test 2, ...). Add `-FreshCopy` only if its board needs to be current.
   Never the live channel here. A test app's first install creates it and copies the live board.
   On a Mac only (one test app there): install it with `sh scripts/install.sh --channel test`.
6. Verify it yourself in your test app, driving it with `tools/cdp.mjs` on its CDP port (9334 for
   Test, 9333+N for Test N; always `--background`). Use dry-run cards on its board. When the change
   shows on screen, save a shot (`CDP_PORT=<its port> node tools/cdp.mjs shot <png>`) and put it in
   your message as `![what it shows](C:/path/to/shot.png)`, so the owner sees it in the card's
   chat. On a Mac only: there is no tools/cdp.mjs and no screenshot. Check the change through `kb`
   and the app's API, and tell the owner what to look at.
   **No computer use and no browser** (card #172), for you and every subagent: no desktop
   screenshots or clicks (computer use), and no Claude in Chrome, built-in browser or
   chrome-devtools. Test with the tests, `kb` and the `kanban_*` tools, the app's API, and
   `tools/cdp.mjs` on your own test app's CDP port; its shots are the card's evidence. The app
   denies those tools to this repo's card sessions and their subagents anyway.
6a. Then run the `card-reviewer` and the `card-tester` together (in-progress.md, *Review and test
   it together*): two Agent calls in one message, both in the background, and wait for both
   notifications. Brief the reviewer with the card number, your worktree and your branch. Brief
   the tester with the card number, the deliverables, the commit, and your test app with its CDP
   port. On any FAIL, from either one, fix it, install the fix (step 5) and run both again: a
   review's fix needs a new test, and a test's fix needs a new review. The move to Human test is
   refused until the latest review and test have both passed, and while either pass started
   before the other's latest FAIL. If your session lacks the `card-reviewer` or the
   `card-tester`, say so on the card, run the one it has, and leave the move to Human test to the
   owner (yours would be refused while that rule is on), or ask them to turn the rule off or
   restart the session. On a Mac only: brief the card-tester to check through `kb` and the API,
   not cdp.mjs.
6b. If the card has `doneWhenChecks` (and `kanban_update_card` lists `doneWhen`), mark each check as
   you verify it in step 6, and say in the tester's brief that the card has them: its `CHECK <n>`
   lines mark them on the card.
7. Commit on the card's branch, in its worktree (a clear message with `(card #<n>)`, the
   Co-Authored-By line). Then push that branch to GitHub, never `main`:
   `git push -u origin <branch>` (`--force-with-lease` when you push it again after a rebase).
   `main` is pushed only in Ship it.
8. Route: no diff goes to Done; otherwise hand over (below) and move to Human test.

## Human test

Before the move, write into `nextStep`:

- one line: "Installed in <your test app> (Claude Kanban DSH Test N, CDP port <port>), branch
  `<branch>`, commit `<short sha>`";
- if the card had a bug report, the reproducer's proof next: the before and after steps, both
  screenshots (in `proof` when `kanban_update_card` lists it, else by path) and the test's name;
- **numbered steps of what to click in that test app** (where to start, what to do, what a pass
  looks like);
- a last line **"Broken if: ..."**.

If `kanban_update_card` lists `changesOutputs` among its allowed fields and the work made a finished
file for the owner (a report, a CSV, a PDF), list it there too, by its full Windows path, as
`[{"path": "C:\\...", "label": "..."}]`.
Skip this on an older app.

If `kanban_update_card` lists `proof` among its allowed fields, put the screenshots and reports
that prove the change in `proof`, by full Windows path, as `[{"path": "...", "label": "..."}]`, and
keep paths out of `nextStep`: the Human test panel shows each one as a picture or a readable report
(card #125). On an older app, as before.

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

Cards of this repo ship one at a time: the app holds the others in Ship it ("Waiting to ship",
card #79) until this ship is done, so each one ships on top of the last.

1. `kanban_ship_result` `shipping`.
1a. Bring the branch up to date in this repo's merge lane (cards #160, #162): every ship's merge
   goes through it. Don't rebase or merge the branch yourself.
   - Commit your work, then call `kanban_merge` with `rules` "every commit in origin/main..HEAD
     carries (card #<n>) for a card of this ship" and `checks` "cargo test --workspace", then end
     your turn. The lane rebases that branch onto the latest `origin/main` (the card's own branch,
     or the stack's branch on a lead) in its own worktree and runs the checks; it never pushes.
     The app sends you its verdict. If `kanban_merge` refuses because it found no checkout for the
     repo, `kanban_ship_result` `needs-hugh` with its reason, and stop; fix any other refusal it
     names and call it again.
   - On **PASS**, in your worktree: `git status --porcelain` is empty and `git rev-parse HEAD` is
     still its `base` (else `needs-hugh`); `git reset --hard <head>`; `git range-diff <onto>
     <base> <head>` shows each of your commits, changed only as the lane listed (else
     `needs-hugh`). Then fast-forward main to the branch in the main checkout:
     `git -C <checkout> merge --ff-only <branch>`. If `<checkout>` isn't on `main`, or the
     fast-forward fails, report `needs-hugh` with why, and stop. Then go on with step 2.
   - On **FAIL** (or "Merge unclear" on the card): `kanban_ship_result` `needs-hugh` with its
     reason, and stop. A commit with no `(card #<n>)` of this ship is someone else's unshipped
     work: never push it.
2. Make sure the tested commit is what's on `main` (`git -C <checkout> log -1`, tests pass).
3. `git -C <checkout> push origin main`. Then delete the card's branch on GitHub if you pushed it:
   `git push origin --delete <branch>` (each card of a stack: its own branch). Leave the card's
   worktree and its local branch: your session runs in that folder, and Windows won't remove a
   folder a running process is in. Log on the card the two commands that remove them once its
   session has stopped: `git -C <checkout> worktree remove <worktreePath>`, then
   `git -C <checkout> branch -d <branch>`.
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

Leave your worktree clean (commit real work, delete junk you made, never someone else's), log
what you did, and say where it stands. If Ship it didn't log the two commands that remove the
card's worktree and local branch, log them now. No after-deploy steps. The owner moves it to Done.
