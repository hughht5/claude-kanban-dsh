//! The chat's slash-command menu (card #94): Claude Code's own commands, and the skills and
//! custom commands the card's session can see (the account's, the project's and its enabled
//! plugins'), as the terminal's `/` menu lists them.

use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Command {
    /// Without the slash: "compact", "kanban", "claude-obsidian:wiki".
    pub name: String,
    pub description: String,
    /// "btw" (answered in the chat), "chat" (typed in as in the terminal: what it prints shows
    /// in the chat, and a screen it opens opens the terminal beside the chat), "app" (not from
    /// the chat: Stop session instead), "skill" or "command".
    pub kind: &'static str,
    /// Where it comes from, for a skill or command: "user", "project" or the plugin's name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Claude Code's commands that only its terminal menu lists (2.1.318, read off that menu on
/// 5 Nov 2026), with its descriptions. The rest (its other built-ins, skills, plugins) come from
/// Claude Code itself (`claude_list`). Which of them open a screen of their own isn't listed: it
/// changes between versions (`/usage` opens the Usage settings), so the app reads the screen
/// (`Live::dialog`). `/btw` is answered in the chat; `/exit` and `/quit` aren't taken from it.
const BUILTIN: &[(&str, &str, &str)] = &[
    ("add-dir", "chat", "Add a new working directory"),
    ("advisor", "chat", "Let Claude consult a stronger model at key moments"),
    ("artifact-capabilities", "chat", "Runtime capabilities for published Artifacts"),
    ("artifact-diagramming", "chat", "Diagramming guidance for Artifacts"),
    ("artifacts", "chat", "Browse your published and shared artifacts"),
    ("auto-mode-setup", "chat", "Teach auto mode about your environment, plus optional rule tweaks"),
    ("autocompact", "chat", "Set how full the context gets before auto-summarizing"),
    ("autofix-pr", "chat", "Monitor and autofix any issues with the current PR"),
    ("background", "chat", "Send this session to the background and free the terminal"),
    ("branch", "chat", "Create a branch of the current conversation at this point"),
    ("btw", "btw", "Ask a quick side question without interrupting the main conversation"),
    ("bug", "chat", "Report a bug or share your conversation"),
    ("cd", "chat", "Move this session to a new working directory"),
    ("chrome", "chat", "Open Claude in Chrome settings"),
    ("clear", "chat", "Start a new session with empty context; previous session stays on…"),
    ("color", "chat", "Set the prompt bar color for this session"),
    ("compact", "chat", "Free up context by summarizing the conversation so far"),
    ("config", "chat", "Open settings"),
    ("context", "chat", "Visualize current context usage as a colored grid"),
    ("copy", "chat", "Copy Claude's last response to clipboard (or /copy N for the…"),
    ("design-login", "chat", "Authorize design-system access for /design-sync with your claude.ai"),
    ("desktop", "chat", "Continue the current session in Claude Desktop"),
    ("diff", "chat", "Toggle the diff panel showing uncommitted changes"),
    ("effort", "chat", "Set effort level for model usage"),
    ("exit", "app", "Not from the chat: use Stop session, which the app can resume"),
    ("export", "chat", "Export the current conversation to a file or clipboard"),
    ("fast", "chat", "Toggle fast mode (Opus 5.5)"),
    ("feedback", "chat", "Send feedback to Anthropic or report a bug"),
    ("focus", "chat", "Toggle focus view: just your prompt, summary, and response"),
    ("fork", "chat", "Copy this conversation into a new background session and keep working"),
    ("goal", "chat", "Set a goal Claude checks before stopping"),
    ("help", "chat", "Show help and available commands"),
    ("hooks", "chat", "View hook configurations for tool events"),
    ("ide", "chat", "Manage IDE integrations and show status"),
    ("import", "chat", "Import config from another AI coding agent"),
    ("init", "chat", "Initialize a new CLAUDE.md file with codebase documentation"),
    ("insights", "chat", "Generate a report analyzing your Claude Code sessions"),
    ("install-github-app", "chat", "Set up Claude GitHub Actions for a repository"),
    ("install-slack-app", "chat", "Install the Claude Slack app"),
    ("keybindings", "chat", "Open your keyboard shortcuts file"),
    ("list-agents", "chat", "List subagents, teammates, and other Claude sessions you can message"),
    ("login", "chat", "Sign in with your Anthropic account"),
    ("logout", "chat", "Sign out from your Anthropic account"),
    ("mcp", "chat", "Manage MCP servers"),
    ("memory", "chat", "Edit CLAUDE.md files and memory settings"),
    ("mobile", "chat", "Show QR code to download the Claude mobile app"),
    ("model", "chat", "Set the AI model for Claude Code (currently Opus 5.5)"),
    ("output-style", "chat", "List output styles or switch to one"),
    ("passes", "chat", "Share a free week of Claude Code with friends and earn usage credits"),
    ("permissions", "chat", "Manage allow and deny tool permission rules"),
    ("plan", "chat", "Enable plan mode or view the current session plan"),
    ("plugin", "chat", "Manage Claude Code plugins"),
    ("powerup", "chat", "Discover Claude Code features through quick interactive lessons"),
    ("privacy-settings", "chat", "View and update your privacy settings"),
    ("radio", "chat", "Listen to Claude FM lo-fi radio"),
    ("rate-limit-options", "chat", "Manage usage limits and upgrade options"),
    ("recap", "chat", "Generate a one-line session recap now"),
    ("release-notes", "chat", "View release notes"),
    ("reload-plugins", "chat", "Activate pending plugin changes in the current session"),
    ("reload-skills", "chat", "Pick up skills added or changed on disk during this session"),
    ("remote-control", "chat", "Control this session from your phone or claude.ai/code"),
    ("remote-env", "chat", "Choose the default environment for cloud agents"),
    ("rename", "chat", "Rename the current conversation"),
    ("resume", "chat", "Resume a previous conversation"),
    ("rewind", "chat", "Restore the code and/or conversation to a previous point"),
    ("scroll-speed", "chat", "Adjust mouse wheel scroll speed"),
    ("security-review", "chat", "Complete a security review of the pending changes on the current…"),
    ("skill-doctor", "chat", "Show which loaded skills are unused and costing context"),
    ("skills", "chat", "List available skills"),
    ("slides", "chat", "Make a new Slides deck artifact from a brief"),
    ("status", "chat", "Show Claude Code status including version, model, account, API"),
    ("statusline", "chat", "Set up Claude Code's status line UI"),
    ("stickers", "chat", "Order Claude Code stickers"),
    ("subtask", "chat", "Send a subagent off with your full context; its result comes back…"),
    ("tasks", "chat", "View and manage everything running in the background"),
    ("team-onboarding", "chat", "Help teammates ramp on Claude Code with a guide from your usage"),
    ("teleport", "chat", "Send this session to the cloud, or resume one from claude.ai"),
    ("terminal-setup", "chat", "Install Shift+Enter key binding for newlines"),
    ("theme", "chat", "Change the theme"),
    ("tui", "chat", "Set the terminal UI renderer (default | fullscreen)"),
    ("ultrareview", "chat", "Start a cloud agent that finds and verifies bugs in…"),
    ("upgrade", "chat", "Upgrade your plan for higher rate limits and more Opus"),
    ("usage", "chat", "Show session cost, plan usage, and activity stats"),
    ("usage-credits", "chat", "Configure usage credits or request them from your admin when you hit…"),
    ("voice", "chat", "Toggle voice mode"),
    ("web-setup", "chat", "Set up cloud sessions with your GitHub account"),
    ("workflows", "chat", "Browse running and completed workflows"),
    ("quit", "app", "Not from the chat: use Stop session, which the app can resume"),
];

/// The terminal menu's descriptions of the skills Claude Code brings itself (2.1.318), which
/// have no file on disk to describe them.
const SKILL_DESC: &[(&str, &str)] = &[
    ("batch", "Plan a large change; background agents each open a PR"),
    ("built-in-browser", "Read this skill before the first step that uses the built-in browser,"),
    ("chrome-browser", "Read this skill before the first step that uses Claude in Chrome, the"),
    ("claude-api", "Build and debug apps that use the Claude API"),
    ("code-review", "3 free /ultrareview · Review the current diff or a PR for bugs and"),
    ("computer-use", "Read this skill before the first step of any request to do something…"),
    ("dataviz", "Chart and dashboard design guidance"),
    ("debug", "Turn on debug logging and investigate problems"),
    ("deep-research", "[dynamic workflow] Deep research harness — fan-out web searches,…"),
    ("design", "Make a new Design artifact from a brief"),
    ("design-sync", "Push your design system components to claude.ai/design"),
    ("docs", "docs (editable docs people share and comment on; the default for any"),
    ("doctor", "Health-check your setup and fix issues: installation, unused…"),
    ("docx", "Use this skill whenever the user wants to create, read, edit, or"),
    ("fewer-permission-prompts", "Pre-approve safe read-only commands based on your usage"),
    ("google-workspace", "Read this before the first Google Drive, Docs, Sheets or Slides"),
    ("import-memory", "Import a memory export from another AI assistant into Claude's memory…"),
    ("loop", "Repeat a prompt or command on an interval (e.g. /loop 5m /foo)"),
    ("morning", "Render the user's morning brief as a styled HTML artifact, or set it…"),
    ("pdf", "Use this skill whenever the user wants to do anything with PDF files."),
    ("plugin-authoring", "Write a mod: a live pane, view or behaviour change in Claude Code"),
    ("pptx", "Use this skill any time a .pptx or .potx file is involved in any way…"),
    ("run", "Launch this project’s app to see your change working"),
    ("run-skill-generator", "Create a skill that knows how to run this project’s app"),
    ("schedule", "Create and manage routines: cloud agents on a schedule"),
    ("simplify", "Clean up the changed code without changing behavior"),
    ("skill-creator", "Create new skills, modify and improve existing skills, and measure…"),
    ("update-config", "Change settings: hooks, permissions, environment variables"),
    ("verify", "Try your change and report if it works"),
    ("workflow-authoring", "Load the reference for writing Workflow tool scripts"),
    ("xlsx", "Use this skill any time a spreadsheet file is the primary input or"),
];

/// The built-in commands.
pub fn builtins() -> Vec<Command> {
    BUILTIN
        .iter()
        .map(|(n, k, d)| Command { name: n.to_string(), description: d.to_string(), kind: k, source: None })
        .collect()
}

/// The `name`, `description` and `user-invocable` of a SKILL.md or command file's front
/// matter: `key: value` lines, and a value YAML folds over the indented lines after it
/// (`description: >-`).
pub fn front_matter(text: &str) -> (Option<String>, Option<String>, bool) {
    let mut lines = text.lines().peekable();
    if lines.next().map(str::trim) != Some("---") {
        return (None, None, true);
    }
    let (mut name, mut desc, mut invocable) = (None, None, true);
    while let Some(l) = lines.next() {
        if l.trim() == "---" {
            break;
        }
        if l.starts_with(char::is_whitespace) {
            continue;
        }
        let Some((k, v)) = l.split_once(':') else { continue };
        let mut v = v.trim().trim_matches('"').trim_matches('\'').to_string();
        if matches!(v.as_str(), ">" | ">-" | ">+" | "|" | "|-" | "|+") {
            let mut parts = vec![];
            while let Some(n) = lines.peek().filter(|n| n.starts_with(char::is_whitespace) || n.trim().is_empty()) {
                parts.push(n.trim().to_string());
                lines.next();
            }
            v = parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ");
        }
        match k.trim() {
            "name" if !v.is_empty() => name = Some(v),
            "description" if !v.is_empty() => desc = Some(v),
            "user-invocable" => invocable = v != "false",
            _ => {}
        }
    }
    (name, desc, invocable)
}

/// The first line of a command file's body, for a description when it has none.
fn first_line(text: &str) -> String {
    let body = if text.starts_with("---") { text.splitn(3, "---").nth(2).unwrap_or("") } else { text };
    body.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").trim_start_matches('#').trim().to_string()
}

/// The skills in `dir` (`<dir>/<skill>/SKILL.md`), named `prefix` + name.
fn skills_in(dir: &Path, prefix: &str, source: &str, out: &mut Vec<Command>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(text) = std::fs::read_to_string(e.path().join("SKILL.md")) else { continue };
        let (name, desc, invocable) = front_matter(&text);
        if !invocable {
            continue;
        }
        let name = name.unwrap_or_else(|| e.file_name().to_string_lossy().to_string());
        out.push(Command { name: format!("{prefix}{name}"), description: desc.unwrap_or_default(), kind: "skill", source: Some(source.into()) });
    }
}

/// The custom commands in `dir` (`<dir>/**/<name>.md`; a subfolder makes `sub:name`).
fn commands_in(dir: &Path, prefix: &str, source: &str, out: &mut Vec<Command>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if p.is_dir() {
            commands_in(&p, &format!("{prefix}{stem}:"), source, out);
        } else if p.extension().is_some_and(|x| x == "md") {
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            let (_, desc, _) = front_matter(&text);
            out.push(Command { name: format!("{prefix}{stem}"), description: desc.unwrap_or_else(|| first_line(&text)), kind: "command", source: Some(source.into()) });
        }
    }
}

/// The newest version folder of a cached plugin (`<cache>/<market>/<plugin>/<version>`).
fn newest_version(plugin_dir: &Path) -> Option<std::path::PathBuf> {
    std::fs::read_dir(plugin_dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
}

/// What Claude Code itself says a session has: its `slash_commands` and which are `skills`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClaudeList {
    pub commands: Vec<String>,
    pub skills: Vec<String>,
}

/// Commands Claude Code reports that its terminal menu doesn't show (2.1.318).
const HIDDEN: &[&str] = &["agents", "heapdump", "workflow-launch-exec", "extra-usage", "design-consent", "design-revoke"];

/// Reads the list from the first line Claude Code prints in print mode (`system`/`init`).
pub fn parse_init(line: &str) -> Option<ClaudeList> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("type")?.as_str()? != "system" || v.get("subtype")?.as_str()? != "init" {
        return None;
    }
    let strs = |k: &str| v.get(k).and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    Some(ClaudeList { commands: strs("slash_commands"), skills: strs("skills") })
}

/// Asks Claude Code for the session's commands: starts `exe` in print mode in `cwd` with `env`
/// (the account's), reads its `init` line and kills it at once, before its first request goes
/// out (if one did, it is on the cheapest model, with hooks off and nothing saved). About two
/// seconds; None if it fails or takes over 30 s.
pub fn claude_list(exe: &Path, env: &[(String, String)], cwd: Option<&Path>) -> Option<ClaudeList> {
    use crate::platform::NoWindow;
    use std::io::BufRead;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["-p", "hi", "--output-format", "stream-json", "--verbose", "--no-session-persistence", "--model", "claude-haiku-5-5", "--max-turns", "1", "--settings", r#"{"disableAllHooks":true}"#])
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .no_window();
    if let Some(c) = cwd.filter(|c| c.is_dir()) {
        cmd.current_dir(c);
    }
    let mut child = cmd.spawn().ok()?;
    let out = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(l) = parse_init(&line) {
                let _ = tx.send(l);
                return;
            }
        }
    });
    let got = rx.recv_timeout(std::time::Duration::from_secs(30)).ok();
    let _ = child.kill();
    let _ = child.wait();
    got
}

/// The menu from Claude Code's own list (`claude_list`): the terminal-only built-ins, then each
/// command it reports as its terminal menu names it (an Anthropic skill without its
/// "anthropic-skills:", no MCP prompts or hidden commands), described from the built-ins or
/// from `disk` (the skills and commands found on disk, `list`).
pub fn with_claude(claude: &ClaudeList, disk: &[Command]) -> Vec<Command> {
    let mut out = builtins();
    for raw in &claude.commands {
        if raw.starts_with("mcp__") || raw.starts_with("__") || HIDDEN.contains(&raw.as_str()) {
            continue;
        }
        // Shown without its prefix unless another command already has that name.
        let name = raw.strip_prefix("anthropic-skills:").filter(|b| !claude.commands.iter().any(|c| c == b)).unwrap_or(raw);
        if out.iter().any(|c| c.name == name) {
            continue;
        }
        let found = disk.iter().find(|c| c.name == name || c.name == *raw);
        let skill = claude.skills.contains(raw);
        out.push(Command {
            name: name.to_string(),
            description: found.map(|c| c.description.clone()).or_else(|| {
                    let bare = raw.strip_prefix("anthropic-skills:").unwrap_or(name);
                    SKILL_DESC.iter().find(|(n, _)| *n == name || *n == bare).map(|(_, d)| d.to_string())
                }).unwrap_or_default(),
            kind: if skill { "skill" } else { found.map_or("chat", |c| c.kind) },
            source: found.and_then(|c| c.source.clone()),
        });
    }
    out
}

/// Everything the menu lists for a session with Claude Code's folder `config` (the account's
/// `.claude`) in the project `cwd`, read from disk: the built-ins, then the project's, the
/// account's and the enabled plugins' skills and commands. The first of a name wins, as in
/// Claude Code. The app uses Claude Code's own list when it has it (`with_claude`).
pub fn list(config: &Path, cwd: Option<&Path>) -> Vec<Command> {
    let mut out = builtins();
    // No folder known (a card that never ran and has no worktree): no project commands, rather
    // than the app's own working folder's.
    if let Some(cwd) = cwd.filter(|c| c.is_absolute()) {
        let project = cwd.join(".claude");
        skills_in(&project.join("skills"), "", "project", &mut out);
        commands_in(&project.join("commands"), "", "project", &mut out);
    }
    skills_in(&config.join("skills"), "", "user", &mut out);
    commands_in(&config.join("commands"), "", "user", &mut out);
    let enabled: Vec<String> = std::fs::read_to_string(config.join("settings.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("enabledPlugins").and_then(|p| p.as_object()).map(|o| o.iter().filter(|(_, on)| on.as_bool() == Some(true)).map(|(k, _)| k.clone()).collect()))
        .unwrap_or_default();
    for key in enabled {
        let Some((plugin, market)) = key.split_once('@') else { continue };
        let Some(root) = newest_version(&config.join("plugins").join("cache").join(market).join(plugin)) else { continue };
        // Claude Code names a plugin's skills by its plugin.json name, which can differ from
        // the key it was installed under ("neon-postgres@neon" is "neon").
        let plugin = std::fs::read_to_string(root.join(".claude-plugin").join("plugin.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string))
            .unwrap_or_else(|| plugin.to_string());
        let plugin = plugin.as_str();
        let prefix = format!("{plugin}:");
        skills_in(&root.join("skills"), &prefix, plugin, &mut out);
        commands_in(&root.join("commands"), &prefix, plugin, &mut out);
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|c| seen.insert(c.name.clone()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_menu_from_claude_codes_own_list() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let claude = ClaudeList {
            commands: s(&["compact", "kanban", "anthropic-skills:pdf", "deep-research", "anthropic-skills:deep-research", "mcp__x__prompt", "agents", "__internal", "neon:neon"]),
            skills: s(&["kanban", "anthropic-skills:pdf", "deep-research", "anthropic-skills:deep-research", "neon:neon"]),
        };
        let disk = vec![Command { name: "kanban".into(), description: "The board".into(), kind: "skill", source: Some("user".into()) }];
        let m = with_claude(&claude, &disk);
        let find = |n: &str| m.iter().find(|c| c.name == n).cloned();
        // The terminal-only built-ins are there, with their kinds.
        assert_eq!(find("btw").unwrap().kind, "btw");
        assert_eq!(find("exit").unwrap().kind, "app");
        assert!(find("resume").is_some());
        // Claude Code's own: described from disk, or the terminal's description of its own skills.
        assert_eq!(find("kanban").unwrap().description, "The board");
        assert!(!find("pdf").unwrap().description.is_empty());
        assert_eq!(find("pdf").unwrap().kind, "skill");
        assert!(find("compact").is_some() && find("neon:neon").is_some());
        // A taken bare name keeps the prefix; MCP prompts and hidden commands aren't listed.
        assert!(find("anthropic-skills:deep-research").is_some() && find("deep-research").is_some());
        assert!(find("mcp__x__prompt").is_none() && find("agents").is_none() && find("__internal").is_none());
        assert_eq!(m.iter().filter(|c| c.name == "compact").count(), 1);
        // The init line it is read from.
        let line = r#"{"type":"system","subtype":"init","slash_commands":["a","b"],"skills":["b"]}"#;
        assert_eq!(parse_init(line), Some(ClaudeList { commands: s(&["a", "b"]), skills: s(&["b"]) }));
        assert_eq!(parse_init(r#"{"type":"assistant"}"#), None);
    }

    #[test]
    fn reads_front_matter() {
        let t = "---\nname: kanban\ndescription: \"Keep the board current\"\nuser-invocable: false\n---\n# Body";
        assert_eq!(front_matter(t), (Some("kanban".into()), Some("Keep the board current".into()), false));
        assert_eq!(front_matter("# No front matter"), (None, None, true));
        // A folded description over several lines.
        let folded = "---
name: research
description: >-
  Research a person
  across systems.
user-invocable: true
---
";
        assert_eq!(front_matter(folded), (Some("research".into()), Some("Research a person across systems.".into()), true));
        assert_eq!(first_line("---\ndescription: x\n---\n\n# Deploy the app\nmore"), "Deploy the app");
    }

    #[test]
    fn lists_builtins_skills_commands_and_enabled_plugins() {
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join(".claude");
        let cwd = home.path().join("repo");
        let w = |p: std::path::PathBuf, t: &str| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, t).unwrap();
        };
        w(config.join("skills/kanban/SKILL.md"), "---\nname: kanban\ndescription: The board\n---\n");
        w(config.join("skills/hidden/SKILL.md"), "---\nname: hidden\nuser-invocable: false\n---\n");
        w(config.join("commands/tools/deploy.md"), "Deploy it now");
        w(cwd.join(".claude/skills/kanban/SKILL.md"), "---\nname: kanban\ndescription: The project's own\n---\n");
        w(cwd.join(".claude/commands/fix.md"), "---\ndescription: Fix it\n---\nbody");
        w(config.join("settings.json"), r#"{"enabledPlugins":{"wiki@mkt":true,"off@mkt":false}}"#);
        w(config.join("plugins/cache/mkt/wiki/1.0.0/skills/query/SKILL.md"), "---\nname: query\ndescription: Ask the wiki\n---\n");
        w(config.join("plugins/cache/mkt/off/1.0.0/skills/nope/SKILL.md"), "---\nname: nope\n---\n");
        w(config.join("settings.json"), r#"{"enabledPlugins":{"wiki@mkt":true,"off@mkt":false,"pg-tools@mkt":true}}"#);
        w(config.join("plugins/cache/mkt/pg-tools/2.0.0/.claude-plugin/plugin.json"), r#"{"name":"pg"}"#);
        w(config.join("plugins/cache/mkt/pg-tools/2.0.0/skills/auth/SKILL.md"), "---\nname: auth\n---\n");
        // No folder: no project commands.
        assert!(list(&config, None).iter().all(|c| c.source.as_deref() != Some("project")));
        assert!(list(&config, Some(Path::new(""))).iter().all(|c| c.source.as_deref() != Some("project")));
        let l = list(&config, Some(&cwd));
        let find = |n: &str| l.iter().find(|c| c.name == n).cloned();
        assert_eq!(find("btw").unwrap().kind, "btw");
        assert_eq!(find("config").unwrap().kind, "chat");
        assert_eq!(find("exit").unwrap().kind, "app");
        // The project's skill wins over the account's of the same name.
        assert_eq!(find("kanban").unwrap().description, "The project's own");
        assert_eq!(l.iter().filter(|c| c.name == "kanban").count(), 1);
        assert!(find("hidden").is_none());
        assert_eq!(find("tools:deploy").unwrap().description, "Deploy it now");
        assert_eq!((find("fix").unwrap().kind, find("fix").unwrap().source), ("command", Some("project".into())));
        assert_eq!(find("wiki:query").unwrap().source.as_deref(), Some("wiki"));
        assert!(find("off:nope").is_none());
        // Named by its plugin.json, not the key it was installed under.
        assert!(find("pg:auth").is_some() && find("pg-tools:auth").is_none());
    }
}
