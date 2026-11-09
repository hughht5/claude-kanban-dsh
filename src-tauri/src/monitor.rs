//! The stuck-session monitor (Hugh, card #99). Every `sessions.stuck_check_minutes` (10 by
//! default, 0 for off) it looks at each card session, in plain Rust with no Claude call. It
//! reads each session's transcript rather than more hooks: the transcript already holds every
//! tool call and its output, more hooks would add a round trip to every tool call, and sessions
//! already running would keep their old hooks.
//!
//! The checks (`check`), most pressing first:
//! - failed: the session's turn failed, or it crashed (`needs.rs`'s reasons);
//! - question: it has waited at an AskUserQuestion dialog for longer than the interval;
//! - repeating: working, and its last `REPEATS` calls (status polls skipped) are one call with
//!   one output;
//! - quiet: working, and nothing has written its transcript or a subagent's for `QUIET`;
//! - dead wait: idle for longer than the interval on an In progress card whose move to Human
//!   test still waits on a subagent check, with a subagent it started since that check was reset
//!   which never reported, wasn't stopped and has written nothing for the interval (a background
//!   tester killed by an app restart), and nothing running or waiting for it.
//! - idle: idle for longer than the interval on an In progress or Planning card, with no
//!   question, no subagent or test running, nothing waiting for it and no blockedReason (the
//!   ping-pong test's cards #187 and #188, 6 Nov: they sat idle for 30 minutes with nothing to
//!   do). One message typed at the idle prompt, then the Router or Hugh is told.
//! - A circle of "Blocked by" cards waits for the card that adds "Blocked by" (#109).
//!
//! What it does (`step`): the card's `stuck` field (`{kind, reason, since, nudged, notified}`)
//! shows a "May be stuck" chip. Repeating gets one nudge typed mid-turn (checked again just
//! before), dead wait and idle one message typed at the idle prompt, and each, still stuck at the next
//! check, a notify to Hugh. Quiet notifies Hugh at once, with no typing. Failed and question get
//! the chip only: `needs.rs` already pins them on the board. It clears when the condition does.
//! It never answers a question, approves anything, or stops, restarts or pauses a session.

use crate::sessions::{Sessions, Watch};
use crate::transcript;
use crate::Board;
use kanban_store::{Card, Patch, Store};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// The card field the monitor keeps.
pub const STUCK_KEY: &str = "stuck";
/// Who the monitor's writes and log lines are by.
pub const BY: &str = "the app (monitor)";
/// The same call with the same output this many times running is a loop.
pub const REPEATS: usize = 5;
/// How much of a transcript's end the repeat check reads. Claude Code stores each result twice
/// in its line (the message and `toolUseResult`), so 64 KB holds too few calls with long output.
pub const TAIL: u64 = 256 * 1024;
/// A working session whose transcripts nobody wrote for this long is quiet.
pub const QUIET: Duration = Duration::from_secs(20 * 60);
/// The longest a reason on the card runs, in bytes.
const REASON_MAX: usize = 160;
/// How often the monitor's thread wakes to see whether a check is due.
const WAKE: Duration = Duration::from_secs(30);

/// One tool call of the main conversation, since its latest prompt.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Call {
    pub name: String,
    /// Its key input (`transcript::key_input`), to show: the command, the path, the pattern.
    pub input: String,
    /// Its whole input, as JSON, to compare: the key input leaves out most of an Edit. Without
    /// the labels in `LABELS`, which don't change what the call does.
    pub full: String,
    /// Its whole result's text.
    pub output: String,
    pub is_error: bool,
    /// Its result has come.
    pub done: bool,
}

impl Call {
    /// The same call with the same result.
    fn same(&self, o: &Call) -> bool {
        self.name == o.name && self.full == o.full && self.output == o.output && self.is_error == o.is_error
    }
}

/// Input keys that only label a call. Claude can word a Bash call's `description` differently
/// each time it runs one command (#119's re-test: 40 identical `echo`s, each described
/// anew), so they aren't compared.
const LABELS: [&str; 1] = ["description"];

/// A call's input as compared: its JSON without the `LABELS`.
fn compared(input: &Value) -> String {
    match input {
        Value::Object(m) => Value::Object(m.iter().filter(|(k, _)| !LABELS.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect()).to_string(),
        v => v.to_string(),
    }
}

/// A `tool_result` block's whole text: its string, or its text blocks joined.
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

/// The main conversation's tool calls in `lines` (a transcript's tail) since its latest prompt,
/// oldest first. A prompt, a message read mid-turn (the monitor's nudge too) or a notice starts
/// the list again: calls before it are another stretch of work. A result whose call is cut off
/// the top is left out.
pub fn calls<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<Call> {
    let mut out: Vec<Call> = vec![];
    let mut ids: HashMap<String, usize> = HashMap::new();
    for line in lines {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v.get("isSidechain").and_then(Value::as_bool) == Some(true) || v.get("isMeta").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let content = v.get("message").and_then(|m| m.get("content"));
        let mut cut = false;
        match (v.get("type").and_then(Value::as_str).unwrap_or(""), content) {
            ("assistant", Some(Value::Array(blocks))) => {
                for b in blocks.iter().filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use")) {
                    let name = b.get("name").and_then(Value::as_str).unwrap_or("tool").to_string();
                    let input = b.get("input").cloned().unwrap_or(Value::Null);
                    if let Some(id) = b.get("id").and_then(Value::as_str) {
                        ids.insert(id.to_string(), out.len());
                    }
                    out.push(Call { input: transcript::key_input(&name, &input).unwrap_or_default(), full: compared(&input), name, ..Default::default() });
                }
            }
            ("user", Some(Value::String(t))) => cut = !t.trim().is_empty(),
            ("user", Some(Value::Array(blocks))) => {
                for b in blocks {
                    match b.get("type").and_then(Value::as_str) {
                        Some("tool_result") => {
                            let Some(&i) = b.get("tool_use_id").and_then(Value::as_str).and_then(|id| ids.get(id)) else { continue };
                            out[i].output = result_text(b.get("content"));
                            out[i].is_error = b.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                            out[i].done = true;
                        }
                        Some("text") => cut |= b.get("text").and_then(Value::as_str).is_some_and(|t| !t.trim().is_empty()),
                        _ => {}
                    }
                }
            }
            ("attachment", _) => cut = v.pointer("/attachment/type").and_then(Value::as_str) == Some("queued_command"),
            _ => {}
        }
        if cut {
            out.clear();
            ids.clear();
        }
    }
    out
}

/// Commands that only look at something, which sessions run again and again on purpose.
const POLLS: [&str; 9] = ["gh run view", "gh run watch", "gh pr checks", "gh pr view", "git status", "git fetch", "sleep", "kb card", "kb sessions"];

/// Whether one shell command (no `&&` or `;` in it) is a poll or a `cd`. In a pipeline only the
/// first command counts (`git status | head`).
fn poll_command(cmd: &str) -> bool {
    let first = cmd.split('|').next().unwrap_or("").trim();
    if first.is_empty() || first == "cd" || first.starts_with("cd ") || first.starts_with("Set-Location") {
        return true;
    }
    // `git -C <dir> status` reads as `git status`, `kb.exe card` as `kb card`.
    let mut c = first.to_string();
    if let Some(rest) = c.strip_prefix("git -C ") {
        c = rest.split_once(' ').map_or(String::new(), |(_, r)| format!("git {}", r.trim_start()));
    }
    if let Some(rest) = c.strip_prefix("kb.exe ") {
        c = format!("kb {rest}");
    }
    POLLS.iter().any(|p| c == *p || c.starts_with(&format!("{p} ")))
}

/// Whether a call is a status poll, which the repeat check skips: the Monitor tool, or a shell
/// command all of whose parts are polls or `cd`s.
pub fn is_poll(c: &Call) -> bool {
    match c.name.as_str() {
        "Monitor" => true,
        "Bash" | "PowerShell" => {
            let parts: Vec<&str> = c.input.split(|ch| ch == ';' || ch == '\n').flat_map(|p| p.split("&&")).filter(|p| !p.trim().is_empty()).collect();
            !parts.is_empty() && parts.iter().all(|p| poll_command(p))
        }
        _ => false,
    }
}

/// The call the session is looping on, and how many times running it made it with the same
/// whole input and the same whole result, counting back from its latest call and skipping
/// polls. None under `REPEATS`, and None while its latest call is still running: a session that
/// has moved on to a long build isn't looping.
pub fn repeating(calls: &[Call]) -> Option<(&Call, usize)> {
    let mut it = calls.iter().rev().filter(|c| !is_poll(c));
    let last = it.next().filter(|c| c.done)?;
    let n = 1 + it.take_while(|c| c.same(last)).count();
    (n >= REPEATS).then_some((last, n))
}

/// A short, fixed-length tag for what a hit is about (FNV-1a), kept on the card so that a
/// different loop is a new episode.
fn tag(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// What the monitor knows about one card's session at a check.
#[derive(Debug, Clone, Default)]
pub struct View {
    /// The live session's state ("working", "idle", "permission", "failed"), or "" for none.
    pub state: String,
    /// How long it has been in that state.
    pub in_state: Duration,
    pub column: String,
    /// `needs.rs`'s failure: "turn failed" or "crashed", or "usage limit".
    pub failed: Option<String>,
    /// A move to Human test still waits on a subagent check (the tester's or the reviewer's).
    pub gate_waiting: bool,
    /// Since anything last wrote its transcript or a subagent's.
    pub last_write: Option<Duration>,
    /// A subagent it started since the waiting check was reset, which never reported, wasn't
    /// stopped, and has written nothing for the interval: (its id, its type).
    pub dead_subagent: Option<(String, String)>,
    /// A subagent, review or test of it is running now (written within the interval, or the
    /// hooks' Testing).
    pub subagent_running: bool,
    /// A message or move waits for it, in the app's queue or typed and not yet taken.
    pub queued: bool,
    /// Its "permission" state is Claude's question dialog (else a permission prompt).
    pub question: bool,
    /// The card says what blocks it (`blockedReason`): it waits on Hugh, and the board shows it.
    pub blocked: bool,
}

/// One kind of stuck, with its reason for Hugh.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub kind: &'static str,
    pub reason: String,
    /// The typed text, for the kinds that type one (repeating, dead wait).
    pub message: Option<String>,
    /// What exactly it is stuck on (a tag of the repeated call, the dead subagent's id), so that
    /// a different one is a new episode and the same one isn't nudged twice.
    pub about: String,
}

fn mins(d: Duration) -> u64 {
    d.as_secs() / 60
}

/// Cuts a reason to `REASON_MAX` bytes, on one line.
pub fn short(s: &str) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let (t, cut) = transcript::truncate(&one, REASON_MAX);
    if cut {
        format!("{t}…")
    } else {
        t
    }
}

/// A call's input to quote, or None when it looks like it holds a secret (the store refuses
/// those, and they go on no card).
fn quotable(input: &str) -> Option<String> {
    let s = short(&mask(input));
    (!s.is_empty() && !kanban_store::looks_secret(&s)).then_some(s)
}

/// Token prefixes that are a secret by themselves.
const TOKEN_PREFIXES: [&str; 11] = ["ghp_", "gho_", "ghs_", "ghu_", "github_pat_", "xoxb-", "xoxp-", "sk-ant-", "sk_live", "sk_test", "akia"];

/// Whether one word of a command is, or holds, a secret: a known token, a long run of key
/// characters with letters and digits, a sign-in link, or the value of a token, key or
/// password setting.
fn secret_word(w: &str) -> bool {
    let bare = w.trim_matches(|c: char| "\"'`()[]{},;".contains(c)).to_ascii_lowercase();
    let value = bare.split_once('=').map_or(bare.as_str(), |(_, v)| v);
    let keyish = |s: &str| s.len() >= 32 && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) && s.chars().any(|c| c.is_ascii_digit()) && s.chars().any(|c| c.is_ascii_alphabetic());
    let named = bare.split_once('=').is_some_and(|(k, _)| ["token", "key", "secret", "password", "pass", "pwd", "auth"].iter().any(|n| k.trim_start_matches('-').ends_with(n)));
    TOKEN_PREFIXES.iter().any(|p| value.starts_with(p))
        || keyish(value)
        || bare.contains("signin")
        || bare.contains("sign-in")
        || (named && !value.is_empty())
}

/// `s` with its secrets hidden, for anything the monitor shows or stores (the router's rule):
/// each secret word, and the word after "Bearer" or "Authorization:", becomes "[hidden]"; then
/// the store's own mask.
pub fn mask(s: &str) -> String {
    let mut out: Vec<String> = vec![];
    let mut hide_next = false;
    for w in s.split_whitespace() {
        let lower = w.trim_matches(|c: char| "\"'`".contains(c)).to_ascii_lowercase();
        if hide_next || secret_word(w) {
            out.push("[hidden]".into());
            hide_next = false;
        } else {
            out.push(w.to_string());
        }
        if lower == "bearer" || lower == "basic" || lower.ends_with("authorization:") || lower == "-u" {
            hide_next = true;
        }
    }
    kanban_store::mask_secrets(&out.join(" "))
}

/// The nudge for a session looping on `c`, in ASD-STE100 (it is for an agent).
pub fn nudge_text(c: &Call, n: usize) -> String {
    let what = quotable(&c.input).map(|i| format!(" ({i})")).unwrap_or_default();
    format!(
        "The kanban app's monitor: you made the same {} call{what} {n} times, and each time the result was the same. Stop this loop. Find the cause, or use a different method. If you cannot continue, write the problem on your card and tell Hugh.",
        c.name
    )
}

/// The message for a session idle on a subagent that died, in ASD-STE100.
pub fn dead_wait_text(agent: &str, interval: Duration) -> String {
    format!(
        "The kanban app's monitor: your card is In progress and waits for a subagent check before Human test. Your subagent \"{agent}\" does not run now: it did not report, and it wrote nothing for {} minutes. Start it again. If you wait for something else, write on your card what you wait for.",
        mins(interval).max(1)
    )
}

/// The message for a session idle with nothing running, in ASD-STE100.
pub fn idle_text(column: &str, interval: Duration) -> String {
    format!(
        "The kanban app's monitor: your card is in {column}, and your session was idle for more than {} minutes. No turn, question, subagent or test runs, and no message waits for you. Continue the work of your column now. If you wait for something, write in nextStep what you wait for, and then stop. The monitor does not send this message again.",
        mins(interval).max(1)
    )
}

/// The columns where an idle session with nothing running has work it isn't doing.
pub const IDLE_COLUMNS: [&str; 2] = ["In progress", "Planning"];

/// Whether the card looks stuck, and how. `calls` is the main conversation's tail.
pub fn check(v: &View, calls: &[Call], interval: Duration) -> Option<Hit> {
    let hit = |kind, reason: String, message, about: String| Some(Hit { kind, reason: short(&reason), message, about });
    if let Some(f) = &v.failed {
        let reason = match f.as_str() {
            "crashed" => "Its session stopped on its own",
            "usage limit" => "Its turn failed on the usage limit",
            _ => "Its last turn failed",
        };
        return hit("failed", reason.into(), None, f.clone());
    }
    match v.state.as_str() {
        // The reasons hold no running time, so the field isn't rewritten at every check: the
        // panel shows since when.
        "permission" if v.in_state > interval && v.question => hit("question", "Waiting at a question".into(), None, String::new()),
        "permission" if v.in_state > interval => hit("permission", "Waiting at a permission prompt".into(), None, String::new()),
        "working" => {
            if let Some((c, n)) = repeating(calls) {
                let what = quotable(&c.input).map(|i| format!(": {i}")).unwrap_or_default();
                let about = tag(&format!("{}\u{1f}{}\u{1f}{}\u{1f}{}", c.name, c.full, c.output, c.is_error));
                return hit("repeating", format!("Made the same {} call {n} times with the same result{what}", c.name), Some(nudge_text(c, n)), about);
            }
            match v.last_write {
                Some(q) if q >= QUIET => hit("quiet", format!("Working, but nothing written for over {} min", mins(QUIET)), None, String::new()),
                _ => None,
            }
        }
        "idle" if v.in_state > interval && !v.subagent_running && !v.queued => {
            if let (true, true, Some((id, agent))) = (v.column == "In progress", v.gate_waiting, v.dead_subagent.as_ref()) {
                return hit("dead wait", format!("Idle, waiting on a {agent} subagent that stopped running"), Some(dead_wait_text(agent, interval)), id.clone());
            }
            // One episode for each column: the session's own turn after the message clears the
            // chip, and the next idle stretch in that column goes to the Router or Hugh, not
            // another message (`Memory`). A move to another column gets its own message.
            if !IDLE_COLUMNS.contains(&v.column.as_str()) || v.blocked {
                return None;
            }
            hit("idle", format!("Idle in {} with nothing running", v.column), Some(idle_text(&v.column, interval)), format!("idle:{}", v.column))
        }
        _ => None,
    }
}

/// The card's `stuck` field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stuck {
    pub kind: String,
    pub reason: String,
    pub since: String,
    /// `Hit::about`: a different one of the same kind is a new episode.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub about: String,
    /// Its one message was sent (or tried).
    #[serde(default)]
    pub nudged: bool,
    /// The message went in (false: the session couldn't take it).
    #[serde(default)]
    pub typed: bool,
    #[serde(default)]
    pub notified: bool,
}

/// One thing to do after a check.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// A line on the card's log.
    Log(String),
    /// Type this into the working session now, after checking it again (repeating).
    Nudge(String),
    /// Queue this for the session's idle prompt (dead wait, idle).
    Queue(String),
    /// Tell Hugh this (a log line and a toast).
    Notify(String),
}

/// What a check's result does, given the card's `stuck` field before it. Returns the field
/// after (None clears it) and the actions. One episode is one kind and one `about`; within it the
/// reason stays as first written, so the field changes only when something happens. A `Nudge`
/// or `Queue` is recorded as sent and typed; the caller sets `typed` false if the session
/// couldn't take it, and the next check then tells Hugh. `nudged_before`: this very thing had its
/// message in an earlier episode, so this one goes straight to Hugh.
pub fn step(prev: Option<&Stuck>, hit: Option<&Hit>, nudged_before: bool, now: &str) -> (Option<Stuck>, Vec<Action>) {
    let Some(hit) = hit else {
        let acts = prev.map(|p| vec![Action::Log(format!("No longer looks stuck (it was: {})", p.reason))]).unwrap_or_default();
        return (None, acts);
    };
    let mut acts = vec![];
    let mut s = match prev.filter(|p| p.kind == hit.kind && p.about == hit.about) {
        Some(p) => p.clone(),
        None => {
            acts.push(Action::Log(format!("May be stuck: {}", hit.reason)));
            Stuck { kind: hit.kind.into(), reason: hit.reason.clone(), since: now.into(), about: hit.about.clone(), nudged: nudged_before, typed: nudged_before, notified: false }
        }
    };
    // notify.rs puts the card's number in front of the toast.
    let tell = |what: &str| Action::Notify(format!("May be stuck: {}. {what}", s.reason));
    match hit.kind {
        "repeating" | "dead wait" | "idle" if !s.nudged => {
            let text = hit.message.clone().unwrap_or_default();
            acts.push(if hit.kind == "repeating" { Action::Nudge(text) } else { Action::Queue(text) });
            s.nudged = true;
            s.typed = true;
        }
        "repeating" | "dead wait" | "idle" if !s.notified => {
            acts.push(tell(if s.typed { "The monitor sent it one message, and it is still stuck." } else { "The monitor couldn't get its message into the session, and it is still stuck." }));
            s.notified = true;
        }
        "quiet" if !s.notified => {
            acts.push(tell("The monitor doesn't type into a working session, so have a look."));
            s.notified = true;
        }
        _ => {}
    }
    (Some(s), acts)
}

// ---- Feeding the checks ------------------------------------------------------------------

/// The last `max` bytes of the file, from its first whole line.
fn tail(path: &Path, max: u64) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(max);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    Some(if start == 0 { text } else { text.split_once('\n').map_or(String::new(), |(_, rest)| rest.to_string()) })
}

/// The main conversation's finished calls at the end of the transcript at `path`.
fn tail_calls(path: &Path) -> Vec<Call> {
    tail(path, TAIL).map(|t| calls(t.lines())).unwrap_or_default()
}

fn parse_at(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|t| t.with_timezone(&chrono::Utc))
}

fn modified(path: &Path) -> Option<chrono::DateTime<chrono::Utc>> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok().map(chrono::DateTime::<chrono::Utc>::from)
}

/// When the card's waiting subagent checks were reset (the earliest `at` of the
/// `RESET_ON_IN_PROGRESS` records that haven't passed): a subagent that started before that
/// isn't what the card waits on.
pub fn waiting_since(card: &Card) -> Option<chrono::DateTime<chrono::Utc>> {
    kanban_store::RESET_ON_IN_PROGRESS
        .iter()
        .filter_map(|k| card.fields.get(*k))
        .filter(|r| r.get("state").and_then(Value::as_str) != Some("passed"))
        .filter_map(|r| r.get("at").and_then(Value::as_str).and_then(parse_at))
        .min()
}

/// From a session's subagents: the newest one started after `since` that never reported, wasn't
/// stopped and hasn't written for `interval` (id, type); and whether any is running (written
/// within `interval`).
pub fn subagent_state(subs: &[transcript::Subagent], since: Option<chrono::DateTime<chrono::Utc>>, interval: Duration, now: chrono::DateTime<chrono::Utc>) -> (Option<(String, String)>, bool) {
    let quiet_for = |s: &transcript::Subagent| s.updated.as_deref().and_then(parse_at).map(|u| (now - u).to_std().unwrap_or_default());
    let open: Vec<&transcript::Subagent> = subs.iter().filter(|s| !s.finished && !s.stopped).collect();
    let running = open.iter().any(|s| quiet_for(s).map_or(false, |q| q < interval));
    let dead = since.and_then(|since| {
        open.iter()
            .filter(|s| s.started.as_deref().and_then(parse_at).is_some_and(|st| st > since))
            .filter(|s| quiet_for(s).is_some_and(|q| q >= interval))
            .last()
            .map(|s| (s.id.clone(), s.agent_type.clone()))
    });
    (dead, running)
}

/// What a StopFailure's `error` says, for the reason: "usage limit" for a rate or usage limit.
pub fn failure(detail: Option<&str>, error: Option<&str>) -> Option<String> {
    let detail = detail?;
    let e = error.unwrap_or("").to_ascii_lowercase();
    let limit = ["rate_limit", "rate limit", "usage_limit", "usage limit"].iter().any(|k| e.contains(k));
    Some(if detail == "turn failed" && limit { "usage limit".into() } else { detail.to_string() })
}

/// One card at a check: its view, the main conversation's tail calls, and its transcript.
struct Seen {
    card: Card,
    view: View,
    calls: Vec<Call>,
}

/// The monitor's memory between checks: what it has nudged, by card, so a thing that comes
/// back after a clear goes to Hugh rather than getting a second message.
#[derive(Default)]
pub struct Memory {
    nudged: HashMap<String, HashSet<String>>,
}

/// The transcript of a live session: its statusLine's report, else found by its id.
fn transcript_of(w: &Watch) -> Option<PathBuf> {
    w.transcript.clone().or_else(|| transcript::find(&w.account, &w.claude_session_id))
}

/// Builds one card's view. Reads files, holding no lock.
fn look(card: Card, w: Option<&Watch>, failed: Option<String>, gate_waiting: bool, queued: bool, interval: Duration) -> Seen {
    let now = chrono::Utc::now();
    let blocked = card.fields.get("blockedReason").and_then(Value::as_str).is_some_and(|r| !r.trim().is_empty());
    let mut view = View { column: card.column.clone(), failed, gate_waiting, queued, blocked, ..Default::default() };
    let mut calls = vec![];
    if let Some(w) = w {
        view.state = w.state.clone();
        view.in_state = w.in_state;
        view.queued |= w.typing;
        view.question = w.question;
        view.subagent_running = w.testing;
        if let Some(path) = transcript_of(w) {
            let subs = transcript::subagents(&path);
            let newest = std::iter::once(modified(&path)).chain(subs.iter().map(|s| s.updated.as_deref().and_then(parse_at))).flatten().max();
            view.last_write = newest.map(|t| (now - t).to_std().unwrap_or_default());
            let (dead, running) = subagent_state(&subs, waiting_since(&card), interval, now);
            view.dead_subagent = dead;
            view.subagent_running |= running;
            if w.state == "working" {
                calls = tail_calls(&path);
            }
        }
    }
    Seen { card, view, calls }
}

/// Whether the session is still looping on the same thing, just before the nudge is typed:
/// working, no question open, nothing holding its typing, and its tail still that loop.
fn still_repeating(live: &Sessions, card_id: &str, about: &str, interval: Duration) -> bool {
    let Some(w) = live.watch_one(card_id) else { return false };
    if w.state != "working" || w.question || w.held || !w.ready {
        return false;
    }
    let Some(path) = transcript_of(&w) else { return false };
    let v = View { state: w.state.clone(), in_state: w.in_state, ..Default::default() };
    check(&v, &tail_calls(&path), interval).is_some_and(|h| h.kind == "repeating" && h.about == about)
}

/// Writes the card's `stuck` field (None removes it). A reason the store refuses as
/// secret-looking is written as the kind alone.
fn write_stuck(store: &mut Store, card_id: &str, s: Option<&Stuck>) -> bool {
    let Ok(card) = store.get(card_id) else { return false };
    let value = |s: Option<&Stuck>| s.map_or(Value::Null, |s| json!(s));
    if card.fields.get(STUCK_KEY).cloned().unwrap_or(Value::Null) == value(s) {
        return false;
    }
    let mut patch = Patch::new();
    patch.insert(STUCK_KEY.into(), value(s));
    match store.update(card_id, card.version, &patch, BY) {
        Ok(_) => true,
        Err(kanban_store::Error::Secret(_)) => {
            let plain = s.map(|s| Stuck { reason: format!("May be stuck ({})", s.kind), ..s.clone() });
            patch.insert(STUCK_KEY.into(), value(plain.as_ref()));
            store.update(card_id, card.version, &patch, BY).is_ok()
        }
        Err(_) => false,
    }
}

fn log(store: &mut Store, card_id: &str, text: &str) {
    if store.add_log(card_id, text, BY).is_err() {
        // A line the store refuses as secret-looking: say so without the text.
        let _ = store.add_log(card_id, "The monitor had something to log that looked like a secret, so it isn't shown", BY);
    }
}

/// One check over every card. `interval` 0 clears every card's `stuck` field (the monitor is off).
pub fn check_all(app: &AppHandle, memory: &mut Memory, interval: Duration) {
    let live = app.state::<Arc<Sessions>>().inner().clone();
    let watches: HashMap<String, Watch> = live.watch().into_iter().map(|w| (w.card_id.clone(), w)).collect();
    let needs = crate::needs::current(app);
    // What needs the store, under its lock; then the files with no lock.
    let gathered: Vec<(Card, Option<String>, bool, bool)> = {
        let board = app.state::<Board>();
        let Ok(store) = board.0.lock() else { return };
        let Ok(cards) = store.list(false) else { return };
        cards
            .into_iter()
            .filter(|c| watches.contains_key(&c.id) || needs.get(&c.id).is_some_and(|n| n.reason == "failed") || c.fields.contains_key(STUCK_KEY))
            .map(|c| {
                let err = watches.get(&c.id).and_then(|w| w.fail_error.as_deref());
                let failed = needs.get(&c.id).filter(|n| n.reason == "failed").and_then(|n| failure(n.detail, err));
                let gate = crate::agents::move_refused(&store, &c, "Human test").is_some();
                let queued = store.waiting(&c.id, "message").ok().flatten().is_some() || store.waiting(&c.id, "move").ok().flatten().is_some();
                (c, failed, gate, queued)
            })
            .collect()
    };
    let mut changed = false;
    for (card, failed, gate, queued) in gathered {
        let off = interval.is_zero() || card.column == "Done";
        let seen = if off { Seen { card, view: View::default(), calls: vec![] } } else { { let w = watches.get(&card.id); look(card, w, failed, gate, queued, interval) } };
        let hit = if off { None } else { check(&seen.view, &seen.calls, interval) };
        changed |= act(app, &live, memory, &seen.card, hit.as_ref(), interval);
    }
    if changed {
        let _ = app.emit("cards-changed", ());
    }
}

/// Runs `step` for one card and does what it says. Returns whether the card changed.
fn act(app: &AppHandle, live: &Arc<Sessions>, memory: &mut Memory, card: &Card, hit: Option<&Hit>, interval: Duration) -> bool {
    let prev: Option<Stuck> = card.fields.get(STUCK_KEY).and_then(|v| serde_json::from_value(v.clone()).ok());
    if prev.is_none() && hit.is_none() {
        // Nothing before, nothing now; a field the monitor can't read is cleared.
        if card.fields.contains_key(STUCK_KEY) {
            let board = app.state::<Board>();
            let Ok(mut store) = board.0.lock() else { return false };
            return write_stuck(&mut store, &card.id, None);
        }
        return false;
    }
    let before = hit.is_some_and(|h| !h.about.is_empty() && memory.nudged.get(&card.id).is_some_and(|s| s.contains(&h.about)));
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let (mut next, acts) = step(prev.as_ref(), hit, before, &now);
    for a in acts {
        match a {
            Action::Nudge(text) => {
                let about = hit.map(|h| h.about.clone()).unwrap_or_default();
                // Checked again at the moment of typing, and never typed at an open question
                // (`type_now` refuses that too). Recorded as the app's before it is typed, so the
                // chat never reads it unmarked (card #153); a nudge not typed matches nothing.
                if let Ok(mut st) = app.state::<Board>().0.lock() {
                    let _ = st.note_app_message(&card.id, &text);
                }
                let typed =still_repeating(live, &card.id, &about, interval) && !crate::router::restarting(&card.id) && live.type_now(&card.id, &text, true).unwrap_or(false);
                if !typed {
                    if let Some(s) = next.as_mut() {
                        s.typed = false; // Hugh is told at the next check if it is still stuck
                    }
                }
                let board = app.state::<Board>();
                let Ok(mut store) = board.0.lock() else { continue };
                if typed {
                    memory.nudged.entry(card.id.clone()).or_default().insert(about);
                    log(&mut store, &card.id, "Typed a nudge into the session: it was making the same call with the same result over and over");
                } else {
                    log(&mut store, &card.id, "Couldn't type a nudge into the session (it was busy with a question, a screen or a switch); you are told at the next check if it is still stuck");
                }
            }
            Action::Queue(text) => {
                let board = app.state::<Board>();
                let Ok(mut store) = board.0.lock() else {
                    if let Some(s) = next.as_mut() {
                        s.typed = false;
                    }
                    continue;
                };
                if store.queue(&card.id, "message", &text).is_ok() {
                    if let Some(h) = hit {
                        memory.nudged.entry(card.id.clone()).or_default().insert(h.about.clone());
                    }
                    let why = if hit.is_some_and(|h| h.kind == "idle") { "it sat idle with nothing running" } else { "the subagent it waits on stopped running" };
                    log(&mut store, &card.id, &format!("Queued a message for the session's idle prompt: {why}"));
                } else if let Some(s) = next.as_mut() {
                    s.typed = false;
                }
            }
            Action::Notify(text) => {
                // Card #119 (Hugh's decision): while the Router runs and takes events, it gets the
                // finding and decides (nudge, restart, reassign or tell Hugh). Down or stuck, Hugh
                // is told as before. `notified` stays a bool: "told" is the same either way.
                if crate::routersession::monitor_finding(app, card, &text) {
                    let board = app.state::<Board>();
                    let Ok(mut store) = board.0.lock() else { continue };
                    log(&mut store, &card.id, &format!("Told the Router: {text}"));
                    continue;
                }
                let board = app.state::<Board>();
                let Ok(mut store) = board.0.lock() else { continue };
                log(&mut store, &card.id, &format!("Told Hugh: {text}"));
                drop(store);
                let _ = app.emit("notify", json!({ "card": card.id, "number": card.number, "text": text }));
            }
            Action::Log(text) => {
                let board = app.state::<Board>();
                let Ok(mut store) = board.0.lock() else { continue };
                log(&mut store, &card.id, &text);
            }
        }
    }
    let board = app.state::<Board>();
    let Ok(mut store) = board.0.lock() else { return false };
    write_stuck(&mut store, &card.id, next.as_ref()) || prev.is_some() != next.is_some()
}

/// Starts the monitor's thread. It wakes every `WAKE` and runs a check once the setting's
/// minutes have passed since the last one (the first comes one interval after the app
/// starts), so a changed setting applies without a restart. Off (0), it clears every chip once.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut memory = Memory::default();
        let mut last = Instant::now();
        let mut cleared = false;
        loop {
            std::thread::sleep(WAKE);
            let mins = {
                let board = app.state::<Board>();
                let Ok(store) = board.0.lock() else { continue };
                store.stuck_check_minutes().unwrap_or(kanban_store::STUCK_CHECK_DEFAULT)
            };
            if mins <= 0 {
                if !cleared {
                    check_all(&app, &mut memory, Duration::ZERO);
                    cleared = true;
                }
                last = Instant::now();
                continue;
            }
            cleared = false;
            let interval = Duration::from_secs(mins as u64 * 60);
            if last.elapsed() >= interval {
                last = Instant::now();
                check_all(&app, &mut memory, interval);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);

    fn call(name: &str, input: &str, output: &str) -> Call {
        Call { name: name.into(), input: input.into(), full: input.into(), output: output.into(), is_error: false, done: true }
    }

    fn bash(cmd: &str) -> Call {
        call("Bash", cmd, "")
    }

    /// A transcript tail of `n` Bash calls of `cmd`, each answered with `outputs(i)`, stored
    /// twice in its line as Claude Code does.
    fn lines(n: usize, cmd: &str, outputs: impl Fn(usize) -> String) -> Vec<String> {
        let mut v = vec![];
        for i in 0..n {
            v.push(json!({ "type": "assistant", "timestamp": "t", "message": { "content": [{ "type": "tool_use", "id": format!("tu{i}"), "name": "Bash", "input": { "command": cmd } }] } }).to_string());
            v.push(json!({ "type": "user", "timestamp": "t", "message": { "content": [{ "type": "tool_result", "tool_use_id": format!("tu{i}"), "content": outputs(i), "is_error": true }] }, "toolUseResult": { "stdout": outputs(i) } }).to_string());
        }
        v
    }

    fn working(last_write: Option<Duration>) -> View {
        View { state: "working".into(), in_state: 30 * MIN, column: "In progress".into(), last_write, ..Default::default() }
    }

    #[test]
    fn the_same_call_with_the_same_output_five_times_is_a_loop() {
        let l = lines(5, "cargo test", |_| "error[E0425]: cannot find value".into());
        let c = calls(l.iter().map(String::as_str));
        assert_eq!(c.len(), 5);
        assert_eq!((c[0].name.as_str(), c[0].input.as_str(), c[0].is_error), ("Bash", "cargo test", true));
        let (looped, n) = repeating(&c).unwrap();
        assert_eq!((looped.input.as_str(), n), ("cargo test", 5));
        let h = check(&working(Some(MIN)), &c, 10 * MIN).unwrap();
        assert_eq!(h.kind, "repeating");
        assert_eq!(h.reason, "Made the same Bash call 5 times with the same result: cargo test");
        let m = h.message.unwrap();
        assert!(m.contains("the same Bash call (cargo test) 5 times") && m.contains("Stop this loop"), "{m}");
        // Four is not yet a loop.
        let four = calls(lines(4, "cargo test", |_| "e".into()).iter().map(String::as_str));
        assert!(repeating(&four).is_none());
    }

    #[test]
    fn the_same_call_with_a_different_output_is_not_a_loop() {
        let l = lines(6, "cargo test", |i| format!("{} tests failed", 6 - i));
        assert!(repeating(&calls(l.iter().map(String::as_str))).is_none());
        // A different call last breaks the run.
        let mut c: Vec<Call> = (0..5).map(|_| bash("cargo build")).collect();
        c.push(bash("cargo test"));
        assert!(repeating(&c).is_none());
        // An error and a success with the same text differ.
        let mut c: Vec<Call> = (0..4).map(|_| bash("x")).collect();
        c.push(Call { is_error: true, ..bash("x") });
        assert!(repeating(&c).is_none());
        // A call whose result hasn't come yet isn't counted.
        let mut l = lines(4, "x", |_| "same".into());
        l.push(json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": "open", "name": "Bash", "input": { "command": "x" } }] } }).to_string());
        assert!(repeating(&calls(l.iter().map(String::as_str))).is_none());
    }

    #[test]
    fn only_calls_since_the_latest_prompt_count() {
        let mut l = lines(5, "cargo test", |_| "same".into());
        l.push(json!({ "type": "user", "message": { "content": "Hugh: try the other fix" } }).to_string());
        assert!(calls(l.iter().map(String::as_str)).is_empty());
        // A message read mid-turn (the monitor's nudge too) starts again as well.
        let mut l = lines(5, "cargo test", |_| "same".into());
        l.push(json!({ "type": "attachment", "attachment": { "type": "queued_command", "prompt": "stop" } }).to_string());
        l.extend(lines(2, "cargo test", |_| "same".into()));
        assert_eq!(calls(l.iter().map(String::as_str)).len(), 2);
        // A loop that moved on to a long build isn't looping while that runs.
        let mut l = lines(5, "cargo test", |_| "same".into());
        l.push(json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": "long", "name": "Bash", "input": { "command": "cargo build --release" } }] } }).to_string());
        let c = calls(l.iter().map(String::as_str));
        assert_eq!((c.len(), c[5].done), (6, false));
        assert!(repeating(&c).is_none());
    }

    #[test]
    fn a_loop_with_a_new_description_each_time_is_still_a_loop() {
        // #119's re-test: one echo command run over and over, each call described differently.
        let mut l = vec![];
        for i in 0..40 {
            l.push(json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": format!("d{i}"), "name": "Bash", "input": { "command": "echo still waiting", "description": format!("Check again, try {i}") } }] } }).to_string());
            l.push(json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": format!("d{i}"), "content": "still waiting" }] } }).to_string());
        }
        let c = calls(l.iter().map(String::as_str));
        let (r, n) = repeating(&c).expect("a loop");
        assert_eq!((r.name.as_str(), r.input.as_str(), n), ("Bash", "echo still waiting", 40));
        // The command and the other inputs still count: another command, or the same command
        // with another timeout, is another call.
        assert_ne!(compared(&json!({ "command": "echo a", "description": "x" })), compared(&json!({ "command": "echo b", "description": "x" })));
        assert_ne!(compared(&json!({ "command": "echo a", "timeout": 1 })), compared(&json!({ "command": "echo a", "timeout": 2 })));
        assert_eq!(compared(&json!({ "command": "echo a", "description": "x" })), compared(&json!({ "command": "echo a", "description": "y" })));
    }

    #[test]
    fn edits_are_compared_whole_and_outputs_in_full() {
        // Five Edits to one file whose old text starts the same are progress, not a loop.
        let edit = |i: usize| {
            vec![
                json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": format!("e{i}"), "name": "Edit", "input": { "file_path": "a.rs", "old_string": format!("    }}\nline {i}"), "new_string": format!("    }}\nnew {i}") } }] } }).to_string(),
                json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": format!("e{i}"), "content": "The file a.rs has been updated successfully." }] } }).to_string(),
            ]
        };
        let l: Vec<String> = (0..5).flat_map(edit).collect();
        let c = calls(l.iter().map(String::as_str));
        assert_eq!(c[0].input, c[1].input, "the shown input is the same");
        assert!(repeating(&c).is_none());
        // Results that differ only after the first 4 KB differ.
        let l = lines(5, "cargo test", |i| format!("{}{} failed", "x".repeat(5000), i));
        assert!(repeating(&calls(l.iter().map(String::as_str))).is_none());
    }

    #[test]
    fn status_polls_are_skipped() {
        for cmd in ["gh run view 123 --log", "gh run watch 9", "gh pr checks 12", "gh pr view 12 --json state", "git status --short", "git fetch origin", "sleep 30", "kb card 99", "kb.exe sessions", "cd C:/x && git status", "git -C C:/Users/h/kanban-app-dsh status", "git status | head -5"] {
            assert!(is_poll(&bash(cmd)), "{cmd}");
        }
        assert!(is_poll(&call("Monitor", "{}", "")));
        for cmd in ["cargo test", "git status && cargo build", "gh pr create", "kb move 9 Done", "sleepy", "cd x && npm test", ""] {
            assert!(!is_poll(&bash(cmd)), "{cmd:?}");
        }
        assert!(!is_poll(&call("Read", "git status", "")));
        // Polling every minute for an hour is no loop...
        let polls: Vec<Call> = (0..30).map(|_| bash("gh run view 1")).collect();
        assert!(repeating(&polls).is_none());
        // ...and polls between the repeats neither count nor break the run.
        let mut c = vec![];
        for _ in 0..5 {
            c.push(bash("cargo test"));
            c.push(bash("git status"));
        }
        assert_eq!(repeating(&c).map(|(_, n)| n), Some(5));
    }

    #[test]
    fn a_secret_in_the_repeated_command_stays_off_the_card() {
        let token = "curl -H \"Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.payload\" https://api.github.com";
        assert!(kanban_store::looks_secret(token), "the test needs a command the store calls secret");
        let c: Vec<Call> = (0..5).map(|_| bash(token)).collect();
        let h = check(&working(Some(MIN)), &c, 10 * MIN).unwrap();
        assert!(h.reason.starts_with("Made the same Bash call 5 times with the same result: curl") && h.reason.contains("[hidden]"), "{}", h.reason);
        assert!(!h.reason.contains("eyJ") && !h.message.unwrap().contains("eyJ"));
    }

    #[test]
    fn tokens_keys_and_sign_in_links_are_masked() {
        for (raw, kept) in [
            ("curl -H 'Authorization: Bearer abc.def' https://x", "https://x"),
            ("gh api -H Authorization:token ghp_0123456789abcdefghijABCDEFGHIJ012345 /user", "/user"),
            ("open http://127.0.0.1:5173/dev-sign-in.html?as=admin", "open"),
            ("curl https://x/sign-in-token?t=1", "curl"),
            ("deploy --api-key=hunter2 now", "now"),
            ("export TOKEN=abc", "export"),
            ("aws s3 ls AKIAABCDEFGHIJKLMNOP", "aws s3 ls"),
            ("use sk-ant-api03-abcdefgh here", "here"),
            ("curl -u hugh:pw https://x", "https://x"),
            ("x 0123456789abcdef0123456789abcdef01234567 y", "y"),
        ] {
            let m = mask(raw);
            assert!(m.contains("[hidden]") && m.contains(kept), "{raw} -> {m}");
            for bad in ["abc.def", "ghp_", "dev-sign-in", "sign-in-token", "hunter2", "=abc", "AKIA", "sk-ant", "hugh:pw", "0123456789abcdef0123"] {
                assert!(!m.contains(bad), "{raw} -> {m} holds {bad}");
            }
        }
        // Ordinary commands are left alone.
        for ok in ["cargo test --workspace", "git -C C:/Users/h/kanban-app-99 log --oneline -3", "npm run build -- --mode=test"] {
            assert_eq!(mask(ok), ok);
        }
    }

    #[test]
    fn quiet_is_twenty_minutes_of_nothing_written_while_working() {
        assert!(check(&working(Some(19 * MIN)), &[], 10 * MIN).is_none());
        let h = check(&working(Some(25 * MIN)), &[], 10 * MIN).unwrap();
        assert_eq!((h.kind, h.reason.as_str(), h.message), ("quiet", "Working, but nothing written for over 20 min", None));
        // Idle, a session is never quiet: it is waiting for Hugh.
        let idle = View { state: "idle".into(), ..working(Some(60 * MIN)) };
        assert!(check(&idle, &[], 10 * MIN).map_or(true, |h| h.kind != "quiet"));
        // No transcript found: no guess.
        assert!(check(&working(None), &[], 10 * MIN).is_none());
        // A loop outranks quiet.
        let c: Vec<Call> = (0..5).map(|_| bash("x")).collect();
        assert_eq!(check(&working(Some(25 * MIN)), &c, 10 * MIN).unwrap().kind, "repeating");
    }

    #[test]
    fn a_question_counts_after_the_interval_and_a_failure_at_once() {
        let q = |m| View { state: "permission".into(), in_state: m * MIN, question: true, ..Default::default() };
        assert!(check(&q(9), &[], 10 * MIN).is_none());
        let h = check(&q(14), &[], 10 * MIN).unwrap();
        assert_eq!((h.kind, h.reason.as_str(), h.message), ("question", "Waiting at a question", None));
        // The same state from a tool's permission prompt says so, not "a question".
        let h = check(&View { question: false, ..q(14) }, &[], 10 * MIN).unwrap();
        assert_eq!((h.kind, h.reason.as_str()), ("permission", "Waiting at a permission prompt"));
        // No running time in a reason: the field isn't rewritten at every check.
        assert_eq!(check(&q(80), &[], 10 * MIN).unwrap().reason, "Waiting at a question");
        for (f, reason) in [("turn failed", "Its last turn failed"), ("crashed", "Its session stopped on its own"), ("usage limit", "Its turn failed on the usage limit")] {
            let v = View { failed: Some(f.into()), ..q(30) };
            let h = check(&v, &[], 10 * MIN).unwrap();
            assert_eq!((h.kind, h.reason.as_str(), h.message), ("failed", reason, None));
        }
        assert_eq!(failure(Some("turn failed"), Some("rate_limit")).as_deref(), Some("usage limit"));
        assert_eq!(failure(Some("turn failed"), Some("server_error")).as_deref(), Some("turn failed"));
        assert_eq!(failure(Some("crashed"), Some("rate_limit")).as_deref(), Some("crashed"));
        assert_eq!(failure(None, Some("rate_limit")), None);
        // Other limits aren't the usage limit.
        assert_eq!(failure(Some("turn failed"), Some("context_limit_exceeded")).as_deref(), Some("turn failed"));
        assert_eq!(failure(Some("turn failed"), Some("max_output_tokens_limit")).as_deref(), Some("turn failed"));
    }

    fn dead_wait() -> View {
        View {
            state: "idle".into(),
            in_state: 12 * MIN,
            column: "In progress".into(),
            gate_waiting: true,
            dead_subagent: Some(("a1".into(), "card-tester".into())),
            last_write: Some(12 * MIN),
            ..Default::default()
        }
    }

    #[test]
    fn idle_on_a_dead_subagent_is_a_dead_wait() {
        let h = check(&dead_wait(), &[], 10 * MIN).unwrap();
        assert_eq!((h.kind, h.about.as_str()), ("dead wait", "a1"));
        assert_eq!(h.reason, "Idle, waiting on a card-tester subagent that stopped running");
        assert!(h.message.unwrap().contains("Your subagent \"card-tester\" does not run now"));
        // Any one of these and it isn't one: too soon, another column, nothing to wait on, a
        // subagent still running, something queued for it, or no dead subagent (it is idle
        // after asking Hugh something, say).
        let not = [
            View { in_state: 9 * MIN, ..dead_wait() },
            View { column: "Human test".into(), ..dead_wait() },
            View { gate_waiting: false, ..dead_wait() },
            View { subagent_running: true, ..dead_wait() },
            View { queued: true, ..dead_wait() },
            View { dead_subagent: None, ..dead_wait() },
            View { state: "working".into(), ..dead_wait() },
        ];
        for v in not {
            assert!(check(&v, &[], 10 * MIN).map_or(true, |h| h.kind != "dead wait"), "{v:?}");
        }
    }

    fn idle(column: &str, m: u32) -> View {
        View { state: "idle".into(), in_state: m * MIN, column: column.into(), last_write: Some(m * MIN), ..Default::default() }
    }

    #[test]
    fn idle_with_nothing_running_in_progress_or_planning_is_stuck() {
        // The ping-pong test's cards: idle in In progress for 30 minutes, nothing running.
        let h = check(&idle("In progress", 30), &[], 10 * MIN).unwrap();
        assert_eq!((h.kind, h.reason.as_str(), h.about.as_str()), ("idle", "Idle in In progress with nothing running", "idle:In progress"));
        assert!(h.message.unwrap().contains("If you wait for something, write in nextStep what you wait for"));
        assert_eq!(check(&idle("Planning", 12), &[], 10 * MIN).unwrap().about, "idle:Planning");
        // Not one: too soon, another column, a subagent or test running, a message queued, a
        // blockedReason, a question or prompt, or working.
        let not = [
            idle("In progress", 9),
            idle("Human test", 30),
            idle("Backlog", 30),
            View { subagent_running: true, ..idle("In progress", 30) },
            View { queued: true, ..idle("In progress", 30) },
            View { blocked: true, ..idle("In progress", 30) },
            View { state: "working".into(), ..idle("In progress", 5) },
        ];
        for v in not {
            assert!(check(&v, &[], 10 * MIN).is_none(), "{v:?}");
        }
        let q = View { state: "permission".into(), question: true, ..idle("In progress", 30) };
        assert_eq!(check(&q, &[], 10 * MIN).unwrap().kind, "question");
        // A dead subagent it waits on is the more exact reason.
        let d = View { gate_waiting: true, dead_subagent: Some(("a1".into(), "card-tester".into())), ..idle("In progress", 30) };
        assert_eq!(check(&d, &[], 10 * MIN).unwrap().kind, "dead wait");
    }

    #[test]
    fn idle_gets_one_message_then_the_router_or_hugh() {
        let h = check(&idle("In progress", 30), &[], 10 * MIN).unwrap();
        // First check: the chip and one message for the idle prompt.
        let (s, a) = step(None, Some(&h), false, "t1");
        assert_eq!(a, vec![Action::Log("May be stuck: Idle in In progress with nothing running".into()), Action::Queue(h.message.clone().unwrap())]);
        // Still idle at the next check (it didn't take the message): told once.
        let (s, a) = step(s.as_ref(), Some(&h), false, "t2");
        assert!(matches!(&a[..], [Action::Notify(t)] if t.contains("sent it one message")), "{a:?}");
        assert_eq!(step(s.as_ref(), Some(&h), false, "t3").1, vec![], "told once");
        // It took the message (a turn clears the chip), then sat idle again in the same column:
        // told at once, no second message, so the monitor and the session can't ping-pong.
        let (s, _) = step(s.as_ref(), None, false, "t4");
        let (s, a) = step(s.as_ref(), Some(&h), true, "t5");
        assert!(matches!(&a[..], [Action::Log(_), Action::Notify(_)]), "{a:?}");
        assert!(s.unwrap().notified);
        // A move to Planning is a new episode with its own message.
        let p = check(&idle("Planning", 30), &[], 10 * MIN).unwrap();
        assert_ne!(p.about, h.about);
    }

    fn sub(id: &str, started: &str, updated: &str, finished: bool, stopped: bool) -> transcript::Subagent {
        transcript::Subagent { id: id.into(), agent_type: format!("type-{id}"), description: String::new(), tool_id: None, finished, stopped, started: Some(started.into()), updated: Some(updated.into()) }
    }

    #[test]
    fn a_dead_subagent_is_unfinished_unstopped_quiet_and_started_since_the_reset() {
        let now = parse_at("2026-11-06T12:00:00Z").unwrap();
        let since = parse_at("2026-11-06T10:00:00Z");
        let old = sub("old", "2026-11-06T09:00:00Z", "2026-11-06T09:30:00Z", false, false);
        let stopped = sub("stop", "2026-11-06T11:00:00Z", "2026-11-06T11:05:00Z", false, true);
        let done = sub("done", "2026-11-06T11:00:00Z", "2026-11-06T11:05:00Z", true, false);
        let dead = sub("dead", "2026-11-06T11:00:00Z", "2026-11-06T11:20:00Z", false, false);
        let (d, running) = subagent_state(&[old.clone(), stopped.clone(), done.clone(), dead.clone()], since, 10 * MIN, now);
        assert_eq!((d, running), (Some(("dead".into(), "type-dead".into())), false));
        // Without it: none (the older one is from before the reset).
        assert_eq!(subagent_state(&[old, stopped, done], since, 10 * MIN, now), (None, false));
        // No reset time known: no evidence, so no dead wait.
        assert_eq!(subagent_state(&[dead.clone()], None, 10 * MIN, now).0, None);
        // One written five minutes ago is running, not dead.
        let busy = sub("busy", "2026-11-06T11:50:00Z", "2026-11-06T11:55:00Z", false, false);
        assert_eq!(subagent_state(&[dead, busy], since, 10 * MIN, now), (Some(("dead".into(), "type-dead".into())), true));
    }

    #[test]
    fn the_reset_time_is_the_earliest_waiting_check() {
        let card = |f: Value| Card { id: "c".into(), number: 1, column: "In progress".into(), order: None, order_col: None, archived: false, fields: f.as_object().cloned().unwrap(), version: 1 };
        let c = card(json!({ "test": { "state": "needed", "at": "2026-11-06T10:00:00.000Z" }, "review": { "state": "failed", "at": "2026-11-06T11:00:00.000Z" } }));
        assert_eq!(waiting_since(&c), parse_at("2026-11-06T10:00:00Z"));
        let c = card(json!({ "test": { "state": "passed", "at": "2026-11-06T10:00:00.000Z" }, "review": { "state": "needed", "at": "2026-11-06T11:00:00.000Z" } }));
        assert_eq!(waiting_since(&c), parse_at("2026-11-06T11:00:00Z"));
        assert_eq!(waiting_since(&card(json!({ "test": { "state": "needed" } }))), None);
    }

    fn hit(kind: &'static str) -> Hit {
        Hit { kind, reason: format!("{kind} reason"), message: Some(format!("{kind} text")), about: format!("{kind} about") }
    }

    #[test]
    fn repeating_nudges_once_then_notifies_once_then_clears() {
        let (s, a) = step(None, Some(&hit("repeating")), false, "t1");
        let s = s.unwrap();
        assert_eq!(a, vec![Action::Log("May be stuck: repeating reason".into()), Action::Nudge("repeating text".into())]);
        assert_eq!((s.since.as_str(), s.nudged, s.notified), ("t1", true, false));
        let (s2, a) = step(Some(&s), Some(&hit("repeating")), true, "t2");
        let s2 = s2.unwrap();
        assert_eq!(a, vec![Action::Notify("May be stuck: repeating reason. The monitor sent it one message, and it is still stuck.".into())]);
        assert_eq!((s2.since.as_str(), s2.nudged, s2.notified), ("t1", true, true));
        let (s3, a) = step(Some(&s2), Some(&hit("repeating")), true, "t3");
        assert_eq!((s3.as_ref(), a.len()), (Some(&s2), 0), "nothing more while it stays stuck");
        let (s4, a) = step(Some(&s2), None, true, "t4");
        assert_eq!((s4, a), (None, vec![Action::Log("No longer looks stuck (it was: repeating reason)".into())]));
        assert_eq!(step(None, None, false, "t5"), (None, vec![]));
    }

    #[test]
    fn a_nudge_that_could_not_be_typed_goes_to_hugh_at_the_next_check() {
        let (s, _) = step(None, Some(&hit("repeating")), false, "t1");
        // The caller couldn't type it (a screen a slash command opened, say), so typed is false.
        let s = Stuck { typed: false, ..s.unwrap() };
        let (s2, a) = step(Some(&s), Some(&hit("repeating")), false, "t2");
        assert_eq!(a, vec![Action::Notify("May be stuck: repeating reason. The monitor couldn't get its message into the session, and it is still stuck.".into())]);
        assert!(s2.unwrap().notified);
    }

    #[test]
    fn a_different_loop_or_dead_subagent_is_a_new_episode() {
        let (s, _) = step(None, Some(&hit("repeating")), false, "t1");
        let (s, _) = step(s.as_ref(), Some(&hit("repeating")), false, "t2");
        assert!(s.as_ref().unwrap().notified);
        let other = Hit { about: "another call".into(), reason: "loop B".into(), ..hit("repeating") };
        let (s2, a) = step(s.as_ref(), Some(&other), false, "t3");
        let s2 = s2.unwrap();
        assert_eq!((s2.since.as_str(), s2.reason.as_str(), s2.about.as_str(), s2.notified), ("t3", "loop B", "another call", false));
        assert_eq!(a, vec![Action::Log("May be stuck: loop B".into()), Action::Nudge("repeating text".into())]);
        // Within one episode the reason stays as first written.
        let same = Hit { reason: "loop B, 7 times".into(), ..other.clone() };
        assert_eq!(step(Some(&s2), Some(&same), false, "t4").0.unwrap().reason, "loop B");
    }

    #[test]
    fn the_same_thing_back_after_a_clear_goes_to_hugh_not_another_message() {
        // A dead wait cleared once its session took the message and went idle again; the same
        // dead subagent at the next check is told to Hugh, with no second message.
        let (s, a) = step(None, Some(&hit("dead wait")), true, "t");
        assert!(matches!(&a[..], [Action::Log(_), Action::Notify(_)]), "{a:?}");
        assert!(s.unwrap().notified);
    }

    #[test]
    fn each_kind_does_its_own_thing() {
        let (_, a) = step(None, Some(&hit("dead wait")), false, "t");
        assert_eq!(a[1], Action::Queue("dead wait text".into()));
        let (s, a) = step(None, Some(&hit("quiet")), false, "t");
        assert!(matches!(&a[1], Action::Notify(t) if t.contains("doesn't type into a working session")));
        assert!(s.as_ref().unwrap().notified);
        assert_eq!(step(s.as_ref(), Some(&hit("quiet")), false, "t2").1, vec![], "quiet notifies once");
        for k in ["failed", "question"] {
            let (s, a) = step(None, Some(&hit(k)), false, "t");
            assert_eq!(a, vec![Action::Log(format!("May be stuck: {k} reason"))], "chip only for {k}");
            assert!(!s.unwrap().nudged);
        }
        // Another kind is a new episode: its own since, nudge and notify.
        let (q, _) = step(None, Some(&hit("quiet")), false, "t1");
        let (r, a) = step(q.as_ref(), Some(&hit("repeating")), false, "t2");
        let r = r.unwrap();
        assert_eq!((r.kind.as_str(), r.since.as_str(), r.notified), ("repeating", "t2", false));
        assert!(matches!(a[1], Action::Nudge(_)));
    }

    #[test]
    fn reasons_are_short_and_on_one_line() {
        let r = short(&format!("a\n  b {}", "x".repeat(400)));
        assert!(r.starts_with("a b x") && r.ends_with('…') && r.len() <= REASON_MAX + '…'.len_utf8());
        let s = Stuck { kind: "quiet".into(), reason: "r".into(), since: "t".into(), about: String::new(), nudged: false, typed: false, notified: true };
        assert_eq!(serde_json::to_value(&s).unwrap(), json!({ "kind": "quiet", "reason": "r", "since": "t", "nudged": false, "typed": false, "notified": true }));
        let a = Stuck { about: "a1".into(), ..s.clone() };
        assert_eq!(serde_json::to_value(&a).unwrap()["about"], "a1");
        assert!(!serde_json::from_value::<Stuck>(json!({ "kind": "q", "reason": "r", "since": "t" })).unwrap().nudged);
    }

    #[test]
    fn the_tail_starts_at_a_whole_line() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.jsonl");
        let mut body = String::new();
        for l in lines(60, "cargo test", |_| "x".repeat(3000)) {
            body.push_str(&l);
            body.push('\n');
        }
        std::fs::write(&p, &body).unwrap();
        assert!(body.len() as u64 > TAIL);
        let c = tail_calls(&p);
        assert!(c.len() >= REPEATS && c.len() < 60, "{}", c.len());
        assert!(repeating(&c).is_some());
    }

    #[test]
    fn a_stuck_field_on_the_card_is_written_cleared_and_never_holds_a_secret() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let c = s.create(None, json!({ "title": "T" }).as_object().cloned().unwrap(), "t").unwrap();
        let st = Stuck { kind: "quiet".into(), reason: "Working, but nothing written for over 20 min".into(), since: "t".into(), about: String::new(), nudged: false, typed: false, notified: true };
        assert!(write_stuck(&mut s, &c.id, Some(&st)));
        assert!(!write_stuck(&mut s, &c.id, Some(&st)), "the same field again is no write");
        let got: Stuck = serde_json::from_value(s.get(&c.id).unwrap().fields[STUCK_KEY].clone()).unwrap();
        assert_eq!(got, st);
        let secret = Stuck { reason: "token eyJhbGciOiJIUzI1NiJ9.x".into(), ..st.clone() };
        assert!(write_stuck(&mut s, &c.id, Some(&secret)));
        assert_eq!(s.get(&c.id).unwrap().fields[STUCK_KEY]["reason"], "May be stuck (quiet)");
        assert!(write_stuck(&mut s, &c.id, None));
        assert!(!s.get(&c.id).unwrap().fields.contains_key(STUCK_KEY));
        // A session can't write it with kanban_update_card.
        assert!(!crate::tools::UPDATABLE.contains(&STUCK_KEY));
    }
}
