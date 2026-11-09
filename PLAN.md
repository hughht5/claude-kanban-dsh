# Claude Kanban DSH: build plan for a Rust desktop app

Written 3 Nov 2026 for Hugh. Revised the same day after the manager session's review, which also settled the decisions.

**Change, 3 Nov 2026 (Hugh): no cut-over.** The app runs entirely separately, as its own board with its own cards in its own SQLite store. The Work Board artifact and the `kanban` skill carry on as they are: nothing is frozen, exported again or switched over, and Phase 5 is cancelled. The cards imported on 3 Nov stay in the app as its starting set. Sessions the app starts use its `kanban_*` MCP tools (wired per session) or `kb`. Where this plan talks about cut-over, closing the artifact's writes, skill v3 or the user-scope `kanban` MCP entry, read it as not happening.

## Contents

- [What it is](#what-it-is)
- [Decisions](#decisions)
- [Stack](#stack)
- [Architecture](#architecture)
- [Terminal](#terminal)
- [Process lifecycle](#process-lifecycle)
- [The two accounts](#the-two-accounts)
- [Storage and migration](#storage-and-migration)
- [Columns, prompts, model and effort](#columns-prompts-model-and-effort)
- [Moves, edits and the held move](#moves-edits-and-the-held-move)
- [Shipping](#shipping)
- [Logins](#logins)
- [Sessions outside the app](#sessions-outside-the-app)
- [Notifications, packaging and install](#notifications-packaging-and-install)
- [What to port from Kangentic, and what to drop](#what-to-port-from-kangentic-and-what-to-drop)
- [Phases](#phases)
- [Deferred](#deferred)
- [Risks](#risks)
- [What can be undone](#what-can-be-undone)

## What it is

A desktop app that replaces the Work Board artifact. It shows the same seven columns: Backlog, Planning, In progress, Blocked, Human test, Ship it and Done. It keeps card numbers, manual order, archive and restore, Add card, the detail dialog with logins, and the ship and plan states. **Each card owns a live Claude Code session in an embedded terminal.** You click a card, its session appears beside the board, you type to it, and you switch cards in the same window, as in Kangentic.

The app is the dispatcher. A drag is handled by the process that saw it, within a second, so these problems go away:

| Problem today | Why it can't happen in the app |
|---|---|
| Drags reach a session only through delayed comment wake-ups, or not at all after a restart | The drag handler writes the card and types into the card's terminal directly. There is no watching session, comment or scheduled task. |
| Card sessions started by scheduled tasks are hidden under Routines | The app starts the sessions itself, and each one is shown on its card. |
| Sessions can't be started while you use Remote Control | Starting a session is a process spawn by the app. No Claude desktop rule is involved. |
| Saves aren't version-pinned, so concurrent edits are lost | One Rust process owns the database. Every write is a transaction that checks the card's `version`. |
| A card's session isn't told when you move its card | Every move and every edit you make is typed into that card's session once its turn ends (see *Moves, edits and the held move*). |

## Decisions

Settled by the manager's review on 3 Nov 2026; 11 to 13 by Hugh on 5 Nov 2026 (card #76); 15 to 20 by the router on 6 Nov 2026, for card #113 (Hugh delegated them: "yes do that. perfect"), its R1 to R6.

| # | Decision | Settled |
|---|---|---|
| 1 | Stack | Tauri 2, xterm.js and `portable-pty` |
| 2 | Where cards live | Local SQLite, imported once. **After cut-over, the artifact refuses writes** (not just a banner), so a session still on the old skill gets an error instead of losing its write. |
| 3 | Phone | No phone support in v1 |
| 4 | Desktop sidebar sessions | Resumed in the app only when you ask. Running sidebar sessions finish where they are. |
| 5 | Model, effort, account | The model and effort table in *Columns*. The account of a new session is the one with the **lower 7-day usage** (your rule), with a per-card override. |
| 6 | A ship from outside the app | Keeps a one-click confirm in the app |
| 7 | Closing the window | The app keeps running in the tray |
| 8 | Updates | No updater in v1: `tauri build` plus an install script |
| 9 | Repo | A GitHub repo of its own, private to start with |
| 10 | Permission mode | Card sessions run in `bypassPermissions` |
| 11 | The app moving a card into Ship it | Allowed only on Hugh's standing order: a card he set to go on by itself to Deploy complete ("Take it to", `autoTo`) is moved from Human test into Ship it for him, as his approval to ship it with no human test. Sessions still never move a card into Ship it. |
| 12 | Hugh's own moves and auto-progress | Any move Hugh makes himself, except into Planning or In progress (starting it, or approving its plan), turns the card's auto-progress off: he has taken it over. |
| 14 | Dropping a card on a Ship it card (card #89) | It joins that card's stack and ships in the same merge while that ship hasn't started (waiting its turn, or not yet handed to its session). Once it has started, the card ships straight after it instead, on its own. A card whose own ship has started never joins. A card not built can't join a stack in Ship it. A card from Ship it keeps Hugh's approval (Passed), and its own ship is cancelled. A built stack can join another. A stack is one repo. Only Hugh's drop does these. Settled in Planning, as the card asked. |
| 15 | What changed on each card (card #108) | Read-only git in the app, never under the board lock, one job at a time with the panel first. A card's own branch is measured from the newest commit it shares with main/master or origin/main/master; a card on main, or a merged branch, from the commits whose message names the card. Uncommitted work only in the panel and only in the card's own linked worktree. The figures are saved on the card (`changesSaved`) each time it enters Human test, so they outlive a deleted branch (router, for Hugh); otherwise "No figures kept". Handed-over files (`changesOutputs`) open from the board only as md, txt, png, jpg or pdf (router, for Hugh); other types get Show in folder. No GitHub PR figures. |
| 13 | The subagent test's verdict | Read by the app from the `SubagentStop` hook (the tester's own last message and transcript), never from the session's word. Sessions can't set the card's `test` field, and a pass that ran no tools doesn't count. |
| 15 | Who approves the sessions of a stack built in parallel (card #113, R1) | Hugh's move of a stack's lead into Planning or In progress approves every start and resume of sessions in that stack, while the lead stays in Planning, In progress or Human test. The app then starts each child that waits on nothing (from Backlog it moves it to Planning first), wakes the lead to merge each child that reaches Human test, and resumes a child it sends back. Children still go through the critic: the app never moves a child to In progress; a child moves itself there once its critic has passed and the lead is In progress, and without the critic subagent it waits for the router or Hugh. |
| 16 | The merger and stacks (card #113, R2) | Card #100's merger is a hard dependency: it gets a "stack" job that runs outside Ship it, records `merge` on the child, and calls the app's `after_child_merge`. Until it lands, a stack built in parallel can't merge its children. |
| 17 | How a child is merged (card #113, R3) | In the lead's worktree: detach at the child's head, rebase onto the stack's branch, then fast-forward the stack's branch to the result. The child's branch is never rewritten or checked out there. Never main, never a push. Tested on a scratch repo with two worktrees. |
| 18 | Order inside a stack (card #113, R4) | A child that waits on a sibling ("Blocked by", card #109's `blockedBy`) starts only once that sibling is merged, and always branches from the stack's branch, never from the sibling's. |
| 19 | The merger's effort (card #113, R5) | High, in the app's subagent table (card #96's `AGENTS`, card #100's entry). The lead's own session runs on low unless its card has an `effortOverride`. |
| 20 | Dev stacks in a stack (card #113, R6) | Only the lead brings up a product repo's dev stack, once, for the integrated build. Children build and unit-test without one, and carry no PR and no logins. |
| 21 | Before-and-after evidence (card #101, 6 Nov 2026; the router, under Hugh's delegation) | Screenshots only (PNG), no GIF. "Before" is the new build just before the tester's action, not the old build; a true old-versus-new comparison can be a later card. No migration: the evidence is an app-only card field, `evidence`, written in the same update as the tester's verdict, so card_revisions keep it append-only. The app copies the shots from the tester's `%TEMP%` into its data folder, checking them before the board lock. Shots only for `hughht5/kanban-app` cards, only from the test app (9334), which now always has its debug port from its own window config (`tauri.test.conf.json`; the router asked for it through `install.ps1`, and the config gives the same result however the test app is started). Each shot carries the test app's build commit, stamped into the PNG by `cdp.mjs`. |
| 22 | Small changes that go on by themselves (card #104, 6 Nov 2026; router's decisions, Hugh delegated) | Off until Hugh lists a repo (`sessions.small_goes_on_repos`, each with the checkout and branch its ship pushes). Docs (`.md`, `.rst`) and CSS only, no UI text in version 1; CSS may not set display, visibility, pointer-events, position, z-index, opacity or content, and needs #101's screenshots. At most 30 lines, 3,000 bytes and 5 files; no sign-in, data, migration, prompt or agent paths; SQL DROP only as SQL. Both subagents passed on the current HEAD and status hash, the review with SCOPE: full. A 10-minute wait in Human test with Stop and a per-card opt-out; at most once per card; every setting change logged and toasted. The hard rule now reads: "Only Hugh moves a card into Ship it, or the app for him when he set the card to go on by itself (Take it to), or turned on small-change auto-ship for that repo." The router accepted this wording for Hugh on 6 Nov 2026 (Hugh delegated it: "104 - make your own judgement"). Off by default with an empty repo list, so Hugh opts in per repo. |
| 23 | Who can make a card start by its waits (card #109; the router for Hugh, 6 Nov 2026) | Only Hugh's own drag of the card into Blocked. The app moves a card only from Blocked, to Planning, when the last column change was his and it has no blocked reason. A card in Backlog is never moved: a wait there only shows the order, and Hugh is notified when it is met. So a wait set by any session can't start a card by itself. |
| 24 | When a card's wait on another is met (card #109; the router for Hugh, 6 Nov 2026) | A card in the same repo: in Human test or later. In another repo: Deploy complete. A card archived in Deploy complete or Done counts. The message to the card that starts names each blocker not shipped yet, with its branch, and says its work is not on main yet. |
| 25 | How the Router archives (card #131; Hugh, 6 to 8 Nov 2026, with "you work it out" for the details) | The Router proposes and Hugh clicks: it asks with `kanban_archive_card`, the card waits in the board's Done pile, and Hugh's **Archive all** moves the pile into Done. No reading of his wording and no git gate (three critiques showed both too fragile: a branch is deleted at ship, and free text can't be parsed safely). Approving is his Archive drop done for him, with its rules: any column but Ship it, and running sessions are stopped (a lead's children's too). Not now stands until Hugh or the card's own session changes the card. The rules are store functions, so they are tested without the app. |
| 26 | Which subagents run on a card (card #151; Hugh, 9 Nov 2026) | Each card keeps its own on/off for the card-router, card-critic, card-reproducer, card-reviewer, card-tester and card-merger, set from Settings > Sessions' defaults when it is made (the four old board-wide rules became the defaults, so the critic stays off; router and merger defaults are new, on). Only Hugh changes it, on Add card or in the card's details; sessions, the Router and `kb` can't. The holds read the card's choice; a card made before #151 follows the defaults. Every session still has all six defined, and is told which to skip. Card #162 took the card-merger off the list (every ship merges in its repo's merge lane): five now. |
| 27 | Size cards before their session starts (card #159; Hugh, 9 Nov 2026: "I would like there to be an option to turn this on or off in the settings") | The setting `sessions.size_before_start`, on by default. On: just before a card's session starts in Planning or In progress (a move, the board Router's start, Hugh's Start a new session), the app runs one quick read-only `claude -p` with the card-router's own rules, on the model the subagent inherits at low effort, records the route and only then starts the session on it: no restart. Its own call, not the board Router, which is often off. A failed call never stops the start: the session routes the card itself, as before. Off: the in-session card-router, as before. Unchanged: Route again, and the switch back to the column's values when a card leaves In progress. Card #161 (decision 28) changes what off and a failed call do. |
| 28 | Sizing off means no sizing (card #161; Hugh, 9 Nov 2026: "If it's off, it should be off. It shouldn't do it later. It should just go with my defaults.") | A card is sized only by the app before its session starts (setting on), or by Hugh's Route again. With the setting off, or when that call fails, or when a card enters Planning with its session already running: no card-router run, no route, no restart; the session runs on Hugh's column defaults and its Planning message says so. A route reported while off isn't recorded; Route again is refused while off. Routes made while on stay, so turning it off restarts nothing. |
| 29 | A card's subagents are three-way (card #162; Hugh, 9 Nov 2026: "overrideable per card") | A card holds only Hugh's overrides; a subagent it doesn't name follows Settings as it is now, so a later change of a default reaches it. Migration 17 took the values card #151 stamped off where they equal the default (archived cards left as they are). |
| 30 | Skip Planning (card #162; Hugh: "an option on a card to skip planning") | A per-card option with a Settings default (off). A move into Planning from Backlog or Blocked, by anyone, goes to In progress instead, with no plan request or critique; the choke points are the store's `move_as` and `dispatch::session_move_to`. Hugh's move into Planning from a later column still plans. A stack's child ignores it. |
| 31 | Skip Human test (card #162; Hugh: "only applies if it hasn't been skipped ... overrideable per card") | A per-card option with a Settings default (off). A card a session (or the stack rules) puts in Human test goes on to Ship it for Hugh, as Take it to: Deploy complete does. Hugh's own drag never skips; Take it to: Human test wins; a child never goes on; it goes before small-change auto-ship. Turning it on for a card a session already put in Human test sends it on, as an autoTo edit does. |
| 32 | Who sets a card's flow (card #162) | Only Hugh: the page, Add card, and his own sessions outside the app and `kb` (calls with no card session). A card's session and the Router are refused. A default changed in Settings is told to the following cards' sessions where it matters. |

## Stack

**Tauri 2** with a Rust backend and a web UI in the system WebView (WebView2 on Windows 11, which is Chromium).

- **The board is already a web page.** Its CSS tokens, card layout, drag and drop, and detail dialog port nearly as they are. The `ArtifactData` calls become Tauri `invoke` calls into Rust.
- **The terminal is xterm.js.** Kangentic proves xterm.js renders Claude Code's TUI well in Chromium. A pure-Rust UI would need a terminal renderer written from scratch, which is the biggest risk such a build would carry, and it buys nothing you would see.
- **Rust owns the difficult parts:** PTYs, Job Objects, process lifecycle, SQLite, the hook receiver, the MCP server and notifications. That is where Electron and node-pty hurt in Kangentic (ConPTY double-kill heap corruption, native-module rebuilds, bridge scripts that need an external `node`).
- **The installer is small** (about 10 MB, against Electron's 150 MB or more).

**Front end:** plain JavaScript with no build step in Phase 1 (the board page's own script, with its data calls moved to Rust). A bundler comes in only if Phase 2's xterm.js needs one. No framework. The board is about 450 lines of plain JavaScript today, and keeping it close to that makes the port a copy rather than a rewrite. It keeps the board's fonts and colour tokens (IBM Plex, the accent blue, both themes).

**Main crates:** `tauri` 2, `portable-pty` (ConPTY), `windows` (Job Objects), `rusqlite` (bundled, WAL), `rmcp` (MCP server, streamable HTTP), `axum` (localhost HTTP for hooks, MCP and `kb.exe`), `tokio`, `serde`, `notify` plus polling (file watching), `tauri-plugin-notification`, `tauri-plugin-single-instance`, and the tray support built into Tauri.

**Toolchain:** Rust isn't installed on this machine. Phase 1 installs `rustup`, the MSVC build tools and the Tauri CLI, **only once you have said so in the build session** (the manager has passed on your permission, but it has to come from you there). Everything installed is listed in `INSTALLED.md`, with how to uninstall it.

## Architecture

```
+---------------------------- Tauri window ----------------------------+
|  Board (7 columns, drag, Add card, search, archive)  |  Card pane     |
|  ported from current-board-page.html                 |  xterm.js      |
|                                                      |  + card detail |
+-------------------- invoke / events (Tauri IPC) ---------------------+
|                           Rust core                                  |
|  store      SQLite, append-only logs, version-pinned card writes     |
|  sessions   one PTY per card, all in one kill-on-close Job Object    |
|  activity   hook events -> idle / thinking / permission / ended      |
|  dispatch   column rules, moves and edits typed in, held moves       |
|  api        127.0.0.1: MCP for sessions, REST for kb.exe, hooks      |
|  notify     Windows toasts, tray, focus card on click                |
+----------------------------------------------------------------------+
     ^ spawns claude.exe in a PTY     ^ hooks POST here (type "http")
                                      ^ kb.exe (CLI) calls the REST API
```

**Hooks use Claude Code's `http` hook type** (as built in Phase 3: `UserPromptSubmit`, `Stop`, `StopFailure`, `Notification` and `SessionEnd` arrive this way, but `SessionStart` does not, so the statusLine's first report marks a session ready). That type is in the changelog of the installed Claude Code, and it POSTs the hook's JSON to a URL, so no sidecar exe is needed for hooks. Phase 3 confirms it against the Claude version found on the machine before relying on it. The statusLine still has to be a command, so it runs `kb.exe status`, which sends the status JSON to the app and also writes it to the session's `status.json`.

**`kb.exe`** is a small CLI that ships with the app. It is how a session reaches the board when it has no MCP tools: a session started while the app was down, or one outside the app (see *Sessions outside the app*).

## Terminal

- **One PTY per card session**, opened with `portable-pty` (ConPTY) as `xterm-256color`. The first size is the pane's fitted size when the pane is open, otherwise a fixed 210x48 "resting grid", as in Kangentic.
- **Output:** each session's bytes go into a 512 KB ring buffer in Rust, and to the front end at about 60 Hz, but only for the session on screen. Hidden sessions keep buffering and are never rendered.
- **Switching cards:** fit xterm.js to the pane, send the resize, wait for the TUI's full repaint (`ESC[2J`, at most 500 ms), then replay the buffer and restore the DEC private modes. This is Kangentic's sequence, and it avoids the half-drawn frame.
- **Input:** keystrokes are batched into one IPC call per animation frame. Paste uses bracketed paste.
- **Resize:** debounced at 100 ms and clamped. Output that arrives in the 1.5 s after a resize doesn't count as activity.
- **xterm.js addons:** fit, WebGL (falling back to canvas if the context is lost), unicode11, web-links and search.
- **Claude TUI settings:** the session's settings set `tui: "fullscreen"`, and the environment sets `CLAUDE_CODE_ALT_SCREEN_FULL_REPAINT=1` (both are Kangentic workarounds for TUI redraw bugs).

## Process lifecycle

### Spawning

The app runs `claude.exe` **directly** in the PTY, with no shell in between. That removes Kangentic's PowerShell `& ` prefix, its quote swapping and its cwd hacks. The binary is found the way `~\bin\claude.cmd` finds it: the newest `%APPDATA%\Claude\claude-code\<version>\claude.exe`, then `~\bin\claude.exe`. You can pin a path in Settings.

| Flag | Value |
|---|---|
| `--session-id <uuid>` on a fresh start, or `--resume <uuid>` | The app makes the UUID and stores it as the card's `claudeSessionId` |
| `--model`, `--effort` | On every spawn, because Claude doesn't remember them |
| `--permission-mode bypassPermissions` | From the column, or the card's override |
| `--settings <session dir>\settings.json` | The project's settings, merged with the app's hooks, statusLine and `tui: "fullscreen"` |
| `--mcp-config <session dir>\mcp.json` | The app's MCP server, with this session's caller URL and token. No `--strict-mcp-config`, so your own MCP servers still load |
| `-- "<first prompt>"` | The column prompt, wrapped as `<task>…</task>` |

- **Environment:** the app removes `CLAUDECODE` and every inherited `CLAUDE_CODE_*` variable (otherwise `--resume` fails with "No conversation found"). It sets `TERM=xterm-256color`, `KANBAN_CARD=<number>` and `KANBAN_SESSION=<uuid>`, and sets `CLAUDE_CONFIG_DIR` for Account 2 (see *The two accounts*).
- **Session files:** `%LOCALAPPDATA%\claude-kanban-dsh\sessions\<uuid>\` holds `settings.json`, `mcp.json` and `status.json`.
- **Working folder:** the repo's folder, from the repo map: as built, the `folder` of the repo's entry in the private folder's `repos.json` (`[{"repo":"owner/name","folder":"~\\name","label":"name"}]`; see README, "Private folder"), otherwise the home folder. A session may make its own worktree, as it does today. The app doesn't create worktrees in v1. Once a card has a `worktreePath`, a resume starts there.
- **Folder trust:** the app **never writes `~\.claude.json`** (or Account 2's copy). It only reads them to check whether a folder is trusted. Every folder in the repo map is one you already use, so it is trusted. For a new folder, the trust question appears in the card's terminal and you answer it once.

### Job Object

Every `claude.exe` the app starts goes into **one Windows Job Object set to kill on close** (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`). The app holds the only handle. If the app crashes or is killed, Windows closes the handle and kills every card session and its child processes. So no session is left behind, and a later `--resume` can never open a conversation that is still running. Two more guards back this up:

- A **resume lock:** the app refuses to start a session for a `claudeSessionId` that already has a live process in the job.
- **WSL processes are outside the job.** A dev stack a session started inside WSL keeps running after the app dies. That is intended, because the stack belongs to the card, and the ship session stops it.

### Stopping

- **Suspend** (Done, Quit): send `Ctrl+C`, then `/exit` and Enter, wait 1.5 s, then kill. Wait another 1.5 s so ConPTY releases its file handles.
- **Kill** clears the PTY handle *before* killing, so a second kill can't happen (the ConPTY heap-corruption lesson). An exit wait has a 10 s safety timeout, because ConPTY sometimes never reports the exit.
- A session that exits on its own is marked `exited`. The card shows a Resume button, and nothing respawns it automatically.

### App restart

- **On Quit:** suspend every session in parallel, recording the state each was in (idle, or mid-turn).
- **On start after a crash:** the Job Object has already killed the sessions. Sessions last recorded as `running` get a new `orphaned` event, and the latest session per card is kept.
- **Mid-turn sessions** resume straight away with `--resume <uuid>`. After their first `SessionStart`, they get "The kanban app restarted while you were working. Carry on from where you were."
- **Idle sessions** resume the first time you open their card, or when a move needs them.
- **`/clear` changes the conversation id.** The app reads the current id from `status.json` (via the statusLine), so a resume uses the right conversation.

## The two accounts

- A session's account is `main` (no `CLAUDE_CONFIG_DIR`) or `account2` (`CLAUDE_CONFIG_DIR=~\.claude-max2`). **It is stored on the session's row** and never changes for that session.
- **A new session takes the account with the lower 7-day usage**, as your rule says (`kangentic-pick-account-with-most-weekly-tokens`). The app reads the newest `rate_limits` (5-hour and 7-day `used_percentage`) that each account's statusLine reported. It falls back to `python ~/.claude/scripts/kangentic_account_pick.py` until each account has reported once, and to `main` if neither has. The card log says which account it chose and both 7-day percentages.
- A card can pin an account in its detail. A pin applies to the next new session; it never moves a running one.
- **A resume always uses the session row's account.** If you choose another account for a card that has a session, the app starts a new session there, because a transcript can't move between accounts. The app refuses a `--resume` whose account doesn't match the row.
- The card pane shows an "Account 2" chip.

## Storage and migration

**SQLite** (`rusqlite`, WAL, `busy_timeout` 5 s, foreign keys on) at `%LOCALAPPDATA%\claude-kanban-dsh\board.db`. Only the Rust core writes to it.

```
meta            key, value                  next_number, schema_version, imported_from
cards           id TEXT PK, number INT UNIQUE, title, column, ord REAL, ord_column,
                repo, issue_number, issue_url, pr, branch, worktree_path, dev_stack, persona,
                account_pin, model_override, effort_override, permission_override,
                summary, next_step, blocked_reason, moved_by_hand INT,
                ship JSON, plan JSON, ship_requested_at, plan_requested_at, dry_run INT,
                logins JSON, links JSON, archived INT, archived_at, archived_by,
                legacy JSON, created_at, created_by, updated_at, updated_by, version INT
card_log        card_id, at, text, by                    INSERT only
card_revisions  card_id, version, at, by, changed JSON   INSERT only (every card write)
sessions        id, card_id, claude_session_id, account, cwd, model, effort, started_at
                                                         INSERT only
session_events  session_id, at, status (queued|running|suspended|exited|orphaned),
                state (idle|thinking|permission|failed), detail
                                                         INSERT only
pending         id, card_id, session_id, kind (move|message), payload, created_at,
                done_at                                  INSERT, then done_at set once
columns         name PK, position, prompt_file, prompt_text, mode, model, effort,
                permission_mode, auto_spawn, on_enter
settings        key, value
```

- **As built in Phase 1:** `cards` holds `number`, `column`, the order, `archived` and `version` as columns, and every other field in a `fields` JSON column under the artifact's own camelCase names. So an imported card reads back exactly as it was exported, and fields the app doesn't use (`watcherNotifiedAt`, the old session fields) are kept as they were.
- **Append-only.** `card_log`, `card_revisions`, `sessions` and `session_events` take inserts only. A session's current status is its latest event. There is **no `DELETE` anywhere in the code**: archiving is a flag, and a unit test greps the Rust and SQL sources for `DELETE` and fails if it finds one. `cards` rows are updated, but every update also writes a `card_revisions` row with the fields it changed, so any earlier state can be rebuilt.
- **Version pinning:** every card write is `UPDATE … WHERE id=? AND version=?`, then `version+1`. A stale save from the UI is **rejected, and your typed text is kept**. The dialog shows the card as it is now next to what you typed, with **Re-apply my text** and **Discard**. The MCP tools and `kb.exe` take an optional `if_version` that works the same way.
- **Card numbers** come from `meta.next_number`, taken and incremented in the same transaction that creates the card. It is seeded from the highest number the artifact ever gave out, archived cards included. A number is never reused, even if a card is lost.
- **The secret filter** from the page (`ct=`, `customtoken`, `idtoken`, JWTs, Stripe keys, `password`) is applied in Rust to every string written, by the UI, the MCP tools and `kb.exe` alike.
- **Backups:** a copy of `board.db` every night and before every migration, in `%LOCALAPPDATA%\claude-kanban-dsh\backups\`, keeping 30.
- **Schema changes** are numbered migrations recorded in `meta.schema_version`.

### Migration

The importer is built and tested in Phase 1, against a copy.

1. **Export:** a Claude session runs `ArtifactData list` on `cards` (every page) and saves every document with its `version` to `~\Documents\kanban-backups\<date>-app-copy\cards.json`. The artifact is not changed.
2. **Import:** `kb.exe import <cards.json>` maps the fields one to one (`order`/`orderCol` to `ord`/`ord_column`, `log` to `card_log`, the old `sessionId`, `sessionLink`, `sessionTitle`, `claudeSessionId` and `account` into `legacy`). It keeps ids, numbers and archived cards, and seeds `next_number`.
3. **Diff:** `kb.exe import --check` reads both sides back and compares counts per column and archived, then every field of every card. It must report zero differences. Anything it can't map fails the import; it isn't skipped.
4. **Cut-over (Phase 5)** happens in this order:
   1. **Close the artifact's writes first.** Republish the board page with its `db` capability rules set to refuse every write to `cards`, then check that a test `update` is refused.
   2. Re-export and re-import, and diff again.
   3. Make skill v3 live.

   This order means no write can land on the artifact after the final export.

## Columns, prompts, model and effort

Each column has a row in `columns`, edited in Settings > Columns. A column can:

- run a **prompt** on entry: a file path (the playbooks stay where they are, and the app reads them when it runs them) or inline text;
- use a **mode:** `immediate` (start the session with the prompt, or type it now), `deferred` (type it when the current turn ends), or `none`;
- set the **model, effort and permission mode** for sessions in it;
- **auto-spawn** a session when a card enters with none;
- **suspend** the card's session when the card enters (Done).

| Column | Prompt on entry | Mode | Model | Effort | Session |
|---|---|---|---|---|---|
| Backlog | none | none | — | — | none started |
| Planning | the start-card brief (from `work-board-start-card\SKILL.md` steps 2 to 5): claim, read the issue, plan, write the approach to `summary` and `nextStep` | immediate | Opus 5.5 | high | auto-spawn |
| In progress | the repo's build playbook (`{prompts}\in-progress.md` as built) | deferred | Opus 5.5 | high | auto-spawn |
| Blocked | "Hugh moved this card to Blocked: <reason, if given>. Wait for his answer." | deferred | Sonnet 5.5 | medium | kept |
| Human test | none (the build playbook says Human test has no agent of its own) | none | — | — | kept, idle |
| Ship it | `ship-it.md`, with the Work Board changes from the skill's *Shipping* section. After a successful ship, the same session follows `deploy-complete.md`. | immediate | Opus 5.5 | medium | auto-spawn or resume |
| Done | none | none | — | — | suspended |

**As built:** the column prompts are files in `{prompts}`, which the app fills in as the private folder's `prompts\` (Settings › Sessions › "Private folder", setting `private.dir`) or, without one, the bundled `prompts\` examples: `planning.md`, `in-progress.md`, `ship-it.md`, `deploy-complete.md`, and `kanban-app.md` for this repo's own cards.

**Per-card overrides:** the card detail gets Model, Effort, Permission and Account. They are blank by default, meaning the column's setting is used.

- **Changing the effort** of a running session types `/effort <level>` when its turn ends.
- **Changing the model** suspends the session and resumes it with `--resume <uuid> --model <new>`.
- The pane shows the model and effort each session really started with (from `sessions`).

## Moves, edits and the held move

### When you move a card

1. **One transaction:** the column, `movedByHand`, a log line, the order, and the ship and plan states (the page's `moveExtras` rules).
2. **The session:** the card's running session, or its latest session resumed, or a new one if the column auto-spawns.
3. **A message**, always: "Hugh moved this card from X to Y.", followed by the column's prompt if the column has one.

### When you edit a card

Saving `summary`, `nextStep`, `blockedReason` or an override sends "Hugh edited <fields> on this card:" followed by the new values, over the same deferred path. Several edits made before the session is idle are merged into one message. Order changes and archive or restore don't send anything.

### How text is typed

The steps follow Kangentic's tested sequence:

1. `Ctrl+U` clears the prompt line.
2. The text goes in as a bracketed paste, in 1 KB pieces.
3. The app waits for the output to settle.
4. `Enter` is sent as a separate write, because Ink misreads it when it arrives in the same read.
5. The app confirms the prompt arrived through the `UserPromptSubmit` hook. If it didn't, it retries `Enter` once. As a last resort it restarts the session with the text as its first prompt.

### When text is typed

- **A new session** gets nothing until its first `SessionStart`.
- **A deferred message** waits for `idle` plus 1.5 s of silence. Until then it stays in `pending`, and the card shows "Message queued".
- **No text is ever typed during a permission prompt**, because it would answer the prompt.
- **An immediate prompt on a busy session** queues behind the current turn.

A card moved with no session, into a column that doesn't auto-spawn, is only moved.

### The held move

`kanban_move_card` (or `kb.exe move`) called by the card's own session answers "Queued: this card moves to X when your turn ends". The app stores one pending move per session, and a later one replaces it. It applies the move at the session's next `Stop` (idle, not in a permission prompt) or exit. A move of a different card, or your drag, happens at once. A session can never move a card into Ship it: the tool refuses.

### Activity, from hooks

The settings file registers only `SessionStart`, `UserPromptSubmit`, `Stop`, `StopFailure`, `Notification` and `SessionEnd`, each as an `http` hook to `http://127.0.0.1:<port>/hook/<sessionId>`. None fires on every tool call, which keeps a slow start-up (an unsigned exe checked by Defender) off the tool path.

| Event | State |
|---|---|
| `UserPromptSubmit` | thinking |
| `Stop` | idle |
| `StopFailure` | failed, or retrying for overloaded and rate-limit errors |
| `Notification`: permission | permission (needs you) |
| `Notification`: waiting for input | idle hint (covers a `Stop` that goes missing when a subagent did the work) |
| `SessionEnd` | ended |

There is a 400 ms stability window, and a 180 s watchdog clears a "thinking" state that has gone stale when the PTY is silent. Without tool hooks, a long silent tool call can trip that watchdog. So the watchdog only marks the card "quiet", never idle, and nothing is typed until a real `Stop` arrives.

### The MCP tools and the CLI

The MCP server runs as `rmcp` streamable HTTP on `127.0.0.1:<port>/mcp/<sessionId>`. Each session's `mcp.json` sends a random token as a header, rotated each launch. The session settings allow `mcp__kanban` and add an `autoMode.allow` rule. The server's instructions say what the card is and how the flow works.

| MCP tool | `kb.exe` | What it does |
|---|---|---|
| `kanban_get_card` | `kb card [<n>]` | This session's card, or one by number |
| `kanban_list_cards` | `kb list` | Every card, with filters |
| `kanban_update_card` | `kb update <n> --field value` | `summary`, `nextStep`, `pr`, `branch`, `issue`, `devStack`, `persona`, `logins`, `links`, `blockedReason`, `choices`, `worktreePath`; optional `if_version` |
| `kanban_add_log` | `kb log <n> "<text>"` | Add a log line |
| `kanban_move_card` | `kb move <n> "<column>"` | Move a card; held for its own card, refused into Ship it; a stack's lead is refused into Human test until its children are ready (the app moves it) |
| `kanban_create_card` | `kb create --title … [--stack <lead>]` | New card with the next number; `stack` creates it into that stack |
| `kanban_stack_list` | `kb stack [<lead>]` | A stack: the lead (branch, pr, devStack, logins) and its children in order (column, passed, nextStep, blockedReason, choices) |
| `kanban_stack_add` | `kb stack <lead> add <n>` | Put a card into a stack |
| `kanban_stack_remove` | `kb stack <lead> remove <n>` | Take a card out of a stack: an ordinary card again |
| `kanban_set_passed` | `kb passed <n> [--off]` | Mark a stack's child passed, or take it back |
| `kanban_ship_result` | `kb ship-result <n> <state>` | `shipping`, `shipped`, `failed` (with retries) or `needs-hugh`, with detail |
| `kanban_notify` | `kb notify <n> "<text>"` | A Windows notification on the card |

`kb.exe` reaches the app through the REST API on the same port, with the token from `%LOCALAPPDATA%\claude-kanban-dsh\token`. `KANBAN_SESSION` in the environment tells the app which session is calling. If the app isn't running, `kb.exe` says so and exits non-zero. It never writes the database itself, so the app stays the only writer.

## Shipping

It stays your decision. A card gets into Ship it only through:

- **your drag;**
- **the app's command bar** (`Ctrl+K`): `34 ship it`, `12 plan it`, `7 to Blocked`, or just `9`. This is `/kanban <n> …` inside the app, and it ships straight away;
- **`/kanban 34 ship it` in a Claude session outside the app.** The skill runs `kb ship 34 --asked-by-hugh`, and the app asks you to confirm "Ship #34?" in a banner and a notification. Without your click it doesn't ship.
- **the app, for you,** when you set the card to go on by itself to Deploy complete (decision 11): it moves the card from Human test into Ship it.

**Then:**

1. The app sets `ship: {state: "requested", retries}`, clears `logins` and `devStack`, and types the ship prompt into the card's own session, resuming it, or starting one in the card's worktree.
2. **The ship session stops the card's dev stack** (with the repo's own stop command), as the first step of its prompt. **The app never runs WSL or dev-stack commands.**
3. The session reports through `kanban_ship_result`:
   - **Failed, with fewer than 3 retries:** the card moves to In progress with `retries+1`, and the message tells the session to fix that failure first.
   - **Failed, with 3 or more retries:** `needs-hugh`, and the card stays in Ship it.
   - **Shipped:** the session runs `deploy-complete.md`, adds the Production login and moves the card to Done.
4. Moving the card out before the session starts sets `cancelled`, and the pending prompt is dropped.
5. `dryRun` cards get "Dry run: say what you would do and change nothing."

## Logins

The detail dialog keeps the logins box: persona, browser, link, Open and Copy. **Open now opens the right Chrome profile**, which a web page couldn't do. Settings maps each browser label to a launch command:

| Browser label | Command |
|---|---|
| Chrome · work (Profile 1) | `chrome.exe --profile-directory="Profile 1" <url>` |
| Chrome · Default (personal) | `chrome.exe --profile-directory="Default" <url>` |
| Built-in browser | the system default browser |

- Links are tokenless local dev-stack sign-in links (`http://127.0.0.1:<port>/...`, naming the role and the page, never a token). The secret filter refuses anything that carries one.
- Before showing a login as live, the app checks that the dev stack answers its local sign-in endpoint. It reads the status code only; the token is never read or stored. A login whose stack doesn't answer is greyed out with "stack down".

## Sessions outside the app

Sessions in the desktop sidebar or the CLI will still add and update cards, but only after cut-over. Until then the artifact is the real board, and the app works on a copy.

- **At install during cut-over,** the install script runs `claude mcp add --scope user kanban …` once per account, using Claude's own command, never by editing the file. After that, the app only checks that the entry is there. It never writes `~\.claude.json`.
- **Skill v3** uses the `kanban_*` tools when they are loaded, and otherwise falls back to `kb.exe` through Bash (for a session started while the app was down). It keeps the hard rules, freedoms, logins and the GitHub Project sync, and drops *Dispatch*, the pool and the watcher. The old skill stays as `SKILL.md.pre-app`.
- **The SessionStart hook** (`work-board-session-start.ps1`) changes its line to point at the app.

## Notifications, packaging and install

**Notifications** are Windows toasts. Clicking one focuses the app on that card's terminal. Nothing fires for the card you are looking at in a focused window.

| Event | Toast |
|---|---|
| Needs you | "#34 needs you: permission prompt" |
| Finished its turn in In progress or Planning | "#34 finished its turn" |
| Blocked | "#34 is blocked: <reason>" |
| Ready to test | "#34 is ready to test" |
| Ship | "#34 shipped (<tag>)", "Ship failed (retry 1 of 3)", "Ship needs you" |
| Crash | "#34's session crashed" (not shown for exits the app started) |
| Ship request from outside | "Ship #34?" with a Ship button |

The tray icon shows how many sessions need you. The `/notify` voice skill keeps working, since sessions call it themselves.

**Packaging:** `cargo tauri build` produces an NSIS installer for a per-user install (no admin) into `%LOCALAPPDATA%\Programs\Claude Kanban DSH`. `scripts\install.ps1` then:

1. quits the running app cleanly, which suspends its sessions;
2. runs the installer silently;
3. at cut-over only, adds the `kanban` MCP entry;
4. starts the app, which resumes its sessions.

Only one copy runs at a time (`tauri-plugin-single-instance`). `kb.exe` is installed next to the app, and its folder is put on your user `PATH`.

## What to port from Kangentic, and what to drop

Kangentic is AGPL-3.0. The Rust app reimplements its ideas and copies no code. Read Kangentic's `docs\session-lifecycle.md`, `activity-detection.md` and `command-injection.md` while building Phases 2 and 3.

**Port:**

| Idea | Where it is in Kangentic |
|---|---|
| Fresh vs resume, with the app owning the UUID | `transition-engine\spawn-intent.ts` |
| Correcting the resume id after `/clear` | `resume-id-reconcile.ts` |
| Removing inherited `CLAUDECODE` and `CLAUDE_CODE_*` | `pty\spawn\pty-spawn.ts` |
| `--model` and `--effort` on every spawn; merged per-session settings and `mcp.json` | `command-builder.ts`, `hook-manager.ts` |
| Hook events driving activity: the stability window, the stale watchdog, `idle_hint` | `activity-engine\engine\activity-engine.ts` |
| File watching plus 1 s polling, because fs.watch goes silent on Windows | `pty\readers\file-watcher.ts` |
| The typing sequence: Ctrl+U, bracketed paste, a separate Enter, confirm, retry | `pty\paste-engine.ts`, `terminal-submit.ts` |
| Waiting for the turn to end, and never typing during a permission prompt | `turn-completion.ts` |
| The held self-move | `agent\mcp-http\self-move-deferral.ts` |
| Graceful suspend, and clearing the handle before a kill | `pty\shutdown\session-suspend.ts`, `pty\lifecycle\pty-kill.ts` |
| Ring buffer, repaint-then-replay, the resting grid, ignoring output after a resize | `pty-buffer-manager.ts`, `resize-manager.ts`, `useTerminal.ts` |
| A localhost MCP server with a rotated token and a caller URL per session | `agent\mcp-http-server.ts`, `caller-url.ts` |
| The fullscreen TUI and the Windows full-repaint variable | `command-builder.ts`, `pty-spawn.ts` |

**Drop:**

- Spawning through a shell, and its quoting and cwd workarounds.
- The Node bridge scripts, replaced by `http` hooks and `kb.exe status`.
- Hooks on every tool call.
- Kangentic's writes to `.claude.json` for trust.
- Multiple agents and multiple projects.
- Board profiles, and transition graphs with webhooks and scripts.
- The embedded browser, analytics, handoffs, the mobile bridge, and PR integration.
- Worktree creation in v1.
- The packed `?ct=as-…` login links, which carry a token.

## Time in each column (card #111, 6 Nov 2026)

- **Source:** `card_revisions`, which already records every column and archive change from every
  writer, rather than the log's wording. No migration and no new table. Older cards' first column
  comes from the first move's log line; where none says, that stretch isn't counted rather than
  guessed.
- **The board's figure is a median, not a mean** (Hugh asked for an average): one card left in a
  column for weeks would swamp a mean. Decided by the router for Hugh (decision A): finished visits
  only, over the last 30 days, dry-run cards left out.
- **The panel shows Planning, In progress and Human test** (the columns Hugh named); the others
  fold away under one line (decision B).
- Wall-clock time; archived time and Done don't count. Not built: times on the card face, in `kb`
  or the MCP tools, or charts.

## Phases

The first usable release is Phase 4. It has the board, a terminal per card, moves and edits typed into sessions, the `kanban_*` tools with `kb.exe`, and restart recovery, **all running on a copy of the cards**. You use it alongside the artifact for a few days. Cut-over (Phase 5) only comes after that, and restart recovery is already in place by then.

**Before each phase starts,** its acceptance tests go into `ACCEPTANCE.md` and its undo steps go into `UNDO.md`. The phase is done when every test passes. The build session reports to the manager after each phase in five lines or fewer.

| Phase | What you get at the end | Size |
|---|---|---|
| **0. Skeleton** (no installs) | Git repo, this plan, `ACCEPTANCE.md` and `UNDO.md` for Phase 1, `INSTALLED.md`, the folder layout, the board page as the port source, and a read-only export of today's cards for the importer tests | 0.25 day |
| **1. Toolchain, store, board on a copy** | After your go-ahead: Rust, the MSVC build tools and the Tauri CLI. Then the Tauri app with the append-only store, `next_number`, version pinning that keeps your text, the secret filter, the importer with its diff, and every board feature (drag and order, Add card, archive and restore, search, the detail dialog, logins opening the right Chrome profile, both themes), all on the imported copy | 1.5 days |
| **2. The card panel, a terminal per card, both accounts** | Clicking a card opens a panel over the board, after Kangentic's card view: the card's conversation read from Claude's transcript (Hugh's messages, Claude's replies as markdown, tool runs folded into "Ran N tools"), a box that types a message into the session, a Terminal toggle with the live xterm.js terminal, and a footer with account, model, effort and Claude Code version. It is in the first usable release, because it is how Hugh talks to a card. Then the session itself: start, resume, suspend, type, switch, resize, scrollback. Every session in the kill-on-close Job Object, with the resume lock. **The account test:** four concurrent sessions on each account, each resume on its row's account, a wrong-account resume refused, a token refresh on one session not signing others out. "Resume this desktop session in the app", when you ask for it. | 2.5 days |
| **3. Hooks, tools and the dispatcher** | `http` hooks confirmed, state dots, the MCP tools and `kb.exe`, the held move, column settings with model, effort and permission, per-card overrides, the 7-day account pick, moves and edits typed into sessions, Planning and Ship it handled, Done suspends, the command bar | 2.5 days |
| **4. First usable release** (on the copy) | Restart recovery (mid-turn sessions resume with the "carry on" message), tray, toasts, the outside-ship confirm, nightly backups, `install.ps1`. **You use it for a few days**, on cards copied from the artifact. | 1 day |
| **5. Cut-over (cancelled 3 Nov 2026: the app runs separately)** | Artifact writes closed and checked, final export, import and diff, the `kanban` MCP entry added in both accounts, skill v3 and the SessionStart line live, the pool tasks and ship watcher switched off (not deleted), the manager session retired | 0.5 day |

**Total: about 8 working days, plus 2 days of slack for ConPTY and TUI surprises, so plan on 10.**

**Testing:**

- **Rust unit tests:** the store (version conflicts, numbering, the secret filter, the no-`DELETE` grep), command building, the account choice and the typing sequence.
- **A fake `claude`:** a small exe that draws a TUI-like screen and POSTs hook events. It is used for spawn, Job Object, resume, typing and held-move tests, which then cost nothing and need no account.
- **A manual run** of each phase on a `dryRun` card with a real `claude`.

## Deferred

These are not in v1. Each can be added later without changing the design:

- the `vt100` screen snapshot (only if switching cards still shows artefacts);
- a cap on concurrent sessions and its queue;
- toast cooldowns;
- an "Interrupt and send" button;
- CI;
- the signed updater;
- start on login;
- a check of Remote Control for card sessions;
- phone access.

## Risks

| Risk | Likelihood | What we do |
|---|---|---|
| ConPTY quirks: exits not reported, handles held, a double kill | Medium | Kangentic's timeouts and handle-clearing from day one, the Job Object as the backstop, the fake-claude kill tests in Phase 2, and slack in the estimate |
| Typed text lands wrong (startup banner, a permission prompt, Enter swallowed) | Medium | Wait for `SessionStart`; never type during a permission prompt; send Enter separately; confirm via `UserPromptSubmit`; as a last resort restart with the text as the first prompt |
| Without tool hooks, a long tool call looks like a stale turn | Medium | The watchdog marks the card "quiet", never idle, and only a real `Stop` lets text be typed |
| The `http` hook type behaves differently from the changelog | Low | Phase 3 tests it first. If it falls short, a tiny `kb.exe hook` command replaces it (one process per hook, still only six events). |
| The Claude TUI draws badly in xterm.js after a resize or switch | Medium | The fullscreen TUI, the repaint variable, repaint-then-replay; the `vt100` snapshot is deferred, not dropped |
| Two accounts interfere (token refresh, the wrong account on resume) | Low to medium | The account on the session row, the resume check, and the Phase 2 account test |
| Claude Code changes a flag, hook or settings key | Low to medium | Command building and the settings merge in one tested module; the app shows the Claude version it found and warns on an untested one |
| A session still on the old skill writes to the artifact after cut-over | Medium | Artifact writes are refused before the final export, so the session gets an error it can see |
| Losing the board's data | Low | The artifact is untouched until cut-over; append-only logs and revisions; no `DELETE`; nightly backups; the export JSON is kept |
| The MSVC build tools need an admin install | Low | Phase 1 checks first. If they do, you run that one installer yourself. |

## Session search (card #110, 6 Nov 2026)

Decisions, after the router's critic (13 findings) and the router's rulings:

- The index is its own file (`search.db`, migrations `search1.sql`…), not part of `board.db`: it is
  much bigger than the board, its writes would hold the board's lock, and every board backup would
  carry it. No board migration; `schema13` stays unused.
- 90 days only, with a new file each month (or for a new index version) and the old one renamed into
  `backups\search`, one copy kept. Claude Code wasn't deleting transcripts at 30 days on this
  machine (the oldest was 72 days old on 6 Nov), so the index can always be rebuilt.
- Size, measured 6 Nov 2026: every transcript of the last 90 days holds 119 MB of indexable text;
  the card sessions' share (272 conversations and 296 subagents) is 20.3 MB, about 100 MB of
  trigram index. Well under the 1 GB line at which the router would have been asked.
- Search is Hugh's only (the board and `kb`); card sessions don't get `kanban_search`.
- Secrets are masked whole before they are stored; sign-in links are hidden entirely.

## What can be undone

Nothing before Phase 5 touches the live setup. The app runs on a copy of the cards, keeps its own session folders, writes hooks only into its per-session `--settings` files, and never writes `~\.claude.json`.

| Change | Phase | Undo |
|---|---|---|
| Toolchain installs | 1 | Each one's uninstall step in `INSTALLED.md` |
| The app | 1 | Uninstall it from Windows Settings, and delete `%LOCALAPPDATA%\claude-kanban-dsh` |
| `kb.exe` folder on the user `PATH` | 4 | Remove that one entry from the user `PATH` |
| A desktop session resumed in the app | 2 | It is still a transcript in `~\.claude\projects`. Reopen it in the desktop app, or run `claude --resume <id>`. |
| The artifact's writes closed | 5 | Republish the board page with its earlier capabilities (`page-versions\`) |
| The `kanban` MCP entry in both accounts | 5 | `claude mcp remove kanban --scope user`, once with `CLAUDE_CONFIG_DIR=~\.claude-max2` |
| Skill v3 and the SessionStart line | 5 | Copy back `SKILL.md.pre-app` and `work-board-session-start.ps1.pre-app` |
| Pool tasks and ship watcher switched off | 5 | Switch them back on in Scheduled tasks. They are disabled, not deleted, because deleting a task archives its runs. |
| Cards changed in the app after cut-over | 5 | `kb.exe export` writes the cards back out as JSON in the artifact's shape, for a session to restore with `ArtifactData` |

Every change is recorded in `UNDO.md` as it is made.

## Card #105: "Done when" and acceptance checks (6 Nov 2026)

Decisions (the router's, from Hugh's delegation, and the card session's), after the router's critic:

- **No migration.** Card fields are free JSON, and an older build keeps fields it doesn't know.
  #105 adds no schema file; schema11.sql is #104's.
- **The status is calculated, never stored** (router): blocked (in Blocked) > done (every check met
  and the card in Human test or later) > waiting on Hugh (in Human test) > working (its session
  starting or working) > paused. One source of truth: the column, the checks and the session.
- **Names start with `doneWhen`** (router), apart from the Done column and the "done" status.
- **The store owns each check's `by` and `at`**, so a writer can't fake who marked it.
- **The card-tester's prompt asks for CHECK lines on every card**, because it is fixed when the
  session starts; the app writes the results onto the card the report names.
- **Hugh's ticks aren't told in Human test, Ship it or Deploy complete**, so testing doesn't wake
  the session.
- **The status rule is checked in the test app through `tools/cdp.mjs`**, as the repo has no
  JavaScript test runner (router accepted).

## Card #146: each card in its own worktree (9 Nov 2026, stack #145)

Decisions (the card session's, from Hugh's brief for stack #145):

- **The app makes the worktree, not the session.** "One card builds at a time" was only a prompt
  rule. With a prompt rule alone each session would make its worktree by hand after starting in the
  shared checkout. Made first, the session starts in it and never works in the checkout.
- **Opt-in per repo** (`sessions.own_worktree_repos`, empty by default), naming the checkout like
  card #104's list, so a repo whose own prompts make their worktrees is untouched.
- **Inside the checkout**, `.claude\worktrees\card-<n>` (the 6 Nov rule): Claude Code trusts a
  trusted folder's subfolders, so the session isn't asked to trust a new folder. Stack children
  are told the same folder name.
- **`<channel>-card-<n>` on test channels**, because test boards are copies whose card numbers run
  ahead of live's.
- **From `origin/main` with no fetch**: the checkout's local `main` may hold a card's unshipped
  commits (the old way of building), and the board is locked during a start. The merger rebases at
  ship.
- **Stacks keep card #113's flow**, and a card with a `worktreePath` keeps it. Only a new session
  makes one: a resumed conversation stays where it ran.

## Card #148: the rules for parallel builds (9 Nov 2026, stack #145)

Decisions (the card session's, from Hugh's brief for stack #145: build in parallel, test each in
its own test app, ship one by one or together if stacked):

- **One rule, everywhere.** Every card for this repo builds in its own worktree and branch, tests in
  its own test app, and ships alone, one at a time per repo, rebased onto the latest `origin/main`
  by its card-merger. A stack ships as one. "One kanban-app card builds at a time" and "work on
  `main` in the shared checkout" are gone from the prompts and docs, and a test keeps them out.
- **A card makes its own worktree when the app hasn't.** Same folder and branch name as card #146's
  (`.claude\worktrees\card-<n>`, `card-<n>`), so the rule holds whether or not Hugh lists the repo.
- **The Router stops integrating single cards.** Its integration branch and install waves existed
  because every card shared one test app. With a test app per card, each card is tested on its own
  branch, and the ship's rebase onto `origin/main` replaces the integration branch. Hugh can
  overturn this; the merger's integrate job stays in the code.
- **Ship it leaves the worktree** and logs the commands to remove it: Windows won't delete a folder
  the card's running session is in.
- **Private prompts change on a branch.** The live app reads the private folder's `prompts\` from
  its checkout, so editing its `main` would change every live session before the code ships. The
  private branch ships with the stack.

## Card #172: no computer use and no browser for this app's cards (9 Nov 2026)

- **What is out** (Hugh): computer use (desktop screenshots, clicks) and any browser (Claude in
  Chrome, the built-in browser, chrome-devtools). **What stays**: `tools/cdp.mjs` into the
  card's own test app, and its shots as the card's evidence.
- **Scope**: this repo's cards only (the Router, for Hugh): other repos keep their browser checks
  until he says. One list, `nobrowser::REPOS`, widens it.
- **How**: deny rules for the whole servers in the card session's own settings, which hold its
  subagents too (live trial), rather than each agent's `disallowedTools` (which would miss
  the session itself and non-app subagents). The prompts say it as well, so a session doesn't
  plan a browser check it can't run.

## Card #162: the reviewer and the tester run together (9 Nov 2026)

- **Both run at once, after the install, and a pass counts only if it started after the other's
  latest FAIL** (Hugh: "Could these two be run in parallel?", yes). The app compares the times on
  the two records (`startedAt`, `failedAt`), not commits: it works for every repo, where card
  #104's snapshot is only for listed ones. A run whose start the app didn't see counts from its
  verdict's time.

## Card #160: one merge lane per repo (9 Nov 2026)

Hugh's ask (agreed): replace the per-card card-merger at ship with one merger per repo, a merge
lane that lives across ships and resolves conflicts with what it learnt merging the earlier
cards. Decisions (the card session's, for Hugh to overturn):

- **The lane only merges; the card's session still pushes and ships.** Pushing from the lane
  would make it the one session that may push, and Ship it's PR, deploy and live staging are card
  work. The lane makes commits on a detached HEAD and reports a SHA; the card's session checks it
  (its branch unmoved, then `git range-diff`), resets to it and pushes, so the one-push rule and
  the ORIGIN check stay as they were.
- **Opt-in per repo, empty by default** (`sessions.merge_lane_repos`, `{repo, checkout}` like the
  own-worktree list), so nothing changes until Hugh lists a repo. The card's merger switch (card
  #151) still decides: off means no merger and no lane.
- **It starts on a ship's merge request, not by a Start button.** Hugh's move into Ship it
  approved that ship: a second exception to "sessions start only by Hugh's click or auto-spawn",
  beside R1.
- **Main account; stopped after 15 min idle.** A resume needs the account it ran on. The
  conversation is kept, so the next merge resumes it without a process sitting idle.
- **Its own worktree, kept between jobs** (`<checkout>\.claude\worktrees\merge-lane`, detached),
  so its build folders stay warm. Cost: one more build folder per repo and a slow first test run.
- **Its jobs are the cards' `merge` records** (queued, running, then a verdict, or cancelled), so
  an app restart loses nothing and there is no queue table.
- **History twice:** the lane's own conversation, plus the repo's last 5 merges in every brief,
  so it survives a compaction or a lost transcript.
- **The stack job stays with the lead.** Merging a child into its stack's branch is the lead's
  card-merger's, as before; the stack still ships once, on the lead, through the repo's lane.
- **No lane panel on this card.** Hugh sees the lane in the card's log and chips and in Settings;
  opening its chat and terminal is card #129's.
- **Card #162 (Hugh, 9 Nov 2026: "this doesn't need to be a setting ... just platform wide"):
  every card with a branch, on every repo, ships through its repo's lane.** No repo list (a saved
  one is kept and ignored) and no card-merger at ship (its per-card switch and default are gone;
  it keeps a stack's children). The app finds the checkout at `kanban_merge` (the card's
  worktree's main checkout, else the own-worktree list, else `repos.json`) and keys the lane by
  the checkout's GitHub `origin`, so an old repo name shares its lane; none found is needs-hugh.

## Platforms (6 Nov 2026, cards #114 and #117)

- Every operating-system call sits in `src-tauri/src/platform/` behind one interface
  (`mod.rs`); Windows in `windows.rs` (card #114, moved unchanged), macOS in `macos.rs` (card
  #117). A test fails on Windows-only code anywhere else, so it shows on Windows before a Mac.
- `main.rs` calls `platform::at_start()` and `platform::on_run_event()` (no-ops on Windows), so
  a system's start-up needs don't touch main.rs.
- Prompts name their folder as `{prompts}` (the public split's syntax), resolved to the
  private folder's `prompts\` (setting `private.dir`), or else the bundled examples, so the same
  board works on any system; a worktree or another clone does not change which prompts sessions
  follow.
- CI: Windows on every push; macOS only when app code, scripts or configs change, plus a
  manual run (macOS minutes cost about ten times more on a private repo).
- macOS decisions (card #117): untested on real hardware and unsigned, with no Apple Developer
  account (Hugh); a self-signed certificate for stable folder permissions; Mac CI only on
  `dsh-117` and manual runs here, moving to the public repo after DSH ships; an account-2 card
  refuses to start on a Mac; product-repo dev stacks Windows-only; Cmd+Q asks when sessions run; a
  card's Stop ends claude only (as on Windows) and the reaper ends the rest when the app ends;
  the smoke test runs its own app (`io.github.hughht5.claude-kanban-dsh.smoke`); data in `~/.claude-kanban-dsh` and apps
  in `~/Applications`, the Windows names, so the later rename covers both.
