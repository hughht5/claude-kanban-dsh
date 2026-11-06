# Working on Claude Kanban DSH

Standing instructions for any session (a card session, a subagent, or Hugh's own) working in this
repo. Distilled from `HANDOVER.md`, `README.md`, `UNDO.md`, `ACCEPTANCE.md`, `PLAN.md` and
`scripts\install.ps1`; read those for detail. A card session for this repo also follows
`{prompts}\kanban-app.md`, which overrides the parts of the column prompts written for other repos.

**Contents**

- [What the app is](#what-the-app-is)
- [Layout](#layout)
- [Build and test](#build-and-test)
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

- Test cards: dry-run cards (`dryRun: true`, repo set to this repo's `owner/name`, `effortOverride: "low"`),
  archived afterwards. Prefer making them on the test board.
- The API's requests are logged in the data folder's `api.log` (token hidden).

## Test version and live

- **Test** ("Claude Kanban DSH Test"): the same code built with `KANBAN_CHANNEL=test`. Install in
  `%USERPROFILE%\Programs\Claude Kanban DSH Test`, data in `%USERPROFILE%\.claude-kanban-dsh-test`, build
  output in `target\test`, its own port, quit event, lock and WebView profile, a warning bar on the
  board, not on PATH (so `kb` is always the live one).
  - Install or refresh: `powershell -ExecutionPolicy Bypass -File scripts/install.ps1 -Channel test`
    (quits and restarts only the test app).
  - `-FreshCopy` replaces the test board with a new copy of the live one (old one to its `backups`).
  - Test sessions are real Claude Code sessions on real repos: treat its cards with care.
- **Live**: `powershell -ExecutionPolicy Bypass -File scripts/install.ps1` with no `-Channel`, from
  the same commit you tested. It **stages** an update by default: the installer and `update.json`
  go to `%USERPROFILE%\.claude-kanban-dsh\update\`, the app shows "Update ready", and **Hugh clicks
  Restart now** when it suits him.
- **Never `-Now` on live**, and never restart, quit or `Stop-Process` the live app without Hugh's
  explicit say-so: it stops every running card session, including probably your own.
- For a change Hugh wants to test himself, put it in the test app and **don't stage live until he
  says so**.
- Never set `KANBAN_CHANNEL` for a live build (install.ps1 clears it and `CARGO_TARGET_DIR`).

## Driving the UI

- Start the app with the DevTools port, always in the background:
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=N --remote-allow-origins=http://127.0.0.1:N'`
  then `Start-Process <exe> -ArgumentList '--background'`.
- Ports: **live 9333, test 9334**. Use the test app unless the task is about live.
- `node tools/cdp.mjs eval "<js>"` or `node tools/cdp.mjs shot <png> [dark]`; set `CDP_PORT=9334`
  for the test app.
- Relaunch without the variable afterwards. Restarting the live app to add the port needs Hugh's
  say-so like any other live restart.

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
- **Only Hugh moves a card into Ship it**, or the app for him when he set the card to go on by
  itself to Deploy complete ("Take it to", `autoTo`). Card sessions never merge or push to `main` unless their
  column prompt is Ship it.
- Sessions start only by Hugh's click or a column's auto-spawn; a card whose conversation is in the
  desktop app is never auto-started.

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
- Commit locally on `main` with a clear message; push only when the brief or column prompt says so.
