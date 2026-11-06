<!-- Planning column: an example prompt. The column's prompt points at this file as
     {prompts}\planning.md. Copy it to your private prompts folder and tailor it to your repo. -->

# Planning

<!-- toc -->
**Contents**

- [The card](#the-card)
- [Plan it](#plan-it)
- [If the card goes on by itself](#if-the-card-goes-on-by-itself)
- [The board's rules](#the-boards-rules)
- [If your card leads a stack](#if-your-card-leads-a-stack)
<!-- /toc -->

Work out how this card should be built, set it up so a build can start, and write the approach on
the card. Don't build it here.

## The card

Your card lives in the kanban app. Use its `kanban_*` tools (in Bash, `kb`; `kb` alone prints its
usage). There is no other board.

- `kanban_get_card`: the card (title, summary, nextStep, repo, issue, pr, branch, worktreePath,
  links) and its log.
- `kanban_update_card`: change those fields. Pass `if_version` from your last read.
- `kanban_add_log`: a line on the card's log, its comment thread.
- `kanban_move_card`: move it, with a one-line reason. A move of your own card waits until your turn
  ends.
- `kanban_notify`: a desktop notification to the owner about the card.

## Plan it

1. Read the card: its summary, links and issue (`gh issue view <n>`), and anything they point at.
2. Make a worktree for the card off `origin/main`, in or beside the repo's checkout
   (`git fetch origin && git worktree add -b <branch> <path> origin/main`), and record it with
   `kanban_update_card` (`worktreePath`, `branch`).
3. Read the code the change touches, and the repo's README and AGENTS.md / CLAUDE.md, before
   deciding anything.
4. Make sure the card has a GitHub issue with the plan on it, and that the card's `issue` field
   holds it (`{number, url}`).
5. Write the approach into `summary` and the first build step into `nextStep`. List every
   deliverable the card names, so In progress can check them off.
6. If a decision is the owner's (scope, a trade-off, anything the card doesn't settle), ask it in
   your message, put the question in `blockedReason` with the answers to pick in `choices` (a
   list of short strings), and leave the card in Planning. If something outside the card blocks it,
   move it to Blocked with the reason in `blockedReason`. Otherwise say the plan is ready: the owner
   moves the card to In progress to start the build.

## If the card goes on by itself

The owner can set a card to go on by itself to Human test or Deploy complete ("Take it to", the
card's `autoTo`); the app's message says so when they have. Then don't wait for the owner to move it on:

- When the plan is ready and nothing in it is the owner's to decide, move the card to In progress yourself
  (`kanban_move_card`) as the last thing in your turn. The app then sends you the In progress
  prompt, and you build it.
- If a decision is the owner's, ask it and leave the card in Planning: it waits for the owner as usual.
- If something else holds it (another card building in the same checkout, say), say so and leave
  it in Planning: the owner moves it on.
- For Deploy complete, the app moves the card from Human test into Ship it for the owner, which is their
  approval to ship it with no human test. Plan the testing with that in mind.

## The board's rules

- Columns: Backlog (noted, not decided), Blocked (waiting on an answer: put the reason in
  `blockedReason`), Planning (the approach isn't settled, or the owner has to agree it), In progress
  (a session is building it), Human test (pushed, a PR is open, `nextStep` says what to try and the
  card has what the owner needs to sign in), Ship it (the owner approved it), Deploy complete (the deploy
  is proven), Done. Move your card when the work's state changes, with a one-line reason; Human test
  back to In progress is normal.
- Logins: one entry per persona (`{label, persona, browser, url}`), where url is a sign-in link to
  the dev stack that carries no secret. Never put a token, key, password or a link with a token in
  it on the card. Remove the logins when the stack stops.
- Never move a card into Ship it: only the owner does, when they approve it, or the app for them when
  they set the card to go on by itself to Deploy complete. Never merge, never push to main.

## If your card leads a stack

A stack is one lead card (yours) that manages a set of child cards built on one branch and tested
on one dev stack. `kanban_stack_list` (or `kb stack`) shows the lead and its children in order.

- **You own the children's cards.** You are the one writer for all of them: plan each child (it gets
  built in In progress), and keep each child's `nextStep` as its own numbered how-to-test (which
  login, the steps, and a last line "Broken if: ..."). Add a card with `kanban_create_card` and
  `stack: <your number>`, or an existing one with `kanban_stack_add`.
- **Move each child to Human test when it is ready**, with a one-line reason. Don't move your own
  card to Human test: the app moves the stack itself once its children are ready (every child, or
  the first, by the owner's setting), and back to In progress if a child goes back. It tells you
  when it does.
- **One branch, one PR, one dev stack, one set of logins**, all on the lead. The children carry none.
- **A question for the owner about a child** goes on that child: the question in `blockedReason`,
  and the answers to pick in `choices`, so it is answered with a button. The answer, and any "Found
  a problem" note, comes to you as a message.
- **Never move a child, or the stack, into Ship it.** Only the owner ships, and the whole stack ships
  together.
