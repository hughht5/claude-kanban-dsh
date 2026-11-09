# Acceptance tests

Each phase's tests are written here before the phase starts (see PLAN.md, *Phases*). A phase is done when every test passes, and its report says which ones passed and how each was run.

`auto` means a `cargo test` or script run. `manual` means a person or session does it in the running app and records what was seen.

## Phase 0: skeleton (no installs)

| # | Test | How |
|---|---|---|
| 0.1 | `~\kanban-app` is a git repo with PLAN.md, ACCEPTANCE.md, UNDO.md, INSTALLED.md and README.md committed | `git log --stat` |
| 0.2 | A read-only export of today's cards exists outside the repo, with a manifest in the repo giving each card's version and the expected counts | `fixtures/export-2026-11-03.manifest.json`; the artifact's cards are unchanged (`ArtifactData list` versions match the manifest) |
| 0.3 | No card content (summaries, logs, logins) is committed to the repo | `git grep -n -E '"(summary|log|logins|nextStep)"' -- fixtures` finds nothing |

## Phase 1: toolchain, store, board on a copy

**Before it starts:** Hugh says in the build session that the toolchain may be installed, and `INSTALLED.md` lists each install as it happens.

### Toolchain

| # | Test | How |
|---|---|---|
| 1.1 | `rustc --version`, `cargo --version` and `cargo tauri --version` work in a new terminal | manual |
| 1.2 | Every install is in `INSTALLED.md` with its source, version and uninstall step | review |
| 1.3 | No system security setting changed (Defender, firewall, UAC, execution policy) | review of `INSTALLED.md` and the commands run |

### Store

| # | Test | How |
|---|---|---|
| 1.4 | Creating 50 cards in parallel threads gives 50 different numbers with no gaps, starting at `next_number` | auto |
| 1.5 | `next_number` is seeded as 21 from the export (highest number 20). An archived card's number is never given out again. | auto |
| 1.6 | A card write with a stale `version` is refused and changes nothing; the current version is returned | auto |
| 1.7 | Every card write adds one `card_revisions` row naming the fields it changed | auto |
| 1.8 | The source never removes a row: a test greps every `.rs` and `.sql` file under `src-tauri/` and `crates/` for a `DELETE FROM` statement (case-insensitive) and fails on a match. The schema's triggers also refuse a delete on every table, and updates on the append-only ones, which a second test checks. | auto |
| 1.9 | The secret filter refuses `ct=`, `customtoken`, `idtoken`, a JWT (`eyJ…`), `sk_live_`/`sk_test_` and `password` in any string field, and accepts a tokenless local dev-stack sign-in link | auto, one case each |
| 1.10 | Archive sets `archived`, `archivedAt` and `archivedBy` and adds a log line; Restore clears them; neither removes a row | auto |
| 1.11 | `board.db` opens in WAL mode, and a backup is written before each migration | auto |

### Import from a copy

| # | Test | How |
|---|---|---|
| 1.12 | `kb import <export dir>` imports all 19 cards with their ids and numbers, 17 archived, 1 In progress, 1 Ship it | auto, against the export in the manifest |
| 1.13 | `kb import --check` reports zero differences, field by field, against the export; fields the app doesn't use (`watcherNotifiedAt`, the old session fields) are kept as they were and listed | auto |
| 1.14 | A card the importer can't map fails the whole import with its id named, and nothing is written | auto, with a broken fixture made in a temp folder |
| 1.15 | Importing twice into the same database is refused unless `--replace` is given; `--replace` keeps the old rows' revisions | auto |
| 1.16 | The artifact is unchanged after an import: its card versions still match the manifest | manual (`ArtifactData list`) |

### Board

| # | Test | How |
|---|---|---|
| 1.17 | The window shows the seven columns, with the imported cards in their columns and their numbers in front of their titles | manual |
| 1.18 | Dragging a card to another column moves it, sets `movedByHand`, logs "Hugh moved it from X to Y", and survives a restart | manual |
| 1.19 | Dragging within a column reorders it, and the order survives a restart | manual |
| 1.20 | Moving into Ship it sets `ship.state: requested` and `shipRequestedAt`; moving out before anything claims it sets `cancelled`; the same for Planning and `plan` (the page's `moveExtras` rules) | auto for the rules, manual for the drag |
| 1.20a | One ship at a time per repo (card #79): a card moved into Ship it while another of its repo is shipping gets `queued` ("Waiting to ship") and its session hears nothing; the oldest waiting card goes once the repo is free; different repos and cards with no repo don't wait; a waiting card leaving Ship it is cancelled; a request no session picked up stops holding the repo after 60 s | auto (`crates/store/tests/ships.rs`), manual in the app |
| 1.20b | Cards that wait on others (card #109): `blockedBy` is cleaned and checked on every write (itself, a missing, archived or already-ready card outside a stack, a stack's lead and its own child, more than 20, a circle of any length, shown in the refusal); a card Hugh dragged to Blocked with no blocked reason moves to Planning once every wait is met (Human test in its repo, Deploy complete in another), never one in Backlog; other cards are told instead; a card it waited on going back is said once; stack children are left alone; the test app starts only dry-run cards; migration 12 adds the index after a backup | auto (`crates/store/tests/waits.rs`), manual in the app |
| 1.21 | Add card creates a card in Backlog with the next number and `createdBy: "Hugh (board)"` | manual |
| 1.22 | Search for a number finds only that card (and a card whose issue has that number); search for text matches title, repo, branch and summary | manual |
| 1.23 | Show archived lists the 17 archived cards, and Restore puts one back in its column | manual |
| 1.24 | **The stale save keeps your text.** With the detail open on a card, change that card from a second path (`kb update <n> --summary …`), then type in the dialog and Save. The save is refused, and the dialog shows the new version beside your text with Re-apply and Discard. Re-apply saves your text on the new version. | manual |
| 1.25 | Logins: Open on a "Chrome · work (Profile 1)" login starts Chrome in Profile 1 at that URL; on "Chrome · Default (personal)" in Default; Copy puts the URL on the clipboard | manual, with a test login on a test card |
| 1.26 | A login whose stack doesn't answer shows "stack down" instead of Open | manual |
| 1.27 | The board follows the Windows light and dark setting, and both are readable | manual |
| 1.28 | Done cards older than 7 days collapse behind "N done over a week ago" | manual |
| 1.29 | Nothing in Phase 1 starts `claude`, writes `~\.claude.json`, or writes to the artifact | review, and `ArtifactData list` versions unchanged |

### Phase 1 results (3 Nov 2026)

| # | Result | How it was run |
|---|---|---|
| 1.1–1.3 | Pass | rustc 1.99.1, cargo 1.99.1, tauri-cli 2.12.4; four installs in `INSTALLED.md`; no security setting touched |
| 1.4–1.15, 1.20 (rules) | Pass | `cargo test --workspace`: 14 store tests |
| 1.12–1.13 on the real export | Pass | `kb import` of the 3 Nov export: 19 cards, 17 archived, 1 In progress, 1 Ship it, next number 21, `check: no differences` |
| 1.16, 1.29 | Pass, with a note | The app has no code that reaches the artifact. Two artifact cards moved on after the export (one v10→14, by its own session; one v15→17, by Hugh on the board), so their versions no longer match the manifest. The other 17 are unchanged. |
| 1.17, 1.21, 1.22, 1.23 (archive), 1.24, 1.26, 1.27 (dark) | Pass | In the running app, driven over the WebView's DevTools port (`tools/cdp.mjs`): seven columns with the imported cards; a test card created as #21 in Backlog; moved to Planning (`plan.state` requested, `movedByHand`, both log lines) and back (cancelled); a stale save refused with the typed text kept, then re-applied; search by number; archived; a dead stack reported down; a `file:` link refused; screenshots in light and dark |
| 1.18, 1.19, 1.25, 1.27 (Windows setting), 1.28 | For Hugh | Need a real mouse drag, a Chrome profile opening on his screen, and his Windows theme. The test card `app-test-card` (#21, archived) is on the copy only. |

## Phase 2: the card panel, a terminal per card, both accounts

Hugh's ask (3 Nov 2026, through the manager): open a card and **see its chat history**, rendered from Claude's transcript, with a box to send it a message, and **a way to open its terminal**. Kangentic's card view is the guide, not a spec.

| # | Test | How |
|---|---|---|
| 2.1 | Clicking a card opens a panel over the board with the card's number, issue and title, its session state, a chat view, a message box, a Terminal toggle, Details (the old dialog) and Close | manual |
| 2.2 | For an imported card with a `claudeSessionId`, the chat view shows that transcript: Hugh's messages, Claude's replies as formatted markdown, and consecutive tool calls folded into one "Ran N tools" line that expands | manual, on card 11 |
| 2.3 | The chat view follows a running session: a new reply appears within 2 s without reopening the panel | manual |
| 2.4 | Start session on a card with none starts `claude` in the repo's folder, with `--session-id`, `--model`, `--effort` and `--permission-mode bypassPermissions`, and records a `sessions` row with its account | auto (command building), manual (start) |
| 2.5 | "Resume in the app" on a card whose session lives in the desktop app resumes it with `--resume <id>` in the transcript's own folder, only when Hugh clicks it | manual |
| 2.6 | Sending a message types it into the session (bracketed paste, then Enter as a separate write) and it appears in the chat view as Hugh's message | manual |
| 2.7 | The Terminal toggle shows the live terminal; typing in it reaches `claude`; closing and reopening the panel, or switching cards, replays what was on screen | manual |
| 2.8 | Every `claude.exe` the app starts is in a kill-on-close Job Object: killing the app's process from Task Manager ends every card session within 5 s | manual (`Stop-Process -Force`, then `Get-Process claude`) |
| 2.9 | A second start for a conversation already running is refused | auto |
| 2.10 | The environment of a started session has no `CLAUDECODE` or inherited `CLAUDE_CODE_*` variables, and has `CLAUDE_CONFIG_DIR` only for Account 2 | auto (environment building) |
| 2.11 | Stop sends Ctrl+C and `/exit`, then kills after 1.5 s; the session row gets a `suspended` event, never an update | manual, plus auto for the append-only rows |
| 2.12 | **Both accounts:** four concurrent sessions on each account; each resume runs on its row's account; a resume on the other account is refused by the app; after an hour, each session is still signed in to its own account | manual (deferred to Hugh's go-ahead, because it uses real allowance) |

### Phase 2 results so far (3 Nov 2026)

| # | Result | How it was run |
|---|---|---|
| 2.1 | Pass | Clicking a card opens the panel: state dot, card and issue number, title, Terminal, Stop session, Details, Close, and a footer with account, model, effort, Claude Code version, folder and conversation |
| 2.2 | Pass | Card 11's desktop transcript (`af6d799d…`, 321 entries) renders: Hugh's messages on the right, Claude's markdown, tool runs folded into "Ran N tools · names" |
| 2.3, 2.6 | Pass | On the test card (#21, dry run), a message sent from the box reached the session, and the reply "pong" appeared in the chat within 2 s |
| 2.4, 2.5, 2.10 | Pass | Unit tests for the flags and the environment; a real start in `kanban-app` on Claude Code 2.1.317 with Opus 5.5, effort high, bypass permissions |
| 2.7 | Pass | The Terminal toggle shows the live TUI; arrow keys and Enter typed through it worked |
| 2.8 | Pass | `Stop-Process -Force` on the app ended that session's `claude.exe` (pid 40472) within 4 s |
| 2.9 | Pass | A second start of a running conversation is refused: "that conversation is already running in the app" |
| 2.11 | Pass | Stop gave the session a `suspended` event; after the forced kill the next start marked the old one `orphaned` |
| 2.12 | Pass, lighter run (the manager's go-ahead on Hugh's behalf: 2 sessions per account, effort low) | Four dry-run cards, `acct-test-main-1/2` and `acct-test-a2-1/2`, started at once. All four answered. Each was stopped, then resumed on its row's account and answered again. Inside each session, `claude auth status` gave the main account's address for the two main sessions and Account 2's for the two Account 2 sessions. Pinning an Account 2 card to main and resuming it is refused by the app. Both accounts were still signed in afterwards. A token refresh can't be forced, so the evidence is four sessions on two accounts running side by side for about 15 minutes with no sign-out. The sessions are stopped and the cards archived. |

**Found while testing:**
- A new folder asks Claude's trust question in the terminal, and in Claude Code 2.1.317 the default answer is "No, exit". Text typed into a session before it is ready answers that question. Until Phase 3's `SessionStart` hook gates typing, the message box should only be used once the terminal shows Claude's prompt.
- The app now picks the newest `claude.exe` by asking each one its version. `~\bin\claude.exe` is 2.1.246, too old for Opus 5.5; `~\.local\bin\claude.exe` is 2.1.317.

**Found in the account test:** Account 2's `projects` folder is a junction to the main account's, so both accounts share every transcript, and Claude resumes any conversation on either account without complaint. Shown by `claude -p --resume <an Account 2 conversation>` on the main account, which added one main-account turn, "ok", to that test conversation. So the account comes only from the app's session row, and the app refuses a resume when the card is pinned to the other account.

## Phase 3: hooks, tools and the dispatcher

| # | Test | How |
|---|---|---|
| 3.1 | A session started by the app reports `SessionStart`, `UserPromptSubmit`, `Stop` and `SessionEnd` to the app through `http` hooks, and the card shows its state (starting, ready, working, needs you, failed, ended) | manual, on a dry-run card |
| 3.2 | Nothing is typed into a session before its `SessionStart` arrives, or while it is working; a queued message is typed once it is ready and has been quiet for 1.5 s | auto (queue rules), manual |
| 3.3 | The session has the `kanban_*` MCP tools: `kanban_get_card` returns its own card, `kanban_update_card` changes it (version-pinned when `if_version` is given), and `kanban_add_log` adds a line | manual (ask a dry-run session to use them) |
| 3.4 | `kb card`, `kb list`, `kb update`, `kb log`, `kb move` and `kb create` work through the running app, and say so plainly when the app isn't running; `kb` never opens the database for these | auto/manual |
| 3.5 | **Held move:** a session's `kanban_move_card` on its own card answers "Queued…" and the card moves only after that turn's `Stop`; a move of another card happens at once; a move into Ship it is refused | manual |
| 3.6 | Hugh's move of a card with a running session types "Hugh moved this card from X to Y." plus the column's prompt into it, after its turn ends | manual |
| 3.7 | Hugh's edit of `summary`, `nextStep`, `blockedReason` or an override types "Hugh edited …" with the new values; several edits before the session is free go as one message | manual, auto (merging) |
| 3.8 | Moving a card with no session into Planning, In progress or Ship it starts one with that column's prompt; a card whose conversation is in the desktop app is not started automatically, and the panel says so | manual |
| 3.9 | Moving a card into Done stops its session | manual |
| 3.10 | Model, effort and permission come from the column's settings, and a card's override wins; changing effort on a running session types `/effort <level>`; changing the model restarts it with `--resume` on the new model | auto (choice), manual |
| 3.11 | A new session's account is the card's pin, else the account with the lower 7-day usage reported by the statusLine, else main; the log line names both percentages when known | auto |
| 3.12 | The command bar (Ctrl+K) takes `34`, `34 ship it`, `34 plan it` and `34 to Blocked` | manual |
| 3.13 | `dryRun` cards' prompts start "Dry run: say what you would do and change nothing." | auto |

### Phase 3 results (3 Nov 2026)

| # | Result | How it was run |
|---|---|---|
| 3.1 | Pass, with a change | The card's state went starting → ready → working → ready → ready (moved) on dry-run card #27. `UserPromptSubmit` and `Stop` arrive as `http` hooks. **`SessionStart` never arrives as an `http` hook** (also seen outside the app, with a throwaway listener), so the statusLine's first report marks a session ready instead. It only draws once Claude's prompt is up, after any trust question. |
| 3.2 | Pass | Messages wait in `pending` and are typed only when the session is ready, idle and quiet; a message queued before the session started was typed after it was ready |
| 3.3 | Pass | Card #27's session called `kanban_get_card` and `kanban_add_log` ("tool test from the session" is on the card) |
| 3.4 | Pass | `kb list`, `kb update 21 --nextStep …`, `kb log 21 …` through the app; `kb move 21 "Ship it"` refused; with the app closed, `kb card 21` says "the Claude Kanban DSH app isn't running" and exits 1 |
| 3.5 | Pass | The session's `kanban_move_card` to Blocked was held, and the card moved only after its turn ended: "Moved from Planning to Blocked by card #27's session (held until its turn ended): held move test" |
| 3.6 | Pass | Hugh's move from Blocked to In progress was typed in as "Dry run: … Hugh moved this card from Blocked to In progress. Build it now: …" once the session was free |
| 3.7 | Pass | Hugh's edit of `summary` was typed in as "Hugh edited summary on this card: - summary: …"; merging of queued messages is a store test |
| 3.8 | Pass | Moving #27 into Planning started its session with the Planning prompt; a card whose conversation is in the desktop app was not started, and its log says why |
| 3.9 | Pass | Moving #27 to Done stopped its session |
| 3.10 | Part | Model, effort and permission come from the column's settings, overridden by the card's fields. Changing them on a running session says "applies when the session next starts"; typing `/effort` live and restarting on a new model are not built yet. |
| 3.11 | Pass | Unit test: pin, then the lower 7-day usage (both percentages in the log), then the card's account, then main. The statusLine reports 7-day usage to the app. |
| 3.12 | Pass | Driven through the page: "27 to Backlog" moved it, "27" opened its panel |
| 3.13 | Pass | Unit test, and seen in #27's prompts |

**Also found:** the app's preferred port 47821 (and its neighbours) can't be bound on this machine, though nothing lists them as reserved, so the API falls back to a port the OS picks. Everything finds it through `api.json`.

## Phase 4: first usable release (on the copy)

| # | Test | How |
|---|---|---|
| 4.1 | Quitting the app (tray > Quit) suspends every session, recording whether each was mid-turn | auto (state recording), manual |
| 4.2 | On the next start, a session that was mid-turn is resumed straight away and, once ready, gets "The kanban app restarted while you were working. Carry on from where you were."; an idle one is resumed only when its card is opened or a move needs it | manual on a dry-run card |
| 4.3 | After a crash (`Stop-Process -Force`), the next start marks the sessions orphaned and treats a mid-turn one as in 4.2 | manual |
| 4.4 | Closing the window keeps the app in the tray with sessions running; the tray menu has Show and Quit; the tray tooltip says how many sessions need Hugh | manual |
| 4.5 | A Windows toast appears for: a session needing permission, a turn finished in Planning or In progress, `kanban_notify`, a ship result, and a session crash; none for the card open in a focused window; clicking one opens that card's panel | manual |
| 4.6 | `kb ship <n> --asked-by-hugh` from a session outside the app shows "Ship #n?" in the app (banner and toast); only Hugh's click ships it; without the flag it is refused | manual, auto (refusal) |
| 4.7 | A nightly backup of `board.db` (and one at each start, at most once a day) lands in `backups\`, keeping the newest 30 | auto (pruning), manual |
| 4.8 | `scripts\install.ps1` builds the NSIS installer, quits the running app cleanly, installs per-user without admin, puts `kb.exe` on the user PATH, and starts the app; running it twice is safe | manual |

### Phase 4 results (3 Nov 2026)

Run on dry-run card #29 (main account, low effort). Toasts and the tray are native UI that the test could not see: those rows say what was inferred, from a temporary log line (since removed) in the toast and tooltip code, from the database's session events and from process state.

| # | Result | How it was run |
|---|---|---|
| 4.1 | Pass | Set `Local\ClaudeKanbanDshQuit` (the same `quit_app` as tray > Quit) while #29 was mid-turn: the app exited in 1.8 s and the session's last event is `suspended`, "mid-turn (working)". An idle session quit the same way got "idle". The tray menu's Quit item itself wasn't clicked. |
| 4.2 | Pass, after a fix | On the next start the mid-turn session was resumed at once (same conversation) and got "The kanban app restarted while you were working. Carry on from where you were." (read with `transcript_read`), then carried on. An idle one was not resumed at start. Resuming one when its card is opened wasn't tried. **Bug found and fixed:** the first restart hung the whole app ("Not responding"): see below. |
| 4.3 | Pass | `Stop-Process -Force` mid-turn: the next start wrote `orphaned`, "mid-turn (working): the app stopped while it ran", resumed the session and it got the same message. The Job Object ended the old `claude.exe`. |
| 4.4 | Part (inferred) | Closing the window (`WM_CLOSE`) hid it; the process kept running and the session stayed live. The tooltip code ran without error and set "Claude Kanban DSH" (no session needed Hugh). The tray icon, its Show and Quit items and a "1 need you" tooltip were not seen: Hugh to check by eye. |
| 4.5 | Part (inferred), after a fix | The toast code ran and `Toast::show` returned Ok for: `kb notify`, a ship result (`kb ship-result 29 failed`), a turn finished in Planning and a session crash (its `claude.exe` killed). No toast on a normal stop. Not triggered: a permission prompt (none came up) and the focused-panel exception. Whether the toasts were visible, and that clicking one opens the card, are for Hugh to check. **Bug found and fixed:** a crash was never noticed (see below). |
| 4.6 | Pass (toast inferred) | From a shell outside the app, `kb ship 29` was refused (exit 1); `kb ship 29 --asked-by-hugh` put the banner "Ship #29 … ? (asked from a session outside the app)" with Ship and Dismiss on the page, and the toast code ran. Dismissed; the card didn't move into Ship it. |
| 4.7 | Part | No start backup today: the 18:38 `pre-migration` backup counts as today's, so `backup_if_none_today` rightly skipped. The 24-hour backup loop wasn't waited for; pruning to 30 is a store test. It runs every 24 h from the app's start, not at night. |
| 4.8 | Pass, one run | Run by the build session after the fix: it built `Claude Kanban DSH_0.1.0_x64-setup.exe` (3.76 MB), installed per-user into `%LOCALAPPDATA%ProgramsClaude Kanban DSH` with `kb.exe` beside the app, added that folder to the user PATH, and started the installed app; `kb list` works from a fresh environment. The clean quit (the `LocalClaudeKanbanDshQuit` signal) didn't stop the running debug build, so the script fell back to stopping it. A second run, to show it is safe to repeat, wasn't done. |

**Bugs found and fixed (commit 181a68e; the installer's in a3b2aa6, above):**
- **The app hung after a restart resume.** Tauri runs a Rust listener on the thread that emits. The restart resume held the board lock and emitted `session-changed`; the tray's tooltip listener then waited on the main thread, which was waiting for the board lock. `kanban_notify`, `kb ship` and ship results had the same shape: they emit while holding the board lock, and the toast listener takes it again. The tray and toast listeners now run in order on one worker thread.
- **A crashed session was never noticed**, so no crash toast and the card still showed it as idle: ConPTY keeps the output pipe open after the process exits, so the reader never ended. A thread now waits on the process as well.

**Also seen:** `kb ship-result 29 failed` on a card in Backlog moved it to In progress ("Recorded the failure… back in In progress"). A ship result for a card not in Ship it should probably be refused. Not changed.

## Card #105: "Done when" and acceptance checks (6 Nov 2026)

| # | Test | How |
|---|---|---|
| 105.1 | `doneWhen` is one clean line of at most 200 characters; "" or null removes it | `crates/store/tests/done_when.rs` `done_when_is_one_clean_line` |
| 105.2 | `doneWhenChecks` takes a string or a list, refuses too many, too long, a bad state or an unknown key, and leaves the card unchanged | `done_when_checks_are_cleaned_and_checked` |
| 105.3 | A filtered word, a sign-in link or a Bearer header is refused, and the error names the check | `a_filtered_word_in_a_check_is_refused_by_name` |
| 105.4 | The store records who changed each check's state and when; reordering keeps it | `a_check_records_who_changed_its_state_and_when` |
| 105.5 | Other writes keep the fields (an older build's behaviour) | `an_older_build_keeps_the_done_when_fields`. It puts the fields in with raw SQL (`json_set`) and runs **this** build's other writes over them; it does not run an older binary. The older-build claim rests on reading the code: `apply`'s generic arm only touches the keys a patch names, in every build since the first |
| 105.6 | The tester's CHECK lines are read, masked and marked on the card its CARD line names (a stack child's on the child); an unclear verdict marks nothing | `src-tauri/src/done_when.rs` tests (incl. `the_agents_ask_about_the_checks`); `agents.rs` `check_results_go_to_the_card_the_report_names`, `a_pass_with_a_failed_check_is_recorded_as_a_fail`, `a_check_list_the_store_refuses_keeps_the_verdict` |
| 105.7 | Hugh's edits are told as numbered lines, and not in Human test, Ship it or Deploy complete | `dispatch.rs` `hughs_edit_of_the_checks_is_told_readably`, `a_check_edit_in_human_test_is_not_told` |
| 105.8 | Sessions and `kb` can set the fields | `tools.rs` `done_when_fields_are_updatable`; `kb` `done_when_fields_pass_through_update` |
| 105.9 | The status rule gives each of the five states | In the test app, `tools/cdp.mjs eval` calling `doneWhenStatus` on a fixture card per state (no JavaScript test runner in this repo) |

## Card #162: the reviewer and the tester run together (9 Nov 2026)

| # | Test | How |
|---|---|---|
| 162.1 | A record keeps `startedAt` (the run's start, else the verdict's time) and `failedAt` (its latest FAIL, carried on by later records) | `crates/store/tests/pair.rs` `a_record_keeps_when_its_run_started_and_when_it_last_failed` |
| 162.2 | A test pass started before the reviewer's latest FAIL doesn't count; one started after does; the same for a review pass and the tester's FAIL | `a_test_pass_that_started_before_the_reviewers_fail_doesnt_count`, `a_review_pass_that_started_before_the_testers_fail_doesnt_count` |
| 162.3 | A record with no `startedAt` counts from its `at`; no times, or no FAIL, hold nothing | `a_record_with_no_start_falls_back_to_its_verdict_time` |
| 162.4 | The move into In progress clears `failedAt` with the round; a stop in Blocked keeps it | `the_move_into_in_progress_resets_failed_at_with_the_round` |
| 162.5 | A card session's move to Human test or Deploy complete is refused for a stale pass, saying which to run again and when the other failed; only while both subagents are on, not while either is capped | `src-tauri/src/agents.rs` `a_pass_that_started_before_the_others_fail_is_refused` |
| 162.6 | A stack lead's own stale pass holds its stack, only while both its rules are on | `pair.rs` `a_leads_own_stale_pass_holds_its_stack_only_while_both_are_on` |
| 162.7 | Every Agent call of a PASS/FAIL agent notes its start, with a place only for a listed repo | `agents.rs` `a_verdict_counts_only_for_work_unchanged_while_its_agent_ran` |
| 162.8 | The prompts install first, then start both in one message; no agent text says the review comes first | `src-tauri/tests/prompt_rules.rs` `the_reviewer_and_the_tester_run_together_after_the_install`, `no_agent_text_says_the_review_comes_first` |
| 162.9 | In a test app: a dry-run card's session runs both together; a review FAIL fixed while the test passes is refused at the move to Human test, and a fresh tester lets it through | By hand, in the card's test app |

## Card #160: one merge lane per repo (9 Nov 2026)

| # | Test | How |
|---|---|---|
| 160.1 | `lane_sessions` is append-only, migration 16 backs up first, and an older build opens the board | `crates/store/tests/lanes.rs` `migration_16_adds_an_append_only_lane_sessions_table`, `an_older_board_is_backed_up_before_migration_16`, `a_board_at_this_version_opens_on_an_older_build` |
| 160.2 | The merge-lane list is empty by default and checked like the own-worktree list; the lanes' model and effort default to Opus high and are checked | `the_list_is_empty_by_default_and_checked_like_the_own_worktree_list`, `the_lanes_model_and_effort_default_to_opus_high_and_are_checked` |
| 160.3 | A lane's latest session is per repo and channel; a board copy cancels the source's waiting jobs; a session can't write `merge` | `a_lane_is_never_a_card_and_its_latest_session_is_per_repo_and_channel`, `a_board_copy_cancels_the_sources_lane_jobs`, `a_session_may_not_write_the_merge_record` |
| 160.4 | The jobs query counts and lists what each lane owes; the history is the repo's last finished merges, newest first | `the_jobs_query_counts_and_lists_what_each_lane_owes`, `the_history_is_the_repos_last_finished_merges_newest_first` |
| 160.5 | `decide`: start, brief when free, a note first, time out at 60 min, resume once, cancel when the card leaves Ship it, give up when unlisted, stop after 15 idle min | `src-tauri/src/lane.rs` tests |
| 160.6 | `kanban_merge` is refused, saying why, for each case; only a card session is offered it | `lane.rs` `kanban_merge_is_refused_saying_why`; `tools.rs` `kanban_merge_is_listed_for_card_sessions`; `api.rs` tool list test |
| 160.7 | The brief names the job, the history and the dry run; the verdict message gives the PASS steps (status, rev-parse, reset, range-diff) or needs-hugh | `the_brief_names_the_job_the_history_and_the_dry_run`, `the_verdict_message_tells_the_session_the_pass_steps_or_needs_hugh` |
| 160.8 | The guard refuses the lane's pushes, `kb`, installs, branch moves and edits outside its worktree, and lets its own work run | `guard.rs` `the_lane_guard_refuses_pushes_kb_installs_branch_moves_and_edits_outside` |
| 160.9 | The lane's worktree is per channel, inside the checkout, detached, found again, and makes no branch | `worktree.rs` `the_lanes_worktree_is_per_channel_inside_the_checkout`, `the_lanes_worktree_is_detached_found_again_and_makes_no_branch` |
| 160.10 | A lane is offered its four tools only and logs only on its job's card | `lanetools.rs` tests |
| 160.11 | A lane's command line has its rules as the system prompt, no subagents, no AskUserQuestion | `sessions.rs` `a_lanes_command_line_has_its_rules_no_agents_and_no_questions` |
| 160.12 | Ship it's message tells a listed repo's card to use `kanban_merge`; not another repo, not with the merger off | `dispatch.rs` `a_ship_of_a_listed_repo_is_told_to_use_its_merge_lane`; `lane.rs` `the_lane_note_is_for_a_listed_repo_in_ship_it_with_the_merger_on` |
| 160.13 | Both ship prompts have the lane step; `merge-lane.md` never pushes and works detached | `merger.rs` `both_ship_prompts_have_the_merge_lane_step`, `the_merge_lane_rules_never_push_and_work_detached`; `lane.rs` `the_rules_say_what_the_lane_must_and_must_not_do` |
| 160.14 | The Merger chips Waiting for the lane, Merging and Merge cancelled, titled with the lane's repo; Settings › Sessions lists the lanes with state, model, effort and Stop; the own-worktree box works as before | `tools/ui-tests/lane.test.mjs` |
| 160.15 | Two cards of a scratch repo ship through one lane: the second resumes the same conversation and its brief lists the first; a FAIL and a repo not listed | In the card's test app with a scratch repo and a local bare origin (the card's deliverable 9) |

Card #162 (the merge lane everywhere) replaces 160.2's list check, 160.12 and 160.13 with
162.11 to 162.14, and 160.5's "give up when unlisted" with 162.15.

## Card #162: the merge lane everywhere (9 Nov 2026)

| # | Test | How |
|---|---|---|
| 162.10 | There is no merge-lane repo list: the page can't read or set it, a saved one is kept as it was and never read | `crates/store/tests/lanes.rs` `the_old_repo_list_is_no_setting_and_a_saved_one_is_kept_as_it_was`; `tools/ui-tests/lane.test.mjs` (the box has no list and never reads it) |
| 162.11 | The lane's checkout is found in order (the card's worktree's main checkout, the own-worktree list, the `repos.json` folder), each a git checkout with a GitHub origin; none is refused naming each place and why, and says needs-hugh | `lane.rs` `the_lanes_checkout_is_found_in_order_or_refused_saying_where_it_looked`, `a_remote_url_names_its_github_repo_or_none`; `private.rs` `repos_come_from_repos_json` |
| 162.12 | Lanes are keyed by the origin repo: an old repo name shares the lane and its worktree; another repo has its own; a test app's lane worktree is its own | `lane.rs` `a_lane_is_keyed_by_its_origin_so_an_alias_shares_it`, `the_lanes_listed_are_each_lane_once_with_its_place`; `crates/store/tests/lanes.rs` `the_lanes_seen_are_each_repos_newest_session_on_this_channel`, `the_history_is_the_repos_last_finished_merges_newest_first` |
| 162.13 | Every Ship it card with a branch, on any repo, is told to use `kanban_merge` and never the card-merger; a card with no branch hears nothing of a merge | `lane.rs` `the_lane_note_is_for_every_ship_with_a_branch`; `dispatch.rs` `every_ship_with_a_branch_is_told_to_use_its_merge_lane` |
| 162.14 | Both ship prompts merge only through the lane (no card-merger, no condition, no rebase of the session's own, needs-hugh with no checkout, a no-branch path); `kanban-app.md` names the lane | `merger.rs` `both_ship_prompts_merge_only_through_the_lane`; `prompt_rules.rs` `cards_ship_one_at_a_time_rebased_or_as_one_stack` |
| 162.15 | A job with no checkout (an older build's) gives up; a lane free in another checkout's worktree stops before the next brief | `lane.rs` `a_card_that_leaves_ship_it_cancels_its_job_and_a_job_with_no_checkout_gives_up`, `a_lane_in_another_checkout_stops_before_the_next_brief` |
| 162.16 | The card-merger is no card subagent: no row, no default setting; an old card's `merger` is ignored and an older page's dropped; it refuses a ship and its report on a Ship it card or a lane job isn't recorded | `crates/store/tests/subagents.rs` `the_card_mergers_switch_and_choice_are_gone_and_ignored`; `subagents.rs` tests; `merger.rs` `the_merger_does_no_ship`; `dispatch.rs` `a_session_is_told_which_subagents_are_off_in_its_column` |
| 162.17 | A card on no list ships through its lane: `kanban_merge` finds the checkout from the card's worktree and queues the job in the lane its origin names; Settings shows that lane | In the card's test app, a dry-run Ship it card of this repo (a manual check) |

## Card #162: a card's flow, skip Planning, skip Human test, three-way subagents (9 Nov 2026)

| # | Test | How |
|---|---|---|
| 162.18 | A card's subagents hold only Hugh's overrides; null puts one back to the default; a later default reaches every card that follows it | `crates/store/tests/subagents.rs` `a_new_card_follows_the_boards_defaults_as_they_change`, `hughs_choice_on_add_card_keeps_only_his_overrides`, `an_override_goes_back_to_the_default_with_null`; `subagents.rs` `an_update_changes_only_the_keys_it_names` |
| 162.19 | Migration 17 backs up first, takes stamped values equal to the default off non-archived cards with a revision each, keeps the ones that differ, is idempotent, and an older build opens the board | `crates/store/tests/flow.rs` `migration_17_unstamps_the_defaults_and_keeps_the_overrides`, `a_board_at_version_17_opens_on_an_older_build` |
| 162.20 | The two defaults are page settings, off by default, logged | `flow.rs` `the_two_defaults_are_page_settings_off_by_default` |
| 162.21 | Skip Planning and skip Human test are three-way; a stack's child never skips; the flow object says each value and whose it is | `flow.rs` `a_skip_is_three_way_on_the_card`, `the_flow_object_says_each_value_and_whose_it_is` |
| 162.22 | A move into Planning from Backlog or Blocked (Hugh's drag, Take it to, the waits) goes to In progress with no plan request, logged; from a later column, a child, or a card set Off it plans | `flow.rs` `a_move_into_planning_from_backlog_or_blocked_goes_to_in_progress`, `planning_still_plans_where_the_rule_doesnt_reach` |
| 162.23 | The plan critique doesn't hold such a card from Backlog or Blocked into In progress | `critic.rs` `a_card_that_skips_planning_needs_no_critique_to_start` |
| 162.24 | Skip Human test goes on after a session's move; not after Hugh's drag, with Take it to: Human test, or on a child; the default reaches a card | `dispatch.rs` `skip_human_test_goes_on_only_after_a_sessions_move`; `flow.rs` `arrival_names_the_mover_and_a_cleared_auto_to` |
| 162.25 | The sessions are told: Planning skipped, Human test skipped (In progress, Ship it), Hugh's edits logged and told | `dispatch.rs` `the_flow_notes_tell_the_session`, `hughs_flow_change_is_logged_and_told` |
| 162.26 | A card that skips Human test is left out of small-change auto-ship | `small.rs` `skip_conditions` |
| 162.27 | A card session and the Router can't set the flow; Hugh's own sessions and kb can | `tools.rs` `only_hughs_own_callers_set_the_cards_flow`, `stack_tools_are_listed`; `kb` `update_takes_the_cards_flow` |
| 162.28 | The card face chips, the Card flow box (one key per change), Add card (only what isn't Default) and the Settings defaults | `tools/ui-tests/flow.test.mjs` |
| 162.29 | In the test app: a card set to skip Planning dragged to Planning lands in In progress and its session hears why; one set to skip Human test moved there by its session goes on to Ship it | By hand in the card's test app (Hugh) |

## macOS (card #117, 6 Nov 2026): untested on real hardware, unsigned

Nobody has a Mac yet, so the proof is CI on GitHub's Mac runners. The app is unsigned (no Apple
Developer ID, not notarised): a user allows it once in System Settings > Privacy & Security >
Open Anyway (README).

### Checked by CI (`.github/workflows/ci.yml`, Mac jobs; run IDs on card #117)

| # | Check | Where |
|---|---|---|
| M.1 | `cargo test --workspace` on Apple silicon, including the macOS-only tests (exit waiter, reaper, update helper swap and failed-move result, flock) | `macos` job |
| M.2 | `install.sh` builds and installs the smoke app; `codesign --verify --deep --strict` passes; `lipo -archs` shows arm64 (and x86_64 on a universal run) for the app and kb | `macos` job |
| M.3 | The data folder is 0700 and `api.json` 0600 | smoke |
| M.4 | Launch 1 (binary run directly): API up, a second launch exits, focus stays on TextEdit, a dry-run card created, moved and read back, `kb notify`, four sessions ready on PTYs with no "Broken pipe", LANG set, `ulimit -n` at least 10240 | smoke |
| M.5 | An account-2 card refuses to start with the "isn't supported on a Mac" text | smoke |
| M.6 | SIGTERM quits with the sessions suspended and nothing left running; kill -9 of the app hangs claude up and the reaper ends a `nohup` child handed to launchd | smoke |
| M.7 | Launch 2 (`open -g`, launchd's bare PATH): claude found through the login-shell PATH; the Apple Event quit suspends the sessions (or SKIP if the runner refuses Automation) | smoke |
| M.8 | The test-app lock survives a killed install.sh: a second run takes it | `macos` job |
| M.9 | Universal run: the `.dmg` built; on the Intel runner its kb runs and launch 1 passes | `macos-intel` job |
| M.10 | After the `{prompts}` commit: the first message a Planning session gets has no `C:\` and no `\` before `planning.md` | smoke `--prompts-check` |

### Real Mac only (to do; nobody has run these)

- A start from Finder: PATH (claude, git, node found), LANG, the log line in `~/.claude-kanban-dsh/macos.log`.
- Focus: a background start (install.sh, Restart now) and a start at log-in don't take focus from a full-screen app.
- App Nap: with the window hidden for an hour, hooks, the dispatcher and the backup keep their timing.
- Folder permission prompts (Documents, Desktop, Downloads) for a session there, and whether a granted permission survives an update (self-signed certificate) and resets once after an ad-hoc build.
- Restart now with real sessions running: does macOS ask for App Management; a failed move puts the old app back and the banner says why; the sessions resume.
- Notifications: they appear, a click opens the card, a card's newer one replaces its older one.
- The menus: Cmd+Q with sessions running asks (and the native alert if the page is slow), Cmd+C/V in text boxes, the menu-bar icon's menu and count, the Dock icon brings the window back.
- The Apple Event quit, if CI printed SKIP for it.
- The universal app on Intel hardware.
- A downloaded `.dmg`: Gatekeeper refuses it, and Open Anyway in Privacy & Security lets it run.
- claude found from the Claude desktop app's own copy.
- A self-detaching process (`setsid`) started by a session outlives the app: the known gap.
