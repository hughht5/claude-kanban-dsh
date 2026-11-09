//! One `claude` process per card, in a ConPTY, inside one kill-on-close Job Object.

use crate::transcript;
use base64::Engine;
use kanban_store::{Card, SessionRow};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const RING_BYTES: usize = 512 * 1024;

/// What a new session or a resume needs, worked out from the card.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub resume: Option<String>,
    pub session_id: String,
    pub account: String,
    pub cwd: String,
    pub model: String,
    pub effort: String,
    pub permission_mode: String,
    pub settings_path: PathBuf,
    /// The app's id for this session (the `sessions` row), known before the spawn.
    pub row_id: String,
    /// Where hooks, the MCP tools and the statusLine reach the app.
    pub api: Option<crate::api::ApiInfo>,
    /// Start it with Claude Code's to-do tools on (Hugh's `sessions.todo_tools`, card #106).
    pub todo_tools: bool,
    /// The Router session (card #119), not a card's: its rules go in as its system prompt, its
    /// subagents run on Sonnet medium, and its guard also sees Edit and Write.
    pub router: bool,
    /// A repo's merge lane (card #160), not a card's: its rules go in as its system prompt, it
    /// has no subagents and can't ask Hugh a question, and its guard (`guard::lane_refusal`) also
    /// sees Edit and Write.
    pub lane: bool,
    /// A card of a repo in `nobrowser::REPOS` (card #172): its settings deny computer use and
    /// every browser server, to it and its subagents, and its `mcp.json` adds no headless
    /// chrome-devtools.
    pub no_browser: bool,
}

/// Whether a live-map key is one of the app's own sessions, not a card's: the Router (card
/// #119) or a repo's merge lane (`lane:<repo>`, card #160). They sit outside the cap and the
/// start queue, are never paused for being idle, and their end is no card's crash.
pub fn is_app_key(key: &str) -> bool {
    key == crate::routersession::KEY || kanban_store::lane_repo(key).is_some()
}

/// Claude Code's "send now" keys (`chat:sendNow`, Ctrl+X Ctrl+S): send what is typed or
/// queued at once, interrupting the turn.
pub const SEND_NOW: &[u8] = b"\x18\x13";

/// The tool whose dialog asks Hugh a question in the terminal.
const QUESTION_TOOL: &str = crate::transcript::ASK_TOOL;

/// Hugh's answer to one question of a question dialog, from the chat panel: the options he
/// picked (0-based; exactly one for a single-select question), or his own words (single
/// select only, as the dialog's "Type something").
#[derive(Debug, Clone, Default, serde::Deserialize, PartialEq)]
pub struct Answer {
    #[serde(default)]
    pub picks: Vec<usize>,
    #[serde(default)]
    pub text: Option<String>,
}

/// The keys that answer a question dialog (`questions` as `transcript::ask_questions` gives
/// them), each sent as its own write. Learnt from Claude Code 2.1's dialog: in a single-select
/// question a digit picks that option and moves on; "Type something" is the digit after the
/// options, then the words (pasted) and Enter. In a multi-select question digits tick options
/// and Right moves on. With more than one question, or any multi-select, a review screen
/// follows, where 1 is "Submit answers".
pub fn answer_keys(questions: &Value, answers: &[Answer]) -> Result<Vec<String>, String> {
    let qs = questions.as_array().ok_or("no questions")?;
    if qs.is_empty() || qs.len() != answers.len() {
        return Err(format!("{} question(s) but {} answer(s)", qs.len(), answers.len()));
    }
    let digit = |i: usize| char::from(b'1' + i as u8).to_string();
    let mut keys = Vec::new();
    let mut review = qs.len() > 1;
    for (n, (q, a)) in qs.iter().zip(answers).enumerate() {
        let options = q.get("options").and_then(Value::as_array).map_or(0, Vec::len);
        if options == 0 || options > 7 {
            return Err(format!("question {} has {options} options", n + 1));
        }
        let text = a.text.as_deref().map(str::trim).filter(|t| !t.is_empty());
        if q.get("multiSelect").and_then(Value::as_bool).unwrap_or(false) {
            review = true;
            if text.is_some() {
                return Err(format!("question {}: type your own answer in the terminal", n + 1));
            }
            let mut picks = a.picks.clone();
            picks.sort_unstable();
            picks.dedup();
            if picks.is_empty() || picks.iter().any(|&p| p >= options) {
                return Err(format!("question {}: pick at least one of its options", n + 1));
            }
            keys.extend(picks.into_iter().map(digit));
            keys.push("\x1b[C".into());
        } else if let Some(t) = text {
            if !a.picks.is_empty() {
                return Err(format!("question {}: pick an option or type an answer, not both", n + 1));
            }
            keys.push(digit(options));
            // Only text goes in the paste: no control characters (an ESC could end the paste
            // early and send the rest as keys), and line breaks as \n.
            let clean: String = t.replace("\r\n", "\n").chars().filter(|c| *c == '\n' || !c.is_control()).collect();
            keys.push(format!("\x1b[200~{clean}\x1b[201~"));
            keys.push("\r".into());
        } else {
            match a.picks.as_slice() {
                [p] if *p < options => keys.push(digit(*p)),
                _ => return Err(format!("question {}: pick one of its options", n + 1)),
            }
        }
    }
    if review {
        keys.push("1".into());
    }
    Ok(keys)
}

/// Whether a turn end the transcript recorded at `ended` closes a session's `state`, set at
/// `state_at`: only a turn in progress, and only an end after the state began.
pub fn ended_by(state: &str, state_at: chrono::DateTime<chrono::Utc>, ended: chrono::DateTime<chrono::Utc>) -> bool {
    matches!(state, "working" | "permission") && ended > state_at
}

/// What decides whether Hugh's message can be typed into a session now (`takes_typing`).
#[derive(Clone, Copy)]
pub struct TypeCheck<'a> {
    pub state: &'a str,
    pub ready: bool,
    pub alive: bool,
    pub stopping: bool,
    /// Just after a slash command (`hold_until`).
    pub held: bool,
    /// The screen has been still for 1.5 s, as `free` asks.
    pub quiet: bool,
    /// How long ago a message was typed whose `UserPromptSubmit` hasn't come.
    pub sent_ago: Option<Duration>,
    pub switch_waiting: bool,
    /// A message typed at the prompt that Claude hasn't taken yet (`Typed`): nothing else is
    /// typed at the prompt before it.
    pub unconfirmed: bool,
    /// Only Hugh's own text is going in: nothing of the app's (column prompts, notices) was
    /// waiting with it, so it may go in mid-turn.
    pub mid_turn_ok: bool,
    /// How long the session has been in its state (`state_since`).
    pub idle_for: Duration,
}

/// How long a session has been idle before a mid-turn-ok message goes in without a still
/// screen (card #154): a turn that ended with a background subagent still running (a
/// card-tester, a card-reviewer) keeps its status line moving, so the screen is never still
/// for 1.5 s and the message would wait until the subagent ended.
pub const SETTLED: Duration = Duration::from_secs(3);

/// Whether Hugh's own message can be typed into a session now. Mid-turn, yes (Claude Code
/// queues typed text and reads it at its next step), unless the app's own text would go in
/// with it, or a message went in under a second ago. At its prompt, on the same terms as
/// `free` (still screen, nothing in flight), and after a failed turn too. No while it is
/// starting, stopping, asking Hugh something (Enter would answer the dialog), just after a
/// slash command, or with a model/effort switch to go first. Whatever can't go now waits in
/// the queue for `free`.
pub fn takes_typing(c: TypeCheck) -> bool {
    if !(c.alive && c.ready && !c.stopping && !c.held && !c.switch_waiting) {
        return false;
    }
    match c.state {
        "working" => c.mid_turn_ok && c.sent_ago.map_or(true, |d| d >= Duration::from_secs(1)),
        "idle" => (c.quiet || (c.mid_turn_ok && c.idle_for >= SETTLED)) && c.sent_ago.is_none() && !c.unconfirmed,
        "failed" => c.sent_ago.is_none(),
        _ => false,
    }
}

/// How many times a message typed at the prompt is typed before the app gives up on it.
pub const TYPE_TRIES: u8 = 3;
/// How long a typed message may go untaken before it is typed again.
pub const RETYPE_AFTER: Duration = Duration::from_secs(15);
/// How long the app waits for a session that hasn't drawn anything since a message was typed
/// into it (its TUI isn't reading its input yet, say just after a resume) before it gives the
/// message up. The wait spends no tries: typing again would only queue more copies (card #175).
pub const DRAW_WAIT: Duration = Duration::from_secs(180);

/// A message typed at Claude's prompt that its `UserPromptSubmit` hasn't confirmed yet. At
/// start-up the TUI can drop the paste as well as the Enter, so it stays on record until
/// Claude takes it (`nudge` presses Enter again or types it again).
#[derive(Clone)]
struct Typed {
    text: String,
    /// When this try was typed (or its Enter pressed again).
    at: Instant,
    /// When it was first typed: the session should have drawn since.
    first: Instant,
    tries: u8,
}

/// What `nudge` does about an unconfirmed message.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Retry {
    Wait,
    /// The message shows in the prompt (`prompt_holds`): press Enter again, typing no new copy.
    Enter,
    /// Type it again, first clearing any copy still in the prompt (`retype_bytes`).
    Retype,
    /// It wasn't taken after `TYPE_TRIES` tries, or the session drew nothing for `DRAW_WAIT`:
    /// show it in the chat as not taken.
    GiveUp,
}

/// What `retry_step` knows about an unconfirmed message.
#[derive(Debug, Clone, Copy)]
pub struct RetryCheck {
    /// Since this try was typed.
    pub since: Duration,
    /// Since it was first typed.
    pub since_first: Duration,
    pub tries: u8,
    /// Ready, idle, not stopping, and nothing printed for a moment.
    pub idle_and_quiet: bool,
    /// The terminal printed something after the message was first typed, so the TUI is reading
    /// its input. If it hasn't, the keys are still waiting for it, in order.
    pub drawn: bool,
    /// The prompt box shows the message (`prompt_holds`).
    pub holds: bool,
}

/// The rule for an unconfirmed message, once `RETYPE_AFTER` has passed with no
/// `UserPromptSubmit` for it, and only at a quiet idle prompt (never into a question dialog,
/// where Enter would answer it, nor mid-turn). A session that has drawn nothing since the
/// message was typed is waited for, up to `DRAW_WAIT`, spending no tries. One whose prompt
/// shows the message gets Enter again; one whose prompt doesn't gets it typed again. Given up
/// after `TYPE_TRIES` tries. Card #175: retyping into a session that hadn't read the first copy
/// yet, or whose prompt already held it, left the text there two or three times over.
pub fn retry_step(c: RetryCheck) -> Retry {
    if c.since < RETYPE_AFTER || !c.idle_and_quiet {
        Retry::Wait
    } else if !c.drawn {
        if c.since_first >= DRAW_WAIT {
            Retry::GiveUp
        } else {
            Retry::Wait
        }
    } else if c.tries >= TYPE_TRIES {
        Retry::GiveUp
    } else if c.holds {
        Retry::Enter
    } else {
        Retry::Retype
    }
}

/// What is typed in Claude Code's prompt box on `screen` (the last box `prompt_showing` would
/// see): its lines' words joined with single spaces, without the "❯". None if no box shows.
pub fn prompt_text(screen: &str) -> Option<String> {
    let rule = |l: &str| l.trim().chars().count() >= 20 && l.trim().chars().all(|c| c == '─');
    let lines: Vec<&str> = screen.lines().collect();
    let start = (1..lines.len()).rev().find(|&i| rule(lines[i - 1]) && lines[i].trim_start().starts_with('❯'))?;
    let end = (start + 1..lines.len()).find(|&i| rule(lines[i]))?;
    let mut words = vec![];
    for (k, l) in lines[start..end].iter().enumerate() {
        let l = if k == 0 { l.trim_start().trim_start_matches('❯') } else { l };
        words.extend(l.split_whitespace());
    }
    Some(words.join(" "))
}

/// The first 40 characters of `text`, with its whitespace evened out to single spaces.
fn opening(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(40).collect()
}

/// Whether `a` and `b` open with the same 40 characters (whitespace evened out).
fn same_start(a: &str, b: &str) -> bool {
    let (a, b) = (opening(a), opening(b));
    !a.is_empty() && a == b
}

/// Whether the prompt box on `screen` holds `text`, once, as Claude Code shows a paste: a long
/// one (more than 2 lines, or 800 characters) as "[Pasted text #N +M lines]", a short one as
/// itself, soft-wrapped (so it is compared by its first 40 characters, with whitespace evened
/// out). Two copies count as not held: Enter would send both, so the retype clears them.
pub fn prompt_holds(screen: &str, text: &str) -> bool {
    let Some(typed) = prompt_text(screen) else { return false };
    let long = text.lines().count() > 2 || text.chars().count() > 800;
    let (want, pasted) = (opening(text), "[Pasted text #");
    if long && typed.contains(pasted) {
        return typed.matches(pasted).count() == 1 && !typed.contains(&want);
    }
    !want.is_empty() && typed.matches(want.as_str()).count() == 1
}

/// Whether a message typed at the prompt is kept until Claude takes it. Not a slash command:
/// local ones (`/model`, `/compact`) send no `UserPromptSubmit`, so they would be typed again.
pub fn confirms(text: &str) -> bool {
    !text.trim_start().starts_with('/')
}

/// How a message is typed again: any copy still in the prompt is cleared first. Ctrl+U clears
/// only the current line (checked in Claude Code's TUI, 5 Nov 2026), so it goes with Backspace,
/// which joins the line above, once per line of the message and twice more; on an empty prompt
/// both do nothing. Then the paste, as `paste_bytes`.
pub fn retype_bytes(text: &str) -> String {
    let lines = text.matches('\n').count() + 3;
    format!("{}{}", "\x15\x7f".repeat(lines.min(2000)), paste_bytes(text))
}

/// When the turn in progress began, after a hook moved the session to `state`: a turn starts
/// when it goes to work and keeps its start through a question to Hugh ("permission") and any
/// message sent mid-turn; it ends at idle, failed or ended.
pub fn turn_start(prev: Option<Instant>, state: &str, now: Instant) -> Option<Instant> {
    match state {
        "working" | "permission" => prev.or(Some(now)),
        _ => None,
    }
}

/// The state a hook puts a session in, or None for a hook that doesn't change it.
pub fn hook_state(event: &str, body: &Value) -> Option<&'static str> {
    let msg = body.get("message").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
    let kind = body.get("notification_type").and_then(Value::as_str).unwrap_or("");
    let tool = body.get("tool_name").and_then(Value::as_str).unwrap_or("");
    Some(match event {
        "PreToolUse" if tool == QUESTION_TOOL => "permission",
        "PostToolUse" if tool == QUESTION_TOOL => "working",
        "Notification" if kind == "permission_prompt" || kind == "elicitation_dialog" => "permission",
        "Notification" if kind == "idle_prompt" => "idle",
        "Notification" if msg.contains("permission") => "permission",
        "Notification" if msg.contains("waiting for your input") => "idle",
        "SessionStart" | "Stop" => "idle",
        "UserPromptSubmit" => "working",
        "StopFailure" => "failed",
        "SessionEnd" => "ended",
        _ => return None,
    })
}

/// What a session waits on Hugh for, beyond its state, for the board's "Needs you" line
/// (card #107). In memory only.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Waits {
    /// A permission prompt's message, from its `Notification` hook.
    pub prompt: Option<String>,
    /// The call that prompt waits on (its id, "<Tool>: <key input>" and transcript), from the
    /// transcript (`transcript::prompt_call`), once a worker has read it.
    pub call: Option<crate::transcript::OpenCall>,
    /// Counts permission prompts, so a late read is kept only for the prompt it was for.
    pub seq: u64,
    /// A `StopFailure`'s error text, when its body has one.
    pub failure: Option<String>,
}

impl Waits {
    /// Takes a hook that moved the session to `state`. `asking` is a question dialog being on
    /// screen. Returns true for a new permission prompt whose call should be read.
    pub fn hook(&mut self, event: &str, state: &str, body: &Value, asking: bool) -> bool {
        if state != "permission" {
            self.prompt = None;
            self.call = None;
        }
        if event == "StopFailure" {
            self.failure = failure_text(body);
        } else if state != "failed" {
            self.failure = None;
        }
        let message = body.get("message").and_then(Value::as_str).map(str::trim).filter(|m| !m.is_empty());
        let tool = message.and_then(crate::transcript::permission_tool);
        if event != "Notification" || state != "permission" || asking || tool.as_deref() == Some(QUESTION_TOOL) {
            return false;
        }
        self.prompt = message.map(str::to_string);
        self.call = None;
        self.seq += 1;
        tool.is_some()
    }

    /// The session's state left "permission" or "failed" without a hook (`turn_ended`).
    pub fn left(&mut self, state: &str) {
        if state != "permission" {
            self.prompt = None;
            self.call = None;
        }
        if state != "failed" {
            self.failure = None;
        }
    }
}

/// A `StopFailure` body's error text: its `error` (Claude Code 2.1: "rate_limit",
/// "server_error"...) and `error_details`, else a `message`. None when it has none.
pub fn failure_text(body: &Value) -> Option<String> {
    let s = |k: &str| body.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
    let error = s("error").map(|e| e.replace('_', " "));
    match (error, s("error_details")) {
        (Some(e), Some(d)) => Some(format!("{e}: {d}")),
        (Some(e), None) => Some(e),
        (None, Some(d)) => Some(d.to_string()),
        (None, None) => s("message").map(str::to_string),
    }
}

/// A running session's waits as the board reads them (`Sessions::waits`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Waiting {
    /// The question dialog's questions (`transcript::ask_questions`), while it is on screen.
    pub questions: Option<Value>,
    /// The permission prompt: what it asks to run, else its message.
    pub prompt: Option<String>,
    pub failure: Option<String>,
}

/// The session's own settings: the fullscreen TUI, nine `http` hooks (`SubagentStop` brings
/// the card-tester's verdicts, card #76), the statusLine and
/// the `kanban` tools allowed. Nothing else; the project's settings still apply under it.
/// `PreToolUse`/`PostToolUse` are only for `AskUserQuestion`: while its dialog is up the
/// session "needs you", so nothing is typed into it (Enter would pick an answer); and for the
/// tools that start a skill or a subagent, which with `SubagentStop` say when a review or a
/// test runs (`testing`).
pub fn settings_json(p: &Plan) -> Value {
    let mut v = serde_json::json!({ "tui": "fullscreen" });
    if let Some(api) = &p.api {
        let mut hooks = serde_json::Map::new();
        for ev in ["SessionStart", "UserPromptSubmit", "Stop", "StopFailure", "Notification", "SessionEnd", "PreToolUse", "PostToolUse", "SubagentStop"] {
            let url = format!("http://127.0.0.1:{}/hook/{}/{}/{ev}", api.port, api.token, p.row_id);
            let mut entry = serde_json::json!({ "hooks": [{ "type": "http", "url": url, "timeout": 5 }] });
            if ev.ends_with("ToolUse") {
                entry["matcher"] = format!("{QUESTION_TOOL}|{}", crate::testing::TOOLS).into();
            }
            let mut entries = vec![entry];
            // Every shell call also asks the app's guard first, which refuses an app subagent's
            // kb, claude config and (the merger's) push (cards #100, #96): an agent's own deny rule
            // would take its whole shell away.
            if ev == "PreToolUse" {
                let url = format!("http://127.0.0.1:{}/hook/{}/{}/guard", api.port, api.token, p.row_id);
                let matcher = if p.router || p.lane { crate::guard::ROUTER_TOOLS } else { crate::guard::SHELL_TOOLS };
                entries.push(serde_json::json!({ "matcher": matcher, "hooks": [{ "type": "http", "url": url, "timeout": 5 }] }));
            }
            hooks.insert(ev.into(), Value::Array(entries));
        }
        v["hooks"] = Value::Object(hooks);
        if let Some(kb) = &api.kb_exe {
            v["statusLine"] = serde_json::json!({ "type": "command", "command": format!("\"{}\" status", kb.display()) });
        }
        v["permissions"] = serde_json::json!({ "allow": ["mcp__kanban"] });
    }
    if p.no_browser {
        // A deny rule here holds the session and every subagent it runs (card #172), with or
        // without the app's API.
        v["permissions"]["deny"] = serde_json::json!(crate::nobrowser::deny_rules());
    }
    v
}

pub fn mcp_json(p: &Plan) -> Option<Value> {
    let api = p.api.as_ref()?;
    let mut v = serde_json::json!({ "mcpServers": { "kanban": {
        "type": "http",
        "url": format!("http://127.0.0.1:{}/mcp/{}/{}", api.port, api.token, p.row_id)
    } } });
    // A card that tests without a browser (card #172) gets no headless Chrome either.
    let devtools = if p.no_browser { None } else { account_config(&p.account).and_then(|c| headless_devtools(&c, &p.cwd)) };
    if let Some(dt) = devtools {
        v["mcpServers"][DEVTOOLS] = dt;
    }
    Some(v)
}

/// The Chrome DevTools MCP server, which card sessions use to check their work in a browser.
const DEVTOOLS: &str = "chrome-devtools";
/// What a card session's copy of it adds: Chrome runs without a window, so it never takes
/// Hugh's focus, and each session gets its own throwaway profile, so two cards don't fight over one.
const DEVTOOLS_FLAGS: [&str; 2] = ["--headless", "--isolated"];

/// The account's own Claude Code config (`.claude.json`), read only. Main is `~\.claude.json`;
/// account2's lives in its config folder.
fn account_config(account: &str) -> Option<Value> {
    let path = if account == "account2" {
        transcript::config_dir("account2").join(".claude.json")
    } else {
        kanban_store::home_var()?.join(".claude.json")
    };
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// The account's `chrome-devtools` server (the session folder's own entry first, then the user
/// one) with `--headless` and `--isolated` added, as the session's override: a `--mcp-config`
/// server of the same name replaces the configured one. None when the account has none (nothing
/// is added) or it isn't a command server.
pub fn headless_devtools(config: &Value, cwd: &str) -> Option<Value> {
    let norm = |s: &str| s.replace('\\', "/").trim_end_matches('/').to_ascii_lowercase();
    let project = config["projects"].as_object().and_then(|ps| {
        ps.iter().find(|(k, _)| norm(k) == norm(cwd)).and_then(|(_, p)| p["mcpServers"].get(DEVTOOLS))
    });
    let mut server = project.or_else(|| config["mcpServers"].get(DEVTOOLS))?.clone();
    server.get("command")?.as_str()?;
    let args = server.as_object_mut()?.entry("args").or_insert_with(|| Value::Array(vec![]));
    let args = args.as_array_mut()?;
    for flag in DEVTOOLS_FLAGS {
        let has = args.iter().filter_map(Value::as_str).any(|a| a == flag || a.starts_with(&format!("{flag}=")));
        if !has {
            args.push(flag.into());
        }
    }
    Some(server)
}

/// The model and effort a column uses (PLAN.md, *Columns*), unless the card overrides them.
pub fn column_defaults(column: &str) -> (&'static str, &'static str) {
    match column {
        "Blocked" => ("claude-sonnet-5-5", "medium"),
        "Ship it" | "Deploy complete" => ("claude-opus-5-5", "medium"),
        _ => ("claude-opus-5-5", "high"),
    }
}

/// The folder a repo's card sessions start in.
/// Set in the private folder's `repos.json` (see `private`); the home folder otherwise.
pub fn repo_folder(repo: Option<&str>) -> PathBuf {
    crate::private::repo_folder_in(&crate::private::repos(), repo, &crate::private::home())
}

pub fn build_args(p: &Plan) -> Vec<String> {
    let mut a = Vec::new();
    match &p.resume {
        Some(id) => a.extend(["--resume".to_string(), id.clone()]),
        None => a.extend(["--session-id".to_string(), p.session_id.clone()]),
    }
    a.extend(["--model".into(), p.model.clone(), "--effort".into(), p.effort.clone()]);
    a.extend(["--permission-mode".into(), p.permission_mode.clone()]);
    a.extend(["--settings".into(), p.settings_path.display().to_string()]);
    if p.api.is_some() {
        let mcp = p.settings_path.with_file_name("mcp.json");
        a.extend(["--mcp-config".into(), mcp.display().to_string()]);
        // The app's subagents: the card-tester and card-reviewer every card passes before
        // Human test (cards #76 and #96), and the rest of `agents::AGENTS`.
        if p.router {
            // Its rules stay after every auto-compaction as its system prompt (finding 11), and
            // its subagents run on Sonnet medium (decision 11).
            a.extend(["--agents".into(), crate::routersession::agents_json()]);
            a.extend(["--append-system-prompt".into(), crate::routersession::PROMPT.to_string()]);
        } else if p.lane {
            // A merge lane (card #160): its rules as its system prompt, so they survive every
            // compaction; no subagents (it does the merge itself); no question to Hugh, who isn't
            // watching it.
            a.extend(["--append-system-prompt".into(), crate::lane::PROMPT.to_string()]);
            a.extend(["--disallowedTools".into(), crate::transcript::ASK_TOOL.to_string()]);
        } else {
            a.extend(["--agents".into(), crate::agents::agents_json()]);
        }
    }
    a
}

/// The child's environment: the app's own, without anything that would make `claude`
/// think it runs inside another Claude session, plus the account and card. `todo_tools` gives
/// back the to-do tools (TaskCreate, TaskUpdate) that Claude Code took away from newer models,
/// so the card can show its step progress (card #106).
pub fn build_env(parent: impl Iterator<Item = (String, String)>, account: &str, card_number: i64, todo_tools: bool) -> Vec<(String, String)> {
    let board = kanban_store::is_test().then(|| (kanban_store::data_dir(), std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from))));
    build_env_for(parent, account, card_number, todo_tools, board)
}

/// `build_env` with the board spelt out. `board` is None for the live app: its sessions get no
/// `KANBAN_DATA_DIR` (an inherited one is dropped), and PATH as it is. A test app passes its data
/// folder and its install folder (finding F1): `KANBAN_DATA_DIR` sends every `kb` and `kb mcp`
/// the session runs (the repo's `.mcp.json` one too) to the test board, and the install folder
/// goes first on PATH, so `kb` is the test app's own kb.exe even where the live one is older.
pub fn build_env_for(parent: impl Iterator<Item = (String, String)>, account: &str, card_number: i64, todo_tools: bool, board: Option<(PathBuf, Option<PathBuf>)>) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = parent
        .filter(|(k, _)| {
            let k = k.to_ascii_uppercase();
            k != "CLAUDECODE" && !k.starts_with("CLAUDE_CODE_") && k != "CLAUDE_CONFIG_DIR" && k != "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS" && k != kanban_store::DATA_DIR_ENV
        })
        .collect();
    if let Some((data, install)) = board {
        env.push((kanban_store::DATA_DIR_ENV.into(), data.display().to_string()));
        if let Some(dir) = install {
            match env.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case("PATH")) {
                Some((_, v)) => {
                    let joined = std::env::join_paths(std::iter::once(dir.clone()).chain(std::env::split_paths(v.as_str())));
                    if let Some(j) = joined.ok().and_then(|j| j.into_string().ok()) {
                        *v = j;
                    }
                }
                None => env.push(("PATH".into(), dir.display().to_string())),
            }
        }
    }
    env.push(("TERM".into(), "xterm-256color".into()));
    env.push(("CLAUDE_CODE_ALT_SCREEN_FULL_REPAINT".into(), "1".into()));
    env.push(("KANBAN_CARD".into(), card_number.to_string()));
    if todo_tools {
        env.push(("CLAUDE_CODE_ENABLE_TODO_TOOLS".into(), "1".into()));
    }
    if account == "account2" {
        env.push(("CLAUDE_CONFIG_DIR".into(), transcript::config_dir("account2").display().to_string()));
    }
    env
}

/// The newest `claude.exe` on this machine, found once per run: the native install in
/// `~.localin`, the desktop app's copies, or `~in`. Each candidate is asked its version.
pub fn claude_exe() -> Option<PathBuf> {
    static FOUND: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    FOUND.get_or_init(|| {
        let home = kanban_store::home_var()?;
        crate::platform::claude_candidates(&home)
            .into_iter()
            .filter(|p| p.is_file())
            .filter_map(|p| claude_version(&p).map(|v| (v, p)))
            .max()
            .map(|(_, p)| p)
    })
    .clone()
}

/// The version of the `claude.exe` the app uses, worked out once.
pub fn claude_exe_version() -> Option<String> {
    static V: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    V.get_or_init(|| claude_version(&claude_exe()?).map(|v| v.iter().map(u64::to_string).collect::<Vec<_>>().join("."))).clone()
}

/// `claude --version` as numbers, e.g. "2.1.317 (Claude Code)" -> [2, 1, 317].
pub fn claude_version(exe: &std::path::Path) -> Option<Vec<u64>> {
    use crate::platform::NoWindow;
    let out = std::process::Command::new(exe).arg("--version").no_window().output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let ver = text.split_whitespace().next()?;
    let parts: Vec<u64> = ver.split(".").map(|p| p.parse().ok()).collect::<Option<_>>()?;
    (!parts.is_empty()).then_some(parts)
}

/// The effort levels `/effort` takes.
pub const EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// What is typed to switch a running session's model: `/model <id>`. The id is a model id or
/// alias (`claude-sonnet-5-5`, `opus`, `opus[1m]`): letters, digits and `-._:[]` only, so
/// nothing else can ride along into the prompt.
pub fn model_command(model: &str) -> Result<String, String> {
    let m = model.trim();
    let ok = !m.is_empty()
        && m.len() <= 64
        && !m.starts_with('-')
        && m.chars().all(|c| c.is_ascii_alphanumeric() || "-._:[]".contains(c));
    if !ok {
        return Err(format!("\"{model}\" isn't a model id"));
    }
    Ok(format!("/model {m}"))
}

/// What is typed to switch a running session's effort: `/effort <level>`.
pub fn effort_command(effort: &str) -> Result<String, String> {
    let e = effort.trim().to_ascii_lowercase();
    if !EFFORTS.contains(&e.as_str()) {
        return Err(format!("effort must be one of {}", EFFORTS.join(", ")));
    }
    Ok(format!("/effort {e}"))
}

/// A model or effort change waiting to be typed into a running session.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Switch {
    pub model: Option<String>,
    pub effort: Option<String>,
}

impl Switch {
    /// Adds a newer request: each field given replaces the waiting one.
    pub fn merge(&mut self, model: Option<String>, effort: Option<String>) {
        if model.is_some() {
            self.model = model;
        }
        if effort.is_some() {
            self.effort = effort;
        }
    }

    /// The next command to type (the model first, then the effort), with its field and value.
    pub fn next(&self) -> Option<(&'static str, String, String)> {
        if let Some(m) = &self.model {
            return model_command(m).ok().map(|c| ("model", m.clone(), c));
        }
        let e = self.effort.as_ref()?;
        effort_command(e).ok().map(|c| ("effort", e.clone(), c))
    }

    pub fn is_empty(&self) -> bool {
        self.model.is_none() && self.effort.is_none()
    }
}

/// How text is typed into a session: Ctrl+U clears the prompt, then a bracketed paste with no
/// CR inside. Enter follows as a separate write.
pub fn paste_bytes(text: &str) -> String {
    format!("\x15\x1b[200~{}\x1b[201~", text.replace('\r', ""))
}

/// Drops the sessions that ended by themselves, reaping each one's process: the macOS exit
/// waiter doesn't reap, so a dead claude isn't left as a zombie (card #117). On Windows it only
/// reads the exit code. Called where the map is looked at often (a start, the running count).
fn drop_ended(live: &mut HashMap<String, Live>) {
    live.retain(|_, l| {
        let keep = l.alive.load(Ordering::SeqCst) || l.stopping;
        if !keep {
            let _ = l.child.try_wait();
        }
        keep
    });
}

/// Account 2 isn't supported on a Mac in this version (card #117, router decision R2): such a
/// card refuses to start rather than run on the main account.
pub const ACCOUNT2_NOT_HERE: &str = "Account 2 isn't supported on a Mac in this version. Set this card to the main account to start it.";

/// Whether `account` can run on this system (`second`: whether account 2 can).
pub fn account_runs_here(account: &str, second: bool) -> Result<(), String> {
    if account == "account2" && !second {
        Err(ACCOUNT2_NOT_HERE.into())
    } else {
        Ok(())
    }
}

pub fn card_account(card: &Card) -> String {
    match card.fields.get("accountPin").or_else(|| card.fields.get("account")).and_then(Value::as_str) {
        Some("account2") => "account2".into(),
        _ => "main".into(),
    }
}

/// The session's last `RING_BYTES` of output, and how many bytes it has put out in all (card
/// #95). Each `pty` event carries that count after its chunk, and the snapshot the count at its
/// end, so a terminal that opens mid-stream skips what its snapshot already holds and loses
/// nothing that came after.
struct Ring {
    buf: VecDeque<u8>,
    total: u64,
}

impl Ring {
    fn new() -> Ring {
        Ring { buf: VecDeque::new(), total: 0 }
    }

    /// Adds a chunk; returns the output's length in all after it.
    fn push(&mut self, b: &[u8]) -> u64 {
        self.buf.extend(b);
        self.total += b.len() as u64;
        let over = self.buf.len().saturating_sub(RING_BYTES);
        if over > 0 {
            self.buf.drain(..over);
        }
        self.total
    }

    /// What the ring holds, and the output's length in all at its end.
    fn snapshot(&self) -> (Vec<u8>, u64) {
        (self.buf.iter().copied().collect(), self.total)
    }
}

struct Live {
    row: SessionRow,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    ring: Arc<Mutex<Ring>>,
    /// The terminal's screen as Claude Code drew it (`screen_text`): a `/btw` answer is only
    /// ever on screen, never in the transcript.
    screen: Arc<Mutex<vt100::Parser>>,
    alive: Arc<AtomicBool>,
    /// When the PTY last printed anything.
    last_out: Arc<Mutex<Instant>>,
    /// From the hooks: "starting", "idle", "working", "permission", "failed" or "ended".
    state: String,
    /// `SessionStart` has arrived, so typed text reaches Claude's prompt.
    ready: bool,
    /// A message was typed and its `UserPromptSubmit` hasn't arrived yet.
    sent_at: Option<Instant>,
    retried: bool,
    /// The app is stopping it: kept in the map until the stop has killed it.
    stopping: bool,
    /// A model or effort change waiting for the session to be free.
    switch: Switch,
    /// Nothing more is typed before this (just after a slash command, which sends no hook).
    hold_until: Option<Instant>,
    /// When `state` last changed, for pausing a session left idle.
    state_since: Instant,
    /// The same, by the clock, to set against transcript timestamps (`turn_ended`).
    state_at: chrono::DateTime<chrono::Utc>,
    /// When the turn in progress began (its first `UserPromptSubmit`), for the chat panel's
    /// "Working… (5m 55s)" line. Kept through a question to Hugh; cleared when the turn ends.
    turn_since: Option<Instant>,
    /// The same, by the clock, for the board's status chip (`doing`).
    turn_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The question dialog on screen, from its `PreToolUse` hook: the call's `tool_use_id`
    /// (if the hook gave one) and its questions. Cleared once the session leaves "permission"
    /// or the chat panel has answered it.
    question: Option<(Option<String>, Value)>,
    /// What it waits on Hugh for beyond its state: a permission prompt's text, a failure's error
    /// (card #107).
    waits: Waits,
    /// The session's transcript, from its statusLine reports, for `turn_check`.
    transcript: Option<std::path::PathBuf>,
    /// The `error` its last `StopFailure` hook named ("rate_limit", say), for the stuck
    /// monitor's reason. Cleared by the next prompt.
    fail_error: Option<String>,
    /// A message typed at the prompt that Claude hasn't taken yet (`Typed`).
    typed: Option<Typed>,
    /// A message the session didn't take after `TYPE_TRIES` tries, for the chat to show.
    untaken: Option<String>,
    /// Its reviews and tests in flight, for the card's "Testing" chip.
    testing: crate::testing::Testing,
    /// Its subagents in flight, by name, for the card's status chip (card #150).
    running: crate::doing::Running,
    /// A `/btw` overlay is up (`ask_btw`): nothing else is typed into the session meanwhile.
    btw: bool,
    /// A slash command was typed, and many open a screen of their own (`/config`, `/cost`, a
    /// bare `/model`; which ones changes between Claude Code versions): nothing is typed while
    /// its prompt box isn't on screen (`prompt_showing`), so a queued message can't go into
    /// that screen. Cleared by `nudge` once the prompt shows, and after `DIALOG_MAX` anyway.
    dialog: Option<Instant>,
}

/// How long after a slash command its screen is given to draw before the prompt box counts.
const DIALOG_DRAW: Duration = Duration::from_secs(4);

/// The longest the app holds its typing for a screen a slash command opened.
const DIALOG_MAX: Duration = Duration::from_secs(30 * 60);

impl Live {
    /// Nothing may be typed into it now: just after a slash command (`hold_until`), while a
    /// `/btw` is open, or while a screen a slash command opened is still up.
    fn blocked(&self) -> bool {
        self.hold_until.map_or(false, |t| Instant::now() < t) || self.btw || self.dialog_drawing() || self.on_screen()
    }

    /// Just after a slash command, while a screen it opens may not have drawn yet: the prompt
    /// still shows, so `on_screen` alone would let typing (and `nudge`) through too soon.
    fn dialog_drawing(&self) -> bool {
        self.dialog.is_some_and(|t| t.elapsed() < DIALOG_DRAW)
    }

    /// A screen a slash command opened is still up (`dialog`).
    fn on_screen(&self) -> bool {
        self.dialog.is_some_and(|t| t.elapsed() < DIALOG_MAX) && !self.prompt_showing()
    }

    fn prompt_showing(&self) -> bool {
        self.screen.lock().map(|s| prompt_showing(&rows_text(&s))).unwrap_or(true)
    }
}

/// The screen's text, one line per row, trailing spaces trimmed.
fn rows_text(p: &vt100::Parser) -> String {
    let s = p.screen();
    let (_, cols) = s.size();
    s.rows(0, cols).map(|r| r.trim_end().to_string()).collect::<Vec<_>>().join("\n")
}

/// Whether Claude Code's prompt box is on `screen`: a rule of "─", a line starting "❯" (what is
/// typed, perhaps over several lines), then another rule. A picker's "❯" marks its choice
/// ("❯ 1. Default") and isn't taken for the prompt.
pub fn prompt_showing(screen: &str) -> bool {
    let rule = |l: &str| l.trim().chars().count() >= 20 && l.trim().chars().all(|c| c == '─');
    let lines: Vec<&str> = screen.lines().filter(|l| !l.trim().is_empty()).collect();
    (1..lines.len()).any(|i| {
        let Some(after) = lines[i].trim_start().strip_prefix('❯') else { return false };
        let a = after.trim_start();
        let choice = a.chars().next().is_some_and(|c| c.is_ascii_digit()) && a.trim_start_matches(|c: char| c.is_ascii_digit()).starts_with('.');
        rule(lines[i - 1]) && !choice && lines[i + 1..].iter().take(40).any(|l| rule(l))
    })
}

#[derive(Serialize, Clone)]
struct PtyOut {
    card: String,
    /// The session row this output is from, so a terminal never mixes two runs.
    run: String,
    /// The session's output length in all after this chunk (`Ring::push`).
    end: u64,
    data: String,
}

/// What a terminal replays when it opens: the ring's bytes (base64), where they end in the
/// session's output, and which run of the session they are from.
#[derive(Serialize, Debug, PartialEq)]
pub struct Snapshot {
    pub run: String,
    pub end: u64,
    pub data: String,
}

/// One live card session, as the stuck monitor sees it (`Sessions::watch`).
#[derive(Debug, Clone, Default)]
pub struct Watch {
    pub card_id: String,
    /// "starting", "idle", "working", "permission" or "failed".
    pub state: String,
    /// How long it has been in that state.
    pub in_state: Duration,
    pub ready: bool,
    /// Nothing may be typed into it now (just after a slash command, a `/btw`, a screen open).
    pub held: bool,
    /// A question dialog is on screen.
    pub question: bool,
    /// It is running a review or a test (`testing.rs`).
    pub testing: bool,
    /// A message typed into it hasn't been taken yet.
    pub typing: bool,
    pub account: String,
    pub claude_session_id: String,
    pub transcript: Option<std::path::PathBuf>,
    pub fail_error: Option<String>,
}

/// One live card session's status, as the board's status chip reads it (`Sessions::doing`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Doing {
    /// "starting", "idle", "working", "permission" or "failed".
    pub state: String,
    /// When it went into that state (RFC 3339).
    pub since: String,
    /// When the turn in progress began, if it is on one.
    pub turn_since: Option<String>,
    /// Its subagents running, oldest first.
    pub running: Vec<crate::doing::Sub>,
    /// It is running a review or a test (`testing.rs`).
    pub testing: bool,
}

pub struct Sessions {
    live: crate::lockwatch::Watched<HashMap<String, Live>>,
    job: crate::platform::Job,
    /// Each card's `/btw` questions and answers this run, oldest first (`crate::btw`).
    btws: Mutex<HashMap<String, Vec<crate::btw::Btw>>>,
}

/// How long a `/btw` may take to show its answer before the app gives up and closes it.
const BTW_WAIT: Duration = Duration::from_secs(180);

impl Sessions {
    pub fn new() -> Result<Sessions, String> {
        Ok(Sessions { live: crate::lockwatch::Watched::new("sessions", HashMap::new()), job: crate::platform::Job::new()?, btws: Mutex::new(HashMap::new()) })
    }

    /// Whether a screen a slash command opened is up in the card's session (`Live::dialog`):
    /// the chat opens the terminal for it, and says why a message waits.
    pub fn on_screen(&self, card_id: &str) -> bool {
        let Ok(live) = self.live.lock() else { return false };
        live.get(card_id).is_some_and(|l| l.alive.load(Ordering::SeqCst) && l.on_screen())
    }

    /// The card's `/btw` questions and answers this run.
    pub fn btws(&self, card_id: &str) -> Vec<crate::btw::Btw> {
        self.btws.lock().ok().and_then(|b| b.get(card_id).cloned()).unwrap_or_default()
    }

    fn set_btw(&self, card_id: &str, i: usize, state: &'static str, text: String) {
        if let Ok(mut b) = self.btws.lock() {
            if let Some(x) = b.get_mut(card_id).and_then(|v| v.get_mut(i)) {
                x.state = state;
                x.text = text;
            }
        }
    }

    /// Hugh's `/btw <question>` from the chat: typed into the running session as in its terminal
    /// (even mid-turn), then read off the screen by a thread (`run_btw`) that keeps the answer
    /// for the chat and closes the overlay. `done` is called once it is answered or failed.
    /// Refused while the session can't take it: not running or not ready, asking Hugh something
    /// (the keys would go to the question), or already on a `/btw`.
    pub fn ask_btw(self: &Arc<Self>, card_id: &str, question: &str, done: impl Fn() + Send + 'static) -> Result<(), String> {
        let question = question.split_whitespace().collect::<Vec<_>>().join(" ");
        if question.is_empty() {
            return Err("write your question after /btw".into());
        }
        {
            let mut live = self.live.lock().map_err(|e| e.to_string())?;
            let l = live.get_mut(card_id).filter(|l| l.alive.load(Ordering::SeqCst) && !l.stopping).ok_or("the session isn't running: start it, then ask with /btw")?;
            if !l.ready {
                return Err("the session is still starting; ask again in a moment".into());
            }
            if l.state == "permission" {
                return Err("the session is asking you something; answer it first, then ask with /btw".into());
            }
            if l.btw {
                return Err("a /btw is already open; wait for its answer".into());
            }
            // Not over something still going in: the paste would clear a message whose Enter
            // hasn't been pressed yet, or land in a screen a slash command opened.
            if l.blocked() || l.sent_at.is_some_and(|t| t.elapsed() < Duration::from_millis(1500)) {
                return Err("something else is still going into the session; ask again in a moment".into());
            }
            // `blocked` holds every other typing until `end_btw`.
            l.btw = true;
            // A message typed just before still gets no extra Enter: it would close the overlay.
            l.retried = true;
        }
        let i = {
            let mut b = self.btws.lock().map_err(|e| e.to_string())?;
            let v = b.entry(card_id.to_string()).or_default();
            v.push(crate::btw::Btw { at: chrono_now(), question: question.clone(), state: "asking", text: String::new() });
            v.len() - 1
        };
        if let Err(e) = self.input(card_id, paste_bytes(&format!("/btw {question}")).as_bytes()) {
            self.end_btw(card_id);
            self.set_btw(card_id, i, "failed", format!("couldn't type it: {e}"));
            return Err(e);
        }
        let me = self.clone();
        let id = card_id.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            let _ = me.input(&id, b"\r");
            let (state, text) = match me.run_btw(&id) {
                Ok(a) => ("answered", a),
                Err(e) => ("failed", e),
            };
            me.end_btw(&id);
            me.set_btw(&id, i, state, text);
            done();
        });
        Ok(())
    }

    /// Lets the session take typing again after a `/btw`.
    fn end_btw(&self, card_id: &str) {
        if let Ok(mut live) = self.live.lock() {
            if let Some(l) = live.get_mut(card_id) {
                l.btw = false;
                l.hold_until = Some(Instant::now() + Duration::from_millis(500));
            }
        }
    }

    /// Waits for the `/btw` answer on screen, reads all of it (scrolling the overlay down while
    /// it adds lines), then closes the overlay with Esc. Esc only while the overlay shows, so a
    /// question dialog that came up meanwhile is never dismissed.
    fn run_btw(&self, card_id: &str) -> Result<String, String> {
        use crate::btw::{merge, view, Merged};
        let start = Instant::now();
        let read = || self.screen_text(card_id).ok_or_else(|| "the session ended".to_string());
        let mut last: Option<Vec<String>> = None;
        let (mut seen, mut gone) = (false, 0);
        let first = loop {
            std::thread::sleep(Duration::from_millis(400));
            if start.elapsed() > BTW_WAIT {
                self.close_btw(card_id);
                return Err("no answer after 3 minutes; it was closed".into());
            }
            match view(&read()?) {
                None => {
                    // One read without it can be a frame caught half drawn: only three in a
                    // row (over a second) count as closed.
                    gone += 1;
                    if seen && gone >= 3 {
                        return Err("the answer was closed before the app could read it; see the terminal".into());
                    }
                    if !seen && start.elapsed() > Duration::from_secs(20) {
                        return Err("the /btw didn't open; see the terminal".into());
                    }
                }
                Some(v) => {
                    (seen, gone) = (true, 0);
                    // Finished once its footer offers "c to copy", and drawn the same twice.
                    if v.answering || !v.done || v.lines.is_empty() {
                        continue;
                    }
                    if last.as_ref() == Some(&v.lines) {
                        break v;
                    }
                    last = Some(v.lines);
                }
            }
        };
        let mut got = first.lines.clone();
        let mut gap = false;
        // Down until the overlay adds nothing (a short answer stops at once). A few keys at a
        // time, each its own write with a gap: a burst of them reads as the scroll wheel, which
        // jumps further than the overlay shows (tried on 2.1.318). At most two minutes.
        let step = (first.lines.len() / 3).clamp(1, 3);
        let (scroll, mut still) = (Instant::now(), 0);
        while scroll.elapsed() < Duration::from_secs(120) {
            for _ in 0..step {
                if !self.key_if_btw(card_id, b"\x1b[B") {
                    break;
                }
                std::thread::sleep(Duration::from_millis(70));
            }
            std::thread::sleep(Duration::from_millis(150));
            let Some(v) = view(&read()?) else { break };
            match merge(&mut got, &v.lines) {
                Merged::Added => still = 0,
                Merged::Gap => {
                    still = 0;
                    gap = true;
                }
                Merged::Nothing => {
                    still += 1;
                    if still >= 2 {
                        break;
                    }
                }
            }
        }
        if still < 2 {
            gap = true;
        }
        self.close_btw(card_id);
        let mut got = crate::btw::unwrap(&got);
        if gap {
            got.push(String::new());
            got.push("(Part of the answer may be missing; the app couldn't read all of it.)".into());
        }
        Ok(got.join("\n"))
    }

    /// Esc, only if the `/btw` overlay still shows.
    fn close_btw(&self, card_id: &str) {
        self.key_if_btw(card_id, b"\x1b");
    }

    /// Writes `key` only while the `/btw` overlay shows, checked under the same lock as the
    /// write, so the key can't reach a prompt or a question dialog instead. Returns whether it did.
    fn key_if_btw(&self, card_id: &str, key: &[u8]) -> bool {
        let Ok(mut live) = self.live.lock() else { return false };
        let Some(l) = live.get_mut(card_id) else { return false };
        let showing = l.screen.lock().map(|s| crate::btw::view(&rows_text(&s)).is_some()).unwrap_or(false);
        showing && l.writer.write_all(key).and_then(|_| l.writer.flush()).is_ok()
    }

    /// Puts a helper process (the app's sizing call, card #159) in the Job Object, so it ends
    /// with the app like every card session.
    pub fn adopt(&self, pid: u32) -> Result<(), String> {
        self.job.assign(pid)
    }

    pub fn is_live(&self, card_id: &str) -> Option<SessionRow> {
        let live = self.live.lock().ok()?;
        live.get(card_id).filter(|l| l.alive.load(Ordering::SeqCst)).map(|l| l.row.clone())
    }

    /// Starts `plan` for `card`. Refuses a conversation that is already running.
    pub fn start(&self, app: &AppHandle, card: &Card, plan: &Plan) -> Result<SessionRow, String> {
        self.start_as(app, &card.id, card.number, plan)
    }

    /// Starts `plan` under `key` in the live map: a card's id, or the Router's key (card #119).
    pub fn start_as(&self, app: &AppHandle, key: &str, number: i64, plan: &Plan) -> Result<SessionRow, String> {
        let mut live = self.live.lock().map_err(|e| e.to_string())?;
        drop_ended(&mut live);
        account_runs_here(&plan.account, crate::platform::SECOND_ACCOUNT)?;
        let convo = plan.resume.clone().unwrap_or_else(|| plan.session_id.clone());
        if live.values().any(|l| l.row.claude_session_id == convo) {
            return Err("that conversation is already running in the app".into());
        }
        if live.contains_key(key) {
            return Err("this card already has a running session".into());
        }
        let exe = claude_exe().ok_or_else(|| format!("couldn't find claude{}", std::env::consts::EXE_SUFFIX))?;
        if let Some(dir) = plan.settings_path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        // Private files: they hold the API token in the hook URLs (card #117).
        crate::platform::write_private(&plan.settings_path, settings_json(plan).to_string().as_bytes()).map_err(|e| e.to_string())?;
        if let Some(m) = mcp_json(plan) {
            crate::platform::write_private(&plan.settings_path.with_file_name("mcp.json"), m.to_string().as_bytes()).map_err(|e| e.to_string())?;
        }

        let pair = native_pty_system()
            .openpty(PtySize { rows: 40, cols: 140, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(&exe);
        for a in build_args(plan) {
            cmd.arg(a);
        }
        cmd.cwd(&plan.cwd);
        cmd.env_clear();
        for (k, v) in build_env(std::env::vars(), &plan.account, number, plan.todo_tools) {
            cmd.env(k, v);
        }
        cmd.env("KANBAN_SESSION", &plan.row_id);
        if let Some(api) = &plan.api {
            cmd.env("KANBAN_API", format!("http://127.0.0.1:{}", api.port));
        }
        let child = pair.slave.spawn_command(cmd).map_err(|e| format!("couldn't start claude: {e}"))?;
        drop(pair.slave);
        if let Some(pid) = child.process_id() {
            self.job.assign(pid)?;
        }
        let row = SessionRow {
            id: plan.row_id.clone(),
            card_id: key.to_string(),
            claude_session_id: convo,
            account: plan.account.clone(),
            cwd: plan.cwd.clone(),
            model: Some(plan.model.clone()),
            effort: Some(plan.effort.clone()),
            started_at: chrono_now(),
            status: Some("running".into()),
        };
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let ring = Arc::new(Mutex::new(Ring::new()));
        let screen = Arc::new(Mutex::new(vt100::Parser::new(40, 140, 0)));
        let alive = Arc::new(AtomicBool::new(true));
        let last_out = Arc::new(Mutex::new(Instant::now()));
        // A session stopped by the app has its event written already. One that ended without
        // the app stopping it is a crash, for the notifications. Called when the PTY's output
        // ends and when the process exits; only the first call counts.
        let ended = {
            let (alive, app, card_id, sid, number) = (alive.clone(), app.clone(), key.to_string(), row.id.clone(), number);
            Arc::new(move || {
                if alive.swap(false, Ordering::SeqCst) {
                    if let Some(board) = app.try_state::<crate::Board>() {
                        if let Ok(mut s) = board.0.lock() {
                            let _ = s.session_event(&sid, "exited", None);
                        }
                    }
                    // The Router's end is told by `routersession::tick`, and a lane's by
                    // `lane::tick`, not as a card's crash.
                    if !is_app_key(&card_id) {
                        let _ = app.emit("session-crashed", serde_json::json!({ "card": card_id, "number": number }));
                    }
                }
                let _ = app.emit("session-changed", card_id.clone());
            })
        };
        // ConPTY keeps the output pipe open after the process exits, so the reader alone
        // never sees a crash: a thread waits on the process itself too.
        if let Some(wait) = child.process_id().and_then(crate::platform::exit_waiter) {
            let ended = ended.clone();
            std::thread::spawn(move || {
                wait();
                ended();
            });
        }
        {
            let (ring, screen, app, card_id, run) = (ring.clone(), screen.clone(), app.clone(), key.to_string(), row.id.clone());
            let (last_out, ended) = (last_out.clone(), ended.clone());
            std::thread::spawn(move || {
                let mut buf = [0u8; 16384];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let end = ring.lock().unwrap_or_else(|e| e.into_inner()).push(&buf[..n]);
                            if let Ok(mut sc) = screen.lock() {
                                sc.process(&buf[..n]);
                            }
                            if let Ok(mut t) = last_out.lock() {
                                *t = Instant::now();
                            }
                            let data = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                            let _ = app.emit("pty", PtyOut { card: card_id.clone(), run: run.clone(), end, data });
                        }
                    }
                }
                ended();
            });
        }
        live.insert(
            key.to_string(),
            Live { row: row.clone(), writer, master: pair.master, child, ring, screen, alive, last_out, state: "starting".into(), ready: false, sent_at: None, retried: false, stopping: false, switch: Switch::default(), hold_until: None, state_since: Instant::now(), state_at: chrono::Utc::now(), turn_since: None, turn_at: None, question: None, waits: Default::default(), transcript: None, fail_error: None, typed: None, untaken: None, testing: Default::default(), running: Default::default(), btw: false, dialog: None },
        );
        Ok(row)
    }

    /// A hook from session `row_id`. Returns the card, its new state and, for a new permission
    /// prompt whose call should be read (`Waits::hook`), the prompt's count and message. None
    /// for a hook that doesn't set a state (a subagent's or a skill's, for `testing_hook` only).
    pub fn hook(&self, row_id: &str, event: &str, body: &Value) -> Option<(String, String, Option<(u64, String)>)> {
        let state = hook_state(event, body)?;
        let mut live = self.live.lock().ok()?;
        let (card_id, l) = live.iter_mut().find(|(_, l)| l.row.id == row_id)?;
        match event {
            "SessionStart" => l.ready = true,
            "UserPromptSubmit" => {
                l.sent_at = None;
                l.retried = false;
                // Any prompt taken counts as the typed message: typing it again after a prompt
                // that was it, with its text changed on the way, would send it twice.
                l.typed = None;
                // A message given up on that Claude takes after all (a TUI that read its input
                // late, card #175): no longer "Not sent", so Send again can't send it twice.
                let prompt = body.get("prompt").and_then(Value::as_str).unwrap_or("");
                if l.untaken.as_deref().is_some_and(|u| same_start(prompt, u)) {
                    l.untaken = None;
                }
                // A new turn, even from "working" (a turn interrupted with no hook): an end
                // the transcript recorded before now belongs to an earlier turn (`ended_by`).
                l.state_at = chrono::Utc::now();
                l.fail_error = None;
            }
            "StopFailure" => {
                l.fail_error = body.get("error").and_then(|e| e.as_str().map(str::to_string).or_else(|| e.get("type").and_then(Value::as_str).map(str::to_string)));
            }
            _ => {}
        }
        if event == "PreToolUse" && state == "permission" {
            let id = body.get("tool_use_id").and_then(Value::as_str).map(str::to_string);
            l.question = body.get("tool_input").and_then(crate::transcript::ask_questions).map(|q| (id, q));
        } else if state != "permission" {
            l.question = None;
        }
        let asking = l.question.is_some();
        let read = l.waits.hook(event, state, body, asking);
        let prompt = l.waits.prompt.clone().filter(|_| read).map(|m| (l.waits.seq, m));
        if l.state != state {
            l.state_since = Instant::now();
            l.state_at = chrono::Utc::now();
        }
        l.turn_since = turn_start(l.turn_since, state, Instant::now());
        l.turn_at = l.turn_since.and(l.turn_at.or_else(|| Some(chrono::Utc::now())));
        l.state = state.to_string();
        Some((card_id.clone(), l.state.clone(), prompt))
    }

    /// A hook from session `row_id`, for its reviews and tests. Returns the card and whether
    /// it is testing now, when that changed.
    pub fn testing_hook(&self, row_id: &str, event: &str, body: &Value) -> Option<(String, bool)> {
        let mut live = self.live.lock().ok()?;
        let (card_id, l) = live.iter_mut().find(|(_, l)| l.row.id == row_id)?;
        let now = Instant::now();
        let was = l.testing.active(now);
        let is = l.testing.hook(event, body, now);
        (was != is).then(|| (card_id.clone(), is))
    }

    /// A hook from session `row_id`, for its subagents (`doing.rs`). Returns the card when
    /// what it runs changed.
    pub fn running_hook(&self, row_id: &str, event: &str, body: &Value) -> Option<String> {
        let mut live = self.live.lock().ok()?;
        let (card_id, l) = live.iter_mut().find(|(_, l)| l.row.id == row_id)?;
        l.running.hook(event, body, chrono::Utc::now()).then(|| card_id.clone())
    }

    /// What each running card's session is doing, for the board's status chip (card #150): its
    /// state, since when, when its turn began, its subagents and whether it is testing.
    pub fn doing(&self) -> HashMap<String, Doing> {
        let Ok(live) = self.live.lock() else { return HashMap::new() };
        let now = Instant::now();
        let wall = chrono::Utc::now();
        // The clock times kept beside the `Instant`s, to the second: the page compares answers.
        let at = |t: chrono::DateTime<chrono::Utc>| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        live.iter()
            .filter(|(_, l)| l.alive.load(Ordering::SeqCst) && !l.stopping)
            .map(|(k, l)| {
                let d = Doing {
                    state: if l.ready { l.state.clone() } else { "starting".into() },
                    since: at(l.state_at),
                    turn_since: l.turn_at.map(at),
                    running: l.running.list(wall),
                    testing: l.testing.active(now),
                };
                (k.clone(), d)
            })
            .collect()
    }

    /// The running cards whose session is running a review or a test.
    pub fn testing_cards(&self) -> Vec<String> {
        let Ok(live) = self.live.lock() else { return vec![] };
        let now = Instant::now();
        live.iter().filter(|(_, l)| l.alive.load(Ordering::SeqCst) && l.testing.active(now)).map(|(k, _)| k.clone()).collect()
    }

    /// Whether session `row_id` is on a turn ("working" or "permission"), so a statusLine
    /// report should check its transcript for a turn that ended without a hook (`turn_ended`).
    pub fn on_turn(&self, row_id: &str) -> bool {
        let Ok(live) = self.live.lock() else { return false };
        live.values().any(|l| l.row.id == row_id && matches!(l.state.as_str(), "working" | "permission"))
    }

    /// The transcript says the turn ended at `ended_at` (`transcript::turn_ended_in`): an
    /// interrupted turn (Esc, or a permission prompt or question turned down in the terminal)
    /// sends no Stop hook, so without this the session would show "working" or "asking you"
    /// until the next prompt. Only an end later than the state's own start counts, so the end
    /// of the turn before a new prompt is never taken for this one's. Returns the card and its
    /// new state, "idle".
    pub fn turn_ended(&self, row_id: &str, ended_at: &str) -> Option<(String, String)> {
        let ended = chrono::DateTime::parse_from_rfc3339(ended_at).ok()?.with_timezone(&chrono::Utc);
        let mut live = self.live.lock().ok()?;
        let (card_id, l) = live.iter_mut().find(|(_, l)| l.row.id == row_id)?;
        if !ended_by(&l.state, l.state_at, ended) {
            return None;
        }
        l.state = "idle".into();
        l.state_since = Instant::now();
        l.state_at = chrono::Utc::now();
        l.turn_since = None;
        l.turn_at = None;
        l.question = None;
        l.waits.left("idle");
        l.testing.turn_ended();
        l.running.turn_ended();
        Some((card_id.clone(), l.state.clone()))
    }

    /// The statusLine draws only once Claude's prompt is up (after any trust question), so its
    /// first report marks the session ready. `SessionStart` doesn't arrive as an `http` hook.
    pub fn status_seen(&self, row_id: &str) -> Option<(String, String)> {
        let mut live = self.live.lock().ok()?;
        let (card_id, l) = live.iter_mut().find(|(_, l)| l.row.id == row_id)?;
        if l.ready {
            return None;
        }
        l.ready = true;
        if l.state == "starting" {
            l.state = "idle".into();
            l.state_since = Instant::now();
            l.state_at = chrono::Utc::now();
        }
        Some((card_id.clone(), l.state.clone()))
    }

    /// Cards whose session can take typed text now: ready, idle, not mid-send, and quiet for 1.5 s.
    pub fn free(&self) -> Vec<String> {
        let Ok(live) = self.live.lock() else { return vec![] };
        live.iter()
            .filter(|(_, l)| {
                l.alive.load(Ordering::SeqCst)
                    && l.ready
                    && l.state == "idle"
                    && l.sent_at.is_none()
                    && l.typed.is_none()
                    && !l.blocked()
                    && l.last_out.lock().map(|t| t.elapsed() > Duration::from_millis(1500)).unwrap_or(false)
            })
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Cards whose session has sat idle (ready, nothing being typed) for at least `d`.
    pub fn idle_for(&self, d: Duration) -> Vec<String> {
        let Ok(live) = self.live.lock() else { return vec![] };
        live.iter()
            // Never the Router or a merge lane: they are idle most of the time on purpose (cards
            // #119 and #160; a lane stops itself after 15 idle minutes).
            .filter(|(k, _)| !is_app_key(k))
            .filter(|(_, l)| l.alive.load(Ordering::SeqCst) && !l.stopping && l.ready && l.state == "idle" && l.sent_at.is_none() && l.typed.is_none() && l.untaken.is_none() && !l.btw && !l.on_screen() && l.state_since.elapsed() >= d)
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Presses Enter once more for a message typed 6 s ago that Claude hasn't taken yet.
    /// A message typed at the prompt that still isn't taken `RETYPE_AFTER` later gets Enter
    /// again if its prompt shows it, else is typed again, and after `TYPE_TRIES` tries is kept
    /// for the chat as not taken (`retry_step`). Returns the cards it did something for, and
    /// what (`Enter`, `Retype` or `GiveUp`), for the panel.
    /// The retry is written under the lock, so a `UserPromptSubmit` can't slip in between the
    /// check and the typing.
    pub fn nudge(self: &Arc<Self>) -> Vec<(String, Retry)> {
        let mut changed = vec![];
        {
            let Ok(mut live) = self.live.lock() else { return changed };
            for (card_id, l) in live.iter_mut() {
                // A /btw overlay, or a screen a slash command opened, is up: an Enter or a retype
                // would go into it (`ask_btw`, `dialog`). Back at the prompt, the screen is done.
                if l.dialog.is_some() && !l.dialog_drawing() && !l.on_screen() {
                    l.dialog = None;
                }
                if l.btw || l.dialog.is_some() {
                    continue;
                }
                if let Some(t) = l.sent_at {
                    if t.elapsed() > Duration::from_secs(6) && !l.retried {
                        l.retried = true;
                        let _ = l.writer.write_all(b"\r");
                        let _ = l.writer.flush();
                    } else if t.elapsed() > Duration::from_secs(20) {
                        l.sent_at = None;
                    }
                }
                let Some((since, since_first, first, tries)) = l.typed.as_ref().map(|t| (t.at.elapsed(), t.first.elapsed(), t.first, t.tries)) else { continue };
                let out = l.last_out.lock().map(|o| *o).ok();
                let idle_and_quiet = l.alive.load(Ordering::SeqCst)
                    && !l.stopping
                    && l.ready
                    && l.state == "idle"
                    && out.is_some_and(|o| o.elapsed() > Duration::from_millis(1500));
                let drawn = out.map_or(true, |o| o > first);
                // Only read the screen when a retry is due and would use it.
                let due = since >= RETYPE_AFTER && idle_and_quiet && drawn && tries < TYPE_TRIES;
                let holds = due && l.typed.as_ref().is_some_and(|t| l.screen.lock().map(|s| prompt_holds(&rows_text(&s), &t.text)).unwrap_or(false));
                let check = RetryCheck { since, since_first, tries, idle_and_quiet, drawn, holds };
                match retry_step(check) {
                    Retry::Wait => {}
                    Retry::Enter => {
                        let Some(t) = l.typed.as_mut() else { continue };
                        if l.writer.write_all(b"\r").and_then(|_| l.writer.flush()).is_ok() {
                            t.at = Instant::now();
                            t.tries += 1;
                            l.sent_at = Some(Instant::now());
                            // This was the Enter: no second one 6 s later.
                            l.retried = true;
                            changed.push((card_id.clone(), Retry::Enter));
                        }
                    }
                    Retry::Retype => {
                        let Some(t) = l.typed.as_mut() else { continue };
                        if l.writer.write_all(retype_bytes(&t.text).as_bytes()).and_then(|_| l.writer.flush()).is_ok() {
                            t.at = Instant::now();
                            t.tries += 1;
                            l.sent_at = Some(Instant::now());
                            l.retried = false;
                            changed.push((card_id.clone(), Retry::Retype));
                        }
                    }
                    Retry::GiveUp => {
                        l.untaken = l.typed.take().map(|t| t.text);
                        l.sent_at = None;
                        changed.push((card_id.clone(), Retry::GiveUp));
                    }
                }
            }
        }
        // Enter for each retype, as a separate write (`send_message`).
        for (card_id, _) in changed.iter().filter(|(_, r)| *r == Retry::Retype) {
            let (me, id) = (self.clone(), card_id.clone());
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                let _ = me.input(&id, b"\r");
            });
        }
        changed
    }

    /// The message typed at the prompt that Claude hasn't taken yet, and one it didn't take at
    /// all, for the chat panel.
    pub fn typing(&self, card_id: &str) -> (Option<String>, Option<String>) {
        let Ok(live) = self.live.lock() else { return (None, None) };
        live.get(card_id).map_or((None, None), |l| (l.typed.as_ref().map(|t| t.text.clone()), l.untaken.clone()))
    }

    /// Forgets the message the session didn't take, when `text` is it (the chat's Send again).
    pub fn clear_untaken(&self, card_id: &str, text: &str) {
        if let Ok(mut live) = self.live.lock() {
            if let Some(l) = live.get_mut(card_id) {
                if l.untaken.as_deref().map(str::trim) == Some(text.trim()) {
                    l.untaken = None;
                }
            }
        }
    }

    /// How many sessions are running, for the cap. One being stopped no longer counts.
    pub fn running_count(&self) -> usize {
        self.live
            .lock()
            .map(|mut live| {
                drop_ended(&mut live);
                // The Router and the merge lanes sit outside the cap (cards #119 and #160).
                live.iter().filter(|(k, l)| !is_app_key(k) && l.alive.load(Ordering::SeqCst)).count()
            })
            .unwrap_or(0)
    }

    /// Asks for a model and/or effort change on the card's running session. It is typed when
    /// the session is next free (ready, idle, quiet), so a turn in progress isn't interrupted.
    /// A value the session already has is dropped. Returns what is now waiting.
    pub fn request_switch(&self, card_id: &str, model: Option<String>, effort: Option<String>) -> Result<Switch, String> {
        let mut live = self.live.lock().map_err(|e| e.to_string())?;
        let l = live.get_mut(card_id).filter(|l| l.alive.load(Ordering::SeqCst)).ok_or("no running session")?;
        l.switch.merge(model, effort);
        if l.switch.model.is_some() && l.switch.model == l.row.model {
            l.switch.model = None;
        }
        if l.switch.effort.is_some() && l.switch.effort == l.row.effort {
            l.switch.effort = None;
        }
        Ok(l.switch.clone())
    }

    pub fn pending_switch(&self, card_id: &str) -> Option<Switch> {
        let live = self.live.lock().ok()?;
        live.get(card_id).map(|l| l.switch.clone()).filter(|s| !s.is_empty())
    }

    /// Types the next waiting model or effort command into a free session. Returns the field,
    /// the value and the session row's id once it is typed; the live row then shows the new value.
    pub fn apply_switch(self: &Arc<Self>, card_id: &str) -> Option<(&'static str, String, String)> {
        let (field, value, cmd) = {
            let live = self.live.lock().ok()?;
            live.get(card_id).filter(|l| l.alive.load(Ordering::SeqCst))?.switch.next()?
        };
        self.send_command(card_id, &cmd).ok()?;
        let mut live = self.live.lock().ok()?;
        let l = live.get_mut(card_id)?;
        if field == "model" {
            l.switch.model = None;
            l.row.model = Some(value.clone());
        } else {
            l.switch.effort = None;
            l.row.effort = Some(value.clone());
        }
        Some((field, value, l.row.id.clone()))
    }

    /// Types a slash command the way `send_message` types a message. A local command sends
    /// no `UserPromptSubmit`, so instead of waiting for one, nothing else is typed for 2.5 s.
    pub fn send_command(self: &Arc<Self>, card_id: &str, text: &str) -> Result<(), String> {
        self.input(card_id, paste_bytes(text).as_bytes())?;
        if let Ok(mut live) = self.live.lock() {
            if let Some(l) = live.get_mut(card_id) {
                l.hold_until = Some(Instant::now() + Duration::from_millis(2500));
            }
        }
        let me = self.clone();
        let id = card_id.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            let _ = me.input(&id, b"\r");
        });
        Ok(())
    }

    pub fn card_of_row(&self, row_id: &str) -> Option<String> {
        let live = self.live.lock().ok()?;
        live.iter().find(|(_, l)| l.row.id == row_id).map(|(k, _)| k.clone())
    }

    /// How long the card's session has been on its current turn, in milliseconds; None when it
    /// isn't mid-turn.
    pub fn turn_ms(&self, card_id: &str) -> Option<u64> {
        let live = self.live.lock().ok()?;
        let l = live.get(card_id).filter(|l| l.alive.load(Ordering::SeqCst))?;
        l.turn_since.map(|t| t.elapsed().as_millis() as u64)
    }

    /// Each running card's state, for the board's dots.
    pub fn states(&self) -> HashMap<String, String> {
        let Ok(live) = self.live.lock() else { return HashMap::new() };
        live.iter()
            .filter(|(_, l)| l.alive.load(Ordering::SeqCst))
            .map(|(k, l)| (k.clone(), if l.ready { l.state.clone() } else { "starting".into() }))
            .collect()
    }

    /// Each running card's waits (`Waiting`), for the board's "Needs you" line, as `states`.
    pub fn waits(&self) -> HashMap<String, Waiting> {
        let Ok(live) = self.live.lock() else { return HashMap::new() };
        live.iter()
            .filter(|(_, l)| l.alive.load(Ordering::SeqCst))
            .map(|(k, l)| {
                let asking = l.state == "permission";
                let w = Waiting {
                    questions: l.question.as_ref().filter(|_| asking).map(|(_, q)| q.clone()),
                    prompt: l.waits.call.as_ref().map(|c| c.text.clone()).or_else(|| l.waits.prompt.clone()),
                    failure: l.waits.failure.clone(),
                };
                (k.clone(), w)
            })
            .collect()
    }

    /// Session `row_id`'s permission prompt, while it is at one: its count (`Waits::seq`) and
    /// its message.
    pub fn prompt_wait(&self, row_id: &str) -> Option<(u64, String)> {
        let live = self.live.lock().ok()?;
        let l = live.values().find(|l| l.row.id == row_id && l.state == "permission")?;
        l.waits.prompt.clone().map(|m| (l.waits.seq, m))
    }

    /// Keeps what permission prompt `seq` of session `row_id` asks to run, if the session is
    /// still at that prompt. Returns its card when kept.
    pub fn set_prompt_call(&self, row_id: &str, seq: u64, call: crate::transcript::OpenCall) -> Option<String> {
        let mut live = self.live.lock().ok()?;
        let (card_id, l) = live.iter_mut().find(|(_, l)| l.row.id == row_id)?;
        if l.state != "permission" || l.waits.seq != seq || l.waits.prompt.is_none() {
            return None;
        }
        l.waits.call = Some(call);
        Some(card_id.clone())
    }

    /// Session `row_id`'s permission prompt whose call is known (`set_prompt_call`), while no
    /// question dialog is open: its count and the call, for `prompt_settled`.
    pub fn prompt_pending(&self, row_id: &str) -> Option<(u64, crate::transcript::OpenCall)> {
        let live = self.live.lock().ok()?;
        let l = live.values().find(|l| l.row.id == row_id && l.state == "permission" && l.question.is_none())?;
        l.waits.call.clone().map(|c| (l.waits.seq, c))
    }

    /// The transcript shows permission prompt `seq`'s call went on (`transcript::call_settled`):
    /// Hugh answered it, and no hook said so (`PostToolUse` is hooked for a few tools only).
    /// The session goes back to "working" and the prompt goes. Returns the card and its state.
    pub fn prompt_settled(&self, row_id: &str, seq: u64) -> Option<(String, String)> {
        let mut live = self.live.lock().ok()?;
        let (card_id, l) = live.iter_mut().find(|(_, l)| l.row.id == row_id)?;
        if l.state != "permission" || l.question.is_some() || l.waits.seq != seq || l.waits.call.is_none() {
            return None;
        }
        l.state = "working".into();
        l.state_since = Instant::now();
        l.state_at = chrono::Utc::now();
        l.waits.left("working");
        Some((card_id.clone(), l.state.clone()))
    }

    pub fn input(&self, card_id: &str, data: &[u8]) -> Result<(), String> {
        let mut live = self.live.lock().map_err(|e| e.to_string())?;
        let l = live.get_mut(card_id).ok_or("no running session")?;
        l.writer.write_all(data).and_then(|_| l.writer.flush()).map_err(|e| e.to_string())
    }

    pub fn resize(&self, card_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let live = self.live.lock().map_err(|e| e.to_string())?;
        let l = live.get(card_id).ok_or("no running session")?;
        let (rows, cols) = (rows.clamp(5, 300), cols.clamp(20, 500));
        l.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string())?;
        if let Ok(mut sc) = l.screen.lock() {
            sc.screen_mut().set_size(rows, cols);
        }
        Ok(())
    }

    /// What the session's terminal shows now, one line per row, trailing spaces trimmed.
    pub fn screen_text(&self, card_id: &str) -> Option<String> {
        let live = self.live.lock().ok()?;
        let l = live.get(card_id)?;
        let sc = l.screen.lock().ok()?;
        Some(rows_text(&sc))
    }

    pub fn snapshot(&self, card_id: &str) -> Option<Snapshot> {
        let live = self.live.lock().ok()?;
        let l = live.get(card_id).filter(|l| l.alive.load(Ordering::SeqCst))?;
        let (bytes, end) = l.ring.lock().ok()?.snapshot();
        Some(Snapshot { run: l.row.id.clone(), end, data: base64::engine::general_purpose::STANDARD.encode(bytes) })
    }

    /// The running session's row id, or None: a cheap check a terminal makes before it replays
    /// a snapshot, so it replays only for a new run (card #95).
    pub fn run_id(&self, card_id: &str) -> Option<String> {
        let live = self.live.lock().ok()?;
        live.get(card_id).filter(|l| l.alive.load(Ordering::SeqCst)).map(|l| l.row.id.clone())
    }

    /// The `tool_use_id` of the question dialog on screen, if the session is asking one (an
    /// empty string when its hook gave no id).
    pub fn question_id(&self, card_id: &str) -> Option<String> {
        let live = self.live.lock().ok()?;
        let l = live.get(card_id)?;
        if l.state != "permission" {
            return None;
        }
        l.question.as_ref().map(|(id, _)| id.clone().unwrap_or_default())
    }

    /// Answers the question dialog on screen from the chat panel: checks `tool_id` is the
    /// question asked (`question_id`), works out the keys from the question the hook gave
    /// (`answer_keys`), and types them one by one. The question is then cleared, so a second
    /// click is refused rather than typed into whatever comes next.
    pub fn answer(self: &Arc<Self>, card_id: &str, tool_id: &str, answers: &[Answer]) -> Result<(), String> {
        let keys = {
            let mut live = self.live.lock().map_err(|e| e.to_string())?;
            let l = live.get_mut(card_id).ok_or("the session isn't running")?;
            let asking = l.alive.load(Ordering::SeqCst) && !l.stopping && l.state == "permission";
            let Some((id, questions)) = l.question.as_ref().filter(|_| asking) else {
                return Err("the session isn't asking that question now; answer it in the terminal".into());
            };
            if id.as_deref().is_some_and(|id| id != tool_id) {
                return Err("that question isn't the one on screen; answer it in the terminal".into());
            }
            let keys = answer_keys(questions, answers)?;
            l.question = None;
            keys
        };
        let me = self.clone();
        let id = card_id.to_string();
        std::thread::spawn(move || {
            for k in keys {
                // Only while the dialog is still up: answered or dismissed in the terminal
                // meanwhile, the rest would go into whatever came next (a "1" there could
                // approve a permission prompt).
                if !me.still_asking(&id) || me.input(&id, k.as_bytes()).is_err() {
                    return;
                }
                // Each key is its own write, with time for the dialog to redraw. A paste needs
                // longer before its Enter, and the Enter of a typed answer longer still: a key
                // sent 350 ms after it still went into the typed answer (tried on 2.1.318).
                let wait = match k.as_str() {
                    "\r" => 1200,
                    _ if k.starts_with("\x1b[200~") => 600,
                    _ => 400,
                };
                std::thread::sleep(Duration::from_millis(wait));
            }
        });
        Ok(())
    }

    /// Whether the session is still on a question dialog (state "permission" since its
    /// `PreToolUse`), for `answer`'s keys.
    fn still_asking(&self, card_id: &str) -> bool {
        let Ok(live) = self.live.lock() else { return false };
        live.get(card_id).is_some_and(|l| l.alive.load(Ordering::SeqCst) && !l.stopping && l.state == "permission")
    }

    /// Each live card session as the stuck monitor sees it (`monitor.rs`), copied under one
    /// lock so that the monitor reads files with no lock held.
    pub fn watch(&self) -> Vec<Watch> {
        let Ok(live) = self.live.lock() else { return vec![] };
        let now = Instant::now();
        live.iter()
            .filter(|(_, l)| l.alive.load(Ordering::SeqCst) && !l.stopping)
            .map(|(card_id, l)| Watch {
                card_id: card_id.clone(),
                state: l.state.clone(),
                in_state: l.state_since.elapsed(),
                ready: l.ready,
                held: l.blocked(),
                question: l.question.is_some(),
                testing: l.testing.active(now),
                typing: l.sent_at.is_some() || l.typed.is_some() || l.untaken.is_some(),
                account: l.row.account.clone(),
                claude_session_id: l.row.claude_session_id.clone(),
                transcript: l.transcript.clone(),
                fail_error: l.fail_error.clone(),
            })
            .collect()
    }

    /// `watch` for one card, for the monitor's check again just before it types.
    pub fn watch_one(&self, card_id: &str) -> Option<Watch> {
        self.watch().into_iter().find(|w| w.card_id == card_id)
    }

    /// Records where session `row_id`'s transcript is (from its statusLine report).
    pub fn note_transcript(&self, row_id: &str, path: &str) {
        let Ok(mut live) = self.live.lock() else { return };
        if let Some(l) = live.values_mut().find(|l| l.row.id == row_id) {
            if l.transcript.as_deref() != Some(std::path::Path::new(path)) {
                l.transcript = Some(path.into());
            }
        }
    }

    /// Sessions on a turn whose state hasn't changed for `d`, with their transcripts: the
    /// app's timer checks these for a turn that ended with no hook, in case no statusLine
    /// report came after the transcript recorded the end.
    pub fn quiet_turns(&self, d: Duration) -> Vec<(String, std::path::PathBuf)> {
        let Ok(live) = self.live.lock() else { return vec![] };
        live.values()
            .filter(|l| matches!(l.state.as_str(), "working" | "permission") && l.state_since.elapsed() > d)
            .filter_map(|l| Some((l.row.id.clone(), l.transcript.clone()?)))
            .collect()
    }

    /// Types a message into the session the way a person would paste it, then presses Enter
    /// as a separate write (the TUI misreads Enter that arrives with the text).
    /// It is kept as unconfirmed until its `UserPromptSubmit` (`nudge` types it again).
    pub fn send_message(self: &Arc<Self>, card_id: &str, text: &str) -> Result<(), String> {
        let first = self.type_at_prompt(card_id, text)?;
        self.track(card_id, text, first);
        Ok(())
    }

    /// Records `text` as typed at the prompt at `first` and not yet taken (`Typed`), as its
    /// first try. A slash command isn't kept (`confirms`).
    fn track(&self, card_id: &str, text: &str, first: Instant) {
        if !confirms(text) {
            return;
        }
        if let Ok(mut live) = self.live.lock() {
            if let Some(l) = live.get_mut(card_id) {
                l.typed = Some(Typed { text: text.to_string(), at: Instant::now(), first, tries: 1 });
            }
        }
    }

    /// The typing itself: the paste, then Enter 300 ms later. A slash command is typed as
    /// `send_command` types one: a local command sends no `UserPromptSubmit`, so no second Enter
    /// follows it (`nudge`), which would answer whatever it opened (a picker, the `/btw` answer).
    /// Returns when the paste was written: anything the terminal prints after that shows the
    /// TUI is reading its input (`RetryCheck::drawn`).
    fn type_at_prompt(self: &Arc<Self>, card_id: &str, text: &str) -> Result<Instant, String> {
        let at = Instant::now();
        self.input(card_id, paste_bytes(text).as_bytes())?;
        if let Ok(mut live) = self.live.lock() {
            if let Some(l) = live.get_mut(card_id) {
                if confirms(text) {
                    l.sent_at = Some(Instant::now());
                    l.retried = false;
                } else {
                    l.sent_at = None;
                    l.retried = true;
                    l.hold_until = Some(Instant::now() + Duration::from_millis(2500));
                    l.dialog = Some(Instant::now());
                }
            }
        }
        let me = self.clone();
        let id = card_id.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            let _ = me.input(&id, b"\r");
        });
        Ok(at)
    }

    /// Types Hugh's message (the panel's box, `kb send`) into the session now, as typing in its
    /// terminal would, if the session can take it (`takes_typing`; `mid_turn_ok` when it is
    /// only his text). Mid-turn, Claude Code queues it itself and reads it at its next step.
    /// Returns false when it has to wait.
    pub fn type_now(self: &Arc<Self>, card_id: &str, text: &str, mid_turn_ok: bool) -> Result<bool, String> {
        let working = {
            let live = self.live.lock().map_err(|e| e.to_string())?;
            let Some(l) = live.get(card_id) else { return Ok(false) };
            let check = TypeCheck {
                state: &l.state,
                ready: l.ready,
                alive: l.alive.load(Ordering::SeqCst),
                stopping: l.stopping,
                held: l.blocked(),
                quiet: l.last_out.lock().map(|t| t.elapsed() > Duration::from_millis(1500)).unwrap_or(false),
                sent_ago: l.sent_at.map(|t| t.elapsed()),
                switch_waiting: !l.switch.is_empty(),
                unconfirmed: l.typed.is_some(),
                mid_turn_ok,
                idle_for: l.state_since.elapsed(),
            };
            if !takes_typing(check) {
                return Ok(false);
            }
            l.state == "working"
        };
        let first = self.type_at_prompt(card_id, text)?;
        // At the prompt it is kept until Claude takes it. Mid-turn the TUI only queues it, so
        // no UserPromptSubmit may come yet: no second Enter for it (`nudge`), which the busy
        // prompt might read as something else, and no retyping.
        if !working {
            self.track(card_id, text, first);
        } else {
            if let Ok(mut live) = self.live.lock() {
                if let Some(l) = live.get_mut(card_id) {
                    l.retried = true;
                }
            }
        }
        Ok(true)
    }

    /// Hugh's "send now" mid-turn, as Ctrl+X Ctrl+S (or Ctrl+Enter) in the terminal: types
    /// `text` (if any) and presses Claude Code's send-now keys, which interrupt the turn and send
    /// it, with anything already queued, at once. Only while the session is working and can
    /// take typing (`takes_typing`); returns false otherwise, and the caller sends it as usual.
    pub fn send_now(self: &Arc<Self>, card_id: &str, text: &str) -> Result<bool, String> {
        {
            let live = self.live.lock().map_err(|e| e.to_string())?;
            let Some(l) = live.get(card_id) else { return Ok(false) };
            let check = TypeCheck {
                state: &l.state,
                ready: l.ready,
                alive: l.alive.load(Ordering::SeqCst),
                stopping: l.stopping,
                held: l.blocked(),
                quiet: false,
                sent_ago: l.sent_at.map(|t| t.elapsed()),
                switch_waiting: !l.switch.is_empty(),
                unconfirmed: l.typed.is_some(),
                mid_turn_ok: true,
                idle_for: l.state_since.elapsed(),
            };
            if l.state != "working" || !takes_typing(check) {
                return Ok(false);
            }
        }
        if !text.trim().is_empty() {
            self.input(card_id, paste_bytes(text).as_bytes())?;
        }
        if let Ok(mut live) = self.live.lock() {
            if let Some(l) = live.get_mut(card_id) {
                l.sent_at = Some(Instant::now());
                l.retried = true; // no extra Enter: the send-now keys are the submit
            }
        }
        let me = self.clone();
        let id = card_id.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            let _ = me.input(&id, SEND_NOW);
        });
        Ok(true)
    }

    /// Ctrl+C, `/exit`, then a kill after 1.5 s if it's still running.
    pub fn stop(&self, app: &AppHandle, card_id: &str) -> Result<(), String> {
        self.suspend(app, card_id, None)
    }

    /// `stop`, recording `detail` on the `suspended` event.
    pub fn suspend(&self, app: &AppHandle, card_id: &str, detail: Option<&str>) -> Result<(), String> {
        // Marked stopped first, so the reader thread doesn't take Claude's own exit after
        // `/exit` for a crash, and nothing more is typed into it meanwhile.
        let row_id = {
            let mut live = self.live.lock().map_err(|e| e.to_string())?;
            live.get_mut(card_id).and_then(|l| {
                l.stopping = true;
                l.alive.swap(false, Ordering::SeqCst).then(|| l.row.id.clone())
            })
        };
        let _ = self.input(card_id, b"\x03");
        std::thread::sleep(Duration::from_millis(200));
        let _ = self.input(card_id, b"/exit\r");
        std::thread::sleep(Duration::from_millis(1500));
        let taken = self.live.lock().map_err(|e| e.to_string())?.remove(card_id);
        // The board is locked only once the sessions are released: the dispatcher takes
        // the board first and the sessions second, so this order can't deadlock with it.
        if let Some(id) = row_id {
            if let Some(board) = app.try_state::<crate::Board>() {
                if let Ok(mut s) = board.0.lock() {
                    let _ = s.session_event(&id, "suspended", detail);
                }
            }
        }
        if let Some(mut l) = taken {
            let _ = l.child.kill();
            // Waited for off this thread, so no ended process is left behind as a zombie on a
            // Mac (card #117); on Windows it only reads the exit code.
            let mut child = l.child;
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        let _ = app.emit("session-changed", card_id.to_string());
        Ok(())
    }

    /// Suspends every live session in parallel, as the app quits. Each `suspended` event
    /// says whether the session was mid-turn, so the next start resumes those straight away.
    pub fn stop_all(&self, app: &AppHandle) {
        // Each live card, its state, and whether a typed message hasn't been taken yet.
        // A message typed and not confirmed, or not taken at all, goes back in the queue, so it
        // is typed again after the resume rather than lost with the session.
        let turns: Vec<(String, String, bool, Option<String>)> = match self.live.lock() {
            Ok(live) => live
                .iter()
                .filter(|(_, l)| l.alive.load(Ordering::SeqCst))
                .map(|(k, l)| (k.clone(), l.state.clone(), l.sent_at.is_some(), l.typed.as_ref().map(|t| t.text.clone()).or_else(|| l.untaken.clone())))
                .collect(),
            Err(_) => return,
        };
        if turns.is_empty() {
            return;
        }
        if let Some(board) = app.try_state::<crate::Board>() {
            if let Ok(mut store) = board.0.lock() {
                for (card_id, _, _, text) in &turns {
                    if let Some(text) = text {
                        // Put back: recorded as the app's (or not) when it was first queued.
                        let _ = store.queue_hugh(card_id, "message", text);
                    }
                }
            }
        }
        let turns: Vec<(String, String, bool)> = turns.into_iter().map(|(k, s, sent, _)| (k, s, sent)).collect();
        // A message still waiting to be typed counts as mid-turn too: it is typed after the resume.
        let waiting = |card_id: &str| {
            app.try_state::<crate::Board>()
                .and_then(|b| b.0.lock().ok().map(|s| s.waiting(card_id, "message").ok().flatten().is_some()))
                .unwrap_or(false)
        };
        let details: Vec<(String, String)> =
            turns.iter().map(|(id, state, typed)| (id.clone(), turn_detail(state, *typed, waiting(id)))).collect();
        std::thread::scope(|scope| {
            for (card_id, detail) in &details {
                scope.spawn(move || {
                    let _ = self.suspend(app, card_id, Some(detail));
                });
            }
        });
    }
}

/// The `suspended` event's detail: "mid-turn (...)" or "idle".
pub fn turn_detail(state: &str, typed_not_taken: bool, message_waiting: bool) -> String {
    if kanban_store::MID_TURN.contains(&state) {
        format!("mid-turn ({state})")
    } else if typed_not_taken {
        "mid-turn (a message was typed but not yet submitted)".into()
    } else if message_waiting {
        "mid-turn (a message was waiting to be typed)".into()
    } else {
        "idle".into()
    }
}

fn chrono_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Card #95: the ring counts every byte the session has put out, also after it wraps, so a
    /// terminal can tell which live chunks its snapshot already holds.
    #[test]
    fn ring_counts_output_and_snapshot_ends_where_events_do() {
        let mut r = Ring::new();
        assert_eq!(r.snapshot(), (vec![], 0));
        assert_eq!(r.push(b"hello "), 6);
        assert_eq!(r.push(b"world"), 11);
        let (bytes, end) = r.snapshot();
        assert_eq!(bytes, b"hello world");
        assert_eq!(end, 11);
        // A chunk after the snapshot ends past it, so the terminal draws it; the earlier ones
        // end at or before it, so it skips them.
        assert!(r.push(b"!") > end);
        // Past the cap the ring keeps only the newest bytes, but the count keeps going.
        let big = vec![b'x'; RING_BYTES];
        let after = r.push(&big);
        assert_eq!(after, 12 + RING_BYTES as u64);
        let (bytes, end) = r.snapshot();
        assert_eq!(bytes.len(), RING_BYTES);
        assert!(bytes.iter().all(|b| *b == b'x'));
        assert_eq!(end, after);
    }

    fn plan(resume: Option<&str>) -> Plan {
        Plan {
            resume: resume.map(str::to_string),
            session_id: "11111111-2222-3333-4444-555555555555".into(),
            account: "main".into(),
            cwd: "C:\\x".into(),
            model: "claude-opus-5-5".into(),
            effort: "high".into(),
            permission_mode: "bypassPermissions".into(),
            settings_path: PathBuf::from("C:\\s\\settings.json"),
            row_id: "row-1".into(),
            api: None,
            todo_tools: false,
            router: false,
            lane: false,
            no_browser: false,
        }
    }

    fn api_plan(no_browser: bool) -> Plan {
        let api = crate::api::ApiInfo { port: 1, token: "t".into(), kb_exe: None };
        Plan { api: Some(api), no_browser, ..plan(None) }
    }

    #[test]
    fn a_no_browser_card_denies_computer_use_and_every_browser() {
        let s = settings_json(&api_plan(true));
        let deny: Vec<&str> = s["permissions"]["deny"].as_array().expect("a deny list").iter().filter_map(Value::as_str).collect();
        for server in ["mcp__computer-use", "mcp__claude-in-chrome", "mcp__Claude_Browser", "mcp__chrome-devtools"] {
            assert!(deny.contains(&server), "denies {server}: {deny:?}");
        }
        assert_eq!(s["permissions"]["allow"], serde_json::json!(["mcp__kanban"]), "the board's tools stay allowed");
        // Another repo's card keeps its browser.
        assert!(settings_json(&api_plan(false))["permissions"].get("deny").is_none());
        // Without the app's API too.
        let bare = settings_json(&Plan { no_browser: true, ..plan(None) });
        assert!(bare["permissions"]["deny"].as_array().is_some_and(|d| d.len() == crate::nobrowser::SERVERS.len()), "{bare}");
    }

    #[test]
    fn a_no_browser_card_gets_no_headless_chrome() {
        let m = mcp_json(&api_plan(true)).unwrap();
        assert!(m["mcpServers"].get(DEVTOOLS).is_none());
        assert!(m["mcpServers"].get("kanban").is_some());
    }

    #[test]
    fn t2_4_new_session_flags() {
        let a = build_args(&plan(None));
        assert_eq!(&a[..2], ["--session-id", "11111111-2222-3333-4444-555555555555"]);
        for (flag, value) in [("--model", "claude-opus-5-5"), ("--effort", "high"), ("--permission-mode", "bypassPermissions")] {
            let i = a.iter().position(|x| x == flag).unwrap();
            assert_eq!(a[i + 1], value);
        }
    }

    fn devtools(args: &[&str]) -> Value {
        serde_json::json!({ "type": "stdio", "command": "cmd", "args": args, "env": {} })
    }

    #[test]
    fn devtools_override_runs_chrome_headless_and_isolated() {
        let cfg = serde_json::json!({ "mcpServers": { "chrome-devtools": devtools(&["/c", "npx", "-y", "chrome-devtools-mcp@latest"]) } });
        let s = headless_devtools(&cfg, "C:\\x").unwrap();
        assert_eq!(s["args"], serde_json::json!(["/c", "npx", "-y", "chrome-devtools-mcp@latest", "--headless", "--isolated"]));
        assert_eq!(s["command"], "cmd");
        assert_eq!(s["type"], "stdio");
    }

    #[test]
    fn devtools_override_keeps_flags_already_set() {
        let cfg = serde_json::json!({ "mcpServers": { "chrome-devtools": devtools(&["chrome-devtools-mcp", "--headless=true"]) } });
        let s = headless_devtools(&cfg, "C:\\x").unwrap();
        assert_eq!(s["args"], serde_json::json!(["chrome-devtools-mcp", "--headless=true", "--isolated"]));
    }

    #[test]
    fn devtools_override_prefers_the_folder_entry() {
        let cfg = serde_json::json!({
            "mcpServers": { "chrome-devtools": devtools(&["user"]) },
            "projects": { "C:/Users/h/web": { "mcpServers": { "chrome-devtools": devtools(&["project"]) } } }
        });
        let s = headless_devtools(&cfg, "C:\\Users\\h\\web").unwrap();
        assert_eq!(s["args"][0], "project");
        assert_eq!(headless_devtools(&cfg, "C:\\Users\\h\\other").unwrap()["args"][0], "user");
    }

    #[test]
    fn no_devtools_server_adds_nothing() {
        assert!(headless_devtools(&serde_json::json!({}), "C:\\x").is_none());
        let http = serde_json::json!({ "mcpServers": { "chrome-devtools": { "type": "http", "url": "http://127.0.0.1:1/" } } });
        assert!(headless_devtools(&http, "C:\\x").is_none());
    }

    #[test]
    fn t2_5_resume_uses_the_conversation_id() {
        let a = build_args(&plan(Some("abc")));
        assert_eq!(&a[..2], ["--resume", "abc"]);
        assert!(!a.contains(&"--session-id".to_string()));
    }

    #[test]
    fn t2_10_environment() {
        let parent = vec![
            ("CLAUDECODE".to_string(), "1".to_string()),
            ("CLAUDE_CODE_ENTRYPOINT".to_string(), "x".to_string()),
            ("CLAUDE_CONFIG_DIR".to_string(), "C:\\old".to_string()),
            ("PATH".to_string(), "C:\\bin".to_string()),
        ];
        let main = build_env(parent.clone().into_iter(), "main", 7, false);
        assert!(main.iter().all(|(k, _)| k != "CLAUDECODE" && k != "CLAUDE_CODE_ENTRYPOINT" && k != "CLAUDE_CONFIG_DIR"));
        assert!(main.contains(&("PATH".into(), "C:\\bin".into())));
        assert!(main.contains(&("KANBAN_CARD".into(), "7".into())));
        let a2 = build_env(parent.clone().into_iter(), "account2", 7, false);
        let dir = a2.iter().find(|(k, _)| k == "CLAUDE_CONFIG_DIR").map(|(_, v)| v.clone()).unwrap();
        assert!(dir.ends_with(".claude-max2"));
    }

    // Card #118, final-test finding F1: a test app's sessions reach only its own board. Every
    // kb they run (the repo's .mcp.json `kb mcp` too) gets KANBAN_DATA_DIR, and the test app's
    // own folder is first on PATH. The live app's sessions get neither, and lose an inherited one.
    #[test]
    fn a_test_apps_sessions_reach_only_its_board() {
        let parent = vec![("Path".to_string(), r"C:\bin".to_string()), ("KANBAN_DATA_DIR".to_string(), r"C:\stale".to_string())];
        let live = build_env_for(parent.clone().into_iter(), "main", 7, false, None);
        assert!(live.iter().all(|(k, _)| k != "KANBAN_DATA_DIR"), "{live:?}");
        assert!(live.contains(&("Path".into(), r"C:\bin".into())));
        let board = Some((PathBuf::from(r"C:\Users\x\.claude-kanban-dsh-test2"), Some(PathBuf::from(r"C:\Users\x\Programs\Claude Kanban DSH Test 2"))));
        let test = build_env_for(parent.into_iter(), "main", 7, false, board);
        let dirs: Vec<_> = test.iter().filter(|(k, _)| k == "KANBAN_DATA_DIR").map(|(_, v)| v.as_str()).collect();
        assert_eq!(dirs, [r"C:\Users\x\.claude-kanban-dsh-test2"]);
        let want = std::env::join_paths([r"C:\Users\x\Programs\Claude Kanban DSH Test 2", r"C:\bin"]).unwrap().into_string().unwrap();
        assert!(test.contains(&("Path".into(), want)), "{test:?}");
        assert_eq!(test.iter().filter(|(k, _)| k.eq_ignore_ascii_case("PATH")).count(), 1);
        // This build's own choice: the live build passes no board.
        if !kanban_store::is_test() {
            let own = build_env(vec![("KANBAN_DATA_DIR".to_string(), "x".to_string())].into_iter(), "main", 7, false);
            assert!(own.iter().all(|(k, _)| k != "KANBAN_DATA_DIR"));
        }
    }

    // Card #106: the to-do tools come back only when Hugh's setting is on; a parent's own copy
    // of the variable never leaks through either way.
    #[test]
    fn the_todo_tools_flag_follows_the_setting() {
        let mut parent = vec![("PATH".to_string(), r"C:\bin".to_string())];
        parent.push(("CLAUDE_CODE_ENABLE_TODO_TOOLS".into(), "1".into()));
        let off = build_env(parent.clone().into_iter(), "main", 7, false);
        assert!(off.iter().all(|(k, _)| k != "CLAUDE_CODE_ENABLE_TODO_TOOLS"));
        let on = build_env(parent.into_iter(), "account2", 7, true);
        assert_eq!(on.iter().filter(|(k, _)| k == "CLAUDE_CODE_ENABLE_TODO_TOOLS").count(), 1);
        assert!(on.contains(&("CLAUDE_CODE_ENABLE_TODO_TOOLS".into(), "1".into())));
    }

    #[test]
    fn t4_1_quit_records_whether_each_session_was_mid_turn() {
        assert_eq!(turn_detail("working", false, false), "mid-turn (working)");
        assert_eq!(turn_detail("permission", false, false), "mid-turn (permission)");
        assert!(turn_detail("idle", true, false).starts_with("mid-turn (a message was typed"));
        assert!(turn_detail("starting", false, true).starts_with("mid-turn (a message was waiting"));
        for idle in ["idle", "starting", "failed", "ended"] {
            assert_eq!(turn_detail(idle, false, false), "idle");
        }
    }

    #[test]
    fn live_switch_commands() {
        assert_eq!(model_command("claude-sonnet-5-5").unwrap(), "/model claude-sonnet-5-5");
        assert_eq!(model_command(" opus[1m] ").unwrap(), "/model opus[1m]");
        assert_eq!(model_command("sonnet").unwrap(), "/model sonnet");
        let long = "m".repeat(65);
        for bad in ["", "  ", "opus\r/exit", "a b", "x;rm", "-p", long.as_str()] {
            assert!(model_command(bad).is_err(), "{bad:?} should be refused");
        }
        assert_eq!(effort_command("high").unwrap(), "/effort high");
        assert_eq!(effort_command(" XHigh ").unwrap(), "/effort xhigh");
        for e in EFFORTS {
            assert_eq!(effort_command(e).unwrap(), format!("/effort {e}"));
        }
        assert!(effort_command("turbo").is_err());
        assert!(effort_command("").is_err());
        // Typed like a pasted message: Ctrl+U, then a bracketed paste, with no CR inside.
        assert_eq!(paste_bytes("/effort low"), "\x15\x1b[200~/effort low\x1b[201~");
        assert_eq!(paste_bytes("a\r\nb"), "\x15\x1b[200~a\nb\x1b[201~");
    }

    #[test]
    fn switch_types_model_then_effort_and_newest_wins() {
        let mut s = Switch::default();
        assert!(s.is_empty() && s.next().is_none());
        s.merge(None, Some("low".into()));
        s.merge(Some("claude-opus-5-5".into()), None);
        s.merge(None, Some("max".into()));
        assert_eq!(s.next(), Some(("model", "claude-opus-5-5".into(), "/model claude-opus-5-5".into())));
        s.model = None;
        assert_eq!(s.next(), Some(("effort", "max".into(), "/effort max".into())));
        s.effort = None;
        assert!(s.is_empty());
    }

    #[test]
    fn hughs_message_is_typed_at_once_unless_the_session_is_asking_or_starting() {
        let ok = TypeCheck { state: "idle", ready: true, alive: true, stopping: false, held: false, quiet: true, sent_ago: None, switch_waiting: false, unconfirmed: false, mid_turn_ok: true, idle_for: Duration::from_secs(1) };
        // Typed at its prompt, mid-turn (even with the screen busy), and after a failed turn.
        assert!(takes_typing(ok));
        assert!(takes_typing(TypeCheck { state: "working", quiet: false, ..ok }));
        assert!(takes_typing(TypeCheck { state: "failed", quiet: false, ..ok }));
        // Never into a dialog (Enter would answer it), a starting or ended session.
        for st in ["permission", "starting", "ended"] {
            assert!(!takes_typing(TypeCheck { state: st, ..ok }), "{st}");
        }
        // Not before the prompt is up, once it died or is being stopped, just after a slash
        // command, or with a model/effort switch to go first.
        assert!(!takes_typing(TypeCheck { ready: false, ..ok }));
        assert!(!takes_typing(TypeCheck { alive: false, ..ok }));
        assert!(!takes_typing(TypeCheck { stopping: true, ..ok }));
        assert!(!takes_typing(TypeCheck { held: true, ..ok }));
        assert!(!takes_typing(TypeCheck { switch_waiting: true, ..ok }));
        // At the prompt, on free()'s terms: the screen still, and no message still in flight.
        assert!(!takes_typing(TypeCheck { quiet: false, ..ok }));
        // Card #154: idle with a background subagent's status line moving (never quiet):
        // Hugh's or the Router's message goes in once the session has been idle 3 s,
        // the app's own text (not mid-turn-ok) still waits for a still screen.
        assert!(takes_typing(TypeCheck { quiet: false, idle_for: Duration::from_secs(3), ..ok }));
        assert!(!takes_typing(TypeCheck { quiet: false, idle_for: Duration::from_secs(2), ..ok }));
        assert!(!takes_typing(TypeCheck { quiet: false, idle_for: Duration::from_secs(60), mid_turn_ok: false, ..ok }));
        assert!(!takes_typing(TypeCheck { sent_ago: Some(Duration::from_secs(5)), ..ok }));
        // Mid-turn: not with the app's own text riding along, nor straight after another.
        let mid = TypeCheck { state: "working", ..ok };
        assert!(!takes_typing(TypeCheck { mid_turn_ok: false, ..mid }));
        assert!(!takes_typing(TypeCheck { sent_ago: Some(Duration::from_millis(300)), ..mid }));
        assert!(takes_typing(TypeCheck { sent_ago: Some(Duration::from_secs(2)), ..mid }));
        // An earlier message not yet taken goes first at the prompt; mid-turn it doesn't matter.
        assert!(!takes_typing(TypeCheck { unconfirmed: true, ..ok }));
        assert!(takes_typing(TypeCheck { unconfirmed: true, ..mid }));
    }

    #[test]
    fn untaken_messages_are_typed_again_then_given_up() {
        let s = Duration::from_secs;
        let c = RetryCheck { since: s(16), since_first: s(16), tries: 1, idle_and_quiet: true, drawn: true, holds: false };
        // Not before RETYPE_AFTER, and only at a quiet idle prompt.
        assert_eq!(retry_step(RetryCheck { since: s(5), ..c }), Retry::Wait);
        assert_eq!(retry_step(RetryCheck { idle_and_quiet: false, ..c }), Retry::Wait);
        assert_eq!(retry_step(c), Retry::Retype);
        assert_eq!(retry_step(RetryCheck { tries: TYPE_TRIES - 1, ..c }), Retry::Retype);
        // After TYPE_TRIES tries it is shown as not taken, never typed a fourth time.
        assert_eq!(retry_step(RetryCheck { tries: TYPE_TRIES, ..c }), Retry::GiveUp);
        assert_eq!(retry_step(RetryCheck { tries: TYPE_TRIES, idle_and_quiet: false, ..c }), Retry::Wait);
    }

    #[test]
    fn a_message_left_in_the_prompt_gets_enter_not_another_copy() {
        // Card #175: the text sat in the prompt and each retype added a copy. Enter alone now.
        let s = Duration::from_secs;
        let c = RetryCheck { since: s(16), since_first: s(16), tries: 1, idle_and_quiet: true, drawn: true, holds: true };
        assert_eq!(retry_step(c), Retry::Enter);
        assert_eq!(retry_step(RetryCheck { tries: TYPE_TRIES - 1, ..c }), Retry::Enter);
        assert_eq!(retry_step(RetryCheck { tries: TYPE_TRIES, ..c }), Retry::GiveUp);
        assert_eq!(retry_step(RetryCheck { since: s(5), ..c }), Retry::Wait);
    }

    #[test]
    fn a_session_that_has_not_drawn_since_the_typing_is_waited_for() {
        // Card #175: after a resume the TUI read its input a minute late, and took the message
        // then; typing it again meanwhile only queued more copies. No tries are spent waiting.
        let s = Duration::from_secs;
        let c = RetryCheck { since: s(16), since_first: s(16), tries: 1, idle_and_quiet: true, drawn: false, holds: false };
        assert_eq!(retry_step(c), Retry::Wait);
        assert_eq!(retry_step(RetryCheck { tries: TYPE_TRIES, since_first: s(120), ..c }), Retry::Wait);
        assert_eq!(retry_step(RetryCheck { since_first: DRAW_WAIT, ..c }), Retry::GiveUp);
        // Once it draws, the usual rule.
        assert_eq!(retry_step(RetryCheck { drawn: true, ..c }), Retry::Retype);
    }

    #[test]
    fn reads_the_message_in_the_prompt_box() {
        let rule = "─".repeat(60);
        let screen = |typed: &str| format!("● OK\n\n✻ Churned for 1s\n{rule}\n{typed}\n{rule}\n  #174 · Opus 5.5\n");
        // As Claude Code 2.1.321 showed them (card #175): a short paste as itself, soft-wrapped.
        let short = screen("❯ Short one. Reply with just OK, and nothing\n  else at all.\n  Second line here.");
        assert_eq!(prompt_text(&short).as_deref(), Some("Short one. Reply with just OK, and nothing else at all. Second line here."));
        assert!(prompt_holds(&short, "Short one. Reply with just OK, and nothing else at all.\nSecond line here."));
        assert!(!prompt_holds(&short, "Another message entirely"));
        // A long one as a placeholder; but a placeholder isn't taken for a short message (Hugh's
        // own paste, say), and two placeholders or two copies would send twice: not held.
        assert!(prompt_holds(&screen("❯ [Pasted text #2 +60 lines]"), "Long one.\nline\nline"));
        assert!(!prompt_holds(&screen("❯ [Pasted text #2 +60 lines]"), "Short one."));
        assert!(!prompt_holds(&screen("❯ [Pasted text #2 +60 lines][Pasted text #3 +60 lines]"), "Long one.\nline\nline"));
        assert!(!prompt_holds(&screen("❯ Short one. Short one."), "Short one."));
        // Waiting text taken after the give-up clears "Not sent" (`hook`): same opening.
        assert!(same_start("Short one.\n  more", "Short  one. more"));
        assert!(!same_start("Short one.", "Other"));
        assert!(!same_start("", ""));
        // An empty prompt, or no prompt box at all, holds nothing.
        assert_eq!(prompt_text(&screen("❯")).as_deref(), Some(""));
        assert!(!prompt_holds(&screen("❯"), "Short one."));
        assert!(!prompt_holds("no box here", "Short one."));
        // The message in the transcript above the box doesn't count.
        let above = format!("❯ Short one.\n\n● OK\n{rule}\n❯\n{rule}\n");
        assert!(!prompt_holds(&above, "Short one."));
    }

    #[test]
    fn a_retype_clears_every_line_of_an_earlier_copy() {
        // Ctrl+U clears one line, Backspace joins the line above: one pair per line, plus two.
        let msg = "line one\nline two\n\nline four";
        let b = retype_bytes(msg);
        assert!(b.starts_with(&"\x15\x7f".repeat(6)));
        assert!(b.ends_with(&paste_bytes(msg)));
        assert_eq!(b.len(), 12 + paste_bytes(msg).len());
        assert!(retype_bytes("hi").starts_with("\x15\x7f\x15\x7f\x15\x7f\x15"));
    }

    #[test]
    fn sees_the_prompt_box_but_not_a_picker() {
        let rule = "─".repeat(60);
        let prompt = format!("● Done.\n\n{rule}\n❯\n{rule}\n  #152 · Opus 5.5");
        assert!(prompt_showing(&prompt));
        // Something typed, over two lines.
        assert!(prompt_showing(&format!("{rule}\n❯ first line\n  second line\n{rule}\n  status")));
        // The /model picker: its choice is marked "❯" too, but it isn't the prompt.
        let picker = format!("{rule}\n Select model\n\n❯ 1. Default (recommended)\n  2. Opus\n{rule}");
        assert!(!prompt_showing(&picker));
        assert!(!prompt_showing(&format!("{rule}\n❯ 1. Default\n{rule}")));
        // A /config screen, no prompt at all.
        assert!(!prompt_showing("Settings\n  Theme   Dark\nEsc to close"));
    }

    #[test]
    fn slash_commands_are_not_kept_for_retyping() {
        // Local commands send no UserPromptSubmit, so they would be typed again and again.
        assert!(!confirms("/model opus"));
        assert!(!confirms("  /compact"));
        assert!(confirms("Dry run: say what you would do."));
        assert!(confirms("use a/b paths"));
    }

    fn qs(spec: &[(bool, usize)]) -> Value {
        Value::Array(spec.iter().map(|&(multi, n)| serde_json::json!({
            "question": "?", "multiSelect": multi,
            "options": (0..n).map(|i| serde_json::json!({ "label": format!("o{i}") })).collect::<Vec<_>>(),
        })).collect())
    }
    fn pick(p: &[usize]) -> Answer {
        Answer { picks: p.to_vec(), text: None }
    }
    fn said(t: &str) -> Answer {
        Answer { picks: vec![], text: Some(t.into()) }
    }

    #[test]
    fn a_turn_ended_in_the_transcript_ends_it_here_only_if_later() {
        let t = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&chrono::Utc);
        let asked = t("2026-11-05T10:13:17Z");
        // Card #53: a permission prompt at 10:13, turned down at 10:36 with no hook.
        assert!(ended_by("permission", asked, t("2026-11-05T10:36:40.537Z")));
        assert!(ended_by("working", asked, t("2026-11-05T10:36:40Z")));
        // The end of the turn before this one's prompt doesn't count.
        assert!(!ended_by("working", asked, t("2026-11-05T10:13:00Z")));
        // Nothing to end when it isn't on a turn.
        assert!(!ended_by("idle", asked, t("2026-11-05T10:36:40Z")));
        assert!(!ended_by("starting", asked, t("2026-11-05T10:36:40Z")));
    }

    #[test]
    fn answer_keys_as_the_dialog_takes_them() {
        // One single-select question: its digit answers it, with no review screen.
        assert_eq!(answer_keys(&qs(&[(false, 3)]), &[pick(&[1])]).unwrap(), ["2"]);
        // Own words: "Type something" (the digit after the options), the paste, Enter.
        assert_eq!(answer_keys(&qs(&[(false, 2)]), &[said(" Sushi\r\nplease ")]).unwrap(), ["3", "\x1b[200~Sushi\nplease\x1b[201~", "\r"]);
        // Control characters never reach the paste, so it can't be ended early.
        assert_eq!(answer_keys(&qs(&[(false, 2)]), &[said("a\x1b[201~\r1\x07b")]).unwrap()[1], "\x1b[200~a[201~1b\x1b[201~");
        // Single then multi-select: a digit, ticks, Right, then Submit on the review screen.
        assert_eq!(answer_keys(&qs(&[(false, 3), (true, 3)]), &[pick(&[0]), pick(&[2, 0, 2])]).unwrap(), ["1", "1", "3", "\x1b[C", "1"]);
        // A lone multi-select question still has the review screen.
        assert_eq!(answer_keys(&qs(&[(true, 3)]), &[pick(&[1])]).unwrap(), ["2", "\x1b[C", "1"]);
        // Two single-select questions: the review screen too.
        assert_eq!(answer_keys(&qs(&[(false, 2), (false, 2)]), &[pick(&[1]), pick(&[0])]).unwrap(), ["2", "1", "1"]);
    }

    #[test]
    fn answer_keys_refuse_what_the_dialog_cant_take() {
        let one = qs(&[(false, 3)]);
        assert!(answer_keys(&one, &[]).is_err(), "an answer per question");
        assert!(answer_keys(&one, &[pick(&[3])]).is_err(), "out of range");
        assert!(answer_keys(&one, &[pick(&[0, 1])]).is_err(), "two picks for a single-select");
        assert!(answer_keys(&one, &[pick(&[])]).is_err(), "no pick");
        assert!(answer_keys(&one, &[said("   ")]).is_err(), "blank words");
        assert!(answer_keys(&one, &[Answer { picks: vec![0], text: Some("x".into()) }]).is_err(), "both");
        let multi = qs(&[(true, 3)]);
        assert!(answer_keys(&multi, &[pick(&[])]).is_err(), "no ticks");
        assert!(answer_keys(&multi, &[said("x")]).is_err(), "own words on a multi-select");
        assert!(answer_keys(&qs(&[(false, 8)]), &[pick(&[0])]).is_err(), "more options than digits");
        assert!(answer_keys(&Value::Null, &[pick(&[0])]).is_err());
    }

    #[test]
    fn a_question_dialog_counts_as_needing_hugh() {
        let q = serde_json::json!({ "tool_name": "AskUserQuestion" });
        assert_eq!(hook_state("PreToolUse", &q), Some("permission"));
        assert_eq!(hook_state("PostToolUse", &q), Some("working"));
        // Other tools' hooks leave the state alone.
        assert_eq!(hook_state("PreToolUse", &serde_json::json!({ "tool_name": "Bash" })), None);
        let n = |kind: &str, msg: &str| serde_json::json!({ "notification_type": kind, "message": msg });
        assert_eq!(hook_state("Notification", &n("permission_prompt", "Claude needs your permission to use Bash")), Some("permission"));
        assert_eq!(hook_state("Notification", &n("elicitation_dialog", "")), Some("permission"));
        assert_eq!(hook_state("Notification", &n("idle_prompt", "Claude is waiting for your input")), Some("idle"));
        // Older Claude Code: no notification_type, the message decides.
        assert_eq!(hook_state("Notification", &serde_json::json!({ "message": "Claude needs your permission to use Bash" })), Some("permission"));
        assert_eq!(hook_state("Notification", &serde_json::json!({ "message": "auth ok" })), None);
        assert_eq!(hook_state("Stop", &Value::Null), Some("idle"));
        assert_eq!(hook_state("StopFailure", &Value::Null), Some("failed"));
        assert_eq!(hook_state("UserPromptSubmit", &Value::Null), Some("working"));
    }

    #[test]
    fn settings_hook_the_question_and_testing_tools_only() {
        let mut p = plan(None);
        p.api = Some(crate::api::ApiInfo { port: 47821, token: "t".into(), kb_exe: None });
        let v = settings_json(&p);
        for ev in ["PreToolUse", "PostToolUse"] {
            assert_eq!(v["hooks"][ev][0]["matcher"], "AskUserQuestion|Agent|Task|Skill", "{ev}");
            assert_eq!(v["hooks"][ev][0]["hooks"][0]["url"], format!("http://127.0.0.1:47821/hook/t/row-1/{ev}"));
        }
        // The shell guard (card #100): a second PreToolUse entry, to its own path.
        assert_eq!(v["hooks"]["PreToolUse"][1]["matcher"], "Bash|PowerShell");
        assert_eq!(v["hooks"]["PreToolUse"][1]["hooks"][0]["url"], "http://127.0.0.1:47821/hook/t/row-1/guard");
        assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
        assert_eq!(v["hooks"]["PostToolUse"].as_array().unwrap().len(), 1);
        assert!(v["hooks"]["Stop"][0].get("matcher").is_none());
        assert!(v["hooks"]["SubagentStop"][0].get("matcher").is_none());
        assert_eq!(v["hooks"]["SubagentStop"][0]["hooks"][0]["url"], "http://127.0.0.1:47821/hook/t/row-1/SubagentStop");
    }

    #[test]
    fn card_sessions_get_the_apps_subagents() {
        assert!(!build_args(&plan(None)).contains(&"--agents".to_string()));
        let mut p = plan(None);
        p.api = Some(crate::api::ApiInfo { port: 47821, token: "t".into(), kb_exe: None });
        let a = build_args(&p);
        let i = a.iter().position(|x| x == "--agents").unwrap();
        let v: Value = serde_json::from_str(&a[i + 1]).unwrap();
        assert!(v["card-tester"]["prompt"].is_string());
        assert!(v["card-reviewer"]["prompt"].is_string());
        assert_eq!(v["card-reviewer"]["effort"], "high");
        assert!(v["card-reproducer"]["prompt"].as_str().unwrap().contains("VERDICT: REPRODUCED"));
        assert!(v["card-reproducer"].get("effort").is_none(), "the reproducer runs at the session's effort");
        assert!(v["card-merger"]["prompt"].is_string());
    }

    // Windows' command line holds 32,767 characters, and every agent's prompt rides on it (card #96).
    #[test]
    fn a_sessions_command_line_stays_well_under_the_windows_limit() {
        let mut p = plan(Some("0b2c9a44-5d7e-4c1a-9f3e-2a6b8c0d1e2f"));
        p.api = Some(crate::api::ApiInfo { port: 47821, token: "t".into(), kb_exe: None });
        p.settings_path = PathBuf::from(r"C:\Users\someone-with-a-long-name\.claude-kanban-dsh-test\sessions\card-123-0b2c9a44-5d7e-4c1a-9f3e-2a6b8c0d1e2f\settings.json");
        // Each argument as Windows quotes it: its own quotes around it and a space, and every `"`
        // or `\` inside it escaped with a `\` (the `--agents` JSON is full of both).
        let len: usize = build_args(&p).iter().map(|a| a.len() + 3 + a.chars().filter(|c| *c == '"' || *c == '\\').count()).sum();
        assert!(len < 30_000, "the command line is {len} characters");
        // The Router's (card #119): its rules as a system prompt and its own agents too.
        p.router = true;
        let args = build_args(&p);
        let len: usize = args.iter().map(|a| a.len() + 3 + a.chars().filter(|c| *c == '"' || *c == '\\').count()).sum();
        assert!(len < 30_000, "the Router's command line is {len} characters");
        let i = args.iter().position(|a| a == "--append-system-prompt").expect("the Router's rules go in as its system prompt");
        assert_eq!(args[i + 1], crate::routersession::PROMPT);
        assert!(args.iter().any(|a| a.contains("claude-sonnet-5-5")), "its subagents run on Sonnet");
        // Its guard sees the editing tools as well as the shells.
        let s = settings_json(&p);
        let pre = s["hooks"]["PreToolUse"].as_array().unwrap();
        assert!(pre.iter().any(|e| e["matcher"] == crate::guard::ROUTER_TOOLS));
        p.router = false;
        assert!(!settings_json(&p)["hooks"]["PreToolUse"].as_array().unwrap().iter().any(|e| e["matcher"] == crate::guard::ROUTER_TOOLS));
    }

    /// Card #160: a merge lane's command line has its rules as the system prompt, no subagents,
    /// no AskUserQuestion, and its guard sees the edits too.
    #[test]
    fn a_lanes_command_line_has_its_rules_no_agents_and_no_questions() {
        let mut p = plan(Some("0b2c9a44-5d7e-4c1a-9f3e-2a6b8c0d1e2f"));
        p.api = Some(crate::api::ApiInfo { port: 47821, token: "t".into(), kb_exe: None });
        p.lane = true;
        let args = build_args(&p);
        let i = args.iter().position(|a| a == "--append-system-prompt").expect("the lane's rules go in as its system prompt");
        assert_eq!(args[i + 1], crate::lane::PROMPT);
        assert!(!args.iter().any(|a| a == "--agents"), "no subagents");
        let j = args.iter().position(|a| a == "--disallowedTools").expect("a deny list");
        assert_eq!(args[j + 1], "AskUserQuestion");
        let len: usize = args.iter().map(|a| a.len() + 3 + a.chars().filter(|c| *c == '"' || *c == '\\').count()).sum();
        assert!(len < 30_000, "the lane's command line is {len} characters");
        let pre = settings_json(&p)["hooks"]["PreToolUse"].as_array().unwrap().clone();
        assert!(pre.iter().any(|e| e["matcher"] == crate::guard::ROUTER_TOOLS));
        // The app's own keys.
        assert!(is_app_key("router") && is_app_key("lane:acme/web"));
        assert!(!is_app_key("card-12") && !is_app_key("lane:"));
    }

    /// Card #117, router decision R2: on a Mac an account-2 card refuses, saying why.
    #[test]
    fn account_2_refuses_where_it_isnt_supported() {
        assert_eq!(account_runs_here("account2", true), Ok(()));
        assert_eq!(account_runs_here("main", false), Ok(()));
        let e = account_runs_here("account2", false).unwrap_err();
        assert!(e.starts_with("Account 2 isn't supported on a Mac"), "{e}");
        assert_eq!(account_runs_here("account2", crate::platform::SECOND_ACCOUNT).is_ok(), cfg!(windows));
    }

    #[test]
    fn defaults_follow_the_plan() {
        assert_eq!(column_defaults("Blocked"), ("claude-sonnet-5-5", "medium"));
        assert_eq!(column_defaults("In progress"), ("claude-opus-5-5", "high"));
    }

    #[test]
    fn a_turn_keeps_its_start_until_it_ends() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(30);
        // Going to work starts a turn; a mid-turn message or a question keeps its start.
        assert_eq!(turn_start(None, "working", t0), Some(t0));
        assert_eq!(turn_start(Some(t0), "working", t1), Some(t0));
        assert_eq!(turn_start(Some(t0), "permission", t1), Some(t0));
        // A question with no turn yet still counts from now.
        assert_eq!(turn_start(None, "permission", t1), Some(t1));
        // Idle, failed, ended or starting: no turn.
        for s in ["idle", "failed", "ended", "starting"] {
            assert_eq!(turn_start(Some(t0), s, t1), None);
        }
    }

    /// `Waits::hook` as `Sessions::hook` calls it: the state from `hook_state`.
    fn wait_hook(w: &mut Waits, event: &str, body: &Value, asking: bool) -> bool {
        let state = hook_state(event, body).unwrap();
        w.hook(event, state, body, asking)
    }

    #[test]
    fn waits_keep_a_prompt_and_a_failure_until_the_state_leaves() {
        let mut w = Waits::default();
        let prompt = serde_json::json!({ "notification_type": "permission_prompt", "message": "Claude needs your permission to use Bash", "transcript_path": "x.jsonl" });
        assert!(wait_hook(&mut w, "Notification", &prompt, false), "a new prompt asks for its call to be read");
        assert_eq!((w.prompt.as_deref(), w.seq), (Some("Claude needs your permission to use Bash"), 1));
        w.call = Some(crate::transcript::OpenCall { id: "b".into(), text: "Bash: ls".into(), path: "x.jsonl".into(), sub: false });
        // The same prompt again is a new one: its call is read afresh.
        assert!(wait_hook(&mut w, "Notification", &prompt, false));
        assert_eq!((w.call.as_ref(), w.seq), (None, 2));
        // Answered: back to work, and the prompt goes.
        wait_hook(&mut w, "UserPromptSubmit", &Value::Null, false);
        assert_eq!(w, Waits { seq: 2, ..Default::default() });
        // A question dialog's own notice keeps no prompt: the question says it.
        assert!(!wait_hook(&mut w, "Notification", &prompt, true));
        let ask = serde_json::json!({ "notification_type": "permission_prompt", "message": "Claude needs your permission to use AskUserQuestion" });
        assert!(!wait_hook(&mut w, "Notification", &ask, false));
        assert_eq!(w.prompt, None);
        // An elicitation has a message but no tool: kept, with nothing to read.
        let el = serde_json::json!({ "notification_type": "elicitation_dialog", "message": "The server asks for a name" });
        assert!(!wait_hook(&mut w, "Notification", &el, false));
        assert_eq!(w.prompt.as_deref(), Some("The server asks for a name"));
        // A failure keeps its error text until the state leaves "failed".
        wait_hook(&mut w, "StopFailure", &serde_json::json!({ "error": "rate_limit", "error_details": "5-hour limit reached" }), false);
        assert_eq!((w.failure.as_deref(), w.prompt.as_deref()), (Some("rate limit: 5-hour limit reached"), None));
        wait_hook(&mut w, "UserPromptSubmit", &Value::Null, false);
        assert_eq!(w.failure, None);
        wait_hook(&mut w, "StopFailure", &serde_json::json!({}), false);
        assert_eq!(w.failure, None, "no error text, none kept");
        // A turn ended with no hook clears both.
        w.prompt = Some("p".into());
        w.failure = Some("f".into());
        w.left("idle");
        assert_eq!((w.prompt, w.failure), (None, None));
    }

    #[test]
    fn a_failures_text_comes_from_its_error_fields() {
        let f = |v: Value| failure_text(&v);
        assert_eq!(f(serde_json::json!({ "error": "server_error" })).as_deref(), Some("server error"));
        assert_eq!(f(serde_json::json!({ "error_details": "overloaded" })).as_deref(), Some("overloaded"));
        assert_eq!(f(serde_json::json!({ "message": "boom" })).as_deref(), Some("boom"));
        assert_eq!(f(serde_json::json!({ "error": "  ", "last_assistant_message": "x" })), None);
    }
}
