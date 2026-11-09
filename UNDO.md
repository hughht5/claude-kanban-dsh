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

## Card #96: the card-reviewer subagent beside the card-tester (6 Nov 2026, stack A, branch `dsh-agents`)

Built in the worktree `~\kanban-app-dsh` on the branch `dsh-agents`; main is untouched until Hugh ships.

| Change | Undo |
|---|---|
| The worktree and branch | `git -C $HOME\claude-kanban-dsh worktree remove $HOME\kanban-app-dsh --force; git -C $HOME\claude-kanban-dsh branch -D dsh-agents` (only before it ships) |
| `src-tauri\src\agents.rs`: one table of the app's subagents, their `--agents` JSON (the reviewer at effort high), the deny list (the card-changing tools under both kanban MCP names; whole tools only, no `Bash(...)` rules, which take the whole shell away), the shared verdict code and the move rules (`GATES`). `tester.rs` keeps the tester's text and rule | `git revert` the commit. Sessions started or resumed after it get only the card-tester again |
| `src-tauri\src\reviewer.rs`: the `card-reviewer` subagent, its `review` field and its rule; its third FAIL queues a move to Blocked | Same revert. To keep the code and stop the hold: Settings › Sessions, untick "Review every card's diff..." (`sessions.review_before_human_test` = `off`) |
| `crates\store`: every move into In progress also sets `review` to needed (`RESET_ON_IN_PROGRESS`); `stack_recheck` holds a ready stack until the lead's own test and review passed (`stackHeld` on the lead); the on/off rules are one list (`RULE_KEYS`) | Same revert. The new card fields (`review`, `stackHeld`) and the setting row stay in the board, harmless to an older build, which ignores them |
| `src-tauri\src\configwatch.rs`: a fingerprint of each account's `settings.json` and the config keys of `.claude.json` when one of the app's subagents starts, compared when it stops; a WARNING log line and a notification on a change. It only reads those files | Same revert |
| A lead's own verdict passing reruns its stack's rules (`dispatch::after_lead_verdict`); a child sending the stack back keeps the lead's own test and review | `git revert` the commit |
| A subagent's verdict text is masked before it is stored (`kanban_store::mask_secrets`, in `record_of`): sign-in links, `Bearer` values and tokens become hidden, "password" becomes "pass word" | `git revert` the commit |
| A capped review (`capped: true`, carried until the In progress reset) stops a card session moving the card from a column before Human test into In progress, Human test or Deploy complete (`agents::capped_hold`), and a capped lead holds its stack (`lead_missing`); the session is told to stop | `git revert` the commit, or untick the review switch |
| `build.rs` compiles in `KANBAN_PROMPTS_DIR`: for a test build, the prompts folder of the checkout being built; for a live build, always main's. `BACK_FROM_TEST` and the stack-join message use it | Same revert (the strings go back to `~\claude-kanban-dsh\prompts`) |
| UI: the review chip and the panel rows from one list (`CHECKS` in `ui\board.js`); the subagent rules' checkboxes from one list (`SUBAGENT_RULES` in `ui\settings.js`) | Same revert |
| Prompts: `in-progress.md` "Review and test it with subagents" and `kanban-app.md` steps 4a, 5 and 6a, each new step conditional on the session having the subagent | Same revert |

## Card #97: the card-critic subagent that checks the plan before building (6 Nov 2026, branch `dsh-97`)

Built in the worktree `~\kanban-app-97` on the branch `dsh-97`, from `dsh-agents` at #96's 146e29d; main is untouched until Hugh ships.

| Change | Undo |
|---|---|
| The worktree and branch | `git -C $HOME\claude-kanban-dsh worktree remove $HOME\kanban-app-97 --force; git -C $HOME\claude-kanban-dsh branch -D dsh-97` (only before it ships) |
| `src-tauri\src\critic.rs`: the `card-critic` subagent (effort high), its `critique` field and its rule (`gate`); one line each in `agents::AGENTS` and `agents::GATES` | `git revert` the commit. Sessions started or resumed after it no longer get the critic. To keep the code and stop the hold: Settings › Sessions, untick "Critique every plan..." (`sessions.critic_before_build` = `off`) |
| `crates\store`: `critique` resets (into Planning; a summary edit in Planning or in Blocked before the build; `started` on every move into In progress) in `cards.rs` `apply`; the rule `CRITIC_RULE_KEY` in `RULE_KEYS` | Same revert. The `critique` card field and the setting row stay in the board, harmless to an older build, which ignores them |
| `dispatch.rs`: the Take it to notes name the critic; a move into Planning says so when the rule is off (`CRITIQUE_OFF`) | Same revert |
| UI: the critique chip and panel row (one entry in `CHECKS`, `ui\board.js`); its checkbox (one entry in `SUBAGENT_RULES`, `ui\settings.js`) | Same revert |
| Prompts: `planning.md` "Check the plan with a subagent" (step 6, the stack lead's line, the Take it to line) and `kanban-app.md` Planning step 6, each conditional on the session having the subagent | Same revert |

## Card #98: the card-router subagent sizes each card in Planning (6 Nov 2026, stack A, branch `dsh-98`)

Built in the worktree `~\kanban-app-98` on the branch `dsh-98`, from `dsh-agents` (card #96's commits); main is untouched until Hugh ships.

| Change | Undo |
|---|---|
| The worktree and branch | `git -C $HOME\claude-kanban-dsh worktree remove $HOME\kanban-app-98 --force; git -C $HOME\claude-kanban-dsh branch -D dsh-98` (only before it ships) |
| `src-tauri\src\router.rs`: the `card-router` subagent (effort low, one line in `agents.rs` `AGENTS`); its report's ROUTE lines are recorded on the card as `route`, once per card | `git revert` the commit. Sessions started or resumed after it no longer get the router |
| The route's model and effort apply in Planning and In progress (`router::settings_for`, which `session_settings` now calls), and the dispatcher restarts a routed card's free session onto them and back, resuming its conversation (`router::restart_if_due` in `dispatch::run`; Route again leaves `routeCleared`). It never types `/model` or `/effort`. Cards with no route are unchanged | Same revert. To stop it on one card without a revert, set its model and effort in the panel: Hugh's choice beats the route |
| A card routed small goes on from Planning to In progress by itself once its plan critique passed: a held move queued by the app (`router::go_on_if_ready`, called from `agents.rs` `record_check` and when a route is recorded; the held-move path reads its `by` and sends the In progress prompt, `dispatch::app_move_message`) | Settings › Sessions, untick "Let a card routed small start building once its plan critique passes" (`sessions.small_go_on` = `off`). Or the same revert |
| `crates\store`: `ROUTE_KEY` (`route`) and the setting `sessions.small_go_on` (`SMALL_GO_ON_KEY`, in `RULE_KEYS` and `PAGE_SETTINGS`) | Same revert. The `route` field and the setting row stay in the board, harmless to an older build, which ignores them |
| The panel's Route again (`route_again` in `session_cmds.rs`): clears `route` and asks the session to run the router again; Planning only | Same revert |
| UI: the Small / Normal / Split chip and the panel's Route row (`ui\board.js`), the setting's checkbox (`ui\settings.js`), the "route" mark by the model and effort (`ui\live-switch.js`) | Same revert |
| Prompts: `planning.md` "Route it first" and `kanban-app.md` Planning step 0, each conditional on the session having the router | Same revert |

## Card #99: the stuck-session monitor (6 Nov 2026, stack A, branch `dsh-99`)

Built in the worktree `~\kanban-app-99` on the branch `dsh-99`; main is untouched until Hugh ships. No migration.

| Change | Undo |
|---|---|
| The worktree and branch | `git -C $HOME\claude-kanban-dsh worktree remove $HOME\kanban-app-99 --force; git -C $HOME\claude-kanban-dsh branch -D dsh-99` (only before it ships) |
| `src-tauri\src\monitor.rs` and its thread (`dispatch::run` starts it), `Sessions::watch` and the StopFailure `error` (`sessions.rs`), `Subagent::stopped` (`transcript.rs`), `tools::UPDATABLE` made `pub(crate)` | `git revert` the commit. To keep the code and stop the monitor: Settings › Sessions, "Look for stuck card sessions every" 0, which also clears every chip |
| The setting `sessions.stuck_check_minutes` (`crates\store\src\config.rs`) | Same revert. A saved value stays in the settings table, ignored by an older build |
| The `stuck` card field | An older build ignores it. To clear it with the monitor still in: set the setting to 0 |
| UI: the May be stuck chip and panel row (`ui\board.js`, `ui\board.css`), the setting's box (`ui\settings.js`) | Same revert |
| Comparing calls without their `description` (6 Nov, branch `dsh-99-desc` from `dsh-integration` 7e06adc): `LABELS` and `compared` in `monitor.rs` | `git revert` that commit (card #99, "description") |
| The idle check (6 Nov, branch `dsh-99-idle` from `dsh-integration` 2b0fb0e): `check`'s "idle" kind, `idle_text`, `IDLE_COLUMNS`, `View::blocked` (`monitor.rs`), its label in `ui\board.js` | `git revert` that commit (card #99, "idle"). A card's `stuck` field of kind "idle" clears at the next check of an older build |

## Card #111: time in each column (6 Nov 2026)

No migration, no new table and no new card fields. Everything is read from the revisions the board
already keeps.

| Change | Undo |
|---|---|
| The panel's "Time in columns" section and the column headers' median tooltips (`ui\times.js`, `ui\times.css`, a script and a stylesheet line in `ui\index.html`, one hook line each in `ui\board.js` `render` and `renderDetail`) | `git revert` the commit |
| The read-only commands `card_times` and `column_medians` (`src-tauri\src\times.rs`, two lines in `src-tauri\src\main.rs`) and the store's rebuild (`crates\store\src\times.rs`, `crates\store\tests\times.rs`, two lines in `crates\store\src\lib.rs`) | Same revert |
| A new card's create revision records the column it was made in (`{"created":true,"column":"Backlog"}`, `Store::create` in `crates\store\src\cards.rs`), and an import's revision the column and archive state it wrote (`crates\store\src\import.rs`) | Same revert. The create revisions written meanwhile keep the extra `column` key: revisions are append-only, nothing else reads that JSON, and an older build ignores it, so they are left as they are |

## Card #102: step through Human test cards with Previous and Next (6 Nov 2026, branch `dsh-102`)

Built in the worktree `~\kanban-app-102` on the branch `dsh-102`, from `dsh-agents`; main is untouched until Hugh ships.

| Change | Undo |
|---|---|
| The worktree and branch | `git -C $HOME\claude-kanban-dsh worktree remove $HOME\kanban-app-102 --force; git -C $HOME\claude-kanban-dsh branch -D dsh-102` (only before it is merged) |
| `ui\review.js` and `ui\review.css` (loaded in `ui\index.html`): the Human test queue, the review bar, Pass with its confirm and 5-second Undo, Fail mode, the Human test box, and the one key handler (`htKeys`) that card #116 reuses | `git revert` the commit. Nothing is stored: a Pass still waiting is dropped |
| `ui\panel.js`: `openPanel(id, {step})` and `closePanel(step)` switch card in place; the review hooks (`reviewOpened`, `reviewClosed`, `reviewOwnsFocus`, `reviewFailing`, `reviewFailSend`, `reviewFailEnd`); no focus on the message box for a Human test card | Same revert |
| `src-tauri\src\session_cmds.rs`: the `human_test_fail` command; `src-tauri\src\dispatch.rs`: `fail_plan`, `failed_log`, `failed_message`, `lead_failed_message` and their tests; `src-tauri\src\main.rs`: `problem_back`, the steps `stack_problem` and a lead's Fail share | Same revert. "Failed in Human test: ..." log lines already written stay on the cards (append-only) |

## Card #103: Time to check on Human test cards (6 Nov 2026, branch `dsh-103`)

Built in the worktree `~\kanban-app-103` on the branch `dsh-103` (from `dsh-agents`); main is untouched until Hugh ships.

| Change | Undo |
|---|---|
| The worktree and branch | `git -C $HOME\claude-kanban-dsh worktree remove $HOME\kanban-app-103 --force; git -C $HOME\claude-kanban-dsh branch -D dsh-103` (only before it ships) |
| `crates\store`: every move into Human test removes `checkSize` (`RESET_ON_HUMAN_TEST`); `Store::check_size_due` | `git revert` the commit. The `checkSize` field stays on cards, harmless to an older build, which ignores it |
| `src-tauri\src\check_size.rs`: the app reads each Human test card's diff with git (read-only) and writes `checkSize` and a log line; `dispatch::run` calls it every 3 s; the `check_size_again` command; `regex` in `src-tauri\Cargo.toml` | Same revert |
| UI: `ui\check-size.js`, `ui\check-size.css` and its script line in `ui\index.html` | Same revert, or delete the script line in `index.html` to hide it all |
| The chip's tooltip and the Time to check box no longer repeat the headline reason (branch `dsh-103-tooltip`, from `dsh-integration` 27fe0a8) | `git revert` that commit |

## Card #107: what a card is waiting on Hugh for (6 Nov 2026, branch `dsh-107`, from `dsh-agents`)

Built in the worktree `~\kanban-app-107` on the branch `dsh-107`; main is untouched until Hugh ships.

| Change | Undo |
|---|---|
| The worktree and branch | `git -C $HOME\claude-kanban-dsh worktree remove $HOME\kanban-app-107 --force; git -C $HOME\claude-kanban-dsh branch -D dsh-107` (only before it ships) |
| Shared board hooks (`boardHooks` with `filters`, `decorate`, `details` in `ui\board.js`); the filter box splits on spaces and every term must match; a label click adds or removes its own term (`ui\labels.js`) | `git revert` the "shared hooks" commit. Revert the cards that push onto `boardHooks` first, or their files throw on load |
| `src-tauri\src\needs.rs`: `ask` on each need, `redact()`, an in-memory reply cache and its background threads; `compute` takes the sessions' waits and the cache | `git revert` the card #107 commit. Nothing is stored, so there is nothing to clean up |
| `src-tauri\src\sessions.rs`: one `waits` field on a running session (prompt text, StopFailure error), `Sessions::waits`, `prompt_wait`, `set_prompt_call` | Same revert |
| `src-tauri\src\dispatch.rs` `on_hook`: hands the Stop hook's reply to `needs::turn_ended`; a permission Notification starts the transcript read | Same revert |
| `src-tauri\src\transcript.rs`: reply and open-call readers at the end of the file | Same revert |
| `src-tauri\Cargo.toml`: `regex` added (already in the workspace) | Same revert |
| UI: the line on the card, the panel's "Waiting on you" box, the `is:waiting` filter and the header button (`ui\needs.js`, new `ui\needs.css`, its link in `ui\index.html`) | Same revert |

## Card #112: the card-reproducer subagent (6 Nov 2026, branch `dsh-112`)

Built in the worktree `~\kanban-app-112` on the branch `dsh-112`, from `dsh-agents`; main is untouched until Hugh ships.

| Change | Undo |
|---|---|
| The worktree and branch | `git -C $HOME\claude-kanban-dsh worktree remove $HOME\kanban-app-112 --force; git -C $HOME\claude-kanban-dsh branch -D dsh-112` (only before it ships) |
| `src-tauri\src\repro.rs`: the `card-reproducer` subagent, its `repro` field, its report reader, the shot copy and its rule; one line each in `agents.rs` `AGENTS` and `GATES` | `git revert` the commit. Sessions started or resumed after it no longer get the reproducer |
| Bug reports set `repro` to needed: Found a problem (`main.rs` `stack_problem`), Report a bug (`session_send`'s `bug`), a card-tester FAIL with a `- BUG:` line (`agents.rs` `record_check`); a stack waits for its lead's open bug report (`crates\store\src\stacks.rs` `stack_recheck`, `repro_open` in `cards.rs`) | Same revert. To keep the code and stop the hold: Settings › Sessions, untick "Prove reported bugs with the reproducer" (`sessions.repro_before_human_test` = `off`) |
| The setting `sessions.repro_before_human_test` (`REPRO_RULE_KEY` in `crates\store\src\config.rs`) | Same revert. The setting row and the cards' `repro` fields stay in the board, harmless to an older build, which ignores them |
| Kept screenshots in the data folder's `repro\<card id>\` | Plain files the app only adds; to remove them, `Remove-Item -Recurse "$env:USERPROFILE\.claude-kanban-dsh-test\repro"` (test) once the app has quit. Nothing on the board points anywhere else |
| `session_cmds.rs` `local_image`: the chat's image check, shared with the shot copy | Same revert |
| A stack's card isn't held for the reproducer: `mark_needed` and `gate` skip cards with `stackOf`; `stack_problem` sends `repro::child_note` and logs on an own-build child's lead; in-progress.md's section is guarded on a `repro` field (whole-build review S4, S8) | `git revert` that commit (card #112's fix on `dsh-112-fix`) |
| UI: the reproducer chip and Reproducer box (`ui\board.js` `CHECKS`, `reproBox`; `ui\board.css`), Report a bug beside Send (`ui\panel.js` `post`), the rule's checkbox (`ui\settings.js`) | Same revert |
| Prompts: `in-progress.md` "Prove a reported bug first", `kanban-app.md` 1a and the hand-over line, and `BACK_FROM_TEST` in `dispatch.rs` | Same revert |

## Card #100: the card-merger subagent at ship (6 Nov 2026, branch `dsh-100`)

Built in the worktree `~\kanban-app-100` on the branch `dsh-100`, from `dsh-agents` (card #96's foundation). Main is untouched until Hugh ships.

| Change | Undo |
|---|---|
| The worktree and branch | `git -C $HOME\claude-kanban-dsh worktree remove $HOME\kanban-app-100 --force; git -C $HOME\claude-kanban-dsh branch -D dsh-100` (only before it ships) |
| `src-tauri\src\merger.rs`: the `card-merger` subagent (edits allowed), its report (`CARD`, `VERDICT`, `HEAD`, `ORIGIN`) recorded as the card's `merge` field with a "Merger: ..." log line; a moved origin also tells Hugh. One line in `agents.rs` `AGENTS`, `mod merger` in `main.rs` | `git revert` the commit. Sessions started or resumed after it no longer get the merger, and the Ship it steps say to go on without it |
| `crates\store`: `MERGE_KEY`; every move into Ship it clears the card's `merge` | Same revert. A `merge` field left on a card is harmless to an older build, which ignores it |
| UI: the Merged / Merge failed / Merge unclear chip on Ship it and Deploy complete cards, and the Merger row in the panel (one `CHECKS` entry in `ui\board.js`) | Same revert |
| The shell guard (`src-tauri\src\guard.rs`): each card session's settings get a second `PreToolUse` hook (`Bash\|PowerShell`, to `/hook/<token>/<row>/guard`, `sessions::settings_json`); `api.rs` answers it with `guard::guard`, which refuses every app subagent's `kb` and `claude config`/`mcp`, and a card-merger's `git push`, `gh pr merge` and `gh api`, and logs it on the card | Same revert. Session settings files are written at each start, so sessions started after it lose the hook. A session still running keeps it; its guard calls then reach the generic hook path and get `{}`, so nothing is refused |
| Prompts: `ship-it.md` step 1a and `kanban-app.md` Ship it step 1a, both conditional on the session having the `card-merger` | Same revert |

## Card #113: a stack's cards build in parallel, with the lead as integrator (6 Nov 2026, branch `dsh-113`)

Built in the worktree `$HOME\kanban-app-113` on the branch `dsh-113`, from `dsh-agents` at `6a0245c`; main is untouched until Hugh ships. **No migration and no data change:** a child's merge record (`merge`) and the start queue's `first` flag are plain fields, and an older build ignores them. Its commits carry `(card #113)` in their messages. Revert them all, newest first: `git -C $HOME\kanban-app-113 log --format=%H -F --grep="(card #113)" | ForEach-Object { git -C $HOME\kanban-app-113 revert --no-edit $_ }` (on whichever branch holds them; after a ship, run it in `$HOME\claude-kanban-dsh` on `main`).

| Change | Undo |
|---|---|
| The worktree and branch | `git -C $HOME\claude-kanban-dsh worktree remove $HOME\kanban-app-113 --force; git -C $HOME\claude-kanban-dsh branch -D dsh-113` (only before it ships) |
| `crates\store`: a child's stage (`stack_stage`), merged (`stack_merged`), readiness counting merged children only (`stack_ready(lead, ..)`), the lead's review counted from its children's (`lead_missing_in_stack`), Passed refused before the merge, the merge record cleared when a child goes back; the lead's start first in the queue (`StartQueue::push_first`, `Store::queue_start_first`) | The revert above. A `merge` field left on a card and a `first` flag left in `sessions.start_queue` are ignored by the older build |
| `src-tauri\src\stackrun.rs` and its calls in `dispatch.rs`, `main.rs`, `session_cmds.rs`, `tools.rs`: children started when Hugh moves the lead, the child's note, the lead woken with a merge brief, `after_child_merge`, a child's session stopped at Human test, Detach refused for a merged child, the lead on low effort, `stack_stages`, the new `kanban_stack_list` fields | The revert above. Children that already have sessions keep them: stop one from its card if it should not run. A lead on low effort: set Effort on its card |
| Prompts: `planning.md` and `in-progress.md` stack sections, `kanban-app.md` "A stack card". Each new step starts "If `kanban_stack_list` shows a `stage` for each child", so an older build takes the old way | The revert above |
| Docs: `PLAN.md` Decisions 15 to 20 (R1 to R6), `design\stacked-cards\SPEC.md` "Parallel build", `README.md` "Stacked cards", `HANDOVER.md` "Stacks build in parallel" | The revert above |

## Card #108: what changed on each card (6 Nov 2026)

No migration. Two new card fields in the free JSON: `changesOutputs` (a session's handed-over
files) and `changesSaved` (the figures the app saves when a card enters Human test). Older builds
carry both through unchanged.

| Change | Undo |
|---|---|
| Read-only git figures for a card, the commands `card_changes`, `card_changes_brief` and `changes_open`, and the save on entering Human test (`src-tauri\src\changes.rs`; `mod changes` and three handler lines in `src-tauri\src\main.rs`; three one-line hooks in `src-tauri\src\dispatch.rs` `after_move` and `after_stack_move`) | `git revert` the commit |
| `changesOutputs` cleaned by the store (`clean_changes_outputs` in `crates\store\src\cards.rs`, re-exported from `crates\store\src\lib.rs`) and settable by sessions (`UPDATABLE` in `src-tauri\src\tools.rs`) | Same revert. Cards that already have the fields keep them harmlessly; to clear one: `kb update <n> --changesOutputs null` |
| The card tag and the panel's "What changed" box (`ui\changes.js`, `ui\changes.css`, one script line in `ui\index.html`) | Same revert |
| The guarded `changesOutputs` line in `prompts\in-progress.md` (hand-over) and `prompts\kanban-app.md` (Human test) | Same revert |
| Fix from the whole-build review (branch `dsh-108-fix`): a Done card's tag comes from its saved figures, and the board runs no git for Done cards (`chgLocalBrief` in `ui\changes.js`, `asks_git` in `src-tauri\src\changes.rs`) | `git revert` that commit |
| Final-test nit (branch `dsh-108-fix2`): the "What changed" box names a branch's base ("against main"), not its 40-character commit id (`baseName` from `base_name` in `src-tauri\src\changes.rs`, read in `chgSource` in `ui\changes.js`; figures saved before it show a 7-character id) | `git revert` that commit |

## Card #101: before-and-after evidence on the card (6 Nov 2026, branch `dsh-humantest`)

Built in the worktree `~\kanban-app-humantest` on the branch `dsh-humantest`, made from `dsh-agents`; main is untouched until Hugh ships.

| Change | Undo |
|---|---|
| The worktree and branch | `git -C $HOME\claude-kanban-dsh worktree remove $HOME\kanban-app-humantest --force; git -C $HOME\claude-kanban-dsh branch -D dsh-humantest` (only before it ships) |
| The app-only card field `evidence`, written by the app with the card-tester's verdict (`Check.evidence`, `agents::record_check`, `src-tauri\src\evidence.rs`, `crates\store\src\evidence.rs`). No migration | `git revert` the commit. The field stays on cards that have it, harmless: an older build ignores it |
| Shot files copied into the data folder, `%USERPROFILE%\.claude-kanban-dsh(-test)\evidence\<run>\` | Leave them: the board is append-only. After the revert nothing reads them. If Hugh wants the disk back, he deletes the `evidence` folder himself (only the shots go; the cards keep their field) |
| The tester prompt's step 5 (`tester.rs`): shots for `hughht5/kanban-app` cards | Same revert. Sessions started or resumed after it get the old prompt |
| `tools\cdp.mjs shot`: needs `CDP_PORT`, crops to a selector, hides the terminal, chat and masked inputs, resets the colour scheme, refuses a blank shot | `git checkout dsh-agents -- tools/cdp.mjs` (or the revert). Until then, run `shot` with `$env:CDP_PORT = '9334'` (or `'9333'` for live, with Hugh's say-so) |
| `src-tauri\tauri.test.conf.json`: the test window's `additionalBrowserArgs` (`--remote-debugging-port=9334 --remote-allow-origins=http://127.0.0.1:9334` and wry's defaults), so the test app always has its DevTools port. Test build only | Same revert, then reinstall the test app (`scripts\install.ps1 -Channel test`); the installed test app keeps its port until it is reinstalled. The live app never had it |
| `cdp.mjs shot` stamps the test app's build commit into each PNG (`tEXt` chunk `kanban-build`), read back as the shot's `build` | Same revert |
| `update_status` (`src-tauri\src\update.rs`) announces a staged-update change it finds (`update-ready`), as the 30 s watcher does, so a shot reading the build can't hide the "Update ready" bar | Same revert |
| The tester's step 5 takes the port of the test app it tests from the brief (test 9334, test N 9333+N, 9334 by default, never 9333); `kanban-app.md` 6 and 6a and `in-progress.md` put the test app and its port in the brief and `CDP_PORT` on the shot command | `git revert` that commit (the step goes back to a fixed 9334) |
| UI: `ui\evidence.js`, `ui\evidence.css`, two lines in `ui\index.html`, the Evidence box and "first .ev wins" in `renderDetail` (`ui\board.js`) | Same revert |
| `prompts\in-progress.md`: one guarded hand-over sentence (the shots as Markdown images) | Same revert |

## Card #104: small, fully checked cards go on by themselves (6 Nov 2026, branch `dsh-104`)

Built in the worktree `~\kanban-app-104` on the branch `dsh-104` (from `dsh-agents`); main is untouched until Hugh ships. It is off until Hugh lists a repo.

| Change | Undo |
|---|---|
| The worktree and branch | `git -C $HOME\claude-kanban-dsh worktree remove $HOME\kanban-app-104 --force; git -C $HOME\claude-kanban-dsh branch -D dsh-104` (only before it is merged) |
| To stop it without a revert | Settings › Sessions, remove every row under "Let small, fully checked cards go on to Ship it by themselves" and Save (`sessions.small_goes_on_repos` = `[]`). A card already waiting stops at its next check, because its repo is no longer listed |
| Migration 11 (`crates\store\src\schema11.sql`): the append-only `settings_log` table, and every change of a page setting written to it (`set_page_setting_by`; `Store::log_setting`, `setting_log`) | `git revert` the commit. The table and its rows stay in the board (append-only; an older build never reads it). The pre-migration backup is in `%USERPROFILE%\.claude-kanban-dsh\backups` |
| The repo list setting (`SMALL_REPOS_KEY`, `SmallRepo`, `check_small_repos`, `small_repos`, `small_repo_for` in `crates\store\src\config.rs`) | Same revert. The setting row stays, ignored |
| A card waiting to go on stops waiting when it leaves Human test (`SMALL_KEY` in `crates\store\src\cards.rs` `apply`) | Same revert. Cards keep their `smallChange` and `smallOptOut` fields, which an older build ignores |
| `src-tauri\src\small.rs` (the git facts, with replace objects, attributes, external diffs, textconv and fsmonitor turned off, and the rules) and `src-tauri\src\goes_on.rs` (the check after a session's move into Human test, the 10-minute wait, the move into Ship it, the checks at the ship's release and its report, Stop and the opt-out) | Same revert |
| The tester's and reviewer's records also hold `tools`, `checkout`, `head` and `statusHash`, and the reviewer's `scope` from a new `SCOPE:` line (`src-tauri\src\agents.rs` `record_of`, `record_check`; `src-tauri\src\reviewer.rs`) | Same revert. Old records keep the extra fields, harmless |
| Dispatch: the held move into Human test calls the check, the loop looks for waits that are over every 20 s, `after_move` checks a released ship first, the Ship it message says to push exactly the checked commit, and `kanban_ship_result` shipped checks the push | Same revert |
| UI: `ui\small.js` (the chips, Stop, the panel's Small change row), its hooks in `ui\board.js` and `ui\index.html`, its styles in `ui\board.css`, and the Settings box with its change list (`smallReposBox` in `ui\settings.js`) | Same revert |
| The hard rule's wording ("Only Hugh moves a card into Ship it, or the app for him when he set the card to go on by itself (Take it to), or turned on small-change auto-ship for that repo.") in `AGENTS.md`, `README.md`, `prompts\planning.md`, `prompts\kanban-app.md`, `prompts\in-progress.md`, `src-tauri\src\api.rs` and the refusals in `src-tauri\src\tools.rs` | Same revert |

## Card #106: step progress on each card (6 Nov 2026, stack C, branch `dsh-106`)

Built in the worktree `~\kanban-app-106` on the branch `dsh-106` (from `dsh-agents`); main is untouched until Hugh ships.

| Change | Undo |
|---|---|
| The worktree and branch | `git -C $HOME\claude-kanban-dsh worktree remove $HOME\kanban-app-106 --force; git -C $HOME\claude-kanban-dsh branch -D dsh-106` (only before it ships) |
| Card sessions start with `CLAUDE_CODE_ENABLE_TODO_TOOLS=1` while `sessions.todo_tools` is on, its default (`build_env`'s `todo_tools`, `Plan::todo_tools`, `Sessions::start`) | Settings › Sessions, untick "Card sessions keep a to-do list" (`sessions.todo_tools` = `off`). Sessions started after that have no to-do tools. Or `git revert` the commit |
| The setting `sessions.todo_tools`, one of the on/off `RULE_KEYS` (`crates\store\src\config.rs`), `kanban_store::mask_tokens` (`secret.rs`) and `Store::entered_at` (`cards.rs`); no migration, nothing written to the board but the setting's row when Hugh saves it | Same revert. A saved setting row stays, harmless to an older build, which ignores it |
| `src-tauri\src\steps.rs` (the reader, the worker, `card_steps`), its line in `dispatch::on_status`, its state and command in `main.rs`, `steps` in `kanban_get_card` | Same revert |
| `ui\steps.js`, `ui\steps.css`, their two lines in `ui\index.html`, the setting's box in `ui\settings.js` | Same revert |
| `prompts\in-progress.md`: the "Keep your steps as a to-do list" paragraph, which a session without the tools skips | Same revert |
| Claude Code's own task files under `~\.claude\tasks\` and `~\.claude-max2\tasks\` (written by the sessions, read only by the app) | Nothing to undo: Claude Code owns them |

## Card #116: review a stack in Human test in its overlay (6 Nov 2026, branch `dsh-116`)

Built in the worktree `~\kanban-app-116` on the branch `dsh-116`, from `dsh-102` (8cf4fd5); main is untouched until Hugh ships. UI only: no Rust, no data, no prompt.

| Change | Undo |
|---|---|
| The worktree and branch | `git -C $HOME\claude-kanban-dsh worktree remove $HOME\kanban-app-116 --force; git -C $HOME\claude-kanban-dsh branch -D dsh-116` (only before it is merged) |
| `ui\stacks.js`: the stack's stop in #102's queue opens the overlay (`stkStackStop`, set as `reviewStackStop`), with a review bar, Previous and Next through the stack's children and a stack page, Pass (P then Enter) and Fail, a key target for #102's handler (`STK_KEYS`), the checks rows and #101's evidence; a click or Enter on a stack card in Human test opens it (`stkReviewEntry`); the overlay is drawn in parts and steps in place | `git revert` the commit. The overlay goes back to Passed, Found a problem and answers, and #102 steps a stack through its lead's panel |
| `ui\stacks.css`: the bar, checks, stack page and Fail line in the overlay | Same revert |
| `ui\panel.js` `openShot`: a full-size screenshot opens inside an open dialog, and the focus goes back where it was | Same revert |

## Card #105: "Done when" and acceptance checks (6 Nov 2026)

No migration: two new card fields, `doneWhen` and `doneWhenChecks`, in each card's JSON. #105 adds
no schema file. Nothing in the data folder changes.

| Change | Undo |
|---|---|
| The fields cleaned, and each check's who and when set by the store (`crates\store\src\done_when.rs`; `crates\store\src\cards.rs` the `apply` arm, `fill_pending` in `write_tx` and `create`; `crates\store\src\lib.rs`) | `git -C $HOME\claude-kanban-dsh revert <commit>`. Cards that have the fields keep them; an older build ignores them and keeps them on every write. To clear one card: `kb update <n> --doneWhen null --doneWhenChecks null` |
| Sessions and `kb` may set them (`src-tauri\src\tools.rs` `UPDATABLE`, `DONE_WHEN_HINT`; `crates\kb\src\main.rs` usage line) | Same revert |
| The card-tester's CHECK lines mark the checks (`src-tauri\src\done_when.rs`; `src-tauri\src\agents.rs` `record_check`, `pass_fails_on_checks`, `mark_checks`; the prompts in `src-tauri\src\tester.rs`, `reviewer.rs` and `critic.rs`; `src-tauri\src\main.rs`) | Same revert. A session started on the new build keeps its tester prompt until it restarts |
| Hugh's edits of them are told to the session, except in Human test, Ship it and Deploy complete (`src-tauri\src\dispatch.rs` `TOLD_DONE_WHEN`, `DONE_WHEN_QUIET`, `checks_text`) | Same revert |
| The card-face line and the panel block (`ui\done-when.js`, `ui\done-when.css`, `ui\index.html`) and the shared hook (`ui\board.js` `CARD_PARTS`, `addCardPart`, `cardLines`, `panelParts`, `panelPartsWire`) | Same revert. Revert any later card that registers a part with `addCardPart` first |
| Guarded prompt steps (`prompts\planning.md` 5a, `prompts\in-progress.md`, `prompts\kanban-app.md` 5a and 6b) | Same revert. Live reads main's prompt files at once, so after the revert sessions stop being asked to write the fields |

## Card #118: several test apps side by side (6 Nov 2026, branch `dsh-118`)

Built in the worktree `$HOME\kanban-app-118` on the branch `dsh-118`, from `dsh-integration` at `f958734`. **No migration and no data change.** Live and `test` keep every name they had. Its commits carry `(card #118)`. Revert them, newest first: `git -C $HOME\kanban-app-118 log --format=%H -F --grep="(card #118)" | ForEach-Object { git -C $HOME\kanban-app-118 revert --no-edit $_ }` (after a ship, run it in `$HOME\claude-kanban-dsh` on `main`).

| Change | Undo |
|---|---|
| The worktree and branch | `git -C $HOME\claude-kanban-dsh worktree remove $HOME\kanban-app-118 --force; git -C $HOME\claude-kanban-dsh branch -D dsh-118` (only before it ships) |
| `crates\store\src\channel.rs`: test slots `test2`..`test10` and the name derivation; a bad `KANBAN_CHANNEL` stops the build; `crates\store\tests\channels.rs` | The revert above |
| `scripts\install.ps1`: `-Channel testN`, `-NamesOnly`, the generated config and hooks in `target\testN` | The revert above. `Remove-Item $HOME\claude-kanban-dsh\target\test2, $HOME\claude-kanban-dsh\target\test3 -Recurse -Force` removes the build folders |
| `ui\board.js` bar and titles, `notify.rs` toast prefix, `build.rs` prompts folder for any test slot, kb usage text, README, AGENTS, HANDOVER | The revert above |
| Final-test finding F1 (branch `dsh-118-f1`, from `dsh-integration` 27fe0a8): a test app's sessions get `KANBAN_DATA_DIR` (its data folder) and its install folder first on PATH (`build_env_for` in `sessions.rs`); `kb` and `kb mcp` read `KANBAN_DATA_DIR` first (`kanban_store::kb_data_dir`); tests in `sessions.rs`, `channel.rs` and `crates\kb\tests\session_board.rs` | The revert above. The live app never sets the variable, so live sessions are unchanged either way |
| An installed slot (when the router installs test 2 or test 3) | Quit it from its tray, then `& "$env:USERPROFILE\Programs\Claude Kanban DSH Test 2\uninstall.exe" /S` (the NSIS uninstaller in its folder). Its data folder `%USERPROFILE%\.claude-kanban-dsh-test2` holds only a copy of the board: rename it rather than delete it if it should go. Same for test 3 |

## Turns on the test app (6 Nov 2026, card #115)

| Change | Undo |
|---|---|
| Card #115's commit on branch `dsh-115` (the queue, `kanban_test_app`, `kb testapp`, the chip and banner, `install.ps1`'s lock and waits, the prompt step) | `git revert <sha>` on the branch it was merged into, then reinstall the test app (and stage live, if it shipped). |
| The setting `testapp.queue` (and `testapp.turn_minutes`, if set) on each board | Nothing to do: older builds ignore them. No migration, no row removed. |
| The folder `%USERPROFILE%\.claude-kanban-dsh-shared\testapp\` (`install.lock`, `turn-*.json`, `hold.json`, `input-test.json`) | With both apps closed and no install running: `Remove-Item "$env:USERPROFILE\.claude-kanban-dsh-shared" -Recurse -Force`. |
| Old DSH lock folders renamed by Release: `%USERPROFILE%\.dsh-test-app.lock.released-*` | `Remove-Item "$env:USERPROFILE\.dsh-test-app.lock.released-*" -Recurse -Force` once DSH is over. |
| A Hold that blocks installs | Click **Let it restart**, or `Remove-Item "$env:USERPROFILE\.claude-kanban-dsh-shared\testapp\hold.json"`. |
| An install turn or tester turn stuck | **Release** on the Test app chip, or `kb testapp release --force` from outside a card session. |

## Wave 3 merge fixes on `dsh-integration` (6 Nov 2026, the merger)

| Change | Undo |
|---|---|
| `scripts\install.ps1`, slot N build (#101 with #118): the slot's own DevTools port (9333 + N) in the generated window config's `additionalBrowserArgs` | `git revert` the `Integrate dsh-118 (wave 3)` merge, or delete the six lines after `$Conf.app.windows[0].title = $Name` |
| `scripts\install.ps1`, `Get-TurnWait` (#115 with #118): reads `turn-live.json` and this run's own slot's `turn-<channel>.json`, not always `turn-test.json` | Put back `foreach ($ch in 'live', 'test')` |
| `src-tauri\src\critic.rs` `CHECK`: `evidence: false` (#97 with #101's new `Check` field) | None needed: the build needs it |

## Card #110: search every card's session history (6 Nov 2026)

No board migration: `board.db` doesn't change. The index is its own
file, `search.db`, in the data folder (`%USERPROFILE%\.claude-kanban-dsh`, or `.claude-kanban-dsh-test` for
the test app), with its own migrations (`search1.sql`).

| Change | Undo |
|---|---|
| The index file and its rebuilds: `search.db` (and `search.new.db` while a month's new file is built) in the data folder, and one copy in `backups\search` | Quit the app (Hugh's say-so for live), then `Remove-Item "$env:USERPROFILE\.claude-kanban-dsh\search.db*", "$env:USERPROFILE\.claude-kanban-dsh\search.new.db*"; Remove-Item "$env:USERPROFILE\.claude-kanban-dsh\backups\search" -Recurse`. The index is only a copy: it is rebuilt from the transcripts on the next start |
| The index (`crates\store\src\search.rs`, `search1.sql`, `Store::all_sessions` in `sessions.rs`; tests in `crates\store\tests\search.rs`) | `git revert` the commit |
| The indexer thread, the hook nudge and the searches (`src-tauri\src\search.rs`; `search::on_hook` in `dispatch::on_hook`; the `search_sessions` and `search_status` commands in `main.rs`) | Same revert |
| `kanban_search` for callers outside a card session (`control.rs` `CONTROL`; `api.rs` `tool_list`) and `kb search` (`crates\kb\src\main.rs`) | Same revert |
| The Search sessions dialog (`ui\search.js`, `ui\search.css`, the button and dialog in `ui\index.html`) and Ctrl+F in a card panel no longer taking Ctrl+Shift+F (`ui\panel.js` `panelKeys`) | Same revert |
| SQLite built optimised in dev and test builds (`[profile.dev.package.libsqlite3-sys]` in `Cargo.toml`) | Same revert, or delete those three lines. Release builds were already optimised |
## Card #109: cards that wait on others (6 Nov 2026, branch `dsh-order`)

Built in the worktree `~\kanban-app-order` on the branch `dsh-order` (from
`dsh-agents`); main is untouched until Hugh ships.

| Change | Undo |
|---|---|
| The worktree and branch | `git -C $HOME\claude-kanban-dsh worktree remove $HOME\kanban-app-order --force; git -C $HOME\claude-kanban-dsh branch -D dsh-order` (only before it ships) |
| `crates\store\src\waits.rs`; the `blockedBy` and `waitedOn` cleaning in `cards.rs` (`apply`, `create`), the check in `write_tx`, `move_after_waits` and the private `How` in `move_as`; the join refusal in `stacks.rs` | `git revert` the commit. Waits already on cards stay in their fields, harmless: an older build shows and moves those cards as before and never acts on them |
| Migration 12 (`schema12.sql`): the partial index `cards_waiting` | Nothing to undo: it changes no row, and an older build keeps it up to date and ignores it. It is never dropped by hand (no direct edits of `board.db`) |
| `dispatch::pump_waits` in the 2-second tick, `wait_move_refused`, `TOLD` and `edit_message` for `blockedBy`, the note in `move_message` | Same revert. To stop the app acting on waits without a revert, take the waits off the cards (the ✕ in the panel, or `kb update <n> --blockedBy null`) |
| `tools.rs`: `blockedBy` in `UPDATABLE`, `waits_refused`, `blockedBy`/`waitedOn` in the list and stack output; `kb`'s usage line | Same revert |
| `ui\waits.js`, `ui\waits.css` and their two lines in `ui\index.html` | Same revert |
| `prompts\planning.md`: the guarded `blockedBy` line | Same revert |

## Card #114: the platform layer (6 Nov 2026)

No migration in this commit, and nothing in the data folder changes. Windows behaves as before:
the code was moved, not changed. (The `{prompts}` commit below is a change on purpose: the
prompt paths are built at run time from the home folder, and read the same on Hugh's PC.)

| Change | Undo |
|---|---|
| Windows-only code moved into `src-tauri\src\platform\windows.rs` behind the interface in `platform\mod.rs`; the call sites in `sessions.rs`, `tray.rs`, `place.rs`, `notify.rs`, `update.rs`, `launch.rs`, `slash.rs`, `stack.rs`, `session_cmds.rs`, `api.rs`, `transcript.rs` and `main.rs` call it | `git revert` the commit |
| `kanban_store::home_var()` (`crates\store\src\channel.rs`), the one place the home folder is read | Same revert |
| `windows` and `tauri-winrt-notification` are Windows-only dependencies (`src-tauri\Cargo.toml`) | Same revert |
| The guard test (`src-tauri\tests\platform_guard.rs`, `platform-allow.txt`) and the WSL tests in `stack.rs` marked Windows-only | Same revert |
| CI (`.github\workflows\ci.yml`) on branch `dsh-114` | Same revert. To stop it running on GitHub at once: `git push origin --delete dsh-114` (the branch only; never `main`) |
| Wave 5 merge (dsh-integration): the newer cards' Windows-only calls go through the platform module too: `no_window()` in `changes.rs`, `check_size.rs` and `small.rs`; `home_var()` in `configwatch.rs` and `testapp.rs`; `share_all`, `process_start` and `PID_LOCKS` (moved from `testapp.rs`); the toast's no-card click (card #104) in `platform\windows.rs`; text-only hits in `platform-allow.txt` | `git revert -m 1` the merge commit |
| Wave 5 merge: `{prompts}` in the board's prompts is filled from `repo_folder` (as #114 had it); the app's own messages (`back_from_test`, the stack plan message) from `PROMPTS_DIR` (card #96), so a test build still reads its own checkout's prompts | Same revert |

### Card #114: `{prompts}` (6 Nov 2026)

| Change | Undo |
|---|---|
| `{prompts}` is filled in by `dispatch::with_prompts` (`fill`, `back_from_test`, the stack plan message in `src-tauri\src\dispatch.rs`) | `git revert` the commit |
| Migration 13, `crates\store\src\schema13.sql` (the shipped column prompts and the "Kanban app" template name `{prompts}`; 11 is card #104's, 12 is card #109's). It runs after the pre-migration backup in `%USERPROFILE%\.claude-kanban-dsh\backups` | An older build opens the board (it ignores a newer version) but would send `{prompts}` as it is, so in that build press **Reset** on Planning, In progress, Ship it and Deploy complete (Settings › Columns), and edit the "Kanban app" template's start prompt back to the full path. Never edit `board.db` by hand |

## Card #117: macOS (6 Nov 2026)

No migration. On Windows nothing changes but two small things below. On a Mac, everything is
new; nobody has a Mac yet.

| Change | Undo |
|---|---|
| `src-tauri\src\platform\macos.rs`, the `kanban-unix` crate (`crates\unix`), kb's hidden `reap` and `install-lock` commands, the macOS dependencies and the `mac-devtools` feature in `src-tauri\Cargo.toml` | `git revert` the commit |
| Hooks that do nothing on Windows: `platform::builder` (main.rs), `tray_style`, `tray_count`, `TRAY_LEFT_CLICK_SHOWS` (tray.rs), `SECOND_ACCOUNT` (dispatch.rs, sessions.rs), `write_private` (api.rs, sessions.rs); the `app_platform` command; Mac branches in `ui\panel.js`; `ui\quit.js`; the failed-update notice in `update.rs` and `ui\update.js` | Same revert |
| Windows-visible: `api.json` has a `pid` field; Sessions reaps a dead session's child with `try_wait` when it drops it | Same revert |
| Guarded "On a Mac only" lines in `prompts\kanban-app.md`, `in-progress.md` and `ship-it.md` | Same revert (Windows sessions skip them) |
| `scripts\install.sh`, `mac-kb.sh`, `smoke-macos.sh`, `smoke\`, `mac-signing-cert.sh`; `src-tauri\tauri.macos*.json`, `tauri.smoke.conf.json`, `icons\tray-template.png` | Same revert |
| The Mac jobs in `.github\workflows\ci.yml` (only on `dsh-117` and manual runs) | Same revert. To stop them at once: `git push origin --delete dsh-117` (the branch only; never `main`) |
| On a Mac (when someone installs): `~/Applications/Claude Kanban DSH.app` and `Claude Kanban DSH Test.app`, `~/.local/bin/kb`, the data folders | Quit the app from the menu bar, then `rm -rf "$HOME/Applications/Claude Kanban DSH.app" "$HOME/Applications/Claude Kanban DSH Test.app" "$HOME/.local/bin/kb"`. Keep the data folders unless the board is no longer wanted |
| On a Mac, if `scripts/mac-signing-cert.sh` was run: the "Claude Kanban DSH Local" certificate and its trust | `security delete-identity -c "Claude Kanban DSH Local"` in Terminal (it also removes the trust setting) |
| This PC: the rustup targets `aarch64-apple-darwin` and `x86_64-apple-darwin` (INSTALLED.md) | `rustup target remove aarch64-apple-darwin x86_64-apple-darwin` |
| Wave 5 merge: `share_all`, `PID_LOCKS` and `process_start` for macOS (`src-tauri\src\platform\macos.rs`, as testapp.rs had them off Windows); a notification with no card shows the board only; `changes::is_wsl` takes a `/` path as WSL only where `DEV_STACKS` (Windows) | `git revert -m 1` the merge commit |

## Card #120: review mode, session search and the Agents feed (6 Nov 2026, panel part, branch `dsh-120`)

The page parts below change no data. The Rust parts ("Card #120: the Rust parts", below) read
data only: no migration, no schema change. Undo in PowerShell 5.1 from the repo folder, then
install the test app again (`scripts\install.ps1 -Channel test`).

| Change | Undo |
|---|---|
| Human test review mode over the whole panel (`ui\review.js`: the bar's head and actions, the review box, the Review/Chat tabs, Open the card; `htLive` skips card #0) and `ui\review-mode.css` | `git checkout main -- ui/review.js; Remove-Item ui\review-mode.css` |
| The check marks (✓ ✗ ○) in the "Done when" block, shown only in review mode (`ui\done-when.js`, `dwPanel`) | `git checkout main -- ui/done-when.js` (also drops the face agent's line change: re-apply it, or edit the one `mk` line out by hand) |
| The Agents feed and its drawer, the header's Agents button and Ctrl+Shift+A (`ui\agents-feed.js`, `ui\agents-feed.css`, `ui\rpane.css`), and its test `tools\ui-tests\feed.test.mjs` | `Remove-Item ui\agents-feed.js, ui\agents-feed.css, ui\rpane.css, tools\ui-tests\feed.test.mjs` |
| Session search restyled as the mockup's rows, and the median time bar under the results (`ui\search.js`, `ui\search.css`, `ui\times.js` `timesBarHtml`, `ui\times.css`) | `git checkout main -- ui/search.js ui/search.css ui/times.js ui/times.css` |
| `ui\index.html`: the block `<!-- card #120: review, search, feed -->` (three `<link>` and one `<script>`) | Delete those lines by hand (the face block beside it is another part of the card) |

## Card #120: the board face (6 Nov 2026)

No migration, no data change from the page. The Rust parts of card #120 have their own rows.

| Change | Undo |
|---|---|
| New `ui\face.js`, `ui\face.css`, `ui\filterbar.js`, `ui\filterbar.css` and their tags in the `<!-- card #120: board face -->` blocks of `ui\index.html` | `git revert` the commit, or `Remove-Item ui\face.js,ui\face.css,ui\filterbar.js,ui\filterbar.css -Confirm:$false` and delete both blocks from `ui\index.html` |
| `ui\board.js`: `isSystemCard` (card #0 never drawn or counted), `boardHooks.final`, the `is:stuck` term, `data-meta` on the meta tags. `ui\board.css`: the `--ask`, `--ask-soft`, `--warn-soft` and `--ag-*` tokens (in all three token blocks), the column head and card sizes | Same revert |
| `ui\needs.js`, `needs.css`: the wait box and the "To test" chip on the face; "Waiting on you" is a pill (the header button is gone) | Same revert |
| `ui\check-size.js`, `check-size.css`: "30-second looks" is a pill (the header button is gone) | Same revert |
| `ui\changes.js` (paths on the face tag), `ui\steps.js`, `steps.css`, `ui\done-when.css` (face styles) | Same revert |
| Tests: `tools\ui-tests\run.mjs`, `dom.mjs`, `harness.mjs`, `face.test.mjs` and `src-tauri\tests\ui_js.rs` | Same revert, or `Remove-Item tools\ui-tests\face.test.mjs,tools\ui-tests\harness.mjs,tools\ui-tests\dom.mjs -Confirm:$false` (keep `run.mjs` and `ui_js.rs` while other `*.test.mjs` files use them) |

## Card #120: the Rust parts and the tests (6 Nov 2026)

**The simplest correct undo for the whole card is a revert of its commit**, then a test install:
`git revert <card #120 commit>; powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Channel test`.
The rows below undo one part at a time.

No migration: the feed's indexes were left out because card #119 takes migration 14 (see HANDOVER).

| Change | Undo |
|---|---|
| `crates\store\src\feed.rs` (`Store::agents_feed`, the agent filter as `by IN (...)`) and its `mod feed` / `pub use feed::...` lines in `crates\store\src\lib.rs` | `Remove-Item crates\store\src\feed.rs -Confirm:$false`, then delete the two `feed` lines from `lib.rs` by hand |
| `crates\store\tests\feed.rs` (the feed tests, the rare-agent page) | `Remove-Item crates\store\tests\feed.rs -Confirm:$false` |
| `src-tauri\src\feed.rs` (the `agents_feed` command), and `mod feed;` and `feed::agents_feed` in `src-tauri\src\main.rs` | `Remove-Item src-tauri\src\feed.rs -Confirm:$false`, then delete those two lines from `main.rs` by hand |
| `crates\store\src\times.rs`: `Visit.start`, `column_since`, `iso`, `Store::column_since_all`, `column_since_of`, `history_marker`; `column_since` in its `pub use` in `lib.rs`; their tests in `crates\store\tests\times.rs` | `git checkout main -- crates/store/src/times.rs crates/store/tests/times.rs`, and take `column_since` out of the `pub use times::...` line in `lib.rs` |
| `src-tauri\src\main.rs`: `list_cards`' `columnSince`, the `ColumnSince` cache (`.manage(ColumnSince::default())`) and `fn column_since` | Put `list_cards` back to returning `doc(&store, c)` for each card, and delete `ColumnSince`, `SinceMap`, `fn column_since` and the `.manage` line by hand (or `git checkout main -- src-tauri/src/main.rs` when the feed lines go too) |
| `src-tauri\src\changes.rs`: `Brief.paths`, `top_paths`, `BRIEF_PATHS` and their test | `git checkout main -- src-tauri/src/changes.rs` |
| `src-tauri\tests\ui_js.rs` (runs the UI tests in `cargo test`) and `tools\ui-tests\` (`run.mjs`, `dom.mjs`, `harness.mjs`, `face.test.mjs`, `feed.test.mjs`) | `Remove-Item src-tauri\tests\ui_js.rs -Confirm:$false; Remove-Item tools\ui-tests -Recurse -Confirm:$false` |
## Card #119: the Router session (6 Nov 2026, branch `dsh-119`)

| Change | Undo |
|---|---|
| Migration 14, `crates\store\src\schema14.sql`: the append-only tables `router_sessions` and `router_log`. It runs after the pre-migration backup in `%USERPROFILE%\.claude-kanban-dsh\backups` | An older build opens the board as it is and never reads the tables. Nothing to undo; never edit `board.db` by hand. To go back to the pre-migration board, quit the app and restore that backup (Hugh only) |
| The Router's settings `router.on`, `router.model`, `router.effort`, `router.may_start_cards`, `router.resume_after`, `router.last_opened` | Press **Stop** in the Router panel (clears `router.on`, so it is not resumed); set the others back in Settings › Router |
| The Router's folder for each channel, made on its first start: `%USERPROFILE%\kanban-router` (live), `kanban-router-test`, `kanban-router-test2` ... (test slots) | Stop that app's Router, then `Remove-Item "$env:USERPROFILE\kanban-router" -Recurse -Force` (or the test slot's folder) |
| `src-tauri\src\routersession.rs`, `routertools.rs`, the Router's guard (`guard.rs`), `Plan.router` and `start_as` (`sessions.rs`), the dispatcher's tick (`dispatch.rs`), `own_or_named` (`agents.rs`, `merger.rs`, `repro.rs`, `router.rs`), the Monitor's hand-off (`monitor.rs`), the merger's integrate job (`merger.rs`), `kanban_router` (`tools.rs`, `api.rs`), `kb router` (`crates\kb`) | `git revert` the card's commits |
| `prompts\router.md` (the Router's system prompt) | Same revert |
| `ui\router.js`, `ui\router.css`, the Router section in `ui\settings.js`, three lines in `ui\index.html` | Same revert |
| Final-test fixes (`d71512c` and after): a Router move runs the card's after-move (its session starts or hears the move), `kanban_start_card` without a worktree uses the repo folder, the open-panel rule without focus, the pill, the Router log in `agents_feed` (`crates\store\src\feed.rs`), the trust buttons, `dryRun` on create, the Router toast, `kb sessions` "router", a folder per channel | `git revert` those commits. Nothing in the board to undo: the feed only reads `router_log` |
| Every event the Router is given is now a `router_log` row (append-only; it grows with the board's activity) | Stop the Router: nothing more is written. The rows stay (the table refuses deletes) |

## Card #120: final-test fixes (6 Nov 2026)

| Change | Undo |
|---|---|
| `ui\board.css` `.col{flex:1 0 240px;min-width:0;...}` | Set it back to `.col{flex:0 0 240px;...}` (fixed 240 px columns, left-aligned) |
| `ui\face.css` `.card.face .face-agent{width:0;min-width:100%}` | Delete the line (do not, without `min-width:0` on `.col`: a long agent line then stretches its column) |
| `crates\store\src\feed.rs`: log rows of a verdict's own write left out (`NOT EXISTS ... card_revisions`) | Set the `card_log` branch's filter back to `"1".to_string()`; delete the test `a_verdict_shows_once_not_also_as_its_own_log_line` in `crates\store\tests\feed.rs` |
| `ui\agents-feed.js` `verdictHtml`: own field not named | Delete the line under the comment "The field is named only when..." and set the feed test back to `TESTER passed` |

## Card #122: the test-app hang under load (6 Nov 2026, branch `dsh-122`)

| Change | Undo |
|---|---|
| `src-tauri\src\notify.rs`: `looking_at` reads a focus flag (`FOCUSED`, kept by `notify::on_window_event` from `WindowEvent::Focused`) instead of asking the window `is_focused()` | `git revert` the card's commit. Do not put `is_focused()` back on its own: under the board lock it deadlocks the app |
| `src-tauri\src\lockwatch.rs` (`Watched<T>`, the watchdog) and `Board` / `Sessions.live` using it (`main.rs`, `sessions.rs`); `lockwatch::start` in `main.rs` setup | Same revert. The watchdog writes `locks.log` in the data folder; deleting that file by hand is not needed (it is only appended to) |
| `src-tauri\src\hang_tests.rs`, the test `no_window_getter_off_the_main_thread` in `notify.rs`, `tools\stress-api.mjs` | Same revert, or `Remove-Item src-tauri\src\hang_tests.rs, tools\stress-api.mjs -Confirm:$false` and delete `mod hang_tests;` from `main.rs` |

## The DSH port to the public repo

| Change | Undo |
|---|---|
| The merger takes a repo's merge rules and check commands from its brief or the repo's CLAUDE.md / AGENTS.md, not from rules built in for one repo (`src-tauri\src\merger.rs`) | Put the rules back in the merger's brief through your private `ship-it.md`, or `git revert` the commit |
| The size check's sign-in folders come from `loginDirs` in the private folder's `repos.json` (`check_size.rs`, `private.rs`) | Remove `loginDirs` from `repos.json`: a change then needs a sign-in only for its logins or dev stack |
| Migration 13 matches the old prompt wording around any folder, not one person's path | Edit a column's prompt in Settings; the migration never touches an edited prompt |
| Sign-in links are hidden when their URL holds `signin`, `sign-in`, `login` or `log-in` (`secret.rs`, `search.rs`, `done_when.rs`, `monitor.rs`, `steps.rs`) | `git revert` the commit; the patterns only hide more, never less |
| `import-from.txt` may set page settings on the imported board (`key=value` lines), and copies every file of the old data folder but its port, token, log and staged update | Quit the app, move `board.db` out of the new data folder; the old folder is only read |
| `install.ps1 -StageVia <old data folder>` stages a renamed build through the old app | Delete `update\update.json` in that folder before Restart now |
| The example prompts are installed beside the app (`prompts\` in the install folder, `bundle.resources`) | Set a private folder; it wins over the installed examples |

## Card worktrees inside the checkout (6 Nov 2026)

Undo (PowerShell 5.1): `git -C ~\claude-kanban-dsh revert --no-edit <this commit>`

## Card #131: the Done pile, the Router's asks to archive (8 Nov 2026, branch `card-131`)

| Change | Undo |
|---|---|
| `crates\store\src\archive_ask.rs`, `Store::revisions_after`, the stale-ask removal in `write_tx` (`cards.rs`) | `git -C ~\claude-kanban-dsh revert --no-edit <the card #131 commits>`. Cards keep any `archiveAsk` / `archiveAskDeclined` field; an older build ignores both |
| `kanban_archive_card` (`routertools.rs`), in `agents::CARD_TOOLS` | Same revert |
| `router_archive_approve`, `router_archive_dismiss` (`main.rs`) | Same revert |
| `ui\archive-ask.js`, `ui\archive-ask.css`, their tags in `ui\index.html`, the Done-pile filter in `ui\board.js` render | Same revert. Without it, cards with an ask show in their own columns again |
| The Router prompt's job 6 and rule 1's sentence (`prompts\router.md`; the private copy in the private repo) | Same revert here; in your private repo, `git revert --no-edit` its card #131 commit |
| A card archived from the Done pile | Open it in the Archive and press **Restore** |

## Card #151: each card's choice of subagents (9 Nov 2026, branch `card-151`)

| Change | Undo |
|---|---|
| The card field `subagents`, stamped on every new card (`crates\store\src\subagents.rs`, `cards.rs`) | `git -C ~\claude-kanban-dsh revert --no-edit <the card #151 commits>`. Cards keep the field; an older build ignores it |
| The gates read the card's choice (`critic.rs`, `reviewer.rs`, `tester.rs`, `repro.rs`, `main.rs`, `stacks.rs`) | Same revert: the four board-wide rules hold every card again |
| Two new settings, `sessions.router_in_planning` and `sessions.merger_at_ship` | Same revert; the saved rows are only read by this build |
| The session notes (`dispatch.rs` `subagents_note`, `subagents_changed`; `stackrun.rs`) | Same revert: card #97's `CRITIQUE_OFF` note comes back |
| `ui\subagents.js`, `ui\subagents.css`, their tags in `ui\index.html`, the Add card ticks and details box in `ui\board.js`, the Settings group in `ui\settings.js` | Same revert |
| The Router prompt's jobs 1 and 3 (`prompts\router.md`) | Same revert |
| One card's choice | Change it back in that card's details (the change is logged on the card) |

## Card #95: the terminal takes typing (9 Nov 2026)

| Change | Undo |
|---|---|
| `Ring` counts the session's output (`total`), `pty` events carry `run` and `end`, `session_snapshot` returns `{run, end, data}`, and the new `session_run` (`sessions.rs`, `session_cmds.rs`, `main.rs`) | `git -C ~\claude-kanban-dsh revert --no-edit <the card #95 commits, newest first>`. Nothing is stored: it is all in memory |
| `ui\termlink.js` and its tag in `ui\index.html`; the card panel (`ui\panel.js`) and the Router pane (`ui\router.js`) use it | Same revert. The terminals go back to replaying the whole ring on every refresh |
| `tools\ui-tests\termlink.test.mjs` | Same revert |

## Router panel: paste and send images (7 Nov 2026, card #127)

UI only; nothing on the board or in the data folder changes (saved images are plain files in `attachments/router` in the data folder).

Undo (PowerShell 5.1): `git -C ~\claude-kanban-dsh revert --no-edit <this commit>`

## A gentler plan critic (9 Nov 2026, card #144)

| Change | Undo |
|---|---|
| The critic's prompt, notes after a PASS, the HUGH: and small-card holds, build notes in the In progress message, one critique for a small card (`critic.rs`, `agents.rs`, `router.rs`, `dispatch.rs`, `crates/store/src/cards.rs`), with tests, the Settings help, the example prompts and the docs | `git -C ~\claude-kanban-dsh revert --no-edit <the card #144 commit>`, then install as usual. No data change: a record's `notes` and `hugh` keys are ignored by an older build. |

## Card #132: a card keeps its place after its need clears (9 Nov 2026, branch `dsh-132`)

| Change | Undo |
|---|---|
| Held places in `ui\needs.js` (`pinFirst`, `loadNeeds`, `holdOf`, `pruneHolds`, `endHolds`, `heldIn`) | `git -C ~\claude-kanban-dsh revert --no-edit <the card #132 commit>`. The board's `localStorage` key `wb.heldPlaces` stays in the WebView profile; an older build ignores it |
| `placeCard` in `ui\board.js` works on the column as shown; a drop among the pinned cards sets held places instead of writing orders | Same revert. Card `order` values a drag wrote stay, as after any drag |
| `tools\ui-tests\order.test.mjs`, the `storage` option of `boot` in `tools\ui-tests\harness.mjs` | Same revert |
| `docs\GUIDE.md`, `HANDOVER.md` text | Same revert |

## A test app per card (9 Nov 2026, card #147)

Undo (PowerShell 5.1): `git -C ~\claude-kanban-dsh revert --no-edit <the card's commit>`, then
reinstall the test apps you use. Nothing to migrate: `testApp`, `testapp.slots`,
`testapp.slot_line` and `testapp.queue.N` are a card field and settings an older build ignores
(`testapp.queue` stays Test's). Files in `%USERPROFILE%\.claude-kanban-dsh-shared\testapp\` that
an older build doesn't write (`input-test2.json` and the like) are harmless and can be left.

| Change | Undo |
|---|---|
| The store's test app per card, slot line, per-slot queues, `testapp.slots` (`crates\store\src\testapp.rs`, `config.rs`, `lib.rs`) | The revert |
| The app's per-slot pump, turn file `slots`, `kanban_test_app` free and slot, `testapp_free`, input per test app (`src-tauri\src\testapp.rs`, `platform\*.rs`, `main.rs`) | The revert |
| `kb testapp --slot N`, `kb testapp free --slot N` | The revert |
| `install.ps1`: per-slot turn and input waits, the owner wait, `channel` in the lock | The revert; an older script still meets the new apps on Test |
| The page: tile chips, popover per test app, card details row, Settings box | The revert |

## Card #146: each card in its own worktree (9 Nov 2026, stack #145, branch `dsh-146`)

PowerShell 5.1 commands. `<checkout>` is the repo's checkout named in the setting, `<n>` the card's number.

| Change | Undo |
|---|---|
| The setting `sessions.own_worktree_repos` (Settings > Sessions, "Build each card in its own worktree") | Remove every row in Settings and Save (empty is off). New sessions then start in the repo's folder, as before |
| A card's worktree, made by the app (`<checkout>\.claude\worktrees\card-<n>`, branch `card-<n>`; `test-card-<n>` and so on from a test app) | Once the card's work is shipped or no longer wanted: `git -C <checkout> worktree remove <checkout>\.claude\worktrees\card-<n>` (it refuses while there is uncommitted work), then `git -C <checkout> branch -d card-<n>`. Clear the card's `worktreePath` and `branch` with `kb update <n>` if it lives on |
| `/.claude/worktrees/` added to a checkout's `.git\info\exclude`, only where git didn't ignore it already | Delete the two lines under "# Card worktrees the kanban app makes (card #146)" from `<checkout>\.git\info\exclude` |
| `start_session` makes the worktree, the note in the session's messages (`session_cmds.rs`, `dispatch.rs`, `src-tauri\src\worktree.rs`), the store setting (`crates\store\src\config.rs`), the Settings box (`ui\settings.js`) | `git revert` the card's commits (marked `(card #146)`) |
| A stack child's note names `<checkout>\.claude\worktrees\card-<n>` (`stackrun::child_worktree`), not `<checkout>-<n>` | The same revert puts back `<checkout>-<n>` |

## Card #148: the rules for parallel builds (9 Nov 2026, stack #145, branch `dsh-148`)

Docs, prompts and one test; no app code, no data. PowerShell 5.1 commands.

| Change | Undo |
|---|---|
| The rule text in `AGENTS.md`, `HANDOVER.md`, `PLAN.md`, `docs/GUIDE.md` and the bundled `prompts/kanban-app.md`, `planning.md`, `in-progress.md` and `router.md` (the Router's job 5 included, which is its system prompt once built) | `git -C ~\claude-kanban-dsh revert --no-edit <the card's commits, marked (card #148)>`, then stage the live update as usual |
| The test `src-tauri\tests\prompt_rules.rs` | The same revert |
| The owner's private copies of the same prompts | Revert the card's commit in the private repo (its own `UNDO.md` has the line) |

## The app's own messages in the chat (9 Nov 2026, card #153)

| Change | Undo |
|---|---|
| Migration 15: the append-only `app_messages` table (`crates\store\src\schema15.sql`), `Store::queue_hugh`, `note_app_message`, `app_messages`, `split_app_parts` | `git -C ~\claude-kanban-dsh revert --no-edit <the card #153 commit>`. The board keeps the table at version 15; an older build opens it as it is and never reads it. The backup taken before the migration is in the data folder's `backups` (`*-pre-migration.db`) |
| The chat's centred "Automatic" messages (`ui\panel.js` `userHtml`, `ui\router.js`, `ui\panel.css`, `ui\router.css`) | Same revert |

## Size cards before their session starts (9 Nov 2026, card #159)

| Change | Undo |
|---|---|
| `src-tauri\src\presize.rs`, `begin_session`'s sizing step and `Begun::Sizing`, `router::record_with` and `router::RULES`, `Sessions::adopt`, the setting `sessions.size_before_start`, the "Sizing…" chip, the panel's start row and the Settings switch | Turn it off without a build: Settings > Sessions > untick "Size cards before their session starts". Or `git -C ~\claude-kanban-dsh revert --no-edit <the card #159 commits>`, then stage the live update as usual. A card left with a `sizing` field is harmless to an older build, which never reads it |
| The bundled `prompts\planning.md` step 1 (a card that already has a route goes on from step 3) | The same revert |
| The owner's private `prompts\planning.md`, the same line | Revert the card #159 commit in the private repo |

## Sizing off means no sizing (9 Nov 2026, card #161, branch `card-161`)

| Change | Undo |
|---|---|
| `presize.rs` (`unsized_why`, `unsized_note`, the failure note and `failed_line`, `TURNED_OFF`), `dispatch::subagents_note`'s Planning note, `router::record_with`'s refusal while off and `router::again_blocker`, `session_cmds::route_again`, the panel's Route row (`board.js` `sizeFirstOff`), the Settings help texts, `tools/ui-tests/sizing.test.mjs` | `git -C ~\claude-kanban-dsh revert --no-edit <the card #161 commits>`, then stage the live update as usual (`powershell -ExecutionPolicy Bypass -File scripts\install.ps1`). No data changed: nothing to clean up |
| The bundled `prompts\planning.md` (*Route it first*: skip it when the card isn't sized) | The same revert |
| The owner's private `prompts\planning.md` and `prompts\kanban-app.md`, the same rule | Revert the card #161 commit in the private repo |

## One merge lane per repo, the backend (9 Nov 2026, card #160, branch `card-160`)

PowerShell 5.1 commands. `<checkout>` is the checkout named in the merge-lane setting, `<repo>` its `owner/name`.

| Change | Undo |
|---|---|
| The setting `sessions.merge_lane_repos` (empty by default: no repo ships through a lane), and `lanes.model` / `lanes.effort` | Empty the list in Settings and Save. Every repo then ships with the per-card card-merger, as before |
| Migration 16: the append-only `lane_sessions` table (`crates\store\src\schema16.sql`, `crates\store\src\lanes.rs`) | `git -C ~\claude-kanban-dsh revert --no-edit <the card #160 commits>`. The board keeps the table at version 16; an older build opens it as it is and never reads it, so it is harmless left in place. The backup taken before the migration is in the data folder's `backups` (`*-pre-migration.db`) |
| A lane's job on a card's `merge` field (states `queued`, `running`, `cancelled`, and `lane`, `base`, `startedAt`, `onto` beside the card-merger's) | Nothing to undo: an older build shows an unknown state as no chip, and every move into Ship it clears the field |
| A lane's worktree, made by the app (`<checkout>\.claude\worktrees\merge-lane`, `<channel>-merge-lane` from a test app), detached, no branch | Once no lane runs: `git -C <checkout> worktree remove --force <checkout>\.claude\worktrees\merge-lane` (it holds only the lane's own commits, already reported by SHA) |
| A lane's session folder and conversation (`%USERPROFILE%\.claude-kanban-dsh\sessions\<row>\`, the transcript under the main account) | Leave them: they are the lane's history. Nothing reads them once the list is empty |
| The lane itself (`src-tauri\src\lane.rs`, `lanetools.rs`, `prompts\merge-lane.md`), `kanban_merge` (`tools.rs`, `api.rs`, `routertools.rs`), `sessions::is_app_key` and the lane plan (`sessions.rs`), `guard::lane_refusal` (`guard.rs`), the Ship it note (`dispatch.rs`), the feed's "merge lane" writers (`feed.rs`, both), `lane_status` / `lane_stop` (`main.rs`) | The same revert, then stage the live update as usual |
| The UI: Settings › Sessions' "Ship through one merge lane per repo" box with each lane's state and Stop, on the list editor now shared with the own-worktree box (`repoListBox`, `ui\settings.js`, `ui\settings.css`); the Merger chips Waiting for the lane, Merging and Merge cancelled (`ui\board.js` `CHECKS`, `ui\board.css`); `tools\ui-tests\lane.test.mjs` | `git -C ~\claude-kanban-dsh revert --no-edit <the card #160 UI commit>`, then stage the live update. Nothing is stored by the UI itself |
| The prompt tests (`src-tauri\src\merger.rs`) and the docs (HANDOVER.md, README.md, AGENTS.md, docs\GUIDE.md, PLAN.md, ACCEPTANCE.md, this file) | `git -C ~\claude-kanban-dsh revert --no-edit <the card #160 test and docs commits>`. Text only: nothing to stage |

## Card #157
Undo: `git revert <card-157 commit>`; the Router then hears of new Backlog cards again.
## Queued messages go in mid-turn (9 Nov 2026, card #154)

Revert the fix's commit, then reinstall (test first):

```powershell
cd $env:USERPROFILE\claude-kanban-dsh
git revert --no-edit <the #154 commit>
powershell -ExecutionPolicy Bypass -File scripts/install.ps1 -Channel test
```

## Card #130: column widths you can drag (9 Nov 2026, branch `dsh-130`)

| Change | Undo |
|---|---|
| `ui\colwidths.js` (the edges, the drag, the saved weights), its tag in `ui\index.html`, `.col` and `.colgrip` in `ui\board.css` | `git -C ~\claude-kanban-dsh revert --no-edit <the card #130 commits>`. Columns go back to equal widths with a 240px floor |
| `tools\cdp.mjs viewport`, the fake DOM's `style.setProperty`, pointer-event fields, window listeners and storage in `tools\ui-tests` | Same revert |
| The widths you set | Stored in the board's WebView only (`localStorage` key `wb.colWeights`). Press an edge twice to reset them, or in DevTools: `localStorage.removeItem("wb.colWeights")`. A leftover key is harmless |

## Repos a card can be for, in Settings (9 Nov 2026, card #155, branch `card-155`)

| Change | Undo |
|---|---|
| Settings › Sessions › "Repos a card can be for" (`reposBox`, `ui\settings.js`, `ui\settings.css`), the `save_repos` command (`src-tauri\src\settings.rs`, `main.rs`), `private::save_repos_in` (`src-tauri\src\private.rs`), `tools\ui-tests\repos.test.mjs` and the docs | `git -C ~\claude-kanban-dsh revert --no-edit <the card #155 commit>`, then stage the live update. The list is then edited by hand in `repos.json` again |
| A Save there rewrites `<private folder>\repos.json`, keeping the one before as `repos.json.bak` | `Copy-Item <private folder>\repos.json.bak <private folder>\repos.json -Force` (only the last Save is kept). In a git repo, `git -C <private folder> checkout -- repos.json` |

## The to-do list in the chat (9 Nov 2026, card #158, branch `card-158`)

| Change | Undo |
|---|---|
| The chat's to-do box (`ui\steps.js` `stepsChat`, `ui\steps.css`, the `#p-todo` row in `ui\panel.js` and `ui\panel.css`) and the worded TaskCreate/TaskUpdate/TaskGet lines (`src-tauri\src\transcript.rs` `key_input`) | `git -C ~\claude-kanban-dsh revert --no-edit <the card #158 commit>`, then stage the live update as usual. Nothing is stored but the fold, `localStorage["panel.todo"]` in the app's WebView, which an older build never reads |
| The docs (HANDOVER.md, docs\GUIDE.md, this file) | The same revert |
## The reviewer and the tester run together (9 Nov 2026, card #162, parallel review and test)

PowerShell 5.1 commands. No migration and no new setting.

| Change | Undo |
|---|---|
| The rule: `agents::stale_pass` in `GATES` (`src-tauri\src\agents.rs`), `kanban_store::stale_passes` (`crates\store\src\pair.rs`) and the stack lead's hold in `lead_missing` (`crates\store\src\stacks.rs`) | `git -C ~\claude-kanban-dsh revert --no-edit <the card #162 code commit>`, then stage the live update as usual. To stop the hold without a build, untick the card-reviewer or the card-tester for the card (the rule needs both on) |
| `startedAt` and `failedAt` on the `test` and `review` records (and the other PASS/FAIL agents'), and `agents::on_pre_tool` noting every run's start | The same revert. The fields are harmless left on cards: an older build never reads them, and the next move into In progress drops them |
| Review fix: a run's start is kept with the card its brief names (`CARD: #n`) and a verdict takes its own card's start; starts over 6 hours old are dropped (`agents::take_start`, `START_MAX_AGE_HOURS`) | `git -C ~\claude-kanban-dsh revert --no-edit <the card #162 start-matching commit>`. Nothing stored: the starts live only in memory |
| The prompts: `prompts\in-progress.md` *Review and test it together*, `prompts\kanban-app.md` In progress 4a and 6a and its stack section; the reviewer's and tester's descriptions and gate texts (`reviewer.rs`, `tester.rs`) | `git -C ~\claude-kanban-dsh revert --no-edit <the card #162 prompts commit>`. The owner's private prompts change in the private repo: revert the card #162 commit there |
| The tests (`crates\store\tests\pair.rs`, `src-tauri\tests\prompt_rules.rs`) and the docs (HANDOVER.md, README.md, docs\GUIDE.md, PLAN.md, ACCEPTANCE.md, this file) | `git -C ~\claude-kanban-dsh revert --no-edit <the card #162 docs commit>`. Text only: nothing to stage |

## The merge lane everywhere (9 Nov 2026, card #162)

PowerShell 5.1 commands. No migration and no new stored field beyond `merge.checkout` on a lane job.

| Change | Undo |
|---|---|
| No `sessions.merge_lane_repos` setting and no list in Settings: every card with a branch, on every repo, ships through its repo's merge lane (`crates\store\src\lanes.rs`, `config.rs`, `ui\settings.js`) | `git -C ~\claude-kanban-dsh revert --no-edit <the card #162 commits>`, then stage the live update as usual. A list card #160's build saved is still on the board, untouched, so the reverted build reads it again |
| The lane's checkout found at `kanban_merge` (`lane::find_checkout`, `private::folder_for`) and kept on the job as `merge.checkout`; lanes keyed by the checkout's GitHub `origin` | The same revert. `merge.checkout` is harmless to an older build (it reads only the fields it knows), and every move into Ship it clears `merge` |
| Review fix: a lane's repo is read from the origin URL's real host (`scheme://[user@]host[:port]/...` or scp's `host:...`), github.com only (`lane::github_repo`) | `git -C ~/claude-kanban-dsh revert --no-edit <the card #162 github_repo commit>` |
| The card-merger off the card's subagents: no `merger` row, no `sessions.merger_at_ship` default (`crates\store\src\subagents.rs`, `ui\subagents.js`); its prompt refuses a ship and its report on a Ship it card isn't recorded (`src-tauri\src\merger.rs`) | The same revert. A saved `sessions.merger_at_ship` value and each card's stored `merger` choice were never touched, so they hold again after it |
| The Ship it step in the bundled `prompts\ship-it.md` and `prompts\kanban-app.md` (the lane path only), `prompts\router.md` job 5 | The same revert |
| Review fix: the lane's prompt (`prompts/merge-lane.md` step 5) merges origin/main once into a branch with merge commits (an integration branch) instead of rebasing it, the card-merger's old rule | `git -C ~/claude-kanban-dsh revert --no-edit <the card #162 integration-branch commit>` |
| The owner's private copies of `ship-it.md` and `kanban-app.md` | Revert the card #162 commit in the private repo |
| A lane worktree made in a checkout no list named before (`<checkout>\.claude\worktrees\merge-lane`, `<channel>-merge-lane` from a test app) | Once no lane runs: `git -C <checkout> worktree remove --force <checkout>\.claude\worktrees\merge-lane` |

## Card #162: card flow options, skip Planning, skip Human test, three-way subagents (9 Nov 2026)

PowerShell 5.1 commands. Nothing here deletes a row: every change below is an ordinary card write with its revision, or a setting.

| Change | Undo |
|---|---|
| A card's `subagents` holds only Hugh's overrides, null for a key back to the default (`crates\store\src\subagents.rs`, `cards.rs`) | `git -C ~\claude-kanban-dsh revert --no-edit <the card #162 commits, newest first>`. A card with no override then follows the defaults as #151's cards made before it did |
| Migration 17: stamped subagent values equal to the default taken off non-archived cards (`schema17.sql`, `subagents::unstamp_defaults`, run by `Store::migrate`) | The board from before it is in `%USERPROFILE%\.claude-kanban-dsh\backups\<stamp>-pre-migration.db`. To put one card's old values back, set them in its Card flow (each is in its revisions, by "the app (migration 17 ...)"). An older build opens the board as it is |
| Review fix: migration 17 writes only `subagents` (and `updatedAt`/`updatedBy` with its revision) through `cards::write_fields_tx`, none of `write_tx`'s rules, so a card in Done isn't archived by it | `git -C ~/claude-kanban-dsh revert --no-edit <the card #162 migration-write commit>`. A board already at version 17 stays as it is |
| The settings `sessions.skip_planning` and `sessions.skip_human_test` (off by default) | Untick them in Settings › Sessions › Card flow defaults. The saved rows are only read by this build |
| The card fields `skipPlanning` and `skipHumanTest` | Set each back to Default in the card's Card flow (logged on the card); or, from your own session, `kb update <n> --skip-planning default --skip-human-test default` |
| The skip: `cards.rs` `planning_skip` in `move_as`, `dispatch::session_move_to`, the `plan` record `{state: "skipped"}`, `critic::gate` | The same revert. A card already moved to In progress stays there: drag it to Planning to plan it |
| Skip Human test in `dispatch::auto_progress` (`skip_human_test_why`, `Store::arrival`), and small-change auto-ship leaving such cards out (`small.rs`, `goes_on.rs`) | The same revert. A card already in Ship it: drag it back to Human test (its ship request is cancelled) |
| The session notes (`dispatch.rs` `flow_note`, `skips_changed_note`, `flow_log_lines`, `after_default_change`; `settings.rs`) | The same revert |
| `kanban_update_card` takes the flow from Hugh's own sessions and `kb` (`tools.rs` `not_updatable_by`), the `flow` object in `kanban_get_card`, `kb update --skip-planning`, `--skip-human-test`, `--subagent` (`crates\kb`) | The same revert |
| Review fix: `kb update --subagent` refuses a retired (`merger`) or unknown name with a plain message instead of dropping it (`crates/kb` `subagent_key`) | `git -C ~/claude-kanban-dsh revert --no-edit <the card #162 kb subagent commit>` |
| `ui\subagents.js` (Card flow box, Add card, chips, `CARD_FLOW_SKIPS`), `ui\subagents.css`, Add card and the move status in `ui\board.js`, Settings' "Card flow defaults" in `ui\settings.js`, `tools\ui-tests\flow.test.mjs` | The same revert |
| The docs (README.md, HANDOVER.md, docs\GUIDE.md, PLAN.md decisions 29 to 32, ACCEPTANCE.md 162.x, this section) | The same revert |
| Review fixes: a session's own held move into In progress gets the build prompt only for Take it to or a skipped Planning, never for skip Human test alone (`dispatch::held_move_message`) | `git -C ~\claude-kanban-dsh revert --no-edit <the card #162 review-fix commit>` |
| Review fixes: Hugh's move into Human test from `kb` or his own session outside the app never skips it, as his drag doesn't (`dispatch::hughs_own`) | The same revert |
| Review fixes: turning the Skip Human test default on moves on the cards that follow it and wait in Human test where a session put them (`dispatch::default_skip_waiting`) | The same revert. A card already moved on: drag it back to Human test |
| Review fixes: a caller naming a session the app doesn't know can't set the card's flow, and writes as "a session the app doesn't know" (`tools::is_hugh`, `UNKNOWN_BY`) | The same revert |


## Router panel: log and chat side by side (card #128, branch `dsh-128`)

UI only; nothing on the board or in the data folder changes. The saved width (`router.width` in the WebView's localStorage) is only read.

Undo (PowerShell 5.1): `git -C ~\claude-kanban-dsh revert --no-edit <the card #128 commit>`. A width saved while this was live still works in the old layout.

## Agents drawer beside the Router panel (9 Nov 2026, card #167)

UI only, no data. `git -C ~\claude-kanban-dsh revert --no-edit <the card #167 commit>`, then stage the live update. Files: `ui\router.css`, `ui\rpane.css`, `ui\router.js`, `ui\agents-feed.js`.

## Card #172: no computer use and no browser for this app's cards (9 Nov 2026)

| Change | Undo |
|---|---|
| `src-tauri\src\nobrowser.rs`; `Plan.no_browser` set in `session_cmds.rs`; `settings_json` adds `permissions.deny` and `mcp_json` drops the headless chrome-devtools for this repo's cards (`sessions.rs`) | `git -C ~\claude-kanban-dsh revert --no-edit <the card #172 commit>`, then stage the update. Sessions already running keep their settings until they restart |
| The card-tester and card-reproducer prompts (`tester.rs`, `repro.rs`): no computer use and no browser for this repo's cards; the tester's evidence step names `hughht5/claude-kanban-dsh` too | The same revert |
| `prompts\kanban-app.md` In progress step 6, `AGENTS.md` Hard rules, README, HANDOVER, PLAN, `src-tauri\tests\prompt_rules.rs` | The same revert |
| The private folder's `prompts\kanban-app.md` In progress step 6 (branch `card-172` in its own repo) | Revert its card #172 commit in the private repo, then push that repo's `main` |
| To keep the rule but let one more repo in or out | Edit `REPOS` in `nobrowser.rs` |

## Card #171: timing tests allow for a busy machine (9 Nov 2026)

Tests only: the app's behaviour is unchanged (the live budget for a worktree's git calls stays 15 s).

| Change | Undo |
|---|---|
| `hang_tests::the_focus_flag_under_the_board_lock_never_hangs` runs until the main thread has run 101 commands (up to 60 s) instead of a fixed 2 s; `load` takes `enough` | `git -C ~\claude-kanban-dsh revert --no-edit <the card #171 commit>` |
| `worktree.rs`: a test build's `MAKE_LIMIT` is 300 s (`#[cfg(test)]`), the error text names the budget the call had (`Git.limit`), and the test `git_past_its_limit_is_not_run` | The same revert |

## Card #176: the card-merger accepts a switched-off reviewer (9 Nov 2026)

Prompt text only, no data. `git -C ~\claude-kanban-dsh revert --no-edit <the card #176 commit>`, then stage the live update. Files: `src-tauri\src\merger.rs` (prompt and test), `src-tauri\src\stackrun.rs` (merge brief), `HANDOVER.md`. A lead session already running keeps the old merger prompt until it restarts.

## Card #126: kb.exe replaced while in use (6 Nov 2026, branch `dsh-126`)

| Change | Undo |
|---|---|
| `src-tauri\windows\hooks.nsh`: the installer deletes old `kb.old-*.exe` copies, renames kb.exe to a free `kb.old-N.exe` (N up to 99), writes the new kb.exe, then deletes the moved copy if nothing runs it (or moves it back if the write failed); uninstall also deletes `kb.old-*.exe` and the emptied folder | `git revert` the card's commit. Leftover `kb.old-N.exe` files are only old copies, in the live folder and every test slot: `Get-ChildItem "$env:USERPROFILE\Programs\Claude Kanban DSH*\kb.old-*.exe" -ErrorAction SilentlyContinue \| Remove-Item -ErrorAction SilentlyContinue` (those still in use stay) |
| `scripts\test-kb-locked.ps1` (new) | `Remove-Item scripts\test-kb-locked.ps1 -Confirm:$false` |


## Card #143: the folder-trust question as Yes/No buttons in a card's chat (9 Nov 2026)

| Change | Undo |
|---|---|
| `session_trust` and `session_trust_answer` (`src-tauri\src\session_cmds.rs`, registered in `main.rs`), `routersession::trust_ask` (the Router's answer uses it too), and their tests | `git -C ~\claude-kanban-dsh revert --no-edit <the card #143 commits>` |
| The trust box in the card panel's chat (`ui\panel.js` `checkTrust`, `#p-trust`; `ui\panel.css`) | The same revert |
| The docs (HANDOVER.md, docs\GUIDE.md, this section) | The same revert |

Nothing in the board's data: an answer only adds a log line on the card.


## Card #149: a card first in line with no holder gets its turn (9 Nov 2026, branch `card-149`)

| Change | Undo |
|---|---|
| `crates\store\src\testapp.rs`: `TestAppQueue::take_now` gives the turn on a take (a first take or a take again from the line) when nobody holds it, the asker isn't blocked and nobody ahead could use it now (`next`'s rule: paused cards ahead are passed over); `take` calls it | `git -C ~\claude-kanban-dsh revert --no-edit <the card's commit>` |
| `src-tauri\src\testapp.rs`: the old DSH lock folder made by an older writer (no pid line) counts as stale, and blocks nobody, once it is 60 min old (`Dsh::stale`, `Dsh::blocks`, a new `alive` field, also in the status's `mkdir`); a folder whose pid still runs blocks whatever its age; the tool's take passes the live sessions | Same revert |
| `scripts\install.ps1`: `Move-DeadDshLock` also renames aside (never deletes) a no-pid-line DSH folder 60 min old or more | Same revert. A folder it renamed is `%USERPROFILE%\.dsh-test-app.lock.released-<time>`: `Rename-Item` it back to `.dsh-test-app.lock` if it is still wanted |

Nothing on the board or in the data folders changed; the queue's JSON is the same shape.

## Card #168: a stopped card's session starts when its test-app turn comes (9 Nov 2026, branch `card-168`)

Also carries card #149's fix (the rows above), cherry-picked onto this branch, and `Get-DshOwnerLines` in `scripts\install.ps1` now drops blank lines, as the app's `read_dsh` does.

| Change | Undo |
|---|---|
| `crates\store\src\testapp.rs`: `WAKE_GRACE_MINUTES`, `woke_at` on `Turn` and `Waiting` (left out of the JSON when empty), `Waiting::waking`, `TestAppQueue::to_wake`, `wake_failed`, `wake_ran_out` and `ran`, `Ended::NoSession`; `next` and `take_now` give nobody behind a waking card the turn | `git -C ~\claude-kanban-dsh revert --no-edit <the card #168 commit>`, then stage the live update |
| `src-tauri\src\testapp.rs`: `step` returns `wake`, `run` returns the wakes, the pump's `wake` starts the card's session (`begin_session`, why "testapp"; `woke_new_message` for a new one; `free_unstarted` frees the test app of a holder whose session couldn't start); the status's `waking` on the holder and line entries | Same revert |
| `ui\testapp.js`: "starting its session" in the popover and "starting" on the tile chip; `tools\ui-tests\testapp.test.mjs` | Same revert |
| AGENTS.md, HANDOVER.md, PLAN.md, this file | Same revert. Text only |

Data: a queue saved while a card was being woken has `wokeAt` on its entry; an older build ignores it. Nothing else on the board changes.

## Skip Planning when a card is made (9 Nov 2026, card #169, branch `card-169`)

PowerShell 5.1 commands.

| Change | Undo |
|---|---|
| `kanban_create_card`'s `skipPlanning` (`src-tauri\src\tools.rs`), `kb create --skip-planning` (`crates\kb\src\main.rs`) and the docs | `git -C ~\claude-kanban-dsh revert --no-edit <the card #169 commit>`, then stage the live update as usual. A card made this way only has #162's `skipPlanning` field |

## Card #170: Router may message a card Hugh has open
Undo: in `src-tauri/src/routertools.rs`, put back the `routersession::hands_off(app, &store, &c)` refusal
at the top of the `kanban_session_send` arm (see `git show <card #170 commit>`), or `git revert` the commit.

## Card #174: the Router may move a card out of Backlog (9 Nov 2026)

PowerShell 5.1 commands. No data changes: no migration, no new setting.

| Change | Undo |
|---|---|
| `routersession::move_refused` lets the Router move Backlog to In progress, as well as to Planning, while "Router may start cards" is on; `routertools::start_column_refused` tells a start of a Backlog card to move it first | `git -C ~\claude-kanban-dsh revert --no-edit <the card #174 commit>`. A card the Router already moved stays where it is: drag it back to Backlog if you want |
| The bundled `prompts\router.md` job 2, and the docs (docs\GUIDE.md, HANDOVER.md, PLAN.md decision 33, this section) | The same revert |
| The owner's private `prompts\router.md` job 2 | Revert the card #174 commit in the private repo |
