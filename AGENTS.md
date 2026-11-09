# Working on Claude Kanban DSH

Standing instructions for any session (a card session, a subagent, or Hugh's own) working in this
repo. Distilled from `HANDOVER.md`, `README.md`, `UNDO.md`, `ACCEPTANCE.md`, `PLAN.md` and
`scripts\install.ps1`; read those for detail. A card session for this repo also follows
`{prompts}\kanban-app.md`, which overrides the parts of the column prompts written for other repos.

**Contents**

- [What the app is](#what-the-app-is)
- [Layout](#layout)
- [Build and test](#build-and-test)
- [Parallel builds](#parallel-builds)
- [Test version and live](#test-version-and-live)
- [Driving the UI](#driving-the-ui)
- [Hard rules](#hard-rules)
- [Working style](#working-style)

## What the app is

- A Rust desktop app (Tauri 2), "Claude Kanban DSH": Hugh's own kanban board. Each card owns a live
  Claude Code session in an embedded terminal, and the app dispatches column moves to it (the
  column prompts in `{prompts}`: the private folder's `prompts\`, or else the bundled examples in
  `prompts\`; see README, "Private folder").
- Repo `~\claude-kanban-dsh`, public https://github.com/hughht5/claude-kanban-dsh, branch `main`.
  The repo is public: the owner's own prompts and repo list live in the private folder, and
  `scripts\check-public.sh` (the pre-commit hook, `git config core.hooksPath scripts/hooks`)
  refuses private material. Never put an employer's name, a work email or a home path here.
- Installed per-user in `%USERPROFILE%\Programs\Claude Kanban DSH`; data (board, backups, sessions,
  `api.json`, `api.log`, `update\`) in `%USERPROFILE%\.claude-kanban-dsh`. Not under AppData: Windows
  virtualises AppData writes from processes the Claude desktop app starts.
- Sessions reach the board through the `kanban_*` MCP tools or `kb` (`kb.exe`, on PATH; `kb` alone
  prints its usage). `kb` and `kb mcp` go through the running app's localhost API.
- One copy per channel runs at a time (single-instance lock).

## Layout

| Folder | What |
|---|---|
| `src-tauri/` | The app: sessions, Job Object, dispatch, update, the localhost API (MCP, REST, hooks) |
| `src-tauri/src/platform/` | Every operating-system call (`windows.rs`; macOS is card #117). Nothing outside it calls a Windows API, reads `USERPROFILE` or names a `.exe`: `src-tauri/tests/platform_guard.rs` fails if it does |
| `src-tauri/src/lane.rs`, `lanetools.rs` | The merge lanes (card #160): one app-owned session per listed repo merges its ships; `prompts/merge-lane.md` is its system prompt |
| `crates/store/` | The SQLite store, migrations (`schemaN.sql`), templates, stacks; tests in `tests/` |
| `crates/kb/` | `kb.exe`, the CLI and `kb mcp` stdio bridge |
| `ui/` | Plain JavaScript, no bundler; vendored libraries in `ui/vendor` (`ui-deps/vendor.mjs`) |
| `prompts/` | Example column prompts (`planning.md`, `in-progress.md`, `ship-it.md`, `deploy-complete.md`) and `kanban-app.md`; a private folder's `prompts\` replaces them |
| `scripts/install.ps1` | Builds, installs or stages the app (both channels) |
| `tools/cdp.mjs` | Drives the WebView over its DevTools port |
| `design/`, `fixtures/` | Specs (for example `design/stacked-cards/SPEC.md`); test manifests (no card contents) |

## Build and test

In Git Bash:

1. `export PATH="$HOME/.cargo/bin:$PATH"`
2. `cargo test --workspace`. It must pass before any install.
3. `cargo build -p claude-kanban-dsh -p kb` for a dev build. A running app locks its exe.
4. Installs go through `scripts\install.ps1` (below), never by copying exes by hand.

- **macOS** (card #117, untested on real hardware, unsigned): `sh scripts/install.sh --channel test`
  installs the test app; `cargo check -p kanban-unix --target aarch64-apple-darwin` checks the Mac
  logic on Windows; the app crate compiles for macOS only on CI (the Mac jobs in `ci.yml` run only
  on `dsh-117` and manual runs). Put Mac-only code in `platform/macos.rs` or `crates/unix`.
- Test cards: dry-run cards (`dryRun: true`, repo set to this repo's `owner/name`, `effortOverride: "low"`),
  archived afterwards. Prefer making them on the test board.
- The API's requests are logged in the data folder's `api.log` (token hidden).

## Parallel builds

Several cards for this repo build at once (card #145), each in its own worktree with its own test
app, and they ship one at a time.

- **Its own worktree and branch.** Every card builds in `~\claude-kanban-dsh\.claude\worktrees\card-<n>`
  on the branch `card-<n>`, made from `origin/main`. For a repo listed in Settings › Sessions ›
  "Build each card in its own worktree" (`sessions.own_worktree_repos`), the app makes it when the
  card's first session starts and records `worktreePath` and `branch` (card #146); otherwise the
  card's session makes it in Planning. A stack's lead and children make their own (card #113).
- **Work only there.** Build, test and commit in the card's worktree: never in the main checkout
  `~\claude-kanban-dsh`, never in another card's worktree, never on `main`. Each worktree has its
  own `target` folder, so its first build is slow.
- **Its own test app.** `kanban_test_app` take gives the card a test app of its own (Test, Test 2,
  ... up to `testapp.slots`), kept on the card as `testApp` while it is In progress or Human test
  (card #147). Its reply names the install command (`-Channel test`, `test2`, ...) and the CDP
  port (9333 + N). Hugh tests each card in its own test app.
- **Ships go one at a time per repo** (card #79). At ship the repo's merge lane (card #160, for a
  repo in `sessions.merge_lane_repos`), or else its card-merger (card #100), rebases the branch
  onto the latest `origin/main` and reruns the tests; `main` is fast-forwarded to it and pushed.
  A lane only merges, in its own detached worktree: the card's session checks its commit
  (`git range-diff`), resets its branch to it and pushes. A stack's cards ship together, once, on the lead (card #113).
- **Private prompts too.** The live app reads the private folder's `prompts\` from its `main`, so a
  change there goes on a branch in a worktree of the private repo, and ships with the card.

## Test version and live

- **Test** ("Claude Kanban DSH Test"): the same code built with `KANBAN_CHANNEL=test`. Install in
  `%USERPROFILE%\Programs\Claude Kanban DSH Test`, data in `%USERPROFILE%\.claude-kanban-dsh-test`, build
  output in `target\test`, its own port, quit event, lock and WebView profile, a warning bar on the
  board, not on PATH (so `kb` is always the live one).
  - Install or refresh: `powershell -ExecutionPolicy Bypass -File scripts/install.ps1 -Channel test`
    (quits and restarts only the test app).
  - A test app per card, turns per test app (cards #115 and #147). A card session gets its own
    test app and a turn on it with `kanban_test_app` take, and gives the turn back with release
    (the test app stays the card's). Whoever installs from outside a card session takes an
    install turn on one test app first (`kb testapp take --install --slot <N> --owner <o>
    --label <l> --wait`), runs `install.ps1` with `KANBAN_LOCK_OWNER=<o>` in the background, and
    the turn ends when the install has started that test app again. A test app that holds a
    card's build refuses an outside install turn: free it first (`kb testapp free --slot <N>`).
    `install.ps1 -Channel <channel>` waits for other installs, a tester's turn on that test app,
    Hugh's Hold, 10 min after Hugh's last input in it, and while it holds another card's build.
  - `-FreshCopy` replaces the test board with a new copy of the live one (old one to its `backups`).
  - Test sessions are real Claude Code sessions on real repos: treat its cards with care.
- **More test slots** (card #118), the test apps the board gives cards (card #147): `-Channel test2`, `test3`, ... (up to
  `test10`; ports 9334 to 9343). Slot N is "Claude Kanban DSH Test N": install `Programs\Claude Kanban DSH Test N`, exe
  `claude-kanban-dsh-testN.exe`, data `.claude-kanban-dsh-testN`, build `target\testN`, identifier
  `io.github.hughht5.claude-kanban-dsh.testN`, quit event `Local\ClaudeKanbanDshQuitTestN`, its own lock, WebView profile
  and API port, "(test N)" in its titles. First install copies the live board; `-FreshCopy` works
  per slot. Its `kb.exe` (in its install folder) talks only to that slot. `test` is unchanged.
- A test app's card sessions reach only its own board: it sets `KANBAN_DATA_DIR` to its data
  folder (every `kb` and `kb mcp` reads it first, the repo's `.mcp.json` one too) and puts its
  install folder first on PATH. The live app sets neither.
- **Live**: `powershell -ExecutionPolicy Bypass -File scripts/install.ps1` with no `-Channel`, from
  the same commit you tested. It **stages** an update by default: the installer and `update.json`
  go to `%USERPROFILE%\.claude-kanban-dsh\update\`, the app shows "Update ready", and **Hugh clicks
  Restart now** when it suits him.
- **Never `-Now` on live**, and never restart, quit or `Stop-Process` the live app without Hugh's
  explicit say-so: it stops every running card session, including probably your own.
- For a change Hugh wants to test himself, put it in the test app and **don't stage live until he
  says so**.
- Never set `KANBAN_CHANNEL` for a live build (install.ps1 clears it and `CARGO_TARGET_DIR`).
- **On a Mac:** the same rules with `sh scripts/install.sh` (`--channel test`; no flags stages
  live; it refuses live `--now` from a card session). Apps in `~/Applications`, data in
  `~/.claude-kanban-dsh` and `~/.claude-kanban-dsh-test`, `kb` linked at `~/.local/bin/kb` (live only).

## Driving the UI

- Start the app with the DevTools port, always in the background:
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=N --remote-allow-origins=http://127.0.0.1:N'`
  then `Start-Process <exe> -ArgumentList '--background'`.
- Ports: **live 9333, test 9334, test 2 9335, test 3 9336** (slot N: 9333 + N). Use a test app
  unless the task is about live.
- A test app always has its port, however it is started: it is in its own window config
  (`src-tauri\tauri.test.conf.json`, card #101; `scripts\install.ps1` gives slot N its own port).
  Never restart it just to add the port.
- `node tools/cdp.mjs eval "<js>"` (9333 unless `CDP_PORT` is set) or
  `CDP_PORT=9334 node tools/cdp.mjs shot <png> [light|dark] [css selector]` (`9335` for test 2,
  and so on). `shot` refuses without `CDP_PORT`, hides the terminal and chat while it shoots, and
  crops to the selector.
- For the live app, relaunch without the variable afterwards. Restarting the live app to add the
  port needs Hugh's say-so like any other live restart.
- **On a Mac** there is no CDP and no UI driver (WKWebView has none): check through `kb` and the
  API. The test build has the Web Inspector (Safari > Develop) for Hugh.

## Hard rules

- **Never take Hugh's focus.** Every launch from a session uses `--background`. He often has a
  full-screen game in front. Never open links in his browser yourself.
- **Data is append-only.** No `DELETE FROM` anywhere (a test checks it); logs, revisions, sessions
  and session events take inserts only, and triggers refuse deletes. Archive, don't delete.
- **Never edit `board.db` directly** (live or test), never touch `%USERPROFILE%\.claude-kanban-dsh` by
  hand. `kb` and the tools go through the app; if the app isn't running, say so.
- Every migration takes a backup first and gets a test; an older build must still open a newer
  board.
- **No tokens, keys, passwords or sign-in links** on the board, in logs or in commits.
- **Never write `~\.claude.json` or any `settings.json`** (either account). Session settings live
  only in the app's per-session files.
- Never sign in to a cloud account as a user from a session. Don't change system security settings. Installs only from official
  sources, recorded in `INSTALLED.md` with their uninstall.
- **Leave the session cap** (`sessions.max_running`) as Hugh set it.
- **Only Hugh moves a card into Ship it, or the app for him when he set the card to go on by
  itself (Take it to), or turned on small-change auto-ship for that repo** (`autoTo`; the repo list
  `sessions.small_goes_on_repos`, card #104). Card sessions never merge or push to `main` unless
  their column prompt is Ship it.
- Sessions start only by Hugh's click or a column's auto-spawn; a card whose conversation is in the
  desktop app is never auto-started. One exception (PLAN.md, R1, card #113): Hugh's move of a
  stack's lead into Planning or In progress approves the app starting and resuming that stack's
  sessions (a child's start, a merge brief to the lead), still only in a column with auto-spawn.
  A second (card #160): a repo's merge lane starts, or resumes its last conversation, when a
  card's Ship it session asks it to merge (`kanban_merge`): Hugh's move into Ship it approved
  that ship. It is app-owned, not a card (`lane_sessions`), stops after 15 idle minutes, and never
  pushes, runs `kb`, installs, moves a branch or edits outside its worktree (`guard::lane_refusal`).
  Never start or stop a lane yourself; Hugh's Stop is in Settings › Sessions.
- **The Router (card #119)** is an app-owned session, not a card (`router_sessions`,
  `router_log`). Only Hugh's Start turns it on; the app resumes it after a restart while it is on.
  It starts card sessions only while Hugh's "Router may start cards" is on, and never ships,
  pushes, installs live or edits code (`guard::router_refusal`). Never start or stop it yourself.

## Working style

- **Make your own decisions and don't ask about them**, except the ones the brief or card says are
  Hugh's (scope, trade-offs it doesn't settle, anything touching live). Say what you decided.
- Plain British English, short sentences, no filler. End on what Hugh should do next.
- **Use subagents for heavy pieces** so Hugh can keep talking to the main session; the main session
  decides, integrates, commits and reports.
- **Write every change's undo into `UNDO.md`** as you make it (PowerShell 5.1 commands).
- **Try every change in the test version first**, then hand it to Hugh there.
- Tests for every data rule and migration, in `crates/store/tests/` or beside the code.
- Update `README.md` and `HANDOVER.md` when behaviour changes; `ACCEPTANCE.md` when a phase's tests
  change; `PLAN.md` for decisions.
- Every change to the app is pushed to GitHub, to the right repo: the app's code, tests and docs to
  this public repo (`hughht5/claude-kanban-dsh`, built in a worktree of `~\claude-kanban-dsh`, also
  for cards whose repo is the old `hughht5/kanban-app`); the owner's own material (column prompts,
  skills, `repos.json`) to the owner's private repo, never here.
  - In progress: commit on the card's own branch in its worktree, with `(card #<n>)` in the
    message, then push that branch (`git push -u origin card-<n>`, or a stack card's branch).
    Never push `main`.
  - Ship it, one card or one stack at a time per repo: the repo's merge lane, or its card-merger,
    rebases the branch onto `origin/main` and reruns the tests, `main` is fast-forwarded to it in the main checkout and
    pushed, the card's branch is deleted on GitHub, then the live update is staged
    (`scripts\install.ps1`, never `-Now`). The card's worktree is removed once its session has
    stopped (Windows won't remove a folder a running session is in).
  - The pre-commit check (`scripts\check-public.sh`) must pass before every commit here.
