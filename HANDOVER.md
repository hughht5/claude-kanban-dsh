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

## Updates (4 Nov 2026)

- **Live default is `-Stage`:** `scripts\install.ps1` builds and copies the NSIS installer to `%USERPROFILE%\.claude-kanban-dsh\update\` (test: `.claude-kanban-dsh-test\update\`) with `update.json` (`version`, `commit`, `built_at`, `installer`, `install_dir`, `app_exe`, `channel`). It never quits the app. `-Now` is the old quit-install-restart; it is still the test channel's default, and `-Channel test -Stage` tries the staged flow on the test copy. A first install must use `-Now`.
- **Build identity:** `src-tauri/build.rs` compiles in `KANBAN_GIT_COMMIT` and `KANBAN_BUILT_AT` (install.ps1 sets both and writes the same into `update.json`; a plain `cargo build` takes HEAD and the build time).
- **The app** (`src-tauri/src/update.rs`) reads the folder at start and every 30 s. A staged `built_at` later than its own shows "Update ready" along the top of the board (`ui/update.js`, `ui/update.css`). Later hides that build until the next start or a newer one.
- **Restart now** starts a hidden, detached PowerShell helper (outside the app's job if Windows allows breakaway), then runs `Sessions::stop_all` and exits, as tray Quit does. The helper waits for the app's pid to exit (up to 5 min), runs the installer with `/S /D=<install_dir>`, then starts `app_exe` (not `--background`: Hugh clicked, so it opens in front) whatever the installer did. A start that isn't `--background` forces the window to the front with `place::bring_forward`, because Windows refuses `SetForegroundWindow` to a process the foreground one didn't start. Restart recovery resumes the sessions that were mid-turn. It logs each step to `update\restart.log`.

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
`Live::untaken`, logged on the card. While a message is unconfirmed or untaken, nothing else is
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
- **Prompts:** `in-progress.md` "Test it with a subagent", `kanban-app.md` step 6a.
- **Tests:** `tester.rs`'s unit tests; `sessions.rs` `card_sessions_get_the_tester_subagent` and the
  hooks test; `crates/store/tests/auto.rs` (the reset and the setting).

## Dev-stack health on logins (4 Nov 2026)

For a repo whose dev stack is its own `scripts/dev-stack.sh`, run inside WSL from the branch's checkout. `stack_health` (`src-tauri/src/stack.rs`, `ui/stack.js`) runs `launch::stack_up`. It reads the branch with `git -C <worktreePath> rev-parse --abbrev-ref HEAD`, using Windows git with no window, and only on a Windows-drive checkout. Otherwise it uses the card's `branch`, and it flags a card `branch` that differs from the checkout's. When the stack is down, **Restart stack** shows `cd <worktree as /mnt/c/...> && scripts/dev-stack.sh restart <branch>` to paste into a WSL terminal. The app never runs it (no WSL from the app).

## Test version

- **What it is:** the same code built with `KANBAN_CHANNEL=test` (read at compile time by `kanban_store::channel()`) and the overlay `src-tauri/tauri.test.conf.json` (productName "Claude Kanban DSH Test", binary `claude-kanban-dsh-test.exe`, identifier `io.github.hughht5.claude-kanban-dsh.test`, hooks `windows/hooks.test.nsh`). Data in `%USERPROFILE%\.claude-kanban-dsh-test`, quit event `Local\ClaudeKanbanDshQuitTest`, build output in `target\test`, install in `%USERPROFILE%\Programs\Claude Kanban DSH Test`, not on PATH. The board shows a `--warn` bar (`app_channel` command).
- **Install or refresh it:** `scripts\install.ps1 -Channel test` (quits and restarts only the test app). The first install copies the live board in with `kb copy-board`; `-FreshCopy` replaces the test board with a new copy (the old one goes to its `backups`).
- **Swap a change in:** try it on the test copy, then run `scripts\install.ps1` with no `-Channel` from the same commit. That stages the update; Hugh clicks Restart now when it suits him (or add `-Now`). The live build clears `KANBAN_CHANNEL` and `CARGO_TARGET_DIR` itself.
- **By hand:** `KANBAN_CHANNEL=test CARGO_TARGET_DIR=target/test cargo tauri build --config src-tauri/tauri.test.conf.json`. Never set `KANBAN_CHANNEL` for a live build.
- **Copied sessions:** `copy_board` records each session still running on the source as suspended ("copied", not mid-turn) in the copy and moves `app.run_started_seq` past them, so the test app never resumes a live conversation.

## Decisions taken (beyond PLAN.md)

- Plain JavaScript UI with no bundler; libraries are vendored into `ui/vendor` by `ui-deps/vendor.mjs`.
- **Private folder:** the column prompts and the repo list aren't in the code. `src-tauri/src/private.rs` reads the setting `private.dir` (Settings › Sessions › "Private folder"). `{prompts}` in a prompt or template stands for that folder's `prompts\` if it exists, else the bundled `prompts\` examples (beside the installed exe, or this repo's). Its `repos.json` (`[{"repo":"owner/name","folder":"~\\name","label":"name"}]`) gives the Add card form's repos and each repo's session folder; a repo not listed starts in the home folder. See README, "Private folder".
- Card fields other than number, column, order and archived live in a `fields` JSON column, under the artifact's own names.
- Done is archived (card #83, 5 Nov 2026): `write_tx` archives any card a write puts in Done (with `archivedFrom`, a log line, and the lead's stack via `stack_archive_children`), `create` archives a card made in Done, and `archive(on: false)` sends a Done card back by `restore_patch`. Migration 10 archived the cards already in Done. The board draws no Done column (`BOARD_COLS` in `ui/board.js`); the Archive column's drop box moves a card to Done through `move_card`, so `after_move` stops its session as before.
- **Append-only:** logs, revisions, sessions and session events take inserts only; there is no `DELETE FROM` anywhere (a test checks this); triggers refuse deletes.
- **Sessions:** started or resumed only by Hugh's click or a column's auto-spawn. A card whose conversation is in the desktop app is never auto-started.
- **Accounts:** the account comes from the session row, because Account 2's `projects` folder is a junction to the main one. A resume on a card pinned to the other account is refused.
- **Readiness:** `SessionStart` doesn't arrive as an `http` hook, so the statusLine's first report (`kb status`, posted to `/status`) marks a session ready. The app's own text is typed only when the session is ready, idle and has been quiet for 1.5 s. Hugh's messages (panel box, `kb send`) are typed at once whenever `sessions::takes_typing` allows (ready; idle, working or failed; nothing held), and the box is shown with no session too: Send starts or resumes it (card #62, 4 Nov 2026). The `AskUserQuestion` tool's Pre/PostToolUse hooks mark its dialog as `permission`, so nothing is typed into it.
- **"Testing" chip (card #86, 5 Nov 2026):** the session's `PreToolUse`/`PostToolUse` hooks match `AskUserQuestion|Agent|Task|Skill`, and it sends `SubagentStop`. `testing::Testing` (one per live session) counts a foreground review or test call from its Pre to its Post (by `tool_use_id`), and a background one (its Post carries `tool_response.agentId`) until that agent's `SubagentStop`, capped at 3 h. An inline skill (its Post has no `agentId`) and a review or test slash command typed as the prompt (`UserPromptSubmit`) count until the turn's `Stop`; a call made inside a background subagent (its hook carries `agent_id`) until that agent stops. What counts: `testing::is_testing_call`, a word starting "test", "review" or "verif", or "qa", in a skill's name or a subagent's description or type (never its prompt), unless the description starts with a build verb ("Build the testing chip"). The app emits `testing` `{card, on}`; the board reads `session_testing` on load and on `session-changed`. `buildingChip` shows Testing while working or idle.
- **"Working…" line (card #66, 4 Nov 2026):** each live session keeps `turn_since`, set when the hooks first say `working` (or `permission`) and cleared at idle, failed or ended (`sessions::turn_start`); `session_info` returns it as `turnMs`. `transcript_read` returns `thought`, the chunk's latest thinking block timed from the line before it (looking back up to 1 MB before the read's offset). The panel draws them under the conversation, ticking every second.
- **Messages:** one waiting message per card, with new text appended; one held move per card, with the newest winning.
- **Port:** the API falls back to an OS-picked port, because 47821 can't be bound on this machine.
- **Questions in the chat (card #78, 5 Nov 2026):** `transcript::ask_questions` puts an `AskUserQuestion` call's questions on its entry (`ask`), and the panel draws them as a question card (`askHtml` in `ui/panel.js`). The `PreToolUse` hook's `tool_use_id` and `tool_input` are kept on the live session (`question`, cleared when it leaves `permission`); `session_info` returns the id as `question`. `session_answer` checks the id and types `sessions::answer_keys`: in a single-select question a digit picks and moves on; "Type something" is the digit after the options, a paste and Enter (1.2 s before the next key, or it lands in the typed text); in a multi-select digits tick and Right moves on; with more than one question, or any multi-select, a review screen follows where `1` submits. Learnt on Claude Code 2.1.318: if the dialog changes, fix `answer_keys` and its tests.
- **Screenshots in the chat (card #84, 5 Nov 2026):** `image_read` (`session_cmds.rs`) returns a local image as a `data:` URL: absolute local paths only (no UNC, so no SMB connection), image extensions from `transcript::image_mime` (the one list; `IMG_EXT` in `ui/panel.js` must match), at most `MAX_ATTACHMENT`. `transcript::parse_line` puts a `Read` of an image on its entry as `images`. In `ui/panel.js` a DOMPurify hook (registered once) turns a local `<img src>` in Markdown into `data-local`; `showShots` loads what is on view (not inside a closed `<details>`), cached per path and message time (so a shot saved again under the same name shows afresh in a later message), up to 150 MB; failures aren't cached. The sessions learn the `![..](C:/..)` form from `api.rs` `INSTRUCTIONS` and the column prompts.
- **Subagents in the chat (card #91, 5 Nov 2026):** Claude Code keeps each subagent of a session in `<session id>\subagents\agent-<id>.jsonl` beside the transcript, with `agent-<id>.meta.json` (`agentType`, or `name` for a forked skill; `description`; `toolUseId`, the Agent call that started it; `finished` once it has reported back). `transcript::subagents` lists them (an id must be letters and digits, so nothing outside the folder is read); `read_in(.., sub: true)` keeps the sidechain lines the main reader drops. The panel polls `subagent_list` every 3 s, draws the strip (`renderSubs`), and `showView(id)` re-reads from offset 0 with `transcript_read`'s `agent`. Running means not `finished` while the card's session is up; `finished` never comes if the session was killed, so that reads Stopped. `transcript::task_notice` turns a `<task-notification>` user line into a "task" entry (summary, task id in `tool_id`, status in `input`). Learnt on Claude Code 2.1.318: if the folder or the meta fields change, fix `transcript::subagents` and its test.
- **Interrupted turns (card #78):** Esc, or turning down a permission prompt or question in the terminal, ends the turn with no `Stop` or `PostToolUse` hook, which left the card on "Needs you" or "Working" until the next prompt. On each statusLine report for a session on a turn, `dispatch::on_status` reads the transcript's last 64 KB (`transcript::turn_ended_in`); a `system`/`turn_duration` line (or Claude Code's "[Request interrupted by user" line) with nothing of the main conversation after it, later than the state began or the latest `UserPromptSubmit` (`sessions::ended_by`), sets the session idle. The 400 ms loop checks again every 20 s for sessions whose state hasn't changed for 10 s (`quiet_turns`), in case no statusLine report came after the end was written. `session_answer` stops typing as soon as the session leaves `permission`, so its keys never reach a later prompt; a typed answer's control characters are dropped.
- **Slash commands in the chat (card #94, 5 Nov 2026):** `slash_commands` gives the menu. Its source is Claude Code itself: `slash::claude_list` starts `claude.exe -p` (account env, card folder, Haiku, hooks off, nothing saved), reads the `system`/`init` line's `slash_commands` and `skills`, and kills it at once; cached per account and folder for the app's run (`CLAUDE_LISTS`), fetched in the background on the first call, which returns the disk scan (`slash::list`) with `complete: false` meanwhile. `slash::with_claude` merges it with `BUILTIN` (the commands only the terminal menu lists, with its descriptions, read off its `/` menu on 2.1.318) and drops what that menu hides (`mcp__` prompts, `HIDDEN`); `anthropic-skills:x` shows as `x`, as the terminal does; descriptions come from `BUILTIN`, the skill's SKILL.md (folded `>-` values joined) or `SKILL_DESC`. `/btw` never reaches the transcript, only the screen: each live session now feeds its PTY output to a `vt100::Parser` (`Live::screen`, resized with the PTY; `session_screen` returns it as text). `Sessions::ask_btw` types `/btw <q>`, sets `Live::btw` (every typing check goes through `Live::blocked`, and `nudge` skips the session), and `run_btw` waits for the overlay's footer to offer "c to copy" (`btw::view`), scrolls it with single Down keys (a burst reads as the scroll wheel and skips lines), merges the windows (`btw::merge`, which flags a gap), unwraps the terminal's line wrapping (`btw::unwrap`) and sends Esc, each key only while the overlay shows (`key_if_btw`). Answers live in `Sessions::btws` (memory only) and reach the panel in `session_info.btw`. A slash command typed by `type_at_prompt` now gets no `nudge` Enter (it sends no `UserPromptSubmit`, so that Enter answered a picker or closed the `/btw`), a 2.5 s hold, and `Live::dialog`, which blocks typing for 4 s (`DIALOG_DRAW`, so `nudge` can't clear it before the screen draws) and then while `prompt_showing` doesn't see the prompt box (a rule, a `❯` line, a rule; a picker's `❯ 1.` doesn't count), at most 30 min. Which commands open a screen changes between versions (`/cost` opens the Usage settings in 2.1.318), so it is read off the screen, not listed. `session_info.screen` says so; the panel opens the terminal for it (`slashSent`) and a queued message says it waits for that screen. `chat_command` takes `/btw` and refuses `/exit` before `back_to_work`; other slash commands skip `back_to_work`. `transcript::command_output` makes `<local-command-stdout|stderr>` a "cmdout" entry; 2.1.318 writes local commands as `system` lines (`local_command`: the command, then its output; `informational`: notices), which `parse_line` now reads. If Claude Code changes the overlay's footer or the prompt box's rules, fix `btw::view` / `prompt_showing` and their tests.
- **Claude version:** the newest `claude.exe` is picked by asking each candidate its version (`~\.local\bin\claude.exe` is current; `~\bin\claude.exe` is too old).

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
