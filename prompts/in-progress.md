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
- [Test it with a subagent](#test-it-with-a-subagent)
- [Routing](#routing)
- [Hand over](#hand-over)
- [If your card leads a stack](#if-your-card-leads-a-stack)
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

Work in the card's worktree (`worktreePath`), on its existing branch. Use the repo's own documented
commands for building, testing and running it (its README, AGENTS.md or CLAUDE.md). If your repo
has traps an agent falls into (a hook that prompts, a version bump the deploy needs), write them
into your private copy of this prompt.

1. Write the code on this card's branch. Never create a branch or push to main.
2. Add tests in the nearest suite: the happy path and the obvious failure case.
3. Run `/code-review` on the diff against main and fix what it finds. If the review never returns,
   review inline and say so.
4. Lint, typecheck, build and test the way the repo documents. Use `set -o pipefail` and don't pipe a
   build through `tail`, which hides its exit code. All must pass.
5. Commit with a clear message. If a commit hook prompts for input it will hang an agent: do by hand
   what the hook would have done, and say so.
6. Push the branch and open a PR. Put it on the card: `kanban_update_card` with `pr`
   (`{number, url}`).
7. Bring up the card's dev stack the way the repo documents, unless the diff is `no-login` (see
   Routing), which has nothing to sign into. Record its address on the card as `devStack`
   (`http://127.0.0.1:<port>`; `127.0.0.1` rather than `localhost`, which can resolve to IPv6 first).

   Keep one stack per card, and only for cards in In progress or Human test (`kanban_list_cards`).
   The Ship it session stops the card's stack first thing. Stop the stack of any branch whose card
   is anywhere else, and clear that card's `devStack` and `logins`. A stop can report success and
   leave the server running: check the port, and confirm the running stack serves your code.
8. Verify it yourself, end to end against your stack. Test the non-admin path too when privilege
   can change the behaviour (permissions, admin-only UI, scoping, anything that reads role). A change
   that looks the same for everyone (layout, styling, copy) needs only one run. Log on the card which
   audiences you tested and why. When something is wrong, fix it and re-run the whole check until
   one pass is clean. After three failed passes, stop, log what fails, and move the card to Blocked
   with the reason in `blockedReason`.

   Where a screenshot shows the check better than words, put it in your message as a Markdown image
   with its full path in forward slashes, `![what it shows](C:/path/to/shot.png)`: the card's chat
   draws it, and the owner can click it for full size. One or two that prove the point.

Don't write the release docs yet; Ship it writes them. Log anything worth documenting on the card.

## Scope gate

Before routing, re-read the card's title and summary and list every deliverable it names. The card
moves on only when every one is built: the owner reads a card in Human test as "built", and a gap
written into a log line, a PR body or an issue doesn't change that. If you think the card should be
split, or one part shipped ahead of the rest, that is the owner's call. Stay in this column, write
the list of deliverables with what is and isn't done into `nextStep`, and ask.

## Test it with a subagent

Every card is tested by a subagent before it goes to Human test. The app holds you to it: a move
from In progress to Human test (or past it) is refused until the card's latest subagent test
passed. A card with no diff (Done) needs none.

1. Once the build is done and your own checks pass, run the **`card-tester`** subagent: the Agent
   tool with `subagent_type: "card-tester"`, and wait for its report (if it runs in the background,
   wait for its notification before you move the card). The app gives every card session this
   subagent; it can't edit files or move cards.
2. **Brief it like a stranger**, because it starts with no memory of your work and shouldn't take
   on your conclusions. Give it the card number, every deliverable from the summary, where the
   change is (worktree, branch, PR) and how to try it (the dev stack and which login). Don't tell it
   what you found.
3. It tests the real thing, fixes nothing, and ends with `CARD: #<n>` and `VERDICT: PASS`, or
   `VERDICT: FAIL` with one line per failure. The app reads those lines when it finishes and
   records the verdict on the card (`test`, shown as Tested or Test failed).
4. **On a FAIL**, fix what it found, run your own checks again, then run a fresh tester. After
   three failed rounds, stop: log what fails and move the card to Blocked with the reason in
   `blockedReason`.
5. If your card leads a stack, test each child before you move it: put the child's number in the
   brief, so the tester's `CARD` line names it.

Say in the hand-over that the subagent test passed, and what it tried.

## Routing

Route from the real diff, `git diff --name-only origin/main...HEAD`, not from the card's labels,
and say which class you picked:

- **No diff**: move to Done.
- **No-login**: nothing in the diff can change what a user signs into or sees (docs, CI config,
  agent prompts, scripts outside the product). Your private copy should list your repo's paths.
  Move to Human test: the owner reviews it there and moves it on.
- **Anything else, or any doubt** (`login`): move to Human test.

Move the card exactly one step, and never to Ship it or Deploy complete: the owner moves a card on
from Human test. If anything failed, don't move it: log what failed and stop. A card set to go on
by itself to Deploy complete (the app's message says so) still goes to Human test with the usual
hand-over: the app then moves it into Ship it for the owner.

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
   - Check each url answers from Bash (`curl -s -o /dev/null -w "%{http_code}"`) before you write it.
   - Never open a link in the owner's browser yourself: it takes their focus.

### Then

4. Write the how-to-test steps into `nextStep`, `kanban_notify` that the card is ready to test (or
   to review, for a no-login card), and move it to Human test. If a step failed, don't move it:
   `kanban_notify` that it is blocked, log what failed and stop.
5. End your message with the hand-over. A few lines on what you built and verified, then numbered
   steps specific to this card's change: which login to start from, what to click, what a pass looks
   like, and any trap that makes a correct build look broken; for a no-login card what to open, what
   to read or run, and what a pass looks like. The owner reads the bottom of the message and clicks,
   so don't end on a status table.

**Later, while the card sits in Human test,** the logins keep working as long as the stack is up. If
the stack restarted on a new port, bring it up again and set the logins with the new port. If the
owner reports a bug, log it on the card and fix it through the build-and-verify loop.

## If your card leads a stack

A stack is one lead card (yours) that manages a set of child cards built on one branch and tested
on one dev stack. `kanban_stack_list` (or `kb stack`) shows the lead and its children in order.

- **You own the children's cards.** You are the one writer for all of them: build each child, and
  keep each child's `nextStep` as its own numbered how-to-test (which login, the steps, and a last
  line "Broken if: ..."). Add a card with `kanban_create_card` and `stack: <your number>`, or an
  existing one with `kanban_stack_add`.
- **Move each child to Human test when it is ready**, with a one-line reason. Don't move your own
  card to Human test: the app moves the stack itself once its children are ready (every child, or
  the first, by the owner's setting), and back to In progress if a child goes back. It tells you
  when it does.
- **One branch, one PR, one dev stack, one set of logins**, all on the lead. The children carry none.
- **A question for the owner about a child** goes on that child: the question in `blockedReason`,
  and the answers to pick in `choices` (a list of short strings), so it is answered with a button.
  The answer, and any "Found a problem" note, comes to you as a message.
- **Never move a child, or the stack, into Ship it.** Only the owner ships, and the whole stack ships
  together.
