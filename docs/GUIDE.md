# Claude Kanban DSH

A Rust desktop app (Tauri 2): a kanban board of its own. Each card owns a live Claude Code session in an embedded terminal, and the app itself dispatches moves to those sessions. Its cards live in `%USERPROFILE%\.claude-kanban-dsh\board.db`.

**Install or update:** `powershell -ExecutionPolicy Bypass -File scripts\install.ps1` (per-user, no admin; puts `kb` on your PATH).

- **The plan:** [PLAN.md](../PLAN.md)
- **What each phase must pass:** [ACCEPTANCE.md](../ACCEPTANCE.md)
- **How to undo each change:** [UNDO.md](../UNDO.md)
- **What was installed to build it:** [INSTALLED.md](../INSTALLED.md)

## Test version

A second copy of the app, "Claude Kanban DSH Test", runs beside the live one so a change can be tried without disturbing your work. It has a warning bar across the top of the board and says "(test)" in its window title, tray tooltip and toasts. It is fully separate: its own install (`%USERPROFILE%\Programs\Claude Kanban DSH Test`), board and data (`%USERPROFILE%\.claude-kanban-dsh-test`), port, quit event and single-instance lock. Its `kb.exe` talks only to the test app and is not on PATH, so `kb` stays the live one.

- **Install or update the test copy:** `powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Channel test`. The first install copies the live board into the test folder; later installs keep the test board.
- **Refresh the test board from the live one:** add `-FreshCopy`. The old test board is kept in `%USERPROFILE%\.claude-kanban-dsh-test\backups`.
- **Copy a board by hand:** `kb copy-board --to <path> [--from <path>] [--replace]` takes a consistent snapshot (SQLite `VACUUM INTO`, safe while the live app runs). `--from` defaults to the live board.
- **Swap a change into the live app:** once it works in the test copy, run `scripts\install.ps1` (no `-Channel`) from the same commit. The live app quits cleanly, its sessions are suspended and resume at the next start.

Sessions started in the test copy are real Claude Code sessions on real repos, so treat its cards with the same care as the live ones. Sessions that were running on the live board when it was copied are marked suspended in the copy and never resumed there.

## Layout

| Folder | What goes there |
|---|---|
| `src-tauri/` | The Tauri app: store, sessions, Job Object, activity, dispatch, the localhost API (MCP, REST, hooks) |
| `crates/kb/` | `kb.exe`, the CLI: `import`, `card`, `update`, `move`, `log`, `ship-result`, `status` |
| `ui/` | The board and the card panel: plain JavaScript with vendored xterm.js, marked and DOMPurify |
| `tools/` | `cdp.mjs`, which drives the app's WebView over its DevTools port for testing |
| `fixtures/` | Test manifests. Card contents stay outside the repo, in `~\Documents\kanban-backups\`. |
| `scripts/` | `install.ps1` |
| `ui-deps/` | npm manifest and `vendor.mjs`, which copies the browser libraries into `ui/vendor/` |
| `prompts/` | Example column prompts, used when no private folder has its own |

## Private folder

Your own column prompts and repo list live outside this repo, in a folder you choose: Settings ›
Sessions › **Private folder** (setting `private.dir`, a full path such as `~\kanban-private`).
Empty uses the bundled examples.

- **`prompts\`** in that folder replaces the bundled `prompts\` examples as a whole, when it
  exists. Column prompts and templates name the folder as `{prompts}`, and the app puts in the
  right one when it sends them.
- **`repos.json`** lists the repos a card can be for: the Add card form's choices, and the folder
  each repo's card sessions start in (`~` is your home folder; a repo not listed starts there):

  ```json
  [{ "repo": "owner/name", "folder": "~\\name", "label": "name" }]
  ```

  `label` is the short name on the board (by default the part after the `/`). Without a
  `repos.json` the list is empty.

To start, copy the bundled `prompts\` into your private folder and tailor them to your repos.

## Stacked cards

A stack is one lead card that manages a set of child cards: work that goes out as one branch, one
PR and one dev stack, which Hugh tests together while each child keeps its own how-to-test and state
(design: `design/stacked-cards/SPEC.md`). The lead lists its children (`stack`), each child names its
lead (`stackOf`), and Hugh's Passed on a child sets `passed`. A card is in at most one stack.

- The lead's session owns the children's cards. It moves each child to Human test when it is
  ready; the app moves the lead to Human test once the children are ready, and back to In progress
  when one goes back. A session can't move a lead to Human test itself.
- When the lead reaches Deploy complete, its children follow. Archiving the lead (or its reaching
  Done, which archives it) archives its children; restoring it restores them. Only Hugh moves the
  lead into Ship it (or the app for him, when he set the lead to go on by itself to Deploy
  complete); a child never goes there on its own.
- Hugh doesn't move a child between columns: its row shows its state, and a click opens the child
  in an overlay over the board (wide enough to read its how-to-test), with the stack's sign-in
  links, Passed, Found a problem, answers, Open card and Detach. Esc, the × or a click outside
  closes it; Passed, Send and an answer close it too. It redraws from the latest card while open. A screenshot pasted into the problem or answer box is saved under the child, and its
  path goes to the stack's session with the note as `[Image: <path>]`. **Detach** (or `kanban_stack_detach`, `kb stack <lead> detach <n>`) tells the stack's
  session to stop building it and log what it did; the child goes to Planning as its own card and
  its own session starts.
- **Drop a card on a card or a stack** (its middle; the edges still reorder) and confirm, or
  `kanban_stack_join` / `kb stack <lead> join <n>`: it joins the stack (the card becomes a stack if
  it wasn't one). The stack's session is told to plan and build it, or, if it's built already
  (Human test or later, or it has a PR), to merge its branch into the stack's and close its PR.
  A stack is one repo: a card for another repo can't join.
- **Drop a card on a card in Ship it to ship them as one** (card #89). The card has to be built:
  in Human test, or in Ship it itself. If the Ship it card's ship hasn't started (it is waiting its
  turn, or its session hasn't been handed it yet), they stack under it. The card waits in the stack
  in Human test, marked Passed, and its own ship request (if it had one) is cancelled. The stack keeps
  its place in the ship queue, taking the earlier place when both were waiting. Its session merges
  the card's branch in before its merge, so it all goes out in one merge, one push and one deploy.
  If that ship has started (its session has it, or it is shipping or shipped), the card can't join
  the push. The dialog offers to ship it straight after instead, on its own, through the ship queue.
  Both cards need the same repo for that. A ship waiting for Hugh takes nothing, in or after it.
- **A card in Ship it can leave it for a stack:** drop it on any card before its own ship has
  started. Its ship is cancelled and it ships with that stack when Hugh moves the stack to Ship it.
- **Drop a whole stack on a card** once it is built (its lead in Human test, or in Ship it before
  its ship started). Its lead and then its cards join the target's stack, in order, and its own
  session stands down. A stack still being built can't be dragged in.
- The board says why when it refuses: a card whose own ship has started, a card not built yet (or
  shipped already) dropped on a card in Ship it, or a card for another repo. The app decides what a
  drop does (`stack_join_preview`), and the dialog says it. These Ship it joins are Hugh's drop
  only. A session's `kanban_stack_add` or `kanban_stack_join` can't add to a stack in Ship it,
  take a card out of Ship it, or put a stack into a stack.
- The setting `stacks.human_test_when` decides when the stack moves: `all` (the default, every
  child in Human test or later) or `first` (as soon as one is).

From Bash: `kb create --title "..." --stack <lead>`, `kb stack [<lead>]` (list),
`kb stack <lead> add <n>`, `kb stack <lead> remove <n>`, `kb passed <n> [--off]`. The tools:
`kanban_stack_list`, `kanban_stack_add`, `kanban_stack_remove`, `kanban_stack_join`, `kanban_stack_detach`, `kanban_set_passed`, and `stack`
on `kanban_create_card`.

## Controlling the app from any Claude session

`kb mcp` is an MCP server (stdio) for Claude Code. It forwards to the running app, so it keeps
working across app restarts. Outside a card session it offers every card tool plus session
control: `kanban_session_start` (with a first message; queued over the cap), `_send`, `_stop`,
`_status`, `_read`, `_switch` (model/effort) and `_keys` (answer a prompt a session is stuck
on). Inside a card session (`KANBAN_SESSION` set) it serves only that session's own card tools,
so one card's session can never drive another.

- This repo: `.mcp.json` registers it as `kanban-app`.
- Every project: run once in a terminal (it writes your own Claude Code config):
  `claude mcp add --scope user kanban-app -- kb mcp`

The same controls from Bash: `kb start <n> --message "..."`, `kb send <n> "..."`, `kb stop <n>`,
`kb sessions [<n>]`, `kb read <n> --last 5`, `kb switch <n> --model claude-sonnet-5-5`,
`kb keys <n> down enter`.

Idle sessions pause after `sessions.idle_pause_minutes` (Settings › Sessions, default 30; 0
never), keeping their conversation, and resume when they are sent a message.

## After a quit or restart: resume what was running

Quitting (tray › Quit, Restart now, an install) suspends every session and records whether each
was mid-turn or idle; a crash records the same at the next start. At the next start:

- **Mid-turn sessions** resume by themselves and are told to carry on.
- **Idle sessions** are offered in a bar along the top of the board: "N sessions were running
  when the app closed: #12 …, #34 …", with **Resume all** and **Not now**. A card's name opens
  the card. Resume all resumes each conversation (over the session cap the rest queue). Not now
  leaves them stopped; each still resumes from its card.

The bar stays across restarts until answered, and a later quit adds its own sessions. A card
resumed by hand, moved to Done or archived drops off it. Sessions you stopped yourself, Done
stopped, or the idle pause stopped are not offered.

## The card panel's "Working…" line

While a card's session is on a turn, a line under its conversation shows what the terminal's
spinner shows: `✻ Mustering… (5m 55s · thought for 4s)`. The time is how long the turn has run,
from its first `UserPromptSubmit` hook (`turnMs` in `session_info`), and counts up every second.
"Thought for" is the turn's latest thinking block, timed from the transcript line before it to the
thinking line itself (Claude Code leaves the thinking text out of the transcript, so the time is
all there is). While Claude is asking something it reads "Waiting for you: answer the question
above" (a question you can answer in the chat) or "Waiting for you: answer in the terminal" (a
permission prompt). The line goes when the turn ends: the `Stop` hook, or, for a turn interrupted
in the terminal (Esc, or a permission prompt or question turned down), which sends no hook, the
transcript's `turn_duration` line, which the app checks on each statusLine report (card #78).
The word is picked at random each turn, as the terminal's is.

## Claude's questions in the chat

When a card's session asks you something with `AskUserQuestion`, the question shows in the chat as
its own card, not folded into "Ran 1 tool": each question's header, its text, and its options with
their descriptions ("pick any" on a multi-select one). While that dialog is on screen you answer
it there:

- one single-select question: click an answer (or type your own and press Enter);
- several questions, or a multi-select one: pick for each (tick any number on a multi-select;
  type your own on a single-select one), then **Submit answers**.

The app types the dialog's keys into the session's terminal for you (`session_answer`), so the
terminal works as before and you can still answer there. It only answers the dialog on screen:
the `PreToolUse` hook gives the app the question's id and options, and an answer for any other
question, or once the session has moved on, is refused with "answer it in the terminal". Once
answered, here or in the terminal, the card shows your answers. A typed answer isn't offered on a
multi-select question; use the terminal for that.

## Screenshots in the chat

A card's chat draws local images (card #84):

- **Claude's screenshots.** A Markdown image of a local file in Claude's message,
  `![what it shows](C:/path/to/shot.png)`, shows the picture in place. Card sessions are told to do
  this where a screenshot proves the check (the app's MCP instructions, `prompts/in-progress.md`
  step 8, `prompts/kanban-app.md`). Backslashes and `file:///` links work too; network paths
  (`\\host\share`) are never opened.
- **Read screenshots.** A `Read` of an image shows it inside that tool's row, once you open it.
- **Your attachments.** A message with `[Attached: <path>]` or `[Image: <path>]` lines shows each
  image as a thumbnail.

Click any of them to see it full size; click again or press Esc to close it. Images are png, jpg,
gif, webp or bmp, at most 25 MB, read through the `image_read` command (the page's CSP allows only
`data:` images). A file that is gone or refused says why in place of the picture.

## Subagents in the chat

A card's chat shows the subagents its session ran (card #91): the `Agent` tool's subagents (a
`card-tester`, an Explore or general-purpose agent) and forked skills such as `/code-review`.

- **The strip under the search box** holds **Main** and one tab per subagent, newest first: its
  type, its description and how long it ran. A green pulsing dot is running, a blue one finished,
  a grey one stopped (its session ended, or was resumed, before it finished). The strip only appears once the
  session has run a subagent.
- **Click a tab** to read that subagent's own conversation: its brief, its tool calls and its
  report, drawn like the main chat, with search. A note at the top says which subagent it is;
  **Back to main** (or the Main tab) goes back. It is read-only: the message box still talks to the
  main session.
- **In the main chat**, an `Agent` call links to the subagent it started ("View card-tester" on its
  "Ran N tools" row, so it shows while the row is folded), and a subagent's finished notice reads as one
  line, "Agent "..." finished", with a View link, instead of its raw `<task-notification>`.

The app reads Claude Code's own files beside the transcript, `<session id>\subagents\agent-<id>.jsonl`
and `.meta.json` (`subagent_list`, and `transcript_read` with `agent`), every 3 s while the panel
is open.

## The card panel's message times

Every message in a card's conversation, and every "Ran N tools" group, shows when it was written,
from the transcript line's own timestamp: `16:43` for today, `3 Nov 16:43` earlier this year,
`1 Feb 2025 23:59` before that. Hovering the time shows the full date and time to the second. A
group shows its first tool's time. Search (Ctrl+F) skips the times.

## The card panel's message box

The box under a card's conversation is always there. **Send** (or Enter):

- **Session running:** the message is typed in at once, as if typed in the terminal: at its
  prompt, after a failed turn, or mid-turn (Claude Code queues it and reads it at its next step).
  It waits only while the session is starting or asking you something in the terminal (a
  permission prompt or an `AskUserQuestion` dialog, where Enter would pick an answer), or while a
  model/effort switch goes first. The footer says why it is waiting. A message sent mid-turn shows in the chat once Claude has read it (Claude Code records it as a `queued_command`, which the chat view now shows as yours).
- **No session:** Send resumes the card's conversation in the app, or starts a new session, and
  the message is typed once it is ready (over the session cap the start queues, and the message
  with it). A conversation that ran in the Claude desktop app isn't started for you: the message
  waits until you click Resume in the app. The row above the box still has Resume and Start a
  new session, to start without a message.

A message the app is holding shows in the chat at once as a queued bubble: "Queued · typed once the session is ready" (or why else it waits) before the session is up, then "Typing it into the session…" until Claude takes it. If Claude hasn't taken it 15 s after it was typed (the terminal can drop typing while it starts), the app types it again, up to 3 tries; after that the bubble says "Not sent" with **Send again** and **Dismiss**. A message the app was still holding when it quits is typed again after the resume.

A message Claude has queued but not yet read shows faded, with a dashed edge and "Sent · waiting for Claude to pick it up". As in the terminal, **Ctrl+X Ctrl+S** (or Ctrl+Enter) in the box, or **Send now** on the waiting message, interrupts the turn and sends it at once.

`kb send` and `kanban_session_send` behave the same. The app's own messages (column prompts,
stack notes) still wait until the session is idle and quiet.

## Building

While a card's session is working on a turn, the card shows a **Building** chip (beside its pulsing
dot), in place of the plan chip ("Session started" and the like). It goes when the turn ends.

While the session runs a review or a test, the chip reads **Testing** instead: a `/code-review` (or any
skill whose name says review, test or verify), or a subagent whose description says it tests, reviews or
verifies (but not one that builds, like "Build the testing chip"). A subagent running in the background keeps the card on Testing after the turn ends, until it
finishes. Sessions started before the update show Building only, until they are restarted.

## Browser checks run in the background

A card session's Chrome DevTools tools (`chrome-devtools`) drive a Chrome with no window, so a card checking its
work in a browser never takes Hugh's focus. The app gives each session its own copy of the account's
`chrome-devtools` server with `--headless --isolated` added (a fresh throwaway profile per session). Sessions started
before the update keep their windowed Chrome until they are restarted.

## Labels on cards

A card can carry up to 8 labels of your own, such as `BUG`, `UX` or `DOCS` (each up to 24
characters). They show at the front of the card's tags as small filled, square-cornered tags, each
in a colour taken from its name (the same on every card; bug-like labels are red), so they read
apart from the round outlined state chips such as Shipped or Ship failed.

- **Add or remove** in the card panel, on the Labels row under the tags: type a label and press
  Enter (or a comma), or pick one already used on the board from the list. A label you've used
  before in another case is kept in its first spelling, so `bug` becomes `BUG`. The ✕ on a label,
  or Backspace in the empty box, removes one. Changes save at once.
- **Filter:** click a label on a card to show only cards with that label (the search box reads
  `label:BUG`); click it again, or clear the box, to show all. Plain search also finds labels.
- **Sessions and `kb`:** `kanban_update_card` with `labels` (a list, or `null` to clear),
  `kb update <n> --labels BUG,UX`, `kb create ... --labels BUG`. The store trims them, drops
  empties and repeats (ignoring case), and refuses more than 8 or one over 24 characters.

## One ship at a time per repo

Move several cards into Ship it and they ship one at a time per repo: the first goes to its session
at once, and the others show a dashed **Waiting to ship** chip (hover it for the card they wait
for) and their sessions hear nothing yet. When the shipping card is done (shipped, failed, Needs
Hugh, moved out of Ship it or archived), the app hands the oldest waiting card of that repo to its
session within a couple of seconds, as if you had just moved it. Cards of different repos ship side
by side, and cards with no repo never wait. Moving a waiting card out of Ship it cancels its
request. A card whose session never started (say, its conversation is in the desktop app) stops
holding its repo after a minute.

## The Archive (no Done column)

The board has no Done column (card #83, 5 Nov 2026). Its last column is the **Archive**: a box at
the top, "Drop here to archive", then the archived cards, newest first (the newest 20; **Show N
older** shows the rest, and the filter box searches them too).

- **Drop a card in the box** to finish it: it moves to Done, which archives it and stops its
  session (a card mid-ship stays put). The card's **Archive** button still archives a card without
  moving it.
- **Any move into Done archives the card**, whoever makes it (Hugh, a session's `kanban_move_card`,
  `kb move`), and a stack's lead takes its children into the archive with it. The card keeps the
  column it came from in `archivedFrom`.
- **Restore** (in an archived card) brings a Done card back to the column it came from, or to
  Deploy complete if that is unknown or was Ship it; any other archived card goes back to its own
  column. A restore that changes the column counts as a move there: the card's session is told, or
  the column starts one. A card that is shipping can't be archived until the ship has finished.
- Migration 10 archived the cards that were in Done, with their last update as `archivedAt`.

## Take it to: cards that go on by themselves

The Add card form has a **Take it to** choice. It sets how far the card goes by itself before it
waits for you:

- **Backlog** (the default): it waits in Backlog until you move it, as before.
- **Planning:** the app moves it into Planning for you, so its session starts and plans it. Then it
  waits for you to move it on.
- **Human test:** as Planning, but when the plan is ready and nothing in it is yours to decide, the
  session moves the card to In progress itself and builds it. It stops in Human test for you, with
  the usual how-to-test.
- **Deploy complete:** as Human test, and then the app moves it from Human test into Ship it for
  you. That counts as your approval, so **it ships with no human test**. For a card on a product repo that
  merges and deploys its PR; for this app it stages the live update, and you still click Restart
  now. The ship waits its turn behind any other ship of its repo, as usual.

If the plan has a question that is yours, the session asks it and the card waits in Planning. The
form's button says what will happen ("Add and plan", "Add and build", "Add and ship"), and the form
always opens on Backlog.

- **The chip:** a card set to go on shows a dashed **→ Human test** or **→ Deploy** chip. It goes
  when the card gets there.
- **Change it later:** the card panel's **Goes on to** select (Nowhere, Human test or Deploy
  complete) saves at once, with no popup, and the card's session is told. It
  doesn't start a card in Backlog: move it to Planning when you want it started. On a card already
  in Human test, Deploy complete ships it straight away.
- **What turns it off:** any move you make yourself, except into Planning or In progress (starting
  it, or approving its plan), or Nowhere in the panel. The card's log says so.
- **Stacks:** a stack's child never goes on by itself. Its lead does, once the stack rules move it
  to Human test.

From a Claude session of your own: `kb create --title "..." --to Planning`, or `--to "Human test"`
(the `to` of `kanban_create_card`). A card's session can't use it, and Deploy complete can only be
set in the app. A card your own session creates straight into Planning or In progress
(`kb create --column Planning`, as the kanban-agent skill does) now starts its session as your move
would; before, it just sat there.

## A message to a card in Human test takes it back to work

When you write to a card in Human test from its chat (Send or Send now), the app moves it back to
In progress first, then sends your message with a note that tells the session why: something you
tested needs more work. The session fixes it, runs a fresh subagent test (a move into In progress
always needs one), writes a new hand-over and moves the card back to Human test itself. The log
reads "Hugh's message took it back to In progress".

Dragging a card back from Human test to In progress yourself now says the same in the message its
session gets, so it looks for what you found instead of starting the build over.

Not for a stack: its cards move by the stack's rules, and a child's problem goes through **Found a
problem**. Typing straight into the terminal doesn't move the card either; use the chat box.
A slash command doesn't move it either: it goes in as typed.

## Slash commands in the chat

Type `/` at the start of a card's chat box and a menu lists what the terminal's `/` menu does:
Claude Code's own commands, and the skills and custom commands the card's session can see (yours,
the project's and your enabled plugins'). Keep typing to filter it. Up and Down pick, Tab fills
the command in, Enter runs it (or fills it in when it needs more after it), Esc closes the menu.

- **`/btw <question>`** asks the session a quick side question without interrupting what it is
  doing, as in the terminal. The answer shows in the chat in a box marked `/btw`; it isn't part of
  the conversation, and the app forgets it when it restarts (Claude Code doesn't keep it either).
  The session has to be running, and not asking you something.
- **Commands that print something** (`/effort high`, `/model opus`, "Unknown command") show
  their output in the chat under the command.
- **Commands that open a screen of their own** (`/config`, `/cost`, `/mcp`, a bare `/model`, and
  others) open the terminal beside the chat so you can use it; Esc there closes the screen. Until
  the session's prompt is back, the app types nothing else into it: a message you send meanwhile
  waits, and says so.
- **`/exit`** isn't taken from the chat: use **Stop session**, which pauses it so it can be resumed.

## Subagent test before Human test

Every card is tested by an independent subagent before it reaches you in Human test (your rule,
5 Nov 2026).

- Each card session the app starts has a **card-tester** subagent. When the build is done, the
  session briefs it (the card number, the deliverables, how to try the change) and waits. It tests
  the real thing and fixes nothing: it can't edit files or move cards. It ends with PASS or FAIL.
- The app reads the verdict itself when the subagent finishes, not from the session's word, and
  records it on the card. A PASS from a tester that ran no tools doesn't count.
- A card session's move of a card into Human test (or Deploy complete) from any earlier column is
  refused until its latest test passed, and each move into In progress needs a fresh test. On a
  FAIL the session fixes what it found and tests again; after three failed rounds it moves the
  card to Blocked.
- It holds card sessions only. Your drag in the app isn't held, nor is your own sessions' `kb move`,
  nor a move to Done (no diff), Blocked or Planning.
- **On the card:** a **Tested**, **Test failed** or **Test unclear** chip in In progress and Human
  test (hover a failed one for what failed), and a **Subagent test** row in the panel's details:
  the state, when, and what failed.
- **Switch it off:** Settings › Sessions, untick "Test every card with a subagent before Human
  test" (on by default). Sessions still have the tester; they just aren't held to it.
