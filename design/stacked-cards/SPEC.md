# Stacked cards (option B): one card that manages a set of child cards

Hugh, 4 Nov 2026. Mockup: `mockup.html` beside this file (open it in a browser; the rows expand).

## Why

Work often goes out as one branch and one dev stack that carries several cards. One round of
feedback is the example: cards 44 to 56 are built on two branches, so Hugh tests them on two stacks.
Thirteen loose cards spread across columns hide that, and every card repeats the same sign-in links.
A stack shows the set as one thing he tests together, while each child keeps its own steps and state.

## What it looks like (option B, chosen over a "deck" and a swimlane)

**The stack card** (the lead card, e.g. #44) sits in a column like any card, with an accent left edge
and a stack icon:
- Title, then a line with what the stack is, its branch, and whether its dev stack is up.
- **A segmented progress strip**: one segment per child, coloured by state. Amber = in Human test,
  green = passed, blue = building, red = needs Hugh. Under it, a legend with counts ("2 to test,
  1 passed, 4 building, 1 needs you") and "3 of 8 ready" at the top right.
- **What's needed to test, once for the whole stack**: the stack's sign-in links (tokenless
  links to its dev stack, one per role), the PR, Open session.
- **Show N cards / Hide N cards** expands the children.

**Each child row** (when expanded): a status dot, number, title, and on the right its state
("Test · 3 steps", "Passed", "Needs you"). Clicking a row opens the child in an overlay over the
board (inline until card #64, 4 Nov 2026, which found the stack card too narrow to read steps in):
- **Its own how-to-test** (the child's `nextStep`): which login, numbered steps, and "Broken if: …".
- **Passed** and **Found a problem** buttons. "Found a problem" asks for a note, moves the child
  back to In progress, and sends the note to the child's own session (in a stack built the old way,
  to the stack's session; the lead gets a log line either way).
- A child that needs a decision shows **Needs you** with the question (`blockedReason`) and answer
  buttons where the answer is a choice; the answer goes to the child's own session as Hugh's
  message (the old way, to the stack's session).

Children don't appear as separate cards in other columns, so the board stays clear.

## Rules

- **Membership:** a child records its stack (`stackOf: <lead card number>`); the lead lists its
  children in order (`stack: [n, …]`). A card belongs to at most one stack. Removing a child from a
  stack puts it back on the board as an ordinary card in its own column.
- **Child state** is the child's own column (Planning / In progress / Human test / Blocked / Done),
  plus a new `passed` flag that "Passed" sets (a passed child is still in Human test until it ships).
  Since card #113 each child also has a **stage** (waiting, building, to-merge, merged, passed,
  shipped; see *Parallel build*), and Passed is refused on a child that isn't merged.
- **The stack's column:** it moves to **Human test once the stack is ready** by the setting
  `stacks.human_test_when`: `all` (the default, every child in Human test or later) or `first` (as
  soon as one is), and back to In progress when a child goes back. Since card #96 the lead's own
  test and review must have passed too. Since card #113 a child in Human test counts only once it
  is merged into the stack's branch, and the lead's review counts as passed when every child's
  review has. Ship it ships the whole stack (all children together: one branch, one PR).
- **Writers:** in a stack built in parallel (card #113) each child's own session writes its own card
  (plan, how-to-test, questions), and the lead's session writes the lead card, the order (a child's
  `blockedBy`) and the merges. In a stack built the old way, the stack's session owns its children's
  cards (it is the one writer for all of them), same as a card session owns its own card.
- **Never** move a stack, or a child, into Ship it from a session: only Hugh.
- **Shipping as one (card #89):** Hugh can drop a built card, or a built stack, on a card in Ship
  it whose ship hasn't started: it joins that stack and ships in the same merge, marked Passed, and
  any ship request of its own is cancelled. On a ship that has started, the card ships straight
  after instead. A card in Ship it can also leave it for a stack (its ship cancelled). A stack is one
  repo. Sessions can't do any of these.
- Archive a stack: archives its children with it; restoring restores them.

## Parallel build (card #113, 6 Nov 2026)

Hugh, 6 Nov 2026: "In this new world, I'm not sure that stacks should be working the way they used
to." Until then the lead's session built every child itself, one after another. Now a stack is a
group for integration, test and ship, not one builder. The router's decisions R1 to R6 are in
`PLAN.md` (Decisions 15 to 20).

- **Each child is an ordinary card session.** It plans itself, runs the card-critic on its plan (if
  its session has it), and builds in its own worktree `<checkout>-<n>` on the branch
  `<stack branch>-<n>`, made from the stack's branch. No commits to the stack's branch, no PR, no
  dev stack. It unit-tests, runs its card-reviewer and card-tester on its branch, writes its own
  how-to-test into `nextStep`, and moves itself to Human test. Its session then stops (the
  conversation is kept), so a child waiting to be merged holds no session slot.
- **Hugh's move of the lead approves the stack (R1).** His move of the lead into Planning or In
  progress approves every start and resume in the stack while the lead is in Planning, In progress
  or Human test. The app moves each child that waits on nothing from Backlog to Planning and starts
  it. It never moves a child to In progress: the child does that itself once its critic has passed
  and the lead is In progress.
- **Order from Blocked by (R4).** A child that waits on a sibling (`blockedBy`, card #109) starts
  only once that sibling is merged, and always from the stack's branch.
- **The lead is the integrator.** It builds nothing (a lead with work of its own makes it a child)
  and runs on low effort unless its card has an `effortOverride`. Its session start goes to the
  front of the start queue; the cap is unchanged. When a child enters Human test, the app starts or
  resumes the lead's session with a merge brief, and the lead runs the card-merger (card #100's
  "stack" job, effort high, R5): in the lead's worktree, detach at the child's head, rebase onto the
  stack's branch, fast-forward the stack's branch (R3). The child's branch is never rewritten.
- **After the merger's verdict:** on PASS the stack rules run again, siblings whose waits are now
  all merged start, and siblings still building are told the stack's branch moved (rebase onto it
  before you finish). On FAIL the child goes back to In progress with the reason and its session is
  resumed to rebase, resolve and test again.
- **Fix forward.** A child that goes back after its merge (Found a problem, a reviewer's finding, a
  merge FAIL) has its merge record cleared. It rebases onto the stack's branch, adds new commits and
  is merged again.
- **Stages.** Each child has a stage: waiting, building, to-merge (in Human test, not merged),
  merged, passed, shipped. `kanban_stack_list` shows it, and the board reads it (`stack_stages`;
  card #116 draws it). Passed is refused on a child that isn't merged. A child with no branch of its
  own (a stack built the old way) counts as merged.
- **One Human test, one ship.** The app moves the stack to Human test once it is ready by the
  board's rule, counting merged children only, and the lead's own subagent test of the integrated
  build has passed. The lead installs that build once (this app: the test app; a product repo: its one
  dev stack) and runs its own card-tester on it. Its review counts as passed when every child's
  review has. One ship for the stack, as before: every child is on the stack's branch.
- **Hugh's notes go to the child.** Found a problem, answers and pasted screenshots go to the
  child's own session; the lead gets a log line. Passed stays an FYI to the lead.
- **Detach** is refused for a merged child (send it back with Found a problem, or let it ship with
  the stack). An unmerged child keeps its session and column (from Human test it goes back to In
  progress) and is told to rebase onto `origin/main` keeping only its own "(card #n)" commits. A
  child with no session goes to Planning as before. **Join:** the card keeps its session and is told
  its stack and base; a built card is merged by the lead.
- **Prompts:** the stack sections of `planning.md` and `in-progress.md`, and `kanban-app.md`'s "A
  stack card". Each starts "If `kanban_stack_list` shows a `stage` for each child", so a build
  without card #113 keeps the old way.

## Reviewing a stack in Human test (card #116)

The stack is one stop in Human test's Previous and Next queue (card #102). Its overlay steps
through the children in Human test, in stack order, then a stack page whose Pass ships the stack.
Pass is P then Enter, as for every card; a child's Fail clears only its own Passed; a child that
isn't merged (card #113's stage) or whose merge failed can't be passed.

## Where it touches the app

- The data model and API: `stack`, `stackOf`, `passed` on a card; `kb` and the `kanban_*` MCP tools
  gain stack membership (create a card into a stack, list a stack, set passed).
- The board UI (`ui/`): the stack card, the progress strip, expand/collapse (remember per stack), the
  child rows with inline steps and buttons; hide children from the columns.
- Dark mode: the strip's four colours must read in both themes.
- Existing loose cards are unaffected; a stack is opt-in.
