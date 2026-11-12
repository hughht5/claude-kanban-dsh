# Handover: Claude Kanban DSH app

For a fresh session carrying on this build. Updated 3 Nov 2026, after Phase 4 and the decision to run the app separately.

## Where things stand

- **Repo:** `~\claude-kanban-dsh`, branch `main`. Read `PLAN.md` (the plan, revised after the manager's review), `ACCEPTANCE.md` (each phase's tests and results), `UNDO.md` and `INSTALLED.md`.
- **Done:**
  - Phase 0: skeleton.
  - Phase 1: store, import, board on a copy.
  - Phase 2: card panel with chat, message box and terminal; Job Object; two-account test.
  - Phase 3: hooks, `kanban_*` MCP tools, `kb`, dispatcher, held move, command bar.
- **Phase 4** (first usable release) is installed per-user and running. **There is no Phase 5:** Hugh decided on 3 Nov 2026 that the app runs entirely separately from the Work Board artifact, as its own board. Don't freeze or sync the artifact, and don't change the `kanban` skill.
- **The app** is installed in `%USERPROFILE%\Programs\Claude Kanban DSH` (`scripts\install.ps1` rebuilds and reinstalls it). Its data (the board, backups, session files, `api.json`, `api.log`) is in `%USERPROFILE%\.claude-kanban-dsh`. **Not under AppData:** Windows redirects AppData writes made by processes started from the Claude desktop app (a packaged app) into `AppData\Local\Packages\Claude_…\LocalCache`, so an AppData board showed Hugh's own launches an empty board while launches from a Claude session saw the real one (3 Nov 2026). Only one copy runs at a time (single-instance plugin); a second launch brings the window forward. Column prompts tell card sessions to make every card write with the `kanban_*` tools, never to the artifact (migration 3).

## How to work on it

- **Build:**
  1. `export PATH="$HOME/.cargo/bin:$PATH"` (Git Bash).
  2. `cargo test --workspace`.
  3. `cargo build -p claude-kanban-dsh -p kb`.
  4. Stop the running app first (`Stop-Process -Name claude-kanban-dsh`), or the exe is locked.
- **Drive the UI from a script:**
  1. Start the app with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333 --remote-allow-origins=http://127.0.0.1:9333`.
  2. Run `node tools/cdp.mjs eval "<js>"` or `node tools/cdp.mjs shot <png> [dark]`.
  3. Relaunch without that variable afterwards.
- **Test cards:** use dry-run cards (`dryRun: true`, repo set to this repo's `owner/name`, `effortOverride: "low"`) and archive them after. The folder `kanban-app` is trusted on both accounts.
- **Requests the API received** are logged in the data folder's `api.log`, with the token hidden. Its port and token are in `api.json`.
- **Use subagents** for heavy pieces (Hugh, 3 Nov). Keep the main session for deciding, integrating, committing and reporting to the manager session after each phase, in five lines or fewer.

## No computer use and no browser for this app's cards (9 Nov 2026, card #172)

Hugh: "for testing i want no computer use or browser use". His answers: `tools/cdp.mjs` into
the card's own test app is fine, and so are its shots (the card's evidence). Not allowed:
computer use (desktop screenshots, clicks) and any browser (Claude in Chrome, the built-in
browser, chrome-devtools). Scope, from the Router: this repo's cards only; other repos stay as they
is until Hugh says.

- `src-tauri/src/nobrowser.rs`: `REPOS` (`hughht5/claude-kanban-dsh`, `hughht5/kanban-app`),
  `SERVERS` (`computer-use`, `remote-devices`, `claude-in-chrome`, `Claude_Browser`,
  `Claude_Preview`, `chrome-devtools`) and `deny_rules()` (`mcp__<server>`, whole servers).
- `session_cmds::start_session` sets `Plan.no_browser` from the card's repo. Then
  `sessions::settings_json` adds `permissions.deny` with those rules to the session's own
  settings, and `sessions::mcp_json` adds no headless chrome-devtools (card #77's override).
  The Router and the merge lanes are not cards and are unchanged.
- Why the settings and not each agent's `disallowedTools`: a live trial (Claude Code 2.1.246,
  a stand-in `chrome-devtools` stdio server) showed that a `--settings` deny of
  `mcp__chrome-devtools` takes the server's tools from the session and from a general-purpose
  subagent it starts, under `bypassPermissions`; the same run without the deny called the tool.
  So one rule covers the session, the app's subagents and any other subagent.
- The card-tester and card-reproducer prompts (`tester.rs`, `repro.rs`) say it too, and the
  tester's evidence step now names `hughht5/claude-kanban-dsh` as well as the old name.
  `prompts/kanban-app.md` (In progress step 6, public and private copies) and AGENTS.md (Hard
  rules) state the rule. Tests: `nobrowser.rs`, `sessions.rs`
  (`a_no_browser_card_denies_computer_use_and_every_browser`,
  `a_no_browser_card_gets_no_headless_chrome`), `tests/prompt_rules.rs`.
- It applies to sessions started (or resumed) after the update: a running session keeps its
  settings until it restarts.
- To widen it to another repo, add the repo to `REPOS`.

## Queued messages go in mid-turn (9 Nov 2026, card #154)

Hugh: "queued messagse take forever - they should come in sooner". A message that went into a
card's queue (the Router's second `kanban_session_send` while its first still waited, or one
sent just after the app typed into the session) waited for the whole turn to end, often many
minutes, though Claude Code takes typing mid-turn and reads it at its next step.

- `dispatch.rs` `MID_TURN_OK`: in memory, a card's waiting text that may go in mid-turn.
  `send_to_session` (Hugh's words, `kanban_session_send` from the Router or a session) records
  it when nothing else waits or what waits was recorded too (`mid_turn_rule`). The app's own
  text (column prompts, notices) joins the waiting message through `Store::queue`, which
  changes the text, so the record no longer matches and the whole message waits for the turn
  to end, as before.
- `type_mid_turn`, each dispatcher tick: types a recorded waiting message with
  `Sessions::type_now(.., true)` as soon as the session can take it, mid-turn included. A held
  move goes first; nothing goes into a session being restarted onto its route (#98).
- After an app restart the record is gone and a waiting message is typed at the prompt, as before.
- Idle but never quiet (second fix, same day): a turn that ended with a background subagent
  still running (a card-tester) keeps the status line moving, so `quiet` (1.5 s still screen)
  never held and Hugh's message to #150 sat queued until the subagent ended. `takes_typing`
  now lets a mid-turn-ok message in at an idle prompt once the session has been idle
  `SETTLED` (3 s), still screen or not. The app's own text still waits for `free`.
- Tests: `dispatch.rs` `a_message_goes_in_mid_turn_only_when_all_that_waits_may`,
  `the_mid_turn_record_is_per_card_and_forgotten`.

## Updates (4 Nov 2026)

- **Live default is `-Stage`:** `scripts\install.ps1` builds and copies the NSIS installer to `%USERPROFILE%\.claude-kanban-dsh\update\` (test: `.claude-kanban-dsh-test\update\`) with `update.json` (`version`, `commit`, `built_at`, `installer`, `install_dir`, `app_exe`, `channel`). It never quits the app. `-Now` is the old quit-install-restart; it is still the test channel's default, and `-Channel test -Stage` tries the staged flow on the test copy. A first install must use `-Now`.
- **Build identity:** `src-tauri/build.rs` compiles in `KANBAN_GIT_COMMIT` and `KANBAN_BUILT_AT` (install.ps1 sets both and writes the same into `update.json`; a plain `cargo build` takes HEAD and the build time).
- **The app** (`src-tauri/src/update.rs`) reads the folder at start and every 30 s. A staged `built_at` later than its own shows "Update ready" along the top of the board (`ui/update.js`, `ui/update.css`). Later hides that build until the next start or a newer one.
- **Restart now** starts a hidden, detached PowerShell helper (outside the app's job if Windows allows breakaway), then runs `Sessions::stop_all` and exits, as tray Quit does. The helper waits for the app's pid to exit (up to 5 min), runs the installer with `/S /D=<install_dir>`, then starts `app_exe` (not `--background`: Hugh clicked, so it opens in front) whatever the installer did. A test app is the exception (card #135): `install.ps1 -Stage` (and `install.sh --stage`) on a test channel writes `"background": true` into `update.json`, and the helper then starts the app with `--background`, so a tester's Restart now over CDP never takes Hugh's focus. `update::restart_in_background` honours the flag only in a test build: a live app comes back in front whatever its `update.json` says. On a Mac the flag is read but not used yet (`update_helper`'s `_background`). A start that isn't `--background` forces the window to the front with `place::bring_forward`, because Windows refuses `SetForegroundWindow` to a process the foreground one didn't start. Restart recovery resumes the sessions that were mid-turn. It logs each step to `update\restart.log`.

## Labels on cards (5 Nov 2026, card #71)

`labels` is an ordinary card field (no migration), but the store cleans it in `cards.rs`
`clean_labels` on every create and update: trimmed, inner spaces collapsed, repeats dropped
ignoring case, at most 8 of up to 24 characters, `[]` or `null` removes the field. A string is
split on commas so `kb update --labels BUG,UX` works. The UI is `ui/labels.js` (`labelTags`,
`labelMatch`, `labelEditor`, `labelWire`, called from `board.js`) and `ui/labels.css`; a label's
hue comes from a hash of its lowercased name.

## Ship queue (5 Nov 2026, card #79)

`crates/store/src/ships.rs`: a card's lane is its `repo` (trimmed, lowercased); cards with no repo
don't queue. A card in Ship it holds its lane while its ship is `shipping`, or `requested` and
handed (`handedAt`, written by `dispatch::mark_handed` and `ship_release`, or the older "Handed to
the card's session in the app" detail), or `requested` less than 60 s ago. So a request no session
ever picked up stops holding the repo after a minute. Order is `(requestedAt parsed as a time,
number)`; a waiting card with an earlier key also goes before a new one. `move_card` checks
`Store::ship_blocker` and calls `ship_wait` (state `queued`, `queuedAt`, `from`, detail naming the
blocker) under the same lock, returning `shipWaitFor` for the status line; `dispatch::after_move`
leaves a `queued` card's session alone and catches any other way in. `dispatch::run` calls
`pump_ships` every 2 s: one `COUNT(*)` when nothing is queued; otherwise `ship_cancel_strays`
(a waiting card that left Ship it by a session's move or archive is cancelled), then `ship_due` (per
lane with nothing holding it, the oldest `queued` card) and `ship_release` (back to `requested`),
and `after_move` runs for it with the saved `from`. Polling finds every way a ship ends without
hooks in each.

## Drop a card on a Ship it card to ship them as one (5 Nov 2026, card #89)

`Store::stack_join(lead, child, by, hugh)` (`crates/store/src/stacks.rs`) is the one join, in one
transaction. Its checks are `join_check`, which `Store::stack_join_check` runs with nothing
written. `stack_add` is `stack_join(.., false)`, the session's join with the old limits, plus
"nothing joins a stack in Ship it" and one repo per stack (`ship_lane` on both, when both have a
repo). With `hugh` (only the board's `stack_join` command):
- **Into a stack in Ship it:** allowed while `ship_started(lead)` is `None`, for a card in a
  `READY` column. Each card coming in (a dropped stack's cards too) must be ready.
- **Into a stack in Ship it, which cards:** only cards in Human test (or Ship it) can join. A card
  in Deploy complete has shipped already.
- **Out of Ship it:** allowed while the child's ship hasn't started. The child goes to Human test,
  with `ship` `{state: cancelled, with: N, detail: "Ships with stack #N"}` and `shipRequestedAt`
  cleared.
- **A stack into a stack:** the child's `stack` becomes null, and its cards follow it into the
  lead's list with `stackOf` rewritten. The child's column must be Human test or Ship it.
- **Passed:** every card that leaves Ship it or joins a stack in Ship it, and is in Human test, is
  marked Passed (`passedAt`).
- **Queue place:** `ships::earlier_place` gives a queued lead the child's earlier `requestedAt`.

The function returns `Joined { unshipped, came_with }`.

`ships::ship_started(card)` says how a ship started. Only a ship still waiting to start (`queued`,
or `requested` and not handed) is open. Anything else in Ship it counts as started: `shipping`,
`shipped`, `needs-hugh`, `requested` and handed, or a state that can't start (`failed`,
`cancelled`, none). `ship_goes_first(card)` says whether a card dropped into Ship it really waits
behind this one: true while it holds its repo in the queue, or once it has shipped.

`create_in_stack` checks what `stack_add` would refuse (a lead in Ship it, another repo) before it
creates the card, so a refusal leaves no card behind. When a whole stack joins, a card archived with
the old lead has its `archivedWith` cleared.

In the app (`src-tauri/src/main.rs`):
- `stack_join` (the command) checks the target first. If its ship has started,
  `ship_after_check` either answers `already` (the card is in Ship it) or moves the card into Ship
  it through `hugh_move`. It refuses when the card wouldn't really wait: the target doesn't go
  first (`needs-hugh`, or a ship in no state to start), or either card has no repo. That is `move_card`'s old body, now shared, with the ship queue and
  `after_move`. The reply carries `shipAfter`.
- Otherwise `stack_join_op(.., hugh)` joins. It builds the lead session's message with
  `dispatch::join_message`, which covers a lead waiting or asked for in Ship it, a card from Ship
  it, a whole stack, the same branch (nothing to merge, as for kanban-app cards on main) and a card
  not built yet. The card's own session gets `stand_down_message`.
- `dispatch::after_move` returns early for a card that `left_ship_for_stack` (by the cancelled
  ship's `with`). That covers a drop between Hugh's move into Ship it and its hand-over. A stack's
  card has no session of its own, so skipping a later `after_move` for it (a restore) is right too.
- `dispatch::built` is the one "built" test (Human test or later, or a PR). The message tells the
  session to plan and build any card of a joining stack that isn't built yet: under the "first"
  rule a stack in Human test can still have one.
- `stack_join_preview` (command) answers `join`, `after`, `already` or `refuse` with why. It runs
  the same checks as the join (`ship_after_check`, `stack_join_check`), with nothing written.

The board (`ui/stacks.js`):
- `stkDrop` asks `stack_join_preview`, then shows that dialog (`stkJoinAsk`, `stkAfterAsk`), or
  the refusal as a status line with no dialog. The board doesn't copy the rules.
- `stkJoinDo` reports what the app actually did.
- Every card dropped in a card's middle goes to `stkDrop`, a stack still being built too: the app
  refuses it with its reason. Before, a drop the board didn't take fell through to the column and
  moved the card there (an unbuilt stack into Ship it, for example).
- A card that moves with a joining stack and had left Ship it for that stack gets its cancelled
  ship's `with` and detail moved to the new lead.
- `status()` in `ui/board.js` keeps a message for 6 s. The board's reload after a change used to
  replace it at once with the card count, so no status line after a move or a drop stayed on
  screen.

`prompts/ship-it.md` tells a lead's session to merge such a card first and to check
`kanban_stack_list` before its merge. No migration.

## Messages typed at start-up are confirmed (5 Nov 2026, card #82)

Found in the test app's `api.log`: on a new or resumed session the first Enter after a typed
message is usually lost, and only `nudge`'s second Enter (6 s on) gets it in. When the TUI drops
the paste as well, that Enter lands on an empty prompt and the message was gone, already marked
`sent` in `pending`, with nothing on screen. Now `Sessions::send_message` (and `type_now` at the
prompt) keeps the text in `Live::typed` until the next `UserPromptSubmit` (any prompt counts:
matching text risked a second copy when the TUI changes it). Slash commands aren't kept
(`confirms`): local ones send no hook. `nudge` types it again after `RETYPE_AFTER` (15 s), only
at a quiet idle prompt (`retry_step`; never into a question dialog or mid-turn), under the lock.
The retype clears the prompt first with `retype_bytes`: Ctrl+U clears only the current line in
the TUI (tried: a 3-line paste plus Ctrl+U plus a new paste sent lines 1-2 of the old one), so it
sends Ctrl+U and Backspace once per line plus two. After `TYPE_TRIES` (3) it goes to
`Live::untaken`, logged on the card.
Card #175 (9 Nov 2026): retyping could leave the text in the prompt two or three times over.
Seen on a busy test app: after a resume the TUI read its input a minute late, so every retype
queued another copy behind the first (it then took the first by itself, after the app had given
up); and at an idle prompt the text sat there until one more bare Enter sent it. So `retry_step`
now looks first: if the terminal has printed nothing since the message was first typed
(`RetryCheck::drawn`, `last_out` against `Typed::first`), the TUI hasn't read it yet and the app
waits, spending no tries, up to `DRAW_WAIT` (3 min); if the prompt box shows it (`prompt_holds`:
its first 40 characters, or "[Pasted text #" for a long paste, read off the vt100 screen), it
presses Enter alone (`Retry::Enter`, logged "Pressed Enter again ..."); only an empty prompt,
or one holding two copies, gets the retype. A placeholder counts only for a message long
enough to be collapsed (more than 2 lines or 800 characters), so Hugh's own paste isn't sent.
A `UserPromptSubmit` whose prompt opens like the `untaken` message clears it (`same_start`),
so a message the TUI took after the give-up no longer shows "Not sent". Known limit: output
the TUI prints without reading its input (a resume redrawing the conversation) counts as
`drawn`; an empty prompt then still gets the retype, as before. While a message is unconfirmed or untaken, nothing else is
typed at the prompt (`free`, `takes_typing`'s `unconfirmed`) and the session isn't paused.
`stop_all` puts an unconfirmed or untaken message back in `pending`, so a restart types it after
the resume. `session_info` gives the panel `waitingText` (the `pending` payload, the app's own
text included), `typing`, `untaken` and `typeTries`; `panel.js` `outbox()` draws them as queued
bubbles, and "Not sent" with **Send again** (`session_send`) and **Dismiss**
(`session_dismiss_untaken`). Known limit: if the `UserPromptSubmit` hook itself is lost, a message
Claude did take is typed again once the session is idle.

## Take it to: auto-progress (5 Nov 2026, card #76)

`autoTo` is an ordinary card field (no migration). The store cleans it in `cards.rs`
`clean_auto_to` on every create and update: `"Human test"` or `"Deploy complete"` (`AUTO_TARGETS`,
any case, trimmed), or `null` / `""` to remove it; `Card::auto_to` reads it. It isn't in `tools.rs`
`UPDATABLE`, so sessions can't set it; Hugh sets it through `update_card` (the panel).

- **Starting a new card:** `main.rs` `create_card` takes `to`. Anything past Backlog drops
  `column`, the two targets also set `autoTo`, and `dispatch::start_new` moves the new card from
  Backlog into Planning with `Store::move_for_hugh`. `kanban_create_card` takes `to` (`Planning` or
  `Human test`) only from a caller with no card of its own; a card's session is refused, and
  `Deploy complete` is refused from every tool. With no `to`, a non-card caller's `column` of
  Planning or In progress (with no `stack`) goes through `start_new` too, so its session starts.
- **`move_for_hugh`** shares `move_as` with `move_by_hugh`: the same plan and ship rules, written
  and logged as `AUTO_BY` ("the app (Hugh's Take it to)") with the reason, no `movedByHand`, and
  `"auto": true` on the plan or ship it asks for. `move_by_hugh` clears `autoTo` on any move outside
  `AUTO_KEEPS` (Planning, In progress), logging "Auto-progress off" (or "Auto-progress done" when he
  moved it to its target himself).
- **What sessions are told:** `dispatch::auto_moved` spots the app's own move (an `auto` plan or
  ship still requested, started or queued), so `move_message` reads "The app moved this card ...
  for Hugh". `auto_note` adds a note for the column (Planning: move it to In progress yourself; In
  progress: where it stops; Ship it: this is Hugh's approval) on a card with `autoTo` that isn't a
  stack child or archived. `held_move_message` now also sends the In progress prompt after a
  session's own held move into In progress on such a card. `autoTo` is in `TOLD`, and
  `edit_message` says Hugh's change in words.
- **Moving on:** `dispatch::auto_progress` runs after `session_move`, after Hugh's edit of `autoTo`
  (`after_edit`), and from `after_stack_move` when the rules move a lead into Human test. A card in
  its target loses `autoTo`. A `Deploy complete` card in Human test goes into Ship it by
  `move_for_hugh`, then `after_move`, which queues it behind another ship of its repo (card #79);
  `ship_wait` and `ship_release` keep the `auto` mark. The held-move loop in `dispatch::run` skips
  `held_move_message` when the card has already left the column it was moved to. A failed ship
  sends the card back to In progress as before, with `autoTo` kept, so it goes round again until
  the third failure.
- **UI:** `ui/board.js` `TAKE_TO` (the Add form's select, hint and button text), `autoChip`, and
  the panel's "Goes on to" select (`AUTO_OPTS`; `autoPend` stops a redraw putting the old choice
  back while it saves; no confirm popup since card #87: the saved line says Deploy complete ships
  with no human test). Styles in `ui/board.css`
  (`.chip.auto`, `.chip.auto-deploy`, `.hint`).
- **Prompts:** `planning.md` "If the card goes on by itself"; the Ship it wording in `planning.md`,
  `in-progress.md`, `ship-it.md`, `kanban-app.md` and `AGENTS.md`.
- **Tests:** `crates/store/tests/auto.rs`; `dispatch.rs` `auto_progress_notes_tell_the_session_what_to_do`.

## A message to a card in Human test takes it back to work (5 Nov 2026, card #93)

- `session_cmds.rs` `back_to_work`, called first by `session_send` and `session_send_now`: a card
  for which `dispatch::message_moves_back` holds (in Human test, not archived, not in a stack)
  is moved with `Store::move_by_hugh` to In progress (so `test` goes back to `needed`), logged
  "Hugh's message took it back to In progress", and the text becomes
  `dispatch::moved_back_message` (a note, then "Hugh's message:" and his text; dry-run cards get
  the Dry run prefix). The move itself dispatches nothing (no `after_move`), so the session hears
  it once, in the message. A send that fails after the move is logged on the card.
- `dispatch::move_message` adds `BACK_FROM_TEST` to Hugh's own move from Human test to In
  progress, so the session looks for what failed rather than building from scratch.
- Not covered: keys typed in the terminal (`session_input`), stack cards, and a message that
  waits behind the card's own held move to Human test (it moves after the message is queued, so
  the card can end in Human test with the message being worked on).
- Test: `dispatch.rs` `a_message_to_a_card_in_human_test_takes_it_back_to_work`; the test reset is
  `crates/store/tests/auto.rs` `a_move_into_in_progress_needs_a_new_test`.

## Subagent test before Human test (5 Nov 2026, card #76)

`src-tauri/src/tester.rs`, Hugh's rule that every card is tested by a subagent before Human test.

- **The subagent:** `sessions.rs` `build_args` passes `--agents` with `tester::agents_json` to every
  card session that has the API, on start and resume: `card-tester`, model `inherit`, with
  `disallowedTools` Edit, Write, NotebookEdit and the `kanban` tools that change cards (move,
  update, create, ship result, set passed, stack add, remove, join, detach). Bash stays, since it has to
  run things, so the rest of "fix nothing" rests on its prompt. The prompt ends its report with
  `CARD: #n` and `VERDICT: PASS|FAIL` (then `- ` lines for the failures).
- **The verdict:** `settings_json` adds a ninth `http` hook, `SubagentStop` (no matcher), and
  `dispatch::on_hook` hands it to `tester::on_subagent_stop`. That ignores any `agent_type` but
  `card-tester` (or `<plugin>:card-tester`), reads `last_assistant_message` with `parse_verdict`
  (the last CARD and VERDICT lines win; bold and backticks allowed) and counts the
  `"type":"tool_use"` entries in `agent_transcript_path`. `record_of` makes the card field `test`:
  `{state, at, agentId, detail}`. A PASS with zero tool calls, or no VERDICT line, is `unclear`; an
  unreadable transcript trusts the verdict. `target_card` records it on the card named only if that
  is the session's own card or a child of the stack it leads, else it logs "Not recorded". Written
  as `tester::BY` ("the app (card-tester subagent)").
- **The gate:** `cards.rs` `apply` sets `test` to `needed` on every move into In progress (unless
  the same patch sets `test`). `tester::move_refused` refuses a move into Human test or Deploy
  complete from any column before it (Backlog, Blocked, Planning, In progress) until `test.state`
  is `passed`, and says how to run the tester. It is checked in `tools.rs` `kanban_move_card` for
  card sessions only (so the session hears at once), and again in `dispatch::run` when a held move
  is applied. Hugh's own moves, his own sessions' `kb move` and the stack rules' moves aren't
  checked; a lead set to Deploy complete ships only once every card in its stack is ready
  (`auto_progress` checks `stack_ready(.., "all")`), whatever the board's stack rule. Under the
  "first" rule the lead is already in Human test by then, so `after_stack_move` runs `auto_progress`
  on a child's lead after every move of the child. `config.rs` `TEST_RULE_KEY`
  (`sessions.test_before_human_test`, `on` or `off`, default on, a page setting) and
  `Store::test_required` switch it.
- **Hugh's own sessions** aren't held to it: they have no `card-tester` and no `SubagentStop` hook.
- **Making a card into Planning or In progress** from a session that isn't a card's (Hugh's own,
  `kb create --column Planning`, as the kanban-agent skill does) now starts it through
  `dispatch::start_new`, as his move there would. A card's own session only places cards. If the
  start fails, the card stays made in Backlog and its log says why.
- **UI:** `ui/board.js` `testChip` (In progress and Human test only) and the panel's "Subagent
  test" row; `ui/settings.js` `sessions` has the checkbox. Styles in `ui/board.css`
  (`.chip.test-passed`, `-failed`, `-unclear`).
- **Prompts:** `in-progress.md` "Review and test it with subagents" (was "Test it with a
  subagent"), `kanban-app.md` step 6a.
- **Tests:** `tester.rs`'s unit tests; `sessions.rs` `card_sessions_get_the_apps_subagents` and the
  hooks test; `crates/store/tests/auto.rs` (the reset and the setting).
- Since card #96 the verdict code is shared, in `agents.rs` (below).

## The app's subagents, and the review before Human test (6 Nov 2026, card #96, stack A)

Built on the branch `dsh-agents` in the worktree `~\kanban-app-dsh`. The cards
for the critic (#97), router (#98), monitor (#99), merger (#100) and reproducer (#112) were
detached from the stack and build in their own sessions, from this card's first commit.

A new PASS/FAIL subagent touches these lists, one entry in each it needs: `agents.rs` `AGENTS`
(always) and `GATES` (if it holds a move); `kanban_store` `RESET_ON_IN_PROGRESS` (or its own
reset), `RULE_KEYS` and `PAGE_SETTINGS` (if it has a switch); `stacks.rs` `stack_recheck`'s
`lead_rules` (if a lead is held to it); `ui/board.js` `CHECKS` and `ui/settings.js`
`SUBAGENT_RULES`.

- **`agents.rs`, one table:** `AGENTS` lists every subagent a card session gets (`&tester::AGENT`,
  `&reviewer::AGENT`); `agents_json` makes the `--agents` JSON from it (`model: inherit`, plus
  `effort` where an agent sets one; the reviewer's is `high`). `denied` gives each agent Edit,
  Write and NotebookEdit (unless `edits`), the nine card-changing kanban tools under **both** server
  names (`kanban` from `--mcp-config`, `kanban-app` from the repo's `.mcp.json`, which a
  kanban-app card session also loads; before #96 only the first was denied). Whole tools only:
  card #100's live trial on Claude Code 2.1.318 showed that a rule with a specifier in an agent's
  `disallowedTools` (`Bash(kb:*)`) takes the whole tool away, which left the agents PowerShell
  with nothing blocked; so the `Bash(...)` rules the plan asked for (router decision X3) were
  taken out again, and blocking `kb` and `claude config` belongs in a `PreToolUse` guard hook
  (card #100 builds one for the merger). `find` maps a hook's `agent_type` (or
  `<plugin>:<name>`) to its agent. `on_subagent_stop` hands a `SubagentStop` to the agent's
  `OnStop`: `Check` (a PASS/FAIL verdict) or `Custom` (its own handler).
- **`Check`:** `record_check` is what `tester::on_subagent_stop` was, made general:
  `parse_verdict`, `tool_calls`, `record_of` (now with `round`, the FAILs since the reset), and
  `target_card`. `record_of` masks the detail and the log line with `kanban_store::mask_secrets`
  (a sign-in link or a `Bearer` value becomes hidden, a token becomes "[hidden]", the bare word
  "password" becomes "pass word"): the board refuses
  text that looks secret, so an unmasked FAIL such as "a password in config.rs" would have lost
  the verdict. A `Check` with `cap` (the reviewer's is 3) sets `blockedReason` to the findings
  at its cap, and queues a held move to Blocked for the session's own card, or moves a stack child
  at once (`dispatch::session_move`), and queues a message telling the session to stop. The record
  gets `capped: true`, which later verdicts carry (`carry_cap`) until the reset. While it is set
  and the card is in a column before Human test, `capped_hold` (in `reviewer::gate`) refuses a
  card session's move into In progress (which would reset it) or on into Human test or Deploy
  complete, so neither a later PASS nor the session's own next move (which would replace the held
  one) gets it past. Once Hugh has moved it past those columns, its session's own moves are free
  again. `lead_missing` counts a capped lead's record as not passed, so the stack rule doesn't
  move it on either. Hugh's move into In progress resets the record and the cap (for a lead the
  stack rule moved back to In progress, by moving it out and in again).
- **`GATES`:** `move_refused` asks each rule in turn (`reviewer::gate`, then `tester::gate`) and
  joins their reasons. `tools.rs` `kanban_move_card` and the held-move path in `dispatch::run`
  call `agents::move_refused` (they called `tester::move_refused`). `not_passed` is the shared
  column-and-state check.
- **The review:** `reviewer.rs`. Field `review` (`kanban_store::REVIEW_KEY`), reset to needed with
  `test` on every move into In progress (`cards.rs` `RESET_ON_IN_PROGRESS`). Setting
  `sessions.review_before_human_test` (`REVIEW_RULE_KEY`, on by default); `RULE_KEYS` holds the
  on/off rules, and `Store::rule_on` reads one. The prompt finds the card's commits by
  `(card #n)`, takes the work not committed, and has a path for a card that changed no repo.
  Tester and reviewer mark a visible bug with `- BUG:` (card #112 reads it).
- **A stack's lead:** `stacks.rs` `stack_recheck` moves a ready stack to Human test only when
  `lead_missing` is empty: the lead's own `test` and `review`, each while its rule is on. Its
  `stackHeld` field holds the reason, so the log line is written once per change of reason. When
  a lead's own verdict passes, `record_check` calls `dispatch::after_lead_verdict`, which runs
  `stack_recheck` again (nothing else would: the rules run on a child's move). When a child sends
  the stack back to In progress, the lead's move carries its own `test` and `review` in the patch,
  so the In progress reset keeps them: the lead's work didn't change.
- **Config guard:** `configwatch.rs`. On a `PreToolUse` for `Agent`/`Task` whose `subagent_type`
  is one of `AGENTS`, a per-session snapshot (if none yet) of four files:
  `~\.claude\settings.json`, `~\.claude.json`, and account2's two in `~\.claude-max2`.
  `settings.json` is hashed whole; `.claude.json` only for `CONFIG_KEYS`, top level and per
  project, because Claude Code rewrites its counters on every session. A watch counts the app's
  agents running in that session; each `SubagentStop` of one compares (`changed`), and the watch
  ends with the last one (else it starts again from now), so a change made between runs is never
  blamed on the next run. A watch whose stops never came is dropped after 3 h. A change writes a
  WARNING log line on the session's card, a "Told Hugh" line, and a `notify` event. It never
  writes the files. Known limit: the files are shared by every session, so a change by another
  session (or Hugh) during the run is reported too; the line says so.
- **Prompts folder:** `build.rs` sets `KANBAN_PROMPTS_DIR`; `dispatch::PROMPTS_DIR` and
  `BACK_FROM_TEST` use it. A test build (`KANBAN_CHANNEL=test`) gets `<checkout>\prompts`
  (`git rev-parse --show-toplevel`), so a test build from a worktree points at that worktree's
  prompts. A live build gets main's checkout's `prompts`, wherever it was built, so a live
  build made from a worktree never points sessions at a folder that goes away. These are only
  the bundled examples: a private folder's `prompts\` (Settings › Sessions › "Private folder")
  replaces them, and `{prompts}` in the column prompts and templates names whichever one is in
  use (`src-tauri/src/private.rs`).
- **UI:** `ui/board.js` `CHECKS` (one entry per verdict field: the panel row, the chip words, the
  columns), `checkChip`, `checkRows`; `testChip` returns every check's chip. `ui/settings.js`
  `SUBAGENT_RULES` and `ruleBox`.
- **Prompts:** `in-progress.md` "Review and test it with subagents" (review first, then test,
  until card #162 ran them together); `kanban-app.md` 4a (review before the install, until #162), 5 (log on a stack's Human test cards before a
  reinstall), 6a. Every new step says what to do if the session has no such subagent.
- **Tests:** `agents.rs` (the verdict, the rounds and cap, every agent defined and fenced, `find`,
  `target_card`, both rules and each switch), `reviewer.rs`, `tester.rs`, `configwatch.rs` (a
  fake home folder), `sessions.rs` (every agent passed; the command line under 30,000
  characters), `crates/store/tests/auto.rs` (the reset, the setting), `crates/store/tests/stacks.rs`
  (`a_ready_stack_waits_for_the_leads_own_test_and_review`).
- Since card #162 the review and the test run together, after the install (below).

## The reviewer and the tester run together (9 Nov 2026, card #162)

Hugh's question: "Could these two be run in parallel?" Yes. The reviewer reads the diff and the
tester tries the installed build; neither needs the other's result.

- **Prompts:** `in-progress.md` *Review and test it together* replaces *Review it first* and
  *Then test it*: install first, then two Agent calls in one message (`card-reviewer`,
  `card-tester`, both `run_in_background: true`), wait for both notifications; on any FAIL from
  either, fix, reinstall and run both again. The reproducer's flow (before the fix, and after the
  fix is installed) stays before the pair. `kanban-app.md` 4a no longer reviews before the
  install; 6a runs both. The stack sections say a child's review and test run together. The
  reviewer's and tester's descriptions and gate texts (`reviewer.rs`, `tester.rs`) say "together"
  and no longer "before the card-tester".
- **Records:** every `test` and `review` record (and every other PASS/FAIL agent's) keeps
  `startedAt` and `failedAt` (`kanban_store::stamp_times`, called in `agents::record_check` next
  to `carry_cap`). `startedAt` is when that run's Agent call started: `agents::on_pre_tool` now
  notes a `Start { at, place }` for every card's PASS/FAIL agent run, keyed as before by (session
  row, agent), oldest first; `place` is card #104's snapshot, only for a card in a listed repo
  (so `placeMoved` is set only when a place was taken). When no start was seen, `startedAt` is the
  verdict's `at`. `failedAt` is the latest FAIL's `at`, carried on by later records; the move into
  In progress resets the record to `{state: needed}`, which drops it with `round`.
- **The rule:** `kanban_store::stale_passes` (`crates/store/src/pair.rs`) finds a `passed` test
  whose `startedAt` (else `at`) is before the review's `failedAt`, and the same the other way
  round. `agents::stale_pass` is the gate, last in `GATES`, so `move_refused` (the
  `kanban_move_card` tool, the held-move path, the monitor) refuses the moves `not_passed` holds
  (from a column before Human test into Human test or Deploy complete). Only while both subagents
  are on for the card (`Store::subagent_on`); not for a lead whose children's reviews stand in
  for its own, nor while either record is capped (`capped_hold` speaks then). The message names
  the FAIL's time and which to run again ("The reviewer failed at 14:05, after this test started
  (14:02): the fix wasn't tested. Run the card-tester again."), in local time (`hhmm`).
- **A stack's lead:** `stacks.rs` `lead_missing` holds a ready stack for a stale pass of the
  lead's own, while both its rules are in the list and on ("subagent test (started before the
  reviewer failed at 14:05)"). A child's merge (`stackrun.rs`) doesn't read the two records: it
  starts when the child reaches Human test, and that move is the child's own session's, held by
  `move_refused` like any other card's.
- **Known limits:** starts and stops are paired oldest first per (session, agent), as for card
  #104, so a lead running two testers at once for two children may pair them the wrong way round.
  A run started after a FAIL but before the fix was made still counts: the rule compares times,
  not commits (card #104's snapshot does that, for listed repos only). Records from before this
  build have no `failedAt`, so an old FAIL holds nothing.
- **Tests:** `crates/store/tests/pair.rs` (the stamps, both directions, the fallback, the reset,
  the lead's hold), `agents.rs` `a_pass_that_started_before_the_others_fail_is_refused` (the gate,
  through `move_refused`, the switches, the cap), `agents.rs` `a_verdict_counts_only_for_work_unchanged_while_its_agent_ran`
  (a start with no place), `src-tauri/tests/prompt_rules.rs` (the prompts' order, and no agent
  text saying the review comes first).

## The plan critique before building (6 Nov 2026, card #97)

Built on the branch `dsh-97` in the worktree `~\kanban-app-97`, from `dsh-agents`
at #96's 146e29d. It plugs into #96's tables; nothing of #96's is changed.

- **`critic.rs`:** `AGENT` (`card-critic`, effort `high`, no edits, #96's common fences) and
  `CHECK` (field `critique`, label "Plan critique", reset in Planning, cap 3), one line each in
  `agents::AGENTS` and `agents::GATES` (the critic's gate first). The verdict, the rounds and the
  cap are #96's `record_check`, unchanged. The prompt marks a finding that is Hugh's to decide
  with `HUGH:`; it is kept under 2,000 characters for #96's command-line test.
- **The hold:** `critic::gate`: a card session's move into In progress, Human test or Deploy
  complete (`INTO_BUILD`, so stale test/review passes can't carry an unchecked plan past In
  progress) from Backlog, Planning or Blocked needs `critique.state == "passed"`, while `sessions.critic_before_build`
  (`CRITIC_RULE_KEY`, in `RULE_KEYS`, on by default; `Store::critique_required`) is on. From
  Blocked it is free when the build had started: `critique.started`, or with no record a `test`
  field (set on every move into In progress since #76: blocked mid-build before this rule). A card
  made straight into Planning or Blocked has neither, so it is held. A session started before
  this build has no card-critic; the refusal tells it to leave the move to Hugh. Like the others, only card sessions are held
  (`tools.rs` checks `agents::move_refused` only with a calling card); Hugh's moves and his own
  sessions' `kb move` never are.
- **The plan's fields:** `PLAN_FIELDS` = `summary`, and card #105's `doneWhen` and
  `doneWhenChecks` (no harm while #105 isn't merged: nothing writes them).
- **The resets:** `cards.rs` `critique_resets`, at the end of `apply`, so every write path (Hugh's
  moves, sessions' moves, stack moves) gets them: a move into Planning sets
  `{state: needed, round: 0}`; a move into In progress adds `started: true` (making the record if
  there is none); a changed `summary` in Planning, or in Blocked with a record and no `started`,
  sets `{state: needed, round: <kept>}` (in Backlog too), so the cap still counts FAILs across plan
  fixes. A patch that sets `critique` itself (the app recording a verdict) is kept, except that a
  `started` from before is carried over (`record_of` writes the record whole).
- **Off:** `dispatch::move_message` adds `CRITIQUE_OFF` to a move into Planning; the Take it to
  notes (`auto_note`, the `autoTo` edit note) say "once its card-critic has passed it".
- **UI:** one entry in `ui/board.js` `CHECKS` (chips on Planning and Blocked; `hide` drops it once
  `started`); one in `ui/settings.js` `SUBAGENT_RULES`.
- **Prompts:** `planning.md` *Check the plan with a subagent* (step 6; the stack lead critiques
  each child; the Take it to line), `kanban-app.md` Planning step 6. All conditional on the
  session having the subagent.
- **Known limits:** a plan edited outside `summary` doesn't reset it. `CRITIQUE_OFF` goes only
  with a move into Planning, so a session already in Planning when Hugh flips the rule isn't told
  (the gate itself follows the rule at once). The cap is #96's `record_check`, which doesn't look
  at the rule: a session that still runs the critic with the rule off can be capped.
- **Tests:** `critic.rs` (the gate's columns, states, `started`, no record, the switch, through
  `agents::move_refused`, the agent's effort and prompt), `crates/store/tests/critique.rs` (every
  reset, the round kept across fixes up to the cap, `started`, Hugh's moves, the setting).

## A gentler plan critic (9 Nov 2026, card #144, branch `dsh-144`)

Hugh: "your critic is too strong - lower it down". Small UI cards (#127, #128, #130) went 6+
rounds on test-script details. Built in `~\claude-kanban-dsh-144` on `dsh-144`.

- **The prompt** (`critic.rs` `PROMPT`, under 2,000 characters): a blocker is only a dropped
  deliverable, a plan that would come out wrong in a way the build wouldn't catch (file:line), or
  a step that is unsafe or breaks a hard rule (live, data, secrets). Everything else is a note;
  notes never fail a plan. Scope questions are HUGH: lines. After the VERDICT line come `- ` lines:
  the blockers after a FAIL, the notes after a PASS, and any HUGH: lines.
- **Notes** (`agents.rs`): `parse_verdict` reads the `- ` lines after a PASS into `Verdict.notes`
  (shared code: any agent's PASS can carry them). `record_of` keeps them on the record as `notes`
  (`note_list`: at most `MAX_NOTES` 8, masked, cut to `NOTE_LEN` 300) and logs
  "<label>: PASS, N notes: ...". On the critic's record only, any `HUGH:` line (`is_hugh`) sets
  `hugh: true`.
  `pass_fails_on_checks` clears the notes when it turns a PASS into a FAIL.
- **The build gets them** (`critic::build_notes`, `dispatch::build_notes_for`): the In progress
  message ends with them when the card comes in from Planning or Backlog, through `move_message`
  (Hugh's and the Router's moves), `app_move_message` (the small-card move) and
  `held_move_message` (now given `from`; a Take it to card's own move). Not from Human test or
  Blocked.
- **The holds** (`critic::gate`), only where today's applies (rule on, from Backlog/Planning/
  Blocked into the build) and after the mid-build pass-through: `hugh: true` on the latest
  critique, and a card routed small (`kanban_store::routed_small`: `route.size` small, not in a
  stack) whose critique is marked `oneFailed` (`critic::one_failed`; not inferred from `round`,
  so a FAIL from before a Route again doesn't count). Both refuse a session's and the Router's
  move; Hugh's never.
  `router::go_on_blocker` waits on the same two.
- **One critique for a small card**: `cards.rs` `critique_resets` keeps a small card's passed or
  failed record through a plan edit (in Planning, Backlog or Blocked before the build);
  `agents::one_critique` marks a small card's FAIL `oneFailed` (`kanban_store::ONE_FAILED_KEY`),
  saves its findings (not its HUGH: lines) as its `notes`, skips the cap, and keeps the mark and
  those notes (ahead of its own) if the critic is run again anyway.
  A move into Planning still resets it to round 0.
- **Texts**: the gate's refusals, the Take it to notes (`auto_note`, the autoTo edit note), the
  Settings help (`ui/settings.js`), `prompts/planning.md` (route step 6, *Check the plan*, *If the
  card goes on by itself*) and `prompts/kanban-app.md` Planning step 6; the private copies on the
  private repo's `card-144` branch, merged at Ship it.
- **Known limits:** sessions started before the install keep the old critic (`--agents` is fixed
  at session start). Planning to Backlog to Planning resets the record, as it does for the cap. A
  card that went to Blocked before its build and then to In progress gets no build notes (the
  message can't tell it from a card blocked mid-build).
- **Tests:** `agents.rs` (`a_pass_keeps_its_dash_lines_as_notes`,
  `a_hugh_line_is_flagged_on_a_pass_or_a_fail`, notes cleared by a failed check), `critic.rs`
  (the prompt, the small-card and HUGH: holds, build notes, no cap for small), `router.rs`
  (`a_small_card_waits_for_hugh_after_a_fail_or_a_question`), `dispatch.rs`
  (`the_in_progress_message_ends_with_the_critics_build_notes`), `crates/store/tests/critique.rs`
  (`a_small_cards_one_critique_survives_a_plan_edit`).

## Sizing off means no sizing (9 Nov 2026, card #161)

- Hugh (9 Nov): "If it's off, it should be off. It shouldn't do it later. It should just go with
  my defaults." A card is sized only by `presize` before its session starts (setting on), or by
  Hugh's Route again. Otherwise no card-router run, no route, no restart: Hugh's column defaults.
- **The note:** `presize::unsized_why` / `unsized_note`: a Planning card with no route, not a
  stack's child, its card-router on, and not being sized now (no `sizing` mark, not `in_flight`)
  is told "Card #n isn't sized: <why>. Don't run the card-router ..." (why: the setting is off, or
  the app didn't size it before this session started). `dispatch::subagents_note` adds it in
  Planning, so the move, held-move and app-move messages carry it; a card with its card-router off
  keeps the old line only. A move that starts a sizing computes its message while the card is in
  flight, so it has no note; `presize::start_after` then queues `presize::note` (routed) or the
  unsized note (failed), merged into the first, still-waiting message.
- **Failure:** `failed_line` says the session starts on Hugh's defaults and isn't sized later.
- **Refusals:** `router::record_with` records nothing while the setting is off
  (`OFF_NOT_RECORDED`), for the subagent and for a pre-size reply that lands after it was turned
  off (`presize::TURNED_OFF`). While on, `router::on_stop` records an in-session card-router's
  route only when Hugh's Route again asked for it (`routeAsked {at}`, set by `route_again`,
  removed by the recorded route; else `NOT_ASKED` is logged), so a session that routes anyway
  (the board Router's free-text start, say) causes no restart. `router::again_blocker`: Route
  again is refused while off.
- **UI:** `board.js` `sizeFirstOff` (read with `get_setting`, again on `settings-changed`): the
  Route row says "Not sized: sizing is off (Settings › Sessions)" and has no Route again. The
  Settings help for the setting and for the small-card go-on say what off does.
- **Prompts:** the bundled and private `planning.md` (*Route it first*) and the private
  `kanban-app.md` (Planning step 0): skip routing when the app's message says the card isn't sized.
- Changed from #159: a card entering Planning while its session already runs is no longer routed
  in-session (with the restart); it gets the note. Unchanged: Route again while on, and existing
  routes (they still apply after the setting is turned off, so no session restarts).
- **Tests:** `presize.rs` (`a_planning_card_is_unsized_unless_it_is_routed_or_being_sized`, the
  failed line), `dispatch.rs` (`a_planning_card_that_isnt_sized_is_told_to_run_no_card_router`),
  `router.rs` (`with_sizing_off_no_route_is_recorded_and_route_again_is_refused`),
  `tools/ui-tests/sizing.test.mjs`.

## Sizing a card before its session starts (9 Nov 2026, card #159)

- **`presize.rs`**, behind the setting `sessions.size_before_start` (`Store::size_first`, a
  `RULE_KEYS` rule, on by default). `begin_session` (live_cmds.rs) first asks `presize::hold`:
  when `blocker` finds nothing (the setting on; a start that `counts`: why "move" or "router",
  or "hugh" with mode "new"; Planning or In progress; no route; not a stack's child; the card's
  card-router on; no live session) it marks the card `sizing {at, mode, why}`, logs it, and a
  thread runs one `claude -p` (`args`: `--output-format json`, `--effort low`, `--tools
  Read,Grep,Glob`, `--strict-mcp-config`, hooks off, nothing saved, 20 turns, 3 min timeout; the
  brief on stdin; the child in the sessions' Job Object via `Sessions::adopt`). The brief is
  `router::RULES` (the subagent prompt's steps 2 to 6 and closing lines, one `sizing_rules!`
  literal, so the subagent's prompt is unchanged) with the card written in. `record` uses
  `router::record_with(.., before: true)`: the route gets `before: true` and the same write
  clears `sizing`. Then `begin_session_now` starts it the normal way (or in the cap's queue),
  and a Planning session gets `presize::note` (skip the card-router, go on from step 3).
- Meanwhile `begin_session` returns `Begun::Sizing` for every start of that card
  (`in_flight`): callers treat it like a queued start, so its messages wait. `session_info`
  has `sizing`; the face shows "Sizing…" in the route chip's place (`board.js` `sizingOf`).
- A failed call (no claude.exe, a timeout, an error result, no ROUTE line) logs
  `failed_line`, clears the mark and starts the session with no route: the planning prompt's
  card-router routed it, with the old restart (card #161: now its session runs on Hugh's defaults
  and is told not to route it). `resume_at_start` sizes again a card left marked
  by an app restart.
- Not changed: Route again, and `restart_if_due`'s switch back to the column's values when a
  card leaves In progress.

## The card-router subagent (6 Nov 2026, card #98, stack A)

Built on the branch `dsh-98` in the worktree `~\kanban-app-98`, from `dsh-agents`
(card #96's commits).

- **`router.rs`:** the `card-router` (effort `low`, Hugh's call) is an `OnStop::Custom` entry in
  `agents.rs` `AGENTS`: not a PASS/FAIL check, so no gate and no reset. `parse_route` reads its
  CARD, ROUTE, MODEL, EFFORT, CHECK and REASON lines (the last of each wins; `- ` lines after the
  last ROUTE are a split's children). `record` puts `route {size, model, effort, check, reason,
  children, at, agentId, by}` (`kanban_store::ROUTE_KEY`) on the card it names (`target_card`),
  once: a route that isn't cleared isn't overwritten. A split on a stack child is recorded as
  normal. A model outside `MODELS` (the panel's list) or an effort outside `EFFORTS` is recorded
  and logged but never applied. Sessions can't write `route` (it isn't in `tools.rs` `UPDATABLE`).
- **Model and effort:** `router::settings_for` is what `session_settings` uses now: Hugh's
  `modelOverride`/`effortOverride`, else the route's (only in `ROUTED_COLUMNS`, Planning and In
  progress, only known values), else the column's. Each comes with its source ("Hugh", "route",
  "column"); `session_info` returns them as `modelFrom`/`effortFrom` for the panel.
  `restart_if_due` runs in `dispatch::run` for each free session, before `apply_switch`. For a
  card with a route (or `routeCleared`, which Route again leaves: the old route's model and
  effort), `reconcile` finds each field whose running value differs from the wanted one, when the
  wanted value is the route's or the running value is the (cleared) route's. If any, the app logs
  it, stops the session and resumes its conversation (`begin_session` "resume"), so the new
  session starts with `--model`/`--effort` from `settings_for`; its waiting message is typed after.
  It never types `/model` or `/effort`: Claude Code saves those as Hugh's default in
  `~\.claude\settings.json` ("saved as your default for new sessions"), which the app must never
  write (the card-tester caught this on the first build). Not while Hugh has the card open
  (`notify::looking_at`), not while one of its subagents still runs (`subagent_running`, from the transcript's subagents, read off the board lock, with the monitor's `QUIET` window; a session waiting on a background card-tester, card-reviewer or card-critic looks idle, so it is tried again every 30 s), and one restart per card at a time (`RESTARTING`). From the decision until the resumed session starts, `router::restarting` holds all typing into that card: the dispatcher skips it, the chat's `send_to_session` and the monitor's nudge leave their text queued. Without it, the In progress prompt of a card routed small could be typed into the old session just before the restart stopped it (the ping-pong test's #187 and #188). It never touches a
  card with no route.
- **Small cards go on:** `go_on_if_ready` (after a route is recorded, and from `record_check` after
  any PASS/FAIL verdict) queues the app's own held move to In progress when `go_on_blocker` finds
  nothing: Planning, route small, `critique.state` passed with its `at` later than the route's
  (card #97's field, `CRITIQUE_KEY`; an older pass was for an older plan), not a stack child or
  lead, `route.wentOnAt` not set, `sessions.small_go_on` on, and no move waiting. The held-move
  path in `dispatch::run` takes the payload's `by` when it is `router::BY`, runs `go_on_blocker`
  again (Hugh may have dragged the card, cleared its route or turned the rule off; a move that no
  longer applies is dropped with a log line and no message), then the move gates, and on success
  marks the route (`went_on`, so once per route) and sends `app_move_message` (the In progress
  prompt). On `dsh-98` alone nothing writes `critique`, so this only fires once #97 is merged.
- **Route again:** `route_again` (`session_cmds.rs`): Planning only (a message to a Human test
  card would move it back, #93), not for a child, not for a conversation in the desktop app. It
  clears `route` as "Hugh (panel)", sends `again_message` through `send_to_session` (the route is
  put back if that fails), and leaves `routeCleared` so `restart_if_due` takes a running session
  off the old route's model and effort. A new route removes `routeCleared`.
- **UI:** `ui/board.js` `routeChip`, `routeRow` and the `#route-again` button; `ui/settings.js`
  `SUBAGENT_RULES` gets `sessions.small_go_on`; `ui/live-switch.js` marks a model or effort that
  comes from the route.
- **Prompts:** `planning.md` "Route it first"; `kanban-app.md` Planning step 0. Both conditional.
- **Tests:** `router.rs` (parsing, the record and its `by`, once per card, a child's split, bad
  values, `settings_for` by column, the reconcile both ways and with Hugh's override, the
  small-card move and every case that stops it, low effort and the prompt's length);
  `dispatch.rs` (`app_move_message`); `crates/store/tests/auto.rs` (the setting, the route kept
  through moves and cleared).
- **Known limits:** the app knows a session's model and effort only from what it started it with
  and typed into it, so a `/model` Hugh types in the terminal himself is unseen: leaving In progress
  can restart that session on the column's model. **Not fixed here (older than this card):** the
  panel's own model and effort switch (`set_session_model_effort`, `apply_switch`) still types
  `/model` and `/effort`, so it too saves Hugh's choice as his default in `~\.claude\settings.json`. `reconcile_live` reads each free session's
  card every tick (one indexed read; only routed cards go further).
- **Git:** every worktree of this repo shares one stash list. Don't `git stash pop` in a worktree
  while other cards build: pop by name (`git stash pop 'stash@{N}'`) or commit instead.

## The stuck-session monitor (6 Nov 2026, card #99, stack A)

Built on the branch `dsh-99` in the worktree `~\kanban-app-99`, from
`dsh-agents` at #96's commit.

- **`monitor.rs`:** pure checks over a `View` (one card's session) and its tail's `Call`s
  (`calls` reuses `transcript::parse_line` and `pair`): `check` returns a `Hit` (kind, reason,
  message, `about`); `step` turns the card's `stuck` field and a hit into the next field and the
  `Action`s (Log, Nudge, Queue, Notify). `check_all` gathers the cards under the board lock (live
  sessions, `needs.rs` failures, cards that carry `stuck`), reads files with no lock (`look`), and
  `act`s. `start` runs it on its own thread, woken every 30 s, so the 400 ms typing loop never
  waits on a file and a changed setting applies without a restart.
- **Hooks unchanged** (the router asked for this to be recorded): the transcript holds every call
  and its output; widening PreToolUse/PostToolUse would add a round trip to every tool call, and
  sessions already running would keep their old hooks.
- **Repeat:** `calls` parses the tail itself (not `transcript::parse_line`, whose key input and
  4 KB output are too lossy to compare: an Edit keeps only its first lines). It keeps each call's
  whole input JSON (less its `description`, `LABELS`: #119's re-test had 40 identical `echo`s, each described anew) and whole result, and starts again at a prompt, a mid-turn message
  (`queued_command`, the monitor's nudge too) or a notice. Five running with the same name,
  input, result and error flag; polls (`POLLS`, the Monitor tool, a `cd`) skipped, neither
  counting nor breaking the run; none while the latest call is unfinished. Reads `TAIL` =
  256 KB, not 64 KB: Claude Code stores each result twice in its line (the message and
  `toolUseResult`).
- **Dead wait:** `waiting_since` is the earliest `at` of the not-passed `RESET_ON_IN_PROGRESS`
  records; `subagent_state` takes `transcript::subagents` that are unfinished, not
  `stoppedByUser` (new `Subagent::stopped`), started after it and quiet for the interval.
  Without that evidence, a session idle after asking Hugh something isn't messaged.
- **Idle** (6 Nov, after the ping-pong test's cards #187 and #188 sat idle 30 minutes unflagged):
  idle past the interval, the column in `IDLE_COLUMNS` (In progress, Planning), nothing running
  or queued, no `blockedReason` (`View::blocked`). A dead wait outranks it. `about` is
  `idle:<column>`, so the session's own reply to the message clears the chip and its next idle
  stretch in that column goes through `Memory` straight to the Router or Hugh: no message loop.
  The message (`idle_text`) goes through `store.queue`, like a dead wait's.
- **Memory:** `Memory::nudged` remembers, per card, each thing already nudged (`Hit::about`: the
  repeated call, or the dead subagent's id), so the same thing back after a clear goes to Hugh,
  not another message. It is in memory only.
- **The `stuck` field:** `{kind, reason, since, about, nudged, typed, notified}`, written by "the
  app (monitor)" only when it changes (`write_stuck`), removed on a clear. One episode is one kind
  and one `about` (an FNV tag of the repeated call, or the dead subagent's id): a different one is
  a new episode with its own nudge. Within an episode the reason stays as first written, and no
  reason holds a running time, so a long wait isn't a write at every check. A nudge that couldn't
  be typed sets `typed` false, and the next check tells Hugh. The kinds: failed, question
  (`Watch::question`), permission (any other "permission" state), repeating, quiet, dead wait, idle. A reason the store calls
  secret-looking is written as "May be stuck (kind)", and an input that looks secret is never
  quoted. Sessions can't write it (`tools::UPDATABLE`).
- **Sessions:** `Sessions::watch` and `watch_one` copy each live session under one lock (`Watch`);
  `fail_error` keeps a StopFailure's `error` for the "usage limit" reason. The nudge goes through
  `type_now(.., mid_turn_ok = true)` after `still_repeating` re-reads the tail; `takes_typing`
  refuses a question as before. The dead-wait message goes through `store.queue`.
- **Setting:** `sessions.stuck_check_minutes` (`STUCK_CHECK_KEY`, default 10, 0 to 1440).
- **UI:** `ui/board.js` `stuckChip` and `stuckRow`; `ui/board.css` `.chip.stuck`;
  `ui/settings.js` the box under the idle pause.
- **Secrets:** every input the monitor quotes goes through `mask` (a known token prefix, a long
  key-like run, the word after Bearer, Basic, Authorization: or -u, a sign-in link, a token, key or
  password value; then `kanban_store::mask_secrets`), and is dropped if the store still calls it
  secret.
- **At the merge, two soft dependencies** (neither branch is on dsh-99, so neither is called):
  - #109 ("Blocked by"): check f, a circle of waits. Add a kind "circle" from
    `Store::wait_cycles()`, chip only, on each card in a cycle, and a wait on a card archived from a
    column other than Done or Deploy complete (never met) as "stuck wait", chip only.
  - #115 (the test-app queue): skip the dead-wait check for a card whose id is in
    `Store::test_app_queue().line`: it is idle on purpose, waiting for its turn.
- **Tests:** `monitor.rs` (each check, the polls, same call with another output, secrets, the
  steps, the retry, the memory, the tail, the field) and `crates/store/tests/settings.rs`
  (`the_stuck_check_is_ten_minutes_a_whole_number_and_can_be_off`).

## Stepping through Human test (6 Nov 2026, card #102, branch `dsh-102`)

- `ui/review.js` owns it; `panel.js` has hooks only. The queue (`rq`) is fixed when Hugh opens a
  Human test card (the column as drawn: filter, `pinFirst`, `byOrder`), because the board re-sorts
  on every write. `rqSync` drops leavers and appends arrivals; a close by Hugh, or opening a card
  outside the queue, ends it. `openPanel(id, {step:true})` switches card without the close path.
- A stack is one stop: the lead's panel, whose Pass waits on `htStackHold` (a child in Human
  test not passed, or one still being built, by stacks.js `kidState`). Card #116 takes the stop
  over by setting `reviewStackStop = lead => { open the overlay; return true }`. Also for #116:
  `reviewShip(leadId)` (its own confirm starts the stack's pending Pass; the banner sits inside
  the open overlay), `reviewStartAt(id)`, `reviewStepFrom(id, d)`, `reviewBackToStack(lead, kid)`, `reviewQueue()`,
  and `htKeys.add(target)` to reuse the key handler inside the overlay. The handler stays off for
  any other dialog, the Detach confirm included.
- Pass: P then Enter for every card. A card or a stack waits `PASS_UNDO_MS` (5 s) in `htPending`,
  then `move_card` with the current version (one retry on a conflict); a stack's hold is checked
  again then. A child is `set_passed`.
- Fail: `human_test_fail` (session_cmds.rs; rules in `dispatch::fail_plan`). An ordinary card goes
  back by #93's rule; a lead by `problem_back`, shared with `stack_problem`; a child's Fail is
  `stack_problem`. The move comes before the "Failed" log line; a note the session can't take is
  `sendError` in the reply, not an error, as the card has moved. Merge point for #112: `repro::mark_needed(&mut store, &card,
  "human-test-fail", &note)` where the comment in `human_test_fail` says.
- The Human test box calls `evidenceHtml(c)` (#101), `changesHtml(c)` (#108) and
  `showEvidence(root)` when they exist. It is drawn by wrapping `renderDetail`, so board.js is
  untouched.
- Checked in a harness (the real `ui/` in headless Edge with a stand-in backend and dry-run
  cards), not yet in the test app: the router installs it once per merge wave.

## Time to check on Human test cards (6 Nov 2026, card #103, branch dsh-103)

- **The record:** the card field `checkSize` (`kanban_store::CHECK_SIZE_KEY`): `{class: "quick"|"real",
  why, reasons[], diff: {from, files, added, removed, kinds: {kind: {files, lines}}, paths: [{path,
  added, removed, kind}] (first 30), more} | null, login, seed: {check} | null, at}`, written as
  `check_size::BY` ("the app (diff check)") with a log line "Time to check: ...". Kinds: tests,
  behaviour, docs, signin, data, build, code, ui. Card #104 reads it. No migration; not in
  `tools.rs` `UPDATABLE`.
- **When:** `cards.rs` `apply` removes every key in `RESET_ON_HUMAN_TEST` (just `checkSize`) on a
  move into Human test, unless the patch sets it, so every way in is covered.
  `Store::check_size_due` (`crates/store/src/check_size.rs`) lists Human test cards with none
  (archived cards and stack children skipped). `dispatch::run` calls `check_size::pump` every 3 s:
  it reads the jobs under the board lock (worktree, branch, numbers with the stack's children, the
  login reasons from the card and its log, `route.check`), then runs git on its own thread with the
  lock let go, one batch at a time (`RUNNING`, cleared by a drop guard even on a panic). `write`
  writes only over the version it read, and only while the card is still in Human test with no
  `checkSize` (`Wrote::Stale` otherwise: tried again next round). A store failure waits
  `RETRY_AFTER` (60 s, `tried`); a git failure or timeout (`Reading.transient`) is tried again
  after it, and written as "couldn't read" only on the `TRANSIENT_TRIES`th (3rd) failure. Every
  due card is listed before the wait filter, so waiting cards don't crowd out new ones.
  `clear_changed_stacks` (via `Store::check_size_leads`) removes a lead's record when its stack's
  numbers differ from the record's `cards`. The command `check_size_again` removes it (Hugh's
  "Work it out again").
- **Git** (`check_size::read`): `git -c core.fsmonitor=false -c core.quotepath=off -C <worktree>`,
  `GIT_OPTIONAL_LOCKS=0`, no window, each call stopped after `GIT_LIMIT` (20 s, `run_limited`).
  Never a WSL checkout (`stack::in_wsl`). The branch is the checkout's HEAD, as `stack.rs` reads
  it; a different card `branch` is a reason. Every revision list ends with `--`. On main or
  master: `marked_commits`, `git log -E --grep '\(card #N[) ,]'` (`card_grep`) with `--numstat`,
  summed per file (`parse_numstat`); uncommitted work isn't checked there (the checkout is
  shared). On a branch: the marked commits in `<merge-base>..<branch>`, else
  `git diff --numstat <merge-base> <branch> --`, where the merge-base is that of whichever of
  main, origin/main, master, origin/master is newest (`nearest_base`); plus
  `git status --porcelain --untracked-files=no`.
- **The rule:** `kind_of` (first match: tests, behaviour, docs, signin, data, build, script code
  (JS, TS and HTML), UI (CSS, SVG, images), code) and `verdict`. Docs are `.md`, `.mdx`, `.rst`,
  `.adoc`, LICENSE and CHANGELOG, plus `.txt` and images under docs/, vault/ or design/; any
  other `.txt` (requirements.txt) is code or build. Sign-in is by whole words (`signin_path`:
  "author" and design "tokens" aren't sign-in); the limits are `UI_LINES`, `UI_FILES`, `DOCS_LINES`. A `route.check`
  (card #98) of 30 min+ makes it real; any other guess is only a reason. `route.size` isn't read.
  The end-to-end check of the seed waits for #98's merge; it has unit tests only.
- **Hidden text:** a path the store's secret filter matches becomes `HIDDEN` in `parse_numstat`,
  and `record` hides any other matching string; a write still refused is retried with
  `minimal_record` (real, "details hidden").
- **UI:** `ui/check-size.js` and `ui/check-size.css` (the script adds the stylesheet); one script
  line in `index.html`; board.js and board.css untouched. Every global starts with `checkSize`.
  It registers in card #107's `boardHooks` (a top-level `const` in board.js, merged from
  `dsh-107` commit 069a64e): a filter for `is:quick` / `is:needs-testing` per term, the chip in
  `decorate`, and `checkSizeBox` (HTML) in `details`. Without it, it wraps `matches`, `cardEl` and
  `renderDetail`. It always wraps `render` for the header button, which uses `qToggle`. A stack's
  lead gets the chip on its `.stk-sub` line; a child gets none. `checkSizeText(card)` gives the
  chip's words for #107's "To test" line.
- **Tests:** `crates/store/tests/check_size.rs`; `check_size.rs`'s unit tests, including git on a
  temporary repo (`reads_a_real_repo`, slow when git is: about 2 s a call on a busy machine).

## The card-reproducer (6 Nov 2026, card #112)

Built on the branch `dsh-112` (worktree `~\kanban-app-112`), from `dsh-agents` at
#96's `fab28a1`. It adds one line to each of #96's tables and keeps its own code in `repro.rs`.

- **`repro.rs`:** `AGENT` (in `agents::AGENTS`, model and effort inherit, `OnStop::Custom(on_stop)`)
  and `gate` (first in `agents::GATES`). The card field is `repro` (`kanban_store::REPRO_KEY`):
  `{state, at, agentId, detail, reports, test, before, after, earlier}`; state is `needed`,
  `reproduced`, `not-reproduced`, `fixed` or `unclear`; `kanban_store::repro_open` says whether a
  round is open. It is not in `RESET_ON_IN_PROGRESS`: a move into In progress keeps it, and
  sessions can't write it (`kanban_update_card` doesn't allow the field).
- **Triggers, one door:** `mark_needed(store, card, source, note) -> Result<(Card, bool), String>`.
  Sources: `found-a-problem` (`main.rs` `stack_problem`), `report-a-bug` (`session_cmds.rs`
  `session_send` with `bug: true`, only on an In progress card that isn't a stack's child:
  `report_refused`), `card-tester` (`agents::record_check`, a FAIL whose lines include `- BUG:`,
  Markdown allowed, `bug_lines`), and `human-test-fail`, which card #102's Fail calls (the
  router's call: in Human test, Fail is the bug report, not Report a bug). Keep that signature.
  The card-reviewer's BUG lines aren't a trigger (code review's finding, card #112): it reads the
  diff, not the installed app (since card #162 it runs beside the tester, after the install), so
  the session fixes its finding in the review-and-test loop. `mark_needed` logs what happens
  to the report, not the note: each caller logs the note itself ("Hugh found a problem", the FAIL
  line, "Hugh reported a bug", #102's "Failed in Human test"). With the rule off it only logs.
  `needed_record`: one open round per card (needed, reproduced or unclear); a report in an open
  round joins its `reports` (last 20). Any `before`, `after` and `test` move to `earlier`
  (`archive`, the last 5 rounds), so the next first run covers every open report and no proof is
  lost. A closed round (fixed, not-reproduced) starts anew.
- **The report:** `parse_report` reads `CARD:`, `VERDICT: REPRODUCED | NOT REPRODUCED | FIXED |
  UNCLEAR` (separators `_`/`-` allowed) with `- ` lines, `STEPS:` (numbered lines), `SHOT:` (a
  path, Markdown image or `file:///`; `none`) and `TEST:`. `record_of`: the first verdict of a cycle
  fills `before`, later ones `after`. FIXED with no `before`, NOT REPRODUCED on a second run, no
  verdict, a verdict with no tool calls, or UNCLEAR (the prompt's "the test app isn't on the fix")
  are `unclear`, which keeps `before`. A run on a closed round starts again (reports kept, the
  old proof to `earlier`). With no round open, a run that proves nothing returns no record (only
  a log line), so a stray run can't make a hold.
- **Shots:** `copy_shot` copies to `<data>\repro\<card id>\<at>-<agentId>.<ext>` after
  `local_image` (`session_cmds.rs`, now shared with `image_data_url`): an image extension, a
  full local path, not UNC, a file within `MAX_ATTACHMENT`. `on_stop` takes the board lock to find
  the card, copies the shot with the lock released, then locks again and reads the card afresh. A
  refused shot is logged and the verdict still recorded. A lead whose round closes reruns its
  stack's rules (`dispatch::after_lead_verdict`).
- **The hold:** `gate`, like the others: from any column before Human test into Human test or
  Deploy complete, while `repro` is needed, reproduced or unclear, under
  `sessions.repro_before_human_test` (`REPRO_RULE_KEY`, in `RULE_KEYS` and `PAGE_SETTINGS`, on by
  default). Its refusal and `ask_note` share the steps text (`steps`, written for a session).
  `stacks.rs` `stack_recheck` holds a ready stack while its lead's own round is open
  ("reproducer proof"), under the same rule.
- **A stack's card (whole-build review S8, the router's decision):** `mark_needed` doesn't set
  `repro` on a card with `stackOf` (it logs that the lead proves it), and `gate` never holds one. A
  child doesn't install the test app, so it couldn't prove a fix. `stack_problem` adds
  `child_note` to the message (own-build child: fix, no install, steps in the hand-over; lead-built
  child: the lead runs the reproducer with `CARD: #<child>` after its install) and, for an
  own-build child, logs on the lead that the bug is for its card-reproducer. The lead's run on the
  child is recorded on the child and holds nothing.
- **Old builds (S4):** in-progress.md's "Prove a reported bug first" starts "Only if
  `kanban_get_card` shows a `repro` field", so a session on an older live build skips it.
- **Masking:** the report notes, steps, notes, test name, a refused shot's path and Hugh's
  "reported a bug" line go through `kanban_store::mask_secrets` before they are stored, so a
  sign-in link or token in a step can't make the store refuse the whole record.
- **UI:** `ui/board.js` `CHECKS` has a `repro` entry (its own chip words and `labels`, and the
  Blocked column); `checkRows` takes an entry's `labels`; `reproBox` draws the proof (`reproRun`
  for before and after, and the `earlier` rounds in a `<details>`), with shots through panel.js
  `shotImg`/`showShots` and `openShot` on a click. `ui/panel.js` `post(bug)` behind Send and
  **Report a bug** (`#p-bug`, shown only on an In progress card not in a stack).
  `ui/settings.js` `SUBAGENT_RULES` has the rule. Styles in `ui/board.css`.
- **Prompts:** `in-progress.md` "Prove a reported bug first" (before the review) and the
  hand-over line; `kanban-app.md` 1a and the Human test hand-over; `BACK_FROM_TEST` says a message
  that reports a bug may still be proved, else log `Not a bug report: <why>`. All conditional on
  the session having the subagent.
- **Tests:** `repro.rs` (the report, rounds, earlier rounds, before and after, what isn't proof, a
  stray run, the hold and the rule, `mark_needed`, the sources, Report a bug's columns and
  message, BUG lines with Markdown, the shot checks, the prompt's commit check, agreement with
  `repro_open`); `agents.rs` `find`; `sessions.rs` (the reproducer passed, effort inherit; the
  command line under 30,000); `tools.rs` (sessions can't write `test`, `review` or `repro`);
  `dispatch.rs` (`BACK_FROM_TEST`'s line); `crates/store/tests/auto.rs` (the rule, and a move
  into In progress keeps `repro`); `crates/store/tests/stacks.rs`
  (`a_ready_stack_waits_for_the_leads_bug_to_be_proved`).
- **Known limits:** the second run's commit check rests on the prompt (the app can't see the test
  app's commit from the hook). A session that never runs the reproducer is held, as with the
  other gates; Hugh can switch the rule off.

## The merger at ship (6 Nov 2026, card #100, stack A)

Built on the branch `dsh-100` in the worktree `~\kanban-app-100`, from
`dsh-agents` (card #96's foundation).

**Card #162 took the ship job away** (below, "The merge lane everywhere"): every ship now merges in
its repo's merge lane. The card-merger keeps its "stack" job (and the Router's "integrate");
its prompt refuses a ship, and its report on a card in Ship it is not recorded. What follows is
how it was built.

- **`merger.rs`:** `AGENT` in `agents.rs` `AGENTS` (`edits: true`, the first agent that may edit:
  it resolves conflicts; `also_denied` empty; effort `high`, the router's call, so a stack lead's
  low-effort session still merges at high). No `GATES` entry: there is no hold, because the push
  comes before any app call could hold it. The one-ship-per-repo lane (card #79) is unchanged.
- **Two jobs:** "ship" (the default: the card in Ship it, steps 1 to 11) and "stack" (card #113:
  a stack child in Human test whose test and review are met: passed, or that subagent switched
  off for the card, as the app's own checks count it (card #176); in the lead's worktree it detaches at
  the child's head SHA, rebases onto the stack's branch and fast-forwards that branch; never the
  child's own branch, never main, never a push). A stack child's recorded merge then calls
  `merger::after_child_merge`, empty here: when #113's branch is merged it becomes the one call
  `crate::dispatch::after_child_merge(app, store, child_id)`.
- **Why not a deny rule (the live trial, Claude Code 2.1.318):** a rule with a specifier in an
  agent's `disallowedTools`, like `Bash(git push:*)`, removes that **whole tool** from the agent.
  With the Bash rules the trial agent pushed through PowerShell; with rules for both shells it had
  no shell. #96 then dropped its common `Bash(kb:*)` rules (627f179): the deny lists name whole
  tools only.
- **The shell guard, `guard.rs` (the router's decision: for all the app's subagents):** every
  card session's settings get a second `PreToolUse` entry, matcher `Bash|PowerShell`
  (`guard::SHELL_TOOLS`), an `http` hook to `/hook/<token>/<row>/guard`. `api.rs` answers it with
  `guard::guard`. `refusal` acts only when the hook's `agent_type` is one of `agents::AGENTS` and
  the tool a shell. For every app subagent it refuses `kb`/`kb.exe` (any command) and
  `claude config`/`claude mcp` (`board_or_config_in`); for the card-merger also its pushes
  (`merger::push_in`: git's subcommand `push` past its options, `-C <dir>`, `-c <k=v>`,
  `--git-dir`; `gh api`; `gh pr merge` past `-R <repo>`). How a line is read: `commands` splits
  it at `;`, `&`, `|`, brackets, backticks and new lines outside quotes; a quoted string with
  spaces is read as a command of its own too (`sh -c "..."`, `powershell -Command "..."`);
  `invocation` takes a command's program as its first word that isn't `X=1`, a wrapper (`env`,
  `cmd /c`, `sh`, `xargs`, ...) or a wrapper's flag, so `ls crates/kb` and a commit message that
  says "push" are fine. A refusal replies `permissionDecision: deny` with the reason, and logs
  "Refused <what> by the <agent> subagent: <command>" on the session's card (the first 200
  characters, or only its kind when it may hold a secret, `shown`); else `{}`. It takes no lock
  for other calls. The trial with an http stand-in refused all five push forms and let
  `git status` run. A shell can still run things in ways it doesn't read (an alias, a script), so
  `configwatch.rs` and the merger's ORIGIN check stay.
- **The report:** `OnStop::Custom(on_stop)`. `parse_report` reads `CARD` and `VERDICT` with the
  shared `parse_verdict`, the last `HEAD: <sha>` and `ORIGIN: <before> <after>` (SHAs or `none`,
  an arrow allowed), and the `- ` lines after the VERDICT line (what it changed, or what failed).
  `record_of` makes the `merge` field `{state, at, agentId, detail, head, origin: {before,
  after}}`. A PASS counts only with tools run, a HEAD and an ORIGIN line, and origin unmoved;
  else it is `unclear`. origin moved (whatever the verdict) also emits `notify` to Hugh.
  `target_card` limits it to the session's card or one in its stack.
- **Store:** `kanban_store::MERGE_KEY`. `cards.rs` `apply` removes `merge` on every move into
  Ship it (unless the patch sets it), so each ship starts with none. `merge` isn't in `tools.rs`
  `UPDATABLE`, so a session can't write it.
- **UI:** one `CHECKS` entry in `ui/board.js` (`merge`, row "Merger", chips Merged / Merge failed
  / Merge unclear, on Ship it and Deploy complete).
- **Prompts:** `ship-it.md` 1a (brief it; on PASS check HEAD and ORIGIN, push with
  `--force-with-lease`, keep 2b's CONFLICTING check after the push and rerun the merger if it
  trips; on FAIL `needs-hugh` and stop). `kanban-app.md` Ship it 1a (main or a stack branch; the
  `(card #n)` check; fast-forward main with `--ff-only`). Both conditional on the session having
  the subagent.
- **Tests:** `merger.rs` (the report, a real report from the trial, every unclear case, the deny
  list and that it may edit, `push_in`, the prompt's lines, both prompts have the step),
  `guard.rs` (every app subagent refused kb and claude config in both shells, their own work
  runs, the program word, the merger's pushes, no secret on the card),
  `crates/store/tests/merge.rs` (the clear on entry to Ship it), `sessions.rs` (card sessions get
  it; the guard hook entry).

## One merge lane per repo (9 Nov 2026, card #160)

**Card #162 made it platform-wide** (next section): no list, no card-merger at ship. What
follows is card #160 as built; the next section says what changed.

Built on the branch `card-160` in its worktree. A repo Hugh lists ships through one **merge
lane**: a Claude Code session the app owns for that repo, which does the ship's merge for every
card of the repo, one at a time, and keeps its conversation across ships. It resolves a conflict
with what it learnt merging the earlier cards. Every other repo keeps the per-card card-merger
(above), unchanged. The card's own session still pushes and ships.

- **The setting:** `sessions.merge_lane_repos` (`crates/store/src/lanes.rs`
  `MERGE_LANE_REPOS_KEY`), a list of `{repo, checkout}`, empty by default. `check_merge_lane_repos`
  checks it as the own-worktree list is checked (`config::check_repo_checkout`, now shared): repo
  `owner/name` and listed once, checkout an absolute path to a git checkout, no token.
  `merge_lane_repos()` reads it (a saved value that no longer parses counts as none);
  `merge_lane_repo_for(repo)` finds a card's entry. `lanes.model` and `lanes.effort`
  (`lane_model_effort()`; default Opus 5.5, effort high, as the merger runs). All three are page
  settings. The card's own merger switch (card #151) still decides: off means no merger and no
  lane.
- **`src-tauri/src/lane.rs`:** the lane. Built like the Router (card #119): key `lane:<repo>`
  (repo lower case, `kanban_store::lane_key`) in the live session map; not a card;
  `sessions::is_app_key` (the Router or a lane) replaces each place that skipped the Router's key
  (the cap, the idle pause, the crash notice, the start queue, `control.rs`, `notify.rs`), so a
  lane sits outside the cap and is never paused. `plan` gives it the main account (a resume
  needs the account it ran on), `bypassPermissions`, and `Plan.lane`: `sessions.rs` then passes
  `prompts/merge-lane.md` as `--append-system-prompt` (its rules survive every compaction), no
  `--agents`, `--disallowedTools AskUserQuestion`, and the guard matcher `ROUTER_TOOLS` (shells and
  the edit tools).
  - `start` makes or finds its worktree (`worktree::make_lane`, 15 s cap, no board lock held),
    then resumes the repo's last conversation on this channel (`latest_lane_session`, when its
    transcript and folder are still there) or starts a new one, and records the start in
    `lane_sessions`. The worktree is `<checkout>\.claude\worktrees\merge-lane`
    (`<channel>-merge-lane` on a test board, `worktree::lane_path`), detached at `origin/main`
    (else `main`), never a branch, and kept between jobs so its `target` stays warm.
  - `request` is `kanban_merge`. `merge_refused` (pure) refuses it, saying why, when the card is
    archived or not in Ship it, its ship hasn't started, its repo isn't listed, the merger is off,
    it has no `branch` or `worktreePath`, or a merge already waits or runs. Then, with the board
    let go, git checks the card's worktree is clean and reads its HEAD (the `base`). It writes
    `merge` = `{state: "queued", lane, base, branch, worktree, branches?, rules?, checks?, at, by}`
    and replies "The merge lane has it ... End your turn". `rules` and `checks` are at most 4000
    characters and refused if they look like a secret.
  - `tick` runs each dispatcher pass (`dispatch.rs`). With nothing waiting, no lane running and
    nothing in memory it costs one COUNT (`lane_jobs_waiting`). Else it gathers the `Facts` for
    each lane and acts on `decide`'s one `Action`: `Start` a lane for a waiting job (on its own
    thread, at most `MAX_STARTS` = 2 tries, then unclear), `Brief` it when it is ready and idle
    (`merge.state` "running", `startedAt`), `Type` a waiting note, `Cancel` a job whose card left
    Ship it (state "cancelled", and the lane is told to drop it), `Resume` a lane that ended during
    its job (once; it gets `resume_note` and the job again), `TimeOut` after 60 min with no report
    (unclear), `GiveUp` (unclear: the repo left the list, a lane that ended twice or never
    started), and `Stop` a lane idle 15 min with no job (its conversation is kept).
  - `brief`: the card, title, branch, base, the card's worktree (read only for the lane), the
    other branches, the rules, the checks, and the repo's last 5 finished merges
    (`recent_merges`, from the cards' `merge` records: card, title, state, head, detail). So the
    history survives a compaction or a new conversation. A dry-run card's brief starts with the
    dry-run line.
  - `record_result` is `kanban_merge_result` (only for the lane's running job). It is recorded
    with `merger::record_of`, as the card-merger's report, by "the app (merge lane)". A PASS also
    needs `onto`, and `head` on top of it (`worktree::on_top_of`, `git merge-base --is-ancestor`),
    else it is unclear. origin moved, or unclear, also tells Hugh. `finish` then wakes the card's
    session (`stackrun::wake`) with `verdict_message`: on a PASS, check the worktree is clean and
    HEAD is still `base`, `git reset --hard <head>`, check `git range-diff <onto> <base> <head>`,
    then go on as 1a's PASS; else `needs-hugh` and stop.
  - `lane_note` (the Ship it message, beside `subagents_note`, `dispatch.rs`): for a listed repo
    with the merger on, "don't run the card-merger subagent ... call kanban_merge ... end your
    turn".
  - `lane_status` / `lane_stop` (Tauri commands, `main.rs`). `status` gives each listed repo's
    `{repo, checkout, worktree, state, card, model, effort, live}`; `state_word` (pure) says
    `off`, `starting`, `idle`, `merging` or `down`. `stop` is Hugh's Stop: the conversation is
    kept and a job it was on is resumed once. Every change emits `lanes-changed`.
- **`src-tauri/src/lanetools.rs`:** a lane's MCP tools and instructions (`api.rs` routes a lane's
  row by `lane::key_of_row`): `kanban_get_card` and `kanban_stack_list` (by number),
  `kanban_add_log` (only on its job's card) and `kanban_merge_result` (only for its job). Nothing
  else. `kanban_merge` is in `tools::CARD_SESSION_ONLY`: only a card's own session is offered it,
  not the Router (`routertools::NOT_FOR_ROUTER`), a lane, `kb` or an outside session.
- **The guard:** `guard::lane_refusal`. An Edit, Write, NotebookEdit or MultiEdit outside its
  worktree is refused. In a shell: what the Router may never run (`router_shell_refusal` with no
  install at all: any push, `gh pr merge`, `gh api`, `kb`, `claude config` / `mcp`,
  `install.ps1`, a stop of the app, the board database), plus any move of a branch or a worktree
  (`moves_a_branch`: `git update-ref`, `git switch`, `git branch -f/-d/-D/-m/-c`, `git checkout
  -b/-B/--orphan`, `git worktree remove/move/prune`). Each refusal is logged on its job's card
  ("Refused ... by the merge lane").
- **The store:** migration 16 (`schema16.sql`), the append-only `lane_sessions` (seq, id, repo,
  claude_session_id, channel, account, cwd, model, effort, started_at; triggers refuse updates and
  deletes; backup first), with `add_lane_session`, `latest_lane_session(repo, channel)`,
  `is_lane_row` and `lane_of_row`. Rows carry their channel, so a test board never resumes
  another channel's lane. A lane's jobs are the cards' `merge` records (`MERGE_STATES`: queued,
  running, passed, failed, unclear, cancelled), so an app restart loses nothing: `lane_jobs`,
  `lane_jobs_all`, `lane_jobs_waiting`. A board copy (`-FreshCopy`) cancels the source's waiting
  jobs (`detach_lane_jobs`). `merge` stays out of `UPDATABLE`. The feeds count "the app (merge
  lane)" and "the merge lane" as the merger.
- **UI:** Settings › Sessions, "Ship through one merge lane per repo" (`ui/settings.js`
  `mergeLaneBox`), on the same `{repo, checkout}` list editor as the own-worktree box
  (`repoListBox`), with its last 5 changes. Under the list: the lanes' model and effort (saved
  at once) and each listed repo's lane, its state chip (Off, Starting, Idle, Merging #n, Down),
  model and effort, and Stop (only while it runs; a confirm while it merges). It polls
  `lane_status` every 3 s while the section is open and redraws on `lanes-changed`; closing
  Settings or changing section stops both. `ui/board.js` `CHECKS` `merge` adds the chips
  "Waiting for the lane" (queued), "Merging" (running) and "Merge cancelled" (cancelled); with
  `merge.lane` set its hover title starts "In the merge lane for <repo>".
- **Prompts:** `prompts/merge-lane.md` (the lane's rules and the ship job, rewritten for a
  detached worktree: clean, fetch, record onto and originBefore, `git checkout --detach <base>`,
  `git rebase <onto>`, the other branches on top by SHA, resolve, the `(card #n)` check, the
  checks, fetch again, report). `ship-it.md` 1a and `kanban-app.md` Ship it 1a: "If the app's
  note says this repo ships through its merge lane", call `kanban_merge` and end the turn; on a
  PASS the checks, reset and range-diff above, then the push as before. The owner's private
  prompts get the same on their own branch.
- **Tests:** `lane.rs` (`decide` for each action, `merge_refused`, the brief, the verdict
  message, `state_word`, `job_of`, the note, the plan, the rules), `lanetools.rs` (four tools
  only, logs only on its job's card), `guard.rs`
  `the_lane_guard_refuses_pushes_kb_installs_branch_moves_and_edits_outside` (and its own work
  runs), `worktree.rs` (per channel; detached, found again, no branch), `sessions.rs` (the
  command line), `dispatch.rs` (the Ship it note), `tools.rs` and `api.rs` (`kanban_merge` for
  card sessions only), `merger.rs` (both ship prompts have the lane step; `merge-lane.md` never
  pushes and works detached), `crates/store/tests/lanes.rs` (append-only, the backup, an older
  build opens the board, the list's checks, model and effort, the jobs query, the history, the
  copy, the merge record isn't a session's), `tools/ui-tests/lane.test.mjs` (the chips, the
  Settings box, Stop, the shared editor).
- **Known limits:** no lane panel: Hugh sees the lane's steps in the card's log, its chip, and
  its state in Settings; opening its chat and terminal is card #129's Running now list. The
  stack job (a child onto its stack's branch) stays with the lead's card-merger: a stack ships
  once, on the lead, through the repo's lane. The lane's history is its conversation plus the
  last 5 merges in each brief; a new conversation (its transcript gone) starts from the brief
  alone. A lane takes one job at a time. There is no button to start a fresh conversation.

## The merge lane everywhere (9 Nov 2026, card #162)

Hugh: "This doesn't need to be a setting ... just platform wide." Every card with a branch, on
every repo, ships through its repo's merge lane. Card #160's list and the per-card card-merger
at ship are gone.

- **No setting.** `sessions.merge_lane_repos` is no page setting now (`config.rs`
  `PAGE_SETTINGS`; `check_merge_lane_repos`, `merge_lane_repos`, `merge_lane_repo_for` and
  `MERGE_LANE_REPOS_KEY` are gone). A board where #160's build saved a list keeps the value
  (nothing deletes it; an older build still reads it); this build never reads it.
  `lanes.model` and `lanes.effort` stay.
- **The lane's place, found at `kanban_merge`** (`lane::find_checkout`, with the board let go):
  the first that works of (1) the main checkout of the card's `worktreePath`
  (`git rev-parse --git-common-dir`, its parent, when it is a `.git`), (2) the card's repo's
  checkout on the own-worktree list (`own_worktree_repo_for`), (3) its `folder` in `repos.json`
  (`private::folder_for`; never the home folder). Each must be a git checkout whose `origin`
  names a GitHub `owner/name` (`lane::github_repo`: https, ssh and scp forms; a token in the URL
  is never kept). The checkout is canonicalised (`clean_path`). None: refused with every place and
  why, ending "Report needs-hugh with this reason".
- **Keyed by the origin repo.** The job's lane is `lane_key(<origin owner/name>)`, lower case,
  so a `hughht5/kanban-app` card built in the claude-kanban-dsh checkout shares that repo's lane
  and its one lane worktree. The job record gains `checkout` (`merge.checkout`), so a restart
  needn't look again; `tick` takes the lane's `MergeLaneRepo` (repo from the key, checkout from
  the job). A job with no checkout (asked for by #160's build) is given up as unclear.
- **Two lanes never share a worktree.** The lane worktree is
  `worktree::lane_path(checkout, channel)` as before (per channel, so a test app's lanes stay
  apart from live's). `start` refuses when another lane key runs in that path, and resumes the
  last conversation only if it ran in this path. If two checkouts of one repo exist, a lane free
  in the other checkout's worktree is stopped (`Facts.elsewhere`, `Action::Stop`) and the next
  pass starts it in the job's checkout. The brief's history is the lane's finished merges
  (`merge.lane`) plus the card's repo's older ones (`recent_merges(key, repo, ...)`).
- **`lane_note`** goes on every Ship it message for a card with a `branch` (any repo; an old
  card's `merger: false` changes nothing). A card with no branch hears nothing: the ship prompt's
  no-branch path. `merge_refused` lost its "not listed" and "merger off" refusals; its no-branch
  refusal says there is nothing to merge.
- **The card-merger leaves the ship.** `subagents.rs`: no `merger` row (five subagents), no
  `sessions.merger_at_ship` (`MERGER_RULE_KEY` gone; a saved value is ignored). An old card's
  stored `merger` is ignored (`subagent_on` says off, `subagents_of` leaves it out); a choice that
  names `merger` (an older page) has it dropped, not refused (`subagents::RETIRED`). The card's
  details and Add card show five. `merger.rs` keeps the stack job and the Router's integrate
  job: its description and prompt no longer speak of ship time, the prompt refuses a ship, and
  `on_stop` doesn't record a report on a card in Ship it or with a lane job waiting or running
  (`not_a_ship`), so it can't overwrite a lane job. `merger::record_of` still records the lane's
  verdicts.
- **Prompts:** `ship-it.md` 1a and `kanban-app.md` Ship it 1a are the lane path only: commit,
  `kanban_merge`, end the turn; on PASS check, reset and range-diff, then push (or fast-forward
  main); on FAIL `needs-hugh`. `kanban_merge` refusing for no checkout is `needs-hugh`. 2b's
  CONFLICTING goes back to the lane. `ship-it.md` has a no-branch path (skip 1a and 2, prove as
  6c). `router.md` job 5 says the lane rebases, never the card-merger for a ship.
- **UI:** Settings › Sessions' "Merge lanes" box: the help, the lanes' model and effort, and each
  lane that has a job, runs, or ran on this channel (`Store::lanes_seen`, `lane::lanes_listed`)
  with its state and Stop. No list editor (the own-worktree box keeps `repoListBox`).
- **Tests:** `lane.rs` (`the_lanes_checkout_is_found_in_order_or_refused_saying_where_it_looked`
  and `a_lane_is_keyed_by_its_origin_so_an_alias_shares_it` on scratch repos, `github_repo`,
  `checkout_of_lane_path`, `lanes_listed`, the note for every repo, a lane elsewhere stops, a job
  with no checkout), `dispatch.rs` `every_ship_with_a_branch_is_told_to_use_its_merge_lane`,
  `merger.rs` `both_ship_prompts_merge_only_through_the_lane` and `the_merger_does_no_ship`,
  `prompt_rules.rs`, `crates/store/tests/lanes.rs` (the old list kept and ignored,
  `lanes_seen`, the history by lane), `crates/store/tests/subagents.rs`
  (`the_card_mergers_switch_and_choice_are_gone_and_ignored`), `subagents.rs`,
  `tools/ui-tests/lane.test.mjs`.

## Stacks build in parallel (6 Nov 2026, card #113)

Built on the branch `dsh-113` in the worktree `~\kanban-app-113`, from `dsh-agents`
at `6a0245c` (card #96 is its base). The plan is on the card (`kb card 113`, "PLAN, revision 2");
the router's decisions R1 to R6 are PLAN.md Decisions 15 to 20; the design is
`design/stacked-cards/SPEC.md` "Parallel build". No migration: the new state is in ordinary card
fields (`merge` on a child, written by card #100) and the start queue's `first` flag.

- **Store (`crates/store/src/stacks.rs`):** `kanban_store::stack_merged(child, lead)` (`stacks::merged`) is true when
  the child's `merge.state` is `passed`, or, for a child its lead built the old way, when it has no
  branch of its own (none, or the lead's). A child whose own session started in the stack carries
  `stackOwnBuild: true` (`stackrun::mark_own_build`, from `start_session`; not session-writable):
  it is merged only by its record, Hugh's notes go to its session (else to the lead's, as
  before), and only it can be refused Detach once merged. `stack_stage(child, lead, waiting)` gives the one
  stage every caller reads: `waiting`, `building`, `to-merge`, `merged`, `passed`, `shipped`.
  `stack_ready(lead, kids, rule)` (it took only the children before) counts a child in Human test
  only once it is merged. `lead_missing_in_stack(lead, kids, rules)` is `lead_missing` with the
  lead's review counted as passed when every live child's review has (#96's ask). `set_passed`
  refuses Passed on a child that isn't merged (taking it back is always allowed).
  `stack_after_move` clears a child's `merge` record, with a log line, when it leaves the ready
  columns, before the rules run: the fix goes forward and is merged again.
- **Start queue (`crates/store/src/start_queue.rs`):** `StartQueue::push_first` puts a request at
  the front, behind other first starts (its `first` flag; an older queue has none), and
  `starts_now_first` lets it start whenever there is a place and no other first start waits, ahead
  of older ordinary requests.
  `Store::queue_start_first` is the store's entry. A lead's start uses it; the cap
  (`sessions.max_running`) is unchanged.
- **The app (`src-tauri/src/stackrun.rs`, re-exported from `dispatch`):**
  - `dispatch::waits_on(card)` reads card #109's `blockedBy` (empty on a build without #109). A
    sibling's wait is met only once it is merged (R4); any other card's once it is in Human test or
    later.
  - `stack_started(lead)`: the lead is in Planning, In progress or Human test (R1's window).
  - `child_note(child, lead)`: what a child's session is told with each move or start (its stack,
    the stack's branch, the lead's worktree, its own worktree and branch, no PR or dev stack, move
    itself to In progress after the critic). `move_message` appends it for any stack child.
  - `start_stack(lead)`: after Hugh's move of a lead into Planning or In progress (`after_move`),
    each child that waits on nothing and has never had a session moves from Backlog to Planning
    (`move_for_hugh`) and starts. `joined_started_stack` does the same for a card that joins a
    started stack.
  - `child_ready(child)`: a child that reached Human test, unmerged, in a started stack, wakes the
    lead with `merge_brief(child, lead)`: the child's branch and head, the stack's branch, the
    lead's worktree and the R3 steps (`MERGE_STEPS`, the same list the scratch-repo test runs).
    `stops_after_move` stops the child's session once its own held move into Human test is applied.
  - `after_child_merge(child)`: PASS runs `after_lead_verdict` (so `stack_recheck` runs again),
    starts each sibling that waited on this child and waits on nothing else, and queues a "the
    stack's branch moved" note for siblings still building. FAIL moves the child to In progress
    (`after_stack_move`) and wakes it with the merger's detail. Any other state logs "didn't count"
    on the lead.
  - `child_moved`, `detach_refused`, `detached_note`: a child moved by someone else hears who and
    why; Detach is refused for a merged child; an unmerged one keeps its session and is told to
    rebase onto `origin/main` with only its own commits.
  - `wake` queues the text and starts or resumes the session within R1 (never a conversation that
    lives in the desktop app).
  - `session_settings` picks effort low for a lead with no `effortOverride`.
- **The board:** the `stack_stages` command (`main.rs`) gives each child's stage, for card #116's
  overlay. `tools.rs` `stack_json` (`kanban_stack_list`, `kb stack`) adds per child `stage`,
  `session` (running, queued, paused, none), `branch`, `waitsOn`, `test`, `review` and `merge`, and
  the stack's `merged` count. The prompts read `stage` to tell the new way from the old.
- **Prompts:** `planning.md` and `in-progress.md` "If your card leads a stack" (the integrator) and
  "If your card is in a stack" (new), and `kanban-app.md` "A stack card" (worktree places, children
  don't install the test app, the lead installs once). Each new step is guarded with "If
  `kanban_stack_list` shows a `stage` for each child", so the live app, which reads the prompts
  from main before Hugh restarts it, keeps the old way. `ship-it.md` is unchanged (card #100 owns
  it): every child is on the stack's branch, so its stack paragraph still holds.
- **Waits for card #100:** the merger's "stack" job outside Ship it (R2, effort high), which records
  `merge` on the child and calls `dispatch::after_child_merge`. Until it lands nothing writes
  `merge`, so a branch-built child stays `to-merge` and the stack never becomes ready; the tests
  call `after_child_merge` directly.
- **Waits for card #109:** the `blockedBy` field. Without it `waits_on` is empty, so every child
  starts as soon as the stack does. #109's `pump_waits` skips stack children, so a child starts
  once, through `start_stack` or `after_child_merge`.
- **Lead hold:** card #96's `stackHeld` and `after_lead_verdict` stay: the lead's own test of the
  integrated build must pass before the app moves the stack to Human test.

## Each card in its own worktree (9 Nov 2026, card #146, stack #145)

Built on the branch `dsh-146` from the stack's branch `dsh-145`. No migration: the list is one
settings row, and the card's worktree is its ordinary `worktreePath` and `branch` fields.

- **The setting:** `sessions.own_worktree_repos` (Settings > Sessions, "Build each card in its
  own worktree"), a list of `{repo, checkout}` checked like card #104's list (owner/name, each
  once, a full path holding `.git`, no secrets). Empty, the default, is off. Store:
  `config.rs` (`OwnWorktreeRepo`, `check_own_worktree_repos`, `own_worktree_repos`,
  `own_worktree_repo_for`); tests in `crates/store/tests/own_worktree.rs`.
- **`src-tauri/src/worktree.rs`:** `plan_for` (pure) plans for a card of a listed repo that has no
  `worktreePath`, isn't archived and isn't in a stack. The folder is
  `<checkout>\.claude\worktrees\<name>` and the branch is `<name>`: `card-<n>` on live,
  `<channel>-card-<n>` on a test channel, so a test board's numbers never take a live card's
  branch. `make` runs git (no window, 15 s for all its commands together, because the board is
  locked; 300 s in a test build, so `cargo test` beside other builds doesn't fail on it, card #171;
  git's error kept): a folder that is already that branch's worktree is used (one git
  still has locked, an add stopped part way, is refused); otherwise `git worktree prune` forgets
  folders deleted by hand, then a branch with no folder gets a worktree, else a new branch from
  `origin/main` (else `main`), made with `--no-track` so it never has main as its upstream. There is no fetch, because the board is locked while a session
  starts; the merger rebases at ship (card #100). Anything else in the folder is refused and left
  alone. If git doesn't ignore `.claude/worktrees/` already, `make` adds it to the checkout's own
  `info/exclude` (never committed), so the checkout's `git status` stays as it was.
- **The start:** `start_session` (`session_cmds.rs`), mode "new" only, calls `own_worktree` first:
  it makes the worktree, writes `worktreePath` and `branch` (by "the app") and logs "Made its own
  worktree ...", so the session starts there. On a failure it logs "Couldn't make its own worktree:
  ...", the session starts in the repo's folder as before, and its first message gets
  `worktree::failed_note` (the git command to make it); if it was made but the card couldn't be
  written, `worktree::unrecorded_note` (record it). A resumed session keeps the folder its
  conversation ran in, on purpose: a card mid-build in the checkout must not lose its
  uncommitted work to a new folder.
- **The note:** `dispatch::worktree_note` (`worktree::note`) goes at the end of `opening`,
  `move_message`, `app_move_message` and `held_move_message` for a card whose `worktreePath` is
  its app-made folder and whose branch is its name (`worktree::made_for`, read from the card
  alone, so it keeps its note if Hugh later edits the list): work only there, never in the checkout or on main,
  don't change `worktreePath` or `branch`, and this replaces the prompts' shared-checkout rules. A
  message already waiting with the note (a new session's first one) isn't given it twice. A start
  that waited under the cap gets it after its move's message.
- **One folder name:** a stack child's note (`stackrun::child_worktree`) now names
  `<checkout>\.claude\worktrees\card-<n>` too, not `<checkout>-<n>`.
- **Not here:** the prompt and rule text (kanban-app.md, AGENTS.md and the rest) is card #148's;
  a test app per card is card #147's. Small-change auto-ship (card #104) still reads only its own
  checkout's `main`, so a card in its own worktree doesn't go on by itself: it waits in Human test.
  Each worktree builds its own `target` folder, so a card's first build is slower.

## Before-and-after evidence on the card (6 Nov 2026, card #101)

Built on the branch `dsh-humantest` in the worktree `~\kanban-app-humantest`, made
from `dsh-agents` (6a0245c). This card doesn't install the test app: the router installs ready
branches in waves on `dsh-integration` (its rule, 6 Nov).

- **Field, no migration:** the app-only card field `evidence`, newest run first:
  `[{run, at, verdict, shots:[{n, kind, caption, bytes, width, height, file, path}]}]`. `run` is the
  tester's agent id; `file` is relative to the data folder, `path` absolute with forward slashes.
  It is not in `tools.rs` `UPDATABLE`. `kanban_store::add_run` keeps every older run;
  `kanban_store::has_tested_evidence(&Card)` is true only when `test` passed and the latest run is
  that test (`run` = `test.agentId` and `at` = `test.at`) with an "after" shot (#104 uses it).
  card_revisions keep each version.
- **The stop path:** `Check.evidence` (true for the tester only). `agents::record_check` finds the
  own card and the target card (`target_card`) under a short lock and returns before copying
  anything if there is none. With the lock released, `evidence::prepare` parses the
  `EVIDENCE: before|after <path> | <caption>` lines and checks and copies the files (it takes no
  Store). Then, under the lock again, it re-reads the card and writes the verdict and the run in
  **one** `store.update`. A failed write logs "Evidence not taken: ..." on the same card as the
  verdict's failure. Checks: an absolute local path (no UNC), its real path inside `%TEMP%` and
  outside the data folder, the PNG signature and a `.png` name, 5 MB a shot, 6 a run. Copies use
  `create_new`, to `<data>\evidence\<run>\<stamp>-<n>-<kind>.png`, where `<stamp>` is the run's
  time, so a tester resumed under the same agent id files a new run. Captions and logged paths go
  through `mask_secrets`. A run whose write failed leaves its files in the folder, unreferenced
  (append-only: nothing deletes them).
- **Build stamp:** `cdp.mjs shot` reads the test app's `update_status` (its compiled-in commit)
  and writes it into the PNG as a `tEXt` chunk, `kanban-build`. `evidence.rs` reads it back as the
  shot's `build`; the run's `build` is the shots' common build, or "mixed". `update_status` now
  emits `update-ready` when its own refresh finds a change, so a shot's call never swallows the
  "Update ready" bar that the 30 s watcher would otherwise have announced.
- **Tester prompt:** step 5, only for `hughht5/kanban-app` cards: shoot the test app the brief
  names, on the port the brief gives (test 9334, test N 9333+N; 9334 when it gives none; never
  9333), crop to the changed element, take no shots if that port doesn't answer or a shot is
  blank, and never start, stop or restart either app. `kanban-app.md` 6 and 6a and `in-progress.md`
  tell the session to put the test app and its port in the brief.
- **`tools/cdp.mjs shot <png> [light|dark] [css selector]`:** refuses without `CDP_PORT`; crops to
  the selector; hides `.xterm`, `.chat` and masked text inputs during the capture (except one that
  holds the element being shot) and puts them back in a `finally`; resets the emulated colour
  scheme after each shot; never brings the window to the front; refuses a one-colour (blank)
  capture. `eval` keeps its 9333 default.
- **The test app's debug port:** `src-tauri\tauri.test.conf.json`'s main window has
  `additionalBrowserArgs` with `--remote-debugging-port=9334` and its allowed origin, plus wry's
  own defaults (setting the args replaces them). It is in the test build only, so the test app has
  its port however it is started; for test N, `install.ps1` rewrites the port to 9333+N in the
  config it generates (card #118); `tests\test_port.rs` checks it, and that the live config has
  none. `install.ps1` is unchanged. Card sessions don't get `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`
  (`sessions::build_env` strips it).
- **UI:** `ui/evidence.js`: `evidenceHtml(c)` (root class `ev`, "" with no evidence) and
  `showEvidence(root)` (a click calls `panel.js`'s `openShot`; opening "Earlier runs" calls
  `showShots`). `renderDetail` keeps only the first `.ev` box, so #102's Human test box and #104's
  row can call `evidenceHtml(c)` and the evidence still shows once.
- **Hand-over:** one guarded sentence in `prompts/in-progress.md`: when `evidence[0]` is the latest
  test (`run` and `at` match `test`), its shots from `evidence[0].shots[].path` as Markdown images;
  otherwise no shots, and it says the latest test took none. The Evidence box says the same
  ("Not from the latest test"). It reaches sessions once main has it.
- **Backups:** the nightly and migration backups copy only `board.db`, not the `evidence` folder.
  A restored board, or a `-FreshCopy` test board, keeps the field, but its shots can show "Can't
  show" (a `-FreshCopy` board's paths still point at the live data folder's files).
- **Tests:** `crates/store/tests/evidence.rs`, `src-tauri/src/evidence.rs` (parsing, each refusal,
  the copy, masking), `agents.rs` (the field per check, the prompt step), `tools.rs` (`evidence`
  refused), `src-tauri/tests/cdp_shot.rs` (`shot` without `CDP_PORT` refuses),
  `src-tauri/tests/test_port.rs` (the test window's port, none on live).

## Small changes that go on by themselves (6 Nov 2026, card #104)

Built on the branch `dsh-104` (from `dsh-agents`, after #96) in the worktree
`~\kanban-app-104`. Off until Hugh lists a repo. The plan, with the router's
critic's 22 findings and the router's decisions, is on card #104. The new hard-rule wording (AGENTS.md) was accepted by the router for Hugh on 6 Nov 2026.

- **The setting:** `sessions.small_goes_on_repos` (`SMALL_REPOS_KEY`), a JSON list of
  `SmallRepo {repo, checkout, branch}`, default `[]`, checked by `check_small_repos` (owner/name,
  listed once, an absolute checkout with a `.git`, a plain branch name). The entry names the
  checkout and branch the ship pushes, because a session can change its card's `worktreePath`.
  `Store::small_repo_for(repo)` finds it, any case; a saved value that doesn't parse is none.
- **The settings log (migration 11):** `settings_log (id, at, key, value, by)`, append-only by
  trigger, safe to run twice. `set_page_setting_by(key, value, by)` writes a row when the value
  changes (`set_page_setting` is the same with by "the app"); Settings' `set_setting` passes
  "Hugh (Settings)". `get_setting_log` lists the latest rows. A change to the repo list also
  toasts (`small-repos` event in `notify.rs`).
- **`small.rs`** (the rules, and git that can't be fooled): `git_facts(checkout, branch)` runs
  plumbing only (`rev-parse`, `symbolic-ref`, `rev-list --parents`, `cat-file`, `diff-tree` with
  `--no-ext-diff --no-textconv --no-renames --no-color`, `status --porcelain=v1 -z`,
  `hash-object --stdin`), with `core.attributesFile`, `core.fsmonitor`, `diff.external` and
  replace objects turned off, `GIT_CONFIG_NOSYSTEM=1`, the `GIT_DIR`-style variables removed and
  a 5 s timeout. `tamper` is set by `<common dir>/info/attributes` or any `refs/replace/` ref.
  `snapshot(checkout)` is HEAD and the status hash. `decide(card, entry, facts, css_evidence)`
  runs the checks in order and returns the evidence or the first reason; `skip(card)` is the
  silent never (archived, a stack, `autoTo`, `smallOptOut`).
- **Verdict records:** when a card session starts the card-tester or card-reviewer (the Agent
  call's `PreToolUse` hook), `agents::on_pre_tool` takes `small::snapshot` of the card's listed
  checkout (`checkout_of`: listed repos only, so no git runs while the list is empty) into
  `STARTS`. At its `SubagentStop`, `record_check` takes the oldest start (`take_start`) and a
  new snapshot off the lock; `place_of` places the verdict only if they match, else it records
  `placeMoved` and the verdict can't let the card go on (work changed while the agent ran). `record_of` adds `tools`, `checkout`, `head` and `statusHash`; the reviewer's `SCOPE:`
  line is parsed into `review.scope`. `decide` needs both verdicts on the current HEAD and status
  hash, so no commit date is trusted.
- **`goes_on.rs`:** `consider` (from the held-move loop in `dispatch::run`, only for a session's
  own card into Human test, with `had_auto` from before the move) reads git in a thread, then
  decides on the card as it is then (`decide_now`: the app's own writes straight after a move,
  such as changes saved and the diff check, bump the version but change nothing the rules read),
  and writes `smallChange` `waiting` (`goesAt` 10 minutes on) or `stopped` with
  the reason. `tick` (every 20 s) runs `go_on` for each card whose wait is over: checked again,
  the same head as announced, then `smallChange` `went on` (`wentOnAt`, kept for good) and
  `move_for_hugh_with` into Ship it with the record in the same change, then
  `dispatch::after_move_checked` (no second git check). A wait whose card can no longer go on
  (off the list, Take it to, opted out, a stack, archived) is stopped by `go_on` with the reason,
  never left waiting. `ship_start_refused`, first thing in
  `dispatch::after_move`, sends a went-on card back to Human test if its checkout moved, is
  dirty or is off its branch. `after_shipped`, after `kanban_ship_result` shipped, checks
  `origin/<branch>` is the checked head and warns if not. `small_opt_out` is Hugh's Stop and
  opt-out. The store ends a wait whenever the card leaves Human test, and turns "went on" into
  "stopped" (keeping `wentOnAt`) when the card leaves Ship it unshipped (`cards.rs` `apply`),
  so Hugh's own later move into Ship it is his. A bounced ship's session is sent why.
- **What sessions are told:** `goes_on::ship_note` (push exactly `<head>:<branch>`), used by
  `dispatch::move_message` and `auto_note` before they read `autoTo`.
- **UI:** `ui/small.js` (`smallChip`, `smallBox`, `smallWire`, the countdown redraw) with hooks
  in `board.js`; `smallReposBox` in `settings.js`.
- **#101 dependency:** `goes_on::css_evidence` is `kanban_store::has_tested_evidence(card)`: a CSS
  change goes on only when the tester's latest pass saved an "after" shot in that run.
- **Tests:** `small.rs` (each rule, and git on temp repos), `goes_on.rs`, `agents.rs` (the new
  record fields and SCOPE), `crates/store/tests/small.rs` (the setting, its log, migration 11, a
  newer board, the wait ending), `notify.rs` (no focus).

## Step progress on each card (6 Nov 2026, card #106, branch `dsh-106`)

- `src-tauri\src\steps.rs`: reads a session's to-do list (task folder first, transcript replay
  only when the folder is missing), cuts it to the card's current visit (`Store::entered_at`), and
  words it ("Step 4 of 7: ..."). A worker thread does every file read with no lock held; the board
  and Sessions locks are taken only to look something up. Its summaries sit in their own mutex: code
  holding the board lock may read them (`kanban_get_card` does), never the other way round.
- Fed by `dispatch::on_status` (`steps::report`, which only queues a job) and a minute's sweep of
  the cards in Planning, In progress and Human test. The board gets the `steps-changed` event and
  one bulk `card_steps` call; `ui\steps.js` changes each tile's `.steps` element in place, never
  calling `render()`. It uses #105's `addCardPart` (slot 20) or #107's `boardHooks.decorate` when
  the build has them, else a MutationObserver on `#board`.
- `sessions.todo_tools` (default on in both channels, Hugh's call on 6 Nov 2026):
  `build_env(..., todo_tools)` adds `CLAUDE_CODE_ENABLE_TODO_TOOLS=1`. The list's folder isn't
  pinned with `CLAUDE_CODE_TASK_LIST_ID` (that would put in-process subagents' tasks on the card's
  list): the worker reads it by the conversation's current id from the statusLine. A session
  started with the flag is checked once for TaskCreate in its transcript's deferred tools list,
  with a log line if it is missing.
- Claude Code (2.1.319) deletes a list's task files once every task is completed (the final test
  found it): an emptied folder counts as missing, so the transcript replay keeps "All N steps
  done" on the card (`folder_tasks`).
- Open: whether a subagent's own TaskCreate lands in the session's
  folder. The card-tester run on the test app shows both.

## The to-do list in the chat (9 Nov 2026, card #158, branch `card-158`)

- `ui\steps.js` `stepsChat()` draws the open card's `steps[cardId].items` (the same `card_steps`
  summary the tile uses, so this visit's steps only) into `#p-todo`, a row of the chat's grid
  between `.busy` and `.compose` (`ui\panel.js`, `ui\panel.css`: the grid has a seventh row).
  It runs when the panel opens and after each `loadSteps` that changed something, and redraws
  only when the items, the fold or "show all" change (`dataset.key`). Styles in `ui\steps.css`.
- Hidden while the panel shows a subagent (`panel.view`; `showView` calls `stepsChat`). A redraw
  keeps the box's scroll and, when a button in it had focus, focuses the new header.
- The fold is kept in memory (`todoFold`) and saved to `localStorage["panel.todo"]`
  (`folded`/`open`) as a best effort, for every card. "Show all" is
  `panel.todoAll`, gone when the panel closes. `TODO_MAX` (8) is the length before the earliest
  done tasks hide.
- `transcript::key_input` words TaskCreate (its subject), TaskUpdate (`#id → status · subject`)
  and TaskGet (`#id`), subjects through `steps::mask`; a TaskUpdate with neither status nor
  subject, or a call without its field, keeps the compact JSON as before.
- Not done: the items carry no `activeForm`, so the task in progress shows its subject, as the
  terminal does.

## Stack review overlay (6 Nov 2026, card #116, branch `dsh-116`)

- A stack in Human test is reviewed in its overlay (`ui/stacks.js`), as one stop in #102's queue.
  `stkStackStop` is set as `reviewStackStop` once the page has loaded (review.js loads after
  stacks.js): it opens the overlay on the first child in Human test not yet passed, or on the
  stack page once they all are, or when Hugh steps back into the stack from the stop after it
  (`stkLeaving`, or the open panel's card). The overlay's stops are the children in Human test, in stack
  order, then the stack page; past either end `reviewStepFrom(lead, ±1)` opens the next stop.
- Keys: one target in #102's handler (`STK_KEYS` via `htKeys.add`), active while the overlay is
  open in review (`stkDlg.rv`). No keydown listener of #116's own for P, F, U or the arrows.
- Pass: P then Enter. A child is `set_passed`, then the next child not yet passed, else the stack
  page. The stack page's Pass calls #102's `reviewShip(lead)` (the 5-second Undo). Pass is off for
  a merge record that isn't a pass (`checkOf(k, "merge")`) or a #113 stage that isn't merged
  (`stack_stages(lead)`, read once per list of cards; without the command there is no stage rule).
- Fail: the overlay's problem box (Enter sends; Esc closes the box). A child goes through
  `stack_problem`, the stack page through #102's `human_test_fail` (its `sendError` is shown). If
  the stack left Human test, the overlay closes and the queue opens the next stop still in it.
- The bar copies the look of #102's `htBarHtml` because #116 may not edit review.js (router rule);
  a later cleanup could give `htBarHtml` a target and share one bar.
- Hugh's close (×, the backdrop, Esc) ends the queue (`reviewClosed(false)`); `stkDlgClose` (a
  step out, a Send, a detach) doesn't. A second open while the overlay is open is a step in the
  same dialog. The overlay is drawn in parts (head, bar, checks, evidence, steps, foot), each
  replaced only when its HTML changes, so the minute-by-minute "ago" text leaves the evidence and
  the bar's focus alone.
- Checks rows are board.js's `checkRows(child)` (a Merger row comes with #100's `CHECKS` entry);
  evidence is `evidenceHtml` and `showEvidence` (#101). `openShot` puts a full-size screenshot
  inside an open dialog. #102's banner sits in the overlay while it is open and goes back to the
  page before the dialog is removed.
- Not built here: the stack card's "waiting" strip state and "n of m merged" (no owner since #113
  dropped its UI).
- Checked in a harness (the real `ui/` in headless Edge with a stand-in backend and dry-run
  cards). The test app run waits for the router's merge wave.

## Done when and acceptance checks (6 Nov 2026, card #105)

Two card fields, no migration: #105 adds no schema file (the number set aside for it, 11, went to
#104's `settings_log`). Built on branch `dsh-glance`.

- **Fields:** `doneWhen` (one line, at most 200 characters; a leading "Done when" is dropped) and
  `doneWhenChecks` (up to 10 checks, each `{text, state, note?, by?, at?}`, state `open`, `met` or
  `failed`). Cleaned in `crates/store/src/done_when.rs`, called from `cards.rs` `apply` and
  `create`. A string or a list of strings is taken (one check per line, bullets dropped); repeats
  are kept once; anything too long, too many, a bad state or an unknown key is refused whole.
- **Who and when:** the store owns `by` and `at`. `done_when::stamp` keeps them for a check whose
  state didn't change (matched by text, so reordering is fine), and marks a changed one pending;
  `write_tx` fills in the writer (`fill_pending`). A writer's own `by`/`at` are dropped.
- **Secrets:** `check_secrets` already refuses the board's words, and names the exact check
  (`doneWhenChecks[1]`). The cleaner also refuses sign-in links and Bearer headers.
  #96's `kanban_store::mask_secrets` hides all of those in text the app copies onto a card (a tester's note).
- **Older builds:** they keep the fields on every write, because `apply`'s generic arm only touches
  the keys a patch names. Their `UPDATABLE` refuses to set them, and their UI ignores them.
- **Sessions:** both names are in `tools.rs` `UPDATABLE`, one per line, with `DONE_WHEN_HINT` in
  the update tool's description. Every new prompt line (planning.md 5a, in-progress.md,
  kanban-app.md 5a and 6b) is guarded "if kanban_update_card lists doneWhen", because live reads
  main's prompt files before Hugh restarts on the new build.
- **The tester:** its prompt (`tester.rs` `PROMPT`, step 5) asks for CHECK lines on every card
  that has checks, because the prompt is fixed at session start. The reviewer's (`reviewer.rs`)
  and the critic's (`critic.rs`) prompts judge the diff and the plan against the checks too. `agents::record_check` reads the CHECK lines
  (`done_when::check_lines`) before it takes the board lock, for the card-tester only. A PASS whose
  report fails one of the card's checks is recorded as a FAIL (`pass_fails_on_checks`), so the move
  to Human test is held. The checks are marked on the card the report's CARD line names, only for
  a real passed or failed verdict, in a second write after the verdict (`mark_checks`), so a list
  the store refuses can't lose the verdict.
- **Told to the session:** `dispatch::TOLD_DONE_WHEN`, written as numbered lines; not told in Human
  test, Ship it or Deploy complete (`DONE_WHEN_QUIET`).
- **UI:** `ui/done-when.js` and `ui/done-when.css`. The status rule is `doneWhenStatus(card,
  sessionState)`. The shared hook in `ui/board.js` (`addCardPart({at, line, panel, wire})`,
  `cardLines`, `panelParts`, `panelPartsWire`) is for other cards' face lines too: suggested slots
  10 for #107, 20 for #106, 30 for #105, 40 for #108.
- **Built on dsh-integration** (rebased onto 64e2abe, after stack A): the reviewer and critic
  lines are in their prompts, and #97's critique already resets on `doneWhen`/`doneWhenChecks`
  edits (`PLAN_FIELDS`). Notes masked with #96's `kanban_store::mask_secrets`.
- **Known limits:** CHECK lines go by list order, so a list changed during a tester run could mark
  the wrong check (the log line names each check's text). A session started before the new build
  keeps the old tester prompt until it restarts.

## Turns on the test app (6 Nov 2026, card #115)

**Why.** Every kanban-app card tested in the one test app (until card #147 gave each card its own). An install restarts it, so an install
during another card's card-tester run spoiled that run, and the DSH night's `mkdir` lock folder
covered only cards that knew about it.

**Shape.** Two layers.
- *Turns* (the app, `crates/store/src/testapp.rs`, JSON in the setting `testapp.queue`; the live
  app's board is the real queue): a **tester** turn per card session, one at a time, and an
  exclusive **install** turn that goes to the front. Pure logic, clock passed in. A card whose
  session isn't running keeps its place but is passed over, by the pump (`next`) and by a take
  (`take_now`, card #149): with nobody holding the turn and nothing outside the queue blocking,
  a take from a card first in line, or behind only paused cards, gets the turn in its reply.
  **A stopped card's turn (card #168):** when the turn is a card's (it holds it and hasn't heard
  it yet, a "your turn" still queued, `Live::unheard`; or nobody holds it and
  it is the first in line `blocked` doesn't stop, past cards whose wake ran out) and its session
  isn't running, `step` marks it (`to_wake`, `woke_at` on its `Turn` or `Waiting`, once until its
  session is seen running again: a live holder, or the card's own take, `ran`) and the pump starts its
  session (`wake`): its app conversation resumed, else a new one in its worktree, through
  `begin_session` (why "testapp"), so the cap and start queue apply. A conversation only in the
  desktop app is never started. For `WAKE_GRACE_MINUTES` (5) a woken card in line keeps the turn's
  place (`next` and `take_now` give it to nobody behind it); once its session runs, `next` gives
  it the turn and its usual message. Then the line moves on, and it keeps its place; a woken
  holder whose session still isn't up loses the turn (`wake_ran_out`, `Ended::NoSession`) and its
  test app (`free_unstarted`), so the next card waiting for a test app gets it. If the start
  fails, the card's log says why: a held turn ends (with its ended message) and its test app is
  freed the same way, and a place in
  line is passed over at once (`wake_failed`). A new session (no app conversation) is first told
  why it runs (`woke_new_message`). It asks again (a take) to be woken again. The status shows
  `waking` on the holder and line entries; the board shows "starting its session".
  `src-tauri/src/testapp.rs` runs it: the pump every 2 s from `dispatch::run` (its first run is the
  start-up check), the `kanban_test_app` tool, `kb testapp`, the page's commands. "Your turn" and
  "your turn ended" are queued messages only: no session is resumed for them (router decision R2),
  except a stopped card's own turn, above (card #168).
  `copy_board` resets the queue, so a test board never inherits the live holder.
- *The script's lock and waits* (`scripts\install.ps1 -Channel test`, mode now): it builds first,
  then takes `install.lock` and waits before quitting the test app. It never calls the app.

**Shared folder** `%USERPROFILE%\.claude-kanban-dsh-shared\testapp\`, outside both channels' data folders:

| File | Written by | What |
|---|---|---|
| `install-wait-<pid>.json` | install.ps1 | while it waits, without the lock (same fields, phase waiting); the board's "restart pending" banner reads it; one whose pid is dead is ignored |
| `install.lock` | install.ps1 | held only once its waits are clear, while it quits, installs and starts the test app: `pid`, `pidStart`, `owner`, `label`, `card`, `commit`, `worktree`, `startedAt`, `phase` (waiting, installing, started), `waitingFor`, `restartAt`, and (card #147) `channel`, the test app it is for |
| `turn-live.json`, `turn-test.json`, `turn-testN.json` | each app | its holder `{kind, card, owner, label, givenAt, expiresAt}` (Test's) and `appPid`/`appStart`, written atomically (temp file, rename); since card #147 also `slots`, per test app |
| `hold.json` | either board | `until` (Hold, 30 min a click, at most 2 h) or `allowUntil` (Let it restart, 5 min) |
| `input-test.json`, `input-testN.json` (card #147) | each test app | `at`: Hugh's last trusted input in its focused window (cdp.mjs sends no trusted input) |

**The lock protocol** (for #114 and #117 too):
- The holder opens `install.lock` with OpenOrCreate, ReadWrite, FileShare Read|Delete,
  DeleteOnClose, and writes only through that handle (seek, write, SetLength, Flush). Windows
  deletes it when the script ends or is killed.
- Readers open with FileShare ReadWrite|Delete and close at once. The JSON files the apps write are
  read the same way, or the rename that replaces them fails.
- The script retries on a sharing violation and on access denied (the delete-pending window).
- Held = the file is there and its `pid` runs with the same start time (`pidStart`, within 2 s),
  so a reused pid doesn't count. Off Windows, #117 checks with a non-blocking flock.

**The script's waits**, before it quits the test app, all without holding `install.lock` (so a run
with no turn never blocks the turn holder's run): a turn file names a live holder that isn't this run
(the run's owner is `KANBAN_LOCK_OWNER`, else `KANBAN_CARD`, else `hugh`; `-IgnoreTurn` is Hugh's
override); a Hold; Hugh's input in the last 10 min; the old DSH folder with another owner; and the
notice (2 min when Hugh used the test app in the last hour, else none), which starts only once the
other waits are clear. Then it takes `install.lock` (waiting for another install counts towards
`-LockWaitMinutes`, default 30) and checks the turn files once more. The app gives no tester turn
while `install.lock` is there for anyone else, and treats the other board's live turn the same way,
so the live and test boards never both give the one test app away. An install turn ends when its
install lets go of `install.lock`, when it stops asking for 10 min while in line (`kb --wait` asks
every 10 s), or when its install hasn't started within 15 min. `-LockOnly` with `-LockDir`
and `-DshLockDir` runs just this, against fixtures: `scripts\test-install-lock.ps1`.

**The change-over (remove after DSH).** While the old folder `%USERPROFILE%\.dsh-test-app.lock`
exists with another owner, the app gives no new turn and the script waits; the new script also takes
that folder itself (owner file: the owner, then `pid <pid> <start>`) and removes it only if it made
it and still owns it. A folder whose pid line names a dead process blocks nobody, and the next run
renames it aside and takes it. A folder an older writer made (no pid line) is stale once it is 60 min
old (`DSH_STALE_MINUTES`, card #149): it then blocks nobody either, and the next run renames it aside
too. A folder whose pid still runs blocks whatever its age. Hugh's Release renames it
to `.dsh-test-app.lock.released-<time>`. The lock only protects runs of a script from a base that
has #115: older worktrees' `install.ps1` ignore it. Remove `read_dsh`/`release_dsh` and the
script's DSH folder block when the last DSH card is in Deploy complete or Done.

**Only unit-tested before the ship.** The live app gives turns only once Hugh ships it, so during
DSH no real card uses the queue (the guarded prompt step skips it on the old live app). The clock
paths (start-up release, 45-min amber, the limit) and a turn surviving its own app's restart are
unit tests. The script's waits are the fixture test. After the ship: with a tester turn held in the
live app, the next install from outside a card session waits for it, the banner shows on both boards, Hold
works, and the install goes ahead after the release.
## A test app per card (9 Nov 2026, card #147, stack #145)

**Why.** #118 made test slots (Test, Test 2 ... Test 10), but the turns (#115) still covered one
test app: every card took turns on Test, a live turn on Test held an install into Test 2, and
once a card gave its turn back another card could install over its build while Hugh tested it.

**Shape.** Two layers, all in `crates/store/src/testapp.rs` (rules) and `src-tauri/src/testapp.rs`
(files, pump, tool).
- *A test app per card.* `kanban_test_app` take (a card session) first gives the card a test
  app of its own: the lowest free slot from 1 to the setting `testapp.slots` (a ceiling, 1 to 10,
  default 10; Settings > Sessions, "Test apps, at most"). Hugh (9 Nov): when every test app holds
  a card's build, make a new one, so a card gets Test 4 when Test to Test 3 are taken, and its
  own install creates it (a first install copies the live board). Stored on the card as
  `testApp` {slot, channel, name, label, port, since}; only the app writes it (it isn't in
  tools.rs `UPDATABLE`). The card keeps it while it is In progress or Human test and not
  archived; a stack's card (`stackOf`) only in In progress, since Hugh tests a stack on its
  lead's install. The pump (`run`) frees it when the card moves on or is archived, logging why;
  Hugh's Free (the popover, `testapp_free`) or `kb testapp free --slot N` takes it at once, and
  the card's session hears so. Only at the ceiling does a card wait, in `testapp.slot_line`; the
  pump gives it the next freed one, with a queued message.
- *Turns per test app.* #115's queue, one per slot: `testapp.queue` stays Test's (an older
  build still reads it), `testapp.queue.N` for Test N. A card's tester turn is on its own test
  app. Outside install turns name one (`kb testapp take --install --slot N`, default 1).
  `release` gives back the turn only: the test app stays the card's.
- *Across boards.* A test app never gives out its own slot, and no board gives one that another
  running app's turn file shows as given or held (`Shared::elsewhere`). `copy_board` clears every
  `testApp`, the slot line and every queue (`Store::clear_test_apps`).

**Shared folder** (Test keeps #115's names, so old scripts and apps still meet on it):
- `install.lock` stays one file for every test app (it covers only the quit, install and
  restart). It and `install-wait-<pid>.json` gain `channel`; an app holds turns only on that test
  app while it is there, and a lock with no channel (an older script) holds every one.
- `input-<channel>.json`: each test app writes its own (`input-test.json` for Test). The page
  records input in every test channel, not only "test".
- `hold.json`: still one, for every test app.
- `turn-<channel>.json` keeps `holder` (Test's turn, for older scripts) and gains `slots`:
  `{"test2": {slot, card, label, holder}}` for each test app the board gave a card or has a turn
  on. An old file (no `slots`) counts as before: the live app's holder on Test, a test app's on
  itself.

**install.ps1** (`-Channel test|testN`, mode now): its turn wait reads every running app's turn
file for its own test app only; a new owner wait holds it while its test app holds another card's
build and this run has no turn on it ("Test 2 holds card #140's build"); it stops at once when
the run's card has a different test app ("card #147's test app is Test 2: run this with
-Channel test2"). A card number counts only on the run's own board (from `KANBAN_DATA_DIR`; none is live), since a test board's #140 is another card. Hugh's own run (owner `hugh`) warns and goes ahead over a card's build. `-IgnoreTurn` (Hugh only) skips all three. An outside install turn (`kb testapp take --install --slot N`) is refused on a test app a card has: free it first. Input and the notice read the run's
own `input-<channel>.json`. `scripts\test-install-lock.ps1` has cases for each.

**The tool's replies** name the test app, its install command (`platform::install_command`) and
CDP port, so a card session needs no prompt change to find its own. #148 has the rule text.

**Open items.** The Mac side (`install.sh`, `kb install-lock`) has no per-slot turns and no owner
wait yet. A freed test app keeps running with its last build until the next card installs into it.

## Parallel builds, ship one at a time or as a stack: the rules (9 Nov 2026, card #148, stack #145)

Built on the branch `dsh-148` from the stack's branch `dsh-145`, after #146 and #147 merged. Docs,
prompts and one test; no app code. The old rule ("one kanban-app card builds at a time, on `main`
in the shared checkout") is gone everywhere.

- **The rule:** every card for this repo builds in its own worktree
  (`<checkout>\.claude\worktrees\card-<n>`, branch `card-<n>`, from `origin/main`; card #146 makes
  it for a listed repo, else the card's Planning session does), installs into its own test app
  (card #147), and ships alone, one at a time per repo (card #79), rebased onto the latest
  `origin/main` by its card-merger (card #100). A stack still ships as one, on its lead (card #113).
- **Where it is written:** `AGENTS.md` "Parallel builds" and the test-app bullets;
  `prompts/kanban-app.md` "Cards build in parallel", Planning 1 to 3, In progress 1 and 4b to 7,
  the Human test line, Ship it 1a to 3 and Deploy complete; `prompts/planning.md` and
  `prompts/in-progress.md` (a stack child's worktree is `<checkout>/.claude/worktrees/card-<n>`,
  as `stackrun::child_worktree` now says; the tester's brief names the card's own test app);
  `prompts/router.md` job 5; `docs/GUIDE.md` "This app's own cards build in parallel". The owner's
  private copies of `kanban-app.md`, `router.md`, `planning.md` and `in-progress.md` say the same,
  with real paths, on a branch of the private repo that ships with this stack.
- **The Router no longer integrates single cards** (decision: card #148, for Hugh to overturn). Its
  job 5 used to merge each finished kanban-app branch onto an integration branch and install the
  one test app after a wave. With a test app per card, each card is tested on its own branch and
  ships alone, so the wave isn't needed. The merger's integrate job (`merger.rs`, "INTEGRATE JOB",
  the `integrate` verdict field) stays in the code, unused by the prompt;
  `routersession.rs`'s prompt test still finds "card-merger" and "integrate" in job 5.
- **Removing a card's worktree.** Ship it can't remove the worktree it runs in: Windows refuses to
  delete a folder that a running process has as its current folder (checked 9 Nov 2026). So Ship
  it deletes the branch on GitHub and logs the two commands for later
  (`git -C <checkout> worktree remove <worktreePath>`, then `git -C <checkout> branch -d <branch>`),
  for once the card's session has stopped. Each worktree's `target` folder is several GB, so they
  should be removed. Open item: the app could do it when a shipped card's session ends.
- **Private prompts ship with the stack.** The live app reads the private folder's `prompts\`
  straight from its checkout, so a change there is made on a branch in a worktree of the private
  repo, never on its `main`, and the ship fast-forwards that repo's `main` too (`kanban-app.md`
  Ship it 3). To try new private prompts in a test app, point that test board's Private folder
  (Settings › Sessions) at the private worktree.
- **Test:** `src-tauri/tests/prompt_rules.rs` reads the bundled `kanban-app.md`, `planning.md`,
  `in-progress.md`, `router.md` and `AGENTS.md`, and fails if the old rule comes back or the new one
  goes.

## Cards that wait on others (6 Nov 2026, card #109, branch `dsh-order`)

- **Fields:** `blockedBy: [n, ...]` and `waitedOn: [n, ...]` in the card's fields (no table).
  `crates/store/src/waits.rs` cleans both (`clean_blocked_by`, from `apply` and `create`) and checks
  a change of `blockedBy` in `write_tx` (`check_waits`: only the newly added numbers, the circle
  search breadth-first over every card's waits, archived ones included), so every write path is
  checked: the panel, MCP, `kb`, REST and the stack writes. `stacks::join_check` refuses a join
  between a card and a card it waits on (`check_join`), and `create_in_stack` a wait on the new
  card's own lead. `Store::update` and `create` log "Now waits on #12" / "No longer waits on #12".
- **The poll:** `dispatch::pump_waits` runs in the dispatcher's 2-second tick after `pump_ships`.
  It takes the board, calls `Store::settle_waits(is_test())`, lets go, then passes on what it did:
  `after_move` on its own thread for a card it moved, `app.emit("notify")` for Hugh, a queued
  message for a card with a session. The empty case is `any_waits`, one query on the partial index
  `cards_waiting` (schema12.sql; a test checks the query plan).
- **`settle_waits`**, per card that isn't a stack child (the `WAITING` query leaves them out, so
  their waits don't keep the poll busy). An error on one card leaves that card for the next tick
  and keeps what was done for the others. First `waitedOn`: a card it waited on that is no longer
  met is logged and dropped, so it is said once (a started card's session is told; a card not
  started yet waits on it again and Hugh is told); one shipped is dropped quietly (on main, nothing
  left to watch). Then `blockedBy`: a wait that can never be met (archived without shipping, or no
  such card) comes off and Hugh is told. Once every blocker `is_met`:
  - in Blocked, the last column change in `card_revisions` by "Hugh (board)", an empty
    `blockedReason`, and on the test channel `dryRun`: `Store::move_after_waits` to Planning,
    logged as `WAIT_BY` ("the app (Blocked by)"), with `plan.waitedOn`, `autoTo` kept, and
    `blockedBy` moved to `waitedOn`;
  - any other card in Backlog or Blocked, or a release `move_after_waits` refused: `blockedBy`
    moved to `waitedOn`, logged with the reason, `WaitKind::TellHugh` (once, not every tick);
  - Planning or later: `blockedBy` moved to `waitedOn`, `WaitKind::TellSession`.

  A card that changed while it looked is left for the next tick.
- **`move_as`** takes a private `How` (`Hugh`, `Auto(why)`, `Waits(ns)`); `move_by_hugh` and
  `move_for_hugh` keep their signatures. `move_message` spots a waits release (`waits_moved`:
  `plan.waitedOn` on a plan requested or started) and adds `Store::wait_note`: each blocker's
  column and branch, and for one in the same repo not shipped yet, "It is not on main yet".
- **Card sessions:** `blockedBy` is in `tools.rs` `UPDATABLE` on its own line; `waits_refused`
  lets a card's own session set it only on a child of the stack its card leads, with every wait
  inside that stack. `dispatch::wait_move_refused` refuses a card session's move of a card not
  started yet (Backlog or Blocked) with unmet waits into Planning or later, in `kanban_move_card`
  and again when a held move is applied. A card already started moves as before.
  `TOLD` has `blockedBy`, and `edit_message` explains it.
- **Interface for #99 (monitor) and #113 (stacks):** `Store::wait_edges() -> Vec<Wait>` (every
  wait of every card that isn't archived, stack children included; `Wait { card, card_id, column,
  on, on_column, on_archived, met, stack, in_cycle }`, `stack` being the waiting card's lead),
  `Store::wait_cycles() -> Vec<Vec<i64>>` (each circle once, from its lowest number; it finds
  circles an older build wrote unchecked), `kanban_store::wait_path(edges, from, to)`,
  `kanban_store::is_met(waiter, blocker)`, `Card::blocked_by()`, `Card::waited_on()`. The poll
  never touches a stack's children: #113 starts them. A lead it releases starts alone:
  `pump_waits` calls `after_wait_release` (no `start_stack`), and the lead's log and message say
  so (`LEAD_NOTE`); its children start on Hugh's own move of the lead (whole-build review N2).
- **Migration 12:** `schema12.sql` adds only the partial index (`CREATE INDEX IF NOT EXISTS`),
  after the usual pre-migration backup. It changes no row, is safe to run twice, and an older
  build keeps the index up to date as it writes. `SCHEMA_VERSION` (`MIGRATIONS.len()`) replaces
  the hard-coded 10 in the tests. Schema 11 is #104's (settings_log); #114 renumbers after this one.
- **UI:** `ui/waits.js` and `ui/waits.css` (two lines in `index.html`). The chip and the panel row
  go through #107's `boardHooks` (`decorate`, `details`) when it is there, and otherwise wrap
  `cardEl` and `renderDetail` (the row goes above Summary, as #107's slot puts it); the row's
  controls use delegated listeners. `board.js` isn't edited. It is one IIFE exposing only
  `window.KBWaits`, so its names can't clash with #107's `needs.js`. The row saves through
  `invoke("update_card")` itself, not `write()`, so it can show the store's refusal text; on a
  version conflict it reloads and tries once more. A stack's child gets no chip on the board (it is
  inside the stack card), only the panel row.
- **Prompt:** `planning.md` has one line on `blockedBy`, guarded on `kanban_update_card` listing it.
- **Tests:** `crates/store/tests/waits.rs`, two in `dispatch.rs`, `tools.rs`, and `kb`'s usage.

## Review mode, session search, Agents feed (6 Nov 2026, card #120, panel part)

- **Review mode** (`ui\review.js`, `ui\review-mode.css`): the panel gets `.rv` while a queued card
  shows the review. The bar `#p-rbar` stays one element (role toolbar, one focus target, the
  `dataset.html` cache): in `.rv` it is a CSS subgrid over the panel's rows, its `.rv-head` in the
  top row beside `.ph`'s buttons and its `.rv-acts` in the row under the details. `#p-det` shows
  only `.htbox` (`display:contents`, its parts ordered by CSS) and done-when.js's `#dw-box`, so the
  checks are drawn once. `.work` is hidden, except the compose box in fail mode (`.rv-failing`).
  `rvView` ("review" or "chat") resets to review on each card. Shot widths at the default panel
  (inset 14 px, details padding 16 px, gap 8 px): about 600 px each at a 1280 px window, about
  920 px at 1920 (height capped at 52% of the window, `object-fit: contain`). Worked out from the
  CSS, not measured in the app.
- **Agents feed** (`ui\agents-feed.js`): `mountAgentsFeed(el, {filter, source, onCard, compact})`,
  helpers on `window.AgentsFeed`, tests in `tools\ui-tests\feed.test.mjs`. The drawer uses
  `ui\rpane.css` (`.rpane`, `-head`, `-digest`, `-feed`, `-tabs`, `-body`), which card #119's Router
  pane reuses; with #119 in, the Agents drawer can open the Router pane instead.
- **Time bar** (`timesBarHtml` in `ui\times.js`) draws into the search dialog's `#ss-times` when it
  opens and when the medians come.

## Search every card's session history (6 Nov 2026, card #110)

- **Its own file.** `search.db` beside `board.db`, in `kanban_store::search` with its own migrations
  (`search1.sql`, counted in its own `user_version`), so the board's lock, 30 backups and
  migrations never carry it. No board migration: `schema13` was left unused.
- **FTS5 trigram**, external-content over `entries`. Every table takes inserts only: triggers refuse
  UPDATE and DELETE, and a known key is ignored (a REPLACE can't remove a row). An entry's key is
  the transcript line's uuid plus its place in the line, or for a line with no uuid the file id, the
  byte offset and the place.
- **What it reads.** Every `sessions` row (archived cards included), each card's desktop
  conversation (`claudeSessionId`), and the file each Stop, SubagentStop and SessionEnd hook names
  (`transcript_path`, so the new file after a /clear is found), each with its subagents. Reads go
  in bounded chunks (4 MB) and only to the last newline. The board's lock is held only to read the
  session list.
- **90 days.** Lines older than that aren't indexed or found. Each month, or when
  `search::INDEX_VERSION` goes up, a new file (`search.new.db`) is built while the old one still
  answers, its `files` rows carried over; then the old one is renamed into `backups\search` (one
  copy kept). A build with a lower index version, or an older schema, leaves a newer file alone.
- **Masking** (`search::mask`): whole tokens, to the next separator, for the board's own secret
  words with their values, JWTs, Stripe, GitHub, Anthropic, Google and Slack tokens, Bearer
  headers, private-key blocks and runs of exactly 32 hex digits (the API token; git and SHA-256
  hashes stay searchable); sign-in links become
  `[sign-in link hidden]`. Raise `INDEX_VERSION` when the mask changes, so old text is re-masked.
- **Speed.** Searches use read-only connections, one for the board and one for `kb search` and
  kanban_search; a new board search interrupts the board's last one, never a tool's. Files are
  read oldest first, so entry numbers follow time and "the newest 2,000" are by number. A search takes the newest 2,000 matches (counts stop at 1,000). On 200,000 entries a
  common word answers in about 150 ms (the test's bound is 300 ms in a release build). Size,
  measured on 6 Nov: the card sessions' last 90 days hold about 20 MB of text, so about 100 MB of
  index.
- **Who can search.** Hugh only: the board, `kb search` and `kanban_search` (a control tool, never
  offered to card sessions; `api::tool_list` and its test).
- **The indexer thread** keeps its writer open; hooks (each turn's end) index the files they name at
  once, and a full sweep still runs every 5 minutes however many hooks arrive. A conversation whose
  transcript isn't on disk is looked for again after an hour. A swap first ends WAL mode on both
  files (so nothing is left beside them) and refuses while either is open; a swap that stopped
  half-way is finished at the next start (`search::recover_swap`).
- SQLite is built optimised in dev and test builds too (`Cargo.toml`), as its full-text inserts are
  about ten times slower unoptimised.

## Dev-stack health on logins (4 Nov 2026)

For a repo whose dev stack is its own `scripts/dev-stack.sh`, run inside WSL from the branch's checkout. `stack_health` (`src-tauri/src/stack.rs`, `ui/stack.js`) runs `launch::stack_up`. It reads the branch with `git -C <worktreePath> rev-parse --abbrev-ref HEAD`, using Windows git with no window, and only on a Windows-drive checkout. Otherwise it uses the card's `branch`, and it flags a card `branch` that differs from the checkout's. When the stack is down, **Restart stack** shows `cd <worktree as /mnt/c/...> && scripts/dev-stack.sh restart <branch>` to paste into a WSL terminal. The app never runs it (no WSL from the app).

## The platform layer (6 Nov 2026, card #114)

Every operating-system call is in `src-tauri/src/platform/`: `mod.rs` lists the interface, and
`windows.rs` holds the Windows code, moved there unchanged from sessions.rs (the Job Object and
the exit waiter), tray.rs (hiding the window, the quit event), place.rs (work area, screens,
SetWindowPos and the Alt-tap), notify.rs (WinRT toasts and their click), update.rs (the
PowerShell helper for Restart now), launch.rs (Chrome's paths), slash.rs and stack.rs (no
console window) and session_cmds.rs (`file:///` paths). The call sites only call it.
`kanban_store::home_var()` is the one place the home folder is read. `main.rs` calls
`platform::at_start()` and `platform::on_run_event()`, which do nothing on Windows, so card
#117 can add macOS (`platform/macos.rs`, the same names) without editing main.rs.

- **The rule:** new Windows-only code goes in `platform/windows.rs` behind a function every
  system provides. `src-tauri/tests/platform_guard.rs` scans every crate's `src/` (outside
  `platform/`, tests and comments) for Windows APIs, `creation_flags`, `USERPROFILE`, `APPDATA`,
  `ProgramFiles`, `.hwnd(`, `powershell`, `.exe"` and `C:\`, and fails with the file and line.
  Deliberate cases are in `src-tauri/tests/platform-allow.txt`, each with a reason; an entry
  that matches nothing fails too.
- **Tests of the moved code** (Windows only): the Job ends its processes when its owner exits
  (the test binary starts itself as the owner), the exit waiter, the quit event on a random
  name (never the live or test app's), the update helper's quoting and its PowerShell parse.
- **CI:** `.github/workflows/ci.yml` runs `cargo test --workspace` and builds the app and kb on
  `windows-latest` for every push. Card #117 adds the macOS jobs.
- **`{prompts}`:** prompts name their folder as `{prompts}` (the public split's syntax).
  `dispatch::with_prompts` fills it in with the private folder's `prompts\`, or else the bundled
  examples (`private.rs`, see "Private folder" below), with
  `{prompts}\` taking the system's separator; `fill` does it first, so a card's title or reason is
  never taken for it. `BACK_FROM_TEST` and the stack plan message use it too. Migration 13
  (`crates/store/src/schema13.sql`; 11 is card #104's, 12 card #109's) rewrites the shipped
  column prompts and the "Kanban app" template, only where the text is exactly what an earlier
  migration wrote.

## macOS (6 Nov 2026, card #117): untested on real hardware, unsigned

**State:** macOS is untested on a real Mac and unsigned: no Apple Developer ID, not notarised (Hugh,
6 Nov: "apple - do your best but put untested on readme"). The README tells a user how to allow
it: System Settings > Privacy & Security > Open Anyway, or `xattr -dr com.apple.quarantine`. The
proof so far is CI on GitHub's Mac runners. ACCEPTANCE.md has the list for a real Mac.

- **Where:** `src-tauri/src/platform/macos.rs` (the Tauri and AppKit glue, the same names as
  `windows.rs`) and the `kanban-unix` crate (`crates/unix`: the logic with no Tauri or SQLite, so
  `cargo check -p kanban-unix --target aarch64-apple-darwin` works on Windows). kb has two hidden
  Unix commands, `kb reap` and `kb install-lock`. The app crate compiles for macOS only on CI.
- **Sessions end with the app:** each claude leads its own process session (portable-pty calls
  setsid). Every 5 s the app records every process in those sessions (pid and start time, by
  session ID) in `<data>/run/pids-<app pid>.json`; the record only grows until a process dies.
  `kb reap`, started by the app in its own process group, waits for the app to exit (kqueue) and
  ends each recorded process whose start time still matches. A card's Stop ends claude only, as
  on Windows. The exit waiter doesn't reap (waitid WNOWAIT); Sessions reaps a dead session's
  child when it drops it.
- **Start-up (`at_start`):** the open-files limit up to 10240; PATH (and LANG if the shell gives
  one) from `$SHELL -l -c` (else the user record's shell, else zsh), 5 s at most, then fallback
  folders; LANG from AppleLocale when none is set; data folders made 0700; an App Nap activity.
  One line per start in `<data>/macos.log` names the shell, PATH, LANG and the file limit.
- **Private files:** `api.json` and the per-session settings and MCP files (they hold the token)
  are written 0600 through `platform::write_private`; on Windows it is a plain write.
- **Focus:** `platform::builder` sets `activate_ignoring_other_apps(false)` for `--background`;
  the window shows with `orderBack`; install.sh starts with `open -g`.
- **Quit:** SIGTERM, SIGINT and SIGHUP quit as tray > Quit does (install.sh uses SIGTERM, on the
  pid now in `api.json`). Cmd+Q is our own menu item: with sessions running the page asks
  (`ui/quit.js`, events `confirm-quit`, `quit-shown`, `quit-answer`), and a native alert asks
  if the page hasn't shown it within 3 s. Dock Quit and log-out quit at once.
- **Notifications:** mac-notification-sys with a click waiter; a click opens the card. A card's
  older notification is removed first, and waiters end after an hour, so at most 16 wait.
- **Updates:** `install.sh` (stage, the live default) leaves a ditto zip of the .app and
  `update.json`. Restart now's sh helper swaps the bundle (the old one to `update/previous.app`,
  put back if the move fails), logs every step and the exact error to `update/restart.log`, and
  writes `update/restart-result.json`; the next start's banner says once if it failed. Windows'
  helper writes the same file since card #134.
- **Bundle:** `tauri.macos.conf.json` (read by Tauri by itself), `tauri.macos.bundle.json` (kb as
  `externalBin` and `scripts/mac-kb.sh`, passed only by install.sh and CI so `cargo test` never
  needs the sidecar), `tauri.test.macos.conf.json` (test icons), `tauri.smoke.conf.json` (the
  smoke test's own app, `io.github.hughht5.claude-kanban-dsh.smoke`, so it never meets the test app).
- **Signing:** `scripts/mac-signing-cert.sh` (Hugh, once): a self-signed "Claude Kanban DSH Local"
  identity, 10 years, usable by codesign without a keychain prompt. install.sh uses it with
  `--timestamp=none`, else signs ad hoc. An Apple Developer account (US$99 a year) is the only
  way to a notarised app; Hugh decided against it for now.
- **The test-app lock (card #115's protocol):** install.sh holds `install.lock` on its own file
  descriptor (`exec 9>>`); `kb install-lock --fd 9` waits by #115's rules and takes the flock on
  that descriptor, so the lock ends with the shell, even if it is killed.
- **CI:** the Mac jobs in `ci.yml` run on this private repo only for pushes to `dsh-117` that
  change code, and for a manual run (router decision R1): Mac minutes bill at about ten times.
  A small Linux job decides, and runs them if in doubt. **After DSH ships to the public repo
  (claude-kanban-dsh), move the Mac jobs there, where they run free**, and take them out of this
  repo's ci.yml.
- **Windows-only for now:** account 2 (refuses to start on a Mac), a product repo's dev stacks,
  `tools/cdp.mjs`.

## Test version

- **What it is:** the same code built with `KANBAN_CHANNEL=test` (read at compile time by `kanban_store::channel()`) and the overlay `src-tauri/tauri.test.conf.json` (productName "Claude Kanban DSH Test", binary `claude-kanban-dsh-test.exe`, identifier `io.github.hughht5.claude-kanban-dsh.test`, hooks `windows/hooks.test.nsh`). Data in `%USERPROFILE%\.claude-kanban-dsh-test`, quit event `Local\ClaudeKanbanDshQuitTest`, build output in `target\test`, install in `%USERPROFILE%\Programs\Claude Kanban DSH Test`, not on PATH. The board shows a `--warn` bar (`app_channel` command).
- **Install or refresh it:** `scripts\install.ps1 -Channel test` (quits and restarts only the test app). The first install copies the live board in with `kb copy-board`; `-FreshCopy` replaces the test board with a new copy (the old one goes to its `backups`).
- **Swap a change in:** try it on the test copy, then run `scripts\install.ps1` with no `-Channel` from the same commit. That stages the update; Hugh clicks Restart now when it suits him (or add `-Now`). The live build clears `KANBAN_CHANNEL` and `CARGO_TARGET_DIR` itself.
- **By hand:** `KANBAN_CHANNEL=test CARGO_TARGET_DIR=target/test cargo tauri build --config src-tauri/tauri.test.conf.json`. Never set `KANBAN_CHANNEL` for a live build.
- **Test slots (card #118):** `KANBAN_CHANNEL` also takes `test2` to `test10` (ports up to 9343). `crates/store/src/channel.rs` derives every name from the slot number (live 0, test 1, testN N): `app_name_for`, `quit_event_for`, `product_name_for`, `exe_name_for`, `identifier_for`, `devtools_port_for` (9333 + N), `data_dir_for`. Any other value stops the build (a `const` assert), so a typo can't make a build that opens the live board. `install.ps1 -Channel testN` derives the same names, writes `target\testN\tauri.testN.conf.json` (tauri.test.conf.json with the slot's productName, binary, identifier and title) and `target\testN\hooks.nsh` (kb.exe from `target\testN\release`), and builds with them; nothing is generated in the checkout. `-NamesOnly` prints the names as JSON; `crates/store/tests/channels.rs` holds the script and `tauri.test.conf.json` to `channel.rs`. The UI takes its name from the `app_name` command ("Claude Kanban DSH (test 2)") for the bar and titles; toasts say "(test 2)". `test` itself is unchanged. **Sessions reach only their own board (final-test finding F1):** a test build's `build_env` passes `build_env_for` its data folder and install folder, so each card session gets `KANBAN_DATA_DIR` and the test app's folder first on PATH; `kb` and `kb mcp` take the board folder from `KANBAN_DATA_DIR` first (`kanban_store::kb_data_dir`). That covers the repo's `.mcp.json` `kb mcp` (the live kb on PATH, once it has this change; until then the PATH entry makes it the test kb) and `kb` in Bash. The live build passes no board and drops an inherited `KANBAN_DATA_DIR`.
- **Copied sessions:** `copy_board` records each session still running on the source as suspended ("copied", not mid-turn) in the copy and moves `app.run_started_seq` past them, so the test app never resumes a live conversation.

## Each card's choice of subagents (9 Nov 2026, card #151)

Hugh sets, once, which of the six card subagents a new card starts with, and turns each on or off
per card. The gates follow the card's own choice.

- **The field:** `subagents` on the card, `{router, critic, reproducer, reviewer, tester,
  merger}`, each true or false (`crates/store/src/subagents.rs`, `SUBAGENTS_KEY`, the table
  `SUBAGENTS`). `Store::create` stamps it on every new card from the board's defaults, with
  whatever Hugh's Add card sent on top (`for_new_card`); an update merges the keys it names
  (`merged`, in `cards.rs` `apply`). A bad key or a value that isn't true or false is refused.
  A card made before #151 has no field: `Store::subagent_on` falls back to the board's default
  for any key it lacks, so it behaves as every card did before.
- **The defaults:** one page setting each, all in `RULE_KEYS` ("on" unless "off"): the four
  board-wide rules they replace keep their keys (`sessions.critic_before_build`,
  `sessions.review_before_human_test`, `sessions.test_before_human_test`,
  `sessions.repro_before_human_test`), so Hugh's critic-off stays off; two are new,
  `sessions.router_in_planning` (`ROUTER_RULE_KEY`) and `sessions.merger_at_ship`
  (`MERGER_RULE_KEY`), on by default. No migration: the field is JSON and an older build ignores it.
- **Only Hugh:** `subagents` isn't in `tools.rs` `UPDATABLE`, and `kanban_create_card` doesn't copy
  it, so card sessions, the Router and `kb` can't set it (a test checks). The app's own `create_card`
  and `update_card` commands (the page) are the only writers. `tools.rs` `doc` shows the card's
  subagents as they hold now; the Router's `compact_list` has `subagentsOff`.
- **The gates:** `critic::gate`, `reviewer::gate` (and its `capped_hold`), `tester::gate`,
  `repro::gate`, `repro::mark_needed`, Found a problem's lead note (`main.rs`), and a stack lead's
  own test, review and repro hold (`stacks.rs` `stack_recheck`) read `Store::subagent_on(card, ..)`
  instead of the board rule (the lead's for its stack's reproducer). The router and the merger
  hold nothing. The merger's choice is for the card's own ship: a stack's lead still merges each
  child with the card-merger (card #113), since the stack is ready only once they are merged.
  Hugh's change of a lead's subagents runs its stack's rules again (`after_lead_verdict`).
- **The sessions:** `dispatch::subagents_note(store, card, column)` lists the subagents run in that
  column that are off on the card (Planning: router, critic; In progress: reproducer, reviewer,
  tester; Ship it: merger), each with what to skip, and whether it is the card's own choice or the
  board's default. It replaces card #97's `CRITIQUE_OFF` in `move_message`, and goes in
  `app_move_message`, `held_move_message`, a stack child's start and moves
  (`stackrun::start_child`, `child_moved`), Found a problem on a child (`stack_problem`) and the
  messages back from Human test (`moved_back_message`, `failed_message`, `lead_failed_message`).
  Hugh's change in the details logs "Hugh set its subagents. On: ... Off: ..." once, only when it
  changed (`main.rs` `update_card`), and sends the session a note (`dispatch::after_edit`,
  `subagents_changed`). Every session still has all six
  in `--agents`, so turning one back on needs no restart.
- **UI:** `ui/subagents.js` (one list, `NEW_CARD_SUBAGENTS`, for all three places; a store test,
  `the_pages_table_is_this_one`, checks it against `SUBAGENTS`) and
  `ui/subagents.css`: Settings > Sessions' "Subagents on a new card" group (six switches, in place
  of the four rule switches), the Add card ticks (`subagentTicks`, set from the switches;
  `subagentChoice` goes in `fields.subagents`), and the details' Subagents box (`subagentsBox`,
  saved as soon as one changes, with a pending state so a redraw doesn't undo it). The verdict rows
  and the Route row say "off for this card", or "off by the default in Settings" for a card made
  before #151.
- **Prompts:** `prompts/router.md` jobs 1 and 3 skip a subagent the card has off. The private
  folder's prompts are unchanged: the app's note tells each session what to skip.
- **Left as it was:** the small-card go-on (#98) still needs the critique's pass, and small-change
  auto-ship (#104) still needs both the reviewer's and tester's passes, so a card with one of them
  off waits for Hugh there.

## A card's flow: skip Planning, skip Human test, three-way subagents (9 Nov 2026, card #162)

Hugh's per-card options for the steps a card goes through, each with a default in Settings ›
Sessions › Card flow defaults, each three-way on the card: true, false, or absent (follow the
default as it is now). `crates/store/src/flow.rs` holds the keys and the store's reads.

- **Subagents, three-way:** a card's `subagents` now holds only Hugh's overrides
  (`subagents::for_new_card` keeps only what Add card chose; `merged` takes null for a key, back to
  the default, and drops the field when none is left; null for the whole field clears them all).
  Nothing is stamped on a new card, so a later change of a default reaches it. **Migration 17**
  (`schema17.sql` is a marker; the work is `subagents::unstamp_defaults`, run by `Store::migrate`
  in the same transaction, after the pre-migration backup): on each card not archived, a stamped
  value equal to the board's default goes; one that differs stays. Each change is an ordinary
  write with its revision, by `UNSTAMP_BY`. Idempotent; an older build reads a card with no
  override from Settings, as before.
- **Skip Planning** (`skipPlanning`, default `sessions.skip_planning`, off): `cards.rs`
  `planning_skip` decides; `move_as` (Hugh's drag, `move_for_hugh` for Add card's Take it to and
  `start_new`, `move_after_waits`) and `dispatch::session_move_to` (a session's or the Router's
  move) send a move into Planning from Backlog or Blocked to In progress instead, with no plan
  request: the `plan` record is `{state: "skipped", whose, from, auto?, waitedOn?}` and the log
  says "Skipped Planning: Hugh's choice (this card's own)" or "(the default in Settings)". The
  card's next move takes the record off (`apply`). `critic::gate` doesn't hold such a card from
  Backlog or Blocked into In progress. `MoveOutcome.skipped_planning` and `move_card`'s
  `skippedPlanning` tell the page. A stack's child never skips; a move into Planning from a later
  column still plans.
- **Skip Human test** (`skipHumanTest`, default `sessions.skip_human_test`, off):
  `dispatch::auto_progress` moves a card in Human test with no `autoTo` into Ship it for Hugh
  (`move_for_hugh`, the ship queue, a lead only once `stack_ready(.., "all")`) when
  `skip_human_test_why` says so: it skips, and `Store::arrival` shows the last move into Human
  test wasn't Hugh's own ("Hugh (board)") and its `autoTo` wasn't cleared since (Take it to:
  Human test stopped it there). A child never goes on. `small::skip` and `goes_on`'s `read`,
  `decide_now` and `cant_go_on` leave such a card out of small-change auto-ship.
- **Sessions told:** `dispatch::flow_note` (in `move_message`, `app_move_message`,
  `held_move_message`): `SKIPPED_PLANNING` on the move that skipped Planning, `SKIPS_HUMAN_TEST`
  in In progress, `SHIP_NO_HUMAN_TEST` in Ship it; `move_message`'s why for the app's ship. Hugh's
  edits: `flow_changed`, `flow_log_lines` (one log line each), `skips_changed_note` and the
  subagents note via `after_edit`; a skipHumanTest edit runs `auto_progress`. A default changed in
  Settings: `settings.rs` `set_setting` calls `dispatch::after_default_change`, which tells each
  following card's session in the column it matters to, logs it, and reruns `after_lead_verdict`
  for following leads.
- **Only Hugh:** the page's `update_card` and `create_card`; his own sessions outside the app and
  plain `kb` (not held in `tools.rs` `call_inner`: `not_updatable_by`), which also log and run
  `after_edit`. A card session and the Router are refused. `kb update` takes `--skip-planning`,
  `--skip-human-test` (on, off, default) and `--subagent <name>=on|off|default`.
  `kanban_create_card` takes `skipPlanning: true` from the same callers (card #169, `kb create
  --skip-planning`; refused with `stack`): it sets the new card's `skipPlanning` and starts it, so
  `move_as` sends it to In progress.
  `kanban_get_card` and `kb card` show a computed `flow` (`Store::flow_of`).
- **UI:** `ui/subagents.js`: the Card flow box (`subagentsBox`, one key per change; card #178 made
  each three-way select a switch, `role="switch"` with `data-own` default|on|off, and a Reset
  button beside an override), Add card's box (`subagentTicks`, `flowChoice`: only what isn't Default), the chips (a
  `boardHooks.decorate` hook, `flowChips`), and `CARD_FLOW_SKIPS` for Settings' "Card flow
  defaults" group in `ui/settings.js`. Tests: `crates/store/tests/flow.rs`, `subagents.rs`,
  `dispatch.rs`, `critic.rs`, `tools.rs`, `small.rs`, `kb`, `tools/ui-tests/flow.test.mjs`.

## The app's own messages in the chat (9 Nov 2026, card #153)

The chat (card panel and Router panel) draws text the app sent on its own in the centre, amber,
tagged "Automatic"; Hugh's words stay on the right and Claude's on the left. Migration 15 adds
the append-only `app_messages` table (`card_id` is the card's id, or `router`). `Store::queue`
records every `message` it queues as the app's; Hugh's words go through `Store::queue_hugh`
(`send_to_session(.., Some(words))`: his chat message, Send now, Report a bug, Fail note, Found a
problem, stack answer) and are never recorded. Where the app wraps his words (a Fail note's
lead-in and reproducer note, a message that moved a card back, a bug report),
`Store::note_app_pieces` records each blank-line piece that holds none of his words, so those
sentences show as the app's; a sentence with his words inside stays his. A message put back in
the queue (`sessions.rs`, untaken) uses `queue_hugh` too, as it was recorded when first queued.
Sends that skip the queue record with `Store::note_app_message`, before the send: the monitor's
typed nudge, the Router's `kanban_session_send` to a card (`routertools.rs`; `session_send`
itself sends as Hugh, for `kb`), the Router's first message, its events batch and a card's
message to it (`send_by` with a `by` not starting "Hugh").
`transcript_read` (the chat's read, main view only) splits each user entry with
`kanban_store::split_app_parts`: the queue joins waiting messages with a blank line, so a piece
between blank lines that equals a recorded text is the app's and the rest is Hugh's, in order
(`Entry.parts`). `transcript_chunk` is the plain read (`kanban_session_read` uses it).
`userHtml` in `panel.js` draws them; `router.js` uses it too.
Limits: messages sent before this build have no record and still show on the right; a message
still waiting (the faded "Sent" or "Queued" bubble) shows on the right until Claude takes it;
text Hugh pastes that equals one of the app's earlier messages is drawn as the app's.

## Router panel: images (7 Nov 2026, card #127)

UI only. The Router panel's message box takes pasted images and dropped files like a card chat's:
`router.js` `onPaste`, `addFiles`, `renderAtts`, `onDragOver`, `onDrop`; files are saved with
`attachment_save` as card id `router` and sent as `[Attached: <path>]` lines. File drags over the
whole `.rpane` are cancelled (the window has no native drop), with `dropEffect` "copy" so the
Router-off message shows; the disabled box has `pointer-events:none`.

## The Router session (6 Nov 2026, card #119, branch `dsh-119`)

One long-running Claude Code session the app owns for the whole board (the DSH "router"). Hugh
starts it from the Router panel (the Router tab in the Agents drawer; the header's Router button went in card #186) and talks to it in the Router panel, with `kb router`,
or any session with `kanban_router`. The decisions below are the router's after the critic's FAIL
(`~\kanban-critic\119.md`); the plan on card #119 has the rest.

- **Not a card.** Migration 14 (`schema14.sql`): `router_sessions` (each start or resume: row id,
  conversation id, channel, account, cwd, model, effort) and `router_log` (kind decision, event,
  session or note; the card it is about). Both append-only. Store API in
  `crates/store/src/router.rs`. Board lists, search, archive, import and the card tools never see
  it. `copy_board` (`-FreshCopy`) calls `detach_router`: the copy has the Router off and never
  resumes the source's conversation (`router.resume_after`).
- **The session.** `routersession.rs`. In the live map its key is `KEY` = "router" (cards use
  their ids), so the terminal, chat, questions, hooks and MCP are the card session's.
  `Sessions::start_as` takes the key; `start` is `start_as` with the card. `Plan.router` gives
  `--append-system-prompt` with `prompts/router.md` (its rules survive every auto-compaction) and
  its own `--agents` (`routersession::AGENTS`: card-router, card-merger, card-tester, on Sonnet
  medium; the full list put the command line over Windows' 32,767 characters). Model and effort
  from `router.model` / `router.effort` (Sonnet, low). Its folder is `%USERPROFILE%\kanban-router`
  (outside the data folder; `kanban-router-<channel>` for each test slot). The first start shows Claude
  Code's trust question in the panel's Terminal tab.
- **Outside the cap**, never in the start queue (`running_count` skips the key), never idle-paused
  (`idle_for` skips it). `router.on` is set by Hugh's Start and cleared by his Stop;
  `resume_at_start` (main.rs setup) resumes it whenever it is on, idle or not. A crash is told to
  Hugh once (`tick`); the app never restarts it by itself.
- **Edits.** The trial on Claude Code 2.1.246 showed a session-level `--disallowedTools Edit,Write`
  takes them from the subagents too. So the Router's `PreToolUse` guard matcher is
  `guard::ROUTER_TOOLS` and `router_refusal` refuses Edit, Write, NotebookEdit and MultiEdit for
  the Router and every subagent but its card-merger. Its shells (and its subagents') are refused
  `kb`, `claude config`/`mcp`, any push or `gh pr merge`, Stop-Process/taskkill of the app,
  `sqlite3` or a script on `board.db`, and `install.ps1` other than `-Channel test` without `-Now`
  (a test-channel Router: no install at all). Refusals go to the Router's log.
- **Events, no polling.** `tick` (every dispatcher pass) reads new `card_revisions` by seq — the
  one source, so every move path is seen — and makes NEW, MOVED and VERDICT lines
  (`event_line`). Its own writes ("the router") and, for 20 s, the app's writes for a card its
  subagent just reported on (`note_own_verdict`) are skipped. Lines are typed after 30 s of quiet
  (or 3 min in all), joined and capped at 25 lines (`events_message`). Hugh's messages go first,
  with no wait. Nothing is queued while it is stopped; `first_message` on a resume says to read
  the board once.
- **Tools.** `routertools.rs` checks, then `tools::call_as_router` runs the card tool with
  `by` = "the router" and the card sessions' gates. `if_version` is required on writes and moves;
  a write or move on a card Hugh has open or changed in the last 2 minutes is refused (`hands_off`; a `kanban_session_send` message is not, card #170); moves follow
  `routersession::move_refused` (Planning, In progress, Human test, Blocked freely; Backlog to
  Planning or In progress only with `router.may_start_cards`, card #174; never Ship it, never out of Ship it or Deploy
  complete, never into Deploy complete or Done). `kanban_start_card` (setting on, the card's own
  `worktreePath`, through the cap and queue), `kanban_router_log`, a compact `kanban_list_cards`
  (no summaries), and `kanban_session_send/status/read` (a send that would resume a stopped
  session needs the setting). No `kanban_set_passed`, `kanban_ship_result` or ship request.
- **The Done pile (card #131).** `kanban_archive_card {card, if_version, reason, evidence?}` never
  archives: after `card_for_write` it calls `Store::ask_archive` (`crates/store/src/archive_ask.rs`),
  which writes `archiveAsk` = {reason, evidence, at, by, column}. Refused in Ship it, for a stack's
  child, for a lead with a child in Ship it, and while Hugh's Not now (`archiveAskDeclined`, the
  version it made) stands: only a later write by "Hugh..." or "card #N's session" for that card
  lifts it (`lifts_decline`, `revisions_after`). `write_tx` removes the ask when the card changes
  column, is archived or restored, or joins a stack (`archive_ask::stale`). The board
  (`ui/archive-ask.js`) draws the pile before the Archive and takes those cards out of their
  columns; **Archive all** calls `router_archive_approve` (`Store::approve_archive`: still asked,
  same column, `archive_refused`, then `move_by_hugh` into Done, so `write_tx` archives it), then
  `dispatch::after_move` per card and `Sessions::stop` for a lead's live children (archived in
  place, which no after-move reaches). **Not now** calls `router_archive_dismiss`. Neither field is
  in `UPDATABLE`, and `kanban_archive_card` is in `agents::CARD_TOOLS`, so no session, `kb` or
  subagent sets them. Tests: `crates/store/tests/archive_ask.rs`, `tools/ui-tests/archive-ask.test.mjs`,
  and the Router's tool list and prompt tests.
- **Verdicts.** `agents::own_or_named`: for the Router's session the card is the one the report
  (or the brief's `CARD:` line, for #104's work snapshot) names. Used by `record_check`, the
  merger, the reproducer and the card-router.
- **Monitor (#99).** Its `Notify` goes to the Router (`monitor_finding`, log "Told the Router")
  while `takes_events`: running, not failed, and nothing waiting to be typed for over 10 min.
  Otherwise Hugh is told as before, and `tick` tells him once that the Router is stuck.
  `stuck.notified` stays a bool.
- **Merger (#100).** Job "integrate" in the prompt: the integration worktree, a lock folder in
  the git dir, `git merge --no-ff` (merge commits allowed only here), `cargo test`, reset on a
  FAIL, a `MODE: integrate` and `TESTS:` line. `on_stop` writes `integrate` (not `merge`), and a
  failed TESTS line makes it a FAIL. The ship job merges origin/main into an integration branch
  once instead of a rebase.
- **Panel.** `ui/router.js`, `ui/router.css`: a panel docked
  right (default min(720 px, 55% of the window), resizable; log and chat side by side from a 620 px width, stacked below it, card #128; a saved 380 counts as unset, double-click on the edge clears the saved width), the digest, the feed (card #120's `mountAgentsFeed` when present,
  else a plain list from `router_feed`), Chat and Terminal tabs, the "Router asks" pill
  (`registerPill` when #120 is merged). Commands: `router_status`, `router_start`, `router_stop`,
  `router_send`, `router_feed`, `router_opened`; event `router-changed`. The digest is the app's
  (`routersession::digest`, no tokens), after 2 h away (`router.last_opened`, a setting).
- **Routing:** the Router routes NEW cards; a Planning session still runs the card-router when its
  card has no route yet. `router::record` keeps the first route, so the two never fight.
- **Change-over (decision 12):** nothing changes until Hugh presses Start. Until then his main
  session stays the router.
- **Tests:** `crates/store/tests/router.rs` (tables, backup, copy, events, settings),
  `routersession.rs` (events, debounce, cap, moves, dot, digest, Hugh's 2 minutes, prompt),
  `routertools.rs` (tool list, compact list, creates), `guard.rs` (edits, every shell form),
  `sessions.rs` (command line, system prompt, matcher), `merger.rs` (integrate), `kb`.
- **Not tested by cargo (finding 25): the model's own decisions.** The trial for the test board,
  with dry-run cards, once the branch is in a test-app wave: (1) Start; the dot goes idle and the
  log says "Started a new conversation". (2) Make a dry-run card in Backlog; within a minute the
  log has an event and the card gets a `route`. (3) With "Router may start cards" off, ask it to
  start the card: refused, and it tells Hugh. (4) Move a card with a failed check into Human test:
  it goes back to In progress with the reason. (5) Make a session loop until the Monitor fires:
  the card's log says "Told the Router". (6) Stop; the dot goes off; restart the app: it stays
  off. Start, restart the app: it resumes.
- **After /code-review (10 findings, all fixed):** a waiting message counts from when it was queued, and a working Router is never "stuck"; `kanban_create_card` only in Backlog, Blocked, Planning or In progress; `kanban_start_card` only for a card in Planning or In progress; adding a card to a stack under way needs the start setting; test-app calls only as owner "router", never forced; the guard refuses any process stop and checks each install command on its own; `kb router` from Hugh is his instruction, a card's message is data; `is_router_row` answers a live row with no board lock; only the one verdict write of a Router subagent is skipped.
- **After the final test (`kanban-critic/final-test-119.md`, all fixed):** F1 a Router move runs `dispatch::after_router_move` (the card's after-move, as Hugh's drag; never a lead's children; not for a move to the column it is in), and `kanban_start_card` starts a card with no worktree in its repo's folder (`routertools::start_folder`; refused for a card whose repo has no folder); F2 `hands_off` reads the open panel (`notify::panel_open`), not the window's focus; F3 the "Router asks" pill is third (order 30), tone `router`, redrawn on each status; F4 `Store::agents_feed` reads `router_log` too (agent "router"; Hugh's own lines "hugh"; a card's message by "card #n's session"), each event is logged with its card, and the panel mounts the Agents feed itself; F5 the folder-trust question shows in the chat with buttons (`router_trust`, `router_trust_answer`: only while the Router is starting, answered with the option's number key); F6 `kanban_create_card` takes `dryRun`; F7 the Router's toast says "The Router asks you" and the question, and waits while its panel is open (`router_panel_opened`); F8 `kb sessions` lists it as "router"; F9 each channel has its own folder (`kanban-router`, `kanban-router-test`, `kanban-router-test3`). Decided: a Router move into Planning or In progress starts the card's session as Hugh's drag would (Hugh's F1); "Router may start cards" still gates Backlog to Planning and `kanban_start_card`.
- **After the re-test (`kanban-critic/final-test-119-retest.md`):** F5 Claude Code 2.1.319 draws the trust menu with no numbers and "No, exit" first, so `trust_menu` finds the two option lines ("No, exit" and "Yes, I trust this folder", numbered or not, next to each other) and the cursor line; Hugh's Yes in the chat sends Down or Up to its line, then Enter (`trust_keys`), never a number key ("1" is "No, exit" there). The real screen is `fixtures/router-trust-screen.txt` and its test. F7 the question comes from the AskUserQuestion PreToolUse hook's input (`routersession::on_hook`, `question_of_input`), kept until its PostToolUse, before the hook sets the "asks" state; the transcript is a fallback and its JSON input is read for the words. Small: Hugh's kb message is logged as his words only (the feed labels it Hugh); a line the Router writes on a card and in its own log shows once (`feed::dedupe_router_lines`), and the Router's `kanban_add_log` no longer copies itself into its log.
- **After re-test 2 (`kanban-critic/final-test-119-retest2.md`):** a card's question toast is "#n asks you" with the question's words (from the session's waits, as `kb sessions <n>` shows them; `notify::permission_toast`), and a real permission prompt says what it asks to run; a new Start begins with one start message (`routersession::begin`), and a session that ends drops what it hadn't taken, so a refused trust question's start message no longer comes in twice; one routing decision shows once in the feed, as its route verdict (`feed::dedupe_verdict_echoes` drops the card-router's "Routed: …" line and the Router's "route recorded" echo), and router.md tells the Router not to log a route itself.
- **Known limits:** a notification for the Router (card "router") opens the board, not a card.
  The Router's sessions have no `session_events` rows (that table's rows belong to cards), so
  its mid-turn state at a quit isn't kept; `resume_at_start` resumes it either way.

## kb.exe is replaced even while it runs (6 Nov 2026, card #126, branch `dsh-126`)

- **The bug:** every Claude session in a checkout with the repo's `.mcp.json` keeps a `kb mcp`
  running from the install folder's kb.exe. Windows won't overwrite or delete a running exe, and
  the silent installer ignored the failed `File`, so kb.exe stayed old while the app updated.
- **The fix** (`src-tauri\windows\hooks.nsh`, NSIS_HOOK_POSTINSTALL): delete `kb.old-*.exe` left
  by earlier installs (those still in use stay), rename kb.exe to the first free `kb.old-N.exe`
  (N up to 99; Windows allows renaming a running exe in its own folder), write the new kb.exe,
  then delete the moved copy (it goes now if nothing runs it). If the new kb.exe didn't land, the
  old one is moved back, so `kb` never goes missing. Running `kb mcp` processes keep the old copy
  until their session ends; `kb` on PATH and new sessions get the new one. Every path gets it:
  live `-Now`, Restart now and every test slot (they all `!include` hooks.nsh). Uninstall also
  deletes `kb.old-*.exe` and then the emptied install folder (Tauri's own `RMDir` runs before the
  hook, while kb.exe is still there).
- **If it can't (card #134):** when no rename works (99 copies in use, or a handle opened
  without delete sharing) and writing over it fails too, or the new kb.exe didn't land and the
  old one was put back, the old kb.exe stays and the installer exits **3** (`SetErrorLevel 3`).
  The app itself is installed. `scripts\kb-holders.ps1` (`Get-KbHolders <path>`, the Windows
  Restart Manager, so it finds file handles too) names the processes holding it.
  - `install.ps1`: exit 3 doesn't throw. It warns "The app is installed, but kb.exe is still the
    old one: held by ...", carries on and starts the app, warns again at the end, and exits 3.
  - Restart now's Windows helper (`platform::update_helper`, which embeds kb-holders.ps1 with
    `include_str!`) now writes `update\restart-result.json`, as the Mac helper does: UTF-8 with
    no byte-order mark, `{ok, kind, error, at}`, before it starts the app. Exit 0 is ok; exit 3
    is kind "kb" with the holders as `error`; any other exit, an installer that didn't start or
    an app that didn't exit in 5 min is kind "install". A file with no kind (the Mac's) is
    "install".
  - `update.rs` reads it once (`last_failure`): kind "install" sets `failed` ("The last update
    didn't install"), kind "kb" sets `kb_notice`, a bar of its own ("Updated, but kb.exe is still
    the old version — held by ..."). OK (`update_failure_seen`, with the page's `kind`) clears only the
    one it showed; the failure shows first. A first install whose kb.exe didn't land exits 3 too.
- **Test:** `scripts\test-kb-locked.ps1 [-Channel test]` (test channels only), in your test-app
  turn. It installs three times; see its header. `-Restart` and `-Fail` (card #136, stack #133)
  test Restart now instead, because install.ps1's own kb.exe copy (its step 4) would hide a
  broken hook and the helper has no such copy. Each stages the build, clicks `#updNow` over the
  slot's CDP port, and checks the app comes back with `--background`. `-Restart` holds the planted
  kb.exe with a `kb mcp` and checks it is replaced, with an ok result and no notice. `-Fail` opens
  kb.exe from the script with no write or delete sharing: a `kb mcp` can't make the installer
  exit 3, since a running exe can still be renamed. It then checks the result has kind "kb" naming
  the script's pid, the planted kb.exe stays, and the kb.exe notice shows until OK.
- **Uninstall (card #137, stack #133):** the clean-up is NSIS_HOOK_POSTUNINSTALL, not PRE, on
  purpose: Tauri runs PREUNINSTALL before its "is the app running?" check, which can abort, and a
  kb.exe deleted there would leave the app with no `kb`. A kb.exe or kb.old-N.exe a session's
  `kb mcp` still runs can't be deleted (and a per-user uninstall can't schedule it for a restart):
  it and the folder stay until that session ends (a line in the uninstaller's window says so; a
  silent uninstall shows nothing). A silent update never runs the uninstaller; a setup.exe run by
  hand with "uninstall first" kept does, and then installs kb.exe afresh.
  `src-tauri\tests\nsis_hooks.rs` holds the hook to that. `scripts\test-uninstall.ps1 -Channel
  <testN>` (test channels only, in your test-app turn) uninstalls that test app twice, plain and
  with kb.exe held, checks the folder goes and the data folder stays, and installs it again.

## Decisions taken (beyond PLAN.md)

- **Time to check (card #103, 6 Nov 2026):** two classes, not minutes. Only styles, markup, images, docs and tests can be a 30-second look; JavaScript and TypeScript are code wherever they are, and prompts and agent instructions need real testing (the router's decisions, after the critic). The router's guess can only make it stricter. No manual override and no setting.
- Plain JavaScript UI with no bundler; libraries are vendored into `ui/vendor` by `ui-deps/vendor.mjs`.
- **Private folder:** the column prompts and the repo list aren't in the code. `src-tauri/src/private.rs` reads the setting `private.dir` (Settings › Sessions › "Private folder"). `{prompts}` in a prompt or template stands for that folder's `prompts\` if it exists, else the bundled `prompts\` examples (beside the installed exe, or this repo's). Its `repos.json` (`[{"repo":"owner/name","folder":"~\\name","label":"name","loginDirs":["frontend"]}]`) gives the Add card form's repos, each repo's session folder and the top-level folders whose changes need a sign-in to check; a repo not listed starts in the home folder. Settings › Sessions › "Repos a card can be for" (card #155, `reposBox` in `ui/settings.js`) edits the list: `edit_repos` reads it as written and refuses a file the board can't read (so a Save never drops what wasn't shown); `save_repos` (`private::save_repos_in`) checks each `owner/name`, refuses a repo twice, keeps each repo's other fields (`loginDirs`; a renamed row is matched by its loaded name, `was`) and nameless entries, and when the text changes keeps the old file as `repos.json.bak` and writes the new one through `repos.json.tmp`. See README, "Private folder".
- Card fields other than number, column, order and archived live in a `fields` JSON column, under the artifact's own names.
- Done is archived (card #83, 5 Nov 2026): `write_tx` archives any card a write puts in Done (with `archivedFrom`, a log line, and the lead's stack via `stack_archive_children`), `create` archives a card made in Done, and `archive(on: false)` sends a Done card back by `restore_patch`. Migration 10 archived the cards already in Done. The board draws no Done column (`BOARD_COLS` in `ui/board.js`); the Archive column's drop box moves a card to Done through `move_card`, so `after_move` stops its session as before.
- **Append-only:** logs, revisions, sessions and session events take inserts only; there is no `DELETE FROM` anywhere (a test checks this); triggers refuse deletes.
- **Sessions:** started or resumed only by Hugh's click or a column's auto-spawn. A card whose conversation is in the desktop app is never auto-started.
- **Accounts:** the account comes from the session row, because Account 2's `projects` folder is a junction to the main one. A resume on a card pinned to the other account is refused.
- **Readiness:** `SessionStart` doesn't arrive as an `http` hook, so the statusLine's first report (`kb status`, posted to `/status`) marks a session ready. The app's own text is typed only when the session is ready, idle and has been quiet for 1.5 s. Hugh's messages (panel box, `kb send`) are typed at once whenever `sessions::takes_typing` allows (ready; idle, working or failed; nothing held), and the box is shown with no session too: Send starts or resumes it (card #62, 4 Nov 2026). The `AskUserQuestion` tool's Pre/PostToolUse hooks mark its dialog as `permission`, so nothing is typed into it.
- **"Testing" chip (card #86, 5 Nov 2026):** the session's `PreToolUse`/`PostToolUse` hooks match `AskUserQuestion|Agent|Task|Skill`, and it sends `SubagentStop`. `testing::Testing` (one per live session) counts a foreground review or test call from its Pre to its Post (by `tool_use_id`), and a background one (its Post carries `tool_response.agentId`) until that agent's `SubagentStop`, capped at 3 h. An inline skill (its Post has no `agentId`) and a review or test slash command typed as the prompt (`UserPromptSubmit`) count until the turn's `Stop`; a call made inside a background subagent (its hook carries `agent_id`) until that agent stops. What counts: `testing::is_testing_call`, a word starting "test", "review" or "verif", or "qa", in a skill's name or a subagent's description or type (never its prompt), unless the description starts with a build verb ("Build the testing chip"). The app emits `testing` `{card, on}`; `session_doing` carries the same flag (card #150, which took the `session_testing` command out: nothing read it any more).
- **The status chip (card #150, 9 Nov 2026):** one chip per tile says what the card is doing, in place of `buildingChip` (Building and Testing, cards #75 and #86) and the test app's own tile chip (card #115). Backend: `doing::Running` (`src-tauri/src/doing.rs`; card #129's `running.rs` is a different module), one per live session, fed in `dispatch::on_hook` by `Sessions::running_hook` from the same `Agent`/`Task` Pre/Post, `SubagentStop`, `Stop` and `SessionEnd` hooks as `testing.rs` and by its rules (foreground from Pre to Post; background from its Post's `agentId` until its `SubagentStop`, capped at 3 h; a Post whose `status` says it finished starts nothing). It keeps each subagent's `subagent_type`, description (120 characters), start and whether it runs in the background; `turn_ended` (an interrupt the transcript recorded) clears the session's own calls too. `session_doing` returns, per live card, `{state, since, turnSince, running, testing}` (`Sessions::doing`; RFC 3339 times from `state_since` and `turn_since`), and the app emits `doing` `{card}` when the subagents change. Page: `ui/doing.js` `doingOf(card, ctx)` takes the first that holds (needs, the card's install, subagents, a review skill, the test app turn, its place in line, working, failed, starting, unmet waits, shipped, idle, no session), from `session_doing` and the other files (`needs`, `stuckOf`, `window.testappState` and the document's `testapp-status` event from testapp.js, `window.Waits.unmet` from waits.js, `steps`, `startQueue`). A stuck card keeps face.js's "May be stuck" box: its chip is what the session does, with the Monitor's reason first in the tooltip. Until `session_doing` first answers (or on an older backend), panel.js's `activity` stands in (Working and Idle with no times) and "No session" isn't said; `activity` also wins for the moment after a state change. `cardEl` draws `doingChip(c)` where `buildingChip` was (the plan chip when it gives nothing, and beside it while the plan is requested or claimed); a decorate hook puts it on a stack's `.stk-sub` and drops `.wt-chip` from a tile whose chip says "Waiting on"; face.js orders it first (`data-doing`, kind `doing`). `doingPaint` updates every tile's chip in place (words, tooltip, kind and colour) on each new `session_doing` answer, each test app change and every 30 s; it redraws the board only when a chip comes or goes, or turns to or from "Waiting on". Tests: `doing.rs`'s own and `tools/ui-tests/doing.test.mjs`.
- **"Working…" line (card #66, 4 Nov 2026):** each live session keeps `turn_since`, set when the hooks first say `working` (or `permission`) and cleared at idle, failed or ended (`sessions::turn_start`); `session_info` returns it as `turnMs`. `transcript_read` returns `thought`, the chunk's latest thinking block timed from the line before it (looking back up to 1 MB before the read's offset). The panel draws them under the conversation, ticking every second.
- **Messages:** one waiting message per card, with new text appended; one held move per card, with the newest winning.
- **Port:** the API falls back to an OS-picked port, because 47821 can't be bound on this machine.
- **The folder-trust question in the chat (card #143, 9 Nov 2026):** as the Router's (card #119, F5), for card sessions. `session_trust` (session_cmds.rs) returns the question's words, naming the latest session's folder, while the session is starting (`state == "starting"` or not yet ready) and `routersession::trust_ask` finds the menu on its screen; `session_trust_answer` checks again and types `routersession::trust_keys` (Down or Up to the option, then Enter, never a digit), logs Hugh's answer on the card and emits `session-changed`. The panel (`checkTrust` in `ui/panel.js`) asks every 1.5 s with its transcript poll, only while `session_info` says running and starting, and draws `#p-trust` (router.css's `.rtrust` box) under the chat.
- **Questions in the chat (card #78, 5 Nov 2026):** `transcript::ask_questions` puts an `AskUserQuestion` call's questions on its entry (`ask`), and the panel draws them as a question card (`askHtml` in `ui/panel.js`). The `PreToolUse` hook's `tool_use_id` and `tool_input` are kept on the live session (`question`, cleared when it leaves `permission`); `session_info` returns the id as `question`. `session_answer` checks the id and types `sessions::answer_keys`: in a single-select question a digit picks and moves on; "Type something" is the digit after the options, a paste and Enter (1.2 s before the next key, or it lands in the typed text); in a multi-select digits tick and Right moves on; with more than one question, or any multi-select, a review screen follows where `1` submits. Learnt on Claude Code 2.1.318: if the dialog changes, fix `answer_keys` and its tests.
- **Screenshots in the chat (card #84, 5 Nov 2026):** `image_read` (`session_cmds.rs`) returns a local image as a `data:` URL: absolute local paths only (no UNC, so no SMB connection), image extensions from `transcript::image_mime` (the one list; `IMG_EXT` in `ui/panel.js` must match), at most `MAX_ATTACHMENT`. `transcript::parse_line` puts a `Read` of an image on its entry as `images`. In `ui/panel.js` a DOMPurify hook (registered once) turns a local `<img src>` in Markdown into `data-local`; `showShots` loads what is on view (not inside a closed `<details>`), cached per path and message time (so a shot saved again under the same name shows afresh in a later message), up to 150 MB; failures aren't cached. The sessions learn the `![..](C:/..)` form from `api.rs` `INSTRUCTIONS` and the column prompts.
- **Subagents in the chat (card #91, 5 Nov 2026):** Claude Code keeps each subagent of a session in `<session id>\subagents\agent-<id>.jsonl` beside the transcript, with `agent-<id>.meta.json` (`agentType`, or `name` for a forked skill; `description`; `toolUseId`, the Agent call that started it; `finished` once it has reported back). `transcript::subagents` lists them (an id must be letters and digits, so nothing outside the folder is read); `read_in(.., sub: true)` keeps the sidechain lines the main reader drops. The panel polls `subagent_list` every 3 s, draws the strip (`renderSubs`), and `showView(id)` re-reads from offset 0 with `transcript_read`'s `agent`. Running means not `finished` while the card's session is up; `finished` never comes if the session was killed, so that reads Stopped. `transcript::task_notice` turns a `<task-notification>` user line into a "task" entry (summary, task id in `tool_id`, status in `input`). Learnt on Claude Code 2.1.318: if the folder or the meta fields change, fix `transcript::subagents` and its test.
- **Interrupted turns (card #78):** Esc, or turning down a permission prompt or question in the terminal, ends the turn with no `Stop` or `PostToolUse` hook, which left the card on "Needs you" or "Working" until the next prompt. On each statusLine report for a session on a turn, `dispatch::on_status` reads the transcript's last 64 KB (`transcript::turn_ended_in`); a `system`/`turn_duration` line (or Claude Code's "[Request interrupted by user" line) with nothing of the main conversation after it, later than the state began or the latest `UserPromptSubmit` (`sessions::ended_by`), sets the session idle. The 400 ms loop checks again every 20 s for sessions whose state hasn't changed for 10 s (`quiet_turns`), in case no statusLine report came after the end was written. `session_answer` stops typing as soon as the session leaves `permission`, so its keys never reach a later prompt; a typed answer's control characters are dropped.
- **Slash commands in the chat (card #94, 5 Nov 2026):** `slash_commands` gives the menu. Its source is Claude Code itself: `slash::claude_list` starts `claude.exe -p` (account env, card folder, Haiku, hooks off, nothing saved), reads the `system`/`init` line's `slash_commands` and `skills`, and kills it at once; cached per account and folder for the app's run (`CLAUDE_LISTS`), fetched in the background on the first call, which returns the disk scan (`slash::list`) with `complete: false` meanwhile. `slash::with_claude` merges it with `BUILTIN` (the commands only the terminal menu lists, with its descriptions, read off its `/` menu on 2.1.318) and drops what that menu hides (`mcp__` prompts, `HIDDEN`); `anthropic-skills:x` shows as `x`, as the terminal does; descriptions come from `BUILTIN`, the skill's SKILL.md (folded `>-` values joined) or `SKILL_DESC`. `/btw` never reaches the transcript, only the screen: each live session now feeds its PTY output to a `vt100::Parser` (`Live::screen`, resized with the PTY; `session_screen` returns it as text). `Sessions::ask_btw` types `/btw <q>`, sets `Live::btw` (every typing check goes through `Live::blocked`, and `nudge` skips the session), and `run_btw` waits for the overlay's footer to offer "c to copy" (`btw::view`), scrolls it with single Down keys (a burst reads as the scroll wheel and skips lines), merges the windows (`btw::merge`, which flags a gap), unwraps the terminal's line wrapping (`btw::unwrap`) and sends Esc, each key only while the overlay shows (`key_if_btw`). Answers live in `Sessions::btws` (memory only) and reach the panel in `session_info.btw`. A slash command typed by `type_at_prompt` now gets no `nudge` Enter (it sends no `UserPromptSubmit`, so that Enter answered a picker or closed the `/btw`), a 2.5 s hold, and `Live::dialog`, which blocks typing for 4 s (`DIALOG_DRAW`, so `nudge` can't clear it before the screen draws) and then while `prompt_showing` doesn't see the prompt box (a rule, a `❯` line, a rule; a picker's `❯ 1.` doesn't count), at most 30 min. Which commands open a screen changes between versions (`/cost` opens the Usage settings in 2.1.318), so it is read off the screen, not listed. `session_info.screen` says so; the panel opens the terminal for it (`slashSent`) and a queued message says it waits for that screen. `chat_command` takes `/btw` and refuses `/exit` before `back_to_work`; other slash commands skip `back_to_work`. `transcript::command_output` makes `<local-command-stdout|stderr>` a "cmdout" entry; 2.1.318 writes local commands as `system` lines (`local_command`: the command, then its output; `informational`: notices), which `parse_line` now reads. If Claude Code changes the overlay's footer or the prompt box's rules, fix `btw::view` / `prompt_showing` and their tests.
- **What a card waits on Hugh for (card #107, 6 Nov 2026):** `needs_you` adds `ask {kind, text, options, more}` from memory only: the session's `Waits` (prompt text, StopFailure `error`/`error_details`), its question, and a per-card reply cache keyed by the reply mark, filled from the Stop hook's `last_assistant_message` or once by a background thread from the transcript's last 256 KB. Everything passes `needs::redact`; the card-field texts go through its copy in the page, `hideSecrets` in `ui/needs.js` (change both together). A permission prompt's call is the last open call to the tool named in the Notification (`transcript::prompt_call`); a statusLine report sets the session back to working once that call has a result (`dispatch::prompt_check`, `transcript::call_settled`), since PostToolUse isn't hooked for every tool. No file is read under a lock; no migration. The UI (`ui/needs.js`, `needs.css`) adds `waitFor(c)` (the server's `ask`, then the card's own fields: Blocked, a stack card's question, a ship needing Hugh, Human test), drawn through the shared `boardHooks` (decorate, details, the `is:waiting` filter) and the header's "Waiting on you" button. The filter box ANDs its space-separated terms (`qTerms`, `qToggle` in `ui/board.js`).
- **Held places (card #132, 9 Nov 2026):** a card that needs Hugh is pinned to the top of its column (`pinFirst`, `ui/needs.js`); when its need clears it keeps that place instead of dropping back to `byOrder`. `loadNeeds` gives each card that left the needs map a hold `{at, col, made}` (its need's `at`, its column, when it was made) and drops the hold of a card that needs him again; `pinFirst` sorts cards that need him or hold a place by `at`, newest first, above the rest. A hold ends for good when its card is seen in another column, archived or gone (`pruneHolds`, a `boardHooks.rendered` hook, only once `loaded`), or entered its column after `made` (`columnSince`). No hold is made for a card that entered its column more than 30 s after its need began (`MOVED_SLACK`: Hugh moved it while it needed him; a session's own move at its turn end is within it), nor for one he dragged while it needed him (`noHold`, in memory). A needs read that a newer read overtook is dropped (`needsSeq`). The Human test queue (`ui/review.js`) uses `pinFirst`, so it follows the same order. No time limit: Hugh moved the card on without choosing one, so a hold lasts until a column move, a drag or a new need, as the card says. Holds live in `localStorage` (`wb.heldPlaces`, per WebView profile, so per channel), never in the store: no data change, no migration. `placeCard` (`ui/board.js`) now works on the column as shown (`pinFirst` applied); a drop among the pinned cards writes nothing: the dropped card and the held cards get holds whose times put them in the order shown, between the needs of the cards around them, which keep theirs, and no later than the newest time already there (`holdPlaces`; a card dragged in from another column is moved first). A drop just below the pinned cards treats them as absent (their orders say nothing about where they show); a neighbour with no order, or neighbours whose orders run backwards, numbers the whole column as shown; otherwise one midpoint write, as before. The dragged card loses its own hold (`isPinned` is the shared test), so a drop lands where it was dropped. Tests: `tools/ui-tests/order.test.mjs` (the harness's `boot` takes a `storage`).
- **Claude version:** the newest `claude.exe` is picked by asking each candidate its version (`~\.local\bin\claude.exe` is current; `~\bin\claude.exe` is too old).
- **What changed (card #108, 6 Nov 2026):** `src-tauri/src/changes.rs` reads a card's figures with read-only git (`-c core.fsmonitor=false`, `--no-ext-diff --no-textconv`, `GIT_OPTIONAL_LOCKS=0`, no window, 10 s kill) on one worker thread; `card_changes` (the panel) jumps the queue ahead of `card_changes_brief` (the board, one job per card). The board lock is held only to read the card and to write. `probe` tells a linked worktree (git-dir differs from git-common-dir) from a repo's main checkout: only a linked worktree's current branch counts, and only there is uncommitted work read. Branch figures are `git diff <merge-base branch main master origin/main origin/master>..branch`; otherwise, or when that is empty, commits on the branch (or the default) kept by `mentions_card` ("card #N", "cards #A, #N"). Results are cached per card by the shas. `on_entered_human_test` (hooked in `dispatch::after_move` and `after_stack_move`, since a session's own move goes through `session_move` and only reaches the latter) saves `changesSaved` (100 files, 20 commits). Strings that look secret are masked before they show or are saved, because the store refuses them. `changesOutputs` is cleaned in `cards.rs` (`clean_changes_outputs`); `changes_open` acts only on a listed path and opens only md, txt, png, jpg/jpeg and pdf. The UI (`ui/changes.js`) doesn't edit `board.js`: it uses #107's `boardHooks` and #105's `addCardPart` when they exist, else wraps `cardEl` and `renderDetail`; `changesHtml(c)` is the one renderer (#102's Human test box calls it) and only the first `.chg` box stays in the details.

## Open items

- 3.10 (built 4 Nov 2026, not yet tried on a real session): the panel header's model and effort dropdowns (`ui/live-switch.js`, command `set_session_model_effort`) record `modelOverride`/`effortOverride` on the card and type `/model <id>` / `/effort <level>` into a running session when it is next free.
- Resume offer (card #65, 4 Nov 2026): `Store::resume_offer` lists each card's latest session whose latest event is a quit or crash stop marked idle (`suspended` "idle", `orphaned` "idle: ...") after the setting `app.resume_offer_after`; `answer_resume_offer` moves that setting to the latest event. `begin_run` seeds it at the last run's start the first time (so an old board offers only its last run), and `copy_board` sets it past every copied event. The commands `resume_offer` and `resume_offer_answer` (`live_cmds.rs`) filter out archived, Done, running and queued cards, and resume through `begin_session`. The bar is `ui/resume.js` (styles in `ui/update.css`).
- Session cap (built 4 Nov 2026): at most `sessions.max_running` (default 10) sessions run; further starts (Hugh's, after a move, restart recovery) wait in `sessions.start_queue` and start oldest-first as places free (`get_session_cap`, `set_session_cap`, `session_queue`, `session_unqueue`; event `session-queue`).
- The deferred list in `PLAN.md`.

## Never take Hugh's focus (3 Nov 2026)

Hugh often has a full-screen game in front. **Every launch from a session uses `--background`**:
`Start-Process <exe> -ArgumentList '--background'`, with the debug-port variable when testing. The app then shows its
window behind everything without activating it (SetWindowPos, not ShowWindow). `install.ps1 -Now` and the update helper do this for both channels.
Opening the app from its shortcut, or a second launch without the flag, still brings it forward. The quit event is
cleared at start-up, so a copy started right after another quit doesn't quit too.

Card sessions' browser checks (card #77, 5 Nov 2026): each session's `mcp.json` overrides the account's
`chrome-devtools` server with the same command plus `--headless --isolated` (`sessions.rs` `headless_devtools`), so
the Chrome it drives has no window. A `--mcp-config` server beats a configured one of the same name (checked: the
page reports `HeadlessChrome`). It applies to sessions started after the update; a running session keeps its Chrome.
Since card #172 this app's own cards get no chrome-devtools at all (it is denied to them); other repos' cards
keep the headless override.

## Time in each column (6 Nov 2026, card #111)

- `crates/store/src/times.rs`. The source is `card_revisions` (every write goes through
  `write_tx`, which records the changed fields, `column` and `archived` included), not the log
  text. `timeline(revs, log, final_col, final_arch, now)` is the pure rebuild: revisions in `seq`
  order, a time earlier than the one before counts as no time, archived stretches and Done are
  skipped, and a new column (not a restore into the same one) starts a new `Visit`. The start
  column comes from the create revision's `column` (written by `create` since this card), else a
  log line matching `from <column> to <column>` anywhere in it whose "to" is the first column
  revision's, within `LOG_WINDOW_MS` (5 s), else the column now if the card never moved; else
  that stretch is `unknown_ms`. `imported` and `replaced` revisions make the column unknown the
  same way. `card_columns` gives the panel's totals; `medians` the board's, over finished visits
  that ended in the last `MEDIAN_DAYS` (30) days. `Store::card_times` and `Store::column_medians`
  read only the revisions that touch `column`, `archived`, `created`, `imported` or `replaced`
  (`json_type`), the log lines with "from ... to", and the cards (for `dryRun`): one pass each,
  joined in Rust. `card_revisions` has no index by card, so `card_times` scans the table too;
  fine at today's size (the test board of 500 cards x 50 revisions takes well under a second).
- `src-tauri/src/times.rs`: the read-only commands `card_times {cardId}` and `column_medians`
  (cached for 5 minutes).
- `ui/times.js`, `ui/times.css`: `timesSection(c)` (called from `renderDetail`, above the Log)
  draws from a cache keyed by card id and version and fetches `card_times` only on a miss;
  `timesHeads()` (called from `render`) sets each `.colhead`'s `title`; the page asks for the
  medians at most once a minute (the app's 5-minute cache is the only real one), and a failed
  `card_times` isn't asked again for the same card version. `timesTick` updates only the running
  column's `.tms` text each minute (so focus, the fold and tooltips stay); `timesRedraw` redraws
  the section when new times or medians arrive. The window's length comes from the store
  (`days` on each median), so the page never states it on its own. Rows: Planning, In progress, Human test, the rest in a
  `<details>` (`wb.timesOther` in localStorage keeps it open or shut).
- Tests: `crates/store/tests/times.rs`.

## The board face (6 Nov 2026, card #120, branch `dsh-120`)

- `ui/face.js` lays out each card's face in the mockup's order. It runs from `boardHooks.final` (new
  in board.js: after every decorate hook), moves nodes only, and skips stack cards (`.stk`). Its pure
  helpers are on `window.Face`. The chip row's order is `Face.CHIP_ORDER`; a chip it does not know
  goes after those, in drawn order. Meta tags carry `data-meta` (board.js) so face.js finds them.
- Data it reads: `columnSince` on each card (null skips the time) and `paths` in `card_changes_brief`
  (the Rust side of card #120); the stuck box uses `stuck.since`, `stuck.nudged` and, when present,
  `stuck.nudgedAt` / `stuck.notifyAt`. Face shots come from the latest `evidence` run through
  panel.js's `loadShot`, one `<img>` per shot kept in a Map, so a render never decodes again.
- `ui/filterbar.js`: `registerPill({key,label,tone,term,count,order,id,title,deps})`. Orders: Waiting
  on you 10, May be stuck 20, Router asks 30 (card #119), 30-second looks 40, labels 90. A file that
  loads before filterbar.js pushes its spec to `window.kbPills`. The row works out counts only when
  the cards, the filter text or a pill's `deps()` change, and touches a button only when its text
  changes. `#waitBtn` and `#checkSizeBtn` keep their ids, titles and `aria-pressed`, now as pills.
- `isSystemCard(c)` (board.js, global): `system: "router"` or number 0. Never drawn, never counted.
- needs.js draws the box only for question, asks, permission, failed and plan; Human test gets a
  "To test" chip; reply, blocked and ship keep the slim line.
- Tests: `node tools/ui-tests/run.mjs` runs every `tools/ui-tests/*.test.mjs` on the real ui files in
  a vm with a tiny DOM (`dom.mjs`, `harness.mjs`; no npm). `cargo test` runs it through
  `src-tauri/tests/ui_js.rs`, and skips with a message when node is not on PATH.
- Not matched to the mockup: the agent line is one line (the mockup wraps to two); the time sits at
  the end of the chip row; the wait box keeps the kind word ("Asks you", "Permission") as its label
  rather than "Waiting on you"; the choices are text only (no answer from the face).
- Contrast (WCAG): warn on warn-soft 4.60 light / 6.50 dark; ask on ask-soft 5.64 / 6.55; text on
  ask-soft 13.80 / 11.77; muted on panel 5.75 / 6.43. Keep warn text no lighter on warn-soft.

## The data side of card #120 (6 Nov 2026, branch `dsh-120`)

- **`columnSince`** on each card from `list_cards`: when it entered its column (from `Store::column_since_of`, the same timelines as `column_medians`). Cached in the `ColumnSince` app state against `Store::history_marker()` (max `card_revisions` and `card_log` seq), so it is worked out again only when the history moves. Null for archived, Done, or no history; an error is printed and leaves it null.
- **`Brief.paths`**: up to 3 changed paths (most changed lines first, forward slashes, secret-looking paths left out) for the face's "what changed" line.
- **`agents_feed`** (`crates/store/src/feed.rs`, command in `src-tauri/src/feed.rs`): the only source of the Agents feed and of #119's Router log. Query `{cards?, agents?, kinds?, since?, before?, limit?}`; result `{items:[{seq, at, card, agent, kind, verdict, field, text}], next}`, newest first, limit 50 (max 200). `kind` "log" is a `card_log` row; "verdict" is a change of route, critique, test, review, repro, merge or stuck in `card_revisions`. The agent comes from `by` only (`agent_of_by`); plain "the app" verdicts go by field. The agent filter is in SQL (`by IN (...)`).
- **No index yet.** The feed scans `card_log` and `card_revisions` ordered by `at`. Indexes on `(by, at)` were tried as migration 14 and left out because card #119 already takes 14 for its Router tables. Add them in a later migration once both cards are in, if the feed gets slow.
- **#119 note:** #119 now keeps the Router's log in its own table, not on card #0. To show Router lines in this feed, #119 adds that table as a third source in `agents_feed` with agent "router" (card 0).

### Final-test fixes (card #120, 6 Nov 2026)

- **Columns fill the board** (router's decision, as the mockup): `.col{flex:1 0 240px;min-width:0}`. Each column is at least 240 px and they share the rest of the width; when they don't fit, the board scrolls sideways as before. `min-width:0` stops a column growing to fit its widest content.
- **The agent line never sets a column's width:** `.card.face .face-agent{width:0;min-width:100%}`. Before, one long line in In progress made that column thousands of pixels wide.
- **Each verdict once in the Agents feed:** `agents_feed` leaves out a log row written in the same write as a verdict (same card, same `at`, same `by`, and the revision sets a verdict field); the verdict item stands for it. A verdict row names its field only when that field is not the row's own agent's ("PASSED", not "TESTER TESTER passed"; a tester setting repro still reads "REPRODUCER needed").

## The hang under load, and the lock watchdog (6 Nov 2026, card #122, branch `dsh-122`)

- **The hang.** The dispatcher (every 400 ms) held the board lock and called
  `notify::looking_at`, which asked the window `is_focused()`. In Tauri 2 that sends a message to
  the event loop and waits for the main thread. The main thread runs the sync commands
  (`needs_you`, `list_cards`, `session_info`), which wait for the board. Each waited for the
  other; every hook, status and tool call then queued on the board, and the window showed "not
  responding". It fired when a free session had route marks (`router::restart_if_due`); the
  Router's tools (`routertools.rs` `card_for_write`, `hands_off`) had the same chain.
- **The fix.** `looking_at` reads `FOCUSED`, an atomic kept by `notify::on_window_event` from
  `WindowEvent::Focused`. A test (`no_window_getter_off_the_main_thread`) refuses any window getter
  (`is_focused()`, `outer_size()`, ...) outside `place.rs` and `platform/macos.rs`, which run on
  the main thread.
- **The watchdog.** `Board` and `Sessions.live` are `lockwatch::Watched` mutexes: each lock records
  its call site and thread. A watchdog thread writes to `locks.log` in the data folder (and stderr):
  a hold over 5 s with its site and the sites of its waiters (again every 30 s while it lasts), a
  hold over 5 s when it ends, and `DEADLOCK` for a cycle between watched locks. A wait on the main
  thread (as in this hang) shows as a long `board` hold with `main` among the waiters.
- **Stress.** `hang_tests.rs` models the load (a main thread, six API pollers, UI events and the
  dispatcher) with the real lock type: the old focus check hangs, the new one never does.
  `node tools/stress-api.mjs [--channel test] [--seconds 60] [--workers 12] [--card N]` loads a
  running app's API with read-only tool calls and fails if a call takes over 5 s.
- **Not changed, worth knowing.** PTY writes (`Sessions::input`, `nudge`) run under `live`, and from
  the dispatcher and `session_send` under `board` too: a stalled ConPTY input pipe would freeze
  both. `claude_exe()`'s first call runs `claude --version` under the same locks. The watchdog
  would name either.
- **The terminal (card #95, 9 Nov 2026):** the card panel and the Router pane draw their xterm through `ui/termlink.js`. It replays `session_snapshot` once per open and again only when `session_run` names a new run, so a refresh (each hook, send or `session-changed`) no longer wipes the screen; before this, every refresh did `term.reset()` and replayed up to 512 KB, and xterm's answers to the queries in that old output were typed into the session. `Ring::push` counts every byte the session puts out; each `pty` event carries `run` and `end` (the count after its chunk) and the snapshot its own `end`, so chunks that arrive while the snapshot loads wait and those it holds are skipped. Keys are dropped only while a replay draws (`L.replaying`, until xterm's write callback). A key with no session, or one `session_input` refuses, writes a dim note in the terminal (at most one per 4 s). Opening the terminal focuses it. Tests: `ring_counts_output_and_snapshot_ends_where_events_do` and `tools/ui-tests/termlink.test.mjs`.

## Column widths you can drag (9 Nov 2026, card #130, branch `dsh-130`)

- **What it does.** Drag the right edge of a board column (a thin strip in the gap) to change its width. The column takes width from its right neighbour while that has room; a neighbour already at the minimum stays there and the column still widens, so the board scrolls further. Press an edge twice within 400 ms to put every column back to equal width. At 640px or less the edges are hidden and columns are 78vw, as before.
- **How it is kept.** Each column has a weight, the smallest being 1, in `localStorage` `wb.colWeights` (`{"<column>": weight}`, the Archive and the Done pile included). `ui\colwidths.js` sets it as `--w`; `.col` is `flex: var(--w) 0 calc(var(--w) * 160px)` with `min-width: 160px` (`COL_MIN`, in both files). A column is weight x 160px when the board overflows, and weight x board / sum of weights when there is room, so the shares hold when the window changes. The weights are shares, not pixels: widths set in a wide window scale down in a narrow one.
- **The minimum is 160px** (it was 240px, so at a 1440px window the board scrolled and nothing could scale). Eight columns need 1350px.
- **Tests.** `tools\ui-tests\colwidths.test.mjs` (drag rule, weights read and saved, `--w` on every column, dead zone, redraw mid-drag, double press); the fake DOM gained `style.setProperty`, pointer-event fields, a settable width, window listeners and a working `localStorage`. `node tools\cdp.mjs viewport <w> <h> <js>...` holds one connection with an emulated page size (the real window's minimum is 720px).

## Agents drawer beside the Router panel (9 Nov 2026, card #167)

UI only. With both open, the Agents drawer floats to the left of the Router panel (same top and bottom, 16 px gap) instead of lying over it, and the board and a card panel keep clear of both. The drawer has its own width, `--apane-w` (380 px); `--rpane-w` is the Router panel's. `agents-open` and `router-open` on `<body>` say which is open (`rpane-open` stays while either is), and `--pane-used` is their total, which the board's right margin and a card panel's box use. Closing one no longer removes the other's room.

## Card-merger accepts a switched-off reviewer (9 Nov 2026, card #176)

The card-merger's prompt (`merger.rs`, the stack job's step 1 and the integrate job's step a) counts a test or review as met when that subagent is off for the card (`subagents.tester` / `subagents.reviewer` false in `kanban_get_card`), as the app's gates do. Before, a stack child with the reviewer off (the board's default since card #162) could never be merged. The lead's merge brief (`stackrun.rs`) says "(or has them switched off)".


## Proof in Human test (9 Nov 2026, card #125, branch `dsh-125`)

- **Store:** `proof` (`PROOF_KEY`, `clean_proof` in `crates/store/src/cards.rs`) shares
  `clean_path_list` with `changesOutputs`; allowed in `tools.rs` UPDATABLE. Tests:
  `crates/store/tests/proof_field.rs`.
- **App:** `src-tauri/src/proof.rs`. `find_in_text` (prose lines only: fenced and indented code
  blocks, code spans, and Markdown link and image targets are skipped; a bare sibling name takes
  the nearest full path's folder on its line and must exist), `proof_items` (the `proof` field,
  then the hand-over's; states ok, missing, refused, dup), `check_read` (resolved paths; home or
  temp; no dot folder bar `.claude\worktrees`; `platform::in_settings_folder`), `decode_doc` (BOM,
  whole-character cut, NUL check after decoding, cut at whitespace, then `search::mask`), and the
  commands `proof_list`, `proof_read`, `proof_open` (each reads the card under the lock, then the
  disk). A dup is an image whose path or bytes match a strip shot (latest evidence run, the
  reproducer's before and after).
- **UI:** `ui/proof.js` (+ `proof.css`), loaded after review.js. review.js calls
  `proofStripFigs`, `proofSectionHtml`, `proofSteps` (via `htStepsHtml`) and `proofFill` from
  `htBox`. Kept per card while the panel shows it: the items (asked again when the card's version
  changes), picture data URLs and drawn reports. Proof pictures never go through `image_read`
  (no `data-local`). `docHtml` sanitises with an allow-list and strips non-http hrefs. panel.js:
  `shotName`, so tooltips, failures and the viewer's caption show file names.
- **Tests:** proof.rs's own (11), `tools/ui-tests/proof.test.mjs` (chips, code, links, the kept
  state). `docHtml` needs a real DOM, so it is checked in the test app over CDP.

## Open any running session the way the Router opens (9 Nov 2026, card #129, branch `card-129`)

- **The session pane is the Router panel** (`ui/router.js`): `openSessionPane(src)` shows the Router, a merge lane (`{kind:"lane", key}`), a card session (`{kind:"card", cardId}`), a subagent (`{kind:"sub", cardId, session, id}`) or an outside session (`{kind:"outside", id}`) in the same docked panel, one at a time; opening another swaps it (`closePane(true)` keeps the board's margin). Built in place of the planned `session-view.js`/`session-pane.js` split, because card #167 already lets the Agents drawer sit beside this panel. Router-only parts (digest, Start/Stop, trust question, `router_send`, toasts held) run only for the Router. A card session sends with `session_send`, answers with `session_answer`, attaches with `attachment_save` (its card id); the rest are read only, and a dropped file on them says so. One terminal per session: `termTaken()` keeps the pane's xterm off a card whose card panel shows its terminal (checked every second and on each state read).
- **The agents along the top, then Running now** (Hugh, 9 Nov: "the summary as is and different agents along the top that i can jump into"; `ui/agents-feed.js`, `runningHtml`, `runningSource`, `runningKind`, `lanesOf`): two rows of tabs at the top of the Agents drawer. The agents: Router, a Merger per lane (`lanesKnown`: every lane `lanes_seen` lists, running or off), Critic, Reviewer, Tester, Reproducer (the running one, else the latest the feed names, via `openSessionFromFeed`), and Monitor (no conversation: it toggles the feed to `{agents:["monitor"]}`). Running now: card sessions, their running subagents and outside sessions, from `sessions_running` (`src-tauri/src/running.rs`), polled every 10 s (5 s while the drawer is open); the count is on the Agents button. Running subagents: not finished, not stopped, written within the monitor's interval with a 10-minute floor (`running_subagents`). Merge lanes come from the live map's `lane:<repo>` keys; `card_transcript` now reads a lane's latest session for `transcript_read`.
- **Outside sessions:** `kb` sends `X-Claude-Session` (from `CLAUDE_CODE_SESSION_ID`) when it isn't in a card session (`crates/kb/src/api.rs` `caller_headers`); `api.rs` notes it on `/mcp/<token>/external` and `/tool` with no card session (`running::note_call`: id, the cards the call named, time; memory only, 50 kept, a day at most). Listed while the last call or the transcript's last write is within 15 minutes, never for an id in this board's `sessions`, the Router's or a lane's. `outside_transcript_read` reads only a listed id. Only a `tools/call` on the MCP route counts (not `initialize` or `tools/list`, which any session with `kb mcp` configured sends). Limit: a `kb mcp` process keeps the session id it started with, so after a `/clear` its calls still name the old conversation; a `kb` command picks up the new one. `sessions_running` lists both accounts' transcripts once per poll (`Index`), keeps each transcript's head once it has a title, and reads only ids (`own_claude_ids`: card, Router and lane conversations) and one card's rows (`card_sessions`).
- **Feed rows open sessions:** a verdict item carries `agent_id` (from the verdict record's `agentId`, `crates/store/src/feed.rs`); `AgentsFeed.sessionTarget` says what a row opens; `subagent_find` looks through every `sessions` row of the card, its desktop conversation, then its stack lead's (`pick_subagent`: by id, else by type and the row's time with a minute's grace); `subagent_read` reads it, refusing a session that isn't that card's or its lead's. Merger rows (the merge lane), monitor and Hugh rows aren't buttons.
- Tests: `running.rs` unit tests, `kb` `caller_tests`, `crates/store/tests/feed.rs` (`agent_id`), `tools/ui-tests/feed.test.mjs` (row targets, Running now).
- **The open tab is marked** (card #173, on top of #129): `runningHtml(r, {open})` takes what the session pane shows (`window.sessionPaneShows()`) and `openMatch` marks its tab (`.on`, `aria-current="true"`, "· open now" in its title; `agents-feed.css` `.rn-tab[aria-current]`): the Router, a lane by `key`, a card by `cardId`, a subagent by `id` plus its kind's agent tab, an outside session by `id`. `router.js` sends a `session-pane` window event when the pane opens, swaps or closes, and the drawer redraws its tabs. Test: `feed.test.mjs`, "the tab of what the session pane shows is marked".

## An agent asks to ship a card for Hugh (10 Nov 2026, card #184, branch `card-184`)

- **Never a move:** `kanban_move_card` to Ship it is still refused for every agent, and the refusal
  (`shipask::HOW_TO_ASK`) says how to ask. With `for_hugh: true` and a `reason` (Hugh's words) it
  calls `tools::ask_ship` instead: a log line "<who> asked Hugh to ship this card for him", a
  `shipask::Ask` kept in the app's `ShipAsks` state, and a `ship-request` event (banner in
  `ui/panel.js`, toast in `notify.rs`; neither takes focus). `kb move <n> "Ship it" --for-hugh "<words>"`
  and `kb ship <n> --asked-by-hugh [--reason r]` reach the same function; the old refusal of a card
  session's `kb ship` is gone. The Router is still refused (`routersession::move_refused` first,
  and `as_router` here).
- **Hugh's click:** the banner's Ship calls `approve_ship_ask {card, ask, version}`: `ShipAsks::take`
  refuses a used, dismissed, replaced, expired (10 min, `TTL_MS`) or other card's request, then
  `hugh_move` moves it as his drag, and the log gets "Hugh approved the request to ship this
  card from <who> (asked <time>)". A refused move (conflict) puts the request back. Dismiss calls
  `dismiss_ship_ask` and logs it. `ship_asks` lists the open ones so the banners come back after a
  page reload. One request per card (a new one replaces the old); memory only, so a restart drops them.
- **Tests:** `shipask::tests` (none, once, expired, other card, replaced, restore and dismiss, the
  refusal text), `tools::tests::t4_6_ship_request_needs_the_flag`, and kb's
  `move_for_hugh_asks_with_his_words`.

## Drag the line between the live log and the chat (11 Nov 2026, card #185, branch `card-185`)

- **`ui/split.js`** (`window.PaneSplit`, loaded before `router.js`): `attach({handle, grid, feed, right, kind})` wires `#r-split`, the `role="separator"` strip at the bottom of the session pane's `.rpane-left`. A drag (pointer events, its `pointerdown` default prevented so the focus and caret stay in the message box) or Up/Down (Shift for 64 px), Home and End on the focused handle set `--log-h` on `#r-grid` and the `split-set` class; `router.css` then gives `.rpane-feed` that height in place of its 40% cap, and the chat takes the rest. Limits: the log at least 60 px, the chat at least 160 px (`clamp`). Kept in localStorage per panel type: `rpane.split.router` and `rpane.split.session` (every other kind); a double-click clears it. A `ResizeObserver` on the grid and `drawFold` call `refit()`, which clamps the saved height to the room there is now, and drops it while the handle is hidden (folded log, or side by side from 618 px, where it does nothing).
- **Tests:** `tools/ui-tests/split.test.mjs` (limits, keys, drag, saved per type, double-click, refit).

## The Agents drawer and the Router panel open at start (11 Nov 2026, card #189, branch `card-189`)

Hugh, 11 Nov: "Agents and router should open automatically on start." UI only.

- **`ui/startup.js`** (loaded last, after `agents-feed.js` and `router.js`): once the page has loaded, `openPanesAtStart()` opens the Agents drawer (`agentsPane.open({focus:false})`, so the focus stays on the board and its keys work) and then the Router panel (`openRouterPanel()`), the drawer floating to its left as card #167 lays them out. It opens nothing already open and never swaps out a session the pane already shows. Each panel's ✕ still closes it; nothing is remembered, so every start (and every page reload) opens them again.
- **The Router's toasts are held while its panel is open** (`router_panel_opened`, final-test F7), so with the panel open from the start they show in the panel, not as notifications, until Hugh closes it.
- **Tests:** `tools/ui-tests/startup.test.mjs`.

## The Router ships on Hugh's typed word (11 Nov 2026, card #188, branch `card-188`)

- **The proof is his typing.** Every message Hugh types in the Router panel (`router_send`, so
  `routersession::send`) is already kept in `router_log` as kind `event` by `Hugh (panel)`; its
  `seq` is the message's id. Nothing else writes that pair: `kb router` is `Hugh (kb)`, a card's
  `kanban_router` message is `the app`, the app's events are `the app`, the Router's own lines are
  `the router`. `kanban_router_log` with `hugh: true` lists his messages with their ids.
- **The move.** The Router's `kanban_move_card` to "Ship it" goes to `routertools::ship_on_word`
  (its schema drops `for_hugh` and adds `hugh_message` and `hugh_words`). `ship_on_word_refused`
  checks: the card's version, In progress or Human test, not a stack's child (a lead only when
  every child is ready), `hughword::check` (the message is his panel typing, at most 30 minutes
  old (`WORD_FOR_MS`), holds the quote, has ship/deploy/"all the way"/"skip human testing" and no "don't ship", and
  names the card's number, or a group word ("all", "these", "both", ...) with the card named in
  the Router's last 5 replies; not used for this card before, by the marker "message m<id>)" in
  the card's log), then the card session's subagent gates (`agents::move_refused`; from In
  progress, those of the move to Human test, so its review and test must have passed). A refusal is
  logged on the card. Then `Store::move_on_hughs_word` (new `How::Word`, by `WORD_BY` "the router
  (on Hugh's word)", ship record `"word": true`), the log line "Router shipped it on Hugh's word:
  '<quote>' (<time>, message m<id>)", a Router decision line, and `dispatch::after_router_move`
  (the ship starts, or waits its turn).
- **Take it all the way.** Hugh's "take #n all the way" (or "skip human testing") about a card
  not ready to ship: the Router's `kanban_update_card` with `fields: {"autoTo": "Deploy complete"}`
  alone, citing his message the same way, goes to `routertools::all_the_way_on_word`
  (`all_the_way_refused`: only Deploy complete, the version, Backlog to Human test or Blocked, not
  a stack's child, then `hughs_word`). It sets the card's own Take it to (by `WORD_BY`), logs
  "Router set it to go on by itself to Deploy complete on Hugh's word: '<quote>' (...)", and runs
  `dispatch::auto_progress`, so the app moves it into Ship it for him once it is in Human test
  (through its subagent test), however long the build takes. The 30 minutes are for the Router to
  act on his word, not for the build. A tester run found the first version, which shipped only
  inside the 30 minutes, too short for a build and test.
- **Unchanged:** with no matching message the move is refused as before; card sessions and other
  agents keep #184's confirm click; `routersession::move_refused` still refuses Ship it on the
  Router's ordinary path.
- **Prompts:** rule 1 of `prompts/router.md` (and the private copy), `api.rs`'s
  `ROUTER_INSTRUCTIONS`, the move tool's description (`ROUTER_MOVE_DESCRIPTION`).
- **Tests:** `hughword::tests` (no message, a matching one, from a card or session, another card,
  all/these, the time limit, no quote or ship word or "don't ship", once per card, numbers) and
  `routertools::tests::a_ship_on_hughs_word_*` (the store path: gates, columns, the move and its
  log).

## Back and Forward between panels and views (11 Nov 2026, card #187, branch `card-187`)

- **`ui/nav.js`** keeps one history of what is on screen: a view is the card panel (its card, its
  subagent tab, and in the Human test review whether it shows the review or the chat), the stack
  overlay (lead and child), the session pane (`sessionPaneShows`), the Agents drawer
  (`agentsPane.isOpen`) and Settings (`settingsShows`, new in `settings.js`). It starts at the
  board (nothing open), and keeps up to 100 views.
- **Recording:** every opener and closer calls `navNote()`: `openPanel`, `closePanel`, `showView`
  (`panel.js`), `htView` (`review.js`), `stkKidOpen`, `stkDlgClose` and the overlay's close event
  (`stacks.js`), `openPane` and `closePane` (`router.js`), the Agents drawer's open and close
  (`agents-feed.js`), `openSettings` and `closeSettings` (`settings.js`). The view is taken in a
  microtask, so "close this, open that" in one click is one step, and a view the same as the last
  isn't added. A new view after Back drops the forward steps.
- **Back and Forward** (`Nav.back`, `Nav.forward`) close what the target view hasn't and open what it
  has, through those same functions (panel before the stack overlay, which is modal; Settings last).
  A view whose card is gone or archived is skipped. At either end nothing happens. While it restores,
  `navNote` is ignored.
- **Keys:** mouse buttons 3 and 4 (`mouseup`; the press and `auxclick` are stopped too, so the
  WebView's own page history never moves), always, even with the focus in a box. Alt+Left and
  Alt+Right and the BrowserBack/BrowserForward keys only when the focus isn't in an input, a
  textarea, a select, anything editable or the terminal. All listeners are on `window`, capture.
- **Typed text:** leaving the card panel or the session pane by Back or Forward keeps the message
  box's unsent text (`Nav.takeText`), and it comes back when that view does (`panel.js`'s
  `compose` puts it in the new box; the pane's box gets it straight away).
- **Tests:** `tools/ui-tests/nav.test.mjs` (nav.js with a stand-in for the panels): empty history,
  stack lead to child (overlay, and the child's own panel) and back, the review to the card's chat
  and back, stepping the review, forward, a new view dropping forward, pane, drawer and Settings,
  an archived card skipped, typed text kept, the keys and buttons, one step per click.

## Plan review (12 Nov 2026, card #195, branch `card-195`)

Hugh (12 Nov): a column for his review of a plan, between Planning and In progress.

- **The column:** `COLUMNS` (store `lib.rs`, `ui/board.js`) has "Plan review" after Planning.
  **Migration 18** (`schema18.sql`) moves the later columns up one and inserts it at position 3:
  mode `none`, no auto-spawn, no prompt, `on_enter` `suspend` (the planning session stops when its
  held move lands there; the conversation is kept). No card changes. Idempotent; an older build
  opens the board and draws a card there in Backlog (`colOf`).
- **The stop:** `Store::plan_review_stop` (flow.rs): a move from Planning into In progress, Human
  test or Deploy complete stops in Plan review unless the card skips it (`skipPlanReview`, default
  `sessions.skip_plan_review`, off), has Take it to (`autoTo`, his approval), or is a stack's child
  (its lead's approval). `dispatch::plan_review_stop` applies it to `session_move_to` (a card
  session's held move, another session's move, the Router's) but never to Hugh's own (`hughs_own`)
  or the app's small-card go-on (`router::BY`, card #98: Hugh's own rule still starts a small card
  building). The log says "Stopped in Plan review on its way to ...". Skip Planning still goes
  Backlog to In progress, so it skips Plan review too.
- **Out of it:** `dispatch::plan_review_gate` is first in `agents::GATES`: a card session or the
  Router moves a card out of Plan review only back (Planning, Backlog, Blocked, Done).
  `routersession::move_refused` refuses the Router's move out of it and lets Planning to Plan
  review through. `critic::INTO_BUILD` has Plan review, so the plan critique holds the move into it.
  Hugh's move to In progress approves (`move_message` adds `plan_review_note`, and the critic's
  build notes go with it, `build_notes_for`); his move back to Planning is a fresh plan request
  with a note to read his notes.
- **Flow:** `skipPlanReview` is the third skip in `SKIPS` and `FLOW_FIELDS` (card field, Settings
  default, `CARD_FLOW_SKIPS` in `ui/subagents.js`, `kb update --skip-plan-review`,
  `skips_changed_note`, `after_default_change` tells the cards in Planning that follow it).
- **For agents:** `kanban_flow_defaults` (`control.rs`, outside a card session only; `kb
  flow-defaults [--skip-planning on|off] [--skip-plan-review on|off] [--skip-human-test on|off]`)
  reads and sets the three skip defaults as Settings would (logged as Hugh's session, with
  `after_default_change`). Per card, Hugh's own sessions already set the skips with
  `kanban_update_card`.
- **UI:** a card in Plan review shows "Plan to review" (`ui/needs.js`, `w-review`); the column is
  in the time views (`ui/times.js`).
- **Prompts** (private repo, branch `card-195`): `planning.md` (*Plan review*), `in-progress.md`
  (step 1a, build the approved plan), `router.md` (job 2), the kanban skill's Project #2 table
  (Plan review maps to In progress). The bundled `prompts/planning.md` says the same in short.
- **Tests:** `crates/store/tests/plan_review.rs` (the column, migration 18 with backup and twice,
  an older build, the three-way skip, the stop rules, Hugh's moves out); `dispatch.rs`,
  `routersession.rs`, `critic.rs` and `control.rs` unit tests; `kb` flag tests.
