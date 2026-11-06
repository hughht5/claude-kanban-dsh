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
  back to In progress, and sends the note to the stack's session.
- A child that needs a decision shows **Needs you** with the question (`blockedReason`) and answer
  buttons where the answer is a choice; the answer goes to the stack's session as Hugh's message.

Children don't appear as separate cards in other columns, so the board stays clear.

## Rules

- **Membership:** a child records its stack (`stackOf: <lead card number>`); the lead lists its
  children in order (`stack: [n, …]`). A card belongs to at most one stack. Removing a child from a
  stack puts it back on the board as an ordinary card in its own column.
- **Child state** is the child's own column (Planning / In progress / Human test / Blocked / Done),
  plus a new `passed` flag that "Passed" sets (a passed child is still in Human test until it ships).
- **The stack's column:** it moves to **Human test only when every child is ready** (Human test or
  Done). Open question for Hugh: or as soon as the first child is ready, with the rest shown as
  building. Ship it ships the whole stack (all children together: one branch, one PR).
- **Writers:** the stack's session owns its children's cards (it is the one writer for all of them),
  same as a card session owns its own card today.
- **Never** move a stack, or a child, into Ship it from a session: only Hugh.
- **Shipping as one (card #89):** Hugh can drop a built card, or a built stack, on a card in Ship
  it whose ship hasn't started: it joins that stack and ships in the same merge, marked Passed, and
  any ship request of its own is cancelled. On a ship that has started, the card ships straight
  after instead. A card in Ship it can also leave it for a stack (its ship cancelled). A stack is one
  repo. Sessions can't do any of these.
- Archive a stack: archives its children with it; restoring restores them.

## Where it touches the app

- The data model and API: `stack`, `stackOf`, `passed` on a card; `kb` and the `kanban_*` MCP tools
  gain stack membership (create a card into a stack, list a stack, set passed).
- The board UI (`ui/`): the stack card, the progress strip, expand/collapse (remember per stack), the
  child rows with inline steps and buttons; hide children from the columns.
- Dark mode: the strip's four colours must read in both themes.
- Existing loose cards are unaffected; a stack is opt-in.
