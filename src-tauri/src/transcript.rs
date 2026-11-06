//! Reading a Claude Code transcript (`<config dir>\projects\<folder>\<session id>.jsonl`)
//! into the entries the card panel's chat view shows.

use serde::Serialize;
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, PartialEq, Default)]
pub struct Entry {
    /// "user", "assistant", "tool", "cmdout" (what a slash command printed), or "result" (a tool's result whose call came in an
    /// earlier chunk: the panel pairs it by `tool_id`).
    pub kind: &'static str,
    /// For a tool: its name, then a tab and a one-line detail.
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    /// The `tool_use` id that pairs a tool call with its result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
    /// A tool's key input (the command, path, pattern...), at most `MAX_INPUT` bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<ToolOutput>,
    /// For an `AskUserQuestion` call: its questions (`ask_questions`), for the panel's
    /// question card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ask: Option<Value>,
    /// Local image files the entry shows: a `Read` of a screenshot carries its path, for the
    /// panel to draw (through `image_read`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
}

/// The media type of an image the chat can draw, by the path's extension. The one list of them
/// (the panel's `IMG_EXT` matches it).
pub fn image_mime(path: &str) -> Option<&'static str> {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => return None,
    })
}

pub fn is_image_path(path: &str) -> bool {
    image_mime(path).is_some()
}

/// A tool's result, cut to `MAX_OUTPUT` bytes so a chunk stays small.
#[derive(Debug, Serialize, PartialEq, Clone)]
pub struct ToolOutput {
    pub text: String,
    /// Lines in the whole result, before it was cut.
    pub lines: usize,
    pub truncated: bool,
    pub is_error: bool,
}

pub const MAX_OUTPUT: usize = 4096;
pub const MAX_INPUT: usize = 2048;

#[derive(Debug, Serialize)]
pub struct Chunk {
    pub entries: Vec<Entry>,
    /// Where to read from next time.
    pub offset: u64,
    pub path: Option<String>,
    /// The chunk's latest thinking block, for the panel's "thought for 4s".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought: Option<Thought>,
}

/// A thinking block: when it was written, and how long the model thought, as the time from the
/// transcript line before it (the prompt or tool result it answered) to this one. Claude Code
/// records thinking blocks with their text left out, so the time is all there is to show.
#[derive(Debug, Serialize, PartialEq, Clone)]
pub struct Thought {
    pub at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
}

/// A main-conversation message's own timestamp: a prompt, a tool result or a reply. A
/// subagent's lines and Claude Code's bookkeeping (queue operations, attachments, system
/// lines) don't count, so a message typed mid-turn doesn't cut a thinking time short.
fn line_at(v: &Value) -> Option<&str> {
    if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    if !matches!(v.get("type").and_then(Value::as_str), Some("user" | "assistant")) {
        return None;
    }
    v.get("timestamp").and_then(Value::as_str)
}

fn is_thinking(v: &Value) -> bool {
    v.get("type").and_then(Value::as_str) == Some("assistant")
        && v.pointer("/message/content").and_then(Value::as_array).map_or(false, |bs| {
            bs.iter().any(|b| matches!(b.get("type").and_then(Value::as_str), Some("thinking" | "redacted_thinking")))
        })
}

/// Milliseconds from `a` to `b` (RFC 3339 times), None if either won't parse or `b` is earlier.
fn ms_between(a: &str, b: &str) -> Option<u64> {
    let a = chrono::DateTime::parse_from_rfc3339(a).ok()?;
    let b = chrono::DateTime::parse_from_rfc3339(b).ok()?;
    u64::try_from((b - a).num_milliseconds()).ok()
}

/// The latest thinking block in `lines`, timed from the main-conversation line before it.
/// `before` is the timestamp of the last line ahead of these (from an earlier read).
pub fn latest_thought<'a>(lines: impl Iterator<Item = &'a str>, before: Option<String>) -> Option<Thought> {
    let mut prev = before;
    let mut found = None;
    for l in lines {
        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
        let Some(at) = line_at(&v) else { continue };
        if is_thinking(&v) {
            found = Some(Thought { at: at.to_string(), ms: prev.as_deref().and_then(|p| ms_between(p, at)) });
        }
        prev = Some(at.to_string());
    }
    found
}

/// The timestamp of the last main-conversation line that ends before `start`, looking back at
/// most 1 MB (a tool result's line can be long).
fn last_at_before(f: &mut std::fs::File, start: u64) -> Option<String> {
    const STEP: u64 = 256 * 1024;
    let mut back = 0u64;
    while back < start && back < 4 * STEP {
        back = (back + STEP).min(start);
        let from = start - back;
        f.seek(SeekFrom::Start(from)).ok()?;
        let mut buf = vec![0u8; back as usize];
        f.read_exact(&mut buf).ok()?;
        let text = String::from_utf8_lossy(&buf);
        // Away from the file's start, the window's first line may be cut: skip it.
        let body = if from == 0 { &text[..] } else { text.split_once('\n').map_or("", |x| x.1) };
        let at = body.lines().rev().find_map(|l| serde_json::from_str::<Value>(l).ok().and_then(|v| line_at(&v).map(str::to_string)));
        if at.is_some() {
            return at;
        }
    }
    None
}

pub fn config_dir(account: &str) -> PathBuf {
    let home = kanban_store::home_var().unwrap_or_default();
    if account == "account2" {
        home.join(".claude-max2")
    } else {
        home.join(".claude")
    }
}

/// The folders under an account's `projects` folder, one per working folder.
fn project_dirs(account: &str) -> Vec<PathBuf> {
    std::fs::read_dir(config_dir(account).join("projects")).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default()
}

/// Finds `<id>.jsonl` under the account's projects folder, then under the other account's.
pub fn find(account: &str, id: &str) -> Option<PathBuf> {
    let other = if account == "account2" { "main" } else { "account2" };
    for acct in [account, other] {
        for d in project_dirs(acct) {
            let p = d.join(format!("{id}.jsonl"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Every main transcript under an account's projects folder, by session id: one listing, for
/// looking up many ids at once (the search index's sweep).
pub fn all_by_id(account: &str) -> std::collections::HashMap<String, PathBuf> {
    let mut out = std::collections::HashMap::new();
    for d in project_dirs(account) {
        let Ok(files) = std::fs::read_dir(&d) else { continue };
        for f in files.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            if let Some(id) = name.strip_suffix(".jsonl") {
                out.entry(id.to_string()).or_insert_with(|| f.path());
            }
        }
    }
    out
}

/// The working folder the transcript's session ran in, from its first entry that records one.
pub fn cwd_of(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find_map(|v| v.get("cwd").and_then(Value::as_str).map(str::to_string))
}

/// Reads whole lines from `offset` on.
pub fn read(path: &Path, offset: u64) -> std::io::Result<Chunk> {
    read_in(path, offset, false)
}

/// A subagent the session ran (the Agent tool, or a forked skill such as `/code-review`), from
/// Claude Code's `<session id>\subagents\agent-<id>.meta.json` beside its transcript.
#[derive(Debug, Serialize, PartialEq, Clone)]
pub struct Subagent {
    pub id: String,
    /// "card-tester", "general-purpose", or a forked skill's name.
    pub agent_type: String,
    pub description: String,
    /// The Agent call in the main conversation that started it, when there was one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
    /// Claude Code marks it finished once it has reported back.
    pub finished: bool,
    /// Someone stopped it (Claude Code's `stoppedByUser`): it won't report, and nothing waits on it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stopped: bool,
    /// Its first line's time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started: Option<String>,
    /// When its transcript was last written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
}

/// The folder of the subagents of the session whose transcript is `main`.
pub fn subagents_dir(main: &Path) -> PathBuf {
    main.with_extension("").join("subagents")
}

/// A subagent id as Claude Code makes them (letters and digits): anything else is refused, so
/// an id from the panel can't reach outside the subagents folder.
pub fn valid_agent_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The transcript of subagent `id` of the session at `main`, if it is there.
pub fn subagent_path(main: &Path, id: &str) -> Option<PathBuf> {
    if !valid_agent_id(id) {
        return None;
    }
    let p = subagents_dir(main).join(format!("agent-{id}.jsonl"));
    p.is_file().then_some(p)
}

/// When a subagent started: the first timestamp in its transcript's first 256 KB, else the
/// file's creation time. Kept per path once found, since it never changes: the panel asks
/// every 3 s.
fn first_at(path: &Path) -> Option<String> {
    static KNOWN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<PathBuf, String>>> = std::sync::OnceLock::new();
    let known = KNOWN.get_or_init(Default::default);
    if let Some(at) = known.lock().ok().and_then(|k| k.get(path).cloned()) {
        return Some(at);
    }
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 256 * 1024];
    let n = f.read(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf[..n]);
    let at = text.lines().find_map(|l| serde_json::from_str::<Value>(l).ok().and_then(|v| v.get("timestamp").and_then(Value::as_str).map(str::to_string)));
    match at {
        Some(at) => {
            if let Ok(mut k) = known.lock() {
                k.insert(path.to_path_buf(), at.clone());
            }
            Some(at)
        }
        // A first line too long to read here, or none written yet: the file's own time, not kept.
        None => f.metadata().and_then(|m| m.created()).ok().map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339()),
    }
}

/// A subagent's type from its `.meta.json`: its name, else its agent type, else "subagent".
pub fn agent_type(meta: &Value) -> String {
    let s = |k: &str| meta.get(k).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_string);
    s("name").or(s("agentType")).unwrap_or_else(|| "subagent".into())
}

/// The session's subagents as Claude Code records them: each one's id, transcript and
/// `.meta.json`, without reading the transcripts. One with no transcript yet is left out. The
/// panel's list (`subagents`) and the search index both use it, so they see the same ones.
pub fn subagent_files(main: &Path) -> Vec<(String, PathBuf, Value)> {
    let Ok(dir) = std::fs::read_dir(subagents_dir(main)) else { return vec![] };
    let mut out = Vec::new();
    for d in dir.flatten() {
        let name = d.file_name().to_string_lossy().to_string();
        let Some(id) = name.strip_prefix("agent-").and_then(|n| n.strip_suffix(".meta.json")) else { continue };
        let Some(path) = subagent_path(main, id) else { continue };
        let meta: Value = std::fs::read_to_string(d.path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        out.push((id.to_string(), path, meta));
    }
    out
}

/// The session's subagents, oldest first. One with no transcript yet is left out.
pub fn subagents(main: &Path) -> Vec<Subagent> {
    let mut out = Vec::new();
    for (id, path, meta) in subagent_files(main) {
        let s = |k: &str| meta.get(k).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_string);
        let updated = std::fs::metadata(&path).and_then(|m| m.modified()).ok().map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
        out.push(Subagent {
            agent_type: agent_type(&meta),
            id,
            description: s("description").unwrap_or_default(),
            tool_id: s("toolUseId"),
            finished: meta.get("finished").and_then(Value::as_bool).unwrap_or(false),
            stopped: meta.get("stoppedByUser").and_then(Value::as_bool).unwrap_or(false),
            started: first_at(&path),
            updated,
        });
    }
    out.sort_by(|a, b| (a.started.as_deref(), &a.id).cmp(&(b.started.as_deref(), &b.id)));
    out
}

/// `read`, for a main transcript (`sub` false) or a subagent's, whose lines are all sidechain.
pub fn read_in(path: &Path, offset: u64, sub: bool) -> std::io::Result<Chunk> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let start = if offset > len { 0 } else { offset };
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    // Stop at the last complete line; a line still being written is read next time.
    let end = buf.iter().rposition(|b| *b == b'\n').map(|i| i + 1).unwrap_or(0);
    let text = String::from_utf8_lossy(&buf[..end]);
    let entries = pair(text.lines().flat_map(|l| parse_line_in(l, sub)).collect());
    // Only look back for the line before when this chunk has thinking to time. A subagent's
    // thinking isn't timed: the busy line belongs to the main conversation.
    let thought = if sub {
        None
    } else if text.contains(r#""type":"thinking""#) || text.contains(r#""type":"redacted_thinking""#) {
        let before = if start > 0 { last_at_before(&mut f, start) } else { None };
        latest_thought(text.lines(), before)
    } else {
        None
    };
    Ok(Chunk { entries, offset: start + end as u64, path: Some(path.display().to_string()), thought })
}

/// The text of a line that is only a slash command's output (`<local-command-stdout>` or
/// `-stderr`), with its colour codes taken out. None for anything else, and for empty output.
fn command_output(s: &str) -> Option<String> {
    let t = s.trim();
    let inner = ["local-command-stdout", "local-command-stderr"].iter().find_map(|tag| {
        let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
        t.strip_prefix(open.as_str())?.strip_suffix(close.as_str()).map(str::to_string)
    })?;
    let mut out = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // CSI: ESC [ params final-byte.
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&n) = chars.peek() {
                    chars.next();
                    if ('@'..='~').contains(&n) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    let out = out.trim().to_string();
    (!out.is_empty()).then_some(out)
}

fn strip_wrappers(s: &str) -> String {
    let mut out = s.to_string();
    for tag in ["system-reminder", "ide_opened_file", "ide_selection", "local-command-stdout", "local-command-stderr", "local-command-caveat"] {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        while let Some(i) = out.find(&open) {
            match out[i..].find(&close) {
                Some(j) => out.replace_range(i..i + j + close.len(), ""),
                None => break,
            }
        }
    }
    // Long text typed into the TUI is recorded inside a pasted_content tag: keep the text only.
    while let Some(i) = out.find("<pasted_content") {
        match out[i..].find('>') {
            Some(j) => out.replace_range(i..i + j + 1, ""),
            None => break,
        }
    }
    out = out.replace("</pasted_content>", "");
    // A slash command reads as the command itself.
    if let (Some(a), Some(b)) = (out.find("<command-name>"), out.find("</command-name>")) {
        let name = out[a + 14..b].trim().to_string();
        let args = match (out.find("<command-args>"), out.find("</command-args>")) {
            (Some(c), Some(d)) => out[c + 14..d].trim().to_string(),
            _ => String::new(),
        };
        return format!("{name} {args}").trim().to_string();
    }
    out.trim().to_string()
}

fn tool_detail(input: &Value) -> String {
    let pick = ["description", "command", "file_path", "pattern", "url", "query", "prompt", "skill"]
        .iter()
        .find_map(|k| input.get(*k).and_then(Value::as_str));
    let s = pick.unwrap_or("");
    let one_line = s.lines().next().unwrap_or("");
    if one_line.chars().count() > 120 {
        format!("{}…", one_line.chars().take(120).collect::<String>())
    } else {
        one_line.to_string()
    }
}

/// When the transcript's latest turn ended: the timestamp of its last `system` /
/// `turn_duration` line, if no prompt, reply or tool call of the main conversation came after
/// it. Claude Code writes that line at the end of every turn, an interrupted one too (Esc, or a
/// permission prompt or question turned down), where no Stop or PostToolUse hook is sent.
pub fn turn_ended_at<'a>(lines: impl DoubleEndedIterator<Item = &'a str>) -> Option<String> {
    for line in lines.rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        match v.get("type").and_then(Value::as_str) {
            Some("system") if v.get("subtype").and_then(Value::as_str) == Some("turn_duration") => {
                return v.get("timestamp").and_then(Value::as_str).map(str::to_string);
            }
            // Claude Code's note of an interrupt ends the turn too, in case no turn_duration
            // line follows it.
            Some("user") if interrupted(&v) => return v.get("timestamp").and_then(Value::as_str).map(str::to_string),
            Some("user" | "assistant") => return None,
            _ => {}
        }
    }
    None
}

/// Claude Code's "[Request interrupted by user]" line (also "... for tool use").
fn interrupted(v: &Value) -> bool {
    let text = |t: &str| t.starts_with("[Request interrupted by user");
    match v.get("message").and_then(|m| m.get("content")) {
        Some(Value::String(s)) => text(s),
        Some(Value::Array(b)) => b.iter().any(|b| b.get("type").and_then(Value::as_str) == Some("text") && b.get("text").and_then(Value::as_str).is_some_and(text)),
        _ => false,
    }
}

/// `turn_ended_at` for the transcript at `path`, reading only its last 64 KB.
pub fn turn_ended_in(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(64 * 1024);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    // The first line may be cut, unless the read started at the top.
    let body = if start == 0 { &text[..] } else { text.split_once('\n').map_or("", |(_, rest)| rest) };
    turn_ended_at(body.lines())
}

/// Claude Code's question tool: its dialog asks Hugh one to four questions.
pub const ASK_TOOL: &str = "AskUserQuestion";

/// An `AskUserQuestion` call's questions, kept to what the panel shows and what answering
/// needs: `[{header, question, multiSelect, options: [{label, description}]}]`. None when
/// the input has no question with options.
pub fn ask_questions(input: &Value) -> Option<Value> {
    let qs = input.get("questions")?.as_array()?;
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let out: Vec<Value> = qs
        .iter()
        .map(|q| {
            let options: Vec<Value> = q
                .get("options")
                .and_then(Value::as_array)
                .map(|o| o.iter().map(|o| serde_json::json!({ "label": s(o, "label"), "description": s(o, "description") })).collect())
                .unwrap_or_default();
            serde_json::json!({
                "header": s(q, "header"),
                "question": s(q, "question"),
                "multiSelect": q.get("multiSelect").and_then(Value::as_bool).unwrap_or(false),
                "options": options,
            })
        })
        .collect();
    if out.is_empty() || out.iter().any(|q| q["options"].as_array().map_or(true, Vec::is_empty)) {
        return None;
    }
    Some(Value::Array(out))
}

/// Cuts `s` to at most `max` bytes, on a character boundary. Also says whether it cut.
pub fn truncate(s: &str, max: usize) -> (String, bool) {
    if s.len() <= max {
        return (s.to_string(), false);
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    (s[..end].to_string(), true)
}

fn bounded(s: String) -> String {
    let (t, cut) = truncate(&s, MAX_INPUT);
    if cut { format!("{t}…") } else { t }
}

/// The input worth showing for a tool call: the command for Bash, the path for file tools,
/// the pattern for searches, and so on; compact JSON for anything else.
pub fn key_input(name: &str, input: &Value) -> Option<String> {
    let s = |k: &str| input.get(k).and_then(Value::as_str).filter(|v| !v.is_empty());
    let n = |k: &str| input.get(k).and_then(Value::as_u64);
    let text = match name {
        "Bash" | "PowerShell" => s("command").map(str::to_string),
        "Read" => s("file_path").map(|p| match (n("offset"), n("limit")) {
            (Some(o), Some(l)) => format!("{p} (lines {o}-{})", o + l),
            (Some(o), None) => format!("{p} (from line {o})"),
            (None, Some(l)) => format!("{p} (first {l} lines)"),
            _ => p.to_string(),
        }),
        "Write" => s("file_path").map(|p| {
            let lines = s("content").map_or(0, |c| c.lines().count());
            format!("{p} ({lines} line{})", if lines == 1 { "" } else { "s" })
        }),
        "Edit" => s("file_path").map(|p| {
            let one = |k: &str| s(k).map(|v| v.lines().next().unwrap_or("").to_string()).unwrap_or_default();
            format!("{p}\n- {}\n+ {}", one("old_string"), one("new_string"))
        }),
        "MultiEdit" => s("file_path").map(|p| {
            let k = input.get("edits").and_then(Value::as_array).map_or(0, Vec::len);
            format!("{p} ({k} edits)")
        }),
        "NotebookEdit" => s("notebook_path").map(str::to_string),
        "Grep" => s("pattern").map(|p| {
            let mut t = p.to_string();
            if let Some(path) = s("path") {
                t.push_str(&format!("  in {path}"));
            }
            if let Some(g) = s("glob").or(s("type")) {
                t.push_str(&format!("  ({g})"));
            }
            t
        }),
        "Glob" => s("pattern").map(|p| match s("path") {
            Some(path) => format!("{p}  in {path}"),
            None => p.to_string(),
        }),
        "WebFetch" => s("url").map(str::to_string),
        "WebSearch" => s("query").map(str::to_string),
        "Task" | "Agent" => match (s("description"), s("prompt")) {
            (Some(d), Some(p)) => Some(format!("{d}\n{p}")),
            (d, p) => d.or(p).map(str::to_string),
        },
        "Skill" => s("skill").map(|k| match s("args") {
            Some(a) => format!("{k} {a}"),
            None => k.to_string(),
        }),
        "TodoWrite" => input.get("todos").and_then(Value::as_array).map(|t| {
            t.iter()
                .map(|x| {
                    let st = x.get("status").and_then(Value::as_str).unwrap_or("");
                    let c = x.get("content").and_then(Value::as_str).unwrap_or("");
                    let mark = match st {
                        "completed" => "x",
                        "in_progress" => ">",
                        _ => " ",
                    };
                    format!("[{mark}] {c}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        }),
        _ => None,
    };
    let text = text.or_else(|| match input {
        Value::Object(m) if m.is_empty() => None,
        Value::Null => None,
        v => Some(v.to_string()),
    })?;
    Some(bounded(text))
}

/// A `tool_result` block's text: its string, or its text blocks joined; images read "[image]".
fn result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| match p.get("type").and_then(Value::as_str) {
                Some("text") => p.get("text").and_then(Value::as_str).map(str::to_string),
                Some("image") => Some("[image]".into()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// A tool's result, cut to `MAX_OUTPUT` bytes; `lines` counts the whole result.
pub fn tool_output(text: &str, is_error: bool) -> ToolOutput {
    let text = text.trim_end();
    let (t, truncated) = truncate(text, MAX_OUTPUT);
    ToolOutput { text: t, lines: text.lines().count(), truncated, is_error }
}

/// Puts each tool result on its call when both are in `entries`. A result whose call came in
/// an earlier chunk stays as a "result" entry, for the panel to pair by `tool_id`.
pub fn pair(entries: Vec<Entry>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::with_capacity(entries.len());
    let mut calls: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for e in entries {
        if e.kind == "result" {
            if let Some(&i) = e.tool_id.as_ref().and_then(|id| calls.get(id)) {
                if out[i].output.is_none() {
                    out[i].output = e.output;
                    continue;
                }
            }
            out.push(e);
            continue;
        }
        if e.kind == "tool" {
            if let Some(id) = &e.tool_id {
                calls.insert(id.clone(), out.len());
            }
        }
        out.push(e);
    }
    out
}

/// The screenshot a `Read` call opens: its `file_path`, when that is an image.
fn read_image(name: &str, input: Option<&Value>) -> Option<String> {
    if name != "Read" {
        return None;
    }
    input?.get("file_path").and_then(Value::as_str).filter(|p| is_image_path(p)).map(str::to_string)
}

pub fn parse_line(line: &str) -> Vec<Entry> {
    parse_line_in(line, false)
}

/// The text between `<tag>` and `</tag>` in `s`, trimmed.
fn tag_text<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let a = s.find(&open)? + open.len();
    let b = s[a..].find(&format!("</{tag}>"))? + a;
    Some(s[a..b].trim())
}

/// A background task's notice that Claude Code hands the main conversation as a user message
/// (`<task-notification>`: a subagent or a background command that stopped), as a "task"
/// entry: its summary as the text, its task id (a subagent's id) in `tool_id` and its status
/// in `input`. The report itself stays out: the subagent's own view has it.
fn task_notice(text: &str, at: Option<String>) -> Option<Entry> {
    let text = text.trim_start();
    if !text.starts_with("<task-notification") {
        return None;
    }
    let summary = tag_text(text, "summary").filter(|s| !s.is_empty()).unwrap_or("A background task stopped");
    Some(Entry {
        kind: "task",
        text: summary.to_string(),
        at,
        tool_id: tag_text(text, "task-id").filter(|s| !s.is_empty()).map(str::to_string),
        input: tag_text(text, "status").filter(|s| !s.is_empty()).map(str::to_string),
        ..Default::default()
    })
}

/// `parse_line`; `sub` keeps sidechain lines, for a subagent's own transcript.
pub fn parse_line_in(line: &str, sub: bool) -> Vec<Entry> {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return vec![] };
    if (!sub && v.get("isSidechain").and_then(Value::as_bool) == Some(true)) || v.get("isMeta").and_then(Value::as_bool) == Some(true) {
        return vec![];
    }
    let at = v.get("timestamp").and_then(Value::as_str).map(str::to_string);
    let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
    let content = v.get("message").and_then(|m| m.get("content"));
    let mut out = Vec::new();
    // A local slash command, which Claude Code (2.1.318) records as `system` lines: the
    // command itself ("/context"), then what it printed. Its notices ("Compacted …") likewise.
    if kind == "system" && v.get("isSidechain").and_then(Value::as_bool) != Some(true) {
        let sub = v.get("subtype").and_then(Value::as_str).unwrap_or("");
        let text = v.get("content").and_then(Value::as_str).unwrap_or("");
        if sub == "local_command" {
            if let Some(o) = command_output(text) {
                out.push(Entry { kind: "cmdout", text: o, at, ..Default::default() });
            } else if text.contains("<command-name>") {
                let t = strip_wrappers(text);
                if !t.is_empty() {
                    out.push(Entry { kind: "user", text: t, at, ..Default::default() });
                }
            }
        } else if sub == "informational" && !text.trim().is_empty() {
            out.push(Entry { kind: "cmdout", text: text.trim().to_string(), at, ..Default::default() });
        }
        return out;
    }
    // A message Hugh sent mid-turn (the panel's box or the terminal): Claude Code reads it at
    // its next step and records it as a `queued_command` attachment, not a `user` line. Task
    // notifications arrive the same way with another origin, and stay hidden.
    // Claude Code's own queue: a message typed while it works is `enqueue`d, and `remove`d
    // (read mid-turn) or `dequeue`d (taken at the turn's end) once Claude has it. The panel
    // shows a "queued" one as sent but not yet picked up, until its "unqueued" line. Task
    // notifications go through the same queue, and stay hidden.
    if kind == "queue-operation" {
        let raw = v.get("content").and_then(Value::as_str).unwrap_or("");
        if raw.trim_start().starts_with("<task-notification") {
            return out;
        }
        let text = strip_wrappers(raw);
        let op = v.get("operation").and_then(Value::as_str).unwrap_or("");
        let kind = if op == "enqueue" { "queued" } else { "unqueued" };
        // An enqueue with nothing to show is skipped; a removal with no content clears them all.
        if kind == "queued" && text.is_empty() {
            return out;
        }
        out.push(Entry { kind, text, at, ..Default::default() });
        return out;
    }
    if kind == "attachment" {
        let a = v.get("attachment").unwrap_or(&Value::Null);
        let human = a.get("origin").and_then(|o| o.get("kind")).and_then(Value::as_str) == Some("human");
        if a.get("type").and_then(Value::as_str) == Some("queued_command") && human {
            let t = strip_wrappers(a.get("prompt").and_then(Value::as_str).unwrap_or(""));
            if !t.is_empty() {
                let at = a.get("timestamp").and_then(Value::as_str).map(str::to_string).or(at);
                out.push(Entry { kind: "user", text: t, at, ..Default::default() });
            }
        }
        return out;
    }
    match (kind, content) {
        ("user", Some(Value::String(s))) => {
            // A notice is read raw: its report may quote tags strip_wrappers acts on.
            if let Some(e) = task_notice(s, at.clone()) {
                out.push(e);
                return out;
            }
            // What a slash command printed (`/cost`, `/effort high`, "Unknown command"): its
            // own line, so the chat shows it under the command as the terminal does.
            if let Some(o) = command_output(s) {
                out.push(Entry { kind: "cmdout", text: o, at, ..Default::default() });
                return out;
            }
            let t = strip_wrappers(s);
            if !t.is_empty() {
                out.push(Entry { kind: "user", text: t, at, ..Default::default() });
            }
        }
        ("user", Some(Value::Array(blocks))) => {
            for b in blocks {
                match b.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        let raw = b.get("text").and_then(Value::as_str).unwrap_or("");
                        if let Some(e) = task_notice(raw, at.clone()) {
                            out.push(e);
                            continue;
                        }
                        let t = strip_wrappers(raw);
                        if !t.is_empty() {
                            out.push(Entry { kind: "user", text: t, at: at.clone(), ..Default::default() });
                        }
                    }
                    Some("tool_result") => {
                        let Some(id) = b.get("tool_use_id").and_then(Value::as_str) else { continue };
                        let err = b.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                        out.push(Entry {
                            kind: "result",
                            at: at.clone(),
                            tool_id: Some(id.to_string()),
                            output: Some(tool_output(&result_text(b.get("content")), err)),
                            ..Default::default()
                        });
                    }
                    _ => {}
                }
            }
        }
        ("assistant", Some(Value::Array(blocks))) => {
            for b in blocks {
                match b.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        let t = b.get("text").and_then(Value::as_str).unwrap_or("").trim().to_string();
                        if !t.is_empty() {
                            out.push(Entry { kind: "assistant", text: t, at: at.clone(), ..Default::default() });
                        }
                    }
                    Some("tool_use") => {
                        let name = b.get("name").and_then(Value::as_str).unwrap_or("tool");
                        let detail = b.get("input").map(tool_detail).unwrap_or_default();
                        let text = if detail.is_empty() { name.to_string() } else { format!("{name}\t{detail}") };
                        out.push(Entry {
                            kind: "tool",
                            text,
                            at: at.clone(),
                            tool_id: b.get("id").and_then(Value::as_str).map(str::to_string),
                            input: b.get("input").and_then(|i| key_input(name, i)),
                            output: None,
                            ask: if name == ASK_TOOL { b.get("input").and_then(ask_questions) } else { None },
                            images: read_image(name, b.get("input")).into_iter().collect(),
                        });
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    out
}

/// The longest text the board shows for what a card waits on Hugh for, in characters.
pub const ASK_MAX: usize = 300;

/// `s` on one line (runs of whitespace as one space), cut to at most `max` characters on a
/// word boundary, ending with "…" when cut.
pub fn cut_words(s: &str, max: usize) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= max || max == 0 {
        return one;
    }
    // Room for the "…".
    let keep: String = one.chars().take(max - 1).collect();
    let next = one.chars().nth(max - 1);
    let cut = if next == Some(' ') {
        keep.as_str()
    } else {
        // Back to the last space, unless the text has none (one long word).
        keep.rfind(' ').map_or(keep.as_str(), |i| &keep[..i])
    };
    format!("{}…", cut.trim_end())
}

/// The last `bytes` of the file at `path`, from its first whole line (the whole file when it
/// is shorter).
fn tail_text(path: &Path, bytes: u64) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(bytes);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    Some(if start == 0 { text } else { text.split_once('\n').map_or(String::new(), |(_, rest)| rest.to_string()) })
}

/// How much of a transcript's end the board reads for a reply or an open tool call.
const TAIL: u64 = 256 * 1024;

/// The text blocks of an assistant line, joined; None for any other line or one with no text.
fn assistant_text(v: &Value) -> Option<String> {
    if v.get("type").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let blocks = v.pointer("/message/content")?.as_array()?;
    let text: Vec<&str> = blocks
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .filter(|t| !t.trim().is_empty())
        .collect();
    (!text.is_empty()).then(|| text.join("\n\n"))
}

/// A user line that is a prompt (typed text, an interrupt note), not only tool results.
fn is_prompt(v: &Value) -> bool {
    if v.get("type").and_then(Value::as_str) != Some("user") {
        return false;
    }
    match v.pointer("/message/content") {
        Some(Value::String(_)) => true,
        Some(Value::Array(b)) => !b.iter().any(|b| b.get("type").and_then(Value::as_str) == Some("tool_result")),
        _ => false,
    }
}

/// The main conversation's last reply in `lines`: the text of its last assistant message
/// (Claude Code writes one line per block, so text lines of the same message are joined).
/// Only the latest turn counts: a prompt before any text means that turn wrote none (None).
pub fn last_reply<'a>(lines: impl DoubleEndedIterator<Item = &'a str>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut id: Option<String> = None;
    for line in lines.rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v.get("isSidechain").and_then(Value::as_bool) == Some(true) || v.get("isMeta").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
        if kind != "assistant" && kind != "user" {
            continue;
        }
        if is_prompt(&v) {
            break;
        }
        let this = v.pointer("/message/id").and_then(Value::as_str).map(str::to_string);
        if parts.is_empty() {
            if let Some(t) = assistant_text(&v) {
                parts.push(t);
                id = this;
            }
            continue;
        }
        // Earlier blocks of the same message (text kept, thinking passed over); anything else
        // ends it.
        let same = id.is_some() && this == id;
        match assistant_text(&v) {
            Some(t) if same => parts.push(t),
            None if same => {}
            _ => break,
        }
    }
    parts.reverse();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// `last_reply` for the transcript at `path`, reading only its last 256 KB.
pub fn last_reply_in(path: &Path) -> Option<String> {
    let text = tail_text(path, TAIL)?;
    last_reply(text.lines())
}

/// The main conversation's last `n` assistant texts in `lines`, oldest first (card #119: the
/// Router's replies for `kb router --replies`). Each message's text blocks are joined.
pub fn recent_replies_of<'a>(lines: impl Iterator<Item = &'a str>, n: usize) -> Vec<String> {
    let mut out: Vec<(Option<String>, String)> = Vec::new();
    for line in lines {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v.get("isSidechain").and_then(Value::as_bool) == Some(true) || v.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(t) = assistant_text(&v) else { continue };
        let id = v.pointer("/message/id").and_then(Value::as_str).map(str::to_string);
        match out.last_mut() {
            Some((last, text)) if id.is_some() && *last == id => {
                text.push_str("\n\n");
                text.push_str(&t);
            }
            _ => out.push((id, t)),
        }
    }
    let skip = out.len().saturating_sub(n);
    out.into_iter().skip(skip).map(|(_, t)| t).collect()
}

/// `recent_replies_of` for the transcript at `path`, reading only its last 256 KB.
pub fn recent_replies(path: &Path, n: usize) -> Vec<String> {
    tail_text(path, TAIL).map(|t| recent_replies_of(t.lines(), n)).unwrap_or_default()
}

/// A reply as the card face reads it: fenced code blocks, images, tables and rules taken out,
/// links as their text, and Markdown's marks (headings, quotes, list bullets, emphasis, inline
/// code) dropped. Paragraphs stay apart by a blank line.
pub fn plain_reply(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut fence: Option<&str> = None;
    for raw in text.lines() {
        let t = raw.trim();
        if let Some(f) = fence {
            if t.starts_with(f) {
                fence = None;
            }
            continue;
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = Some(&t[..3]);
            out.push(String::new());
            continue;
        }
        // A table row, or a rule.
        if t.starts_with('|') || (t.len() >= 3 && t.chars().all(|c| matches!(c, '-' | '*' | '_' | ' '))) {
            out.push(String::new());
            continue;
        }
        // A heading's "# " to "###### ", but not "#108".
        let hashes = t.chars().take_while(|&c| c == '#').count();
        let mut l = if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') { t[hashes..].trim_start() } else { t };
        while let Some(rest) = l.strip_prefix('>') {
            l = rest.trim_start();
        }
        for bullet in ["- ", "* ", "+ "] {
            if let Some(rest) = l.strip_prefix(bullet) {
                l = rest;
            }
        }
        let digits = l.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && (l[digits..].starts_with(". ") || l[digits..].starts_with(") ")) {
            l = &l[digits + 2..];
        }
        out.push(inline_plain(l));
    }
    // Paragraphs: lines joined, blank lines between.
    let mut paras: Vec<String> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    for l in out {
        if l.trim().is_empty() {
            if !cur.is_empty() {
                paras.push(cur.join(" "));
                cur.clear();
            }
        } else {
            cur.push(l.trim().to_string());
        }
    }
    if !cur.is_empty() {
        paras.push(cur.join(" "));
    }
    paras.join("\n\n")
}

/// One line without images, with links as their text and no emphasis or code marks.
fn inline_plain(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    // For a `[` at `open`: the index of its `]`, and just past the `(...)` after it.
    let link_end = |open: usize| -> Option<(usize, usize)> {
        let close = (open + 1..chars.len()).find(|&j| chars[j] == ']')?;
        if chars.get(close + 1) != Some(&'(') {
            return None;
        }
        let end = (close + 2..chars.len()).find(|&j| chars[j] == ')')?;
        Some((close, end + 1))
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '!' && chars.get(i + 1) == Some(&'[') {
            if let Some((_, end)) = link_end(i + 1) {
                i = end;
                continue;
            }
        }
        if c == '[' {
            if let Some((close, end)) = link_end(i) {
                out.extend(&chars[i + 1..close]);
                i = end;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    unemphasise(&out)
}

/// `s` with paired emphasis and code marks taken off (`**x**`, `__x__`, `~~x~~`, `` `x` ``,
/// `*x*`, `_x_`). A lone "*", "~" or "_" stays ("~/build/*.tmp", "snake_case").
fn unemphasise(s: &str) -> String {
    static RES: std::sync::OnceLock<Vec<regex::Regex>> = std::sync::OnceLock::new();
    let res = RES.get_or_init(|| {
        [
            r"`([^`\n]+)`",
            r"\*\*([^*\s](?:[^*\n]*[^*\s])?)\*\*",
            r"\b__([^_\s](?:[^_\n]*[^_\s])?)__\b",
            r"~~([^~\s](?:[^~\n]*[^~\s])?)~~",
            r"\*([^*\s](?:[^*\n]*[^*\s])?)\*",
            r"\b_([^_\s](?:[^_\n]*[^_\s])?)_\b",
        ]
        .iter()
        .map(|p| regex::Regex::new(p).expect("emphasis"))
        .collect()
    });
    let mut out = s.to_string();
    for re in res {
        out = re.replace_all(&out, "$1").into_owned();
    }
    out
}

/// What a reply could ask Hugh, kept per card so the board never reads a transcript to say it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReplyParts {
    /// The question its last paragraph asks: the last sentence there that ends with "?".
    pub question: Option<String>,
    /// Its last paragraph, when it says a plan is ready ("plan ready" or "plan is ready").
    pub plan: Option<String>,
}

/// The parts of a reply that ask Hugh something (`ReplyParts`), from its text as written.
pub fn reply_parts(text: &str) -> ReplyParts {
    let plain = plain_reply(text);
    let Some(last) = plain.split("\n\n").map(str::trim).filter(|p| !p.is_empty()).last() else { return ReplyParts::default() };
    let lower = plain.to_lowercase();
    let plan = (lower.contains("plan ready") || lower.contains("plan is ready")).then(|| last.to_string());
    ReplyParts { question: last_question(last), plan }
}

/// The last sentence of `para` that ends with "?" (closing quotes or brackets after it allowed).
fn last_question(para: &str) -> Option<String> {
    let mut sentences: Vec<String> = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = para.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        cur.push(c);
        if matches!(c, '.' | '!' | '?') && chars.get(i + 1).map_or(true, |n| n.is_whitespace()) {
            sentences.push(std::mem::take(&mut cur));
        }
    }
    sentences.push(cur);
    sentences
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| s.trim_end_matches(|c: char| matches!(c, '"' | '\'' | ')' | ']' | '”' | '’')).ends_with('?'))
        .last()
}

/// What an unread reply asks Hugh, for the board: ("plan", its last paragraph) when the card
/// is in Planning and the reply says the plan is ready, else ("reply", its question) when its
/// last paragraph asks one. None for a reply that only tells him something. At most `ASK_MAX`
/// characters.
pub fn ask_of(parts: &ReplyParts, planning: bool) -> Option<(&'static str, String)> {
    let (kind, text) = match (&parts.plan, &parts.question) {
        (Some(p), _) if planning => ("plan", p),
        (_, Some(q)) => ("reply", q),
        _ => return None,
    };
    Some((kind, cut_words(text, ASK_MAX)))
}

/// `ask_of` for a reply's text as written.
pub fn reply_ask(text: &str, planning: bool) -> Option<(&'static str, String)> {
    ask_of(&reply_parts(text), planning)
}

/// The tool a permission prompt's message names ("Claude needs your permission to use Bash").
pub fn permission_tool(message: &str) -> Option<String> {
    let i = message.find("permission to use ")? + "permission to use ".len();
    let name = message[i..].split_whitespace().next()?.trim_end_matches(|c: char| !(c.is_alphanumeric() || c == '_'));
    (!name.is_empty()).then(|| name.to_string())
}

/// The last call to `tool` in `lines` that has no result yet: its `tool_use` id, and
/// "<Tool>: <key input>" (the tool alone when it has no input worth showing). `sub` keeps
/// sidechain lines, for a subagent's own transcript.
pub fn open_call<'a>(lines: impl Iterator<Item = &'a str>, tool: &str, sub: bool) -> Option<(String, String)> {
    let mut calls: Vec<(String, String)> = Vec::new();
    let mut done: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in lines {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if !sub && v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let Some(blocks) = v.pointer("/message/content").and_then(Value::as_array) else { continue };
        for b in blocks {
            let id = b.get("id").or(b.get("tool_use_id")).and_then(Value::as_str).unwrap_or("").to_string();
            match b.get("type").and_then(Value::as_str) {
                Some("tool_use") if b.get("name").and_then(Value::as_str) == Some(tool) => {
                    let text = match b.get("input").and_then(|i| key_input(tool, i)) {
                        Some(k) => format!("{tool}: {k}"),
                        None => tool.to_string(),
                    };
                    calls.push((id, text));
                }
                Some("tool_result") => {
                    done.insert(id);
                }
                _ => {}
            }
        }
    }
    calls.into_iter().rev().find(|(id, _)| !done.contains(id))
}

/// `open_call` for the transcript at `path` (its last 256 KB).
pub fn open_call_in(path: &Path, tool: &str, sub: bool) -> Option<(String, String)> {
    let text = tail_text(path, TAIL)?;
    open_call(text.lines(), tool, sub)
}

/// The call a permission prompt waits on (`prompt_call`): its `tool_use` id, what it asks to
/// run, and the transcript it is in (`sub`: a subagent's).
#[derive(Debug, Clone, PartialEq)]
pub struct OpenCall {
    pub id: String,
    pub text: String,
    pub path: PathBuf,
    pub sub: bool,
}

/// What a permission prompt asks to run: the last open call to `tool` in the main transcript,
/// else in the newest subagent transcript (the one written last).
pub fn prompt_call(main: &Path, tool: &str) -> Option<OpenCall> {
    if let Some((id, text)) = open_call_in(main, tool, false) {
        return Some(OpenCall { id, text, path: main.to_path_buf(), sub: false });
    }
    let dir = std::fs::read_dir(subagents_dir(main)).ok()?;
    let newest = dir
        .flatten()
        .map(|d| d.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .filter_map(|p| Some((std::fs::metadata(&p).and_then(|m| m.modified()).ok()?, p)))
        .max_by_key(|(t, _)| *t)?;
    let (id, text) = open_call_in(&newest.1, tool, true)?;
    Some(OpenCall { id, text, path: newest.1, sub: true })
}

/// Whether the call `id` in `lines` has gone past its permission prompt: a result for it, or
/// a later assistant line of another message (Claude went on). The calls of one message are
/// written one line each, so a parallel call after it doesn't count. `sub` as `open_call`.
pub fn call_settled<'a>(lines: impl Iterator<Item = &'a str>, id: &str, sub: bool) -> bool {
    let mut message: Option<Option<String>> = None;
    for line in lines {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if !sub && v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let blocks = v.pointer("/message/content").and_then(Value::as_array);
        let has = |kind: &str, key: &str| blocks.is_some_and(|b| b.iter().any(|b| b.get("type").and_then(Value::as_str) == Some(kind) && b.get(key).and_then(Value::as_str) == Some(id)));
        if has("tool_result", "tool_use_id") {
            return true;
        }
        let this = v.pointer("/message/id").and_then(Value::as_str).map(str::to_string);
        if v.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        match &message {
            None if has("tool_use", "id") => message = Some(this),
            Some(m) if this.is_none() || *m != this => return true,
            _ => {}
        }
    }
    false
}

/// `call_settled` for the transcript at `path` (its last 256 KB).
pub fn call_settled_in(path: &Path, id: &str, sub: bool) -> bool {
    tail_text(path, TAIL).is_some_and(|t| call_settled(t.lines(), id, sub))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_user_assistant_and_tools() {
        let u = r#"{"type":"user","message":{"role":"user","content":"Fix it<system-reminder>noise</system-reminder>"},"timestamp":"t1"}"#;
        assert_eq!(parse_line(u), vec![Entry { kind: "user", text: "Fix it".into(), at: Some("t1".into()), ..Default::default() }]);
        let a = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":""},{"type":"text","text":"Done."},{"type":"tool_use","id":"tu1","name":"Bash","input":{"command":"ls\nmore","description":"List files"}}]}}"#;
        let e = parse_line(a);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].text, "Done.");
        assert_eq!(
            e[1],
            Entry {
                kind: "tool",
                text: "Bash\tList files".into(),
                tool_id: Some("tu1".into()),
                input: Some("ls\nmore".into()),
                ..Default::default()
            }
        );
        let side = r#"{"type":"assistant","isSidechain":true,"message":{"content":[{"type":"text","text":"x"}]}}"#;
        assert!(parse_line(side).is_empty());
        // A result with no id can't be paired, so it is dropped.
        let result = r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"ok"}]}}"#;
        assert!(parse_line(result).is_empty());
        let pasted = r#"{"type":"user","message":{"content":"<pasted_content id=\"a1\">\nHello\n</pasted_content>"}}"#;
        assert_eq!(parse_line(pasted)[0].text, "Hello");
        let cmd = r#"{"type":"user","message":{"content":"<command-name>/kanban</command-name><command-args>34 ship it</command-args>"}}"#;
        assert_eq!(parse_line(cmd)[0].text, "/kanban 34 ship it");
    }

    #[test]
    fn a_slash_commands_output_is_its_own_entry() {
        let out = r#"{"type":"user","timestamp":"2026-11-05T16:00:00Z","message":{"content":"<local-command-stdout>Set model to \u001b[1mOpus 5\u001b[22m and saved</local-command-stdout>"}}"#;
        let e = &parse_line(out)[0];
        assert_eq!((e.kind, e.text.as_str(), e.at.as_deref()), ("cmdout", "Set model to Opus 5 and saved", Some("2026-11-05T16:00:00Z")));
        let err = r#"{"type":"user","message":{"content":"<local-command-stderr>Unknown command: /nope</local-command-stderr>"}}"#;
        assert_eq!(parse_line(err)[0].kind, "cmdout");
        // Empty output, or a caveat, shows nothing; text around the tags is an ordinary message.
        assert!(parse_line(r#"{"type":"user","message":{"content":"<local-command-stdout></local-command-stdout>"}}"#).is_empty());
        assert!(parse_line(r#"{"type":"user","message":{"content":"<local-command-caveat>Caveat: x</local-command-caveat>"}}"#).is_empty());
        let mixed = r#"{"type":"user","message":{"content":"see <local-command-stdout>x</local-command-stdout>"}}"#;
        assert_eq!(parse_line(mixed)[0].kind, "user");
        // Claude Code 2.1.318 records a local command and its output as system lines.
        let cmd = r#"{"type":"system","subtype":"local_command","isSidechain":false,"content":"<command-name>/context</command-name>\n  <command-message>context</command-message>\n  <command-args></command-args>"}"#;
        assert_eq!((parse_line(cmd)[0].kind, parse_line(cmd)[0].text.as_str()), ("user", "/context"));
        let printed = r#"{"type":"system","subtype":"local_command","content":"<local-command-stdout>Kept model as `Opus 5.5`</local-command-stdout>"}"#;
        assert_eq!((parse_line(printed)[0].kind, parse_line(printed)[0].text.as_str()), ("cmdout", "Kept model as `Opus 5.5`"));
        let notice = r#"{"type":"system","subtype":"informational","content":"Compacted using a summary"}"#;
        assert_eq!(parse_line(notice)[0].kind, "cmdout");
        // Other system lines (turn_duration and the like) still show nothing.
        assert!(parse_line(r#"{"type":"system","subtype":"turn_duration","content":"x"}"#).is_empty());
    }

    #[test]
    fn a_turn_ends_at_its_turn_duration_line() {
        let tool = call("a", "Bash", r#"{"command":"ls"}"#);
        let rejected = result("a", r#""The user doesn't want to proceed""#, true);
        let interrupted = r#"{"type":"user","isSidechain":false,"message":{"content":[{"type":"text","text":"[Request interrupted by user for tool use]"}]}}"#;
        let ended = r#"{"type":"system","subtype":"turn_duration","isSidechain":false,"timestamp":"2026-11-05T10:36:40.537Z"}"#;
        let link = r#"{"type":"pr-link","timestamp":"2026-11-05T10:36:41Z"}"#;
        let side = r#"{"type":"assistant","isSidechain":true,"message":{"content":[{"type":"text","text":"x"}]}}"#;
        let lines = [tool.as_str(), rejected.as_str(), interrupted, ended, link, side];
        assert_eq!(turn_ended_at(lines.iter().copied()).as_deref(), Some("2026-11-05T10:36:40.537Z"));
        // Still on the turn: a call waiting on a prompt, or a new prompt after the last end.
        assert_eq!(turn_ended_at([ended, tool.as_str()].iter().copied()), None);
        // An interrupt with no turn_duration line after it still ends the turn.
        let int_at = r#"{"type":"user","timestamp":"2026-11-05T10:36:40.400Z","message":{"content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#;
        assert_eq!(turn_ended_at([tool.as_str(), int_at].iter().copied()).as_deref(), Some("2026-11-05T10:36:40.400Z"));
        let prompt = r#"{"type":"user","message":{"content":"Next"}}"#;
        assert_eq!(turn_ended_at([ended, prompt].iter().copied()), None);
        assert_eq!(turn_ended_at([link].iter().copied()), None);
    }

    #[test]
    fn a_read_of_a_screenshot_carries_its_path() {
        let a = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"r1","name":"Read","input":{"file_path":"C:\\tmp\\shot.PNG"}},{"type":"tool_use","id":"r2","name":"Read","input":{"file_path":"C:\\src\\main.rs"}},{"type":"tool_use","id":"r3","name":"Write","input":{"file_path":"C:\\tmp\\b.png","content":""}}]}}"#;
        let e = parse_line(a);
        assert_eq!(e[0].images, vec![r"C:\tmp\shot.PNG".to_string()]);
        // Not an image, or not a Read: nothing to draw.
        assert!(e[1].images.is_empty() && e[2].images.is_empty());
        // An entry with no images doesn't send the field.
        assert!(!serde_json::to_string(&e[1]).unwrap().contains("images"));
        assert!(is_image_path("a/b.webp") && is_image_path("x.JPEG") && !is_image_path("png") && !is_image_path("a.png.txt"));
    }

    #[test]
    fn a_question_carries_its_questions_for_the_chat() {
        let a = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"header":"Colour","question":"Which colour?","multiSelect":false,"options":[{"label":"Red","description":"warm"},{"label":"Blue"}]}]}}]}}"#;
        let e = parse_line(a);
        assert_eq!(e[0].kind, "tool");
        assert_eq!(
            e[0].ask,
            Some(serde_json::json!([{ "header": "Colour", "question": "Which colour?", "multiSelect": false,
                "options": [{ "label": "Red", "description": "warm" }, { "label": "Blue", "description": "" }] }]))
        );
        // Another tool, or a question with no options, gets no question card.
        assert_eq!(parse_line(&call("b", "Bash", r#"{"command":"ls"}"#))[0].ask, None);
        assert_eq!(ask_questions(&serde_json::json!({ "questions": [{ "question": "?", "options": [] }] })), None);
        assert_eq!(ask_questions(&serde_json::json!({})), None);
    }

    #[test]
    fn a_message_sent_mid_turn_shows_as_hughs() {
        let human = r#"{"type":"attachment","isSidechain":false,"timestamp":"t9","attachment":{"type":"queued_command","prompt":"Also say PONG3.","commandMode":"prompt","origin":{"kind":"human"},"timestamp":"t8","humanTurn":true}}"#;
        assert_eq!(parse_line(human), vec![Entry { kind: "user", text: "Also say PONG3.".into(), at: Some("t8".into()), ..Default::default() }]);
        // A background task's notification comes the same way, and stays hidden.
        let task = r#"{"type":"attachment","attachment":{"type":"queued_command","prompt":"<task-notification>x</task-notification>","commandMode":"task-notification","origin":{"kind":"task-notification"}}}"#;
        assert!(parse_line(task).is_empty());
        // So do other attachments.
        assert!(parse_line(r#"{"type":"attachment","attachment":{"type":"todo_reminder"}}"#).is_empty());
    }

    #[test]
    fn the_queue_shows_what_claude_hasnt_picked_up_yet() {
        let q = |op: &str, content: &str| parse_line(&format!(r#"{{"type":"queue-operation","operation":"{op}","timestamp":"t1","content":{}}}"#, serde_json::json!(content)));
        assert_eq!(q("enqueue", "Also say PONG3."), vec![Entry { kind: "queued", text: "Also say PONG3.".into(), at: Some("t1".into()), ..Default::default() }]);
        // Read mid-turn, or taken at the turn's end: no longer waiting.
        for op in ["remove", "dequeue"] {
            assert_eq!(q(op, "Also say PONG3.")[0].kind, "unqueued", "{op}");
        }
        // A pasted message shows as its text, so its enqueue and removal still match.
        assert_eq!(q("enqueue", "<pasted_content id=\"a\">\nLong note\n</pasted_content>")[0].text, "Long note");
        // Task notifications use the same queue, and stay hidden both ways.
        assert!(q("enqueue", "<task-notification>\n<task-id>x</task-id>").is_empty());
        assert!(q("remove", "<task-notification>\n<task-id>x</task-id>").is_empty());
        // A removal with no content clears them all; an empty enqueue is skipped.
        assert_eq!(parse_line(r#"{"type":"queue-operation","operation":"clear"}"#)[0].text, "");
        assert!(q("enqueue", "").is_empty());
    }

    fn call(id: &str, name: &str, input: &str) -> String {
        format!(r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"{id}","name":"{name}","input":{input}}}]}}}}"#)
    }
    fn result(id: &str, content: &str, err: bool) -> String {
        format!(r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"{id}","is_error":{err},"content":{content}}}]}}}}"#)
    }
    fn parse_all(lines: &[String]) -> Vec<Entry> {
        pair(lines.iter().flat_map(|l| parse_line(l)).collect())
    }

    #[test]
    fn times_the_latest_thinking_from_the_line_before_it() {
        let user = r#"{"type":"user","message":{"content":"Go"},"timestamp":"2026-11-04T15:06:00.000Z"}"#;
        let think = |t: &str| format!(r#"{{"type":"assistant","message":{{"content":[{{"type":"thinking","thinking":"","signature":"x"}}]}},"timestamp":"{t}"}}"#);
        let tool = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"a","name":"Bash","input":{}}]},"timestamp":"2026-11-04T15:06:05.000Z"}"#;
        let result = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"a","content":"ok"}]},"timestamp":"2026-11-04T15:06:09.000Z"}"#;
        let side = r#"{"type":"user","isSidechain":true,"message":{"content":"sub"},"timestamp":"2026-11-04T15:06:15.000Z"}"#;
        let t1 = think("2026-11-04T15:06:04.000Z");
        let t2 = think("2026-11-04T15:06:21.500Z");
        let queued = r#"{"type":"queue-operation","operation":"enqueue","content":"hi","timestamp":"2026-11-04T15:06:18.000Z"}"#;
        // The latest one wins, timed from the message before it: a subagent's line and a queue
        // operation don't count.
        let lines = [user, t1.as_str(), tool, result, side, queued, t2.as_str(), "{bad json"];
        assert_eq!(
            latest_thought(lines.into_iter(), None),
            Some(Thought { at: "2026-11-04T15:06:21.500Z".into(), ms: Some(12_500) })
        );
        // A thinking line first in a read is timed from the line an earlier read ended on.
        assert_eq!(latest_thought([t1.as_str()].into_iter(), Some("2026-11-04T15:06:00.000Z".into())).unwrap().ms, Some(4_000));
        // With nothing before it there is no time, and no thinking means nothing at all.
        assert_eq!(latest_thought([t1.as_str()].into_iter(), None).unwrap().ms, None);
        assert_eq!(latest_thought([user, tool].into_iter(), None), None);
    }

    #[test]
    fn read_times_thinking_across_reads() {
        let dir = std::env::temp_dir().join(format!("kanban-thought-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.jsonl");
        let user = r#"{"type":"user","message":{"content":"Go"},"timestamp":"2026-11-04T15:06:00.000Z"}"#;
        std::fs::write(&p, format!("{user}\n")).unwrap();
        let first = read(&p, 0).unwrap();
        assert_eq!(first.thought, None);
        let think = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":""}]},"timestamp":"2026-11-04T15:06:07.000Z"}"#;
        std::fs::write(&p, format!("{user}\n{think}\n")).unwrap();
        let second = read(&p, first.offset).unwrap();
        assert_eq!(second.thought, Some(Thought { at: "2026-11-04T15:06:07.000Z".into(), ms: Some(7_000) }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pairs_results_with_their_calls_by_id() {
        let lines = [
            call("a", "Read", r#"{"file_path":"C:\\x\\main.rs","offset":10,"limit":5}"#),
            call("b", "Grep", r#"{"pattern":"fn main","path":"src","glob":"*.rs"}"#),
            // Results arrive in a different order from the calls.
            result("b", r#"[{"type":"text","text":"src/main.rs:1"},{"type":"image","source":{}}]"#, false),
            result("a", r#""line one\nline two\n""#, true),
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Found it."}]}}"#.to_string(),
        ];
        let e = parse_all(&lines);
        assert_eq!(e.len(), 3, "results fold into their calls: {e:?}");
        assert_eq!(e[0].input.as_deref(), Some("C:\\x\\main.rs (lines 10-15)"));
        let a = e[0].output.as_ref().unwrap();
        assert_eq!((a.text.as_str(), a.lines, a.truncated, a.is_error), ("line one\nline two", 2, false, true));
        assert_eq!(e[1].input.as_deref(), Some("fn main  in src  (*.rs)"));
        assert_eq!(e[1].output.as_ref().unwrap().text, "src/main.rs:1\n[image]");
        assert_eq!(e[2].kind, "assistant");
    }

    #[test]
    fn a_result_for_an_earlier_chunk_stays_for_the_panel() {
        let e = parse_all(&[result("old", r#""done""#, false)]);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].kind, "result");
        assert_eq!(e[0].tool_id.as_deref(), Some("old"));
        assert_eq!(e[0].output.as_ref().unwrap().text, "done");
        // A call with no result yet has no output.
        let e = parse_all(&[call("n", "Bash", r#"{"command":"cargo test"}"#)]);
        assert_eq!(e[0].output, None);
        assert_eq!(e[0].input.as_deref(), Some("cargo test"));
    }

    #[test]
    fn long_outputs_and_inputs_are_cut_on_a_char_boundary() {
        let big: String = (0..2000).map(|i| format!("line {i} é\n")).collect();
        let o = tool_output(&big, false);
        assert!(o.truncated);
        assert!(o.text.len() <= MAX_OUTPUT);
        assert_eq!(o.lines, 2000);
        assert!(big.starts_with(&o.text));
        // Multi-byte characters right at the limit don't split.
        let s = "é".repeat(MAX_OUTPUT);
        let (t, cut) = truncate(&s, MAX_OUTPUT + 1);
        assert!(cut && t.len() == MAX_OUTPUT && t.chars().all(|c| c == 'é'));
        assert_eq!(truncate("short", 10), ("short".to_string(), false));
        let cmd = serde_json::json!({ "command": "x".repeat(MAX_INPUT * 2) });
        let k = key_input("Bash", &cmd).unwrap();
        assert!(k.ends_with('…') && k.len() <= MAX_INPUT + '…'.len_utf8());
        // A whole chunk read from disk stays bounded too.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.jsonl");
        let content = serde_json::to_string(&big).unwrap();
        std::fs::write(&p, format!("{}\n{}\n", call("z", "Bash", r#"{"command":"seq 2000"}"#), result("z", &content, false))).unwrap();
        let c = read(&p, 0).unwrap();
        assert_eq!(c.entries.len(), 1);
        let json = serde_json::to_string(&c.entries).unwrap();
        assert!(json.len() < MAX_OUTPUT + 1024, "chunk is {} bytes", json.len());
    }

    #[test]
    fn key_inputs_for_common_tools() {
        let k = |n: &str, v: &str| key_input(n, &serde_json::from_str(v).unwrap());
        assert_eq!(k("Write", r#"{"file_path":"a.txt","content":"x\ny"}"#).as_deref(), Some("a.txt (2 lines)"));
        assert_eq!(k("Edit", r#"{"file_path":"a.rs","old_string":"foo\nbar","new_string":"baz"}"#).as_deref(), Some("a.rs\n- foo\n+ baz"));
        assert_eq!(k("Glob", r#"{"pattern":"**/*.js"}"#).as_deref(), Some("**/*.js"));
        assert_eq!(k("WebFetch", r#"{"url":"https://x.test","prompt":"p"}"#).as_deref(), Some("https://x.test"));
        assert_eq!(
            k("TodoWrite", r#"{"todos":[{"content":"A","status":"completed"},{"content":"B","status":"in_progress"}]}"#).as_deref(),
            Some("[x] A\n[>] B")
        );
        assert_eq!(k("mcp__x__y", r#"{"id":3}"#).as_deref(), Some(r#"{"id":3}"#));
        assert_eq!(k("ExitPlanMode", "{}"), None);
    }

    #[test]
    fn lists_a_sessions_subagents_and_reads_one() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("sess.jsonl");
        std::fs::write(&main, "").unwrap();
        let subs = subagents_dir(&main);
        assert_eq!(subs, dir.path().join("sess").join("subagents"));
        // No folder: no subagents.
        assert!(subagents(&main).is_empty());
        std::fs::create_dir_all(&subs).unwrap();
        let line = |t: &str, body: &str| format!(r#"{{"isSidechain":true,"agentId":"x","timestamp":"{t}",{body}}}"#);
        let brief = line("2026-11-05T15:00:00Z", r#""type":"user","message":{"role":"user","content":"Test card 91"}"#);
        let reply = line("2026-11-05T15:01:00Z", r#""type":"assistant","message":{"content":[{"type":"text","text":"VERDICT: PASS"}]}"#);
        std::fs::write(subs.join("agent-a2.jsonl"), format!("{brief}\n{reply}\n")).unwrap();
        std::fs::write(subs.join("agent-a2.meta.json"), r#"{"agentType":"card-tester","description":"Test card 91","toolUseId":"toolu_1","finished":true}"#).unwrap();
        // A forked skill: named, and no Agent call.
        std::fs::write(subs.join("agent-a1.jsonl"), line("2026-11-05T14:00:00Z", r#""type":"user","message":{"content":"/code-review"}"#) + "\n").unwrap();
        std::fs::write(subs.join("agent-a1.meta.json"), r#"{"agentType":"general-purpose","name":"code-review","description":"/code-review medium"}"#).unwrap();
        // A meta file with no transcript yet, and a name that isn't an id, are left out.
        std::fs::write(subs.join("agent-a3.meta.json"), r#"{"agentType":"Explore"}"#).unwrap();
        std::fs::write(subs.join("agent-..%5Cx.meta.json"), "{}").unwrap();
        let list = subagents(&main);
        assert_eq!(list.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["a1", "a2"], "oldest first");
        assert_eq!((list[0].agent_type.as_str(), list[0].tool_id.as_deref(), list[0].finished), ("code-review", None, false));
        assert_eq!((list[1].agent_type.as_str(), list[1].description.as_str(), list[1].tool_id.as_deref(), list[1].finished), ("card-tester", "Test card 91", Some("toolu_1"), true));
        assert_eq!(list[1].started.as_deref(), Some("2026-11-05T15:00:00Z"));
        assert!(list[1].updated.is_some());
        // Its own transcript reads in full, though every line is a sidechain one; the main
        // reader still skips them.
        let p = subagent_path(&main, "a2").unwrap();
        let c = read_in(&p, 0, true).unwrap();
        assert_eq!(c.entries.iter().map(|e| (e.kind, e.text.as_str())).collect::<Vec<_>>(), [("user", "Test card 91"), ("assistant", "VERDICT: PASS")]);
        assert!(read(&p, 0).unwrap().entries.is_empty());
        // An id that could leave the folder is refused.
        for bad in ["", "..", "../sess", "a2/../a1", r"a\b"] {
            assert!(subagent_path(&main, bad).is_none(), "{bad}");
        }
        assert!(subagent_path(&main, "a9").is_none(), "no such subagent");
    }

    #[test]
    fn a_subagents_notice_reads_as_one_line_not_its_xml() {
        let body = "<task-notification>\n<task-id>a8cea45</task-id>\n<tool-use-id>toolu_1</tool-use-id>\n<status>completed</status>\n<summary>Agent \"Test card 35\" finished</summary>\n<result>VERDICT: PASS</result>\n</task-notification>";
        let want = vec![Entry {
            kind: "task",
            text: "Agent \"Test card 35\" finished".into(),
            at: Some("t1".into()),
            tool_id: Some("a8cea45".into()),
            input: Some("completed".into()),
            ..Default::default()
        }];
        let line = serde_json::json!({ "type": "user", "timestamp": "t1", "message": { "content": body } }).to_string();
        assert_eq!(parse_line(&line), want);
        let blocks = serde_json::json!({ "type": "user", "timestamp": "t1", "message": { "content": [{ "type": "text", "text": body }] } }).to_string();
        assert_eq!(parse_line(&blocks), want);
        // With no summary it still reads as a notice; an ordinary message is untouched.
        let bare = serde_json::json!({ "type": "user", "message": { "content": "<task-notification>\n<status>killed</status>" } }).to_string();
        assert_eq!(parse_line(&bare)[0].text, "A background task stopped");
        assert_eq!(parse_line(&bare)[0].kind, "task");
        // A report that quotes a slash command's tags is still a notice, not "/foo".
        let quoting = serde_json::json!({ "type": "user", "message": { "content": "<task-notification>\n<summary>Agent \"x\" finished</summary>\n<result>ran <command-name>/foo</command-name></result>" } }).to_string();
        assert_eq!((parse_line(&quoting)[0].kind, parse_line(&quoting)[0].text.as_str()), ("task", "Agent \"x\" finished"));
        let typed = serde_json::json!({ "type": "user", "message": { "content": "what does <task-notification> mean?" } }).to_string();
        assert_eq!(parse_line(&typed)[0].kind, "user");
    }

    #[test]
    fn a_reply_asks_only_when_it_ends_with_a_question_or_a_plan_is_ready() {
        // A statement gives nothing.
        assert_eq!(reply_ask("Done. The tests pass and the card is in Human test.", false), None);
        // A question in an earlier paragraph doesn't count.
        assert_eq!(reply_ask("Should I? I did.\n\nAll done.", false), None);
        // A final question gives that sentence.
        assert_eq!(
            reply_ask("I built it.\n\nThe tests pass. Shall I install it in the test app? Say so.", false),
            Some(("reply", "Shall I install it in the test app?".to_string()))
        );
        // Code blocks, images, tables and Markdown marks are stripped.
        let md = "## Result\n\n```rust\nfn main() { println!(\"Is this code?\"); }\n```\n\n| a | b? |\n|---|---|\n| 1 | 2? |\n\n![shot](C:/x/shot.png)\n\n> **Next:** should I use `dsh-agents` as the [base](https://x.test/b)?";
        assert_eq!(reply_ask(md, false), Some(("reply", "Next: should I use dsh-agents as the base?".to_string())));
        assert_eq!(plain_reply("- one\n- two\n\n1. three\n\n---\n\n__bold__ snake_case"), "one two\n\nthree\n\nbold snake_case");
        // Plan ready, in Planning: its last paragraph; outside Planning, only a question counts.
        let plan = "**Plan ready** (revision 3).\n\nIt adds one field to Live and a filter.";
        assert_eq!(reply_ask(plan, true), Some(("plan", "It adds one field to Live and a filter.".to_string())));
        assert_eq!(reply_ask(plan, false), None);
        assert_eq!(reply_ask("The plan is ready.\n\nBuild it?", true), Some(("plan", "Build it?".to_string())));
        assert_eq!(reply_ask("The plan is ready.\n\nBuild it?", false), Some(("reply", "Build it?".to_string())));
        // A long question is cut to 300 characters on a word boundary, multi-byte safe.
        let long = format!("{}?", "é word ".repeat(80));
        let (_, t) = reply_ask(&long, false).unwrap();
        assert!(t.chars().count() <= ASK_MAX && t.ends_with('…'), "{t}");
        assert!(!t.trim_end_matches('…').ends_with(' ') && long.starts_with(t.trim_end_matches('…')));
        assert_eq!(cut_words("a\n b   c", 10), "a b c");
        assert_eq!(cut_words("abcdefghij", 5), "abcd…");
        assert_eq!(cut_words("ab cd ef", 6), "ab cd…");
    }

    #[test]
    fn the_last_reply_joins_its_messages_text_lines() {
        let text = |id: &str, t: &str| serde_json::json!({ "type": "assistant", "message": { "id": id, "content": [{ "type": "text", "text": t }] } }).to_string();
        let think = serde_json::json!({ "type": "assistant", "message": { "id": "m2", "content": [{ "type": "thinking", "thinking": "" }] } }).to_string();
        let lines = [
            text("m1", "Old reply?"),
            call("t1", "Bash", r#"{"command":"ls"}"#),
            result("t1", r#""ok""#, false),
            text("m2", "First part."),
            think,
            text("m2", "Shall I go on?"),
            r#"{"type":"system","subtype":"turn_duration","timestamp":"t9"}"#.to_string(),
        ];
        assert_eq!(last_reply(lines.iter().map(String::as_str)).as_deref(), Some("First part.\n\nShall I go on?"));
        assert_eq!(last_reply([lines[1].as_str()].into_iter()), None);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.jsonl");
        std::fs::write(&p, lines.join("\n") + "\n").unwrap();
        assert_eq!(last_reply_in(&p).as_deref(), Some("First part.\n\nShall I go on?"));
    }

    #[test]
    fn the_last_reply_is_only_the_latest_turns() {
        let text = |t: &str| serde_json::json!({ "type": "assistant", "message": { "id": "m1", "content": [{ "type": "text", "text": t }] } }).to_string();
        let prompt = serde_json::json!({ "type": "user", "message": { "content": "Go on" } }).to_string();
        // The latest turn ran a tool and wrote no text: no reply, not the turn before's.
        let lines = [text("Old reply?"), prompt.clone(), call("t1", "Bash", r#"{"command":"ls"}"#), result("t1", r#""ok""#, false)];
        assert_eq!(last_reply(lines.iter().map(String::as_str)), None);
        // A prompt and nothing else after the old reply: none either.
        assert_eq!(last_reply([text("Old reply?"), prompt.clone()].iter().map(String::as_str)), None);
        // A prompt sent as text blocks counts too; a reply after it is the one.
        let blocks = serde_json::json!({ "type": "user", "message": { "content": [{ "type": "text", "text": "Go" }] } }).to_string();
        assert_eq!(last_reply([text("Old?"), blocks, text("New.")].iter().map(String::as_str)).as_deref(), Some("New."));
    }

    #[test]
    fn plain_text_keeps_lone_marks() {
        assert_eq!(plain_reply("See #108 and ~/build/*.tmp"), "See #108 and ~/build/*.tmp");
        assert_eq!(plain_reply("# Title\n\n###### Small"), "Title\n\nSmall");
        assert_eq!(plain_reply("**bold** *it* _it_ `code` ~~gone~~ a * b snake_case_name"), "bold it it code gone a * b snake_case_name");
    }

    #[test]
    fn a_permission_prompt_finds_its_open_call() {
        assert_eq!(permission_tool("Claude needs your permission to use Bash").as_deref(), Some("Bash"));
        assert_eq!(permission_tool("Claude needs your permission to use mcp__kanban__kanban_move_card.").as_deref(), Some("mcp__kanban__kanban_move_card"));
        assert_eq!(permission_tool("Claude is waiting for your input"), None);
        let par = |calls: &[(&str, &str, &str)]| {
            let blocks: Vec<Value> = calls.iter().map(|(id, n, i)| serde_json::json!({ "type": "tool_use", "id": id, "name": n, "input": serde_json::from_str::<Value>(i).unwrap() })).collect();
            serde_json::json!({ "type": "assistant", "message": { "content": blocks } }).to_string()
        };
        let text = |l: Option<(String, String)>| l.map(|(_, t)| t);
        // Parallel calls: a done Bash, the open Bash asking, then another tool last.
        let lines = [
            par(&[("a", "Bash", r#"{"command":"ls"}"#), ("b", "Bash", r#"{"command":"cargo test --workspace"}"#), ("c", "Read", r#"{"file_path":"x.rs"}"#)]),
            result("a", r#""ok""#, false),
            result("c", r#""text""#, false),
        ];
        assert_eq!(open_call(lines.iter().map(String::as_str), "Bash", false), Some(("b".to_string(), "Bash: cargo test --workspace".to_string())));
        // Two open calls to the tool: the last one is the one asking.
        let two = [call("p", "Bash", r#"{"command":"sleep 100"}"#), call("q", "Bash", r#"{"command":"git push"}"#)];
        assert_eq!(text(open_call(two.iter().map(String::as_str), "Bash", false)).as_deref(), Some("Bash: git push"));
        // No open call to that tool.
        assert_eq!(open_call(lines.iter().map(String::as_str), "Write", false), None);
        assert_eq!(text(open_call(lines[..1].iter().map(String::as_str), "Read", false)).as_deref(), Some("Read: x.rs"));
        // A subagent's prompt: nothing open in the main transcript, so its newest subagent's.
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("s.jsonl");
        std::fs::write(&main, format!("{}\n{}\n{}\n", lines[0], lines[1], result("b", r#""ok""#, false))).unwrap();
        assert_eq!(prompt_call(&main, "Bash"), None);
        let subs = subagents_dir(&main);
        std::fs::create_dir_all(&subs).unwrap();
        let side = |l: String| l.replacen("{", r#"{"isSidechain":true,"#, 1);
        std::fs::write(subs.join("agent-old.jsonl"), side(call("o", "Bash", r#"{"command":"old"}"#)) + "\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(subs.join("agent-new.jsonl"), side(call("n", "Bash", r#"{"command":"git status"}"#)) + "\n").unwrap();
        let found = prompt_call(&main, "Bash").unwrap();
        assert_eq!((found.id.as_str(), found.text.as_str(), found.sub), ("n", "Bash: git status", true));
        assert_eq!(found.path, subs.join("agent-new.jsonl"));
        // The main reader passes over sidechain lines.
        assert_eq!(open_call([side(call("z", "Bash", r#"{"command":"x"}"#)).as_str()].into_iter(), "Bash", false), None);
    }

    #[test]
    fn a_prompted_call_settles_on_its_result_or_a_later_message() {
        let msg = |id: &str, block: Value| serde_json::json!({ "type": "assistant", "message": { "id": id, "content": [block] } }).to_string();
        let tool = |id: &str| serde_json::json!({ "type": "tool_use", "id": id, "name": "Bash", "input": {} });
        let asking = msg("m1", tool("b"));
        // Waiting: the call alone, or with a parallel call of the same message after it.
        assert!(!call_settled([asking.as_str()].into_iter(), "b", false));
        assert!(!call_settled([asking.clone(), msg("m1", tool("c"))].iter().map(String::as_str), "b", false));
        // Its result came: approved (or turned down) and run.
        assert!(call_settled([asking.clone(), result("b", r#""ok""#, false)].iter().map(String::as_str), "b", false));
        // Claude went on with another message.
        let next = msg("m2", serde_json::json!({ "type": "text", "text": "Done." }));
        assert!(call_settled([asking.clone(), next.clone()].iter().map(String::as_str), "b", false));
        // Another call's result, or a subagent's line in the main transcript, isn't it.
        assert!(!call_settled([asking.clone(), result("x", r#""ok""#, false)].iter().map(String::as_str), "b", false));
        let side = next.replacen("{", r#"{"isSidechain":true,"#, 1);
        assert!(!call_settled([asking.clone(), side.clone()].iter().map(String::as_str), "b", false));
        // In a subagent's own transcript its lines count.
        let sub_ask = asking.replacen("{", r#"{"isSidechain":true,"#, 1);
        assert!(call_settled([sub_ask, side].iter().map(String::as_str), "b", true));
        // From a file.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.jsonl");
        std::fs::write(&p, format!("{asking}\n")).unwrap();
        assert!(!call_settled_in(&p, "b", false));
        std::fs::write(&p, format!("{asking}\n{}\n", result("b", r#""ok""#, false))).unwrap();
        assert!(call_settled_in(&p, "b", false));
    }
}
