# Claude Kanban DSH

**Run a dozen Claude Code sessions without losing track of any of them.**

Claude Kanban DSH is a desktop kanban board where every card is a live Claude Code session. Drag a card into Planning and its session plans the work. Move it to In progress and it builds. When it's ready for you, it lands in Human test with the steps to check it, and it ships only when you say so.

Most people lose track at about three to five Claude Code sessions: which one is waiting for an answer, which has stalled, which is done. The board keeps track for you. One look tells you what each card is doing, what it needs from you, and what's ready to ship.

It runs on your own Claude subscription through the Claude Code CLI on your machine. There are no API keys and no extra bill, and your code stays local.

> **Status:** a working personal tool, used daily, and still changing fast. It's Windows-only for now, and you build it from source.

---

## What it does

- **Every card is a session.** Each card owns a real Claude Code session in an embedded terminal, with a chat view beside it. Open a card to talk to it, watch it work, or take over its terminal.
- **Columns drive the work.** Moving a card sends its session that column's instructions: plan in Planning, build in In progress, hand over in Human test, merge and release in Ship it. The instructions are plain Markdown files you can edit.
- **You stay in charge of shipping.** Sessions build and test on their own, but only you move a card into Ship it, unless you've set that card to go on by itself.
- **Tested before it reaches you.** Every card is tried for real by a separate tester subagent before it lands in Human test. If it fails, the session fixes it and tests again.
- **Questions come to you.** When a session needs a decision, the card shows the question and the answers to pick from, so you can reply from the board.
- **Stacks:** group cards that ship together, then test them together and ship them as one.
- **Resume after a restart.** Quitting suspends every session; the next start resumes the ones that were running.
- **Drive it from anywhere.** `kb`, the command-line tool, plus an MCP server, lets any Claude session create, move and message cards.
- **A separate test copy.** A second build, Claude Kanban DSH Test, runs beside your real board with its own data, for trying changes safely.
- **Keeps a full history.** Card history, logs and session events are append-only: nothing is ever deleted, only archived.

### Coming next

Ideas from [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness), rebuilt on Claude Code:

- **A team of helper agents** on every card: a **router** that sizes and splits work, a **critic** that attacks the plan before anything is built, a **reviewer** that reads the diff, a **reproducer** that proves bugs and fixes, a **merger** for clean ships, and a **monitor** that spots stuck sessions and nudges them.
- **Faster review:** before-and-after screenshots on every hand-over, Previous and Next through cards waiting for you, and a "30-second look" label for small changes.
- **Cards you can read at a glance:** "Done when…" with acceptance checks, step progress, what changed, and what each card is waiting on you for.
- **Ordering and history:** "Blocked by #12", search across every session, and time spent in each column.
- **Stacks that build in parallel**, each card in its own session and branch.

---

## Requirements

- Windows 10 or 11 with the WebView2 Runtime (built into Windows 11)
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code), signed in to your Claude account
- To build: [Rust](https://rustup.rs) (stable, `x86_64-pc-windows-msvc`), Visual Studio Build Tools 2022 with the "Desktop development with C++" workload, Node.js, Git, and optionally the GitHub CLI (`gh`)

## Install

```powershell
git clone https://github.com/hughht5/claude-kanban-dsh
cd claude-kanban-dsh
powershell -ExecutionPolicy Bypass -File scripts\install.ps1
```

It installs for your user only and needs no admin rights. The app goes in `%USERPROFILE%\Programs\Claude Kanban DSH`, your board in `%USERPROFILE%\.claude-kanban-dsh`, and `kb` on your PATH.

To install the test copy beside it:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Channel test
```

## Quick start

1. Open **Claude Kanban DSH** from the Start menu.
2. In **Settings › Sessions**, add your repositories (or point **Private folder** at a folder with your own `repos.json` and column prompts).
3. Click **Add card**, describe the work, pick a repo, and drop it into **Planning**. Its session starts and writes a plan.
4. Read the plan on the card. Move it to **In progress** to build.
5. When it reaches **Human test**, follow the steps on the card. Move it to **Ship it** when you're happy.

From any terminal or Claude session:

```text
kb list                      the board
kb create --title "Fix the login redirect" --repo you/your-app --column Planning
kb card 12                   one card as JSON
kb send 12 "Use the v2 endpoint instead"
kb move 12 "In progress"
```

Add `kb mcp` to Claude Code as an MCP server, and the `kanban_*` tools let any session manage the board.

## Make it yours

- **Column prompts:** each column's instructions are Markdown files. The repo ships generic examples in `prompts\`. Put your own in a private folder (**Settings › Sessions › Private folder**) and the app uses those instead.
- **Repos:** list them in `<private folder>\repos.json`, so your own projects never land in this repo.
- **Templates:** card templates set a starting prompt for a kind of work, such as a bug fix.

## How it's built

| Folder | What |
|---|---|
| `src-tauri/` | The Tauri 2 app: sessions, dispatch, the localhost API (MCP, REST, hooks), updates |
| `crates/store/` | The SQLite store and its migrations |
| `crates/kb/` | `kb.exe`, the CLI and MCP bridge |
| `ui/` | The board and card panel: plain JavaScript, no bundler |
| `prompts/` | Example column prompts |
| `scripts/` | `install.ps1`, the public-repo check and its pre-commit hook |
| `tools/` | `cdp.mjs`, which drives the app's WebView for testing |

```powershell
cargo test --workspace
cargo build -p claude-kanban-dsh -p kb
```

The full guide to every feature is in [docs/GUIDE.md](docs/GUIDE.md). The design notes are in [PLAN.md](PLAN.md), and each change's undo is in [UNDO.md](UNDO.md).

## Safety

- Sessions run Claude Code on your machine, in your repos, with the permissions you give them. Read the column prompts before you use them on code that matters.
- Nothing pushes to your main branch or deploys outside **Ship it**.
- Your board and logs stay in your user folder. The app talks to nothing but Claude Code and, if your prompts use them, `git` and `gh`.

## Contributing

Issues and pull requests are welcome. Run `cargo test --workspace` before you send one, and turn on the pre-commit check with `git config core.hooksPath scripts/hooks`.

## Credits

Built by [Hugh Halford-Thompson](https://github.com/hughht5) with Claude Code. The helper-agent ideas come from DeepSeek's [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) and AI Labs' Claude Code "super agent" workflow.

Not affiliated with or endorsed by Anthropic or DeepSeek. Claude and Claude Code are trademarks of Anthropic.

## Licence

[MIT](LICENSE)
