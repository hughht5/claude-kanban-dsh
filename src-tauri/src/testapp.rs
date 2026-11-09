//! Turns on the shared test app (card #115): the app's side.
//!
//! The queue itself (tester and install turns, the line, the rules) is pure logic in
//! `kanban_store::testapp`. This module joins it to the world:
//!
//! - the shared folder `%USERPROFILE%\.claude-kanban-dsh-shared\testapp\`, outside both
//!   channels' data folders, which `scripts\install.ps1` reads:
//!   - `install.lock`, held by install.ps1 while it waits for and does a test-app install
//!     (delete-on-close, so Windows lets go of it if the script is killed);
//!   - `turn-live.json` / `turn-test.json`, this app's current holder, written here
//!     atomically, so the script waits for a tester before it restarts the test app;
//!   - `hold.json`, Hugh's Hold (or Let it restart), from either board's banner;
//!   - `input-test.json`, the test app's last input from Hugh: installs wait 10 min after it;
//! - the old DSH lock folder `%USERPROFILE%\.dsh-test-app.lock` (the change-over: while
//!   another owner has it, nobody gets a turn; Hugh's Release renames it, never deletes);
//! - the pump, every 2 s from the dispatch loop (its first run is the start-up check):
//!   prune the line, end stale turns, renew turns in use, give the next turn, tell sessions
//!   by a queued message only (never a resume), write the turn file, emit "testapp";
//! - the kanban_test_app tool and the page's commands.
//!
//! Files are read before the board's mutex is taken, and written after it is let go.

use crate::{sessions::Sessions, Board};
use chrono::{DateTime, Duration, Utc};
use kanban_store::testapp::{self as q, stamp, Ask, Ended, Take, TestAppQueue, Turn, INSTALL, TESTER};
use kanban_store::Store;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

/// Hold restarts this long per click, and never more than HOLD_MAX from now.
pub const HOLD_STEP_MINUTES: i64 = 30;
pub const HOLD_MAX_MINUTES: i64 = 120;
/// Let it restart: Hold, Hugh's input and the notice don't count for this long.
pub const ALLOW_MINUTES: i64 = 5;
/// install.ps1 waits this long after Hugh's last input in the test app.
pub const INPUT_QUIET_MINUTES: i64 = 10;
/// The old DSH lock folder, made by an older writer (no pid line), counts as stale after this
/// long and then blocks nobody (card #149). install.ps1 uses the same number.
pub const DSH_STALE_MINUTES: i64 = 60;
/// A turn in use is renewed at most this often.
pub const RENEW_EVERY_SECS: i64 = 60;

fn home() -> PathBuf {
    kanban_store::home_var().unwrap_or_else(|| PathBuf::from("."))
}

pub fn shared_dir() -> PathBuf {
    home().join(".claude-kanban-dsh-shared").join("testapp")
}

pub fn dsh_dir() -> PathBuf {
    home().join(".dsh-test-app.lock")
}

use kanban_store::testapp::parse;

// --- files ------------------------------------------------------------------------------

/// Reads a file the way the protocol says: with every kind of sharing (a holder may have it
/// open for writing and delete-on-close), closed at once.
pub fn read_shared(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    crate::platform::share_all(&mut o);
    let mut f = o.open(path).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s.trim_start_matches('\u{feff}').to_string())
}

/// Writes `value` to `path` through a temp file and a rename, so no reader sees half a file.
/// A rename refused by a reader that holds the file is tried again a few times; the next
/// pump writes it anyway.
pub fn write_atomic(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let mut last = String::new();
    for _ in 0..5 {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) => last = e.to_string(),
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
    let _ = std::fs::remove_file(&tmp);
    Err(last)
}

/// The start time of process `pid`, if it is running (the platform module, card #114; none
/// off Windows, where install.sh's flock is the real check).
pub use crate::platform::process_start;

/// Process `pid` is running and started at `start` (within 2 s): a reused pid doesn't count.
pub fn alive_since(pid: u32, start: Option<DateTime<Utc>>) -> bool {
    match (process_start(pid), start) {
        (Some(s), Some(want)) => (s - want).num_milliseconds().abs() <= 2000,
        _ => false,
    }
}

/// install.lock as the holder wrote it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lock {
    pub pid: u32,
    pub pid_start: Option<DateTime<Utc>>,
    pub owner: String,
    pub label: String,
    pub card: Option<String>,
    pub phase: String,
    pub waiting_for: Option<String>,
    pub restart_at: Option<String>,
    pub started_at: Option<String>,
}

pub fn parse_lock(text: &str) -> Option<Lock> {
    let v: Value = serde_json::from_str(text).ok()?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    Some(Lock {
        pid: v.get("pid").and_then(Value::as_u64)? as u32,
        // RFC 3339 text (install.ps1, kb install-lock); milliseconds too, as older kb wrote it.
        pid_start: s("pidStart").as_deref().and_then(parse).or_else(|| v.get("pidStart").and_then(Value::as_i64).and_then(DateTime::from_timestamp_millis)),
        owner: s("owner").unwrap_or_default(),
        label: s("label").unwrap_or_default(),
        card: v.get("card").and_then(|c| c.as_str().map(str::to_string).or_else(|| c.as_i64().map(|n| n.to_string()))),
        phase: s("phase").unwrap_or_default(),
        waiting_for: s("waitingFor"),
        restart_at: s("restartAt"),
        started_at: s("startedAt"),
    })
}

/// The install holding install.lock now, if any: the file is there and its writer runs.
pub fn read_lock(dir: &Path) -> Option<Lock> {
    let path = dir.join("install.lock");
    // On a Mac the file outlives its install: only a held flock means an install (card #117).
    if crate::platform::file_lock_held(&path) == Some(false) {
        return None;
    }
    let l = parse_lock(&read_shared(&path)?)?;
    held(&l).then_some(l)
}

/// The installs waiting to restart the test app (install-wait-<pid>.json, written by
/// install.ps1 before it takes install.lock), whose writers still run.
pub fn read_waits(dir: &Path) -> Vec<Lock> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut out: Vec<Lock> = rd
        .flatten()
        .filter(|e| e.file_name().to_str().map_or(false, |n| n.starts_with("install-wait-") && n.ends_with(".json")))
        .filter_map(|e| parse_lock(&read_shared(&e.path())?))
        .filter(held)
        .collect();
    out.sort_by(|a, b| a.started_at.cmp(&b.started_at));
    out
}

/// The writer runs (`alive_since`). Off Windows the file's presence is all this checks
/// (`platform::PID_LOCKS`); #117 replaces it with a flock test.
fn held(l: &Lock) -> bool {
    !crate::platform::PID_LOCKS || alive_since(l.pid, l.pid_start)
}

/// hold.json: Hugh's Hold and Let it restart.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hold {
    pub until: Option<DateTime<Utc>>,
    pub allow_until: Option<DateTime<Utc>>,
}

/// A hold as it counts at `now`: expired parts drop, and an until more than HOLD_MAX after
/// the file was written counts as that.
pub fn parse_hold(text: &str, written: Option<DateTime<Utc>>, now: DateTime<Utc>) -> Hold {
    let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let at = |k: &str| v.get(k).and_then(Value::as_str).and_then(parse);
    let cap = written.map(|w| w + Duration::minutes(HOLD_MAX_MINUTES));
    let until = at("until").map(|u| cap.map_or(u, |c| u.min(c))).filter(|u| *u > now);
    let allow_until = at("allowUntil").filter(|a| *a > now);
    Hold { until, allow_until }
}

pub fn read_hold(dir: &Path, now: DateTime<Utc>) -> Hold {
    let path = dir.join("hold.json");
    let written = std::fs::metadata(&path).and_then(|m| m.modified()).ok().map(DateTime::<Utc>::from);
    read_shared(&path).map(|t| parse_hold(&t, written, now)).unwrap_or_default()
}

/// Hold: 30 min more from the later of now and the current hold, never past 2 h from now.
pub fn held_until(current: Option<DateTime<Utc>>, now: DateTime<Utc>) -> DateTime<Utc> {
    (current.unwrap_or(now).max(now) + Duration::minutes(HOLD_STEP_MINUTES)).min(now + Duration::minutes(HOLD_MAX_MINUTES))
}

pub fn read_input(dir: &Path) -> Option<DateTime<Utc>> {
    let v: Value = serde_json::from_str(&read_shared(&dir.join("input-test.json"))?).ok()?;
    v.get("at").and_then(Value::as_str).and_then(parse)
}

/// The old DSH lock folder: its owner and how long it has been there.
#[derive(Debug, Clone, PartialEq)]
pub struct Dsh {
    pub owner: String,
    pub age_minutes: i64,
    /// Its owner file names a process (line 2, "pid <pid> <start>", written by the new
    /// install.ps1) that no longer runs: a killed run's folder, which blocks nobody.
    pub dead: bool,
    /// Its owner file names a process that still runs: an install holds it, whatever its age.
    pub alive: bool,
}

impl Dsh {
    /// It counts as nobody's (card #149): its run has ended, or an older writer (no pid line)
    /// made it DSH_STALE_MINUTES or more ago. A folder whose run still runs is never stale.
    pub fn stale(&self) -> bool {
        self.dead || (!self.alive && self.age_minutes >= DSH_STALE_MINUTES)
    }

    /// It keeps the test app from everyone but its owner: a fresh folder, or one an install
    /// still holds.
    pub fn blocks(&self, owner: &str) -> bool {
        !self.stale() && self.owner != owner
    }
}

pub fn read_dsh(dir: &Path, now: DateTime<Utc>) -> Option<Dsh> {
    let meta = std::fs::metadata(dir).ok().filter(|m| m.is_dir())?;
    let made = meta.created().or_else(|_| meta.modified()).ok().map(DateTime::<Utc>::from).unwrap_or(now);
    let text = read_shared(&dir.join("owner")).unwrap_or_default();
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let owner = lines.next().unwrap_or("").to_string();
    // Some(running) when line 2 names a process; None for an older writer's folder.
    let runs = lines.next().and_then(|l| {
        let mut w = l.split_whitespace();
        (w.next() == Some("pid")).then_some(())?;
        let pid = w.next()?.parse::<u32>().ok()?;
        Some(alive_since(pid, w.next().and_then(parse)))
    });
    Some(Dsh { owner, age_minutes: (now - made).num_minutes().max(0), dead: runs == Some(false), alive: runs == Some(true) })
}

/// Hugh's Release of the old DSH lock folder: renamed aside, never deleted.
pub fn release_dsh(dir: &Path, now: DateTime<Utc>) -> Result<PathBuf, String> {
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or(".dsh-test-app.lock");
    let aside = dir.with_file_name(format!("{name}.released-{}", now.format("%Y%m%dT%H%M%SZ")));
    std::fs::rename(dir, &aside).map_err(|e| e.to_string())?;
    Ok(aside)
}

// --- what the pump sees -------------------------------------------------------------------

/// Everything read from files, before the board's mutex.
#[derive(Debug, Clone, Default)]
pub struct Seen {
    /// The install holding install.lock (it is quitting, installing or starting the app).
    pub lock: Option<Lock>,
    /// install.lock is there at all. Delete-on-close removes it when its install ends, so a
    /// file that is there but can't be read this instant still counts as an install running.
    pub lock_present: bool,
    /// Installs waiting to restart the test app.
    pub waits: Vec<Lock>,
    pub hold: Hold,
    pub input: Option<DateTime<Utc>>,
    pub dsh: Option<Dsh>,
    /// The other board's turn on the same test app (its owner), if it holds one now.
    pub other: Option<String>,
}

pub fn look(dir: &Path, dsh: &Path, now: DateTime<Utc>) -> Seen {
    let lock = read_lock(dir);
    let lock_path = dir.join("install.lock");
    let lock_present = lock.is_some() || crate::platform::file_lock_held(&lock_path).unwrap_or_else(|| lock_path.exists());
    let other_ch = if kanban_store::is_test() { "live" } else { "test" };
    Seen {
        lock,
        lock_present,
        waits: read_waits(dir),
        hold: read_hold(dir, now),
        input: read_input(dir),
        dsh: read_dsh(dsh, now),
        other: read_shared(&dir.join(format!("turn-{other_ch}.json"))).and_then(|t| other_holder(&t, now)),
    }
}

/// The owner in another app's turn file, if that app still runs and the turn hasn't expired.
pub fn other_holder(text: &str, now: DateTime<Utc>) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    let h = v.get("holder").filter(|h| !h.is_null())?;
    let expires = h.get("expiresAt").and_then(Value::as_str).and_then(parse)?;
    let pid = v.get("appPid").and_then(Value::as_u64)? as u32;
    let start = v.get("appStart").and_then(Value::as_str).and_then(parse);
    (expires > now && alive_since(pid, start)).then(|| h.get("owner").and_then(Value::as_str).unwrap_or("").to_string())
}

impl Seen {
    /// Someone has the test app outside this queue, so only `owner` may get a turn now: an
    /// install holding install.lock, the other board's turn, the old DSH lock folder.
    pub fn blocks(&self, owner: &str) -> bool {
        let lock = self.lock_present && self.lock.as_ref().map_or(true, |l| l.owner != owner);
        let other = self.other.as_deref().map_or(false, |o| o != owner);
        let dsh = self.dsh.as_ref().map_or(false, |d| d.blocks(owner));
        lock || other || dsh
    }
}

/// The sessions' side: which cards have a live session and which are busy (working, or a
/// test or review subagent running).
#[derive(Debug, Clone, Default)]
pub struct Live {
    pub live: HashSet<String>,
    pub busy: HashSet<String>,
}

fn live_of(app: &AppHandle) -> Live {
    let s = app.state::<Arc<Sessions>>();
    let states = s.states();
    let mut busy: HashSet<String> = states.iter().filter(|(_, st)| matches!(st.as_str(), "working" | "permission")).map(|(k, _)| k.clone()).collect();
    busy.extend(s.testing_cards());
    Live { live: states.into_keys().collect(), busy }
}

/// What one pump decided, for the logs and messages it sends after.
#[derive(Debug, Default, PartialEq)]
pub struct Step {
    /// (card id, log line).
    pub logs: Vec<(String, String)>,
    /// (card id, message for its session, queued only).
    pub messages: Vec<(String, String)>,
    pub changed: bool,
}

/// The given message to a card's session.
pub fn given_message(number: i64, limit: i64) -> String {
    // The test app this queue gives turns on: this app itself in a test slot, else slot 1.
    let port = kanban_store::devtools_port_for(if kanban_store::is_test() { kanban_store::slot() } else { 1 });
    format!(
        "It is your turn on the test app (card #{number}). Use it now for your own checks and your card-tester (CDP port {port}). \
If your card needs its build installed first, run install.ps1 in the background now: it waits for other installs and for Hugh, and says why. When you are done with it, release the turn: kanban_test_app with action \"release\". \
The turn ends by itself when this card leaves In progress and Human test, or after {limit} min with nothing running."
    )
}

pub fn ended_message(why: &str) -> String {
    format!("Your turn on the test app ended: {why}. If you still need the test app, take a new turn: kanban_test_app with action \"take\".")
}

/// One pass of the rules over the queue: pure apart from `card`, so it is tested here.
pub fn step(
    queue: &mut TestAppQueue,
    now: DateTime<Utc>,
    limit: i64,
    seen: &Seen,
    live: &Live,
    card: impl Fn(&str) -> q::CardState,
) -> Step {
    let before = queue.clone();
    let mut out = Step::default();
    for (w, why) in queue.prune(now, &card) {
        if let Some(id) = w.who.card_id {
            out.logs.push((id, format!("Left the line for the test app: {}", why.text())));
        }
    }
    // In use now: the clock starts again.
    let lock_owner = seen.lock.as_ref().map(|l| l.owner.as_str());
    if let Some(h) = queue.holder.as_mut() {
        let own = h.who.owner.as_str();
        let installing = lock_owner == Some(own) || seen.waits.iter().any(|w| w.owner == own);
        if h.who.kind == INSTALL && installing {
            h.install_seen = true;
        }
        let busy = h.who.card_id.as_deref().map_or(false, |id| live.busy.contains(id));
        // At most once a minute, so a busy holder doesn't rewrite the setting, the turn file
        // and the page's event every 2 s; the limit is minutes.
        let due = parse(&h.renewed_at).map_or(true, |r| now - r >= Duration::seconds(RENEW_EVERY_SECS));
        if (installing || busy) && due {
            queue.renew(now);
        }
    }
    // Done only once install.lock is gone (or another's): a file there but unreadable for a
    // moment is still this install's.
    let install_done = queue.holder.as_ref().map_or(false, |h| {
        let own = h.who.owner.as_str();
        !seen.waits.iter().any(|w| w.owner == own) && (!seen.lock_present || seen.lock.as_ref().map_or(false, |l| l.owner != own))
    });
    if let Some(why) = queue.stale(now, limit, &card, install_done) {
        if let Some(t) = queue.force() {
            ended(&mut out, &t, &why, &card);
        }
    }
    if let Some(t) = queue.next(now, |id| live.live.contains(id), |owner| seen.blocks(owner)) {
        if let (Some(id), Some(n)) = (t.who.card_id.clone(), t.who.card) {
            out.logs.push((id.clone(), "Got its turn on the test app".into()));
            out.messages.push((id, given_message(n, limit)));
        }
    }
    out.changed = *queue != before;
    out
}

fn ended(out: &mut Step, t: &Turn, why: &Ended, card: impl Fn(&str) -> q::CardState) {
    let Some(id) = t.who.card_id.clone() else { return };
    out.logs.push((id.clone(), format!("Its turn on the test app ended: {}", why.text())));
    // A card that left the columns that test has nothing to hear.
    if matches!(card(&id), Some((col, false)) if q::TESTER_COLUMNS.contains(&col.as_str())) {
        out.messages.push((id, ended_message(&why.text())));
    }
}

// --- status -------------------------------------------------------------------------------

fn who_json(a: &Ask) -> Value {
    json!({ "kind": a.kind, "card": a.card, "cardId": a.card_id, "owner": a.owner, "label": a.label })
}

/// The status the page, the tool and kb show.
pub fn status(queue: &TestAppQueue, now: DateTime<Utc>, limit: i64, seen: &Seen, live: &Live) -> Value {
    let holder = queue.holder.as_ref().map(|h| {
        let mut v = who_json(&h.who);
        let held = parse(&h.given_at).map(|g| (now - g).num_minutes()).unwrap_or(0);
        v["givenAt"] = json!(h.given_at);
        v["renewedAt"] = json!(h.renewed_at);
        v["expiresAt"] = json!(queue.expires_at(limit).map(stamp));
        v["heldMinutes"] = json!(held.max(0));
        v["overdue"] = json!(queue.overdue(now));
        v
    });
    let line: Vec<Value> = queue
        .line
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let mut v = who_json(&w.who);
            v["place"] = json!(i + 1);
            v["live"] = json!(w.who.card_id.as_deref().map_or(true, |id| live.live.contains(id)));
            v["askedAt"] = json!(w.asked_at);
            v
        })
        .collect();
    let install = seen.lock.as_ref().or(seen.waits.first()).map(|l| {
        json!({ "owner": l.owner, "label": l.label, "card": l.card, "phase": l.phase, "waitingFor": l.waiting_for,
                "restartAt": l.restart_at, "startedAt": l.started_at })
    });
    // A restart is pending: an install waiting to restart, else an install turn held or in line.
    let pending = seen
        .waits
        .first()
        .or(seen.lock.as_ref().filter(|l| l.phase == "waiting"))
        .map(|l| l.label.clone())
        .or_else(|| queue.holder.as_ref().filter(|h| h.who.kind == INSTALL).map(|h| h.who.label.clone()))
        .or_else(|| queue.line.iter().find(|w| w.who.kind == INSTALL).map(|w| w.who.label.clone()))
        .map(|label| json!({ "label": label }));
    let input_recent = seen.input.map_or(false, |at| now - at < Duration::minutes(INPUT_QUIET_MINUTES));
    json!({
        "channel": kanban_store::channel(),
        "limitMinutes": limit,
        "holder": holder,
        "line": line,
        "install": install,
        "pending": pending,
        "hold": seen.hold.until.map(|u| json!({ "until": stamp(u) })),
        "allowUntil": seen.hold.allow_until.map(stamp),
        "hughInputAt": seen.input.map(stamp),
        "hughInputRecent": input_recent,
        "mkdir": seen.dsh.as_ref().map(|d| json!({ "owner": d.owner, "ageMinutes": d.age_minutes, "stale": d.stale(), "dead": d.dead, "alive": d.alive })),
        "otherBoard": seen.other,
    })
}

/// The turn file install.ps1 reads: this app's holder, and who this app is, so a file left
/// by an app that has quit doesn't count.
pub fn turn_file(queue: &TestAppQueue, limit: i64, me: (u32, Option<DateTime<Utc>>), now: DateTime<Utc>) -> Value {
    let holder = queue.holder.as_ref().map(|h| {
        json!({ "kind": h.who.kind, "card": h.who.card, "owner": h.who.owner, "label": h.who.label,
                "givenAt": h.given_at, "expiresAt": queue.expires_at(limit).map(stamp) })
    });
    json!({ "channel": kanban_store::channel(), "appPid": me.0, "appStart": me.1.map(stamp), "writtenAt": stamp(now), "holder": holder })
}

// --- the pump -----------------------------------------------------------------------------

/// What was last written and sent, so the file and the event change only on a change.
#[derive(Default)]
struct Last {
    turn: Option<Value>,
    status: Option<Value>,
}

static LAST: Mutex<Option<Last>> = Mutex::new(None);

fn card_state(store: &Store) -> impl Fn(&str) -> q::CardState + '_ {
    move |id: &str| store.get(id).ok().map(|c| (c.column.clone(), c.archived))
}

/// Every 2 s from the dispatch loop, and after each tool call or command.
pub fn pump(app: &AppHandle) {
    let now = Utc::now();
    let seen = look(&shared_dir(), &dsh_dir(), now);
    let live = live_of(app);
    let board = app.state::<Board>();
    let (st, queue, limit) = {
        let Ok(mut store) = board.0.lock() else { return };
        let mut queue = store.test_app_queue().unwrap_or_default();
        let limit = store.test_app_limit().unwrap_or(q::DEFAULT_LIMIT);
        let st = step(&mut queue, now, limit, &seen, &live, card_state(&store));
        if st.changed {
            let _ = store.save_test_app_queue(&queue);
        }
        for (id, line) in &st.logs {
            let _ = store.add_log(id, line, "the app");
        }
        // Queued only: typed when the session is next free, or heard at its next start.
        for (id, text) in &st.messages {
            let _ = store.queue(id, "message", text);
        }
        (st, queue, limit)
    };
    publish(app, &queue, limit, &seen, &live, now);
    if !st.logs.is_empty() {
        let _ = app.emit("cards-changed", ());
    }
}

/// Writes the turn file and sends "testapp" when either changed.
fn publish(app: &AppHandle, queue: &TestAppQueue, limit: i64, seen: &Seen, live: &Live, now: DateTime<Utc>) {
    static ME: std::sync::OnceLock<(u32, Option<DateTime<Utc>>)> = std::sync::OnceLock::new();
    let me = *ME.get_or_init(|| (std::process::id(), process_start(std::process::id())));
    let turn = turn_file(queue, limit, me, now);
    let status = status(queue, now, limit, seen, live);
    let Ok(mut last) = LAST.lock() else { return };
    let last = last.get_or_insert_with(Last::default);
    // writtenAt alone isn't a change.
    let mut cmp = turn.clone();
    cmp["writtenAt"] = Value::Null;
    if last.turn.as_ref() != Some(&cmp) {
        let file = shared_dir().join(format!("turn-{}.json", kanban_store::channel()));
        if write_atomic(&file, &turn).is_ok() {
            last.turn = Some(cmp);
        }
    }
    // The minutes held move on their own; the page counts them from the times.
    let mut scmp = status.clone();
    if let Some(h) = scmp.get_mut("holder").and_then(Value::as_object_mut) {
        h.remove("heldMinutes");
    }
    if last.status.as_ref() != Some(&scmp) {
        last.status = Some(scmp);
        let _ = app.emit("testapp", status);
    }
}

/// The current status, read fresh.
pub fn snapshot(app: &AppHandle) -> Result<Value, String> {
    let now = Utc::now();
    let seen = look(&shared_dir(), &dsh_dir(), now);
    let live = live_of(app);
    let board = app.state::<Board>();
    let store = board.0.lock().map_err(|e| e.to_string())?;
    let queue = store.test_app_queue().map_err(|e| e.to_string())?;
    let limit = store.test_app_limit().unwrap_or(q::DEFAULT_LIMIT);
    Ok(status(&queue, now, limit, &seen, &live))
}

// --- the tool -----------------------------------------------------------------------------

/// What a take or release told the asker.
fn take_reply(t: &Take, label: &str, limit: i64, number: Option<i64>) -> String {
    match t {
        Take::Given => match number {
            Some(n) => given_message(n, limit),
            None => format!("The test app is yours ({label}): install it now. The turn ends when your install has started the test app again, when you release it, or after {limit} min with nothing running."),
        },
        Take::Renewed => format!("The test app is still yours ({label}); its {limit} min start again."),
        Take::Waiting(p) => match number {
            Some(_) => format!("You are number {p} in line for the test app. The app tells this session when it is your turn (a message, typed when you are next free). End your turn now; don't wait in a loop."),
            None => format!("Number {p} in line for the test app (an install goes ahead of every tester, as soon as the current turn ends). Check with kb testapp, or wait with kb testapp take --install --wait."),
        },
    }
}

/// kanban_test_app. A card's session takes and releases tester turns for its own card.
/// A session outside the app takes install turns (owner, label) and may force a release.
pub fn tool(app: &AppHandle, caller: Option<&str>, args: &Value) -> Result<Value, String> {
    let action = args.get("action").and_then(Value::as_str).unwrap_or("status");
    if action == "status" {
        return snapshot(app);
    }
    let now = Utc::now();
    if action == "release-mkdir" {
        let own = {
            let board = app.state::<Board>();
            let store = board.0.lock().map_err(|e| e.to_string())?;
            crate::tools::caller_card(app, &store, caller)
        };
        if own.is_some() {
            return Err("a card's session can't release the old DSH lock folder".into());
        }
        let aside = release_dsh(&dsh_dir(), now)?;
        pump(app);
        return Ok(json!(format!("Released the old DSH lock folder: renamed to {}.", aside.display())));
    }
    let seen = look(&shared_dir(), &dsh_dir(), now);
    let live = live_of(app);
    let reply = {
        let board = app.state::<Board>();
        let mut store = board.0.lock().map_err(|e| e.to_string())?;
        let own = crate::tools::caller_card(app, &store, caller);
        let mut queue = store.test_app_queue().map_err(|e| e.to_string())?;
        let limit = store.test_app_limit().unwrap_or(q::DEFAULT_LIMIT);
        let reply = act(&mut store, &mut queue, own.as_deref(), action, args, now, limit, &seen, &live)?;
        store.save_test_app_queue(&queue).map_err(|e| e.to_string())?;
        reply
    };
    // Hands a released turn on, writes the turn file and tells the page.
    pump(app);
    let _ = app.emit("cards-changed", ());
    Ok(json!(reply))
}

/// The tool's rules over the store, apart from files and sessions.
/// A take gives the turn now by the pump's own rule (`TestAppQueue::take_now`): paused cards
/// ahead are passed over, and `seen` blocks as it does in `step`.
#[allow(clippy::too_many_arguments)]
pub fn act(store: &mut Store, queue: &mut TestAppQueue, own: Option<&str>, action: &str, args: &Value, now: DateTime<Utc>, limit: i64, seen: &Seen, live: &Live) -> Result<String, String> {
    let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
    let install = args.get("install").and_then(Value::as_bool).unwrap_or(false) || args.get("kind").and_then(Value::as_str) == Some(INSTALL);
    match (own, action) {
        (Some(id), "take") => {
            if install {
                return Err("a card's session takes a tester turn only; installs are taken from outside a card session (kb testapp take --install), by Hugh's own session".into());
            }
            let c = store.get(id).map_err(|e| e.to_string())?;
            if let Some(why) = q::take_refused(&c.column, c.archived) {
                return Err(format!("refused: {why}"));
            }
            let ask = Ask::tester(id, c.number);
            let t = queue.take_now(ask, now, |id| live.live.contains(id), |o| seen.blocks(o));
            let line = match &t {
                Take::Given => "Took its turn on the test app".to_string(),
                Take::Renewed => "Renewed its turn on the test app".to_string(),
                Take::Waiting(p) => format!("Waits for the test app: number {p} in line"),
            };
            let _ = store.add_log(id, &line, &format!("card #{}'s session", c.number));
            Ok(take_reply(&t, &format!("#{}", c.number), limit, Some(c.number)))
        }
        (Some(id), "release") => {
            if force {
                return Err("a card's session can't force a release; it releases only its own turn".into());
            }
            let c = store.get(id).map_err(|e| e.to_string())?;
            match queue.release(Some(id), &c.number.to_string(), TESTER) {
                Some(true) => {
                    let _ = store.add_log(id, "Released its turn on the test app", &format!("card #{}'s session", c.number));
                    Ok("Released. The next card in line gets the test app.".into())
                }
                Some(false) => Ok("Left the line for the test app.".into()),
                None => Ok("This card has no turn on the test app and isn't in line.".into()),
            }
        }
        (None, "take") => {
            if !install {
                return Err("from outside a card session, take an install turn: kb testapp take --install --owner <name> --label <label>".into());
            }
            let owner = args.get("owner").and_then(Value::as_str).map(str::trim).filter(|o| !o.is_empty()).ok_or("pass the install's owner (the KANBAN_LOCK_OWNER its install.ps1 runs with), for example --owner hugh")?;
            let label = args.get("label").and_then(Value::as_str).unwrap_or("");
            let ask = Ask::install(owner, label);
            let shown = ask.label.clone();
            let t = queue.take_now(ask, now, |id| live.live.contains(id), |o| seen.blocks(o));
            Ok(take_reply(&t, &shown, limit, None))
        }
        (None, "release") => {
            if force {
                return Ok(match queue.force() {
                    Some(t) => {
                        if let Some(id) = &t.who.card_id {
                            let _ = store.add_log(id, "Its turn on the test app was released by a session outside the app", "the app");
                            let _ = store.queue(id, "message", &ended_message("a session outside the app released it"));
                        }
                        format!("Released {}'s turn on the test app.", t.who.label)
                    }
                    None => "Nobody holds the test app.".into(),
                });
            }
            let owner = args.get("owner").and_then(Value::as_str).map(str::trim).filter(|o| !o.is_empty()).unwrap_or("hugh");
            Ok(match queue.release(None, owner, INSTALL) {
                Some(true) => format!("Released {owner}'s install turn."),
                Some(false) => format!("Took {owner}'s install out of the line."),
                None => format!("{owner} has no install turn and isn't in line."),
            })
        }
        (_, other) => Err(format!("action is take, release or status, not {other}")),
    }
}

pub fn schema() -> Value {
    json!({ "name": "kanban_test_app",
            "description": "Turns on the shared test app. take: ask for a tester turn for your own card (In progress or Human test). You get it now, or a place in line and a message when it is yours; take again to renew it. Use the installed test app only during your turn, for your own checks and your card-tester. release: give it back (or leave the line). status: who has it, the line, and any install. The turn ends by itself when your card leaves In progress and Human test, or after a time with nothing running.",
            "inputSchema": { "type": "object", "properties": {
                "action": { "type": "string", "enum": ["take", "release", "status"] },
                "install": { "type": "boolean", "description": "Outside a card session only: an install turn (ahead of every tester)." },
                "owner": { "type": "string", "description": "Outside only: the install's owner, the KANBAN_LOCK_OWNER its install.ps1 runs with (default hugh)." },
                "label": { "type": "string", "description": "Outside only: what the board shows, for example wave 3." },
                "force": { "type": "boolean", "description": "Outside only, with release: release whoever holds it." } }, "required": ["action"] } })
}

// --- the page's commands ------------------------------------------------------------------

fn changed(app: &AppHandle) -> Result<Value, String> {
    pump(app);
    snapshot(app)
}

#[tauri::command]
pub fn testapp_status(app: AppHandle) -> Result<Value, String> {
    snapshot(&app)
}

/// Hugh's Release on the chip: whoever holds the turn loses it.
#[tauri::command]
pub fn testapp_release(app: AppHandle) -> Result<Value, String> {
    {
        let board = app.state::<Board>();
        let mut store = board.0.lock().map_err(|e| e.to_string())?;
        let mut queue = store.test_app_queue().map_err(|e| e.to_string())?;
        if let Some(t) = queue.force() {
            store.save_test_app_queue(&queue).map_err(|e| e.to_string())?;
            if let Some(id) = &t.who.card_id {
                let _ = store.add_log(id, "Hugh released its turn on the test app", "Hugh");
                let _ = store.queue(id, "message", &ended_message("Hugh released it"));
            }
        }
    }
    let _ = app.emit("cards-changed", ());
    changed(&app)
}

/// Hold: no test-app restart for 30 min more (at most 2 h from now).
#[tauri::command]
pub fn testapp_hold(app: AppHandle) -> Result<Value, String> {
    let now = Utc::now();
    let dir = shared_dir();
    let current = read_hold(&dir, now).until;
    let until = held_until(current, now);
    write_atomic(&dir.join("hold.json"), &json!({ "until": stamp(until), "by": "Hugh", "allowUntil": null, "at": stamp(now) }))?;
    changed(&app)
}

/// Let it restart: clears the Hold, and for 5 min Hugh's input and the notice don't count.
#[tauri::command]
pub fn testapp_let_restart(app: AppHandle) -> Result<Value, String> {
    let now = Utc::now();
    let allow = now + Duration::minutes(ALLOW_MINUTES);
    write_atomic(&shared_dir().join("hold.json"), &json!({ "until": null, "by": "Hugh", "allowUntil": stamp(allow), "at": stamp(now) }))?;
    changed(&app)
}

/// The test app's page: Hugh used it (trusted input in a focused window). The live app
/// records nothing.
#[tauri::command]
pub fn testapp_input() -> Result<(), String> {
    if !kanban_store::is_test() {
        return Ok(());
    }
    write_atomic(&shared_dir().join("input-test.json"), &json!({ "at": stamp(Utc::now()) }))
}

/// Hugh's Release of the old DSH lock folder (renamed aside, never deleted).
#[tauri::command]
pub fn testapp_release_mkdir(app: AppHandle) -> Result<Value, String> {
    release_dsh(&dsh_dir(), Utc::now())?;
    changed(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// kb install-lock on a Mac and install.ps1 both write pidStart as RFC 3339 text; an
    /// older kb wrote milliseconds. Either is read (card #117).
    #[test]
    fn a_lock_s_pid_start_reads_as_text_or_milliseconds() {
        let text = parse_lock(r#"{"pid": 7, "pidStart": "2025-11-06T12:00:00.000Z", "owner": "o"}"#).unwrap();
        let ms = parse_lock(r#"{"pid": 7, "pidStart": 1762430400000, "owner": "o"}"#).unwrap();
        assert!(text.pid_start.is_some());
        assert_eq!(text.pid_start, ms.pid_start);
    }

    use std::collections::HashMap;

    fn t(min: i64) -> DateTime<Utc> {
        parse("2026-11-06T03:00:00Z").unwrap() + Duration::minutes(min)
    }
    fn lv(live: &[&str], busy: &[&str]) -> Live {
        Live { live: live.iter().map(|s| s.to_string()).collect(), busy: busy.iter().map(|s| s.to_string()).collect() }
    }
    fn cols(m: &[(&str, &str)]) -> impl Fn(&str) -> q::CardState {
        let m: HashMap<String, String> = m.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        move |id: &str| m.get(id).map(|c| (c.clone(), false))
    }

    #[test]
    fn the_next_card_hears_its_turn_by_a_queued_message_and_paused_cards_wait() {
        let mut queue = TestAppQueue::default();
        queue.take(Ask::tester("a", 1), t(0), false);
        queue.take(Ask::tester("b", 2), t(0), false);
        queue.take(Ask::tester("c", 3), t(0), false);
        let card = cols(&[("a", "Human test"), ("b", "In progress"), ("c", "In progress")]);
        // a moved on to Blocked: its turn ends; b is paused, so c gets it.
        let card2 = cols(&[("a", "Blocked"), ("b", "In progress"), ("c", "In progress")]);
        let s = step(&mut queue, t(1), 60, &Seen::default(), &lv(&["a", "c"], &[]), &card);
        assert!(!s.changed, "Human test keeps the turn");
        let s = step(&mut queue, t(2), 60, &Seen::default(), &lv(&["a", "c"], &[]), &card2);
        assert!(s.changed);
        assert_eq!(queue.holder.as_ref().unwrap().who.label, "#3");
        assert_eq!(queue.line.len(), 1, "b keeps its place");
        assert!(s.messages.iter().any(|(id, m)| id == "c" && m.contains("your turn")));
        assert!(!s.messages.iter().any(|(id, _)| id == "a"), "a left the testing columns: nothing to hear");
        assert!(s.logs.iter().any(|(id, l)| id == "a" && l.contains("moved to Blocked")));
    }

    #[test]
    fn a_busy_holder_or_its_running_tester_keeps_the_turn_past_the_limit() {
        let mut queue = TestAppQueue::default();
        queue.take(Ask::tester("a", 1), t(0), false);
        queue.take(Ask::tester("b", 2), t(0), false);
        let card = cols(&[("a", "In progress"), ("b", "In progress")]);
        step(&mut queue, t(50), 60, &Seen::default(), &lv(&["a", "b"], &["a"]), &card);
        let soon = t(50) + Duration::seconds(30);
        assert!(!step(&mut queue, soon, 60, &Seen::default(), &lv(&["a", "b"], &["a"]), &card).changed, "renewed at most once a minute");
        let s = step(&mut queue, t(100), 60, &Seen::default(), &lv(&["a", "b"], &[]), &card);
        assert!(!s.changed, "renewed at 50, so not out until 110");
        let s = step(&mut queue, t(111), 60, &Seen::default(), &lv(&["a", "b"], &[]), &card);
        assert_eq!(queue.holder.as_ref().unwrap().who.label, "#2");
        assert!(s.messages.iter().any(|(id, m)| id == "a" && m.contains("not used for 60 min")));
    }

    #[test]
    fn an_install_turn_ends_when_its_install_lets_go_of_the_lock() {
        let mut queue = TestAppQueue::default();
        queue.take(Ask::install("router", "wave 3"), t(0), false);
        queue.take(Ask::tester("a", 1), t(0), false);
        let card = cols(&[("a", "In progress")]);
        let lock = Seen { lock: Some(Lock { owner: "router".into(), phase: "installing".into(), ..Default::default() }), lock_present: true, ..Default::default() };
        step(&mut queue, t(1), 60, &Seen::default(), &lv(&["a"], &[]), &card);
        assert_eq!(queue.holder.as_ref().unwrap().who.label, "wave 3", "not seen installing yet: kept");
        step(&mut queue, t(2), 60, &lock, &lv(&["a"], &[]), &card);
        assert!(queue.holder.as_ref().unwrap().install_seen);
        // Someone else's install doesn't count as this one's.
        let other = Seen { lock: Some(Lock { owner: "hugh".into(), ..Default::default() }), lock_present: true, ..Default::default() };
        step(&mut queue, t(3), 60, &other, &lv(&["a"], &[]), &card);
        assert!(queue.holder.is_none(), "router's install is done, and Hugh's install holds the lock: nobody yet");
        let s = step(&mut queue, t(4), 60, &Seen::default(), &lv(&["a"], &[]), &card);
        assert_eq!(queue.holder.as_ref().unwrap().who.label, "#1");
        assert!(s.messages.iter().any(|(id, _)| id == "a"));
    }

    #[test]
    fn the_old_dsh_folder_holds_new_turns_for_its_owner() {
        let mut queue = TestAppQueue::default();
        queue.take(Ask::tester("a", 1), t(0), false);
        queue.take(Ask::tester("b", 2), t(0), false);
        queue.force();
        let dsh = Seen { dsh: Some(Dsh { owner: "105".into(), age_minutes: 3, dead: false, alive: false }), ..Default::default() };
        let card = cols(&[("a", "In progress"), ("b", "In progress")]);
        assert!(!step(&mut queue, t(1), 60, &dsh, &lv(&["a", "b"], &[]), &card).changed);
        let mine = Seen { dsh: Some(Dsh { owner: "2".into(), age_minutes: 3, dead: false, alive: false }), ..Default::default() };
        step(&mut queue, t(2), 60, &mine, &lv(&["a", "b"], &[]), &card);
        assert_eq!(queue.holder.as_ref().unwrap().who.label, "#2");
    }

    #[test]
    fn an_install_outside_the_queue_or_the_other_board_blocks_new_turns() {
        let card = cols(&[("a", "In progress"), ("b", "In progress")]);
        let mk = || {
            let mut q = TestAppQueue::default();
            q.take(Ask::tester("z", 9), t(0), false);
            q.take(Ask::tester("a", 1), t(0), false);
            q.take(Ask::tester("b", 2), t(0), false);
            q.force();
            q
        };
        // Hugh's own install holds install.lock with no turn: nobody gets one.
        let hugh = Seen { lock: Some(Lock { owner: "hugh".into(), ..Default::default() }), lock_present: true, ..Default::default() };
        let mut q = mk();
        assert!(!step(&mut q, t(1), 60, &hugh, &lv(&["a", "b"], &[]), &card).changed);
        // Present but unreadable this instant: still an install.
        let blur = Seen { lock_present: true, ..Default::default() };
        assert!(!step(&mut q, t(1), 60, &blur, &lv(&["a", "b"], &[]), &card).changed);
        // The live board's turn blocks the test board's queue, except for its own owner.
        let other = Seen { other: Some("2".into()), ..Default::default() };
        step(&mut q, t(2), 60, &other, &lv(&["a", "b"], &[]), &card);
        assert_eq!(q.holder.as_ref().unwrap().who.label, "#2");
        // A killed run's DSH folder (dead pid) blocks nobody.
        let mut q = mk();
        let dead = Seen { dsh: Some(Dsh { owner: "router".into(), age_minutes: 5, dead: true, alive: false }), ..Default::default() };
        step(&mut q, t(3), 60, &dead, &lv(&["a", "b"], &[]), &card);
        assert_eq!(q.holder.as_ref().unwrap().who.label, "#1");
    }

    fn dsh(owner: &str, age_minutes: i64, dead: bool, alive: bool) -> Seen {
        Seen { dsh: Some(Dsh { owner: owner.into(), age_minutes, dead, alive }), ..Default::default() }
    }

    /// Card #149, as it was on 9 Nov: nobody holds the turn, #131 is first in line and live,
    /// and an older writer's DSH folder (owner 108, no pid line) is 3410 min old.
    #[test]
    fn a_stale_old_dsh_folder_no_longer_blocks_the_line() {
        let card = cols(&[("card-131", "In progress")]);
        let mk = || {
            let mut q = TestAppQueue::default();
            q.take(Ask::tester("card-131", 131), t(0), true);
            q
        };
        let old = dsh("108", 3410, false, false);
        assert!(!old.blocks("131"));
        let mut q = mk();
        let s = step(&mut q, t(1), 60, &old, &lv(&["card-131"], &[]), &card);
        assert_eq!(q.holder.as_ref().unwrap().who.label, "#131");
        assert!(s.messages.iter().any(|(id, m)| id == "card-131" && m.contains("your turn")), "its session is told");
        assert_eq!(status(&q, t(1), 60, &old, &Live::default())["mkdir"]["stale"], true);
        // Just under the stale age, it still blocks; at it, it doesn't.
        let mut q = mk();
        assert!(!step(&mut q, t(1), 60, &dsh("108", DSH_STALE_MINUTES - 1, false, false), &lv(&["card-131"], &[]), &card).changed);
        step(&mut q, t(2), 60, &dsh("108", DSH_STALE_MINUTES, false, false), &lv(&["card-131"], &[]), &card);
        assert_eq!(q.holder.as_ref().unwrap().who.label, "#131");
    }

    #[test]
    fn a_fresh_dsh_folder_or_one_whose_install_still_runs_still_blocks() {
        let card = cols(&[("card-131", "In progress")]);
        let live = lv(&["card-131"], &[]);
        for (seen, what) in [(dsh("108", 3, false, false), "fresh, no pid line"), (dsh("108", 3410, false, true), "old, but its run still runs")] {
            let mut q = TestAppQueue::default();
            q.take(Ask::tester("card-131", 131), t(0), true);
            assert!(!step(&mut q, t(1), 60, &seen, &live, &card).changed, "{what}");
            assert!(q.holder.is_none(), "{what}");
            assert_eq!(status(&q, t(1), 60, &seen, &live)["mkdir"]["stale"], false, "{what}");
        }
        // Its own owner isn't blocked by it.
        let mut q = TestAppQueue::default();
        q.take(Ask::tester("card-131", 131), t(0), true);
        step(&mut q, t(1), 60, &dsh("131", 3, false, true), &live, &card);
        assert_eq!(q.holder.as_ref().unwrap().who.label, "#131");
    }

    #[test]
    fn an_install_still_blocks_the_first_live_card() {
        let card = cols(&[("card-131", "In progress")]);
        let live = lv(&["card-131"], &[]);
        let mk = || {
            let mut q = TestAppQueue::default();
            q.take(Ask::tester("card-131", 131), t(0), true);
            q
        };
        // An install outside the queue holding install.lock, or its file there but unreadable.
        for seen in [
            Seen { lock: Some(Lock { owner: "hugh".into(), phase: "installing".into(), ..Default::default() }), lock_present: true, ..Default::default() },
            Seen { lock_present: true, ..Default::default() },
        ] {
            let mut q = mk();
            assert!(!step(&mut q, t(1), 60, &seen, &live, &card).changed);
        }
        // An install turn held in the queue.
        let mut q = TestAppQueue::default();
        q.take(Ask::install("router", "wave 3"), t(0), false);
        q.take(Ask::tester("card-131", 131), t(0), false);
        let installing = Seen { lock: Some(Lock { owner: "router".into(), ..Default::default() }), lock_present: true, ..Default::default() };
        step(&mut q, t(1), 60, &installing, &live, &card);
        assert_eq!(q.holder.as_ref().unwrap().who.label, "wave 3");
        assert_eq!(q.line.len(), 1);
    }

    /// The tool's take, as #131's session called it: the reply says it has the turn.
    #[test]
    fn a_take_from_first_in_line_or_behind_a_paused_card_gets_the_turn() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("b.db")).unwrap();
        let mut p = kanban_store::Patch::new();
        p.insert("title".into(), json!("x"));
        let mut ids = vec![];
        for _ in 0..2 {
            let c = store.create(None, p.clone(), "t").unwrap();
            store.move_by_hugh(&c.id, c.version, "In progress", kanban_store::Patch::new()).unwrap();
            ids.push(c.id);
        }
        let (paused, me) = (ids[0].clone(), ids[1].clone());
        let live = lv(&[&me], &[]);
        let take = |s: &mut Store, q: &mut TestAppQueue, seen: &Seen| act(s, q, Some(&me), "take", &json!({}), t(1), 60, seen, &live).unwrap();
        // Blocked by a fresh DSH folder: in line. Asked again once it is stale: given.
        let mut queue = TestAppQueue::default();
        let r = take(&mut store, &mut queue, &dsh("108", 3, false, false));
        assert!(r.contains("number 1 in line"), "{r}");
        let r = take(&mut store, &mut queue, &dsh("108", 3410, false, false));
        assert!(r.contains("your turn"), "{r}");
        assert!(queue.line.is_empty());
        // Behind a card whose session isn't running.
        let mut queue = TestAppQueue::default();
        queue.take(Ask::tester(&paused, 1), t(0), true);
        let r = take(&mut store, &mut queue, &Seen::default());
        assert!(r.contains("your turn"), "{r}");
        assert_eq!(queue.line.len(), 1, "the paused card keeps its place");
    }

    #[test]
    fn an_install_turn_isn_t_done_while_the_lock_file_is_there() {
        let mut queue = TestAppQueue::default();
        queue.take(Ask::install("router", "wave 3"), t(0), false);
        queue.holder.as_mut().unwrap().install_seen = true;
        let blur = Seen { lock_present: true, ..Default::default() };
        step(&mut queue, t(1), 60, &blur, &Live::default(), cols(&[]));
        assert!(queue.holder.is_some(), "unreadable for a moment is not done");
        let waiting = Seen { waits: vec![Lock { owner: "router".into(), ..Default::default() }], ..Default::default() };
        step(&mut queue, t(2), 60, &waiting, &Live::default(), cols(&[]));
        assert!(queue.holder.is_some(), "its run is waiting to restart");
        step(&mut queue, t(3), 60, &Seen::default(), &Live::default(), cols(&[]));
        assert!(queue.holder.is_none());
    }

    #[test]
    fn other_board_turn_counts_only_while_its_app_runs() {
        let me = std::process::id();
        let start = process_start(me).map(stamp);
        let now = Utc::now();
        let live = json!({ "appPid": me, "appStart": start, "holder": { "owner": "7", "expiresAt": stamp(now + Duration::minutes(5)) } }).to_string();
        assert_eq!(other_holder(&live, now).as_deref(), Some("7"));
        let gone = json!({ "appPid": me, "appStart": "2020-01-01T00:00:00Z", "holder": { "owner": "7", "expiresAt": stamp(now + Duration::minutes(5)) } }).to_string();
        assert!(other_holder(&gone, now).is_none());
        let expired = json!({ "appPid": me, "appStart": start, "holder": { "owner": "7", "expiresAt": stamp(now - Duration::minutes(1)) } }).to_string();
        assert!(other_holder(&expired, now).is_none());
        assert!(other_holder(&json!({ "appPid": me, "appStart": start, "holder": null }).to_string(), now).is_none());
    }

    #[test]
    fn the_turn_messages_name_the_port_and_no_router() {
        let m = given_message(115, 60);
        assert!(m.contains("card #115") && m.contains("CDP port 9334") && m.contains("60 min"), "{m}");
        for text in [m, ended_message("Hugh released it")] {
            assert!(!text.to_lowercase().contains("router") && !text.contains("wave"), "{text}");
        }
    }

    #[test]
    fn tool_rules() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("b.db")).unwrap();
        let mut p = kanban_store::Patch::new();
        p.insert("title".into(), json!("x"));
        let c = store.create(None, p.clone(), "t").unwrap();
        store.move_by_hugh(&c.id, c.version, "In progress", kanban_store::Patch::new()).unwrap();
        let d = store.create(None, p, "t").unwrap();
        let mut queue = TestAppQueue::default();
        let run = |s: &mut Store, q: &mut TestAppQueue, own: Option<&str>, action: &str, args: Value| act(s, q, own, action, &args, t(0), 60, &Seen::default(), &Live::default());
        // A card in Backlog is told which columns take a turn.
        let e = run(&mut store, &mut queue, Some(&d.id), "take", json!({})).unwrap_err();
        assert!(e.contains("In progress or Human test") && e.contains("Backlog"), "{e}");
        assert!(run(&mut store, &mut queue, Some(&c.id), "take", json!({ "install": true })).is_err());
        let r = run(&mut store, &mut queue, Some(&c.id), "take", json!({})).unwrap();
        assert!(r.contains("your turn"), "{r}");
        assert!(run(&mut store, &mut queue, Some(&c.id), "release", json!({ "force": true })).is_err());
        // Outside: an install turn with an owner; a tester take is refused.
        assert!(run(&mut store, &mut queue, None, "take", json!({})).is_err());
        assert!(run(&mut store, &mut queue, None, "take", json!({ "install": true })).is_err());
        let r = run(&mut store, &mut queue, None, "take", json!({ "install": true, "owner": "router", "label": "wave 3" })).unwrap();
        assert!(r.contains("Number 1 in line"), "{r}");
        let r = run(&mut store, &mut queue, None, "release", json!({ "force": true })).unwrap();
        assert!(r.contains(&format!("#{}", c.number)), "{r}");
        // The forced card hears it, queued.
        assert!(store.waiting(&c.id, "message").unwrap().is_some());
        assert_eq!(run(&mut store, &mut queue, None, "release", json!({ "owner": "router" })).unwrap(), "Took router's install out of the line.");
        assert!(run(&mut store, &mut queue, None, "dance", json!({})).is_err());
    }

    #[test]
    fn status_shows_the_pending_restart_and_the_paused_card() {
        let mut queue = TestAppQueue::default();
        queue.take(Ask::tester("a", 1), t(0), false);
        queue.take(Ask::tester("b", 2), t(0), false);
        queue.take(Ask::install("router", "wave 3"), t(0), false);
        let seen = Seen {
            lock: Some(Lock { owner: "router".into(), label: "wave 3".into(), phase: "waiting".into(), waiting_for: Some("the test app's turn: #1 (tester)".into()), ..Default::default() }),
            hold: Hold { until: Some(t(20)), allow_until: None },
            input: Some(t(-3)),
            dsh: Some(Dsh { owner: "105".into(), age_minutes: 70, dead: false, alive: false }),
            ..Default::default()
        };
        let s = status(&queue, t(46), 60, &seen, &lv(&["a"], &[]));
        assert_eq!(s["holder"]["label"], "#1");
        assert_eq!(s["holder"]["overdue"], true);
        assert_eq!(s["holder"]["expiresAt"], "2026-11-06T04:00:00Z");
        assert_eq!(s["line"][0]["label"], "wave 3");
        assert_eq!(s["line"][1]["live"], false);
        assert_eq!(s["pending"]["label"], "wave 3");
        assert_eq!(s["install"]["phase"], "waiting");
        assert_eq!(s["hold"]["until"], "2026-11-06T03:20:00Z");
        assert_eq!(s["mkdir"]["stale"], true);
        assert_eq!(s["hughInputRecent"], false, "3 min before 03:00 is 49 min before 03:46");
        let quiet = status(&TestAppQueue::default(), t(0), 60, &Seen::default(), &Live::default());
        assert!(quiet["holder"].is_null() && quiet["pending"].is_null());
    }

    #[test]
    fn the_turn_file_names_the_holder_and_the_app() {
        let mut queue = TestAppQueue::default();
        queue.take(Ask::tester("a", 115), t(0), false);
        let f = turn_file(&queue, 60, (42, Some(t(-10))), t(1));
        assert_eq!(f["holder"]["owner"], "115");
        assert_eq!(f["holder"]["kind"], "tester");
        assert_eq!(f["holder"]["expiresAt"], "2026-11-06T04:00:00Z");
        assert_eq!(f["appPid"], 42);
        assert_eq!(f["appStart"], "2026-11-06T02:50:00Z");
        assert!(turn_file(&TestAppQueue::default(), 60, (42, None), t(1))["holder"].is_null());
    }

    #[test]
    fn the_install_scripts_parse_as_powershell() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("scripts");
        for f in ["install.ps1", "test-install-lock.ps1"] {
            let file = root.join(f);
            let cmd = format!("$e=$null;$t=$null;[void][System.Management.Automation.Language.Parser]::ParseFile('{}',[ref]$t,[ref]$e);$e.Count", file.display());
            let Ok(out) = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", &cmd]).output() else {
                return; // no PowerShell here
            };
            assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0", "parse errors in {f}");
        }
    }

    #[test]
    fn holds_are_capped_and_expire() {
        let now = t(0);
        let h = parse_hold(r#"{"until":"2026-11-06T09:00:00Z"}"#, Some(now), now);
        assert_eq!(h.until, Some(t(HOLD_MAX_MINUTES)), "capped at 2 h after it was written");
        assert_eq!(parse_hold(r#"{"until":"2026-11-06T02:00:00Z"}"#, Some(now), now), Hold::default());
        assert!(parse_hold(r#"{"allowUntil":"2026-11-06T03:04:00Z"}"#, Some(now), now).allow_until.is_some());
        assert_eq!(parse_hold("half a fi", Some(now), now), Hold::default());
        assert_eq!(held_until(None, now), t(30));
        assert_eq!(held_until(Some(t(30)), now), t(60));
        assert_eq!(held_until(Some(t(110)), now), t(120));
    }

    #[cfg(windows)]
    #[test]
    fn files_round_trip_and_a_held_lock_is_read_while_held() {
        let dir = tempfile::tempdir().unwrap();
        let me = std::process::id();
        let start = process_start(me).expect("our own start time");
        assert!(alive_since(me, Some(start)));
        assert!(!alive_since(me, Some(start - Duration::minutes(5))), "a reused pid has another start");
        // The holder's way: write sharing off, delete-on-close.
        let path = dir.path().join("install.lock");
        let body = json!({ "pid": me, "pidStart": stamp(start), "owner": "router", "label": "wave 3", "phase": "waiting" });
        {
            use std::io::Write;
            use std::os::windows::fs::OpenOptionsExt;
            // FILE_SHARE_READ | FILE_SHARE_DELETE; FILE_FLAG_DELETE_ON_CLOSE
            let mut f = std::fs::OpenOptions::new().read(true).write(true).create(true).access_mode(0x8000_0000 | 0x4000_0000 | 0x0001_0000).share_mode(1 | 4).custom_flags(0x0400_0000).open(&path).unwrap();
            f.write_all(body.to_string().as_bytes()).unwrap();
            f.flush().unwrap();
            let l = read_lock(dir.path()).expect("read while held");
            assert_eq!(l.owner, "router");
            assert_eq!(l.phase, "waiting");
        }
        assert!(!path.exists(), "deleted on close");
        assert!(read_lock(dir.path()).is_none());
        // A file left by a holder that died (a pid that doesn't match) isn't held.
        std::fs::write(&path, json!({ "pid": me, "pidStart": "2020-01-01T00:00:00Z", "owner": "x" }).to_string()).unwrap();
        assert!(read_lock(dir.path()).is_none());
        // Atomic writes replace a file a reader shares fully.
        let turn = dir.path().join("turn-test.json");
        write_atomic(&turn, &json!({ "holder": null })).unwrap();
        write_atomic(&turn, &json!({ "holder": { "owner": "1" } })).unwrap();
        assert!(read_shared(&turn).unwrap().contains("\"owner\": \"1\""));
        // The old DSH folder: owner and age; released by a rename, never deleted.
        let dsh = dir.path().join(".dsh-test-app.lock");
        std::fs::create_dir(&dsh).unwrap();
        std::fs::write(dsh.join("owner"), "105\n").unwrap();
        let d = read_dsh(&dsh, Utc::now()).unwrap();
        assert_eq!(d.owner, "105");
        assert!(!d.dead && !d.alive, "an old card's folder has no pid line");
        assert!(!d.stale() && d.blocks("1"), "fresh, it blocks");
        let later = Utc::now() + Duration::minutes(DSH_STALE_MINUTES + 1);
        assert!(read_dsh(&dsh, later).unwrap().stale(), "an hour on, it doesn't (card #149)");
        std::fs::write(dsh.join("owner"), "router\npid 4 2020-01-01T00:00:00Z\n").unwrap();
        let d = read_dsh(&dsh, Utc::now()).unwrap();
        assert_eq!(d.owner, "router");
        assert!(d.dead && !d.alive);
        std::fs::write(dsh.join("owner"), format!("router\npid {me} {}\n", stamp(start))).unwrap();
        let d = read_dsh(&dsh, later).unwrap();
        assert!(!d.dead && d.alive);
        assert!(!d.stale(), "its run still runs: never stale, whatever its age");
        let aside = release_dsh(&dsh, Utc::now()).unwrap();
        assert!(!dsh.exists() && aside.join("owner").exists());
        assert!(read_dsh(&dsh, Utc::now()).is_none());
    }
}
