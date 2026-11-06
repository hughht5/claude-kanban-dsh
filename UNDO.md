# Claude Kanban DSH app: test and undo

Each phase's undo steps are written here before the phase starts, and every change is recorded as it is made. Commands are PowerShell 5.1.

## Phase 0: skeleton (3 Nov 2026)

| Change | Undo |
|---|---|
| `~\kanban-app` made a git repo, with the plan and its docs | `Remove-Item ~\kanban-app\.git -Recurse -Force` removes only the history. The files stay. |
| A read-only export of the Work Board's cards to `~\Documents\kanban-backups\2026-11-03-app-copy\` | `Remove-Item` that folder. The board itself was only read. |

Nothing in Phase 0 touched the board, the kanban skill, the scheduled tasks, either account's settings, or `~\.claude.json`.

## Phase 1: toolchain, store, board on a copy (3 Nov 2026, in progress)

Installs are recorded with their uninstall steps in `INSTALLED.md`. rustup added `%USERPROFILE%\.cargo\bin` to the user `PATH`; `rustup self uninstall` removes it.

| Change | Undo |
|---|---|
| Toolchain installs (rustup with the Rust toolchain, the MSVC build tools, the Tauri CLI, and any others) | Each one's uninstall step in `INSTALLED.md`, in reverse order |
| Crates and npm packages for the build | `Remove-Item ~\kanban-app\target, ~\kanban-app\ui-deps\node_modules -Recurse -Force`. The shared crate cache is `~\.cargo\registry`, removed with rustup's uninstall. |
| The app's data: `%LOCALAPPDATA%\claude-kanban-dsh\` (`board.db`, its backups and the token) | Quit the app, then `Remove-Item "$env:LOCALAPPDATA\claude-kanban-dsh" -Recurse -Force`. It holds only a copy of the cards; the artifact is the real board until cut-over. |
| A dev build of the app (no installer in Phase 1) | Nothing installed. Delete `target\`. |
| A private GitHub repo for the app | `gh repo delete <owner>/kanban-app`. Only Hugh can run this; it needs the `delete_repo` scope. |

**Not touched in Phase 1:** the Work Board artifact and its cards, the kanban skill, the SessionStart hook, the scheduled tasks, both accounts' `settings.json` and `~\.claude.json`, and the desktop app's sessions. Test 1.29 checks this.

## Phase 2: the card panel and terminals (planned)

| Change | Undo |
|---|---|
| npm packages for the panel (`@xterm/xterm`, `@xterm/addon-fit`, `marked`, `dompurify`), pinned in `ui-deps/package-lock.json`, copied into `ui/vendor/` | `Remove-Item ui-deps\node_modules, ui\vendor -Recurse -Force` |
| Crates for PTYs and Job Objects (`portable-pty`, `windows`, `uuid`, `base64`) | delete `target\` |
| Sessions the app starts: `claude.exe` processes, their transcripts under `~\.claude\projects` (or `~\.claude-max2\projects`), and `sessions` rows | Stop them from the panel, or quit the app (the Job Object ends them). Transcripts are Claude Code's own files and stay; resume any with `claude --resume <id>`. |
| A desktop session resumed in the app (only on Hugh's click) | Stop it in the app; reopen it in the desktop app. Both use the same transcript. |
| Session settings files in `%LOCALAPPDATA%\claude-kanban-dsh\sessions\<id>\` | delete that folder |

Not touched: either account's `settings.json` or `~\.claude.json`, the artifact, the kanban skill.

**Done in Phase 2 so far:** Claude Code recorded `~\kanban-app` as a trusted folder in `~\.claude.json`. Claude wrote it itself, when the build session answered the trust question for the test card. Undo it with `/permissions` in a `claude` session in that folder, or by removing the folder's `hasTrustDialogAccepted` entry from `~\.claude.json` while no `claude` is running. Test sessions on card #21 left transcripts in `~\.claude\projects\` (the folder named after the repo's path, ending `-kanban-app`).

## Phase 3: hooks, tools and the dispatcher (planned)

| Change | Undo |
|---|---|
| The app listens on `127.0.0.1:47821` while it runs, for hooks, the MCP tools and `kb`; the token in `%LOCALAPPDATA%\claude-kanban-dsh\token` changes at every start | Quit the app. Nothing listens when it isn't running. |
| Hooks, statusLine and the `kanban` MCP server are written only into each session's own `--settings` and `--mcp-config` files, under `%LOCALAPPDATA%\claude-kanban-dsh\sessions\` | Delete that folder. Neither account's `settings.json` or `~\.claude.json` is changed. |
| Column prompts and settings seeded in `board.db` (migration 2, with a backup taken first) | Restore `backups\<stamp>-pre-migration.db` over `board.db` while the app is closed |

## Phase 4: first usable release (planned)

| Change | Undo |
|---|---|
| The app installed per-user in `%LOCALAPPDATA%\Programs\Claude Kanban DSH` by `scripts\install.ps1` | Uninstall "Claude Kanban DSH" in Windows Settings > Apps |
| `kb.exe`'s folder added to the user `PATH` | Remove that one entry: System Properties > Environment Variables > User > Path |
| Nightly backups in `%LOCALAPPDATA%\claude-kanban-dsh\backups\` | Delete the folder; they are copies only |
| A tray icon, and Windows notifications from "Claude Kanban DSH" | Quit from the tray; turn notifications off in Windows Settings > Notifications |

## Data and install moved out of AppData (3 Nov 2026)

The board is now in `%USERPROFILE%\.claude-kanban-dsh` and the app in `%USERPROFILE%\Programs\Claude Kanban DSH`, because AppData paths were virtualized for processes started from the Claude desktop app. The board with Hugh's cards (from the virtualized folder) was copied there; nothing was deleted.

| Left behind | What to do |
|---|---|
| `%LOCALAPPDATA%\Programs\Claude Kanban DSH`, the old install, possibly with an old Start menu entry | Uninstall "Claude Kanban DSH" in Windows Settings > Apps if it is listed twice, keeping the one in `%USERPROFILE%\Programs`; or just use the desktop shortcut |
| `%LOCALAPPDATA%\claude-kanban-dsh\`, the old data folder (empty board) | Delete it once the app has been used from the new folder |
| `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Local\claude-kanban-dsh\` and `...\Programs\Claude Kanban DSH`, the virtualized copies | Delete them; the board was copied out of there |
| The user PATH entry for kb, added from a Claude session | If `kb` isn't found in a new terminal, the registry write was virtualized too: add `%USERPROFILE%\Programs\Claude Kanban DSH` to the user Path by hand |

## Test version (3 Nov 2026)

| Change | Undo |
|---|---|
| "Claude Kanban DSH Test" installed per-user in `%USERPROFILE%\Programs\Claude Kanban DSH Test` by `scripts\install.ps1 -Channel test`, with its own Start menu entry; not added to PATH | Quit it from its tray icon, then uninstall "Claude Kanban DSH Test" in Windows Settings > Apps |
| Its data in `%USERPROFILE%\.claude-kanban-dsh-test` (a copy of the live board, backups, sessions, `api.json`, `api.log`) | Delete the folder once the test app is uninstalled; it is a copy only |
| Its WebView profile in `%LOCALAPPDATA%\io.github.hughht5.claude-kanban-dsh.test` | Delete the folder after uninstalling |
| Test build output in `target\test` | `cargo clean --target-dir target/test`, or delete the folder |
| `kb copy-board` reads the live board (read-only, `VACUUM INTO`); with `--replace` it moves the board it replaces into `backups` beside it | Nothing to undo on the live board. Restore a replaced test board from `.claude-kanban-dsh-test\backups\<stamp>-replaced-by-copy.db` while the test app is closed |
| Live code: named "Claude Kanban DSH" as before; `install.ps1` without `-Channel` now also clears `KANBAN_CHANNEL` and `CARGO_TARGET_DIR` before building | `git revert` the commit |

## Staged updates and stack health (4 Nov 2026)

| Change | Undo |
|---|---|
| `scripts\install.ps1` stages by default for the live app: the installer and `update.json` go to `%USERPROFILE%\.claude-kanban-dsh\update\` (test: `.claude-kanban-dsh-test\update\`), and the running app isn't quit | Run it with `-Now` for the old behaviour. Delete the `update` folder to drop a staged update; it holds only build copies and `restart.log` |
| The app reads that folder every 30 s and shows "Update ready" on the board. Restart now suspends the sessions, exits, and a hidden PowerShell helper installs silently and starts the app again with `--background` | Click Later, or delete `update\update.json`. If a restart goes wrong, read `update\restart.log` and start the app from its shortcut. Sessions are resumed by restart recovery as after any quit |
| Build identity (`KANBAN_GIT_COMMIT`, `KANBAN_BUILT_AT`) compiled in by `src-tauri\build.rs` | `git revert` the commit |
| Login buttons show the stack's branch, and when it's down a Restart stack command to paste into WSL. The app runs only `git rev-parse` in a Windows-drive checkout, never WSL or `dev-stack.sh` | `git revert` the commit |

Nothing here writes `~\.claude.json` or any `settings.json`, or deletes board rows.

## 4 Nov 2026: kb mcp registered for every project

- Hugh ran `claude mcp add --scope user kanban-app -- kb mcp` (his own Claude Code config). Undo: `claude mcp remove kanban-app -s user`.
- The stale user PATH entry `%LOCALAPPDATA%\Programs\Claude Kanban DSH` (from the first install, before the move out of AppData) was removed, so `kb` is the one in `%USERPROFILE%\Programs\Claude Kanban DSH`. Undo: add it back to the user PATH.
- That leftover folder's `kb.exe` was overwritten with the current one, so processes started before the PATH change also get a working `kb mcp`. The folder can be deleted once nothing old is running.

## 4 Nov 2026: stacked cards

| Change | Undo |
|---|---|
| Migration 8 adds one setting, `stacks.human_test_when` (`all`, or `first`). No card row changes; stack membership is in the cards' own `fields` (`stack`, `stackOf`, `passed`, `passedAt`, `choices`, `archivedWith`), which older builds ignore | Restore the `pre-migration` backup in `backups\` while the app is closed. An older build also opens a schema-8 board as it is: it ignores the setting and shows every card loose |
| A stack on the board (for example #44 with its children) | `kb stack <lead> remove <n>` for each child (or Remove from stack in the child's row). Each child goes back on the board as an ordinary card in its own column |
| The stack rules move a lead to Human test (or back to In progress) when its children change, and carry children to Deploy complete or Done with their lead; archiving a lead archives its children | Every move is logged on the cards, "The app moved the stack …" / "… with its stack #n". Move them back by hand; restoring the lead restores the children archived with it |
| `kb` gains `stack`, `passed` and `create --stack`; the MCP tools gain `kanban_stack_list`, `_add`, `_remove`, `kanban_set_passed`; `kanban_update_card` allows `choices` | `git revert` the commit. The existing commands are unchanged |
| The test board was replaced with a fresh copy of the live board (`install.ps1 -Channel test -FreshCopy`), and the copies of #44–#56 on it are dry runs | The old test board is in `.claude-kanban-dsh-test\backups\<stamp>-replaced-by-copy.db`. Nothing on the live board changed |

## 4 Nov 2026: the panel's message box sends at once, and starts a stopped session (card #62)

Round 2 (waiting-message bubble, Send now / Ctrl+X Ctrl+S, command `session_send_now`): `git revert` its commit. No data changes.

| Change | Undo |
|---|---|
| Hugh's message (the panel box, `kb send`, `kanban_session_send`) is typed into a running session at once, mid-turn too, instead of waiting until it is idle and quiet; it is still recorded in `pending` (queued, then marked `sent`) | `git revert` the commit. No data changes: `pending` rows are written as before |
| The box is shown when no session runs; Send starts or resumes it and the message waits for it (`ui\panel.js`, `ui\panel.css`) | `git revert` the commit |
| The chat view shows a message sent mid-turn (Claude Code's `queued_command` attachment from a human) as Hugh's message (`transcript.rs`) | `git revert` the commit |
| New sessions' settings file gains `PreToolUse`/`PostToolUse` `http` hooks matched to `AskUserQuestion`, so its dialog shows as "Needs you" and nothing is typed into it. Written only into the app's per-session settings files | `git revert` the commit; sessions started after that get the old six hooks |

## 4 Nov 2026: paste screenshots into a stack child's boxes (card #60)

| Change | Undo |
|---|---|
| `ui\paste.js` takes an image pasted into a stack child's "Found a problem" or answer box, saves it under `attachments\<child id>\` in the data folder, and puts `[Image: <path>]` in the box | `git revert` the commit. Saved images are plain files; nothing on the board changes |

## 4 Nov 2026: the app comes back in front after Restart now (card #63)

| Change | Undo |
|---|---|
| The Restart now helper starts the updated app without `--background`, so it opens in front (Hugh clicked for it). Any start that isn't `--background` now forces the window to the front (`place::bring_forward`: `SetForegroundWindow`, then an Alt tap and again, as tao does), where Windows used to refuse it and only flash the taskbar | `git revert` the commit. Nothing on the board or in the data folder changes |

## 4 Nov 2026: a stack child opens in an overlay, not inline (card #64)

| Change | Undo |
|---|---|
| A stack child's row opens the child in a centred overlay (`ui\stacks.js` `stkKidOpen`, `ui\stacks.css` `dialog.stk-kdlg`) instead of inline in the narrow stack card; `ui\paste.js` looks for the problem and answer boxes there. The remembered open child (`wb.stackKid` in the WebView's local storage) is no longer read or written | `git revert` the commit. Nothing on the board or in the data folder changes; the old `wb.stackKid` value is left where it was and read again |

## 4 Nov 2026: the "Working…" line in the card panel (card #66)

| Change | Undo |
|---|---|
| While a session is on a turn, the card panel shows `✻ Mustering… (5m 55s · thought for 4s)` under the conversation (`ui\panel.js` `noteTurn`/`renderBusy`, `ui\panel.css` `.chat .busy`). `session_info` gains `turnMs` (`sessions.rs` `turn_start`, `turn_ms`); `transcript_read` gains `thought`, the latest thinking block's time (`transcript.rs` `latest_thought`) | `git revert` the commit. Nothing on the board or in the data folder changes |

## 4 Nov 2026: offer to resume the sessions that were running at the last quit (card #65)

| Change | Undo |
|---|---|
| At the start, a bar on the board (`ui\resume.js`, `ui\update.css` `.resumebar`) offers the sessions that were idle when the app last quit or crashed, with Resume all and Not now (`resume_offer`, `resume_offer_answer` in `src-tauri\src\live_cmds.rs`; `Store::resume_offer`, `answer_resume_offer`) | `git revert` the commit. The only data written is the setting `app.resume_offer_after` (a session event number); an older build ignores it. Sessions resumed with Resume all are ordinary sessions: stop them from their cards |

## 4 Nov 2026: times on the card panel's messages (card #67)

| Change | Undo |
|---|---|
| Every message and "Ran N tools" group in the card panel's conversation shows its time from the transcript (`ui\panel.js` `stamp`, `ui\panel.css` `.ts`): `16:43` today, `3 Nov 16:43` earlier this year, `1 Feb 2025 23:59` before; hovering shows the full date and time. Search skips the times | `git revert` the commit. Nothing on the board or in the data folder changes |

## 5 Nov 2026: labels on cards (card #71)

| Change | Undo |
|---|---|
| Cards take a `labels` field (a list of short strings: at most 8, each up to 24 characters), cleaned and checked by the store (`crates\store\src\cards.rs` `clean_labels`) on create and update. Sessions may set it (`kanban_update_card`, `kanban_create_card`, `kb update --labels`, `kb create --labels`); `kanban_list_cards` returns it. The board draws them on cards and edits them in the card panel (`ui\labels.js`, `ui\labels.css`); search takes `label:BUG` | `git revert` the commit. No migration: labels live in each card's `fields`, so an older build keeps them and simply doesn't show them. To clear one card's labels: `kb update <n> --labels null` |

## 5 Nov 2026: "Building" on cards whose session is working (card #75)

| Change | Undo |
|---|---|
| A card whose session is working shows a "Building" chip (`ui\panel.js` `buildingChip`, `ui\board.js` `cardEl`, `ui\board.css` `.chip.building`), in place of the plan chip while it works; it goes when the turn ends | `git revert` the commit. Nothing on the board or in the data folder changes |

## 5 Nov 2026: card sessions' Chrome runs in the background (card #77)

| Change | Undo |
|---|---|
| A card session's own `mcp.json` (in its session folder) now also carries a `chrome-devtools` server: the account's own entry from its `.claude.json` (the session folder's project entry first, then the user one), with `--headless` and `--isolated` added (`src-tauri\src\sessions.rs` `headless_devtools`). It replaces the configured one for that session only, so its Chrome has no window and takes no focus, and each session gets a throwaway profile. Nothing is added when the account has no `chrome-devtools` server. `.claude.json` is only read | `git revert` the commit. Sessions started after the revert get the configured, windowed Chrome again; running sessions keep what they started with. Nothing on the board or in the data folder changes |

## 5 Nov 2026: Claude's questions in the chat, and interrupted turns (card #78)

| Change | Undo |
|---|---|
| An `AskUserQuestion` call shows in the card panel's chat as a question card, answerable there while its dialog is on screen; the app types the dialog's keys into the session (`ui\panel.js` `askHtml`/`setupAsk`, `ui\panel.css` `.m.ask`, `src-tauri\src\transcript.rs` `ask_questions`, `src-tauri\src\sessions.rs` `answer_keys`/`answer`, the `session_answer` command) | `git revert` the commit. Nothing on the board or in the data folder changes |
| A turn interrupted in the terminal (no `Stop` hook) now ends in the app when the transcript records its `turn_duration` line, checked on each statusLine report (`transcript::turn_ended_in`, `Sessions::turn_ended`, `dispatch::on_status`) | Same revert. Sessions then stay on "Needs you" or "Working" after an interrupt until their next prompt, as before |

## 5 Nov 2026: one ship at a time per repo (card #79)

| Change | Undo |
|---|---|
| A card moved into Ship it while another card of the same repo is shipping waits with ship state `queued` (a "Waiting to ship" chip) and its session isn't told; the app hands the oldest waiting card to its session once that repo has nothing shipping (`crates\store\src\ships.rs`, `src-tauri\src\dispatch.rs` `after_move` and `pump_ships`, `ui\board.js`, `ui\board.css` `.chip.queued`, `ui\panel.js`). Moving a waiting card out of Ship it cancels it | `git revert` the commit. No migration. A card left `queued` by this build is never handed to its session by an older one: move it out of Ship it and back in, or tell its session to ship it |

## 5 Nov 2026: the Archive column replaces Done (card #83)

| Change | Undo |
|---|---|
| The board draws no Done column; its last column is the Archive: a drop box ("Drop here to archive"), then the archived cards, newest 20 first (`ui\board.js` `archiveCol`, `archiveDrop`, `BOARD_COLS`; `ui\board.css` `.archdrop`, `.archhead`; the "Show archived" button gone from `ui\index.html`) | `git revert` the commit. The old build shows Done again, empty, and archived cards behind "Show archived" |
| Any write that puts a card in Done archives it (`archivedFrom` holds the column it came from) and archives a lead's stack with it; Restore brings a Done card back to `archivedFrom` or Deploy complete (`crates\store\src\cards.rs` `write_tx`, `create`, `archive`, `restore_patch`; `crates\store\src\stacks.rs`) | Same revert. Cards archived this way stay archived: restore any you want back from the card (an older build restores them into Done) |
| Migration 10 (`schema10.sql`) archives every card that was in Done, with a revision and a log line each; a `pre-migration` backup is taken first | An older build opens the board as is. To un-archive them, quit the app and copy the `backups\*-pre-migration.db` from before the update over `board.db` (this loses every change made since), or restore the cards one by one |

## 5 Nov 2026: screenshots in the chat (card #84)

| Change | Undo |
|---|---|
| The card chat draws local images: Markdown images of local files in Claude's messages, a `Read` of an image (inside its tool row) and a message's attached images; click for full size (`ui\panel.js` `md`/`showShots`/`openShot`, `ui\panel.css` `.shot`/`.shotview`, `src-tauri\src\transcript.rs` `images`/`image_mime`, the `image_read` command in `src-tauri\src\session_cmds.rs`) | `git revert` the commit. Nothing on the board or in the data folder changes |
| Card sessions are told to show screenshots that way (`src-tauri\src\api.rs` `INSTRUCTIONS`, `prompts\in-progress.md` step 8, `prompts\kanban-app.md` In progress step 6) | Same revert. Sessions already running keep the instructions they started with |

## 5 Nov 2026: messages sent before the session is up (card #82)

| Change | Undo |
|---|---|
| A message typed at a session's prompt stays unconfirmed until its `UserPromptSubmit`; it is typed again after 15 s, up to 3 tries, then shown as "Not sent" with Send again and Dismiss. On quit an unconfirmed message goes back in the `pending` queue, so it is typed after the resume (`src-tauri\src\sessions.rs` `Typed`, `retry_step`, `retype_bytes`, `confirms`, `nudge`, `send_message`, `type_now`, `stop_all`; `session_dismiss_untaken`; `src-tauri\src\dispatch.rs` logs a retype or give-up) | `git revert` the commit. No migration. The state is in memory, apart from the `pending` row written on quit, which an older build types as any queued message |
| The chat shows the app's waiting, typing and untaken messages as queued bubbles (`session_info`'s `waitingText`, `typing`, `untaken`; `ui\panel.js` `outbox`, `ui\panel.css` `.ptxt`, `.untaken`) | Same revert |

## 5 Nov 2026: Take it to and the subagent test (card #76)

There are no migrations: `autoTo` and `test` are fields in each card's `fields`, and the gate's setting is one settings row, all of which an older build ignores. Nothing in the data folder needs undoing.

| Change | Undo |
|---|---|
| The Add card form's "Take it to" (Backlog, Planning, Human test, Deploy complete). Past Backlog the app moves the new card into Planning for Hugh (`Store::move_for_hugh`, logged as "the app (Hugh's Take it to)"); Human test and Deploy complete set the card field `autoTo` (`crates\store\src\cards.rs` `clean_auto_to`, `src-tauri\src\main.rs` `create_card`, `src-tauri\src\dispatch.rs` `start_new`) | `git revert` the commit. An older build ignores `autoTo`, so such a card just waits for Hugh at each step. To stop one card going on: the panel's Goes on to › Nowhere, or move it yourself anywhere but Planning or In progress |
| A card with `autoTo` moves itself on: its session moves it from Planning to In progress when the plan is ready, and the app sends it the In progress prompt; the app clears `autoTo` at the target, and moves a Deploy complete card from Human test into Ship it for Hugh (ship `"auto": true`, through the ship queue) as his standing approval (`dispatch::auto_progress`, `auto_note`, `move_message`, `held_move_message`). Hugh's own moves, except into Planning or In progress, turn it off | `git revert` the commit. A card the app has already put into Ship it ships as if Hugh had moved it there: move it out of Ship it to cancel a ship not yet started, as before |
| The card's "→ Human test" / "→ Deploy" chip, the panel's "Goes on to" select (Deploy complete asks to confirm), and the session told when Hugh changes it (`ui\board.js` `autoChip`, `TAKE_TO`, `AUTO_OPTS`; `ui\board.css` `.chip.auto`, `.hint`; `dispatch::TOLD`, `edit_message`) | `git revert` the commit |
| `kb create --to Planning\|"Human test"` and `kanban_create_card`'s `to`, from Hugh's own sessions only. A card a non-card session creates into Planning or In progress now starts its session (`crates\kb\src\main.rs`, `src-tauri\src\tools.rs`) | `git revert` the commit. Cards already started are ordinary cards |
| Every card session gets the `card-tester` subagent (`--agents`, `src-tauri\src\tester.rs` `agents_json`) and a ninth `http` hook, `SubagentStop`, written only into the app's per-session settings files (`sessions.rs` `settings_json`, `build_args`). Neither account's `settings.json` or `~\.claude.json` is changed | `git revert` the commit. Sessions started or resumed after it get the old eight hooks and no tester |
| The app records each tester's verdict on the card it names (`test`: `{state, at, detail, agentId}`; `tester::on_subagent_stop`), resets `test` to needed on every move into In progress (`cards.rs` `apply`), and refuses a session's move from In progress to Human test or Deploy complete until it passed (`tester::move_refused`, in `tools.rs` and `dispatch::session_move`). Setting `sessions.test_before_human_test` (`config.rs`), with a checkbox in Settings › Sessions (`ui\settings.js`); the "Tested" / "Test failed" / "Test unclear" chips and the panel's "Subagent test" row (`ui\board.js` `testChip`) | To turn the gate off without a revert: Settings › Sessions, untick "Test every card with a subagent before Human test". Otherwise `git revert` the commit; the `test` fields stay on the cards, unused |
| Prompts and rules: the Ship it rule ("Only Hugh moves a card into Ship it, or the app for him when he set the card to go on by itself to Deploy complete") in `AGENTS.md`, `prompts\planning.md`, `prompts\ship-it.md` and `prompts\kanban-app.md`; `planning.md` "If the card goes on by itself"; `in-progress.md` "Test it with a subagent"; `kanban-app.md` step 6a | `git revert` the commit |
| The kanban-agent skill (`skills\kanban-agent\SKILL.md` and its live copy `~\.claude\skills\kanban-agent\SKILL.md`) says to add `--to "Human test"` when Hugh asks for the card to be planned and built | Restore the previous text from git: `git revert` the commit restores the repo copy, then `Copy-Item ~\kanban-app\skills\kanban-agent\SKILL.md "$env:USERPROFILE\.claude\skills\kanban-agent\SKILL.md"` puts it back live |

## 5 Nov 2026: "Testing" on cards running a review or a test (card #86)

| Change | Undo |
|---|---|
| A card session's own settings now hook `PreToolUse`/`PostToolUse` for `AskUserQuestion`, `Agent`, `Task` and `Skill` (was `AskUserQuestion` only) and send `SubagentStop` (`src-tauri\src\sessions.rs` `settings_json`). The app tracks reviews and tests in flight (`src-tauri\src\testing.rs`, `Sessions::testing_hook`, `dispatch::on_hook`, the `session_testing` command) and the card's chip reads "Testing" instead of "Building" while one runs (`ui\panel.js` `buildingChip`, `ui\board.css` `.chip.testing`) | `git revert` the commit. Sessions started after the revert get the old hooks; running ones keep theirs, and the extra hooks they send are ignored. Nothing on the board or in the data folder changes |

## Card #87: no popup when Goes on to is set to Deploy complete (5 Nov 2026)

| Change | Undo |
|---|---|
| The card panel's Goes on to select saves Deploy complete straight away, with no `confirm` popup; the saved line reads "Saved: it ships with no human test once it is built" (`ui\board.js`, the `#f-auto` change handler). The Add card form never had a popup and is unchanged | `git revert` the commit. No data changed |
| The panel's message line ("Saved" and the like) is kept for 5 seconds and put back when the save's own live update redraws the panel, which used to wipe it at once (`ui\board.js` `say`, `said`, end of `renderDetail`) | `git revert` the commit |

## Card #89: drop a card on a Ship it card to ship them as one (5 Nov 2026)

No migration. The joins write only fields an older build already reads (`stack`, `stackOf`, `passed`, `ship`).

| Change | Undo |
|---|---|
| `Store::stack_join` (`crates\store\src\stacks.rs`) and `ship_started` / `earlier_place` (`crates\store\src\ships.rs`): Hugh's drop can add a built card or stack to a stack in Ship it, take a card out of Ship it into a stack (its ship cancelled, the card in Human test and Passed), or put a built stack into another. A stack is one repo; a session's `kanban_stack_add` / `kanban_stack_join` can't add to a stack in Ship it | `git revert` the commit. Stacks made this way stay as they are: take a card out with the stack's Remove, then move it back to Ship it yourself if it should ship alone |
| The board's `stack_join` command ships a card straight after a target whose ship has started (`ship_after_check`, `hugh_move`, which is `move_card`'s old body), `stack_join_op`'s messages (`dispatch::join_message`, `stand_down_message`), and `after_move` leaving a card that `left_ship_for_stack` (`src-tauri\src\main.rs`, `src-tauri\src\dispatch.rs`) | `git revert` the commit. A card already moved into Ship it this way is an ordinary ship: drag it out to cancel it before it starts |
| The drop's preview, dialogs and refusals (`stack_join_preview`, `Store::stack_join_check`; `ui\stacks.js` `stkDrop`, `stkJoinAsk`, `stkAfterAsk`, `stkJoinDo`; `ui\stacks.css` `.i-ship`), the status line kept for 6 s (`ui\board.js` `status`), a card's middle taking every drop so a refused one says why instead of moving the card into that column, the tool descriptions (`src-tauri\src\tools.rs`) and the Ship it prompt's paragraph on a card joining before the merge (`prompts\ship-it.md`) | `git revert` the commit |

## Card #91: subagents in the chat (5 Nov 2026)

No migration, and nothing in the data folder changes: the app only reads Claude Code's subagent files.

| Change | Undo |
|---|---|
| `transcript::subagents`, `subagent_path`, `read_in`, `parse_line_in` read a session's `subagents\agent-<id>.jsonl` and `.meta.json`; the `subagent_list` command and `transcript_read`'s `agent` (`src-tauri\src\transcript.rs`, `src-tauri\src\session_cmds.rs`, `src-tauri\src\main.rs`, `src-tauri\src\control.rs`) | `git revert` the commit |
| A `<task-notification>` in the main chat reads as one "task" line instead of a raw user bubble (`transcript::task_notice`) | Same revert |
| The chat's subagent strip, the read-only subagent view with Back to main, and the links on `Agent` calls and finished notices (`ui\panel.js` `refreshSubs`, `renderSubs`, `showView`, `subNote`, `taskHtml`, `setupSubs`; `ui\panel.css` `.subs`, `.subdot`, `.subnote`, `.subopen`, `.m.task`) | Same revert |

## Card #93: a message to a card in Human test takes it back to In progress (5 Nov 2026)

No migration, and no new fields.

| Change | Undo |
|---|---|
| Hugh's chat message (Send, Send now) to a card in Human test that isn't in a stack moves it to In progress first and sends a note with it (`src-tauri\src\session_cmds.rs` `back_to_work`; `src-tauri\src\dispatch.rs` `message_moves_back`, `moved_back_message`) | `git revert` the commit. Cards already moved stay in In progress: drag them back to Human test if they should be there |
| Hugh's move from Human test to In progress tells the session it came back from testing (`dispatch::move_message`, `BACK_FROM_TEST`) | Same revert |

## Card #94: slash commands in the chat (5 Nov 2026)

No migration, and nothing in the data folder changes: `/btw` answers are kept in memory only.

| Change | Undo |
|---|---|
| The chat's `/` menu, `/btw` answers and command output (`ui\panel.js` `slashMenu`, `drawSlash`, `pickSlash`, `slashKeys`, `slashSent`, `btwHtml`, the "cmdout" entry; `ui\panel.css` `.slashmenu`, `.m.btw`, `.m.cmdout`) | `git revert` the commit |
| The menu's list (`src-tauri\src\slash.rs`, the `slash_commands` command) and `/btw` read off the screen (`src-tauri\src\btw.rs`; `Sessions::ask_btw`, `run_btw`, `btws`; the `session_btw` and `session_screen` commands; the `vt100` crate in `src-tauri\Cargo.toml`) | Same revert |
| Typing holds: a slash command gets no extra Enter from `nudge`, and the app types nothing while a `/btw` is open or a screen a slash command opened is up (`Live::blocked`, `Live::dialog`, `prompt_showing` in `src-tauri\src\sessions.rs`) | Same revert |
| The menu asks Claude Code for its list (`slash::claude_list`, `with_claude`, `CLAUDE_LISTS` in `session_cmds.rs`). `/btw` and `/exit` from the chat are handled before a message is sent, and no slash command takes a Human test card back to work (`chat_command` in `src-tauri\src\session_cmds.rs`) | Same revert |
| `<local-command-stdout>` shows in the chat as a command's output (`transcript::command_output`) | Same revert |

## The public split (claude-kanban-dsh)

| Change | Undo |
|---|---|
| Setting `private.dir` (Settings > Sessions > Private folder): `{prompts}` and `repos.json` come from it | Clear the field and Save: the bundled example prompts are used and the add-card form lists only this app |
| Column prompts on new boards name `{prompts}\<file>.md` | Edit a column's prompt in Settings to any path you want |
| The app renamed to Claude Kanban DSH: `~\Programs\Claude Kanban DSH`, data in `~\.claude-kanban-dsh` | Uninstall it in Windows Settings > Apps; an old install and its data folder are left as they were |
| A data folder with `import-from.txt` and no board imports that folder's board once, then renames the request `imported-from.txt` | Quit the app, move `board.db` out of the new data folder; the old folder was only read |
| `scripts\hooks\pre-commit` (`git config core.hooksPath scripts/hooks`) runs `scripts\check-public.sh` | `git config --unset core.hooksPath` |
