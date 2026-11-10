//! What is running now (card #129), for the Agents drawer's "Running now" list and the session
//! pane that opens any of them the way the Router's opens:
//!
//! - the Router, when it runs;
//! - each merge lane that runs (cards #160, #162: the merger, one per repo);
//! - each card session the app runs, with its subagents that are running now;
//! - sessions outside the app (Claude Code in a terminal or the desktop app) that use this board.
//!
//! How an outside session is known: Claude Code puts `CLAUDE_CODE_SESSION_ID` in each session's
//! environment, and `kb` (and the `kb mcp` it starts) sends it as `X-Claude-Session` when it isn't
//! a card session's (`crates/kb/src/api.rs`). The API notes each such call here (`note_call`), in
//! memory only: the id, the cards the call named and when. So a session is listed only by the app
//! it called (a test app's sessions never show on live) and its cards are this board's. Ids the
//! app runs itself (its `sessions` rows, the Router) are never outside.
//!
//! Read only: nothing here writes the board. Outside sessions are read-only in the pane: Claude
//! Code offers no way for another program to type into them.

use crate::{monitor, transcript, Board, CmdError, CmdResult};
use chrono::{DateTime, Duration as CDuration, Utc};
use kanban_store::{Card, Store};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Manager};

/// An outside session drops off the list this long after its last board call and its last
/// transcript write.
pub const OUTSIDE_MINUTES: i64 = 15;
/// The most outside sessions kept in memory; the oldest goes first.
pub const OUTSIDE_CAP: usize = 50;
/// How long a call is kept at all (a session writing its transcript stays listed while it does).
const KEEP_HOURS: i64 = 24;
/// The cards kept per outside session, newest last.
const CARDS_KEPT: usize = 20;
/// How much of a transcript's head is read for its folder and title.
const HEAD_BYTES: usize = 64 * 1024;

// ---------- the outside callers ----------

#[derive(Debug, Clone, PartialEq)]
pub struct Caller {
    pub id: String,
    pub cards: Vec<i64>,
    pub last: DateTime<Utc>,
}

/// The outside sessions that called the app, by Claude session id.
#[derive(Debug, Default)]
pub struct OutsideBook {
    calls: HashMap<String, Caller>,
}

impl OutsideBook {
    /// A call from session `id` naming `cards`, at `now`.
    pub fn note(&mut self, id: &str, cards: &[i64], now: DateTime<Utc>) {
        let c = self.calls.entry(id.to_string()).or_insert_with(|| Caller { id: id.to_string(), cards: vec![], last: now });
        for n in cards {
            c.cards.retain(|x| x != n);
            c.cards.push(*n);
        }
        if c.cards.len() > CARDS_KEPT {
            let extra = c.cards.len() - CARDS_KEPT;
            c.cards.drain(..extra);
        }
        c.last = c.last.max(now);
        self.prune(now);
    }
    /// Drops calls older than a day, then the oldest beyond the cap.
    pub fn prune(&mut self, now: DateTime<Utc>) {
        self.calls.retain(|_, c| now - c.last <= CDuration::hours(KEEP_HOURS));
        while self.calls.len() > OUTSIDE_CAP {
            let Some(oldest) = self.calls.values().min_by_key(|c| c.last).map(|c| c.id.clone()) else { break };
            self.calls.remove(&oldest);
        }
    }
    /// Every caller, the latest call first.
    pub fn list(&self) -> Vec<Caller> {
        let mut v: Vec<Caller> = self.calls.values().cloned().collect();
        v.sort_by(|a, b| b.last.cmp(&a.last).then(a.id.cmp(&b.id)));
        v
    }
    pub fn has(&self, id: &str) -> bool {
        self.calls.contains_key(id)
    }
}

fn book() -> &'static Mutex<OutsideBook> {
    static BOOK: OnceLock<Mutex<OutsideBook>> = OnceLock::new();
    BOOK.get_or_init(Default::default)
}

/// A Claude session id as Claude Code makes them: letters, digits, `-` and `_`.
pub fn plain_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The card numbers a call names: a `kb`/tool call's `card` (or `number`), or an MCP call's
/// `params.arguments.card`. `12`, `"12"` and `"#12"` all count.
pub fn cards_in(body: &Value) -> Vec<i64> {
    let num = |v: Option<&Value>| -> Option<i64> {
        match v? {
            Value::Number(n) => n.as_i64(),
            Value::String(s) => s.trim().trim_start_matches('#').parse().ok(),
            _ => None,
        }
    };
    let mut out = vec![];
    for args in [Some(body), body.pointer("/params/arguments")].into_iter().flatten() {
        for k in ["card", "number"] {
            if let Some(n) = num(args.get(k)).filter(|n| *n > 0) {
                if !out.contains(&n) {
                    out.push(n);
                }
            }
        }
    }
    out
}

/// The API's note of a call from outside a card session (`X-Claude-Session`).
pub fn note_call(id: &str, body: &Value) {
    if !plain_id(id) {
        return;
    }
    if let Ok(mut b) = book().lock() {
        b.note(id, &cards_in(body), Utc::now());
    }
}

// ---------- running subagents ----------

/// The subagents running now: not finished, not stopped, and written within `interval` (the
/// monitor's, at least `STUCK_CHECK_DEFAULT` minutes, so a monitor turned off doesn't hide them
/// all). Claude Code often never marks a subagent finished, so the write time decides.
pub fn running_subagents(subs: &[transcript::Subagent], interval: std::time::Duration, now: DateTime<Utc>) -> Vec<transcript::Subagent> {
    let floor = std::time::Duration::from_secs(kanban_store::STUCK_CHECK_DEFAULT as u64 * 60);
    let interval = interval.max(floor);
    subs.iter()
        .filter(|s| !s.finished && !s.stopped)
        .filter(|s| s.updated.as_deref().and_then(monitor::parse_time).is_some_and(|u| (now - u).to_std().unwrap_or_default() < interval))
        .cloned()
        .collect()
}

fn interval(store: &Store) -> std::time::Duration {
    let m = store.stuck_check_minutes().unwrap_or(kanban_store::STUCK_CHECK_DEFAULT).max(0) as u64;
    std::time::Duration::from_secs(m * 60)
}

// ---------- a transcript's head ----------

/// The working folder and a title (its first line from Hugh, 80 characters) from a transcript's
/// first lines. A line the app or Claude Code wrote (a command, a reminder, a tool result) isn't
/// a title.
pub fn head_info(text: &str) -> (Option<String>, Option<String>) {
    let (mut cwd, mut title) = (None, None);
    for l in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
        if cwd.is_none() {
            cwd = v.get("cwd").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        }
        if title.is_none() && v.get("type").and_then(Value::as_str) == Some("user") && v.get("isMeta").and_then(Value::as_bool) != Some(true) {
            let c = v.pointer("/message/content");
            let text = match c {
                Some(Value::String(s)) => Some(s.clone()),
                Some(Value::Array(a)) => a.iter().find(|x| x.get("type").and_then(Value::as_str) == Some("text")).and_then(|x| x.get("text").and_then(Value::as_str)).map(str::to_string),
                _ => None,
            };
            if let Some(t) = text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty() && !t.starts_with('<')) {
                let line = t.lines().next().unwrap_or("").trim();
                let mut s: String = line.chars().take(80).collect();
                if line.chars().count() > 80 {
                    s.push('…');
                }
                title = Some(kanban_store::mask_secrets(&s));
            }
        }
        if cwd.is_some() && title.is_some() {
            break;
        }
    }
    (cwd, title)
}

fn read_head(path: &Path) -> String {
    let mut buf = vec![0u8; HEAD_BYTES];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut buf)).unwrap_or(0);
    // Only whole lines: a cut last line is dropped.
    let text = String::from_utf8_lossy(&buf[..n]).to_string();
    match (n == HEAD_BYTES, text.rfind('\n')) {
        (true, Some(i)) => text[..i].to_string(),
        _ => text,
    }
}

fn modified(path: &Path) -> Option<DateTime<Utc>> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok().map(DateTime::<Utc>::from)
}

/// The account whose projects folder holds `path`.
fn account_of(path: &Path) -> &'static str {
    if path.starts_with(transcript::config_dir("account2")) { "account2" } else { "main" }
}

// ---------- the list ----------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubRun {
    pub id: String,
    pub agent_type: String,
    pub description: String,
    pub started: Option<String>,
    pub updated: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CardRun {
    pub card_id: String,
    pub number: i64,
    pub title: String,
    pub column: String,
    pub state: String,
    /// The Claude session the subagents are in (for `subagent_read`).
    pub session: Option<String>,
    pub subagents: Vec<SubRun>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OutsideRun {
    pub id: String,
    pub account: Option<String>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    /// The later of its last board call and its last transcript write.
    pub last_at: String,
    pub cards: Vec<i64>,
    /// Its transcript is on this machine.
    pub readable: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RouterRun {
    pub state: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LaneRun {
    /// Its live-map key, `lane:<repo>`: the session pane reads it with `transcript_read`.
    pub key: String,
    pub repo: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Running {
    pub router: Option<RouterRun>,
    pub lanes: Vec<LaneRun>,
    /// Every repo's merge lane this channel has run, running or not (state "off" when it isn't),
    /// for the drawer's Merger tab (card #129: Hugh's agents along the top). Not counted.
    pub lanes_known: Vec<LaneRun>,
    pub cards: Vec<CardRun>,
    pub outside: Vec<OutsideRun>,
    /// Everything listed: the Router, each lane, each card session, each running subagent, each outside one.
    pub count: usize,
}

impl Running {
    pub fn counted(mut self) -> Self {
        self.count = self.router.iter().count() + self.lanes.len() + self.cards.iter().map(|c| 1 + c.subagents.len()).sum::<usize>() + self.outside.len();
        self
    }
}

/// Every Claude session id the app has run itself: card sessions, every Router and merge-lane
/// conversation (only the ids are read).
fn own_ids(store: &Store) -> HashSet<String> {
    store.own_claude_ids().unwrap_or_default()
}

/// Where each transcript is, by session id, from one listing of both accounts' projects folders
/// per poll (not a folder scan per session).
pub struct Index(HashMap<String, PathBuf>);
impl Index {
    pub fn read() -> Self {
        let mut m = transcript::all_by_id("main");
        for (id, p) in transcript::all_by_id("account2") {
            m.entry(id).or_insert(p);
        }
        Index(m)
    }
    pub fn find(&self, id: &str) -> Option<PathBuf> {
        self.0.get(id).cloned()
    }
}

/// A transcript's folder and title, kept per path once it has a title (they never change).
fn head_of(path: &Path) -> (Option<String>, Option<String>) {
    static KNOWN: OnceLock<Mutex<HashMap<PathBuf, (Option<String>, Option<String>)>>> = OnceLock::new();
    let known = KNOWN.get_or_init(Default::default);
    if let Some(h) = known.lock().ok().and_then(|k| k.get(path).cloned()) {
        return h;
    }
    let h = head_info(&read_head(path));
    if h.1.is_some() {
        if let Ok(mut k) = known.lock() {
            if k.len() > 500 {
                k.clear();
            }
            k.insert(path.to_path_buf(), h.clone());
        }
    }
    h
}

/// The outside sessions to list: callers whose id isn't the app's, with their transcript's head,
/// kept while their last call or last write is within `OUTSIDE_MINUTES`.
pub fn outside_runs(callers: &[Caller], own: &HashSet<String>, find: impl Fn(&str) -> Option<PathBuf>, now: DateTime<Utc>) -> Vec<OutsideRun> {
    let mut out = vec![];
    for c in callers {
        if own.contains(&c.id) {
            continue;
        }
        let path = find(&c.id);
        let wrote = path.as_deref().and_then(modified);
        let last = wrote.map_or(c.last, |w| w.max(c.last));
        if now - last > CDuration::minutes(OUTSIDE_MINUTES) {
            continue;
        }
        let (cwd, title) = path.as_deref().map(head_of).unwrap_or((None, None));
        out.push(OutsideRun {
            id: c.id.clone(),
            account: path.as_deref().map(|p| account_of(p).to_string()),
            cwd,
            title,
            last_at: last.to_rfc3339(),
            cards: c.cards.clone(),
            readable: path.is_some(),
        });
    }
    out.sort_by(|a, b| b.last_at.cmp(&a.last_at));
    out
}


/// `invoke("sessions_running")`: what runs now, for the Running now list and the Agents count.
#[tauri::command]
pub async fn sessions_running(app: AppHandle) -> CmdResult<Running> {
    tauri::async_runtime::spawn_blocking(move || running(&app)).await.map_err(CmdError::other)?
}

fn running(app: &AppHandle) -> CmdResult<Running> {
    let live = app.state::<Arc<crate::sessions::Sessions>>();
    let states = live.states();
    let now = Utc::now();
    // The board's lock only while the rows are read, never while a transcript is.
    let (cards, own, wait, seen) = {
        let board = app.state::<Board>();
        let store = board.0.lock().map_err(CmdError::other)?;
        let mut cards = vec![];
        for (key, state) in &states {
            let Ok(card) = store.get(key) else { continue }; // the Router, a lane: not cards
            let row = live.is_live(key).or_else(|| store.latest_session(key).ok().flatten());
            cards.push((card, state.clone(), row));
        }
        let seen = store.lanes_seen(crate::routersession::channel()).unwrap_or_default();
        (cards, own_ids(&store), interval(&store), seen)
    };
    let index = Index::read();
    let mut card_runs: Vec<CardRun> = cards
        .into_iter()
        .map(|(card, state, row)| {
            let session = row.as_ref().map(|r| r.claude_session_id.clone());
            let subs = row
                .as_ref()
                .and_then(|r| index.find(&r.claude_session_id))
                .map(|p| running_subagents(&transcript::subagents(&p), wait, now))
                .unwrap_or_default();
            CardRun {
                card_id: card.id.clone(),
                number: card.number,
                title: card.title().to_string(),
                column: card.column.clone(),
                state,
                session,
                subagents: subs.into_iter().map(|s| SubRun { id: s.id, agent_type: s.agent_type, description: s.description, started: s.started, updated: s.updated }).collect(),
            }
        })
        .collect();
    card_runs.sort_by_key(|c| c.number);
    let router = {
        let s = crate::routersession::status(app);
        s.live.then(|| RouterRun { state: s.state })
    };
    let mut lanes: Vec<LaneRun> = states
        .iter()
        .filter_map(|(k, st)| kanban_store::lane_repo(k).map(|r| LaneRun { key: k.clone(), repo: r.to_string(), state: st.clone() }))
        .collect();
    lanes.sort_by(|a, b| a.repo.cmp(&b.repo));
    let lanes_known = known_lanes(&lanes, seen.iter().map(|l| l.repo.as_str()));
    let callers = book().lock().map(|b| b.list()).unwrap_or_default();
    let outside = outside_runs(&callers, &own, |id| index.find(id), now);
    Ok(Running { router, lanes, lanes_known, cards: card_runs, outside, count: 0 }.counted())
}

/// Every lane: the running ones as they are, then each other repo seen, "off".
pub fn known_lanes<'a>(running: &[LaneRun], seen: impl Iterator<Item = &'a str>) -> Vec<LaneRun> {
    let mut out = running.to_vec();
    for repo in seen {
        let key = kanban_store::lane_key(repo);
        if !out.iter().any(|l| l.key == key) {
            out.push(LaneRun { key, repo: repo.to_string(), state: "off".into() });
        }
    }
    out.sort_by(|a, b| a.repo.cmp(&b.repo));
    out
}

/// `invoke("outside_transcript_read", {id, offset})`: an outside session's conversation, read
/// only. Refuses an id that never called this app, or one of the app's own.
#[tauri::command]
pub async fn outside_transcript_read(app: AppHandle, id: String, offset: u64) -> CmdResult<transcript::Chunk> {
    if !plain_id(&id) || !book().lock().map(|b| b.has(&id)).unwrap_or(false) {
        return Err(CmdError::other("that session hasn't used this board"));
    }
    {
        let board = app.state::<Board>();
        let store = board.0.lock().map_err(CmdError::other)?;
        if own_ids(&store).contains(&id) {
            return Err(CmdError::other("that is one of the app's own sessions"));
        }
    }
    tauri::async_runtime::spawn_blocking(move || {
        let path = transcript::find("main", &id).ok_or_else(|| CmdError::other("that session's conversation isn't on this machine"))?;
        transcript::read(&path, offset).map_err(CmdError::other)
    })
    .await
    .map_err(CmdError::other)?
}

// ---------- a subagent from a feed row ----------

/// The conversations a card's subagents may be in, newest first: every session the app ran for
/// the card, its desktop conversation, then the same for its stack's lead (the merge and a
/// stack's reproducer run in the lead's). Each is (card id, Claude session id, account).
fn conversations(store: &Store, card: &Card) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = vec![];
    let add = |c: &Card, out: &mut Vec<(String, String, String)>| {
        for r in store.card_sessions(&c.id).unwrap_or_default() {
            if !out.iter().any(|(_, s, _)| *s == r.claude_session_id) {
                out.push((c.id.clone(), r.claude_session_id.clone(), r.account.clone()));
            }
        }
        if let Some(d) = c.fields.get("claudeSessionId").and_then(Value::as_str).filter(|d| plain_id(d)) {
            if !out.iter().any(|(_, s, _)| s == d) {
                out.push((c.id.clone(), d.to_string(), crate::sessions::card_account(c)));
            }
        }
    };
    add(card, &mut out);
    if let Some(lead) = card.stack_of().and_then(|n| store.get_by_number(n).ok()) {
        add(&lead, &mut out);
    }
    out
}

/// Which subagent a feed row means. By its id when the row has one (a verdict); else the one of
/// `kind` (`card-critic`, ...) that started last at or before `before`, across every
/// conversation. `found` holds each conversation's subagents, in `conversations` order.
pub fn pick_subagent<'a>(found: &'a [(usize, Vec<transcript::Subagent>)], agent_id: Option<&str>, kind: Option<&str>, before: Option<&str>) -> Option<(usize, &'a transcript::Subagent)> {
    if let Some(id) = agent_id.filter(|i| !i.is_empty()) {
        if let Some(hit) = found.iter().find_map(|(i, subs)| subs.iter().find(|s| s.id == id).map(|s| (*i, s))) {
            return Some(hit);
        }
        // Not in any of the card's conversations on this machine: the kind and time may still find it.
    }
    let kind = kind?;
    let cut = before.and_then(monitor::parse_time);
    found
        .iter()
        .flat_map(|(i, subs)| subs.iter().map(move |s| (*i, s)))
        .filter(|(_, s)| s.agent_type == kind)
        .filter_map(|(i, s)| {
            let st = s.started.as_deref().and_then(monitor::parse_time)?;
            // A minute's grace: a log line can carry a time a moment before the subagent's first line.
            cut.map_or(true, |c| st <= c + CDuration::minutes(1)).then_some((st, i, s))
        })
        .max_by_key(|(st, _, _)| *st)
        .map(|(_, i, s)| (i, s))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FoundSub {
    /// The card whose session it ran in (the card itself, or its stack's lead).
    pub card_id: String,
    pub session: String,
    pub id: String,
    pub agent_type: String,
    pub description: String,
    pub running: bool,
}

/// `invoke("subagent_find", {card, agentId?, kind?, before?})`: the subagent a feed row on card
/// number `card` means (see `pick_subagent`), or null.
#[tauri::command]
pub async fn subagent_find(app: AppHandle, card: i64, agent_id: Option<String>, kind: Option<String>, before: Option<String>) -> CmdResult<Option<FoundSub>> {
    tauri::async_runtime::spawn_blocking(move || {
        let (convs, wait) = {
            let board = app.state::<Board>();
            let store = board.0.lock().map_err(CmdError::other)?;
            let c = store.get_by_number(card).map_err(CmdError::other)?;
            (conversations(&store, &c), interval(&store))
        };
        let found: Vec<(usize, Vec<transcript::Subagent>)> = convs
            .iter()
            .enumerate()
            .filter_map(|(i, (_, s, a))| transcript::find(a, s).map(|p| (i, transcript::subagents(&p))))
            .collect();
        let now = Utc::now();
        Ok(pick_subagent(&found, agent_id.as_deref(), kind.as_deref(), before.as_deref()).map(|(i, s)| {
            let running = !running_subagents(std::slice::from_ref(s), wait, now).is_empty();
            FoundSub { card_id: convs[i].0.clone(), session: convs[i].1.clone(), id: s.id.clone(), agent_type: s.agent_type.clone(), description: s.description.clone(), running }
        }))
    })
    .await
    .map_err(CmdError::other)?
}

/// `invoke("subagent_read", {cardId, session, agent, offset})`: a subagent's own conversation,
/// read only, from any of the card's conversations (or its lead's). Refuses a session that isn't
/// one of them.
#[tauri::command]
pub async fn subagent_read(app: AppHandle, card_id: String, session: String, agent: String, offset: u64) -> CmdResult<transcript::Chunk> {
    let account = {
        let board = app.state::<Board>();
        let store = board.0.lock().map_err(CmdError::other)?;
        let card = store.get(&card_id).map_err(CmdError::other)?;
        // The card here is the one the row is on, or the lead the subagent ran in: either way the
        // session must be one of that card's conversations or its lead's.
        conversations(&store, &card).into_iter().find(|(_, s, _)| *s == session).map(|(_, _, a)| a)
    };
    let account = account.ok_or_else(|| CmdError::other("that conversation isn't this card's"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let main = transcript::find(&account, &session).ok_or_else(|| CmdError::other("that conversation isn't on this machine"))?;
        let path = transcript::subagent_path(&main, &agent).ok_or_else(|| CmdError::other("that subagent's transcript isn't on this machine"))?;
        transcript::read_in(&path, offset, true).map_err(CmdError::other)
    })
    .await
    .map_err(CmdError::other)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn t(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }
    fn sub(id: &str, kind: &str, started: &str, updated: &str, finished: bool, stopped: bool) -> transcript::Subagent {
        transcript::Subagent {
            id: id.into(),
            agent_type: kind.into(),
            description: String::new(),
            tool_id: None,
            finished,
            stopped,
            started: Some(started.into()),
            updated: Some(updated.into()),
        }
    }

    #[test]
    fn every_lane_seen_is_known_and_a_running_one_keeps_its_state() {
        let running = vec![LaneRun { key: kanban_store::lane_key("owner/b"), repo: "owner/b".into(), state: "working".into() }];
        let all = known_lanes(&running, ["owner/b", "owner/a"].into_iter());
        assert_eq!(all.iter().map(|l| (l.repo.as_str(), l.state.as_str())).collect::<Vec<_>>(), vec![("owner/a", "off"), ("owner/b", "working")]);
        let r = Running { router: None, lanes: running.clone(), lanes_known: all, cards: vec![], outside: vec![], count: 0 }.counted();
        assert_eq!(r.count, 1, "only what runs is counted");
    }

    #[test]
    fn a_call_is_kept_with_its_cards_once_per_session() {
        let mut b = OutsideBook::default();
        let now = t("2026-11-09T10:00:00Z");
        b.note("s1", &[12], now);
        b.note("s1", &[5, 12], now + CDuration::minutes(1));
        b.note("s2", &[], now);
        let l = b.list();
        assert_eq!(l.len(), 2, "one entry per session id");
        assert_eq!(l[0].id, "s1", "the latest call first");
        assert_eq!(l[0].cards, vec![5, 12], "cards kept once, the latest last");
    }

    #[test]
    fn the_book_is_capped_and_forgets_after_a_day() {
        let mut b = OutsideBook::default();
        let now = t("2026-11-09T10:00:00Z");
        b.note("old", &[], now - CDuration::hours(25));
        for i in 0..(OUTSIDE_CAP + 5) {
            b.note(&format!("s{i}"), &[], now + CDuration::seconds(i as i64));
        }
        assert_eq!(b.list().len(), OUTSIDE_CAP);
        assert!(!b.has("old"), "a day-old call is gone");
        assert!(!b.has("s0"), "the oldest goes beyond the cap");
        assert!(b.has(&format!("s{}", OUTSIDE_CAP + 4)));
    }

    #[test]
    fn cards_come_from_a_tool_call_or_an_mcp_call() {
        assert_eq!(cards_in(&json!({ "card": 12, "text": "x" })), vec![12]);
        assert_eq!(cards_in(&json!({ "card": "#7" })), vec![7]);
        assert_eq!(cards_in(&json!({ "jsonrpc": "2.0", "method": "tools/call", "params": { "name": "kanban_get_card", "arguments": { "card": 129 } } })), vec![129]);
        assert_eq!(cards_in(&json!({ "number": 3, "card": 3 })), vec![3]);
        assert!(cards_in(&json!({ "title": "new" })).is_empty());
        assert!(cards_in(&json!({ "card": 0 })).is_empty());
    }

    #[test]
    fn only_a_plain_id_is_noted() {
        assert!(plain_id("344b450b-a120-445a-9002-8993d291430b"));
        assert!(!plain_id(""));
        assert!(!plain_id("../x"));
        assert!(!plain_id(&"a".repeat(65)));
    }

    #[test]
    fn the_apps_own_sessions_and_quiet_ones_are_never_listed() {
        let now = t("2026-11-09T10:00:00Z");
        let callers = vec![
            Caller { id: "mine".into(), cards: vec![1], last: now },
            Caller { id: "outside".into(), cards: vec![2], last: now - CDuration::minutes(3) },
            Caller { id: "quiet".into(), cards: vec![], last: now - CDuration::minutes(OUTSIDE_MINUTES + 1) },
        ];
        let own: HashSet<String> = ["mine".to_string()].into();
        let runs = outside_runs(&callers, &own, |_| None, now);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].id, "outside");
        assert_eq!(runs[0].cards, vec![2]);
        assert!(!runs[0].readable, "no transcript on this machine");
    }

    #[test]
    fn a_session_still_writing_stays_listed() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("s.jsonl");
        std::fs::write(&p, "{\"cwd\":\"C:/work\",\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"fix the board\"}}\n").unwrap();
        let now = Utc::now();
        let callers = vec![Caller { id: "s".into(), cards: vec![], last: now - CDuration::hours(2) }];
        let runs = outside_runs(&callers, &HashSet::new(), |_| Some(p.clone()), now);
        assert_eq!(runs.len(), 1, "its transcript was written just now");
        assert_eq!(runs[0].cwd.as_deref(), Some("C:/work"));
        assert_eq!(runs[0].title.as_deref(), Some("fix the board"));
        assert!(runs[0].readable);
    }

    #[test]
    fn the_head_gives_the_folder_and_hughs_first_line() {
        let text = [
            json!({ "type": "summary", "summary": "x" }).to_string(),
            json!({ "type": "user", "cwd": "C:/repo", "message": { "content": "<command-name>/clear</command-name>" } }).to_string(),
            json!({ "type": "user", "isMeta": true, "message": { "content": "Caveat: meta" } }).to_string(),
            json!({ "type": "user", "message": { "content": [{ "type": "text", "text": "Open any running agent session\nmore" }] } }).to_string(),
            "not json".to_string(),
        ]
        .join("\n");
        let (cwd, title) = head_info(&text);
        assert_eq!(cwd.as_deref(), Some("C:/repo"));
        assert_eq!(title.as_deref(), Some("Open any running agent session"));
        let long = json!({ "type": "user", "message": { "content": "y".repeat(100) } }).to_string();
        assert_eq!(head_info(&long).1.unwrap().chars().count(), 81, "80 and an ellipsis");
    }

    #[test]
    fn only_live_unfinished_unstopped_subagents_run_with_a_floor_on_the_interval() {
        let now = t("2026-11-09T10:00:00Z");
        let subs = vec![
            sub("a", "card-critic", "2026-11-09T09:50:00Z", "2026-11-09T09:58:00Z", false, false),
            sub("b", "card-tester", "2026-11-09T09:00:00Z", "2026-11-09T09:30:00Z", false, false), // quiet half an hour
            sub("c", "card-critic", "2026-11-09T09:55:00Z", "2026-11-09T09:59:00Z", true, false),
            sub("d", "card-critic", "2026-11-09T09:55:00Z", "2026-11-09T09:59:00Z", false, true),
        ];
        let ids = |v: Vec<transcript::Subagent>| v.into_iter().map(|s| s.id).collect::<Vec<_>>();
        assert_eq!(ids(running_subagents(&subs, std::time::Duration::from_secs(600), now)), vec!["a"]);
        // The monitor off (0) still lists one written two minutes ago: the floor is 10 minutes.
        assert_eq!(ids(running_subagents(&subs, std::time::Duration::ZERO, now)), vec!["a"]);
        // A longer interval keeps the quieter one too.
        assert_eq!(ids(running_subagents(&subs, std::time::Duration::from_secs(3600), now)), vec!["a", "b"]);
    }

    #[test]
    fn a_row_finds_its_subagent_by_id_else_by_kind_and_time_in_any_conversation() {
        let latest = vec![sub("new1", "card-critic", "2026-11-09T10:00:00Z", "2026-11-09T10:05:00Z", true, false)];
        let older = vec![
            sub("old1", "card-critic", "2026-11-08T10:00:00Z", "2026-11-08T10:05:00Z", true, false),
            sub("old2", "card-tester", "2026-11-08T11:00:00Z", "2026-11-08T11:05:00Z", true, false),
        ];
        let lead = vec![sub("m1", "card-reproducer", "2026-11-08T12:00:00Z", "2026-11-08T12:05:00Z", true, false)];
        let found = vec![(0, latest), (1, older), (2, lead)];
        // By id, in an older conversation.
        assert_eq!(pick_subagent(&found, Some("old1"), None, None).map(|(i, s)| (i, s.id.as_str())), Some((1, "old1")));
        assert!(pick_subagent(&found, Some("gone"), None, None).is_none());
        // An id not found falls back to the kind and the row's time.
        assert_eq!(pick_subagent(&found, Some("gone"), Some("card-tester"), Some("2026-11-08T12:00:00Z")).map(|(_, s)| s.id.as_str()), Some("old2"));
        // By kind and time: yesterday's critic row finds yesterday's critic, not today's.
        assert_eq!(pick_subagent(&found, None, Some("card-critic"), Some("2026-11-08T10:10:00Z")).map(|(_, s)| s.id.as_str()), Some("old1"));
        assert_eq!(pick_subagent(&found, None, Some("card-critic"), Some("2026-11-09T10:06:00Z")).map(|(_, s)| s.id.as_str()), Some("new1"));
        // In the stack lead's conversation.
        assert_eq!(pick_subagent(&found, None, Some("card-reproducer"), None).map(|(i, s)| (i, s.id.as_str())), Some((2, "m1")));
        // None before the row's time.
        assert!(pick_subagent(&found, None, Some("card-tester"), Some("2026-11-08T09:00:00Z")).is_none());
    }
}
