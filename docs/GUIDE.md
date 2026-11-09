# Claude Kanban DSH

A Rust desktop app (Tauri 2): a kanban board of its own. Each card owns a live Claude Code session in an embedded terminal, and the app itself dispatches moves to those sessions. Its cards live in `%USERPROFILE%\.claude-kanban-dsh\board.db`.

**Install or update:** `powershell -ExecutionPolicy Bypass -File scripts\install.ps1` (per-user, no admin; puts `kb` on your PATH).

- **The plan:** [PLAN.md](PLAN.md)
- **What each phase must pass:** [ACCEPTANCE.md](ACCEPTANCE.md)
- **How to undo each change:** [UNDO.md](UNDO.md)
- **What was installed to build it:** [INSTALLED.md](INSTALLED.md)

## Test version

A second copy of the app, "Claude Kanban DSH Test", runs beside the live one so a change can be tried without disturbing your work. It has a warning bar across the top of the board and says "(test)" in its window title, tray tooltip and toasts. It is fully separate: its own install (`%USERPROFILE%\Programs\Claude Kanban DSH Test`), board and data (`%USERPROFILE%\.claude-kanban-dsh-test`), port, quit event and single-instance lock. Its `kb.exe` talks only to the test app and is not on PATH, so `kb` stays the live one.

- **Install or update the test copy:** `powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Channel test`. The first install copies the live board into the test folder; later installs keep the test board.
- **Refresh the test board from the live one:** add `-FreshCopy`. The old test board is kept in `%USERPROFILE%\.claude-kanban-dsh-test\backups`.
- **More test copies side by side:** `-Channel test2`, `-Channel test3` (and so on) install "Claude Kanban DSH Test 2", "Test 3", each with its own install folder (`Programs\Claude Kanban DSH Test 2`), data (`.claude-kanban-dsh-test2`), quit event, lock and `kb.exe`, "(test 2)" in its titles and warning bar. `-FreshCopy` works on each. DevTools ports: live 9333, test 9334, test 2 9335, test 3 9336, up to test 10 on 9343. `-NamesOnly` prints a channel's names without building anything. The board hands these out, one per card (*Turns on the test app*, below).
- **Copy a board by hand:** `kb copy-board --to <path> [--from <path>] [--replace]` takes a consistent snapshot (SQLite `VACUUM INTO`, safe while the live app runs). `--from` defaults to the live board.
- **Swap a change into the live app:** once it works in the test copy, run `scripts\install.ps1` (no `-Channel`) from the same commit. The live app quits cleanly, its sessions are suspended and resume at the next start.

Sessions started in the test copy are real Claude Code sessions on real repos, so treat its cards with the same care as the live ones. Sessions that were running on the live board when it was copied are marked suspended in the copy and never resumed there.

## Turns on the test app

Each card gets a test app of its own (card #147), and the use of each test app goes in turns (card
#115). The **Test apps** chip in the board header ("Test apps: 2 in use") opens a row per test app
in use: its card, its turn, its line and any install, with **Release** (the turn) and **Free** (take
the test app from its card; its session is told). It also shows Hugh's Hold and the old DSH lock
folder.

- **A card's own test app**: a card in In progress or Human test asks with `kanban_test_app`
  (action `take`). Its first take gives it the lowest free test app (Test, Test 2, ... up to
  Settings › Sessions › **Test apps, at most**, default 10), kept on the card as `testApp` and
  shown on its tile ("Test 2 · testing") and in its details. The reply names the install command
  and the CDP port. The card keeps it while it is In progress or Human test (a stack's child: In
  progress only, since you test a stack on its lead's install), so you test each card's build in
  its own test app. Moving the card on or archiving it frees it. Only when every one is taken does
  a card wait ("Test app: 1st in line"), and it gets the next one freed.
- **A tester turn**: the card's session uses its own test app (its own checks and its
  card-tester) during its turn, or waits in line for it and gets a message when the turn is its
  own. A paused session is never resumed for it: the turn passes to the next card with a running
  session, and the paused card keeps its place ("Test app: turn waits for its session" on its
  tile). Release gives back the turn only: the test app stays the card's.
- **An install turn**: a session outside the app (Hugh's own, for example) takes one on one test
  app with `kb testapp take --install --slot <N> --owner <o> --label "wave 3"`. It goes ahead of
  every tester on that test app, waits only for the current turn to end, and ends when its install
  has started the test app again. A test app that holds a card's build refuses it: free it first
  (`kb testapp free --slot <N>`).
- A turn ends when it is released, when its card leaves In progress and Human test or is archived,
  or after `testapp.turn_minutes` (default 60) with nothing running (a working session or a running
  card-tester keeps it). Past 45 min with someone waiting, the chip turns amber.
- `scripts\install.ps1 -Channel <channel>` builds first, then waits before it restarts that test
  app: for another install, for a tester's turn on it, for Hugh's Hold, while Hugh has used it in
  the last 10 min, and while it holds another card's build. Both boards show **Test app restart
  pending (wave 3)** with **Hold** (30 min, at most 2 hours) and **Let it restart**. No toast;
  nothing takes focus.

## This app's own cards build in parallel

Several cards for this app's own repo build at once (card #145). Each one:

- **builds in its own git worktree** inside the checkout, `.claude\worktrees\card-<n>`, on the
  branch `card-<n>`, made from `origin/main`. List the repo under Settings › Sessions › **Build
  each card in its own worktree** and the app makes it when the card's first session starts
  (card #146); otherwise the card's Planning session makes it. The main checkout is never built in.
- **is tested in its own test app** (*Turns on the test app*, above), so several cards can wait
  for you in Human test side by side. The card's hand-over names its test app and port.
- **ships alone.** Cards of one repo ship one at a time (*One ship at a time per repo*), and each
  ship's merge lane first rebases the card's branch onto the latest `origin/main` and reruns the
  tests (*One merge lane per repo*), so each ships on top of the last. A stack still ships as one, on
  its lead (*Stacked cards*).

A ship can't remove the card's worktree, because the card's session runs in it and Windows won't
remove a folder in use. The card's log gives the two git commands that remove the folder and its
branch once the session has stopped. Each worktree has its own build folder of several GB, so
remove them.

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
  [{ "repo": "owner/name", "folder": "~\\name", "label": "name", "loginDirs": ["frontend"] }]
  ```

  `label` is the short name on the board (by default the part after the `/`). `loginDirs`
  (optional) lists the repo's top-level folders whose changes need a signed-in user to check. Without a
  `repos.json` the list is empty.

  You needn't edit the file by hand: Settings › Sessions › **Repos a card can be for**, just
  under the private folder, lists each repo with its short name and folder. Add a repo, change
  one or Remove one, then **Save**: the app writes `repos.json` (the old one is kept as
  `repos.json.bak`), keeps each repo's `loginDirs` as they were, and the Add card list has the
  change the next time it opens. Cards already on a removed repo keep it; their sessions then
  start in your home folder. If your private folder is a git repo, commit the change there.

To start, copy the bundled `prompts\` into your private folder and tailor them to your repos.

## Stacked cards

A stack is one lead card that manages a set of child cards: work that goes out as one branch, one
PR and one dev stack, which Hugh tests together while each child keeps its own how-to-test and state
(design: `design/stacked-cards/SPEC.md`). The lead lists its children (`stack`), each child names its
lead (`stackOf`), and Hugh's Passed on a child sets `passed`. A card is in at most one stack.

- **The children build in parallel** (card #113, 6 Nov 2026). Each child is an ordinary card
  session: it plans itself, goes through the critic, and builds in its own worktree on its own
  branch (`<stack branch>-<n>`, from the stack's branch), with no PR and no dev stack. Once its own
  review and test pass it writes its how-to-test and moves itself to Human test, and its session
  stops. Hugh's move of the lead into Planning or In progress approves every start and resume in
  the stack (while the lead is in Planning, In progress or Human test), and starts each child that
  waits on nothing; a child that waits on a sibling ("Blocked by") starts once that sibling is
  merged. The app never moves a child to In progress: the child does, once its critic has passed
  and the lead is In progress.
- **The lead is the integrator.** It builds nothing and runs on low effort (unless the card sets
  its own), and its session start goes to the front of the start queue. When a child reaches Human
  test, the app wakes the lead with a merge brief, and the lead's card-merger rebases the child
  onto the stack's branch in the lead's worktree and fast-forwards that branch. A PASS starts the
  children that waited on it and tells the ones still building that the base moved; a FAIL sends
  the child back to In progress to rebase and fix. A child that goes back after its merge has its
  merge cleared and is merged again. The lead installs the integrated build once and runs its own
  card-tester on it; its review counts as passed when every child's has.
- **Each child has a stage:** waiting, building, to-merge, merged, passed or shipped
  (`kanban_stack_list`'s `stage`, the `stack_stages` command). Passed is refused on a child that
  isn't merged. A child with no branch of its own (a stack built the old way, by its lead) counts
  as merged.
- The app moves the lead to Human test once the stack is ready, counting merged children only, and
  the lead's own test and review have passed; it moves it back to In progress when a child goes
  back. A session can't move a lead to Human test itself. In a stack built the old way, the lead's
  session owns the children's cards and moves each child to Human test when it is ready.
- When the lead reaches Deploy complete, its children follow. Archiving the lead (or its reaching
  Done, which archives it) archives its children; restoring it restores them. Only Hugh moves the
  lead into Ship it (or the app for him, when he set the lead to go on by itself to Deploy
  complete); a child never goes there on its own. Small-change auto-ship never moves a stack.
- Hugh doesn't move a child between columns: its row shows its state, and a click opens the child
  in an overlay over the board (wide enough to read its how-to-test), with the stack's sign-in
  links, Passed, Found a problem, answers, Open card and Detach. Esc, the × or a click outside
  closes it; Passed, Send and an answer close it too. A stack in Human test is reviewed in the
  same overlay with Previous and Next (see "Reviewing a stack in Human test"). It redraws from the latest card while open. Found a problem, an answer and a screenshot pasted into the problem or answer box (saved under the child, its
  path sent with the note as `[Image: <path>]`) go to the child's own session, and the lead gets a log line; in a stack built the old way they go to the stack's session. Passed is an FYI to the lead. **Detach** (or `kanban_stack_detach`, `kb stack <lead> detach <n>`) is refused for a merged child (send it back with Found a problem, or let it ship with the stack). An unmerged child keeps its session and column (from Human test it goes back to In progress), and its session is told to rebase onto `origin/main` keeping only its own "(card #n)" commits. A child with no session goes to Planning as its own card and its own session starts; in a stack built the old way, the stack's session is told to stop building it and log what it did.
- **Drop a card on a card or a stack** (its middle; the edges still reorder) and confirm, or
  `kanban_stack_join` / `kb stack <lead> join <n>`: it joins the stack (the card becomes a stack if
  it wasn't one). In a stack built in parallel the card keeps its session and is told its stack and
  base; a built card is merged by the lead. In a stack built the old way, the stack's session is
  told to plan and build it, or, if it's built already (Human test or later, or it has a PR), to
  merge its branch into the stack's and close its PR.
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

**The folder-trust question (card #143).** When a card's session starts in a folder Claude Code
hasn't trusted yet, it stops at "Do you trust the files in this folder?" before its prompt. The
chat shows that question under the conversation, naming the folder, with **Yes, I trust this
folder** and **No, exit**. A click presses the arrow keys to that option and Enter in the
session's terminal (never a number key); the card's log records your answer. It shows only while
the session is starting and the question is on its screen, and you can still answer in the
Terminal tab. No exits the session, as in the terminal.

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

## What a card is waiting on you for

A card waiting on you says what for, in one line under its title, and its panel repeats it in a
"Waiting on you" box (card #107):

- **Asks you:** a question dialog is open. The first question and its options, "and N more
  questions" for several. Answer it in the chat.
- **Permission:** a permission prompt. The tool and what it would run, for example
  "Bash: cargo test", read from the session's transcript; else the prompt's own message.
- **Failed:** "its turn failed" or "its session crashed", plus Claude Code's error when the hook
  gives one. The tooltip on the icon for a question now says "asking you a question".
- **Replied** or **Plan ready:** an unread reply gets a line only when it ends with a question (the
  question), or, in Planning, when it says the plan is ready (its last paragraph). Any other unread
  reply keeps only the dot, as before.
- From the card itself: **Blocked** (its reason), **Asks you** for a stack card's question with its
  choices (the stack shows its first such card, "and N more cards"), **Ship** when a ship needs you,
  and **To test** for a Human test card not yet passed.

**Waiting on you · N**, beside the filter box, shows only those cards (it adds `is:waiting` to the
box; click again to take it out). It combines with other terms, so `is:waiting label:DSH` works.
The icon, the pinning to the top and the tray count are as before.

A card that rose to the top because it needed you keeps its place once its need clears (card
#132): open it, read it or answer it, and it stays exactly where it is instead of dropping down the
column. A card that needs you later goes above it. It stays until it moves to another column, you
drag it somewhere else, or it needs you again. A drag still lands exactly where you drop it, also
among the cards at the top, and the other cards there keep their places.
The board remembers these places across a reload or a restart.

The app hides secrets before any of this reaches the board: a link keeps only its host and first
path segment ("https://host/m/…"), with no query and no user or password; tokens, `Authorization`
headers and `key=value`, `key: value` or JSON pairs with token, secret, key, password or auth in
the key become "[hidden]", as do long opaque strings; if anything still looks secret, the line says
"details hidden". The page applies the same rules to the card's own fields (a blocked reason, a
stack card's question and choices, a ship's detail), which never pass the server. A stack card
names its cards straight after the label ("Asks you (#169 and 1 more card): ..."), so the
two-line clip never hides how many wait. Each line is at most 300
characters. Nothing new is stored: after a restart the app reads each unread card's last reply
from its transcript once, in the background.

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

A column prompt or template (Settings) can name the prompts folder as `{prompts}`, as in
`follow {prompts}\planning.md`: the app fills in the private folder's `prompts` folder, or else
the bundled examples (see "Private folder"), with the system's own separator (card #114).

## Building

While a card's session is working on a turn, the card shows a **Building** chip (beside its pulsing
dot), in place of the plan chip ("Session started" and the like). It goes when the turn ends.

While the session runs a review or a test, the chip reads **Testing** instead: a `/code-review` (or any
skill whose name says review, test or verify), or a subagent whose description says it tests, reviews or
verifies (but not one that builds, like "Build the testing chip"). A subagent running in the background keeps the card on Testing after the turn ends, until it
finishes. Sessions started before the update show Building only, until they are restarted.

## macOS (untested, unsigned)

**macOS support is untested on real hardware and unsigned.** It builds and passes its checks on
GitHub's Mac machines (unit tests, an install, and a smoke test of the installed app), but nobody
has run it on a real Mac yet. The app has no Apple Developer ID signature and is not notarised,
so macOS will refuse to open it the first time. ACCEPTANCE.md lists what still needs a real Mac.

**Allowing the app the first time.** Open the app once. When macOS says it can't be opened (or
"can't check it for malicious software"), go to **System Settings > Privacy & Security**, scroll
down to **Security**, and click **Open Anyway** beside Claude Kanban DSH. Confirm with Touch ID or your
Mac login, then open the app again. Do the same for Claude Kanban DSH Test. From Terminal instead:
`xattr -dr com.apple.quarantine "$HOME/Applications/Claude Kanban DSH.app"`. An app built on the Mac
itself with `scripts/install.sh` usually opens without this step; a downloaded `.dmg` always needs it.

**Before you install.** Xcode Command Line Tools (`xcode-select --install`), rustup with the stable
toolchain, the Tauri CLI (`cargo install tauri-cli --version 2.12.4 --locked`), for a universal
build both Apple targets (`rustup target add aarch64-apple-darwin x86_64-apple-darwin`), and about
10 GB free. Clone your repos to the folders your
`repos.json` names (see "Private folder"). Put PATH lines in `~/.zprofile`, not only `~/.zshrc`: the app
reads PATH from your login shell (`$SHELL -l`), which doesn't read `~/.zshrc`.

**Install.** From `~/kanban-app`:

- `sh scripts/install.sh --channel test` builds and installs **Claude Kanban DSH Test** into
  `~/Applications` and starts it in the background. Its data is in `~/.claude-kanban-dsh-test`; the
  first install copies the live board if there is one.
- `sh scripts/install.sh --now` installs the live app (`~/Applications/Claude Kanban DSH.app`, data in
  `~/.claude-kanban-dsh`) and links `~/.local/bin/kb`. Afterwards `sh scripts/install.sh` (no flags)
  stages an update for the app's "Update ready" bar, as on Windows. Never `--now` from a card session.

**Signing (optional, once).** `sh scripts/mac-signing-cert.sh` makes a self-signed code-signing
certificate, "Claude Kanban DSH Local", valid for 10 years, in your login keychain (it asks for your Mac
login once, in Terminal). install.sh then signs with it, so folder permissions (Documents, Desktop
and the like) survive updates. It does **not** stop the Open Anyway step for a downloaded copy:
only a Developer ID with notarisation does that, and there isn't one. Without the certificate
install.sh signs ad hoc and says so; permissions then reset after each update. A `.dmg` from CI is
signed ad hoc, so installing it and then a local build resets permissions once. To remove the
certificate: `security delete-identity -c "Claude Kanban DSH Local"`.

**What's different on a Mac.**

- The menu-bar icon opens its menu on a click; the number beside it is the cards that need you.
  Clicking the Dock icon brings the window back.
- **Cmd+Q asks first** when card sessions are running: Quit, Hide window or Cancel. Quitting from
  the menu-bar icon, the Dock or a log-out quits at once, and suspends the sessions as on Windows.
- Shortcuts use Cmd (Cmd+F, Cmd+K), so Ctrl+F and Ctrl+K reach the terminal; Option is the
  terminal's Meta key.
- Notifications open their card when clicked.
- When the app ends, cleanly or in a crash, it ends everything the card sessions left running
  (a dev server started with `&` or `nohup`), as the Windows Job Object does. A process that
  detaches itself with `setsid` is not caught.
- The test app's Web Inspector is in Safari's Develop menu; `tools/cdp.mjs` is Windows-only.

**Windows-only in this version:** account 2 (a card set to it refuses to start on a Mac, saying
why), a product repo's dev stacks (the stack panel says so, and the prompts tell the session to stop), and
`tools/cdp.mjs` screenshots.

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
  The box splits on spaces and a card must match every term, so `label:DSH board` shows DSH
  cards that mention "board". A label with a space is quoted (`label:"Sprint 9"`); clicking a label
  adds its term to what is there, or takes it out again (card #107).
- **Sessions and `kb`:** `kanban_update_card` with `labels` (a list, or `null` to clear),
  `kb update <n> --labels BUG,UX`, `kb create ... --labels BUG`. The store trims them, drops
  empties and repeats (ignoring case), and refuses more than 8 or one over 24 characters.

## What changed on each card

Cards outside Backlog and Planning show what their work changed, read from git by the app
(card #108). The app only reads: it never fetches, commits or takes git's index lock.

- **On the board:** a tag such as "3 files +48 −6", and "1 handed over" when the session handed
  over a finished file. Hover for the totals. No tag on archived cards.
- **In the card's panel:** a "What changed" box with the totals, where they come from, the files
  (the first 15, then Show all), the commits, work not committed yet, and the handed-over files
  with Open and Show in folder.
- **Where the figures come from:** the card's checkout (`worktreePath`). A card on its own branch
  is measured against the newest commit it shares with main, master, origin/main or origin/master.
  A card on main (as this app's cards were before card #145), or whose branch was merged, is measured from the
  commits whose message names it, such as "(card #94)", "(cards #89, #94)" or a range
  "(cards #96-#112)"; renames across those commits are followed. A stack child with
  no checkout uses its lead's, counting only commits that name the child. The checkout's current
  branch is used only in the card's own linked worktree, never in a repo's main checkout.
- **Not committed yet:** only in the panel, and only in the card's own worktree on its own branch.
  The board tag counts committed work only.
- **Saved figures:** each time a card enters Human test, the app saves its committed figures on
  the card (`changesSaved`), so they stay after a ship deletes the branch or worktree. When git
  finds nothing, the saved figures show; with none, the box says "No figures kept".
- **Handed-over files:** a session lists them in `changesOutputs`, a list of
  `{"path": "C:\full\path", "label": "..."}` (up to 20; full local paths only; a label up to
  80 characters; `null` clears it), through `kanban_update_card` or
  `kb update <n> --changesOutputs '[...]'`. Open works only for md, txt, png, jpg and pdf files;
  everything else gets Show in folder, so nothing a session wrote can run from the board.
- **Limits:** WSL and network checkouts aren't read. Each git call stops after 10 s. Lists stop at
  300 files and 20 commits (the totals count everything); saved figures keep 100 files.

## Proof in Human test

A card's proof is the screenshots and reports that show its change works (card #125). The Human
test panel shows them as pictures and readable reports, never as file paths.

- **Where a session puts it:** `proof`, a list of `{"path": "C:\full\path", "label": "..."}` (the
  same rules as `changesOutputs`: up to 20, full local paths, a label up to 80 characters, `null`
  clears it), through `kanban_update_card` or `kb update <n> --proof '[...]'`. Old cards need
  nothing: pictures, md and txt reports and PDFs named in the hand-over steps count too. A bare
  file name after a full path on the same line takes that path's folder.
- **What shows:** in the review view, proof pictures join the shot strip after the agents' shots,
  and a Proof section under it holds the reports (each a fold) and other files. The Chat view's
  Human test box has the same section, pictures too. In the steps, each path is its file name: a
  chip that shows the picture or opens the report. Other paths (source files, folders) show their
  last part, the full path in the tooltip. The full-size viewer's caption is the file name.
- **What the app reads:** only files the card names, inside your home or temp folder, never in a
  dot folder (the app's data, `.claude`, `.ssh` and the like; a repo's `.claude\worktrees` card
  worktrees are fine) or AppData outside its Temp. A refused file says why ("Can't show r.md: in
  the app's data folder"); a gone one says "Not there any more".
- **Reports:** md or txt, read up to 1 MB (UTF-8 or UTF-16), tokens and keys shown as `[hidden]`.
  A report shows only plain Markdown (no forms, styles or pictures); only http and https links
  work. Open and Show in folder are on each, for you to click.

## One ship at a time per repo

Move several cards into Ship it and they ship one at a time per repo: the first goes to its session
at once, and the others show a dashed **Waiting to ship** chip (hover it for the card they wait
for) and their sessions hear nothing yet. When the shipping card is done (shipped, failed, Needs
Hugh, moved out of Ship it or archived), the app hands the oldest waiting card of that repo to its
session within a couple of seconds, as if you had just moved it. Cards of different repos ship side
by side, and cards with no repo never wait. Moving a waiting card out of Ship it cancels its
request. A card whose session never started (say, its conversation is in the desktop app) stops
holding its repo after a minute. Each card built on its own branch, so its repo's merge lane first
rebases that branch onto the latest `origin/main` (*One merge lane per repo*): the second card ships on
top of the first.

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

### The Done pile (card #131)

The Router can ask to archive a card whose work is live (merged, shipped, or in Deploy complete),
or one you asked it to clear. It never archives by itself: the card goes into the **Done pile**, a
column just before the Archive, and leaves its own column while it waits. Each card there shows
where it came from, the Router's reason and evidence, and any running session archiving would
stop (for a stack's lead, its children's too).

- **Archive all N** moves every card shown in the pile into Done, exactly as your Archive drop
  does: it archives them, stops their sessions (a lead's children's as well) and counts as done for
  any card waiting on them. A card that changed since the pile was drawn, or went into Ship it,
  stays with the reason on it. The filter box narrows what Archive all takes.
- **Not now** on a card puts it back in its column. The Router can't ask again for it until you or
  the card's own session change the card.
- An ask goes by itself when the card moves to another column, is archived or restored, or joins a
  stack. The Router never asks for a card in Ship it, or for a stack's child (it asks for the lead).
- The pile shows only while it holds a card. **Restore** brings a card back as for any archived card.

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

## Card flow: skip Planning, skip Human test, and the agents per card (card #162)

Each card's details have a **Card flow** box: **Skip Planning**, **Skip Human test** and the five
card agents, each with three choices. **Default (on)** or **Default (off)** follows Settings ›
Sessions › Card flow defaults, now and whenever you change it there. **On** or **Off** is that
card's own. Each change saves at once, on its own, and the card's session is told. Add card has
the same box, every option at Default.

- **Skip Planning:** a move into Planning from Backlog or Blocked goes straight to In progress:
  your drag, Add card's Take it to, a blocked card starting by itself, the Router's move. No plan
  request, no Planning session, no plan critique. The session is told to read the card, make its
  worktree if the app hasn't, write a short approach into the summary, and build. The log says
  "Skipped Planning: Hugh's choice (this card's own)" or "(the default in Settings)". Your own move
  into Planning from a later column still plans it.
- **Skip Human test:** once the card's session (or the stack rules) moves it into Human test,
  after its agent checks passed, the app moves it into Ship it for you, as Take it to: Deploy
  complete does. That is your approval to ship it with no human test. It waits its turn in the
  ship queue, and a stack waits until every card in it is ready. Your own drag into Human test
  never skips it, Take it to: Human test still stops it there, and small-change auto-ship doesn't
  start its 10-minute wait. Set it On on a card already waiting in Human test (its session put it
  there) and it goes on at once.
- **Stacks:** a stack's cards never skip either: their lead's flow plans and ships them.
- **On the card:** a small chip for each choice of its own that differs from the default: "No
  plan", "No human test", "Planning on", "Human test on", "Critic off", "Tester on", and so on.
- **Change a default:** Settings › Sessions › Card flow defaults. It reaches every card that
  follows it at once; a card whose session is in the column where it matters is told.
- **From your own Claude session:** `kb update <n> --skip-planning on|off|default`,
  `--skip-human-test on|off|default`, `--subagent critic=on|off|default` (once per agent).
  `kb create --title ... --skip-planning` (card #169) makes a card that skips Planning and starts
  it now, straight in In progress.
  `kb card <n>` shows a `flow` object: each option's value and whether it is the card's own or the
  default. A card's session and the Router can't set them.

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

## Stepping through Human test (card #102)

Open any card in the Human test column and a review bar shows under the panel's header:
Previous, "Human test · 2 of 5", Next, the card-tester's and reviewer's verdicts, Fail and Pass.
The top of the card's details shows a Human test box: the verdicts, the hand-over steps ("To
try"), and the evidence (#101) and what changed (#108) once those are built.

- **Previous and Next** (or the Left and Right arrows) switch the panel to the next Human test
  card without closing it. The order is the column's as it was when you opened the first one (the
  board's filter, needs-you cards first, then your order); cards that arrive go on the end. A
  stack is one stop: its overlay, where you step through its cards (card #116, below). Without
  #116 the stop is the lead's panel, whose Pass ships the stack once every card in it is passed
  (a card still being built holds it too).
- **Pass** (P, then Enter) moves the card to Ship it, as your drag does, after a 5-second Undo:
  a banner at the bottom left counts down, and U (or its Undo) puts it back. The next card opens
  at once. On a stack's lead it ships the stack, checked again when the 5 seconds end; on a
  stack's child it marks it Passed.
- **Fail** (F) turns the message box into a note box. Enter sends it: the card goes back to In
  progress, its log says "Failed in Human test: ...", its session gets your note, and the next
  card opens. If the session can't take the note, the status line says so; the note stays on the
  card's log. Esc leaves fail mode and keeps what you typed. On a stack's lead the stack goes back
  too; on a child it is "Found a problem".
- The keys never act while you type in a box, the terminal or the search box, or while a dialog,
  Settings or a full-size screenshot is open. Close the panel, or open a card outside Human test,
  to stop stepping.

**Review mode (card #120).** A Human test card opened in the queue takes the whole panel, as the
DSH mockup: the header (Previous, "#n title", "2 of 5 in Human test", Review and Chat, Next), the
tester's before and after shots side by side (the reproducer's under them; one shot takes the whole
width; "No shots" when there are none), one verdict table (Critic, Tester, Reviewer, Reproducer:
the word and the detail, rows only for checks that ran), the acceptance checks (✓ met, ✗ failed,
○ open; edit them in the Chat view), the hand-over steps folded under "To try", then the
check-size chip, the diff size and the buttons: **Pass**, **Fail with a note**, **Open the card**.
Every state of the bar stays (Enter confirms, the 5-second Undo, Passed and Undo passed, a held
Pass with its reason). **Chat** (or Open the card, which also puts the cursor in the message box,
or Terminal) goes back to the normal panel with the bar under its header; **Review** comes back.
Fail shows the message box alone under the review. The router's card #0 is never in the queue.

## Reviewing a stack in Human test (card #116)

A stack in Human test is one stop in the Human test queue. Click the stack card (or press Enter on
it), or step onto it with the arrows, and its overlay opens on its first card not yet passed.

- The bar under the card's title shows where you are ("Stack #210 · 2 of 3 · Human test 2 of 5"),
  the card-tester's and reviewer's verdicts, Fail and Pass. Below it: the Subagent test and review
  rows (and the merger's, once card #100 is in), the evidence (card #101), then the how-to-test.
- **Previous and Next** (Left and Right) step through the stack's cards in Human test, in stack
  order, then a last page for the whole stack. Past either end they open the card before or after
  the stack; Left from the card after the stack comes back in on the stack page.
- **Pass** (P, then Enter) marks the card Passed and shows the next one not yet passed. Any other
  key, Esc or a click cancels. A passed card shows "Passed" and an Undo passed button. Pass is off,
  with the reason, while a card is waiting to be merged or its merge failed (card #113).
- **Fail** (F) opens the note box. Enter sends it: the card goes back to In progress (only its own
  Passed is cleared), and the stack's session gets your note. If the stack leaves Human test with
  it, the overlay closes and the next card in the queue opens.
- **The stack page** lists the stack's cards and the lead's own checks and steps. Its Pass (P, then
  Enter) ships the stack, after the same 5-second Undo as any card; its Undo button cancels that
  ship. Fail sends the whole stack back; if its session can't take the note, the status line says so.
- Esc, the × or a click outside closes the overlay and stops stepping. U still undoes the newest
  Pass that is counting down, and its banner shows over the overlay.

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
- **Switch it off:** for one card, set card-tester Off in its Card flow (or on Add card); for
  every card that follows the default, untick it under Settings › Sessions › Card flow defaults
  (on by default). Sessions
  still have the tester; a card with it off just isn't held to it, and its session skips it.

## Subagent review before Human test

Every card's diff is read by an independent reviewer before it reaches you in Human test (stack A,
card #96, 6 Nov 2026), as well as tested.

- Each card session the app starts also has a **card-reviewer** subagent, at high effort. Once the
  build is installed, the session starts it and the tester together, and briefs it with the card
  number and where the change is. It reads the card's
  own commits (those with `(card #n)` in their message) and the work not yet committed. It looks
  for a deliverable that isn't built, mistakes, risky changes (data loss, a migration without a
  backup or test, a delete, a secret, anything touching live), work the card didn't ask for, and
  breaks of the repo's hard rules. It changes nothing. It ends with PASS or FAIL. A card that
  changed no repo can still pass: the session says what it delivered and where.
- The app records its verdict on the card, as it does the tester's. A card session's move into
  Human test (or Deploy complete) is refused until **both** the latest review and the latest test
  have passed, and the refusal says which is missing. Each move into In progress needs both again.
- **Together** (card #162, 9 Nov 2026): the reviewer and the tester run at the same time, on the
  installed build, and a FAIL from either means a fix and both again. The app holds the session
  to it: the move to Human test is refused while the test's pass started before the reviewer's
  latest FAIL (the fix wasn't tested), or the review's pass started before the tester's latest
  FAIL (the fix wasn't reviewed). The refusal says which to run again and the time of the FAIL. It
  applies only while both are on for the card, and to a stack's lead's own passes too. Each
  record keeps when its run started and when it last failed (`startedAt`, `failedAt`); the move
  into In progress clears them with the rest.
- **Three FAILs:** the reviewer's third FAIL since the card came into In progress sends the card
  to Blocked, with the findings as its blocked reason, and its session is told to stop. From then
  on the session can't move it back into In progress or on to Human test, even if a later review
  passes, and a stack doesn't go on for a capped lead: you move it on, and your move back into In
  progress starts the count again.
- **A stack's lead:** the stack goes to Human test only once the lead's own review and test have
  passed too. Until then the lead's log says, once, what is missing, and the stack goes on as soon
  as they pass. A child going back doesn't cost the lead its own passes.
- **On the card:** a **Reviewed**, **Review failed** or **Review unclear** chip beside the test
  chip, and a **Subagent review** row in the panel's details (the state, when, how many FAILs,
  and what failed).
- **Switch it off:** for one card, set card-reviewer Off in its Card flow (or on Add card); for every
  card that follows the default, under Settings › Sessions › Card flow defaults (on by default). It is separate
  from the tester's switch.
- **Guard rail:** the app's subagents may not use the card-changing board tools (under either of
  the two names a kanban-app session sees them by). In their shells the app refuses `kb`,
  `claude config` and `claude mcp` (card #100's guard, a hook on every shell call; a deny rule
  naming a command would take the whole shell away), and logs any refusal on the card. A shell can
  still run things in forms the guard doesn't read, so this is a guard rail and not a fence. The app also checks each account's
  `settings.json` and the settings parts of `.claude.json` before and after each of its subagents
  runs. If one changed, the card's log says so in a WARNING line, and you get a notification.

## Time to check: 30-second look or Needs real testing

Every card in Human test says how long it should take you to check (card #103, 6 Nov 2026).

- **The chip:** **30-second look** or **Needs real testing**. Hover over it for why: the headline
  reason, every other reason, and the diff in brief ("2 files, +31 −9, from branch dsh-12 against
  main"). A stack shows it on its branch line; it covers the whole stack's branch.
- **30-second looks · N:** a button beside the filter box shows only the quick cards (it puts
  `is:quick` in the filter box; click it again to clear it). `is:needs-testing` shows the others.
- **Time to check**, a box in the card's panel (after the Ship and Planning boxes): the verdict,
  every reason, where the diff came from, the files with their +/−, and **Work it out again**.
  A stack's child has none: its stack's covers it.
- **The rule is strict on purpose.** A 30-second look is a change you judge by reading it or by
  looking at it: only styles (CSS, SVG), images, docs and tests. At most 80 changed
  lines and 5 files of UI, and at most 150 lines of docs (tests don't count). It needs real testing
  if it changes any script or code (JavaScript, TypeScript and HTML included), prompts or agent
  instructions (`prompts/`, `.claude/`, skills, AGENTS.md, CLAUDE.md), sign-in, data or
  migrations, the build, scripts or dependencies; if checking it needs a sign-in (logins or a dev
  stack on the card, a "Route: login" hand-over, or a change to a product repo's
  app code that needs a signed-in user); if its diff is empty, can't be read, has uncommitted work
  or is on another branch than the card says; or if the router expected 30 min+.
- **Where the diff comes from:** a card built on main (as this app's cards were before card #145): its commits marked
  "(card #N)" (and its stack's children's). A card on its own branch: the branch's commits marked
  "(card #N)" since it left main (or origin/main, whichever it left more recently), so a branch
  made from another unmerged branch isn't charged with that branch's work; with none marked, the
  whole branch against main. Read with git from the card's worktree; never from a checkout inside
  WSL. If git fails or is slow, the app tries again a minute later, up to three times.
- **When:** each time the card comes into Human test, within a few seconds, and again for a stack
  whose cards change while it waits there. The card's log says what it found. Sessions can't set
  it.

  runs. If one changed, the card's log says so in a WARNING line, and you get a notification. The
  files are shared, so the change may have been another session's (or yours) at the same time.

## Plan critique before building

Every card's plan is checked by an independent critic before it is built (card #97, 6 Nov 2026).

- Each card session the app starts also has a **card-critic** subagent, at high effort. At the end
  of Planning, once the whole plan is in the card's summary, the session briefs it with the card
  number and where the code is. It reads the card, the code the plan names and the repo's
  AGENTS.md / CLAUDE.md. Since card #144 (9 Nov 2026) it fails a plan only for a real problem: a
  deliverable the card asks for that the plan dropped, a plan that would come out wrong in a way
  the build wouldn't catch, or a step that is unsafe or breaks a hard rule (live, data, secrets).
  Everything else (test steps and wording, the number of checks, small test-harness gaps, details
  the build will find, style) is a **note**: notes never fail a plan. A question that is yours to
  decide starts "HUGH:", and the session asks you. It changes nothing, and ends PASS or FAIL.
- **Notes go to the build.** A PASS can list notes. The app keeps them on the critique and in its
  log line ("Plan critique: PASS, 2 notes: ..."), and adds them to the end of the In progress
  message when the card comes into In progress from Planning or Backlog (your move, the app's
  small-card move, or a Take it to card's own move).
- **A question for you holds the card.** While the latest critique has a HUGH: line (after a PASS
  or a FAIL), no card session and not the Router can move the card into the build, and a small
  card doesn't go on by itself. Your own move takes it on.
- **A card routed small gets one critique.** After a pass, an edit to its plan needs no fresh
  critique, and it goes on by itself as before. After a FAIL, the session fixes the plan and
  doesn't run the critic again; the card waits in Planning for your move (never Blocked), and the
  critic's findings go to the build as its notes. Not for a stack's cards.
- The app records its verdict on the card (`critique`). A card session's own move into In
  progress (or straight on to Human test) is refused until the latest critique passed: a card set to go on by itself, a stack's
  lead moving a child, any session moving its own card. **Your moves are never held**, so the
  planning prompt is the main enforcement.
- **What makes it needed again:** every move into Planning, and an edit to the plan (the summary, or the
  "done when" checks)
  while the card is in Planning or Backlog, or in Blocked before its build started. Once a card has been in
  In progress, a stop in Blocked doesn't need a new critique. A card routed small keeps its one
  critique through a plan edit.
- **Three FAILs** since the card came into Planning send it to Blocked, with the findings as its
  blocked reason (not a card routed small: it has one critique). Fixing the plan doesn't reset that count; a move back into Planning does.
- **On the card:** a **Plan critiqued**, **Critique failed** or **Critique unclear** chip on
  Planning and Blocked cards, and a **Plan critique** row in the panel's details.
- **Switch it off:** for one card, set card-critic Off in its Card flow (or on Add card); for every
  card that follows the default, under Settings › Sessions › Card flow defaults (on by default). Then nothing is
  held, and a card coming into Planning tells its session to skip the step.

## The card-router sizes each card in Planning

When a card comes into Planning, its session first asks a quick **card-router** subagent, on low
effort, how big the card is (stack A, card #98, 6 Nov 2026).

- It reads the card and skims the code, changes nothing, and decides: **small** (a short plan),
  **normal** (a usual plan) or **split** (child cards in a stack, which the session then creates).
  It also suggests the session's model and effort, and how long your check should take
  (5 min, 15 min or 30 min+).
- **On the card:** a **Small**, **Normal** or **Split** chip on Planning and In progress cards (hover
  for the reason), a **Route** row in the panel's details, and a log line: "Routed: normal,
  claude-opus-5-5 high, check ~15 min: ...".
- **Model and effort:** the route's apply only while the card is in Planning or In progress. When
  a running session is on the wrong ones, the app restarts it at the end of its turn and resumes
  its conversation on the right ones, and back to the column's when the card moves on. It doesn't
  type `/model` or `/effort`, because Claude Code would save those as your default for every new
  session. It waits while you have the card open. The panel marks them "route". Your own choice in
  the panel always wins.
- **Small cards start by themselves:** once a card routed small has passed its plan critique, the
  app moves it to In progress at the end of the session's turn, and logs why. Not a stack's cards.
  To switch it off: Settings › Sessions, untick "Let a card routed small start building once its
  plan critique passes" (on by default).
- **Once per card:** a card keeps its route. **Route again** in the panel (Planning only) clears
  it and asks the session to run the router again. A stack's child is never routed: its lead plans
  it.
- **Sizing first, or not at all** (cards #159 and #161): with Settings › Sessions › "Size cards
  before their session starts" on (the default), the app sizes the card with one quick call just
  before its session starts, so the session starts on the route's model and effort. A card is never
  sized later inside its session. With the setting **off**, no card is sized: no card-router run,
  no route, no restart, and every session runs on your defaults for its column (the Route row says
  "Not sized: sizing is off", and Route again is gone). The same if the quick call fails: that
  card's session runs on your defaults. Routes already on cards stay.

## The stuck-session monitor

The app watches every card session for signs that it is stuck (stack A, card #99, 6 Nov 2026).
It is plain Rust, with no Claude call: it reads each session's transcript, so no new hooks.

- **When:** every 10 minutes (Settings › Sessions, "Look for stuck card sessions every"; 0 turns
  it off and clears every chip). The first check comes one interval after the app starts.
- **What it looks for**, most pressing first:
  - **Failed:** the session's turn failed (on the usage limit, when the hook says so) or the
    session stopped on its own.
  - **At a question or a permission prompt:** waiting at one for longer than the interval.
  - **Repeating:** working, and its last five tool calls since its latest prompt (status polls
    skipped: `gh run view` or `watch`, `gh pr checks` or `view`, `git status`, `git fetch`,
    `sleep`, `kb card`, `kb sessions`, the Monitor tool) are one call, whole input and whole
    result (a call's description, which Claude words anew each time, doesn't count). Not while its latest call is still running. It reads the last 256 KB of the
    transcript.
  - **Quiet:** working, and nothing has written its transcript or a subagent's for 20 minutes.
  - **Dead wait:** idle for longer than the interval on an In progress card that still waits on
    its subagent test or review, with a subagent it started since then that never reported, wasn't
    stopped and has written nothing for the interval (a background tester killed by a restart),
    and nothing running or waiting for it.
  - **Idle:** idle for longer than the interval on an In progress or Planning card, with no
    question, no subagent or test running, no message waiting for it and no blocked reason
    (added 6 Nov after the ping-pong test, where two cards sat idle for 30 minutes unflagged).
- **What it does:** a red **May be stuck** chip on the card (the reason as its title) and a
  **May be stuck** row in the panel's details (the kind, since when, what the app did), with log
  lines when it starts and clears. Then:
  - Repeating: **one nudge** typed into the session mid-turn, checked again just before, and never
    while a question is open. Still repeating at the next check, or the nudge couldn't be typed:
    you are told, once. A different loop is a new episode, with its own nudge.
  - Dead wait: **one message** queued for the session's idle prompt. Still there next time (or the
    same dead subagent again after a clear): you are told, once.
  - Idle: **one message** queued for the session's idle prompt: carry on, or write in its next
    step what it waits for. Still idle at the next check, or idle again later in the same column:
    you are told, once each time, with no second message. A move to another column gets a new
    message.
  - While the Router runs, "you are told" goes to the Router instead (card #119).
  - Quiet: you are told at once. Nothing is typed into a working session.
  - Failed, or at a question or prompt: the chip only. The board already pins those cards for you.
- **Never:** it doesn't answer questions, approve anything, or stop, restart or pause a session.
- A dead wait is caught only while the check interval is shorter than the idle pause (30 minutes
  by default): a paused session isn't checked.

## Time in each column

The card panel's details show how long the card has spent in each column (card #111), above the
Log, so you can see where cards wait.

- **Planning, In progress and Human test** each get a row: the card's total time there, "2 visits"
  when it went there more than once, and the board's **median** for that column. The column the
  card is in now is marked **now** and keeps counting (the section redraws every minute).
- **Other columns** (Backlog, Blocked, Ship it, Deploy complete) fold away under one line that
  lists the ones the card has time in. Click it to see their rows; the panel remembers whether you
  left it open.
- **The median** is over visits to that column that **finished in the last 30 days** (the card
  moved on, or was archived there), across the whole board, archived cards included and dry-run
  test cards left out. A median, not a mean, so one card left in Backlog for weeks doesn't swamp
  it. Hover it for how many visits it covers. Each **column header's tooltip** gives the same
  median ("Cards spend a median of 2h 10m here").
- **What counts:** wall-clock time while the card isn't archived. Archiving stops its clock and
  Restore starts it again, in the same visit if the column is the same. Done never counts.
  Durations read `<1m`, `45m`, `3h 20m`, `2d 4h`.
- **Older cards:** the times come from the card's revisions (every move records its new column),
  which go back to when the card was made on this board. A card made before this change doesn't
  record the column it started in, so that is read from its first move's log line ("moved it from
  Backlog to Planning"); if no line says, that first stretch isn't counted and the panel says how
  long it was. A card **imported** from the old Work Board artifact is counted from the import,
  not from when it was first made; the panel says so.

## The reproducer: proof of a reported bug and its fix

A bug reported on a card comes back to you with proof, not another round of testing by hand (card
#112, 6 Nov 2026).

- **A bug report** is your **Found a problem** on a stack's card, your **Fail** in Human test (when
  card #102's review bar lands), your **Report a bug** (the button beside Send, on an In progress
  card that isn't in a stack), or a card-tester FAIL that names a bug you could see (`- BUG:`). A
  plain message or your drag back isn't one, and nor is a reviewer's finding: the reviewer reads
  the diff, not the running app, so the session fixes it in its review-and-test loop.
- The app marks the card's bug as needing proof (**Bug to reproduce**). The card's session runs
  its **card-reproducer** subagent before it fixes anything. The reproducer reproduces the bug in
  the test app on dry-run cards, writes down the steps and takes a screenshot (**Bug reproduced**),
  and drafts a test that fails because of the bug. The session adds that test, fixes the bug, and
  puts the fix in the test app. Then the reproducer checks the test app runs the fix, repeats the
  same steps, runs the test, and takes another screenshot (**Fix proved**).
- **It holds the move:** the session can't move the card to Human test until the latest run says
  the fix is proved (or that the bug couldn't be reproduced). A run that doesn't count (no
  verdict, no tools run, a "fixed" with nothing to compare, or a test app not running the fix) is
  **Repro unclear** and holds too.
- **Not reproduced:** the session asks you one question on the card and moves it to Blocked. Your
  answer goes back to the session, which runs the reproducer again if you gave more detail.
- **Several reports** on one card join one round: the next run covers all of them. A proof already
  made isn't lost: it moves to **Earlier rounds** in the panel.
- **A stack's lead** with an open bug report holds the stack out of Human test too, until it is
  proved. **A stack's card** isn't held: it doesn't install the test app, so the lead's
  reproducer proves the bug on the stack's install, and the card's session is told so.
- **A stray run** (no bug report open, and the run proved nothing) is only logged, so it can't
  hold a card.
- **On the card:** the chip on In progress, Human test and Blocked cards, and a **Reproducer** box
  in the panel: the reports, the steps and screenshot before the fix and after it (click a
  screenshot for full size), and the test's name. The screenshots are copied into the data folder
  (`repro\<card>\`), so they outlast the temp folder. The hand-over in Human test leads with the
  same proof.
- **Switch it off:** for one card, set card-reproducer Off in its Card flow (or on Add card); for every
  card that follows the default, under Settings › Sessions › Card flow defaults (on by default). Bug reports
  on such a card are then only logged, and nothing is held.
- Like the other subagents, the reproducer can't edit files or use the card-changing board tools,
  `kb` or `claude config`. It keeps Bash, so its "changes no code" rests on its prompt.

## The card-merger (a stack's children)

Each card session the app starts has a **card-merger** subagent (card #100, 6 Nov 2026). It no
longer does a ship (card #162): every ship's merge goes through its repo's merge lane (*One merge
lane per repo*, next), and the card-merger refuses a ship's brief.

- **A stack's finished child** (card #113): a stack's lead runs it to bring one child that passed
  its test and review onto the stack's branch, in the lead's worktree, without touching the
  child's own branch or `main`. It rebases, resolves the conflicts by the repo's own merge rules
  (from its brief, or the repo's CLAUDE.md or AGENTS.md), and reruns the checks that don't need a
  dev stack. It ends with PASS or FAIL. It runs at high effort, whatever the session's.
- **It never pushes.** On a FAIL it puts the stack's branch back where it started.
- **On the card:** a **Merged**, **Merge failed** or **Merge unclear** chip, and a **Merger** row
  in the panel's details. A report on a card in Ship it is never recorded: that card's merge is
  its lane's.
- **It can't push:** the merger may edit files (it resolves conflicts), but the app refuses its
  `git push` (in any form it can see, `git -C ...` and `sh -c "..."` included), `gh pr merge` and
  `gh api`, through a hook on every shell call, and logs the refusal on the card. A shell can
  still push in ways the app doesn't see, so its report also gives origin's SHA before and after:
  if origin moved while it ran, the card says "Merge unclear" and you get a notification.

## One merge lane per repo

Every repo ships through its own **merge lane** (cards #160 and #162, 9 Nov 2026). It is one
Claude Code session the app keeps for that repo. It merges each of the repo's ships, one at a
time, and keeps its conversation between them, so it resolves a conflict with what it learnt
merging the earlier cards.

- **Nothing to set up.** Every card with a branch, on every repo, ships through its repo's lane.
  The app finds the repo's checkout when the card asks: the main checkout of the card's
  worktree, else the repo's checkout under "Build each card in its own worktree", else its
  folder in `repos.json`. The checkout's `origin` on GitHub names the lane, so a card filed under
  an old name of the repo shares its lane. If the app finds no checkout, the ship stops with
  **Needs Hugh**, saying where it looked. A card with no branch has nothing to merge.
- **Its model and effort** are in Settings › Sessions › Merge lanes (default Opus 5.5, effort
  high). A change applies the next time a lane starts.
- **When it runs:** your move into Ship it approves the ship. The card's session commits and asks
  the lane to merge, and the lane starts (or picks up its last conversation). It stops after 15
  minutes with nothing to do; the next ship resumes the same conversation. Each brief also lists
  the repo's last 5 merges, so the history survives a new conversation too.
- **Where it works:** its own worktree, `.claude\worktrees\merge-lane` inside the checkout,
  detached. It never touches the card's worktree or branch, the main checkout, or any branch: it
  makes commits and reports one SHA.
- **On the card:** **Waiting for the lane**, then **Merging**, then **Merged**, **Merge failed**
  or **Merge unclear** (hover for the repo and the detail). The lane logs each step on the card.
  **Merge cancelled** means the card left Ship it while its merge waited or ran.
- **After a PASS** the card's session checks its branch hasn't moved, resets it to the lane's
  commit, checks with `git range-diff` that its own commits are all there, and then pushes as
  before. The lane itself never pushes, and the app refuses it a push, `kb`, an install, a move of
  a branch and an edit outside its worktree, and logs each refusal on the card.
- **When it goes wrong:** a FAIL, or no report within 60 minutes, leaves the card in Ship it with
  Needs Hugh and the reason. A lane that stops during a merge (a crash, your Stop, an app restart)
  is resumed once; a second stop makes the merge unclear.
- **In Settings,** each lane that has run or has a merge waiting shows its state (Off,
  Starting, Idle, Merging #n, Down), its model and effort, and a Stop button while it runs. Its
  conversation is kept. A test app's lanes are its own, in their own worktrees.
- A stack still ships once, on its lead, through the repo's lane. Merging a child into its
  stack's branch stays with the lead's own card-merger.

## Before-and-after evidence on the card

The card-tester leaves screenshots of the change it tested on the card, so you can judge a small
change by looking (card #101, 6 Nov 2026).

- **Who takes them:** the card-tester, for `hughht5/kanban-app` cards only, from the test app it
  was briefed to test, on that app's port (test 9334, test N 9333+N; 9334 when the brief names
  none, never live's 9333), with `tools/cdp.mjs shot`. "Before" is the new build just before the tester's
  action; "after" is once the change shows. A change you only look at gets only an "after". It
  is not a comparison with the old build.
- **How they get on the card:** the tester names each shot in an `EVIDENCE:` line of its report.
  When it finishes, the app checks each file (a PNG, from `%TEMP%`, at most 5 MB, at most 6 a
  run) and copies it into its own data folder, `evidence\<run>\`. Then it records the run on the
  card with the tester's verdict. A file it refuses is named on the card's log, with why.
- **On the card:** an **Evidence** box at the top of the panel's details: the latest run's
  Before and After side by side, with captions, the verdict, the time and the build the test app
  was running. Older runs are folded under "Earlier runs". Click a shot to see it full size. If
  the card was tested again since and that test took no shots, the box says the shots are from
  an earlier run.
- **Which build:** `cdp.mjs shot` stamps the test app's build commit into each PNG, and the app
  reads it back, so a shot of the wrong build shows. Each card has its own test app (card #147),
  but a freed test app keeps its last card's build until the next card installs into it.
- **Kept:** every run stays on the card; a new test run adds to it and never replaces one. The
  field is `evidence`, which only the app writes (sessions and `kb update` can't).
- **Disk use:** up to 30 MB a run in the data folder's `evidence` folder, never pruned.
- **Privacy:** `cdp.mjs shot` hides the terminal, the chat and masked text inputs while it
  shoots, and the tester crops each shot to the changed element. `shot` refuses to run without
  `CDP_PORT`, so it never shoots the live app by accident.
- **The test app's debug port:** each test app now always opens its DevTools port (9334, or
  9333+N for test N), however it is started, so the tester can shoot it. It is in the test
  channel's own window config (`src-tauri\tauri.test.conf.json`, which `install.ps1` renumbers for
  each slot); the live app never has one.

## Small changes that go on by themselves

A small, fully checked change can go on to Ship it without waiting for you in Human test (card #104,
6 Nov 2026). It is off until you list a repo.

- **Turn it on:** Settings › Sessions, "Let small, fully checked cards go on to Ship it by
  themselves". Add a row per repo: its name as cards hold it (`owner/name`), the checkout its ship
  pushes (for this app, `~\claude-kanban-dsh`) and the branch (`main`). An empty list is
  off, the default. Every change to the list is kept below it (when, who, what) and you get a
  notification, so a change you didn't make is seen.
- **Which cards:** a card in a listed repo whose own session moves it into Human test. Never a
  stack (lead or child), an archived card, a card with Take it to set, or a card you opted out.
  A card goes on by itself at most once: if its ship fails and it comes back, it stops for you.
- **What counts as small and fully checked**, all of it checked by the app itself, on the
  listed checkout:
  - its card-reviewer (which read the whole diff: `SCOPE: full`) and its card-tester both passed,
    each having run tools, on the commit now at HEAD, with the folder exactly as it was then;
  - the checkout is on the listed branch, with nothing uncommitted and no untracked files, and
    every commit ahead of `origin/<branch>` names the card, with no merges;
  - at most 30 changed lines, 3,000 changed bytes and 5 files, all of them edits (no new,
    deleted, renamed or binary files);
  - docs (`.md`, `.rst`) or CSS only. CSS may not set `display`, `visibility`,
    `pointer-events`, `position`, `z-index`, `opacity` or `content`, and needs the tester's
    before-and-after screenshots from the same passing run (card #101's evidence);
  - no path about sign-in, sessions, tokens, data, the store, schemas, SQL, config, settings,
    deploys, builds, prompts or agents (`agents/`, `skills/`, `hooks/`, `.claude/`, `AGENTS.md`,
    `CLAUDE.md` and so on), and no added line with SQL that deletes or drops, a token, a secret or
    a sign-in link. The path words are blunt: a file called `keyboard.md` or `design.md` stops too.
- **The wait:** a card that passes shows **Goes on in 10 min** with a **Stop** button, and the
  panel's **Small change** row says "This card would go on by itself" with **Don't let it go on by
  itself**. You get a notification. After 10 minutes the app checks it all again and moves it
  into Ship it for you, logged as the app's move with the files, lines, commits and both verdicts.
  It then shows **Went on by itself**.
- **The ship:** the Ship it session is told to push exactly the checked commit
  (`git push origin <sha>:<branch>`). When the ship is released the app checks the checkout is
  still at that commit, clean, on its branch, and moves the card back to Human test if not. When
  the session reports the ship, the app checks `origin/<branch>` is that commit, and warns you if
  it isn't. For this app, you still click **Restart now** for the update.
- **Otherwise** the card stops in Human test for you as before, and its log says the first
  reason ("Stops in Human test for Hugh: ...").

## Step progress on each card

A card whose session keeps a to-do list shows where it has got to, under its summary: "Step 4 of
7: Writing the tests" and a thin bar (green when every step is done). Hover it for every step,
marked done (✓), in progress (▶) or to do (○). `kanban_get_card` and `kb card` return the same
as `steps` (`done`, `total`, `current`, `line`). Card #106.

- **The to-do tools.** Claude Code no longer gives newer models (Opus 4.8, Sonnet 5, Fable 5 and
  later) its to-do tools. Settings › Sessions › "Card sessions keep a to-do list"
  (`sessions.todo_tools`, default on; Hugh, 6 Nov 2026) starts card sessions with
  `CLAUDE_CODE_ENABLE_TODO_TOOLS=1`, which brings back TaskCreate and TaskUpdate (deferred tools
  the session loads with ToolSearch). It applies to sessions started after the change. The In progress prompt asks the session to keep
  its build's steps there.
- **Where it is read.** Claude Code keeps the list in
  `<config folder>\tasks\<session id>\<n>.json`, named by the conversation's current id (its
  statusLine reports it; it changes on `/clear`) (`~\.claude` for the main account,
  `~\.claude-max2` for account 2). The app reads that folder; when it doesn't exist,
  or holds no task (Claude Code deletes the files once every task is completed), it replays
  the session's TaskCreate, TaskUpdate and TodoWrite calls from its transcript. Each
  statusLine report queues a read on a worker thread, and every minute the worker reads the cards
  in Planning, In progress and Human test. Nothing is stored in the board.
- **This visit only.** In In progress (and every column after it) a card counts the tasks still
  open and those finished since its last move into In progress; in Planning, since its last move
  into Planning. A new session starts a new list.
- **Hidden text.** A step that looks like a token, a key, a sign-in link or a Bearer header shows
  as "(hidden)".
- **In the chat too** (card #158, 9 Nov 2026). The card's chat shows the same list just above
  the message box, as the session's terminal does: "6 tasks (4 done, 1 in progress, 1 open)",
  then each task, ✓ done (struck through), ■ in progress (bold) or □ open. It changes as the
  session works and is hidden while the card has no steps. Click its header to fold it to that
  one line (with the task in progress beside it); the fold is remembered for every card. A list
  longer than 8 hides its earliest done tasks behind "… +N completed": click it to see them all.
  TaskCreate, TaskUpdate and TaskGet calls in the chat read as the task ("Write the tests",
  "#3 → completed") instead of JSON.
- **Limits.** A card whose conversation is in the Claude desktop app shows no steps (its
  `update_plan` list isn't read). If a later Claude Code drops `CLAUDE_CODE_ENABLE_TODO_TOOLS`,
  cards show nothing; the app then logs once on the card, "This session started with the to-do
  tools on, but its Claude Code offers no TaskCreate".

## Done when, and acceptance checks

A card can say what "done" means (card #105, 6 Nov 2026).

- **Planning writes them.** With the plan, the session writes one plain line, **Done when** ...,
  and 3 to 7 **acceptance checks**: each one thing a tester can see pass or fail.
- **On the card:** one line under the summary: a dot for its status, "Done when ...", how many
  checks are met (2/5), and "1 failed" in red if any failed. Cards without them show nothing new.
- **The status** is worked out, never stored: **blocked** in Blocked; **done** when every check is
  met and the card is in Human test or later; **waiting on Hugh** in Human test; **working** while
  its session works; otherwise **paused**.
- **In the panel:** a Done when block. Edit the line; tick a check to mark it met (untick to open
  it again); add or remove checks. Each check says who marked it and when ("met by Hugh (board),
  2 min ago", or by the card-tester).
- **The card-tester checks them.** When a card has checks, the tester ends its report with a
  `CHECK n: PASS|FAIL` line for each, and the app marks them on the card it tested. A PASS that
  fails a check counts as a FAIL, so the card waits.
- **Your ticks reach the session**, except in Human test, Ship it and Deploy complete, where a
  tick is only recorded on the card and doesn't wake the session.
- The board refuses a check holding the word "password" or a `ct=` link, as it does for any field,
  and also sign-in links and Bearer headers; the error names the check.

## The Agents feed (card #120)

**Agents** in the header (or Ctrl+Shift+A, also over a card's panel) opens a pane on the right
with what the router, critic, tester, reviewer, reproducer, merger and monitor did, newest first:
the time (the date on an earlier day; hover for the full time), the agent's name in its colour,
and what it did, each "#n" a link that opens that card. A verdict names its check ("TESTER
passed"). **Show older** reads the next page. The feed reads the app's `agents_feed` (the log's
author, not guesses from the text) when it opens and when the board changes, at most once every
2 s. The pane is not a dialog: Human test keys (P, F, U, the arrows) go on working with it open.
Esc (with the focus in the pane) or ✕ closes it.

## Open any running session (card #129)

The Agents pane starts with **Running now**: the Router, each merge lane, each card session the
app runs (with the subagents running in it now), and each Claude Code session outside the app
that has used this board in the last 15 minutes. The count on the **Agents** button is the same
list, kept current with the pane closed. Click any of them and it opens in the Router's panel,
the way the Router opens: its live log beside its chat.

- **A card session:** its chat takes messages and images, its questions are answered there, and
  it has a Terminal tab. If the card's own panel already shows its terminal, the Terminal tab
  says so instead, so the two never fight over its size. **Open card** opens the card's panel.
- **A subagent** (critic, tester, reviewer, reproducer, card-router): its own conversation, read
  only. Its card's session takes the messages: **Open card session**.
- **A merge lane** (the merger, one per repo): its conversation, read only.
- **Outside the app:** read only, with its folder and the cards it named. Type to it where it
  runs. A session counts as outside only if its `kb` (or `kb mcp`) called this app with its
  Claude session id, so a test app's sessions never show on live, and the app's own sessions
  never show as outside. An older `kb.exe` sends no id, so its session isn't listed. A session counts once it calls a tool, not when it merely starts with `kb mcp` configured. After a `/clear`, a session's `kb mcp` still names the conversation it started in.

In the feed, an agent's name on a row is a button too: a verdict opens the subagent that gave it
(by the id the verdict records, in any of the card's conversations or its stack lead's), a
critic, tester, reviewer or reproducer line opens the one of that kind that started last before
it, and a Router line opens the Router. Opening another session swaps what the panel shows; the
Agents pane stays open beside it.

## Search every card's session history

"Which card touched the invoice code?" **Search sessions** in the header (or Ctrl+Shift+F, also
over a card's panel) searches every card's Claude Code conversations from the last 90 days,
archived cards included: Hugh's prompts, Claude's replies, each tool call (its name and key input:
the command, path or pattern) and its subagents' conversations. Tool output and thinking aren't
indexed.

- Any run of 3 or more letters matches, ignoring case, so `voicereq` finds `InvoiceRequest`.
  Every word must match; `"quote"` a phrase. Shorter words are left out.
- Results come as you type, grouped by card (most matches first). A hit shows who wrote it, when,
  and the words round the match. Clicking one opens the card with the words in its conversation
  search (a subagent's hit opens that subagent's view). A hit from an earlier session is marked:
  the panel shows only the card's latest conversation.
- Tokens, keys, passwords and sign-in links are masked before anything is stored, so a search
  shows `[hidden]` or `[sign-in link hidden]` instead.
- `kb search <words> [--card n] [--limit n]` does the same from a session outside the app (the
  `kanban_search` tool). Card sessions don't get it: a card session sees only its own card.
- Card #120: each card shows as a small box, "#n title" and "N hits"; each hit is a line of text
  with the words marked. Under the results, **Median time in each column**: one bar and its
  legend, from the board's medians (finished visits of the last 30 days; Backlog, Deploy complete
  and the Archive left out).

The index is its own file, `search.db`, beside the board (never inside `board.db`). The app builds
it in the background 20 s after it starts, then adds to it whenever a session's turn ends, with a
sweep every 5 minutes. Each month (or when the indexing code changes) it builds a new file and moves
the old one into `backups\search`, which keeps one copy.
## Cards that wait on others

A card can wait on other cards ("Blocked by #12"): use it when one card needs another's work
first. Add the waits, drag the card to Blocked, and it starts by itself once they are ready.

- **Set it** in the card panel, on the **Waits on** row: type a card number (`12`, `#12`, or
  `12, 40`) and press Enter. The ✕ on a chip takes that wait off. Changes save at once. The board
  refuses a wait on the card itself, on a card that doesn't exist or is archived, on a card that is
  ready already (outside a stack), between a stack's lead and its own child, more than 20 waits,
  and any wait that would make a circle (the message shows it, such as `#40 → #12 → #40`).
- **On the card:** a "Waits on #12, #40" chip. Its tooltip says where each card is; a ready one, an
  archived one and one in a circle look different. The panel also lists the cards that wait on
  this one, and says what will happen when the waits are met.
- **When a wait is met:** a card in the same repo, once it is in Human test or later (built and
  tested, though not on main until it ships). A card in another repo, only at Deploy complete. A
  card archived from Deploy complete or Done counts as met.
- **When all of a card's waits are met** (the app looks every 2 seconds):
  - **In Blocked, put there by your own drag, with no blocked reason:** the app moves it to
    Planning for you, and its session starts as if you had moved it. The session is told which
    cards were ready and, for any not shipped yet, their branch: their work is not on main yet.
  - **In Backlog:** it stays there, and you get a notification ("Drag it to Planning to start
    it"). The waits never start a card from Backlog: you do, or the Router when you let it.
  - **In Blocked with a blocked reason, or put there by a session:** it stays, and you are
    notified.
  - **In Planning or later:** it stays where it is, and its session is told.

  The waits then come off the card (logged, as a new revision of the card).
- **A card it waited on goes back** (to In progress, say): once the card has started, its log says
  so and its session is told, once. If you haven't started it yet, it waits on that card again,
  and you are told.
- **A card it waits on is archived without shipping:** that wait comes off and you are told, so the
  card is never stuck waiting for good.
- **Stacks:** cards in one stack may wait on each other whatever their column. The stack's own
  rules decide when a stack's card starts, not its waits. A stack's lead that starts by itself
  this way starts alone: its cards start when you move the lead on yourself.
- **Test app:** only dry-run cards start by themselves there, so a copy of the live board can't
  start real sessions.
- **Sessions and `kb`:** `kanban_update_card` with `blockedBy` (a list of numbers, or `null` to
  clear), `kb update <n> --blockedBy 12,40`, `--blockedBy null`. A card's own session may set waits
  only on the children of the stack its card leads, and can't move a card out of Backlog or Blocked
  into Planning or later while its waits aren't met. Your own drag is never refused. `kanban_list_cards` shows `blockedBy`
  and `waitedOn`.

## The card face and the pill row (card #120)

Every card's face is laid out in one order, after the DSH mockup (ui/face.js runs last on each card,
through `boardHooks.final`, and only moves what the other files drew):

1. The title: the needs icon, the session dot, the number, the title.
2. A box when the card waits on you for a question, a choice, a permission prompt, a failed session or
   a plan to read: the kind word as its label, the text, the choices as plain text (answer in the chat).
   A stuck card has an amber border and box: "May be stuck · why, how long. Nudged".
3. Done when, then the step line and bar.
4. Human test only: the latest test's two shots, small.
5. One chip row: labels first, then Waits on, Router ("Router: small · Sonnet low"), Critic
   ("Critic: 2 findings"), test, review, reproducer, merger, the dashed check size, then the other
   chips (ship, plan, building, "Goes on by itself", small, queued, To test), and the time in this column.
6. The newest agent line ("Router: ...", "Critic: ..."), one line; when there is none, the summary.
7. What changed: "2 files +31 −9 · ui/panel.js · ui/board.css".
8. A muted row: repo, issue, PR, logins, dev stack, acct 2, last change.

A stack card keeps its own layout. The router's card (#0) never shows and is never counted.

Under the header, a row of pills filters the board: **Waiting on you**, **May be stuck** (`is:stuck`),
**30-second looks** and any `label:x` in the filter box. A click adds or takes out the pill's term in the
filter box, so pills combine (every term must match). A pill with nothing to show hides.
## The Router (card #119)

One long-running Claude Code session for the whole board. It routes new cards (size, model,
effort, split), checks a card that reaches Human test against its Done-when checks and its
verdicts, and takes the stuck-session Monitor's findings. It never writes code, never pushes and
never moves a card into Ship it. It doesn't merge this app's own cards onto an integration branch
any more (card #148): each is tested in its own test app and ships alone, and a stack's lead
integrates its own children.

- **Start it** with the **Router** button in the board's header ("Start Router" before the first
  start). Nothing changes on the board until you do. Its panel docks on the right: a digest when
  you have been away 2 hours or more, the Router's log, then **Chat** and **Terminal**. **Stop**
  turns it off; while it is on, the app resumes it after a restart.
- **Send it a screenshot.** Paste an image into its message box, or drop a file on the panel: a
  thumbnail shows above the box, and Send adds an `[Attached: <path>]` line the Router can Read,
  as in a card chat. The Router must be running to attach files.
- **Talk to it** in the panel, or from any shell with `kb router "what is stuck?"` and
  `kb router --replies 3`. Card sessions can reach it with the `kanban_router` tool. A question it
  asks you shows with answer buttons, a "Router asks" pill and a notification that never takes
  focus.
- **What it may do** without asking: move cards between Planning, In progress, Human test and
  Blocked (with a reason on the card), route, check and send cards back. **Settings › Router ›
  Router may start cards** (off by default) also lets it move a card from Backlog to Planning or
  In progress (its session starts, as with your drag) and start a card's session in its own
  worktree. So "clear every card for this repo" is something it does itself. It leaves a card alone while you have its panel open
  or changed it in the last 2 minutes.
- **Clearing finished cards:** it puts them in the board's **Done pile** (see *The Done pile*), and
  your **Archive all** archives them. It never archives a card itself.
- It runs on Sonnet at low effort (its subagents on Sonnet at medium); Settings › Router changes
  the model and effort. It sits outside the running-sessions cap and is never paused for being
  idle. It wakes only on events (moves, new cards, verdicts, the Monitor, your messages), joined
  after 30 seconds of quiet. Its context is only Claude Code's own compaction; it re-reads the
  board for every fact.
- The first start may show Claude Code's "trust this folder" question in the Terminal tab
  (its folder is `%USERPROFILE%\kanban-router`); answer it once.
