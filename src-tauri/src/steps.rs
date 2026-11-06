//! Step progress on each card (Hugh, card #106): "Step 4 of 7: Writing the tests", from the to-do
//! list the card's Claude Code session keeps.
//!
//! Claude Code took its to-do tools away from Opus 4.8, Sonnet 5, Fable 5 and newer models; the
//! app gives them back to card sessions with `CLAUDE_CODE_ENABLE_TODO_TOOLS=1` when Hugh's
//! setting `sessions.todo_tools` is on (`sessions::build_env`). The session then has TaskCreate,
//! TaskUpdate, TaskGet and TaskList, as deferred tools it loads with ToolSearch.
//!
//! Where the list is read from:
//! - **The task folder first**: Claude Code keeps the list it shows the model as one small file per
//!   task in `<account's config folder>\tasks\<session id>\<n>.json`. No replay, no guessing.
//! - **The transcript when that folder doesn't exist or holds no task**: Claude Code (2.1.319)
//!   deletes the task files once every task is completed, so a finished list lives on only in
//!   the transcript. A replay of the main conversation's
//!   TaskCreate/TaskUpdate calls (keyed by task id, so the repeated blocks Claude Code writes are
//!   harmless) and TodoWrite (a whole list). Read from a stored offset, a line at a time.
//! - The desktop app's `update_plan` isn't read: its `advance`/`done`/`start` forms change the list
//!   relative to the last one, and desktop-app cards are left out (README).
//!
//! Only this visit's steps count: tasks still open, and tasks finished since the card's last move
//! into In progress (or into Planning, while it is in Planning). A new session has a new folder.
//!
//! Locks: every file read here runs on the worker thread with no lock held. The board lock and
//! the Sessions lock are each taken briefly to look something up and let go before any file is
//! opened; the summaries' own lock is taken last and only to swap a result in. Code holding the
//! board lock may read the summaries (`kanban_get_card` does), never the other way round.

use crate::{sessions::Sessions, transcript, Board};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

/// One task, as the card shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub id: String,
    pub subject: String,
    /// The present-tense form Claude Code shows while it works on it ("Writing the tests").
    pub active_form: Option<String>,
    /// "pending", "in_progress" or "completed".
    pub status: String,
    /// When it last changed: the task file's modified time, or the transcript line's timestamp.
    pub changed: Option<DateTime<Utc>>,
}

fn status_of(s: &str) -> &'static str {
    match s {
        "completed" => "completed",
        "in_progress" => "in_progress",
        _ => "pending",
    }
}

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn id_text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// A task file's JSON (`{id, subject, activeForm?, status, ...}`). None when it doesn't parse, has
/// no subject, or was deleted.
pub fn parse_task(json_text: &str, changed: Option<DateTime<Utc>>) -> Option<Task> {
    let v: Value = serde_json::from_str(json_text).ok()?;
    let status = v.get("status").and_then(Value::as_str).unwrap_or("pending");
    if status == "deleted" {
        return None;
    }
    Some(Task {
        id: id_text(v.get("id"))?,
        subject: text(&v, "subject")?,
        active_form: text(&v, "activeForm"),
        status: status_of(status).to_string(),
        changed,
    })
}

/// Sorts task ids as numbers ("2" before "10"); any other id after them, by its text.
fn id_order(id: &str) -> (u64, String) {
    (id.parse().unwrap_or(u64::MAX), id.to_string())
}

/// A session's task folder, when its id is a plain session id (never a path).
pub fn task_dir(config_dir: &Path, session_id: &str) -> Option<PathBuf> {
    let ok = !session_id.is_empty() && session_id.len() <= 64 && session_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then(|| config_dir.join("tasks").join(session_id))
}

/// The tasks in a session's task folder that the card shows: None when the folder doesn't exist
/// or holds no task, so the transcript replay takes over. Claude Code deletes the task files once
/// every task is completed (keeping only `.highwatermark` and `.lock`), and "All 3 steps done"
/// would otherwise vanish from the card seconds after the last step (card #106's final test).
pub fn folder_tasks(dir: &Path) -> Option<Vec<Task>> {
    read_folder(dir).filter(|t| !t.is_empty())
}

/// The tasks in a session's task folder, in id order; None when the folder doesn't exist.
/// Lock files, hidden files and files that don't parse are skipped.
pub fn read_folder(dir: &Path) -> Option<Vec<Task>> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut tasks: Vec<Task> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || !name.ends_with(".json") {
                return None;
            }
            let changed = e.metadata().ok().and_then(|m| m.modified().ok()).map(DateTime::<Utc>::from);
            parse_task(&std::fs::read_to_string(e.path()).ok()?, changed)
        })
        .collect();
    tasks.sort_by_key(|t| id_order(&t.id));
    Some(tasks)
}

/// The to-do list replayed from a transcript's main conversation: the fallback when a session's
/// task folder doesn't exist.
#[derive(Debug, Default)]
pub struct Replay {
    pub tasks: Vec<Task>,
    /// TaskCreate calls waiting for their result, which carries the new task's id.
    creates: HashMap<String, (String, Option<String>)>,
    /// How far into the transcript it has read (whole lines only).
    pub offset: u64,
}

/// The id in a TaskCreate result: "Task #3 created successfully: ...".
fn created_id(s: &str) -> Option<String> {
    let rest = &s[s.find("Task #")? + "Task #".len()..];
    let end = rest.find(char::is_whitespace)?;
    let (id, after) = rest.split_at(end);
    (after.trim_start().starts_with("created successfully") && !id.is_empty()).then(|| id.to_string())
}

fn result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

impl Replay {
    /// Applies one transcript line. Lines that can't touch the list are passed over before any
    /// JSON parse.
    pub fn feed(&mut self, line: &str) {
        if !["TaskCreate", "TaskUpdate", "TodoWrite", "created successfully"].iter().any(|k| line.contains(k)) {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            return;
        }
        let at = v.get("timestamp").and_then(Value::as_str).and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|t| t.with_timezone(&Utc));
        let Some(blocks) = v.pointer("/message/content").and_then(Value::as_array) else { return };
        for b in blocks {
            match b.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    let input = b.get("input").cloned().unwrap_or(Value::Null);
                    match b.get("name").and_then(Value::as_str) {
                        Some("TaskCreate") => {
                            if let (Some(id), Some(subject)) = (b.get("id").and_then(Value::as_str), text(&input, "subject")) {
                                self.creates.insert(id.to_string(), (subject, text(&input, "activeForm")));
                            }
                        }
                        Some("TaskUpdate") => self.update(&input, at),
                        Some("TodoWrite") => self.todo_write(&input, at),
                        _ => {}
                    }
                }
                Some("tool_result") => {
                    let Some(call) = b.get("tool_use_id").and_then(Value::as_str) else { continue };
                    let Some((subject, active_form)) = self.creates.get(call).cloned() else { continue };
                    if b.get("is_error").and_then(Value::as_bool) == Some(true) {
                        continue;
                    }
                    let id = created_id(&result_text(b.get("content"))).or_else(|| id_text(v.pointer("/toolUseResult/task/id")));
                    let Some(id) = id else { continue };
                    // A repeated block of a create already seen changes nothing.
                    if !self.tasks.iter().any(|t| t.id == id) {
                        self.tasks.push(Task { id, subject, active_form, status: "pending".into(), changed: at });
                    }
                }
                _ => {}
            }
        }
    }

    fn update(&mut self, input: &Value, at: Option<DateTime<Utc>>) {
        let Some(id) = id_text(input.get("taskId")) else { return };
        let status = input.get("status").and_then(Value::as_str);
        if status == Some("deleted") {
            self.tasks.retain(|t| t.id != id);
            return;
        }
        let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) else { return };
        if let Some(s) = status {
            t.status = status_of(s).to_string();
        }
        if let Some(s) = text(input, "subject") {
            t.subject = s;
        }
        if let Some(a) = text(input, "activeForm") {
            t.active_form = Some(a);
        }
        t.changed = at;
    }

    /// TodoWrite sends the whole list each time. An item that didn't change keeps its time.
    fn todo_write(&mut self, input: &Value, at: Option<DateTime<Utc>>) {
        let Some(todos) = input.get("todos").and_then(Value::as_array) else { return };
        let before = std::mem::take(&mut self.tasks);
        for (i, x) in todos.iter().enumerate() {
            let Some(subject) = text(x, "content") else { continue };
            let status = status_of(x.get("status").and_then(Value::as_str).unwrap_or("")).to_string();
            let same = before.iter().find(|t| t.subject == subject && t.status == status);
            self.tasks.push(Task {
                id: format!("todo-{}", i + 1),
                changed: same.map_or(at, |t| t.changed),
                subject,
                active_form: text(x, "activeForm"),
                status,
            });
        }
    }

    /// Reads the whole lines added to `path` since the last read. A half-written last line waits
    /// for the next read; a file that got shorter (a new transcript at that path) starts over.
    pub fn read_new(&mut self, path: &Path) -> std::io::Result<()> {
        let mut f = std::fs::File::open(path)?;
        if f.metadata()?.len() < self.offset {
            *self = Replay::default();
        }
        f.seek(SeekFrom::Start(self.offset))?;
        let mut r = std::io::BufReader::with_capacity(256 * 1024, f);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            let n = r.read_until(b'\n', &mut buf)?;
            if n == 0 || buf.last() != Some(&b'\n') {
                return Ok(());
            }
            self.offset += n as u64;
            self.feed(&String::from_utf8_lossy(&buf));
        }
    }
}

/// The first `max` bytes of a file, as text.
fn head(path: &Path, max: u64) -> Option<String> {
    let mut buf = Vec::new();
    std::fs::File::open(path).ok()?.take(max).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Whether a session offers the to-do tools, from the start of its transcript, where Claude Code
/// lists its deferred tools (`deferred_tools_delta`). None until that list is written.
pub fn offers_task_tools(head: &str) -> Option<bool> {
    let named = head.contains("\"TaskCreate\"") || head.contains("\"TodoWrite\"");
    if named {
        return Some(true);
    }
    head.contains("deferred_tools_delta").then_some(false)
}

/// Keeps this visit's steps: every task still open, and the finished ones that changed at or
/// after `since` (the card's last move into the column its steps count from). A finished task
/// with no known time stays.
pub fn cut(tasks: Vec<Task>, since: Option<DateTime<Utc>>) -> Vec<Task> {
    let Some(since) = since else { return tasks };
    tasks.into_iter().filter(|t| t.status != "completed" || t.changed.map_or(true, |c| c >= since)).collect()
}

/// The column whose last entry starts a card's list: Planning while it is planned, In progress
/// for the build and everything after it. None (every task counts) in Backlog.
pub fn cut_column(column: &str) -> Option<&'static str> {
    match column {
        "Backlog" => None,
        "Planning" => Some("Planning"),
        _ => Some("In progress"),
    }
}

/// A step's text with anything secret hidden (the router's rule 5), and ordinary words left alone
/// ("Add the password reset form", "--select=fast"): a token or key becomes "[hidden]"
/// (`kanban_store::mask_tokens`), so does a Bearer header's token, and a step that holds a sign-in
/// link (a signin or sign-in URL) shows as "(hidden)".
pub fn mask(s: &str) -> String {
    let l = s.to_ascii_lowercase();
    let link = (l.contains("signin") || l.contains("sign-in")) && (l.contains("http") || l.contains("?as=") || l.contains("token="));
    if link {
        return "(hidden)".into();
    }
    let mut out = kanban_store::mask_tokens(s);
    // "Bearer <token>": the token (12 characters or more) goes.
    let mut from = 0;
    while let Some(i) = out[from..].to_ascii_lowercase().find("bearer ").map(|i| i + from) {
        let start = i + "bearer ".len();
        let end = out[start..].find(|c: char| c.is_whitespace() || c == '\'' || c == '"').map_or(out.len(), |e| start + e);
        if end - start >= 12 {
            out.replace_range(start..end, "[hidden]");
        }
        from = start;
    }
    out
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Item {
    pub text: String,
    pub status: String,
}

/// What a card shows: `line` ("Step 4 of 7: Writing the tests"), the counts for its bar, and
/// every step for its tooltip.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Summary {
    pub done: usize,
    pub total: usize,
    /// The step in progress, counted from 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<usize>,
    pub line: String,
    pub items: Vec<Item>,
}

pub fn summarise(tasks: &[Task]) -> Option<Summary> {
    if tasks.is_empty() {
        return None;
    }
    let total = tasks.len();
    let done = tasks.iter().filter(|t| t.status == "completed").count();
    let current = tasks.iter().position(|t| t.status == "in_progress");
    let line = match current {
        Some(i) => {
            let t = &tasks[i];
            format!("Step {} of {total}: {}", i + 1, mask(t.active_form.as_deref().unwrap_or(&t.subject)))
        }
        None if done == total && total == 1 => "1 of 1 step done".to_string(),
        None if done == total => format!("All {total} steps done"),
        None => {
            let next = tasks.iter().find(|t| t.status != "completed").map(|t| mask(&t.subject)).unwrap_or_default();
            format!("{done} of {total} done · next: {next}")
        }
    };
    Some(Summary {
        done,
        total,
        current: current.map(|i| i + 1),
        line,
        items: tasks.iter().map(|t| Item { text: mask(&t.subject), status: t.status.clone() }).collect(),
    })
}

/// Claude Code's projects folder name for a working folder: every character that isn't a letter
/// or a digit becomes "-" (`C:\Users\someone\kanban-app` is `C--Users-someone-kanban-app`).
pub fn project_folder(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// Where a card's session is, as the worker needs it.
#[derive(Debug, Clone, PartialEq)]
struct Source {
    /// The app's session row.
    row: String,
    account: String,
    /// The conversation's current id: its task folder's name. A live session's statusLine says
    /// it (it changes on /clear); otherwise the row's.
    session: String,
    cwd: Option<String>,
    transcript: Option<PathBuf>,
}

enum Job {
    /// A statusLine report of session row `row`.
    Report { row: String, session: String, transcript: Option<PathBuf> },
    /// Read every card in Planning, In progress and Human test, and every card showing steps
    /// (at start and every minute).
    Sweep,
}

/// The worker's own state for a card, never shared.
#[derive(Default)]
struct Work {
    source: Option<Source>,
    replay: Option<Replay>,
    /// Whether the transcript offers the tools (the fallback's gate). Some(false) is final for
    /// the session: its deferred tools are listed and TaskCreate isn't among them.
    offers: Option<bool>,
    /// The transcript the fallback reads, once looked for (Some(None): not found).
    path: Option<Option<PathBuf>>,
    /// (the card's column, the cut's time): looked up again when the column changes, and at
    /// most once a minute (each sweep clears it), never on every report.
    since: Option<(String, Option<DateTime<Utc>>)>,
}

pub struct Steps {
    summaries: Mutex<HashMap<String, Summary>>,
    tx: Mutex<mpsc::Sender<Job>>,
    /// Session rows started with the to-do tools on, not yet checked for TaskCreate.
    flagged: Mutex<HashSet<String>>,
}

impl Steps {
    /// Starts the worker thread; it reads the board once a few seconds after start, then every minute.
    pub fn start(app: AppHandle) -> Arc<Steps> {
        let (tx, rx) = mpsc::channel();
        let me = Arc::new(Steps { summaries: Mutex::new(HashMap::new()), tx: Mutex::new(tx), flagged: Mutex::new(HashSet::new()) });
        let weak = Arc::downgrade(&me);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(3));
            let mut work: HashMap<String, Work> = HashMap::new();
            let mut jobs = vec![Job::Sweep];
            loop {
                let Some(me) = weak.upgrade() else { return };
                for job in coalesce(std::mem::take(&mut jobs)) {
                    me.run(&app, &mut work, job);
                }
                drop(me);
                match rx.recv_timeout(Duration::from_secs(60)) {
                    Ok(j) => {
                        jobs.push(j);
                        jobs.extend(rx.try_iter());
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => jobs.push(Job::Sweep),
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        });
        me
    }

    fn send(&self, job: Job) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(job);
        }
    }

    /// A card's steps, when its session keeps a list.
    pub fn get(&self, card_id: &str) -> Option<Summary> {
        self.summaries.lock().ok()?.get(card_id).cloned()
    }

    /// Marks a session row as started with the to-do tools on, so its first transcript is checked
    /// for TaskCreate (a later Claude Code could drop the flag and the card would show nothing).
    pub fn flag(&self, row_id: &str) {
        if let Ok(mut f) = self.flagged.lock() {
            f.insert(row_id.to_string());
        }
    }

    fn set(&self, app: &AppHandle, card_id: &str, s: Option<Summary>) {
        let changed = {
            let Ok(mut m) = self.summaries.lock() else { return };
            let changed = m.get(card_id) != s.as_ref();
            match s {
                Some(s) => {
                    m.insert(card_id.to_string(), s);
                }
                None => {
                    m.remove(card_id);
                }
            }
            changed
        };
        if changed {
            let _ = app.emit("steps-changed", json!({ "card": card_id }));
        }
    }

    fn run(&self, app: &AppHandle, work: &mut HashMap<String, Work>, job: Job) {
        match job {
            Job::Report { row, session, transcript } => {
                // Each lock on its own, let go before the next and before any file read. The live
                // session's own row gives the account (a report can come before the row is stored).
                let live = app.state::<Arc<Sessions>>();
                let Some(card) = live.card_of_row(&row) else { return };
                let Some(r) = live.is_live(&card).filter(|r| r.id == row) else { return };
                self.check_flag(app, &card, &row, transcript.as_deref());
                let src = Source { row, account: r.account, session, cwd: Some(r.cwd), transcript };
                self.refresh(app, work.entry(card.clone()).or_default(), &card, src);
            }
            Job::Sweep => {
                let showing: HashSet<String> = self.summaries.lock().map(|m| m.keys().cloned().collect()).unwrap_or_default();
                let (keep, due) = {
                    let board = app.state::<Board>();
                    let Ok(store) = board.0.lock() else { return };
                    let cards = store.list(false).unwrap_or_default();
                    let keep: HashSet<String> = cards.iter().map(|c| c.id.clone()).collect();
                    let due: Vec<(String, Source)> = cards
                        .iter()
                        .filter(|c| matches!(c.column.as_str(), "Planning" | "In progress" | "Human test") || showing.contains(&c.id))
                        .filter_map(|c| {
                            let r = store.latest_session(&c.id).ok().flatten()?;
                            Some((c.id.clone(), Source { row: r.id, account: r.account, session: r.claude_session_id, cwd: Some(r.cwd), transcript: None }))
                        })
                        .collect();
                    (keep, due)
                };
                // Archived cards show nothing.
                for id in showing.iter().filter(|k| !keep.contains(*k)) {
                    work.remove(id);
                    self.set(app, id, None);
                }
                for (id, mut src) in due {
                    let w = work.entry(id.clone()).or_default();
                    // The same session row: keep what its reports said (its current id, transcript).
                    if let Some(old) = w.source.as_ref().filter(|o| o.row == src.row) {
                        src.session = old.session.clone();
                        src.transcript = old.transcript.clone();
                    }
                    w.since = None;
                    self.refresh(app, w, &id, src);
                }
            }
        }
    }

    /// Reads a card's steps and swaps them in. Files are read with no lock held.
    fn refresh(&self, app: &AppHandle, w: &mut Work, card_id: &str, src: Source) {
        if w.source.as_ref().map_or(true, |o| o.session != src.session || o.account != src.account) {
            w.replay = None;
            w.offers = None;
            w.path = None;
        }
        let (archived, since) = {
            let board = app.state::<Board>();
            let Ok(store) = board.0.lock() else { return };
            let Ok(card) = store.get(card_id) else { return };
            let since = match w.since.as_ref().filter(|(col, _)| *col == card.column) {
                Some((_, s)) => *s,
                None => cut_column(&card.column)
                    .and_then(|col| store.entered_at(card_id, col).ok().flatten())
                    .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
                    .map(|t| t.with_timezone(&Utc)),
            };
            w.since = Some((card.column.clone(), since));
            (card.archived, since)
        };
        if archived {
            w.source = Some(src);
            self.set(app, card_id, None);
            return;
        }
        let config = transcript::config_dir(&src.account);
        let tasks = match task_dir(&config, &src.session).and_then(|d| folder_tasks(&d)) {
            Some(t) => Some(t),
            None => Self::fallback(w, &src),
        };
        w.source = Some(src);
        self.set(app, card_id, tasks.and_then(|t| summarise(&cut(t, since))));
    }

    /// The transcript replay, for a session with no task in its folder whose transcript offers the
    /// tools. The transcript is looked for once per session, and a session whose deferred tools
    /// leave out TaskCreate is never read again.
    fn fallback(w: &mut Work, src: &Source) -> Option<Vec<Task>> {
        if w.offers == Some(false) {
            return None;
        }
        if src.transcript.is_some() {
            w.path = Some(src.transcript.clone());
        }
        let path = w
            .path
            .get_or_insert_with(|| {
                let in_cwd = src.cwd.as_deref().map(|cwd| {
                    transcript::config_dir(&src.account).join("projects").join(project_folder(cwd)).join(format!("{}.jsonl", src.session))
                });
                in_cwd.filter(|p| p.is_file()).or_else(|| transcript::find(&src.account, &src.session))
            })
            .clone()?;
        if w.offers.is_none() {
            w.offers = head(&path, 256 * 1024).and_then(|h| offers_task_tools(&h));
        }
        if w.offers != Some(true) {
            return None;
        }
        let replay = w.replay.get_or_insert_with(Replay::default);
        replay.read_new(&path).ok()?;
        Some(replay.tasks.clone())
    }

    /// Once per session started with the tools on: if its transcript lists its deferred tools and
    /// TaskCreate isn't among them, says so on the card.
    fn check_flag(&self, app: &AppHandle, card_id: &str, row: &str, transcript: Option<&Path>) {
        if !self.flagged.lock().map_or(false, |f| f.contains(row)) {
            return;
        }
        let Some(found) = transcript.and_then(|p| head(p, 256 * 1024)).and_then(|h| offers_task_tools(&h)) else { return };
        if let Ok(mut f) = self.flagged.lock() {
            f.remove(row);
        }
        if !found {
            {
                let board = app.state::<Board>();
                let Ok(mut store) = board.0.lock() else { return };
                let _ = store.add_log(card_id, WARNING, "the app");
            }
            let _ = app.emit("cards-changed", ());
        }
    }
}

const WARNING: &str = "This session started with the to-do tools on, but its Claude Code offers no TaskCreate: no step progress will show.";

/// Several reports of one session in a row need only the last; a sweep after them still runs.
fn coalesce(jobs: Vec<Job>) -> Vec<Job> {
    let mut sweep = false;
    let mut reports: Vec<Job> = Vec::new();
    for j in jobs {
        match j {
            Job::Sweep => sweep = true,
            Job::Report { ref row, .. } => {
                reports.retain(|r| !matches!(r, Job::Report { row: x, .. } if x == row));
                reports.push(j);
            }
        }
    }
    if sweep {
        reports.push(Job::Sweep);
    }
    reports
}

/// A statusLine report (`dispatch::on_status`): queues a read of that session's steps. It returns
/// at once; the worker does the reading.
pub fn report(app: &AppHandle, row_id: &str, body: &Value) {
    let Some(steps) = app.try_state::<Arc<Steps>>() else { return };
    let Some(session) = body.get("session_id").and_then(Value::as_str) else { return };
    let transcript = body.get("transcript_path").and_then(Value::as_str).map(PathBuf::from);
    steps.send(Job::Report { row: row_id.to_string(), session: session.to_string(), transcript });
}

/// Every card's steps, for the board: `{ cardId: { done, total, current?, line, items } }`.
#[tauri::command]
pub fn card_steps(steps: State<Arc<Steps>>) -> Value {
    json!(steps.summaries.lock().map(|m| m.clone()).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Option<DateTime<Utc>> {
        Some(DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc))
    }

    fn task(id: &str, subject: &str, status: &str, changed: &str) -> Task {
        Task { id: id.into(), subject: subject.into(), active_form: None, status: status.into(), changed: t(changed) }
    }

    #[test]
    fn a_task_file_parses_and_deleted_or_broken_ones_dont() {
        let x = parse_task(r#"{"id":"3","subject":"Write the tests","activeForm":"Writing the tests","status":"in_progress","blocks":[],"blockedBy":[]}"#, None).unwrap();
        assert_eq!((x.id.as_str(), x.subject.as_str(), x.active_form.as_deref(), x.status.as_str()), ("3", "Write the tests", Some("Writing the tests"), "in_progress"));
        assert_eq!(parse_task(r#"{"id":4,"subject":"A"}"#, None).unwrap().status, "pending", "a number id, no status");
        assert!(parse_task(r#"{"id":"1","subject":"A","status":"deleted"}"#, None).is_none());
        assert!(parse_task(r#"{"id":"1","subject":"  "}"#, None).is_none());
        assert!(parse_task("not json", None).is_none());
    }

    #[test]
    fn the_task_folder_is_read_in_id_order_skipping_locks_and_bad_files() {
        let dir = tempfile::tempdir().unwrap();
        let w = |n: &str, s: &str| std::fs::write(dir.path().join(n), s).unwrap();
        w("10.json", r#"{"id":"10","subject":"Ten","status":"pending"}"#);
        w("2.json", r#"{"id":"2","subject":"Two","status":"completed"}"#);
        w("1.json", r#"{"id":"1","subject":"One","status":"completed"}"#);
        w(".lock", "");
        w(".highwatermark", "10");
        w("3.json", "{broken");
        w("notes.txt", "x");
        let got = read_folder(dir.path()).unwrap();
        assert_eq!(got.iter().map(|t| t.subject.as_str()).collect::<Vec<_>>(), ["One", "Two", "Ten"]);
        assert!(got.iter().all(|t| t.changed.is_some()), "each has its file's modified time");
        assert!(read_folder(&dir.path().join("missing")).is_none(), "no folder is None, not an empty list");
        std::fs::create_dir(dir.path().join("empty")).unwrap();
        assert_eq!(read_folder(&dir.path().join("empty")), Some(vec![]));
    }

    // Claude Code deletes the task files when every task is completed; the folder keeps only
    // `.highwatermark` and `.lock`. The card then reads the transcript, so a finished list stays.
    #[test]
    fn an_emptied_task_folder_falls_back_to_the_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let tasks = dir.path().join("tasks");
        std::fs::create_dir(&tasks).unwrap();
        std::fs::write(tasks.join("1.json"), r#"{"id":"1","subject":"A","status":"completed"}"#).unwrap();
        assert_eq!(folder_tasks(&tasks).unwrap().len(), 1, "a folder with tasks is read");
        std::fs::remove_file(tasks.join("1.json")).unwrap();
        std::fs::write(tasks.join(".highwatermark"), "3").unwrap();
        std::fs::write(tasks.join(".lock"), "").unwrap();
        assert_eq!(read_folder(&tasks), Some(vec![]), "the folder is there");
        assert_eq!(folder_tasks(&tasks), None, "but it holds no task, so the replay takes over");
        assert_eq!(folder_tasks(&dir.path().join("missing")), None);

        // The replay of that session's transcript still has the finished list.
        let t = dir.path().join("t.jsonl");
        let lines = [
            call("c1", "TaskCreate", r#"{"subject":"Read the first note"}"#, "2026-11-06T10:00:00Z"),
            created("c1", "1", "Read the first note", "2026-11-06T10:00:01Z", true),
            call("c2", "TaskCreate", r#"{"subject":"Read the second note"}"#, "2026-11-06T10:00:02Z"),
            created("c2", "2", "Read the second note", "2026-11-06T10:00:03Z", true),
            call("c3", "TaskCreate", r#"{"subject":"Read the third note"}"#, "2026-11-06T10:00:04Z"),
            created("c3", "3", "Read the third note", "2026-11-06T10:00:05Z", true),
            call("u1", "TaskUpdate", r#"{"taskId":"1","status":"completed"}"#, "2026-11-06T10:01:00Z"),
            call("u2", "TaskUpdate", r#"{"taskId":"2","status":"completed"}"#, "2026-11-06T10:02:00Z"),
            call("u3", "TaskUpdate", r#"{"taskId":"3","status":"completed"}"#, "2026-11-06T10:03:00Z"),
        ];
        std::fs::write(&t, lines.join("
") + "
").unwrap();
        let mut w = Work { offers: Some(true), ..Default::default() };
        let src = Source { row: "r".into(), account: "main".into(), session: "s".into(), cwd: None, transcript: Some(t) };
        let got = folder_tasks(&tasks).or_else(|| Steps::fallback(&mut w, &src)).unwrap();
        assert_eq!(summarise(&cut(got, t_at("2026-11-06T09:00:00Z"))).unwrap().line, "All 3 steps done");
    }

    fn t_at(s: &str) -> Option<DateTime<Utc>> {
        Some(DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc))
    }

    #[test]
    fn a_task_folder_is_only_a_plain_session_id() {
        let c = Path::new(r"C:\Users\x\.claude-max2");
        assert_eq!(task_dir(c, "95e231f4-14fe-4f79-870a-d974bd66abc5").unwrap(), c.join("tasks").join("95e231f4-14fe-4f79-870a-d974bd66abc5"));
        for bad in ["", "..", r"..\x", "a/b", "a b"] {
            assert!(task_dir(c, bad).is_none(), "{bad:?}");
        }
        // Account 2's tasks live in its own config folder.
        assert!(transcript::config_dir("account2").ends_with(".claude-max2"));
    }

    fn call(id: &str, name: &str, input: &str, at: &str) -> String {
        format!(r#"{{"type":"assistant","timestamp":"{at}","message":{{"content":[{{"type":"tool_use","id":"{id}","name":"{name}","input":{input}}}]}}}}"#)
    }
    fn created(call_id: &str, n: &str, subject: &str, at: &str, tur: bool) -> String {
        let r = if tur { format!(r#","toolUseResult":{{"task":{{"id":"{n}","subject":"{subject}"}}}}"#) } else { String::new() };
        format!(r#"{{"type":"user","timestamp":"{at}","message":{{"content":[{{"type":"tool_result","tool_use_id":"{call_id}","content":"Task #{n} created successfully: {subject}"}}]}}{r}}}"#)
    }

    #[test]
    fn the_replay_follows_creates_and_updates_and_ignores_repeats() {
        let mut r = Replay::default();
        let lines = [
            call("c1", "TaskCreate", r#"{"subject":"Read the file","description":"x","activeForm":"Reading the file"}"#, "2026-11-06T10:00:00Z"),
            created("c1", "1", "Read the file", "2026-11-06T10:00:01Z", true),
            call("c2", "TaskCreate", r#"{"subject":"Write the tests","description":"x"}"#, "2026-11-06T10:00:02Z"),
            created("c2", "2", "Write the tests", "2026-11-06T10:00:03Z", true),
            call("c3", "TaskCreate", r#"{"subject":"Ship it","description":"x"}"#, "2026-11-06T10:00:04Z"),
            created("c3", "3", "Ship it", "2026-11-06T10:00:05Z", false),
            // Claude Code writes the same blocks again, with no toolUseResult.
            call("c1", "TaskCreate", r#"{"subject":"Read the file","description":"x"}"#, "2026-11-06T10:00:00Z"),
            created("c1", "1", "Read the file", "2026-11-06T10:00:01Z", false),
            call("u1", "TaskUpdate", r#"{"taskId":"1","status":"in_progress"}"#, "2026-11-06T10:01:00Z"),
            call("u2", "TaskUpdate", r#"{"taskId":"1","status":"completed"}"#, "2026-11-06T10:02:00Z"),
            call("u3", "TaskUpdate", r#"{"taskId":"2","status":"in_progress","activeForm":"Writing the tests"}"#, "2026-11-06T10:02:01Z"),
            call("u4", "TaskUpdate", r#"{"taskId":"1","status":"completed"}"#, "2026-11-06T10:02:00Z"),
        ];
        for l in &lines {
            r.feed(l);
        }
        let s: Vec<(&str, &str)> = r.tasks.iter().map(|t| (t.id.as_str(), t.status.as_str())).collect();
        assert_eq!(s, [("1", "completed"), ("2", "in_progress"), ("3", "pending")]);
        assert_eq!(r.tasks[0].active_form.as_deref(), Some("Reading the file"));
        assert_eq!(summarise(&r.tasks).unwrap().line, "Step 2 of 3: Writing the tests");

        r.feed(&call("u5", "TaskUpdate", r#"{"taskId":"3","status":"deleted"}"#, "2026-11-06T10:03:00Z"));
        r.feed(&call("u6", "TaskUpdate", r#"{"taskId":"9","status":"completed"}"#, "2026-11-06T10:03:00Z"));
        assert_eq!(r.tasks.len(), 2, "deleted goes; an unknown id is ignored");
    }

    #[test]
    fn the_replay_skips_subagents_and_failed_creates() {
        let mut r = Replay::default();
        let side = call("s1", "TaskCreate", r#"{"subject":"Sub"}"#, "2026-11-06T10:00:00Z").replacen(r#"{"type":"assistant""#, r#"{"isSidechain":true,"type":"assistant""#, 1);
        r.feed(&side);
        r.feed(&created("s1", "1", "Sub", "2026-11-06T10:00:01Z", true));
        assert!(r.tasks.is_empty(), "a subagent's create isn't the session's");
        r.feed(&call("c1", "TaskCreate", r#"{"subject":"A"}"#, "2026-11-06T10:00:00Z"));
        r.feed(&r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"c1","is_error":true,"content":"Task #1 created successfully: A"}]}}"#.to_string());
        assert!(r.tasks.is_empty(), "an error result creates nothing");
    }

    #[test]
    fn todo_write_replaces_the_whole_list() {
        let mut r = Replay::default();
        r.feed(&call("t1", "TodoWrite", r#"{"todos":[{"content":"A","status":"completed","activeForm":"Doing A"},{"content":"B","status":"in_progress","activeForm":"Doing B"}]}"#, "2026-11-06T10:00:00Z"));
        assert_eq!(summarise(&r.tasks).unwrap().line, "Step 2 of 2: Doing B");
        r.feed(&call("t2", "TodoWrite", r#"{"todos":[{"content":"A","status":"completed"},{"content":"B","status":"completed"},{"content":"C","status":"pending"}]}"#, "2026-11-06T10:05:00Z"));
        assert_eq!(r.tasks.len(), 3);
        assert_eq!(r.tasks[0].changed, t("2026-11-06T10:00:00Z"), "an unchanged item keeps its time");
        assert_eq!(r.tasks[1].changed, t("2026-11-06T10:05:00Z"));
        assert_eq!(summarise(&r.tasks).unwrap().line, "2 of 3 done · next: C");
    }

    #[test]
    fn reading_in_two_goes_matches_one_and_a_half_line_waits() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.jsonl");
        let a = call("c1", "TaskCreate", r#"{"subject":"A"}"#, "2026-11-06T10:00:00Z");
        let b = created("c1", "1", "A", "2026-11-06T10:00:01Z", true);
        let c = call("u1", "TaskUpdate", r#"{"taskId":"1","status":"in_progress"}"#, "2026-11-06T10:00:02Z");
        let noise = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"}]}}"#;
        std::fs::write(&p, format!("{a}\n{noise}\n{b}\n{}", &c[..20])).unwrap();
        let mut r = Replay::default();
        r.read_new(&p).unwrap();
        assert_eq!(r.tasks.len(), 1);
        assert_eq!(r.tasks[0].status, "pending", "the cut line waits");
        std::fs::write(&p, format!("{a}\n{noise}\n{b}\n{c}\n")).unwrap();
        r.read_new(&p).unwrap();
        let mut once = Replay::default();
        once.read_new(&p).unwrap();
        assert_eq!(r.tasks, once.tasks);
        assert_eq!(r.offset, once.offset);
        assert_eq!(r.tasks[0].status, "in_progress");
        // A shorter file at the same path starts over.
        std::fs::write(&p, format!("{a}\n")).unwrap();
        r.read_new(&p).unwrap();
        assert!(r.tasks.is_empty(), "the create's result isn't in the new file");
    }

    #[test]
    fn only_this_visits_steps_count() {
        let tasks = vec![
            task("1", "Plan A", "completed", "2026-11-06T09:00:00Z"),
            task("2", "Plan B", "pending", "2026-11-06T09:00:00Z"),
            task("3", "Build", "completed", "2026-11-06T10:30:00Z"),
            task("4", "Test", "in_progress", "2026-11-06T10:40:00Z"),
        ];
        let kept = cut(tasks.clone(), t("2026-11-06T10:00:00Z"));
        assert_eq!(kept.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["2", "3", "4"], "a finished planning step drops, an open one stays");
        assert_eq!(cut(tasks.clone(), None).len(), 4);
        let mut unknown = tasks[0].clone();
        unknown.changed = None;
        assert_eq!(cut(vec![unknown], t("2026-11-06T10:00:00Z")).len(), 1, "no time: kept");
        assert_eq!(cut_column("Backlog"), None);
        assert_eq!(cut_column("Planning"), Some("Planning"));
        for c in ["In progress", "Blocked", "Human test", "Ship it", "Deploy complete"] {
            assert_eq!(cut_column(c), Some("In progress"), "{c}");
        }
    }

    #[test]
    fn the_cards_line_reads_well() {
        assert_eq!(summarise(&[]), None);
        let mut v = vec![task("1", "Read the code", "completed", "2026-11-06T10:00:00Z"), task("2", "Write the tests", "in_progress", "2026-11-06T10:00:00Z"), task("3", "Ship", "pending", "2026-11-06T10:00:00Z")];
        let s = summarise(&v).unwrap();
        assert_eq!((s.done, s.total, s.current, s.line.as_str()), (1, 3, Some(2), "Step 2 of 3: Write the tests"));
        v[1].active_form = Some("Writing the tests".into());
        assert_eq!(summarise(&v).unwrap().line, "Step 2 of 3: Writing the tests");
        v[1].status = "completed".into();
        assert_eq!(summarise(&v).unwrap().line, "2 of 3 done · next: Ship");
        v[2].status = "completed".into();
        assert_eq!(summarise(&v).unwrap().line, "All 3 steps done");
        assert_eq!(summarise(&v[..1]).unwrap().line, "1 of 1 step done");
        let items = summarise(&v).unwrap().items;
        assert_eq!(items[0], Item { text: "Read the code".into(), status: "completed".into() });
    }

    #[test]
    fn secret_looking_steps_are_hidden() {
        assert_eq!(mask("Open http://127.0.0.1:3006/dev-sign-in.html?as=admin"), "(hidden)");
        assert_eq!(mask("GET http://127.0.0.1:3006/api/dev/sign-in-token?as=admin"), "(hidden)");
        assert_eq!(mask("curl -H 'Authorization: Bearer abcdefghijklmnop'"), "curl -H 'Authorization: Bearer [hidden]'");
        assert_eq!(mask("use sk_live_123 here"), "use [hidden] here");
        assert_eq!(mask("token eyJhbGciOiJIUzI1NiJ9.x"), "token [hidden]");
        // Ordinary steps keep their words.
        for s in ["Write the tests", "Add the password reset form", "Run pytest --select=fast", "gcloud --project=acme-prod", "Write dev-sign-in tests", "Explain bearer tokens"] {
            assert_eq!(mask(s), s);
        }
        let v = vec![Task { active_form: Some("Signing in at http://x/dev-sign-in.html?as=user".into()), ..task("1", "Sign in", "in_progress", "2026-11-06T10:00:00Z") }];
        assert_eq!(summarise(&v).unwrap().line, "Step 1 of 1: (hidden)");
    }

    #[test]
    fn a_transcript_says_whether_it_offers_the_tools() {
        assert_eq!(offers_task_tools(r#"{"type":"attachment","attachment":{"type":"deferred_tools_delta","addedNames":["SendMessage","TaskCreate","TaskGet"]}}"#), Some(true));
        assert_eq!(offers_task_tools(r#"{"type":"attachment","attachment":{"type":"deferred_tools_delta","addedNames":["SendMessage","TaskStop"]}}"#), Some(false));
        assert_eq!(offers_task_tools(r#"{"type":"user","message":{"content":"hi"}}"#), None, "not listed yet");
    }

    #[test]
    fn project_folders_are_named_as_claude_code_names_them() {
        assert_eq!(project_folder(r"C:\Users\someone\kanban-app"), "C--Users-someone-kanban-app");
        assert_eq!(project_folder(r"C:\Users\someone\kanban-app-106"), "C--Users-someone-kanban-app-106");
    }

    #[test]
    fn reports_of_one_session_coalesce_and_a_sweep_runs_last() {
        let r = |row: &str, s: &str| Job::Report { row: row.into(), session: s.into(), transcript: None };
        let out = coalesce(vec![r("a", "1"), Job::Sweep, r("b", "2"), r("a", "3")]);
        let names: Vec<String> = out
            .iter()
            .map(|j| match j {
                Job::Report { row, session, .. } => format!("{row}{session}"),
                Job::Sweep => "sweep".into(),
            })
            .collect();
        assert_eq!(names, ["b2", "a3", "sweep"]);
    }

    #[test]
    fn created_ids_are_read_from_the_result_text() {
        assert_eq!(created_id("Task #12 created successfully: Write it").as_deref(), Some("12"));
        assert_eq!(created_id("Task #12 updated"), None);
        assert_eq!(created_id("nothing"), None);
    }
}
