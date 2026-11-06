<!-- Planning column: an example prompt. The column's prompt points at this file as
     {prompts}\planning.md. Copy it to your private prompts folder and tailor it to your repo. -->

# Planning

<!-- toc -->
**Contents**

- [The card](#the-card)
- [Route it first](#route-it-first)
- [Plan it](#plan-it)
- [Check the plan with a subagent](#check-the-plan-with-a-subagent)
- [If the card goes on by itself](#if-the-card-goes-on-by-itself)
- [The board's rules](#the-boards-rules)
- [If your card leads a stack](#if-your-card-leads-a-stack)
- [If your card is in a stack](#if-your-card-is-in-a-stack)
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

## Route it first

Do this only if your session has the card-router subagent (the Agent tool lists `card-router`).
If it doesn't, say so on the card in one log line and go on to *Plan it*.

1. Read your card. Skip this section if the card already has a `route`, or if it is a child in a
   stack (it has `stackOf`): a child is planned by its lead.
2. Run the card-router subagent (`subagent_type` `card-router`). Brief it with the card number and
   the checkout. Wait for its result: if it runs in the background, wait for its notification.
3. The app records its route on the card (`route`: size, model, effort, check, reason). Read it.
   - **small**: keep the plan short.
   - **normal**: plan as usual.
   - **split**: create the child cards it names (`kanban_create_card` with `stack`: your card's
     number; a lead adds only the children it lacks), then plan as a lead (*If your card leads a
     stack*, below).
4. The route sets the session's model and effort in Planning and In progress; the app switches
   them for you (it may restart your session between turns; the conversation carries on). Never
   type /model or /effort yourself: Claude Code would save them as the owner's default. The owner's
   drag, or their model and effort in the panel, beat the route.
5. Never run the card-router for a stack's child. Run it once per card: the owner's Route again (the
   panel) clears the route and asks you to run it again.
6. A card routed small goes on to In progress by itself once its plan critique passes (the owner's
   setting): the app moves it when your turn ends and sends you the In progress prompt.

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
5a. **Done when, and its checks.** Only if `kanban_update_card` lists `doneWhen` among its allowed
   fields (an older build doesn't: then skip this step). With the plan, write `doneWhen`: one plain
   line, the card is done when ... (what the owner will see). And write `doneWhenChecks`: 3 to 7
   acceptance checks, each one thing a tester can see pass or fail, in the order a tester would try
   them, as a list of strings (`["the card face shows the line on one line", "a tick survives a
   reload"]`). The board shows them on the card, and the card-tester reports on each one. Keep to
   plain words: the board refuses any field with the word "password" or a `ct=` link in it (the
   error names the check), so say "the login secret" instead.
6. Check the plan with the card-critic subagent (below), and fix what it finds.
7. If a decision is the owner's (scope, a trade-off, anything the card doesn't settle), ask it in
   your message, put the question in `nextStep`, and leave the card in Planning. If something
   outside the card blocks it, move it to Blocked with the reason in `blockedReason`. Otherwise tell
   the owner the plan is ready: they move the card to In progress to start the build.

## Check the plan with a subagent

If your session has the card-critic subagent (the Agent tool lists it), do this once the whole plan
is in `summary`, before you say the plan is ready or move the card on. If it doesn't (the session
started before this rule), say so on the card and go on as before, but leave the move to In progress
to the owner: yours would be refused. If the app's message says the owner turned the plan critique
off, skip it.

1. Run it: the Agent tool, `subagent_type` "card-critic". Brief it like a stranger: the card number
   and where the code is (the checkout and branch). Don't give it your conclusions. Wait for its
   result; if it runs in the background, wait for its notification.
2. It ends with CARD and VERDICT lines, and the app records its verdict on the card (a "Plan
   critiqued" or "Critique failed" chip).
3. Fix the plan in `summary`, then run a fresh critic. A finding that starts "HUGH:" is the owner's
   to decide: put the question in `nextStep` (or in `blockedReason` with `choices`) and leave the
   card in Planning.
4. Any edit to `summary` after a pass makes the critique needed again: finish the plan first, and
   run the critic last. `nextStep` and the log don't count.
5. Its third FAIL since the card came into Planning sends the card to Blocked, with the findings in
   `blockedReason`.
6. Your own move of the card into In progress (or straight on to Human test) is refused until its
   latest critique passed. The owner's moves never are.

## If the card goes on by itself

The owner can set a card to go on by itself to Human test or Deploy complete ("Take it to", the
card's `autoTo`); the app's message says so when they have. Then don't wait for the owner to move
it on:

- When the plan is ready, its critique has passed (if your session has the card-critic) and nothing
  in it is theirs to decide, move the card to In progress yourself
  (`kanban_move_card`) as the last thing in your turn. The app then sends you the In progress
  prompt, and you build it.
- If a decision is the owner's, ask it and leave the card in Planning: it waits for the owner as
  usual.
- If something else holds it (another card building in the same checkout, say), say so and leave
  it in Planning: the owner moves it on.
- For Deploy complete, the app moves the card from Human test into Ship it for the owner, which is
  their approval to ship it with no human test. Plan the testing with that in mind.

## The board's rules

- Columns: Backlog (noted, not decided), Blocked (waiting on an answer: put the reason in
  `blockedReason`), Planning (the approach isn't settled, or the owner has to agree it), In progress
  (a session is building it), Human test (pushed, a PR is open, the stack is up, `nextStep` says
  what to try and the card has logins), Ship it (the owner approved it), Deploy complete (the deploy
  is proven), Done. Move your card when the work's state changes, with a one-line reason; Human test
  back to In progress is normal.
- Logins: one entry per persona (`{label, persona, browser, url}`), where url is a one-click
  sign-in link to the dev stack that carries no secret (127.0.0.1, never localhost), and browser
  names the browser profile to open it in. How your dev stack signs a test persona in belongs in
  your private prompts folder. Never put a token, key, password or a `ct=` link on the card. Remove
  the logins when the stack stops.
- Never move a card into Ship it. Only the owner moves a card into Ship it, or the app for them when
  they set the card to go on by itself (Take it to), or turned on small-change auto-ship for that
  repo. Never merge, never push to main.
- If `kanban_update_card` lists `blockedBy`: when this card can't start until other cards are
  ready (it needs their work first), name them in your message and in `nextStep`. The owner sets
  `blockedBy` (a card's own session sets it only on the children of the stack its card leads). A
  card the owner puts in Blocked starts by itself once the cards it waits on are ready.

## If your card leads a stack

A stack is one lead card (yours) and a set of child cards that go out together: one branch, one
Human test, one ship. `kanban_stack_list` (or `kb stack`) shows the lead and its children in order.

If `kanban_stack_list` shows a `stage` for each child, the children build in parallel, each in its
own session, and you are the stack's integrator:

- **You build nothing yourself.** Each child plans and builds itself. If your card names work of
  its own, make that work a child (`kanban_create_card` with `stack: <your number>`) and don't plan
  it here.
- **Set up the stack's branch and worktree** from main, as in *Plan it* step 2 (for kanban-app, the
  place `kanban-app.md` gives), and record `worktreePath` and `branch` on your card. Every child
  makes its own branch from this one, and the merger merges each child back into it.
- **Set the order with Blocked by.** A child that needs a sibling's work first gets that sibling's
  number in its `blockedBy` (`kanban_update_card`). It starts only once that sibling is merged, and
  always from the stack's branch. A child that waits on nothing starts as soon as the stack does.
  Waits stay inside the stack, and never between you and a child.
- **Check each child has a plan.** Each child's own session writes its `summary` (every
  deliverable) and `nextStep`. Read them. If one is missing, or two children change the same thing,
  log it on both cards and say so in your message.
- **The owner's move of your card into Planning or In progress approves the stack.** It approves
  every start and resume of a session in the stack while your card is in Planning, In progress or
  Human test. The children still go through the critic: each moves itself to In progress once its
  plan has passed and your card is In progress. Don't move the children yourself.
- Tell the owner the stack's plan is ready: the children, which waits on which, and anything that is
  theirs to decide. They move your card to In progress to start the build.
- **Never move a child, or the stack, into Ship it.** Only the owner ships, and the whole stack
  ships together.

Otherwise, as before: you plan every child yourself (keep each child's `nextStep` as its own
numbered how-to-test, with a last line "Broken if: ..."), add cards with `kanban_create_card` and
`stack: <your number>` or `kanban_stack_add`, and the stack builds on your one branch, with one PR,
one dev stack and one set of logins. A question for the owner about a child goes on that child
(`blockedReason`, and `choices` for their answer buttons); their answer and any "Found a problem"
note come to you as a message. Critique each child's plan with the card-critic, as above, with the
child's number in the brief (it ends `CARD: #<child>`), and your own plan too: your move of a child
into In progress is refused until its critique passed. Never move a child, or the stack,
into Ship it.

## If your card is in a stack

Your card is in a stack when its `stackOf` names a lead. If `kanban_stack_list` (with the lead's
number) shows a `stage` for each child, you plan and build this card yourself, in parallel with its
siblings, and the lead merges it into the stack's branch:

- **Plan only this card.** Follow *Plan it*, but in place of step 2 make your own worktree
  `<checkout>-<n>` on the branch `<stack branch>-<n>`, from the stack's branch (the app's note names
  both; for kanban-app, `kanban-app.md` gives the place), and record `worktreePath` and `branch`.
  Never work in the lead's worktree or on the stack's branch.
- **Your siblings build at the same time.** If your plan touches what a sibling changes, log it on
  both cards and on the lead. If you wait on a sibling (your `blockedBy`), you started only once it
  was merged, so the stack's branch already holds its work.
- **The critic.** If your session has the `card-critic` subagent (the Agent tool lists it), run it
  on your plan and fix what it finds until it passes. Then, if the lead is In progress, move this
  card to In progress yourself, as the last thing in your turn: the owner's move of the lead
  approved it. If the lead isn't In progress yet, leave the card in Planning: the app sends you a
  message when the owner moves the lead to In progress, and you move it on then. If your session has
  no `card-critic`, leave the card in Planning for the owner. The app never moves a child to In
  progress.
- **A question for the owner** goes on this card: the question in `blockedReason`, and the answers
  they can pick in `choices`. Their answer comes to this session.
- **Never move this card, the lead or the stack into Ship it.**

Otherwise, as before: the lead's session plans and builds this card. Don't plan or build it here;
say so in your message and stop.
