<!-- In progress column: an example prompt. The column's prompt points at this file as
     {prompts}\in-progress.md. Copy it to your private prompts folder and tailor the build, dev
     stack, sign-in and routing parts to your repo. -->

# In progress

<!-- toc -->
**Contents**

- [The card](#the-card)
- [Before you build](#before-you-build)
- [Build](#build)
- [Scope gate](#scope-gate)
- [Review and test it with subagents](#review-and-test-it-with-subagents)
- [Routing](#routing)
- [Hand over](#hand-over)
- [If your card leads a stack](#if-your-card-leads-a-stack)
- [If your card is in a stack](#if-your-card-is-in-a-stack)
<!-- /toc -->

Build this card to the standard in the repo's AGENTS.md / CLAUDE.md, prove it works yourself, then
route it by its real diff. The owner's time is the scarce resource: an agent loop is cheap, a human
test cycle that finds a bug you could have found is not.

## The card

Your card lives in the kanban app. Use its `kanban_*` tools (in Bash, `kb`; `kb` alone prints its
usage). There is no other board.

- `kanban_get_card`: the card (title, summary, nextStep, issue, pr, branch, worktreePath, devStack,
  logins, links, ship) and its log.
- `kanban_update_card`: change those fields. Pass `if_version` from your last read.
- `kanban_add_log`: a line on the card's log. This is the card's comment thread: anything this file
  says to "post on the card" goes here.
- `kanban_move_card`: move it, with a one-line reason. A move of your own card waits until your turn
  ends, so make it once the card is ready and then finish your message.
- `kanban_notify`: a desktop notification to the owner about the card.
- `kanban_list_cards`: the whole board.

Never move a card into Ship it, never merge, never push to main.

## Before you build

1. Read the card (`kanban_get_card`). If `ship.state` is `failed`, the card came back from Ship it:
   read `ship.detail` and the latest log lines, fix that failure first, and say what you changed.
   Don't re-run the pipeline unchanged.
2. Make sure the plan is on the card's GitHub issue, creating the issue if it is missing, and that
   the card's `issue` field holds it (`{number, url}`).
3. Rebase: `git fetch origin main && git rebase origin/main`. Cards run in parallel against a
   moving main, so build against what is true now.

## Build

Keep your steps as a to-do list, so the card shows "Step 4 of 7: ...". Load the tools
with ToolSearch `select:TaskCreate,TaskUpdate`. If they load, make one task per step of this build,
in order, and mark each in progress and completed as you go. If they don't load, skip this.

1. Write the code on this card's existing branch. Never create a branch or push to main.
2. Add tests in the nearest suite: the happy path and the obvious failure case.
3. Run `/code-review` on the diff against main and fix what it finds. If the forked review never
   returns, review inline and say so.
4. Lint, typecheck, build and test the way the repo documents. Some repos' root-level scripts fail
   in a worktree: then use the per-project ones. Use `set -o pipefail` and don't pipe a build
   through `tail`, which hides its exit code. All must pass.
5. Commit with a clear message. If a commit hook prompts for input it will hang an agent: do by hand
   what the hook would have done (a version bump the deploy needs, say), and say so.
6. Push the branch and open a PR. Put it on the card: `kanban_update_card` with `pr`
   (`{number, url}`).
7. Bring up the card's dev stack the way the repo documents, unless the diff is `no-login` (see
   Routing), which has nothing to sign into. Record its address on the card as `devStack`
   (`http://127.0.0.1:<port>`; `127.0.0.1` rather than `localhost`, which can resolve to IPv6
   first). If the stack runs from a separate clone (in WSL or a container, say), push first and let
   it fetch the branch: never copy the worktree across. If its start script can change under you
   while it runs (another card editing a shared copy), give each card its own copy. Put the steps
   for your own repos in your private prompts folder.

   Keep one stack per card, and only for cards in In progress or Human test (`kanban_list_cards`).
   The Ship it session stops the card's stack first thing. Stop the stack of any branch whose card
   is anywhere else, and clear that card's `devStack` and `logins`. A stop can report success and
   leave the server running: check the port, and confirm the running stack serves your code.
8. Verify it yourself, end to end against your stack. Test the non-admin path too when privilege
   can change the behaviour (permissions, admin-only UI, scoping, anything that reads role). A
   change that looks the same for everyone (layout, styling, copy) needs only one run. Log on the
   card which audiences you tested and why. When something is wrong, fix it and re-run the whole
   check until one pass is clean. After three failed passes, stop, log what fails, and move the card
   to Blocked with the reason in `blockedReason`.

   Where a screenshot shows the check better than words, put it in your message as a Markdown image
   with its full path in forward slashes, `![what it shows](C:/path/to/shot.png)`: the card's chat
   draws it, and the owner can click it for full size. One or two that prove the point.

   **Acceptance checks.** Only if `kanban_update_card` lists `doneWhen` among its allowed fields
   and the card has `doneWhenChecks`: as you verify, mark each check `met`, or `failed` with a short
   `note`, by sending the whole list back with `kanban_update_card`. The app records who changed
   each one and when.

Don't write vault docs yet; Ship it writes them. Log anything worth documenting on the card.

## Scope gate

Before routing, re-read the card's title and summary and list every deliverable it names (and, if
it has `doneWhenChecks`, check that every one is met). The card moves on only when every one is
built: the owner reads a card in Human test as "built", and a gap written into a log line, a PR body
or an issue doesn't change that. If you think the card should be split, or one part shipped ahead of
the rest, that is the owner's call. Stay in this column, write the list of deliverables with what is
and isn't done into `nextStep`, and ask.

## Review and test it with subagents

The owner's rules: every card's diff is reviewed by a subagent, and every card is tested by
a subagent, before it goes to Human test. The app holds you to both: a move from In
progress to Human test (or past it) is refused until the card's latest subagent review and latest
subagent test have both passed. A card with no diff (Done) needs neither.

### Prove a reported bug first

Only if `kanban_get_card` shows a `repro` field on your card: an older build of the app doesn't
set one, and then you skip this section. A stack's card never gets one: its stack's lead proves the
bug on the stack's install (see *If your card leads a stack*).

The owner's rule: a bug reported on a card is proved before the fix and after it, so the
card comes back to them with proof instead of another round of testing by hand. A report is the
owner's Found a problem, Fail in Human test or Report a bug, or a card-tester FAIL line that starts
`- BUG:`. The app then sets the card's `repro` to needed, and holds the move to Human test until
the latest reproducer run says FIXED (or NOT REPRODUCED). A card-reviewer's `- BUG:` line isn't a
report: it is found in the diff before the install, so fix it in the review loop.

If your session has the **`card-reproducer`** subagent (the Agent tool lists it), do these steps.
If it doesn't (the session started before this rule), the move to Human test is still held: say so
on the card, and ask the owner to turn the rule off or restart the session.

1. Before you change anything, run the `card-reproducer`: the Agent tool with
   `subagent_type: "card-reproducer"`. Brief it with the card number and where the change is. It
   reads the reports from the card, reproduces them in the test app (or the card's dev stack), saves
   the steps and a screenshot, and drafts a failing test. The app keeps them on the card as the
   "before".
2. Add the test it drafted. See it fail. Then fix the bug.
3. Put the fix where it runs: install it in the card's own test app (or bring the dev stack up on
   it).
4. Run the `card-reproducer` again. Give it the fix commit and the test's name. It checks that the
   app runs the fix, repeats the same steps, runs the test, and ends FIXED or REPRODUCED. The app
   keeps it as the "after".
5. On REPRODUCED, fix again and go back to step 3. On NOT REPRODUCED (first run) of a report from
   the owner, put one question for them in `blockedReason` with `choices`, and move the card to
   Blocked. When their answer has new detail, run the reproducer again. For a card-tester's report,
   log what the reproducer tried and go on: the hold lets the card through. On UNCLEAR, fix what it
   says (often: the test app isn't on the fix) and run it again.
6. Then review and test the card as below.

A message from the owner that they didn't mark as a bug follows the usual loop. If it reports a bug
they saw, you may still run the reproducer before and after the fix (the app records both); else log
`Not a bug report: <why>` on the card.

### Review it first

If your session has the **`card-reviewer`** subagent (the Agent tool lists it), do these steps.
If it doesn't (the session started before this rule), say so on the card and go on to the test,
but leave the move to Human test to the owner: yours would be refused while the review rule is on.
Or ask them to turn the rule off or restart the session.

1. Once the build is done and your own checks pass, run the `card-reviewer`: the Agent tool with
   `subagent_type: "card-reviewer"`. Wait for its report. If it runs in the background, wait for
   its notification.
2. Brief it with the card number and where the change is (the worktree and branch). Don't give
   it your conclusions. It finds the card's own commits by `(card #<n>)` in their messages, so put
   that in every commit message. If the card changed no repo, say what it delivered and where.
3. It reads the diff, changes nothing, and ends with `CARD: #<n>` and `VERDICT: PASS`, or
   `VERDICT: FAIL` with one line per problem. The app records it on the card (`review`, shown as
   Reviewed or Review failed).
4. On a FAIL, fix what it found and run a fresh reviewer. Its third FAIL since the card came into
   In progress sends the card to Blocked with the findings: the app does that itself.
5. Review before you test: a fix after the tester's pass means running both again.

### Then test it

1. Once the review has passed, run the **`card-tester`** subagent: the Agent
   tool with `subagent_type: "card-tester"`, and wait for its report (if it runs in the background,
   wait for its notification before you move the card). The app gives every card session this
   subagent; it can't edit files or move cards.
2. **Brief it like a stranger**, because it starts with no memory of your work and shouldn't take
   on your conclusions. Give it the card number, every deliverable from the summary, where the
   change is (worktree, branch, PR) and how to try it (the dev stack and which login; for the
   kanban app, the card's own test app (`kanban_test_app` take names it) and its CDP port, 9334
   for Test or 9333+N for Test N, and dry-run cards). Don't tell it what you found. If the
   card has `doneWhenChecks` (and `kanban_update_card` lists `doneWhen`), say so in the brief: the
   tester reads them from the card and ends with one `CHECK <n>: PASS|FAIL` line per check, which
   the app marks on the card its `CARD` line names. A PASS that fails a check counts as a FAIL.
3. It tests the real thing, fixes nothing, and ends with `CARD: #<n>` and `VERDICT: PASS`, or
   `VERDICT: FAIL` with one line per failure. The app reads those lines when it finishes and
   records the verdict on the card (`test`, shown as Tested or Test failed).
4. **On a FAIL**, fix what it found, run your own checks again, then run a fresh tester. After
   three failed rounds, stop: log what fails and move the card to Blocked with the reason in
   `blockedReason`.
5. If your card leads a stack and `kanban_stack_list` shows a `stage` for each child, each child
   reviews and tests itself on its own branch, and you test the integrated build (*If your card
   leads a stack*). Otherwise, as before: review and test each child before you move it, with the
   child's number in each brief so the `CARD` line names it, and review and test the lead's own
   work too: the app moves the stack to Human test only once the lead's own review and test have
   passed.
6. A failure that is a bug a person can see in the app or page comes on a line that starts
   `- BUG:`.

Say in the hand-over that the subagent review and test passed, and what they looked at. If the card
had a bug report, lead the hand-over with the reproducer's proof: the before and after steps, both
screenshots (as Markdown images), and the test's name.

## Routing

Route from the real diff, `git diff --name-only origin/main...HEAD`, not from the card's labels,
and say which class you picked:

- **No diff**: move to Done.
- **No-login**: nothing in the diff can change what a user signs into or sees (docs, CI config,
  agent prompts, scripts outside the product). Your private copy should list your repo's paths.
  Move to Human test: the owner reviews it there and moves it on.
- **Anything else, or any doubt** (`login`): move to Human test.

Move the card exactly one step, and never to Ship it or Deploy complete: the owner moves a card on
from Human test. If anything failed, don't move it: log what failed and stop. A card the owner set
to go on by itself to Deploy complete (the app's message says so) still goes to Human test with the
usual hand-over: the app then moves it into Ship it for them. So may a small, fully checked change
in a repo where the owner turned on small-change auto-ship: docs or CSS only, both subagents passed
on the commit you hand over, nothing after it. It still goes to Human test with the usual hand-over;
the app checks it, waits 10 minutes for the owner to stop it, then moves it on. So hand over only
work that is committed, and run both subagents last.

Hand over before you move it to Human test (next section). Human test has no session prompt of its
own: the links and how-to-test steps come from here, once.

Use `kanban_notify` when the card goes straight to Done, when you stop here blocked, and when the
hand-over below is up.

## Hand over

Human test is where the owner tests the card. Nothing runs there, so give everything needed from
here, once step 8 is clean and before the move. Write it once: your final message is the hand-over,
so don't also end with a separate build summary that repeats it.

### No-login card: review notes

Write into `nextStep`:

- **The diff in brief**: `git diff --stat origin/main...HEAD`, grouped by area, in a few lines.
- **What to look at**: the PR link if one is open, else the branch, and the files that matter. Say
  what changes in behaviour and how to judge it.

### Login card: the card's logins

1. **Is the stack current?** It must serve the commit you pushed. If you changed anything since,
   confirm it still serves `HEAD`: a file watcher can die while the ports still answer 200.
2. **Pick the first page for each audience**: the exact place step 1 of the how-to-test starts.
   Give a non-admin login only when privilege can change what the owner sees, by the same rule as
   your audience testing in step 8; say which you chose and why.
3. **Set the logins** with `kanban_update_card`, field `logins`: one entry per persona,
   `{label, persona, browser, url}`. The app draws each as a button on the card. The url is a
   sign-in link to the stack that lands on the page to test and carries no secret (how your dev
   stack signs a test persona in belongs in your private copy of this prompt). For example:

       [{ "label": "Admin", "persona": "admin", "browser": "Chrome · Work profile",
          "url": "http://127.0.0.1:3000/<sign-in page>?as=admin&next=%2Fsettings" }]

   - Name the browser profile on every login: opening a link in the wrong profile can sign that
     profile in as the other persona.
   - Never put a token, key, password or a link with a token in it on the card.
   - Check each url answers from Bash (`curl -s -o /dev/null -w "%{http_code}"`) before you write
     it.
   - Never open a link in the owner's browser yourself: it takes their focus.

### Then

4. Write the how-to-test steps into `nextStep`, `kanban_notify` that the card is ready for the owner
   to test (or to review, for a no-login card), and move it to Human test. If a step failed, don't
   move it: `kanban_notify` that it is blocked, log what failed and stop.
   If `kanban_update_card` lists `changesOutputs` among its allowed fields and the work made a
   finished file for the owner (a report, a CSV, a PDF), list it there first, by its full Windows
   path (not a WSL one: copy a WSL file to the Windows drive first):
   `[{"path": "C:\\full\\path\\report.md", "label": "Report"}]`. Skip this on an older app.

5. End your message with the hand-over. A few lines on what you built and verified, then numbered
   steps specific to this card's change: for a login card which login button to start from, what
   to click, what a pass looks like, which Chrome profile each login is for, and any trap that makes
   a correct build look broken (wrong org, a grant on the wrong org's page, the wrong profile); for
   a no-login card what to open, what to read or run, and what a pass looks like. The owner reads
   the bottom of the message and clicks, so don't end on a status table. If kanban_get_card shows an
   evidence field on your card, and its first run is from your latest test (evidence[0].run equals
   test.agentId and evidence[0].at equals test.at), put that run's shots in your final message as
   Markdown images, using evidence[0].shots[].path (the copied files, never the tester's own %TEMP%
   paths), and say that "before" is the new build just before the tester acted. If the first run is
   from an earlier test, show no shots and say the latest test took none.

**Later, while the card sits in Human test,** the logins keep working as long as the stack is up. If
the stack restarted on a new port, bring it up again and set the logins with the new port. If the
owner reports a bug, log it on the card and fix it through the build-and-verify loop.

## If your card leads a stack

A stack is one lead card (yours) and a set of child cards that go out together: one branch, one
Human test, one ship. `kanban_stack_list` (or `kb stack`) shows the lead and its children in order.

If `kanban_stack_list` shows a `stage` for each child, the children build in parallel, each in its
own session, worktree and branch, and you are the integrator. You build nothing: you merge each
child into the stack's branch, test the whole once, and hand it over. Your session runs on low
effort unless your card sets its own. A child's `stage` is `waiting` (on a sibling not yet merged),
`building`, `to-merge` (in Human test, not merged yet), `merged`, `passed` (the owner's Passed) or
`shipped`.

- **The merge brief.** When a child moves to Human test, the app starts or resumes your session
  with a brief: the child's number, its branch and head, the stack's branch, your worktree and the
  merge steps. One brief per child; they wait while you are busy.
- **Merge it with the merger.** If your session has the `card-merger` subagent, run it with the
  brief (its stack job) and wait for its verdict. In your worktree it detaches at the child's head,
  rebases onto the stack's branch, fast-forwards the stack's branch to the result and runs the
  tests. The child's branch is never rewritten, nothing goes to main and nothing is pushed. The app
  records the verdict on the child (`merge`). If your session has no `card-merger`, don't merge by
  hand: say so on the card, `kanban_notify`, and leave the child in `to-merge` for
  the owner.
- **What the app does next.** On a PASS it runs the stack's rules again, starts each child whose
  waits are now all merged (from the stack's branch), and tells the children still building that
  the stack's branch moved. On a FAIL it moves the child back to In progress with the reason, and
  the child's session rebases, fixes and tests again; its next move to Human test brings you a new
  brief. Don't move the children yourself.
- **Test the integrated build once.** When the stack is ready by the owner's rule (every child
  merged, or the first, by `stacks.human_test_when`), bring up its one build from the stack's
  branch: for a web product, push the stack's branch, open its one PR and bring up its one dev stack
  (*Build* steps 6 and 7), and set the logins on your card; for kanban-app, your card's own test
  app (`kanban-app.md`). Then run your own `card-tester` on it, briefed with every child's deliverables.
  Your own review counts as passed once every child's review has passed. If a child is merged after
  that, bring the build up to date and test again, so the owner tests what the stack's branch holds.
- **The app moves the stack to Human test** once it is ready, counting merged children only, and
  your test has passed. Don't move your card to Human test yourself. It moves the stack back to In
  progress if a child goes back.
- **Hand over** as in *Hand over*, once, for the stack: your `nextStep` holds the stack's
  how-to-test (where the build is, which login or test app, what to try across the children), and
  each child's `nextStep` keeps its own numbered steps, which its session wrote. Check each child's
  steps still hold on the integrated build; log on the child if one doesn't.
- **The owner's notes go to the child.** Found a problem, their answers and pasted screenshots go to
  the child's own session; you get a log line. If a log line says the owner's bug on a child is for
  your card-reproducer, and your session has the `card-reproducer`: once your install has the
  child's fix, run it with `CARD: #<child>` and the steps from the child's hand-over. The app
  records the result on the child and doesn't hold it. Passed on a child is an FYI to you. A child
  that goes back after its merge has its merge cleared, fixes forward and comes back to you with a
  new brief.
- **Never move a child, or the stack, into Ship it.** Only the owner ships, and the whole stack
  ships together.

Otherwise, as before: you own the children's cards and build each child yourself on your one branch
(one PR, one dev stack, one set of logins, all on your card), keep each child's `nextStep` as its
own numbered how-to-test (with a last line "Broken if: ..."), and move each child to Human test when
it is ready. The app moves the stack itself once its children are ready (every child, or the first,
by the owner's setting), and back to In progress if a child goes back. A question for the owner
about a child goes on that child (`blockedReason`, `choices`); their answer and any "Found a
problem" note come to you as a message. If that message names the `card-reproducer` and your session
has it, run it with `CARD: #<child>` after you install the fix. Never move a child, or the stack,
into Ship it.

## If your card is in a stack

Your card is in a stack when its `stackOf` names a lead. If `kanban_stack_list` (with the lead's
number) shows a `stage` for each child, you build this card yourself, in parallel with its siblings,
and the lead merges it into the stack's branch:

- **Your own worktree and branch.** Work in the worktree you made in Planning,
  `<checkout>/.claude/worktrees/card-<n>` on the branch `<stack branch>-<n>` from the stack's
  branch (the app's note names both; for
  kanban-app, `kanban-app.md` gives the place). If you have none yet, make it now and record
  `worktreePath` and `branch`. In *Before you build* step 3, rebase onto the stack's branch, not
  `origin/main`. Never commit to the stack's branch.
- **No PR and no dev stack.** Only the lead pushes, opens the stack's PR and boots a dev stack. Do
  *Build* steps 1 to 5 and 8 on your branch, without the push, the PR, the dev stack (steps 6 and 7)
  and the logins. Verify what you can without a stack, and say what is left for the stack's build.
- **Review and test on your branch.** Run your `card-reviewer` and then your `card-tester` (*Review
  and test it with subagents*), briefed with this card's number, your worktree and your branch. The
  move to Human test needs both passed.
- **Hand over on this card.** Write your own how-to-test into `nextStep`: numbered steps for the
  owner to follow on the stack's build, and a last line "Broken if: ...". Then move this card to
  Human test. That move stops your session (the conversation is kept) and brings the lead a brief to
  merge you. The owner's Passed on this card counts only once you are merged.
- **The base moved.** A note that the stack's branch moved means a sibling was merged into it.
  Before you move this card to Human test, rebase your branch onto the stack's branch and run your
  tests again.
- **Fix forward.** If the card comes back after a merge (Found a problem, a reviewer's finding, a
  merge FAIL), its merge is cleared. Rebase your branch onto the stack's branch (your merged commits
  drop out as already applied), resolve, add the fix as new commits, review and test again, and move
  the card to Human test again. The lead merges it again.
- **Detached.** If you are told this card was detached, it is its own card now: rebase onto
  `origin/main` keeping only its own `(card #<n>)` commits, as the app's note says, and go on as for
  any card.
- **Never move this card, the lead or the stack into Ship it.**

Otherwise, as before: the lead's session builds this card. Don't build it here; say so in your
message and stop.
