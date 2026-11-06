//! Session search (card #110): the indexer thread that keeps `search.db` current from the
//! card sessions' transcripts, and the searches the board, `kb search` and the kanban_search
//! control tool run. The index itself is `kanban_store::search`.
//!
//! The board's lock is held only to read the session list and the cards, never while a
//! transcript is read or the index is written.

use crate::{transcript, Board};
use kanban_store::search::{self, Found, IndexedFile, InterruptHandle, NewEntry, SearchIndex};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Manager};

/// The first backfill starts this long after launch, so the app's own start isn't slowed.
const START_DELAY: Duration = Duration::from_secs(20);
/// A full sweep this often catches anything that grew with no hook (desktop conversations,
/// new subagents), however many hooks arrive meanwhile.
const SWEEP_EVERY: Duration = Duration::from_secs(5 * 60);
/// A conversation whose transcript wasn't found is looked for again after this long.
const MISS_RETRY: Duration = Duration::from_secs(60 * 60);
/// A pause between chunks, so a backfill never hogs the disk or a core.
const PAUSE: Duration = Duration::from_millis(20);

/// The backfill's progress, for the dialog's "Indexing: 120 of 182 transcripts".
#[derive(Debug, Clone, Default, Serialize)]
pub struct Progress {
    pub running: bool,
    pub done: usize,
    pub total: usize,
}

/// A reason to index now rather than at the next sweep.
#[derive(Debug, Clone, PartialEq)]
pub enum Nudge {
    /// A hook from session row `row` named this transcript (a new file after a /clear).
    Hook { row: String, path: String },
}

/// Who a search is for. Each has its own read connection, so a new search from the board
/// stops only the board's last one, never a `kb search` or kanban_search, and the other way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Who {
    Board,
    Tool,
}

/// A read-only connection and what stops the search running on it.
#[derive(Default)]
struct Slot {
    read: Mutex<Option<SearchIndex>>,
    stop: Mutex<Option<InterruptHandle>>,
}

pub struct Search {
    dir: PathBuf,
    board: Slot,
    tools: Slot,
    nudges: Mutex<Vec<Nudge>>,
    wake: Condvar,
    progress: Mutex<Progress>,
}

impl Search {
    pub fn new(dir: PathBuf) -> Search {
        Search { dir, board: Slot::default(), tools: Slot::default(), nudges: Mutex::new(vec![]), wake: Condvar::new(), progress: Mutex::new(Progress::default()) }
    }

    pub fn nudge(&self, n: Nudge) {
        if let Ok(mut q) = self.nudges.lock() {
            q.push(n);
            self.wake.notify_one();
        }
    }

    /// Waits up to `d` for nudges, and takes them.
    fn wait(&self, d: Duration) -> Vec<Nudge> {
        let Ok(q) = self.nudges.lock() else { return vec![] };
        let mut q = if q.is_empty() { self.wake.wait_timeout(q, d).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0) } else { q };
        std::mem::take(&mut *q)
    }

    pub fn progress(&self) -> Progress {
        self.progress.lock().map(|p| p.clone()).unwrap_or_default()
    }

    fn set_progress(&self, f: impl FnOnce(&mut Progress)) {
        if let Ok(mut p) = self.progress.lock() {
            f(&mut p);
        }
    }

    fn slot(&self, who: Who) -> &Slot {
        match who {
            Who::Board => &self.board,
            Who::Tool => &self.tools,
        }
    }

    /// The slot's connection, opened if need be. None before the index exists.
    fn opened<'a>(&self, slot: &'a Slot) -> Result<MutexGuard<'a, Option<SearchIndex>>, String> {
        let mut read = slot.read.lock().map_err(|e| e.to_string())?;
        if read.is_none() {
            let p = search::search_path(&self.dir);
            if p.exists() {
                let ix = SearchIndex::open_read_only(&p).map_err(|e| e.to_string())?;
                if let Ok(mut stop) = slot.stop.lock() {
                    *stop = Some(ix.interrupt_handle());
                }
                *read = Some(ix);
            }
        }
        Ok(read)
    }

    /// Runs a search, first stopping the one still running for the board (a tool's search
    /// runs to the end). `Ok(None)` means a newer search stopped this one.
    pub fn find(&self, who: Who, query: &str, card: Option<&str>, limit: usize) -> Result<Option<Found>, String> {
        let slot = self.slot(who);
        if who == Who::Board {
            if let Ok(stop) = slot.stop.lock() {
                if let Some(h) = stop.as_ref() {
                    h.interrupt();
                }
            }
        }
        let read = self.opened(slot)?;
        let Some(ix) = read.as_ref() else {
            return Ok(Some(Found { short: search::query_terms(query).is_empty(), ..Default::default() }));
        };
        match ix.search(query, card, &search::since(chrono::Utc::now()), limit) {
            Ok(f) => Ok(Some(f)),
            Err(e) if search::is_interrupted(&e) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    /// How many entries the index holds (0 before it exists), on the board's connection, so a
    /// swap (which holds it) never meets a connection of its own.
    pub fn entries(&self) -> usize {
        let Ok(read) = self.opened(&self.board) else { return 0 };
        read.as_ref().and_then(|ix| ix.stats().ok()).map(|s| s.entries).unwrap_or(0)
    }

    /// Closes both read connections and keeps them closed while the guards live.
    fn close_reads(&self) -> Result<Vec<MutexGuard<'_, Option<SearchIndex>>>, String> {
        let mut out = Vec::new();
        for slot in [&self.board, &self.tools] {
            let mut read = slot.read.lock().map_err(|e| e.to_string())?;
            *read = None;
            if let Ok(mut stop) = slot.stop.lock() {
                *stop = None;
            }
            out.push(read);
        }
        Ok(out)
    }
}

/// The transcript a hook body names: `transcript_path` (the main conversation; after a
/// /clear, the new file), else the main transcript a subagent's `agent_transcript_path` sits
/// under (`<main>\subagents\agent-<id>.jsonl`).
pub fn hook_path(body: &Value) -> Option<String> {
    if let Some(p) = body.get("transcript_path").and_then(Value::as_str).filter(|p| p.ends_with(".jsonl")) {
        return Some(p.to_string());
    }
    let a = Path::new(body.get("agent_transcript_path").and_then(Value::as_str)?);
    let folder = a.parent().filter(|d| d.file_name().is_some_and(|n| n == "subagents"))?.parent()?;
    Some(format!("{}.jsonl", folder.display()))
}

/// From `dispatch::on_hook`: a turn or session ended, so index its transcript now. The path
/// is taken from the hook body, so a file Claude Code started after a /clear is found too.
pub fn on_hook(app: &AppHandle, row_id: &str, event: &str, body: &Value) {
    if !matches!(event, "Stop" | "SubagentStop" | "SessionEnd") {
        return;
    }
    let Some(path) = hook_path(body) else { return };
    if let Some(s) = app.try_state::<Arc<Search>>() {
        s.nudge(Nudge::Hook { row: row_id.to_string(), path });
    }
}

/// The entries a transcript line gives the index: prompts, replies, tool calls (name, detail
/// and key input) and task notices. Tool output, thinking and Claude Code's bookkeeping are
/// left out, and so is anything from before `since`. A line with no timestamp takes the last
/// one seen in the file (`last_at`), else `fallback_at`.
pub fn line_entries(line: &str, file: i64, line_offset: u64, sub: bool, since: &str, fallback_at: &str, last_at: &mut Option<String>) -> Vec<NewEntry> {
    let parsed = transcript::parse_line_in(line, sub);
    let keep: Vec<_> = parsed.into_iter().filter(|e| matches!(e.kind, "user" | "assistant" | "tool" | "task")).collect();
    if keep.is_empty() {
        return vec![];
    }
    let v: Value = serde_json::from_str(line).unwrap_or(Value::Null);
    let uuid = v.get("uuid").and_then(Value::as_str);
    let mut out = Vec::new();
    for (place, e) in keep.into_iter().enumerate() {
        let at = match e.at.clone() {
            Some(a) => {
                *last_at = Some(a.clone());
                a
            }
            None => last_at.clone().unwrap_or_else(|| fallback_at.to_string()),
        };
        if at.as_str() < since {
            continue;
        }
        let (who, text) = if e.kind == "tool" {
            let name = e.text.split('\t').next().unwrap_or("").to_string();
            let mut t = e.text.replace('\t', " ");
            if let Some(input) = e.input.as_deref().filter(|i| !t.contains(*i)) {
                t.push('\n');
                t.push_str(input);
            }
            (name, t)
        } else {
            (String::new(), e.text.clone())
        };
        out.push(NewEntry { key: search::entry_key(uuid, file, line_offset, place), kind: e.kind.to_string(), who, at, text });
    }
    out
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn rfc3339(t: SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn cutoff() -> SystemTime {
    SystemTime::now() - Duration::from_secs(search::KEEP_DAYS as u64 * 86_400)
}

/// Reads file `f` on from where the index stopped, in bounded chunks, storing its entries.
/// Returns how many were new.
fn index_file(ix: &mut SearchIndex, f: &IndexedFile, from: u64, since: &str) -> Result<usize, String> {
    let path = Path::new(&f.path);
    let Some(m) = mtime(path) else { return Ok(0) };
    let fallback = rfc3339(m);
    let sub = f.agent.is_some();
    let mut off = from;
    let mut last_at = None;
    let mut added = 0;
    loop {
        let r = search::read_lines(path, off, search::MAX_CHUNK).map_err(|e| format!("{}: {e}", f.path))?;
        if r.offset == off && r.lines.is_empty() {
            break;
        }
        let batch: Vec<NewEntry> = r.lines.iter().flat_map(|(lo, line)| line_entries(line, f.id, *lo, sub, since, &fallback, &mut last_at)).collect();
        added += ix.add_chunk(f.id, &batch, r.offset).map_err(|e| e.to_string())?;
        off = r.offset;
        if !r.more {
            break;
        }
        std::thread::sleep(PAUSE);
    }
    Ok(added)
}

/// Records a main transcript's subagents for its card. Returns the ids of the subagent files.
fn add_subagents(ix: &mut SearchIndex, main: &IndexedFile, paths: &mut HashSet<String>, cut: SystemTime) -> Vec<i64> {
    let mut out = Vec::new();
    for (_, p, meta) in transcript::subagent_files(Path::new(&main.path)) {
        let p = p.display().to_string();
        if paths.insert(p.clone()) {
            if mtime(Path::new(&p)).is_some_and(|m| m >= cut) {
                if let Ok(id) = ix.add_file(&p, &main.card_id, &main.session, Some(&transcript::agent_type(&meta))) {
                    out.push(id);
                }
            }
        } else if let Ok(Some(id)) = ix.file_id(&p) {
            out.push(id);
        }
    }
    out
}

/// One conversation to index: a session row, or a desktop conversation a card names.
#[derive(Debug, Clone)]
struct Target {
    card_id: String,
    session: String,
    account: String,
}

/// The board's session rows (archived cards' included) and desktop conversations, read under
/// the board's lock, which is released before any file is touched.
fn targets(app: &AppHandle) -> Vec<Target> {
    let board = app.state::<Board>();
    let Ok(store) = board.0.lock() else { return vec![] };
    let rows = store.all_sessions().unwrap_or_default();
    let cards = store.list(true).unwrap_or_default();
    drop(store);
    let mut out: Vec<Target> = rows.into_iter().map(|r| Target { card_id: r.card_id, session: r.claude_session_id, account: r.account }).collect();
    for c in cards {
        if let Some(id) = c.fields.get("claudeSessionId").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            out.push(Target { card_id: c.id.clone(), session: id.to_string(), account: crate::sessions::card_account(&c) });
        }
    }
    out
}

/// Conversations whose transcript wasn't found (never written, or older than 90 days), so a
/// sweep doesn't list every projects folder for them each time. Forgotten every hour.
struct Misses {
    set: HashSet<String>,
    since: Instant,
}

impl Misses {
    fn new() -> Misses {
        Misses { set: HashSet::new(), since: Instant::now() }
    }

    fn fresh(&mut self) -> &mut HashSet<String> {
        if self.since.elapsed() >= MISS_RETRY {
            self.set.clear();
            self.since = Instant::now();
        }
        &mut self.set
    }
}

/// Records a file the hooks named for session row `row` (`card`, `session`). Returns its id.
pub fn record_hook(ix: &mut SearchIndex, card: &str, session: &str, path: &str) -> Option<i64> {
    if !Path::new(path).is_file() {
        return None;
    }
    ix.add_file(path, card, session, None).ok()
}

/// Brings `ix` up to date: finds every target's transcript and its subagents, then reads
/// whatever grew. `show` reports progress to the dialog (the first backfill and a rebuild).
fn pass(app: &AppHandle, s: &Search, ix: &mut SearchIndex, show: bool, misses: &mut Misses) -> Result<(), String> {
    let since = search::since(chrono::Utc::now());
    let cut = cutoff();
    let known: HashSet<String> = ix.files().map_err(|e| e.to_string())?.into_iter().filter(|f| f.agent.is_none()).map(|f| f.session).collect();
    let missed = misses.fresh();
    let missing: Vec<Target> = targets(app).into_iter().filter(|t| !known.contains(&t.session) && !missed.contains(&t.session)).collect();
    if !missing.is_empty() {
        // One listing of both accounts' transcripts, rather than a search of every project
        // folder for each conversation.
        let on_disk: HashMap<&str, HashMap<String, PathBuf>> = ["main", "account2"].into_iter().map(|a| (a, transcript::all_by_id(a))).collect();
        for t in missing {
            let other = if t.account == "account2" { "main" } else { "account2" };
            let p = [t.account.as_str(), other].into_iter().find_map(|a| on_disk.get(a).and_then(|m| m.get(&t.session)));
            match p.filter(|p| mtime(p).is_some_and(|m| m >= cut)) {
                Some(p) => {
                    let _ = ix.add_file(&p.display().to_string(), &t.card_id, &t.session, None);
                }
                None => {
                    missed.insert(t.session);
                }
            }
        }
    }
    let files = ix.files().map_err(|e| e.to_string())?;
    let mut paths: HashSet<String> = files.iter().map(|f| f.path.clone()).collect();
    for f in files.iter().filter(|f| f.agent.is_none()) {
        add_subagents(ix, f, &mut paths, cut);
    }
    index_grown(s, ix, ix.files().map_err(|e| e.to_string())?, &since, cut, show)
}

/// Reads the files in `files` that grew since the index last read them, oldest first: entries
/// are numbered as they go in, and a search takes the highest numbers as the newest.
fn index_grown(s: &Search, ix: &mut SearchIndex, files: Vec<IndexedFile>, since: &str, cut: SystemTime, show: bool) -> Result<(), String> {
    let offsets = ix.offsets().map_err(|e| e.to_string())?;
    let mut todo: Vec<(SystemTime, IndexedFile, u64)> = files
        .into_iter()
        .filter_map(|f| {
            let m = std::fs::metadata(&f.path).ok()?;
            let at = m.modified().ok()?;
            let from = offsets.get(&f.id).copied().unwrap_or(0);
            (m.len() != from && at >= cut).then_some((at, f, from))
        })
        .collect();
    todo.sort_by(|a, b| a.0.cmp(&b.0));
    if show {
        s.set_progress(|p| *p = Progress { running: true, done: 0, total: todo.len() });
    }
    for (_, f, from) in todo {
        if let Err(e) = index_file(ix, &f, from, since) {
            eprintln!("search index: {e}");
        }
        if show {
            s.set_progress(|p| p.done += 1);
        }
    }
    if show {
        s.set_progress(|p| p.running = false);
    }
    Ok(())
}

/// The writer for this month: `cur` kept open from the last loop, else opened. When the month
/// or the index version changed, a new file is built (the old one answers searches meanwhile)
/// and swapped in. None when the file is a newer build's, which this build leaves alone.
fn writer(app: &AppHandle, s: &Search, cur: Option<SearchIndex>, misses: &mut Misses) -> Result<Option<SearchIndex>, String> {
    let month = search::month_of(chrono::Utc::now());
    let open = || SearchIndex::open(search::search_path(&s.dir), &month).map_err(|e| e.to_string());
    let ix = match cur {
        Some(ix) => ix,
        None => {
            {
                let _closed = s.close_reads()?;
                search::recover_swap(&s.dir).map_err(|e| e.to_string())?;
            }
            open()?
        }
    };
    if !ix.writable() {
        return Ok(None);
    }
    if !ix.needs_new(&month).map_err(|e| e.to_string())? {
        return Ok(Some(ix));
    }
    let old_month = ix.month().map_err(|e| e.to_string())?.unwrap_or_default();
    let mut new = ix.start_new(&search::new_search_path(&s.dir), &month).map_err(|e| e.to_string())?;
    let built = pass(app, s, &mut new, true, misses);
    drop(new);
    drop(ix);
    built?;
    let swapped = {
        // No search connection is open on either file while they are renamed.
        let _closed = s.close_reads()?;
        search::swap_in(&s.dir, &old_month)
    };
    if let Err(e) = swapped {
        // Nothing was moved: the next loop carries the new file on and tries again.
        eprintln!("search index: swap: {e}");
    }
    Ok(Some(open()?))
}

/// Starts the indexer thread.
pub fn start(app: AppHandle) {
    let _ = std::thread::Builder::new().name("search-index".into()).spawn(move || {
        std::thread::sleep(START_DELAY);
        let s = app.state::<Arc<Search>>().inner().clone();
        let mut ix: Option<SearchIndex> = None;
        let mut last_pass: Option<Instant> = None;
        let mut misses = Misses::new();
        let mut nudges: Vec<Nudge> = vec![];
        loop {
            match writer(&app, &s, ix.take(), &mut misses) {
                Ok(w) => ix = w,
                Err(e) => eprintln!("search index: {e}"),
            }
            if let Some(w) = ix.as_mut() {
                if !nudges.is_empty() {
                    if let Err(e) = on_nudges(&app, &s, w, &nudges) {
                        eprintln!("search index: {e}");
                    }
                }
                // The sweep runs on time even while hooks keep waking the thread.
                if last_pass.map_or(true, |t| t.elapsed() >= SWEEP_EVERY) {
                    if let Err(e) = pass(&app, &s, w, last_pass.is_none(), &mut misses) {
                        eprintln!("search index: {e}");
                    }
                    last_pass = Some(Instant::now());
                }
            }
            let left = last_pass.map_or(SWEEP_EVERY, |t| SWEEP_EVERY.saturating_sub(t.elapsed()));
            nudges = s.wait(left.max(Duration::from_secs(1)));
        }
    });
}

/// Indexes what the hooks named: each file (recorded for its row's card) and its subagents.
fn on_nudges(app: &AppHandle, s: &Search, ix: &mut SearchIndex, nudges: &[Nudge]) -> Result<(), String> {
    let rows: HashSet<&str> = nudges.iter().map(|Nudge::Hook { row, .. }| row.as_str()).collect();
    let by_row: HashMap<String, (String, String)> = {
        let board = app.state::<Board>();
        let store = board.0.lock().map_err(|e| e.to_string())?;
        rows.into_iter().filter_map(|r| store.session(r).ok().flatten().map(|row| (row.id, (row.card_id, row.claude_session_id)))).collect()
    };
    let mut wanted: HashSet<i64> = HashSet::new();
    for Nudge::Hook { row, path } in nudges {
        if let Some((card, session)) = by_row.get(row) {
            if let Some(id) = record_hook(ix, card, session, path) {
                wanted.insert(id);
            }
        }
    }
    let cut = cutoff();
    let files = ix.files().map_err(|e| e.to_string())?;
    let mut paths: HashSet<String> = files.iter().map(|f| f.path.clone()).collect();
    for f in files.iter().filter(|f| wanted.contains(&f.id)).cloned().collect::<Vec<_>>() {
        wanted.extend(add_subagents(ix, &f, &mut paths, cut));
    }
    let files: Vec<IndexedFile> = ix.files().map_err(|e| e.to_string())?.into_iter().filter(|f| wanted.contains(&f.id)).collect();
    index_grown(s, ix, files, &search::since(chrono::Utc::now()), cut, false)
}

/// The board's view of a search: each card's number, title and column, and which hits are
/// from an earlier session than the card's latest.
pub fn run(app: &AppHandle, who: Who, query: &str, card: Option<&str>, limit: Option<usize>) -> Result<Value, String> {
    let s = app.state::<Arc<Search>>().inner().clone();
    let indexing = s.progress();
    let found = s.find(who, query, card, limit.unwrap_or(search::MAX_ROWS))?;
    let Some(found) = found else { return Ok(json!({ "status": "interrupted", "cards": [], "total": 0, "limited": false, "indexing": indexing })) };
    if found.short {
        return Ok(json!({ "status": "short", "cards": [], "total": 0, "limited": false, "indexing": indexing }));
    }
    let board = app.state::<Board>();
    let store = board.0.lock().map_err(|e| e.to_string())?;
    let mut cards = Vec::new();
    for g in found.cards {
        let Ok(c) = store.get(&g.card_id) else { continue };
        let latest = store.latest_session(&c.id).ok().flatten().map(|r| r.claude_session_id);
        let hits: Vec<Value> = g
            .hits
            .into_iter()
            .map(|h| {
                let earlier = latest.as_deref().is_some_and(|l| l != h.session);
                json!({ "kind": h.kind, "who": h.who, "agent": h.agent, "agentId": h.agent_id, "at": h.at, "earlier": earlier, "snippet": h.snippet })
            })
            .collect();
        cards.push(json!({
            "card": c.id, "number": c.number, "title": c.title(), "column": c.column, "archived": c.archived,
            "count": g.count, "countCapped": g.capped, "hits": hits,
        }));
    }
    Ok(json!({ "status": "ok", "cards": cards, "total": found.total, "limited": found.limited, "indexing": indexing }))
}

/// The search's state, for the dialog's indexing line.
pub fn status(app: &AppHandle) -> Value {
    let s = app.state::<Arc<Search>>().inner().clone();
    let p = s.progress();
    json!({ "running": p.running, "done": p.done, "total": p.total, "entries": s.entries() })
}

/// kanban_search and `kb search`: the same search as plain text, three hits a card.
pub fn tool(app: &AppHandle, args: &Value) -> Result<Value, String> {
    let query = args.get("query").and_then(Value::as_str).filter(|q| !q.trim().is_empty()).ok_or("pass the words to find as `query`")?;
    let card = match args.get("card").and_then(Value::as_i64) {
        Some(n) => {
            let board = app.state::<Board>();
            let store = board.0.lock().map_err(|e| e.to_string())?;
            Some(store.get_by_number(n).map_err(|e| e.to_string())?.id)
        }
        None => None,
    };
    let limit = args.get("limit").and_then(Value::as_u64).map(|l| l as usize);
    let v = run(app, Who::Tool, query, card.as_deref(), limit)?;
    Ok(json!(text_result(&v)))
}

/// A search result as text.
pub fn text_result(v: &Value) -> String {
    match v["status"].as_str() {
        Some("short") => return "Type at least 3 letters: shorter words are left out of a search.".into(),
        Some("interrupted") => return "A newer search stopped this one.".into(),
        _ => {}
    }
    let cards = v["cards"].as_array().cloned().unwrap_or_default();
    if cards.is_empty() {
        return "No matches in the last 90 days.".into();
    }
    let mut out = String::new();
    for c in &cards {
        let col = if c["archived"] == true { "Archived".to_string() } else { c["column"].as_str().unwrap_or("").to_string() };
        let plus = if c["countCapped"] == true { "+" } else { "" };
        out.push_str(&format!("#{} {} ({col}): {}{plus} match(es)\n", c["number"], c["title"].as_str().unwrap_or(""), c["count"]));
        for h in c["hits"].as_array().into_iter().flatten().take(3) {
            let who = match h["kind"].as_str() {
                Some("user") => "Prompt".to_string(),
                Some("assistant") => "Claude".to_string(),
                Some("task") => "Task".to_string(),
                _ => h["who"].as_str().unwrap_or("Tool").to_string(),
            };
            let who = match h["agent"].as_str() {
                Some(a) => format!("{who} · {a}"),
                None => who,
            };
            let at = h["at"].as_str().unwrap_or("").get(..16).unwrap_or("").replace('T', " ");
            let text: String = h["snippet"].as_array().into_iter().flatten().map(|p| p["t"].as_str().unwrap_or("")).collect();
            let earlier = if h["earlier"] == true { " (earlier session)" } else { "" };
            out.push_str(&format!("  {at}  {who}{earlier}: {text}\n"));
        }
    }
    if v["limited"] == true {
        out.push_str("(the newest 2,000 matches only)\n");
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SINCE: &str = "2026-08-08T00:00:00.000Z";

    #[test]
    fn a_hook_body_names_the_transcript_or_the_subagents_parent() {
        assert_eq!(hook_path(&json!({ "transcript_path": "C:\\p\\new-after-clear.jsonl" })).as_deref(), Some("C:\\p\\new-after-clear.jsonl"));
        let sub = json!({ "agent_transcript_path": "C:\\p\\abc\\subagents\\agent-a1.jsonl" });
        assert_eq!(hook_path(&sub).as_deref(), Some("C:\\p\\abc.jsonl"));
        assert_eq!(hook_path(&json!({ "agent_transcript_path": "C:\\p\\x.jsonl" })), None);
        assert_eq!(hook_path(&json!({})), None);
    }

    /// A /clear starts a new transcript under a new id; the hook's path is recorded for the
    /// hook's card, under the session row's id.
    #[test]
    fn a_clear_path_from_a_hook_is_recorded_for_its_card() {
        let d = tempfile::tempdir().unwrap();
        let mut ix = SearchIndex::open(kanban_store::search::search_path(d.path()), "2026-11").unwrap();
        let file = d.path().join("new-id.jsonl");
        std::fs::write(&file, "{\"type\":\"user\",\"uuid\":\"u1\",\"timestamp\":\"2026-11-06T01:00:00.000Z\",\"message\":{\"content\":\"after the clear: invoice\"}}\n").unwrap();
        let body = json!({ "session_id": "new-id", "transcript_path": file.display().to_string() });
        let path = hook_path(&body).unwrap();
        let id = record_hook(&mut ix, "card-42", "old-id", &path).unwrap();
        assert_eq!(record_hook(&mut ix, "card-42", "old-id", "C:\\nowhere\\gone.jsonl"), None);
        let f = ix.files().unwrap().into_iter().find(|f| f.id == id).unwrap();
        assert_eq!((f.card_id.as_str(), f.session.as_str()), ("card-42", "old-id"));
        let s = Search::new(d.path().to_path_buf());
        index_grown(&s, &mut ix, vec![f], SINCE, SystemTime::UNIX_EPOCH, false).unwrap();
        let found = ix.search("invoice", None, SINCE, 100).unwrap();
        assert_eq!(found.cards[0].card_id, "card-42");
    }

    /// Files go in oldest first, so the entries' numbers follow time and a search's newest
    /// matches are the newest transcripts'.
    #[test]
    fn files_are_read_oldest_first() {
        let d = tempfile::tempdir().unwrap();
        let mut ix = SearchIndex::open(kanban_store::search::search_path(d.path()), "2026-11").unwrap();
        let line = |u: &str, t: &str| format!("{{\"type\":\"user\",\"uuid\":\"{u}\",\"timestamp\":\"2026-11-0{t}T01:00:00.000Z\",\"message\":{{\"content\":\"invoice {u}\"}}}}\n");
        let (new, old) = (d.path().join("new.jsonl"), d.path().join("old.jsonl"));
        std::fs::write(&new, line("newer", "6")).unwrap();
        std::fs::write(&old, line("older", "1")).unwrap();
        let f = std::fs::File::options().write(true).open(&old).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(3 * 86_400)).unwrap();
        drop(f);
        let a = ix.add_file(&new.display().to_string(), "card-1", "s", None).unwrap();
        let b = ix.add_file(&old.display().to_string(), "card-2", "s", None).unwrap();
        let files = ix.files().unwrap().into_iter().filter(|f| f.id == a || f.id == b).collect();
        let s = Search::new(d.path().to_path_buf());
        index_grown(&s, &mut ix, files, SINCE, SystemTime::UNIX_EPOCH, false).unwrap();
        let first = ix.search("invoice", None, SINCE, 1).unwrap();
        assert_eq!(first.cards[0].card_id, "card-1", "the newest match comes first");
    }

    #[test]
    fn lines_give_prompts_replies_and_tools_but_not_output_or_old_lines() {
        let mut last = None;
        let user = r#"{"type":"user","uuid":"u1","timestamp":"2026-11-06T01:00:00.000Z","message":{"content":"Fix the invoice"}}"#;
        let e = line_entries(user, 1, 0, false, SINCE, "x", &mut last);
        assert_eq!(e.len(), 1);
        assert_eq!((e[0].key.as_str(), e[0].kind.as_str(), e[0].text.as_str()), ("u1#0", "user", "Fix the invoice"));
        let tool = r#"{"type":"assistant","uuid":"a1","timestamp":"2026-11-06T01:00:01.000Z","message":{"content":[{"type":"text","text":"Looking."},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"grep -rn InvoiceRequest server","description":"Find it"}}]}}"#;
        let e = line_entries(tool, 1, 100, false, SINCE, "x", &mut last);
        assert_eq!(e.len(), 2);
        assert_eq!(e[1].key, "a1#1");
        assert_eq!(e[1].who, "Bash");
        assert!(e[1].text.contains("Find it") && e[1].text.contains("grep -rn InvoiceRequest"), "{}", e[1].text);
        // A tool's output isn't indexed.
        let result = r#"{"type":"user","uuid":"r1","timestamp":"2026-11-06T01:00:02.000Z","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"server/invoice.ts:1"}]}}"#;
        assert!(line_entries(result, 1, 200, false, SINCE, "x", &mut last).is_empty());
        // Older than 90 days: left out.
        let old = r#"{"type":"user","uuid":"o1","timestamp":"2026-07-01T00:00:00.000Z","message":{"content":"old words"}}"#;
        assert!(line_entries(old, 1, 300, false, SINCE, "x", &mut last).is_empty());
        // No uuid: the file and offset key; no timestamp: the last one seen.
        let mut last = Some("2026-11-06T02:00:00.000Z".to_string());
        let queued = r#"{"type":"attachment","attachment":{"type":"queued_command","prompt":"also the totals","origin":{"kind":"human"}}}"#;
        let e = line_entries(queued, 7, 4096, false, SINCE, "x", &mut last);
        assert_eq!(e[0].key, "f7@4096#0");
        assert_eq!(e[0].at, "2026-11-06T02:00:00.000Z");
    }

    /// The board and the tools search on their own connections, so a board search's interrupt
    /// never reaches a tool's search; a swap closes both.
    #[test]
    fn the_board_and_the_tools_search_on_their_own_connections() {
        let d = tempfile::tempdir().unwrap();
        drop(SearchIndex::open(kanban_store::search::search_path(d.path()), "2026-11").unwrap());
        let s = Search::new(d.path().to_path_buf());
        assert!(s.find(Who::Board, "invoice", None, 10).unwrap().is_some());
        assert!(s.find(Who::Tool, "invoice", None, 10).unwrap().is_some());
        assert!(s.board.stop.lock().unwrap().is_some() && s.tools.stop.lock().unwrap().is_some(), "each opened its own connection");
        drop(s.close_reads().unwrap());
        assert!(s.board.read.lock().unwrap().is_none() && s.tools.read.lock().unwrap().is_none());
    }

    #[test]
    fn text_results_name_cards_and_hits() {
        let v = json!({ "status": "ok", "limited": false, "cards": [{ "number": 110, "title": "Search", "column": "Done", "archived": true, "count": 1000, "countCapped": true,
            "hits": [{ "kind": "tool", "who": "Bash", "agent": "card-tester", "at": "2026-11-06T03:39:01.001Z", "earlier": true, "snippet": [{ "t": "grep ", "m": false }, { "t": "invoice", "m": true }] }] }] });
        let t = text_result(&v);
        assert!(t.starts_with("#110 Search (Archived): 1000+ match(es)"), "{t}");
        assert!(t.contains("2026-11-06 03:39  Bash · card-tester (earlier session): grep invoice"), "{t}");
        assert!(text_result(&json!({ "status": "short" })).contains("3 letters"));
        assert!(text_result(&json!({ "status": "ok", "cards": [] })).starts_with("No matches"));
    }
}
