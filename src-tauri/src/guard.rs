//! The shell guard (cards #100 and #96): what the app's subagents may not run in a shell.
//!
//! A deny rule like `Bash(kb:*)` in an agent's `disallowedTools` takes the whole tool away (the
//! live trial, 6 Nov 2026, Claude Code 2.1.318): with it the agent ran its commands through
//! PowerShell, and with the rule for both shells it had no shell at all. So the agents' deny
//! lists name whole tools only (`agents::denied`), and every card session sends each Bash and
//! PowerShell call to the app first (`sessions::settings_json`, a `PreToolUse` `http` hook to
//! `/hook/<token>/<row>/guard`). The hook's body names the subagent making the call
//! (`agent_type`), and `guard` refuses:
//!
//! - for every one of the app's subagents (`agents::AGENTS`): `kb` / `kb.exe` (it would act as
//!   the card's session) and `claude config` / `claude mcp` (Claude Code's own config);
//! - for the card-merger, also its pushes (`merger::push_in`): the session that ships pushes.
//!
//! The session's own calls, and other subagents', always run. A shell still runs anything in a
//! form this doesn't read (an alias, a script), so it is a guard rail; `configwatch.rs` still
//! compares the config around each run, and the merger's report still gives origin's SHAs. If
//! the app doesn't answer, Claude Code runs the command.

use crate::agents::own_card;
use crate::Board;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

/// The shell tools the guard sees, as `sessions::settings_json`'s matcher names them.
pub const SHELL_TOOLS: &str = "Bash|PowerShell";
/// Who its refusals are logged as.
pub const BY: &str = "the app (subagent shell guard)";

/// claude's options that take the next word as their value, so `claude --model x config` is
/// still `config`.
const CLAUDE_VALUED: [&str; 12] = [
    "--model",
    "--effort",
    "--settings",
    "--agents",
    "--mcp-config",
    "--permission-mode",
    "--resume",
    "-r",
    "--session-id",
    "--add-dir",
    "--output-format",
    "--append-system-prompt",
];

/// What board or config command a shell line runs, if any: `kb` (any of its commands, through
/// a full path to kb.exe too), `claude config` or `claude mcp`.
pub fn board_or_config_in(command: &str) -> Option<&'static str> {
    for words in commands(&command.to_ascii_lowercase()) {
        match invocation(&words) {
            Some(("kb", _)) => return Some("kb"),
            Some(("claude", rest)) => match subcommand(rest, &CLAUDE_VALUED) {
                Some((_, "config")) => return Some("claude config"),
                Some((_, "mcp")) => return Some("claude mcp"),
                _ => {}
            },
            _ => {}
        }
    }
    None
}

/// Programs that run the next word as the command (`env X=1 git`, `cmd /c kb`).
const WRAPPERS: [&str; 15] = ["env", "sudo", "exec", "time", "nohup", "command", "builtin", "call", "start", "cmd", "xargs", "sh", "bash", "pwsh", "powershell"];

/// The program a command runs, and its arguments: its first word that isn't an environment
/// setting (`X=1`), a wrapper (`env`, `cmd /c`, `sh`) or a wrapper's option. Only that word is a
/// program: `ls crates/kb` runs ls.
pub fn invocation(words: &[String]) -> Option<(&str, &[String])> {
    let mut i = 0;
    while i < words.len() {
        let w = words[i].as_str();
        let flag = w.starts_with('-') || (w.starts_with('/') && w.len() <= 3);
        let setting = !w.starts_with('-') && w.split_once('=').is_some_and(|(k, _)| !k.is_empty() && !k.contains(['/', '\\']));
        if setting || flag || WRAPPERS.contains(&program(w)) {
            i += 1;
            continue;
        }
        return Some((program(w), &words[i + 1..]));
    }
    None
}

/// A word as a program name: its last path part, without `.exe`.
pub fn program(word: &str) -> &str {
    word.rsplit(['/', '\\']).next().unwrap_or(word).trim_end_matches(".exe")
}

/// The first word that isn't an option (an option in `valued` takes the next word with it),
/// and where it is.
pub fn subcommand<'a>(words: &'a [String], valued: &[&str]) -> Option<(usize, &'a str)> {
    let mut i = 0;
    while i < words.len() {
        let w = words[i].as_str();
        if !w.starts_with('-') {
            return Some((i, w));
        }
        i += if valued.contains(&w) { 2 } else { 1 };
    }
    None
}

/// A shell line's commands, each as its words. Commands end at `;`, `&`, `|`, a bracket, a
/// backtick or a new line, outside quotes. A quoted word is one word, and is also read as a line
/// of its own when it holds more than one word (`sh -c "git push"`).
pub fn commands(line: &str) -> Vec<Vec<String>> {
    let (mut cmds, mut nested) = (Vec::new(), Vec::new());
    let (mut words, mut word, mut quote, mut quoted): (Vec<String>, String, Option<char>, bool) = (Vec::new(), String::new(), None, false);
    let mut end_word = |words: &mut Vec<String>, word: &mut String, quoted: &mut bool| {
        if !word.is_empty() || *quoted {
            if *quoted && word.trim().contains(char::is_whitespace) {
                nested.push(word.clone());
            }
            words.push(std::mem::take(word));
        }
        *quoted = false;
    };
    for c in line.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => word.push(c),
            None => match c {
                '"' | '\'' => {
                    quote = Some(c);
                    quoted = true;
                }
                ';' | '&' | '|' | '\n' | '\r' | '(' | ')' | '{' | '}' | '`' => {
                    end_word(&mut words, &mut word, &mut quoted);
                    if !words.is_empty() {
                        cmds.push(std::mem::take(&mut words));
                    }
                }
                c if c.is_whitespace() => end_word(&mut words, &mut word, &mut quoted),
                c => word.push(c),
            },
        }
    }
    end_word(&mut words, &mut word, &mut quoted);
    if !words.is_empty() {
        cmds.push(words);
    }
    for n in nested {
        cmds.extend(commands(&n));
    }
    cmds
}

/// Why a `PreToolUse` hook's call must be refused, if it must: (what it runs, why). Only an app
/// subagent's shell call is ever refused.
pub fn refusal(body: &Value) -> Option<(&'static str, String)> {
    let agent = crate::agents::find(body.get("agent_type").and_then(Value::as_str).unwrap_or(""))?;
    let tool = body.get("tool_name").and_then(Value::as_str).unwrap_or("");
    if !SHELL_TOOLS.split('|').any(|t| t == tool) {
        return None;
    }
    let cmd = body.pointer("/tool_input/command").and_then(Value::as_str).unwrap_or("");
    if let Some(what) = board_or_config_in(cmd) {
        let why = if what == "kb" {
            "it would act on the board as the card's session. Read the card with kanban_get_card; report what you found, and the session changes the card"
        } else {
            "Claude Code's own config is not yours to change"
        };
        return Some((what, format!("The {} subagent may not run {what}: {why}.", agent.name)));
    }
    if agent.name == crate::merger::MERGER {
        if let Some(what) = crate::merger::push_in(cmd) {
            return Some((what, format!("The card-merger may not run {what}: the session that ships the card pushes, after your PASS. Don't push; finish the merge and report.")));
        }
    }
    None
}

/// The tools the Router session's guard sees (card #119): the shells, and the editing tools,
/// which only its card-merger may use. A session-level deny would take them from its subagents
/// too (the trial on Claude Code 2.1.246, 6 Nov 2026), so they are refused here instead.
pub const ROUTER_TOOLS: &str = "Bash|PowerShell|Edit|Write|NotebookEdit|MultiEdit";
const EDIT_TOOLS: [&str; 4] = ["Edit", "Write", "NotebookEdit", "MultiEdit"];

/// What a shell line run in the Router's session (by it, or by any of its subagents) must never
/// do (card #119, finding 19): the board's CLI, Claude Code's config, a push or a PR merge, a
/// live restart (Stop-Process or taskkill of Claude Kanban DSH), the board's database, and an install
/// other than the test app's without `-Now` (none at all from a test-channel Router).
pub fn router_shell_refusal(command: &str, test_channel: bool) -> Option<&'static str> {
    if let Some(what) = board_or_config_in(command) {
        return Some(what);
    }
    if let Some(what) = crate::merger::push_in(command) {
        return Some(what);
    }
    let low = command.to_ascii_lowercase();
    for words in commands(&low) {
        let Some((prog, rest)) = invocation(&words) else { continue };
        let line = rest.join(" ");
        // The Router stops no process at all: any stop (by name, by id, from a pipe) could be the
        // live app, which would end every card session.
        if ["stop-process", "spps", "taskkill", "kill", "pkill", "killall", "tskill", "wmic"].contains(&prog) {
            return Some("a stop of a process");
        }
        if prog == "sqlite3" || ((prog == "python" || prog == "python3" || prog == "node") && line.contains("board.db")) {
            return Some("the board's database");
        }
        // Each install command on its own: the test app only, never -Now.
        if words.iter().any(|w| w.ends_with("install.ps1")) {
            if test_channel {
                return Some("an install (a test-channel Router never installs)");
            }
            let test = words.windows(2).any(|p| p[0] == "-channel" && p[1] == "test");
            if !test || words.iter().any(|w| w == "-now") {
                return Some("an install other than the test app's");
            }
        }
    }
    if low.contains("board.db") && (low.contains("sqlite") || low.contains("remove-item") || low.contains("del ") || low.contains("rm ")) {
        return Some("the board's database");
    }
    // An install the words above didn't see as one (inside a script block, say).
    if low.contains("install.ps1") && (test_channel || low.contains("-now") || !low.contains("-channel test")) {
        return Some("an install other than the test app's");
    }
    None
}

/// Why a call in the Router's session must be refused, if it must (card #119). `agent_type` is
/// set for a call by one of its subagents, empty for the Router's own.
pub fn router_refusal(body: &Value, test_channel: bool) -> Option<(&'static str, String)> {
    let agent = body.get("agent_type").and_then(Value::as_str).unwrap_or("");
    let tool = body.get("tool_name").and_then(Value::as_str).unwrap_or("");
    if EDIT_TOOLS.contains(&tool) {
        let merger = crate::agents::find(agent).is_some_and(|a| a.name == crate::merger::MERGER);
        if merger {
            return None;
        }
        return Some((
            "an edit",
            if agent.is_empty() {
                "The Router writes no code. Send the card back with the reason, or brief the card-merger for a merge conflict.".to_string()
            } else {
                format!("In the Router's session only the card-merger edits files, not the {agent} subagent.")
            },
        ));
    }
    if !SHELL_TOOLS.split('|').any(|t| t == tool) {
        return None;
    }
    let cmd = body.pointer("/tool_input/command").and_then(Value::as_str).unwrap_or("");
    let what = router_shell_refusal(cmd, test_channel)?;
    let who = if agent.is_empty() { "The Router".to_string() } else { format!("The {agent} subagent of the Router") };
    Some((what, format!("{who} may not run {what}. Use the kanban_* tools for the board; only Hugh pushes, ships, installs live or restarts the app.")))
}

/// The `guard` hook (`POST /hook/<token>/<session>/guard`, a `PreToolUse` for the shell tools
/// of every card session): `{}` lets the call run, a `deny` refuses it. A refusal is logged on
/// the session's card. Anything else takes no lock, so other calls go straight through.
pub fn guard(app: &AppHandle, row_id: &str, body: &Value) -> Value {
    if crate::routersession::is_router_row(app, row_id) {
        let Some((what, why)) = router_refusal(body, kanban_store::is_test()) else { return json!({}) };
        let cmd = shown(body.pointer("/tool_input/command").and_then(Value::as_str).unwrap_or(""), what);
        if let Ok(mut store) = app.state::<Board>().0.lock() {
            let _ = store.add_router_log("note", None, &format!("Refused {what}: {cmd}"), BY);
        }
        let _ = app.emit("router-changed", ());
        return deny(&why);
    }
    let Some((what, why)) = refusal(body) else { return json!({}) };
    let agent = body.get("agent_type").and_then(Value::as_str).unwrap_or("");
    let cmd = shown(body.pointer("/tool_input/command").and_then(Value::as_str).unwrap_or(""), what);
    if let Ok(mut store) = app.state::<Board>().0.lock() {
        if let Some(own) = own_card(app, &store, row_id) {
            let _ = store.add_log(&own.id, &format!("Refused {what} by the {agent} subagent: {cmd}"), BY);
        }
    }
    let _ = app.emit("cards-changed", ());
    deny(&why)
}

/// A refused command as the card's log shows it: its first 200 characters, or only what it was
/// when it may hold a secret (a push URL can carry a token: `https://<token>@github.com/...`).
pub fn shown(command: &str, what: &str) -> String {
    let cmd: String = command.chars().take(200).collect();
    if kanban_store::looks_secret(&cmd) || kanban_store::looks_like_token(&cmd) || cmd.contains('@') {
        format!("a {what} (not shown: it may hold a secret)")
    } else {
        cmd
    }
}

fn deny(why: &str) -> Value {
    json!({ "hookSpecificOutput": { "hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": why } })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(agent: Option<&str>, tool: &str, cmd: &str) -> Option<(&'static str, String)> {
        let mut b = json!({ "hook_event_name": "PreToolUse", "tool_name": tool, "tool_input": { "command": cmd } });
        if let Some(a) = agent {
            b["agent_type"] = json!(a);
        }
        refusal(&b)
    }

    #[test]
    fn every_app_subagent_is_refused_kb_and_claude_config() {
        for agent in crate::agents::AGENTS.iter().map(|a| a.name) {
            for (cmd, what) in [
                ("kb list", "kb"),
                ("kb.exe move 12 \"Ship it\"", "kb"),
                ("& \"C:\\Users\\x\\.cargo\\bin\\kb.exe\" update 5 --nextStep x", "kb"),
                ("cd x && kb log 3 hi", "kb"),
                ("sh -c \"kb send 4 go\"", "kb"),
                ("claude config set -g theme dark", "claude config"),
                ("claude.exe mcp add x", "claude mcp"),
                ("claude --model opus mcp list", "claude mcp"),
            ] {
                for tool in ["Bash", "PowerShell"] {
                    let (w, why) = call(Some(agent), tool, cmd).unwrap_or_else(|| panic!("{agent} may run {cmd} in {tool}"));
                    assert_eq!(w, what, "{cmd}");
                    assert!(why.starts_with(&format!("The {agent} subagent may not run {what}")), "{why}");
                }
            }
            // Its own work runs: reading code, the tests, and claude for anything else.
            for cmd in ["cargo test --workspace", "git log --grep kb", "grep -rn kb_exe src", "echo kb", "claude --version", "ls crates/kb", "git diff -- crates/kb/src/main.rs"] {
                assert!(call(Some(agent), "Bash", cmd).is_none(), "{agent} can't run {cmd}");
            }
        }
        // The session itself, other subagents, and other tools are never refused.
        assert!(call(None, "Bash", "kb move 3 \"Human test\"").is_none());
        assert!(call(Some("general-purpose"), "Bash", "kb list").is_none());
        assert!(call(Some("card-tester"), "Read", "kb list").is_none());
        assert!(call(Some("plugin:card-reviewer"), "PowerShell", "kb list").is_some());
    }

    #[test]
    fn the_program_is_the_first_real_word() {
        let prog = |line: &str| commands(&line.to_ascii_lowercase()).first().and_then(|w| invocation(w).map(|(p, _)| p.to_string()));
        assert_eq!(prog("kb list").as_deref(), Some("kb"));
        assert_eq!(prog("ls crates/kb").as_deref(), Some("ls"));
        assert_eq!(prog("cmd /c kb list").as_deref(), Some("kb"));
        assert_eq!(prog("env KANBAN_CARD=3 kb list").as_deref(), Some("kb"));
        assert_eq!(prog("GIT_TRACE=1 git push").as_deref(), Some("git"));
        assert_eq!(prog("C:/Users/x/bin/kb.exe card 4").as_deref(), Some("kb"));
        assert_eq!(prog("xargs -n1 kb log 3").as_deref(), Some("kb"));
        assert_eq!(prog("/usr/bin/git status").as_deref(), Some("git"));
        assert_eq!(prog("cargo test -p kb").as_deref(), Some("cargo"));
        assert_eq!(prog("").as_deref(), None);
    }

    #[test]
    fn only_the_mergers_shell_pushes_are_refused() {
        let (what, why) = call(Some("card-merger"), "Bash", "git push origin main").unwrap();
        assert_eq!(what, "git push");
        assert!(why.contains("Don't push"), "{why}");
        assert!(call(Some("card-merger"), "PowerShell", "git -C C:\\w push").is_some());
        assert!(call(Some("plugin:card-merger"), "PowerShell", "gh api user").is_some());
        // Not a push, not the merger, or not a shell: it runs.
        assert!(call(Some("card-merger"), "Bash", "git rebase origin/main").is_none());
        assert!(call(None, "Bash", "git push origin main").is_none(), "the session itself pushes");
        assert!(call(Some("card-tester"), "Bash", "git push").is_none());
        assert!(call(Some("general-purpose"), "Bash", "git push").is_none());
        assert!(call(Some("card-merger"), "Read", "git push").is_none());
        assert_eq!(deny("x")["hookSpecificOutput"]["permissionDecision"], "deny");
    }

    #[test]
    fn a_refused_command_never_puts_a_secret_on_the_card() {
        assert_eq!(shown("git push origin dsh-100", "git push"), "git push origin dsh-100");
        for cmd in ["git push https://ghp_abcdefghijklmnopqrstuvwxyz0123456789@github.com/hughht5/kanban-app.git main", "git push https://x-access-token:abc@github.com/o/r HEAD"] {
            assert_eq!(shown(cmd, "git push"), "a git push (not shown: it may hold a secret)", "{cmd}");
        }
        assert_eq!(shown(&"git push ".repeat(100), "git push").chars().count(), 200);
    }

    fn router(agent: &str, tool: &str, input: serde_json::Value, test: bool) -> Option<(&'static str, String)> {
        router_refusal(&json!({ "agent_type": agent, "tool_name": tool, "tool_input": input }), test)
    }

    #[test]
    fn the_router_writes_no_code_but_its_merger_may() {
        for t in ["Edit", "Write", "NotebookEdit", "MultiEdit"] {
            assert!(router("", t, json!({ "file_path": "x" }), false).is_some(), "{t} by the Router");
            assert!(router("card-tester", t, json!({ "file_path": "x" }), false).is_some(), "{t} by its tester");
            assert!(router("card-merger", t, json!({ "file_path": "x" }), false).is_none(), "{t} by its merger");
        }
        assert!(router("", "Read", json!({ "file_path": "x" }), false).is_none());
    }

    #[test]
    fn the_routers_shell_guard_refuses_each_form() {
        let refused = [
            "kb list",
            "\"C:/Users/h/Programs/Claude Kanban DSH/kb.exe\" move 3 Done",
            "claude config set x",
            "git push origin main",
            "git -C ../x push",
            "gh pr merge 12",
            "Stop-Process -Name 'Claude Kanban DSH'",
            "taskkill /IM \"claude-kanban-dsh.exe\" /F",
            "sqlite3 C:/Users/h/.claude-kanban-dsh/board.db .tables",
            "python -c \"import sqlite3; sqlite3.connect('board.db')\"",
            "powershell -ExecutionPolicy Bypass -File scripts/install.ps1",
            "powershell -File scripts/install.ps1 -Channel test -Now",
            "powershell -File scripts/install.ps1 -Now",
            "Get-Process 'Claude Kanban DSH' | Stop-Process -Force",
            "Stop-Process -Id 1234",
            "powershell -File scripts/install.ps1 -Channel test; powershell -File scripts/install.ps1",
        ];
        for c in refused {
            assert!(router("", "PowerShell", json!({ "command": c }), false).is_some(), "refused: {c}");
            assert!(router("card-merger", "Bash", json!({ "command": c }), false).is_some(), "refused for its merger too: {c}");
        }
        let allowed = ["git status", "git log --oneline -5", "cargo test --workspace -j 4", "powershell -ExecutionPolicy Bypass -File scripts/install.ps1 -Channel test", "Get-Process", "git commit -m wip"];
        for c in allowed {
            assert!(router("", "PowerShell", json!({ "command": c }), false).is_none(), "runs: {c}");
        }
        // A test-channel Router never installs.
        assert!(router("", "PowerShell", json!({ "command": "powershell -File scripts/install.ps1 -Channel test" }), true).is_some());
    }

    #[test]
    fn the_router_guard_matcher_covers_shells_and_edits() {
        for t in ["Bash", "PowerShell", "Edit", "Write", "NotebookEdit"] {
            assert!(ROUTER_TOOLS.split('|').any(|x| x == t), "{t}");
        }
    }
}
