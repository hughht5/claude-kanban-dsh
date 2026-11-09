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
use kanban_store::testapp::{self as q, stamp, Ask, Ended, SlotClaim, SlotLine, Take, TestAppQueue, Turn, INSTALL, TESTER};
use kanban_store::Store;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
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
    /// The test app the install is for (card #147): "test", "test2", ... None from a script
    /// without that change, which may be installing any slot, so it counts for every one.
    pub channel: Option<String>,
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
        channel: s("channel").filter(|c| !c.trim().is_empty()),
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

/// Hugh's last input in test app `slot`: `input-test.json` for Test, `input-test2.json` for
/// Test 2, ... (card #147; each test app writes its own).
pub fn read_input(dir: &Path, slot: u32) -> Option<DateTime<Utc>> {
    let v: Value = serde_json::from_str(&read_shared(&dir.join(input_file(slot)))?).ok()?;
    v.get("at").and_then(Value::as_str).and_then(parse)
}

pub fn input_file(slot: u32) -> String {
    format!("input-{}.json", q::slot_channel(slot))
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

/// One test app's view of the shared folder (`Shared::for_slot`): what its turns go by.
#[derive(Debug, Clone, Default)]
pub struct Seen {
    /// The install holding install.lock (it is quitting, installing or starting the app).
    pub lock: Option<Lock>,
    /// install.lock is there at all. Delete-on-close removes it when its install ends, so a
    /// file that is there but can't be read this instant still counts as an install running.
    pub lock_present: bool,
    /// Installs waiting to restart the test app.
    pub waits: Vec<Lock>,
    pub input: Option<DateTime<Utc>>,
    pub dsh: Option<Dsh>,
    /// Another board's turn on the same test app (its owner), if it holds one now.
    pub other: Option<String>,
}

/// What another running app's board has on one test app (its turn file, card #147).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OtherSlot {
    /// The card that board gave this test app to.
    pub card: Option<i64>,
    /// The owner of that board's live turn on it.
    pub holder: Option<String>,
}

/// Everything read from the shared folder, before the board's mutex, for every test app at once.
#[derive(Debug, Clone, Default)]
pub struct Shared {
    pub lock: Option<Lock>,
    pub lock_present: bool,
    pub waits: Vec<Lock>,
    pub hold: Hold,
    /// Hugh's last input, per test app.
    pub inputs: BTreeMap<u32, DateTime<Utc>>,
    pub dsh: Option<Dsh>,
    /// The other running apps' boards, test app by test app.
    pub others: BTreeMap<u32, Vec<OtherSlot>>,
}

/// An install is for this channel, or names none (a script from before card #147).
fn for_channel(l: &Lock, ch: &str) -> bool {
    l.channel.as_deref().map_or(true, |c| c == ch)
}

impl Shared {
    /// Test app `slot`'s view, for its turns: an install for another test app doesn't count, an
    /// install that names none does.
    pub fn for_slot(&self, slot: u32) -> Seen {
        self.view(slot, false)
    }

    /// The same for the page: an install that names no test app shows on Test only.
    fn shown(&self, slot: u32) -> Seen {
        self.view(slot, true)
    }

    fn view(&self, slot: u32, shown: bool) -> Seen {
        let ch = q::slot_channel(slot);
        let ours = |l: &Lock| if shown && l.channel.is_none() { slot == 1 } else { for_channel(l, &ch) };
        let lock = self.lock.clone().filter(|l| ours(l));
        // A lock that is there but unreadable this instant counts for every test app.
        let lock_present = if self.lock.is_some() { lock.is_some() } else { self.lock_present && (!shown || slot == 1) };
        Seen {
            lock,
            lock_present,
            waits: self.waits.iter().filter(|w| ours(w)).cloned().collect(),
            input: self.inputs.get(&slot).copied(),
            dsh: self.dsh.clone(),
            other: self.others.get(&slot).and_then(|v| v.iter().find_map(|o| o.holder.clone())),
        }
    }

    /// The test apps another running app's board has given to a card or has a turn on: this
    /// board gives none of them out.
    pub fn elsewhere(&self) -> BTreeSet<u32> {
        self.others.iter().filter(|(_, v)| v.iter().any(|o| o.card.is_some() || o.holder.is_some())).map(|(s, _)| *s).collect()
    }
}

pub fn look(dir: &Path, dsh: &Path, now: DateTime<Utc>) -> Shared {
    let lock = read_lock(dir);
    let lock_path = dir.join("install.lock");
    let lock_present = lock.is_some() || crate::platform::file_lock_held(&lock_path).unwrap_or_else(|| lock_path.exists());
    let inputs = (1..=q::MAX_SLOTS).filter_map(|s| read_input(dir, s).map(|at| (s, at))).collect();
    let mine = format!("turn-{}.json", kanban_store::channel());
    let mut others: BTreeMap<u32, Vec<OtherSlot>> = BTreeMap::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with("turn-") || !name.ends_with(".json") || name == mine {
                continue;
            }
            let Some(text) = read_shared(&e.path()) else { continue };
            for (slot, o) in other_slots(&text, now) {
                others.entry(slot).or_default().push(o);
            }
        }
    }
    Shared { lock, lock_present, waits: read_waits(dir), hold: read_hold(dir, now), inputs, dsh: read_dsh(dsh, now), others }
}

/// What another app's turn file says, test app by test app, while that app still runs. An old
/// file (no `slots`: an app from before card #147) has only `holder`: the live app's is on Test,
/// a test app's is on itself.
pub fn other_slots(text: &str, now: DateTime<Utc>) -> Vec<(u32, OtherSlot)> {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return vec![] };
    let Some(pid) = v.get("appPid").and_then(Value::as_u64) else { return vec![] };
    let start = v.get("appStart").and_then(Value::as_str).and_then(parse);
    if !alive_since(pid as u32, start) {
        return vec![];
    }
    let holder = |h: Option<&Value>| -> Option<String> {
        let h = h.filter(|h| !h.is_null())?;
        let expires = h.get("expiresAt").and_then(Value::as_str).and_then(parse)?;
        (expires > now).then(|| h.get("owner").and_then(Value::as_str).unwrap_or("").to_string())
    };
    if let Some(slots) = v.get("slots").and_then(Value::as_object) {
        return slots
            .iter()
            .filter_map(|(ch, e)| Some((q::channel_slot(ch)?, OtherSlot { card: e.get("card").and_then(Value::as_i64), holder: holder(e.get("holder")) })))
            .collect();
    }
    let ch = v.get("channel").and_then(Value::as_str).unwrap_or("");
    let Some(slot) = (if ch == "live" { Some(1) } else { q::channel_slot(ch) }) else { return vec![] };
    holder(v.get("holder")).map(|h| vec![(slot, OtherSlot { card: None, holder: Some(h) })]).unwrap_or_default()
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
    /// Holders with a queued message still waiting for their session (card #168): a turn given
    /// while the session was stopped, its "your turn" not heard yet. Filled in by `run`.
    pub unheard: HashSet<String>,
}

fn live_of(app: &AppHandle) -> Live {
    let s = app.state::<Arc<Sessions>>();
    let states = s.states();
    let mut busy: HashSet<String> = states.iter().filter(|(_, st)| matches!(st.as_str(), "working" | "permission")).map(|(k, _)| k.clone()).collect();
    busy.extend(s.testing_cards());
    Live { live: states.into_keys().collect(), busy, unheard: HashSet::new() }
}

/// What one pump decided, for the logs and messages it sends after.
#[derive(Debug, Default, PartialEq)]
pub struct Step {
    /// (card id, log line).
    pub logs: Vec<(String, String)>,
    /// (card id, message for its session, queued only).
    pub messages: Vec<(String, String)>,
    /// The card whose turn it is but whose session is stopped (card #168): the pump starts it.
    pub wake: Option<String>,
    /// A card given the turn while stopped whose session didn't start in time (card #168): `run`
    /// frees its test app too, so the next card waiting for one gets it.
    pub free: Option<String>,
    pub changed: bool,
}

/// One pass of the rules over the queue: pure apart from `card`, so it is tested here.
pub fn step(
    queue: &mut TestAppQueue,
    now: DateTime<Utc>,
    limit: i64,
    seen: &Seen,
    live: &Live,
    card: impl Fn(&str) -> q::CardState,
    slot: u32,
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
    // A holder woken for its turn whose session didn't come up within the grace: the line
    // moves on (card #168).
    if let Some(t) = queue.wake_ran_out(now, |id| live.live.contains(id)) {
        let why = Ended::NoSession(format!("didn't start within {} min", q::WAKE_GRACE_MINUTES));
        ended(&mut out, &t, &why, &card);
        out.free = t.who.card_id.clone();
    }
    // Its turn came while its session was stopped: the pump starts it (card #168). Before
    // `next`, so the card behind it doesn't take the turn while its session starts. A holder
    // that heard its turn and then stopped (paused when idle) isn't woken: it has nothing new.
    let quiet = queue.holder.as_ref().and_then(|h| h.who.card_id.as_deref()).is_some_and(|id| !live.live.contains(id) && !live.unheard.contains(id));
    out.wake = if quiet { None } else { queue.to_wake(now, |id| live.live.contains(id), |owner| seen.blocks(owner)) };
    if let Some(t) = queue.next(now, |id| live.live.contains(id), |owner| seen.blocks(owner)) {
        if let (Some(id), Some(n)) = (t.who.card_id.clone(), t.who.card) {
            out.logs.push((id.clone(), "Got its turn on the test app".into()));
            out.messages.push((id, given_message(n, limit, slot)));
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

/// One test app's turns, as the page, the tool and kb show them.
pub fn slot_status(queue: &TestAppQueue, now: DateTime<Utc>, limit: i64, seen: &Seen, live: &Live) -> Value {
    let holder = queue.holder.as_ref().map(|h| {
        let mut v = who_json(&h.who);
        let held = parse(&h.given_at).map(|g| (now - g).num_minutes()).unwrap_or(0);
        v["givenAt"] = json!(h.given_at);
        v["renewedAt"] = json!(h.renewed_at);
        v["expiresAt"] = json!(queue.expires_at(limit).map(stamp));
        v["heldMinutes"] = json!(held.max(0));
        v["overdue"] = json!(queue.overdue(now));
        let up = h.who.card_id.as_deref().map_or(true, |id| live.live.contains(id));
        v["live"] = json!(up);
        v["waking"] = json!(!up && h.woke_at.is_some());
        v
    });
    let line: Vec<Value> = queue
        .line
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let mut v = who_json(&w.who);
            v["place"] = json!(i + 1);
            let up = w.who.card_id.as_deref().map_or(true, |id| live.live.contains(id));
            v["live"] = json!(up);
            // The app is starting its session for its turn (card #168).
            v["waking"] = json!(!up && w.waking(now));
            v["askedAt"] = json!(w.asked_at);
            v
        })
        .collect();
    let install = seen.lock.as_ref().or(seen.waits.first()).map(|l| {
        json!({ "owner": l.owner, "label": l.label, "card": l.card, "phase": l.phase, "waitingFor": l.waiting_for,
                "restartAt": l.restart_at, "startedAt": l.started_at, "channel": l.channel })
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
        "holder": holder,
        "line": line,
        "install": install,
        "pending": pending,
        "hughInputAt": seen.input.map(stamp),
        "hughInputRecent": input_recent,
        "otherBoard": seen.other,
    })
}

/// The board's test apps: each one's turns and card, and the cards waiting for one.
#[derive(Debug, Clone, Default)]
pub struct Apps {
    pub queues: BTreeMap<u32, TestAppQueue>,
    /// Test app to (card number, card id, since).
    pub owners: BTreeMap<u32, (i64, String, String)>,
    pub line: SlotLine,
    pub ceiling: u32,
}

impl Apps {
    pub fn read(store: &Store) -> Apps {
        let queues = (1..=q::MAX_SLOTS).map(|s| (s, store.test_app_queue_for(s).unwrap_or_default())).collect();
        let owners = store
            .slot_owners()
            .unwrap_or_default()
            .into_iter()
            .map(|(s, c)| {
                let since = q::test_app_since(&c);
                (s, (c.number, c.id, since))
            })
            .collect();
        Apps { queues, owners, line: store.slot_line().unwrap_or_default(), ceiling: store.test_slots_ceiling().unwrap_or(q::DEFAULT_SLOTS) }
    }

    fn queue(&self, slot: u32) -> TestAppQueue {
        self.queues.get(&slot).cloned().unwrap_or_default()
    }
}

/// The status the page, the tool and kb show: each test app in use, the cards waiting for one,
/// and what holds every restart.
pub fn status(apps: &Apps, now: DateTime<Utc>, limit: i64, shared: &Shared, live: &Live) -> Value {
    let mut slots = vec![];
    let mut pending = Value::Null;
    for s in 1..=q::MAX_SLOTS {
        let queue = apps.queue(s);
        let mut v = slot_status(&queue, now, limit, &shared.shown(s), live);
        let owner = apps.owners.get(&s);
        let used = owner.is_some() || !queue.is_empty() || ["install", "pending", "otherBoard"].iter().any(|k| !v[*k].is_null());
        if !used {
            continue;
        }
        if pending.is_null() && !v["pending"].is_null() {
            pending = json!({ "label": v["pending"]["label"], "slot": s, "slotLabel": q::slot_label(s) });
        }
        v["slot"] = json!(s);
        v["channel"] = json!(q::slot_channel(s));
        v["label"] = json!(q::slot_label(s));
        v["name"] = json!(kanban_store::product_name_for(s));
        v["port"] = json!(kanban_store::devtools_port_for(s));
        v["card"] = owner.map_or(Value::Null, |(n, id, since)| json!({ "number": n, "cardId": id, "label": format!("#{n}"), "since": since }));
        slots.push(v);
    }
    let slot_line: Vec<Value> = apps
        .line
        .line
        .iter()
        .enumerate()
        .map(|(i, w)| json!({ "card": w.card, "cardId": w.card_id, "label": format!("#{}", w.card), "place": i + 1, "askedAt": w.asked_at, "live": live.live.contains(&w.card_id) }))
        .collect();
    json!({
        "channel": kanban_store::channel(),
        "limitMinutes": limit,
        "ceiling": apps.ceiling,
        "slots": slots,
        "slotLine": slot_line,
        "pending": pending,
        "hold": shared.hold.until.map(|u| json!({ "until": stamp(u) })),
        "allowUntil": shared.hold.allow_until.map(stamp),
        "mkdir": shared.dsh.as_ref().map(|d| json!({ "owner": d.owner, "ageMinutes": d.age_minutes, "stale": d.stale(), "dead": d.dead, "alive": d.alive })),
    })
}

/// The turn file install.ps1 reads: who this app is (so a file left by an app that has quit
/// doesn't count), `holder` (Test's turn, as before card #147, for older scripts), and `slots`:
/// each test app this board has given to a card or has a turn on.
pub fn turn_file(apps: &Apps, limit: i64, me: (u32, Option<DateTime<Utc>>), now: DateTime<Utc>) -> Value {
    let holder_of = |queue: &TestAppQueue| {
        queue.holder.as_ref().map(|h| {
            json!({ "kind": h.who.kind, "card": h.who.card, "owner": h.who.owner, "label": h.who.label,
                    "givenAt": h.given_at, "expiresAt": queue.expires_at(limit).map(stamp) })
        })
    };
    let mut slots = serde_json::Map::new();
    for s in 1..=q::MAX_SLOTS {
        let queue = apps.queue(s);
        let owner = apps.owners.get(&s);
        if owner.is_none() && queue.holder.is_none() {
            continue;
        }
        slots.insert(
            q::slot_channel(s),
            json!({ "slot": s, "card": owner.map(|o| o.0), "label": owner.map(|o| format!("#{}", o.0)), "holder": holder_of(&queue) }),
        );
    }
    json!({ "channel": kanban_store::channel(), "appPid": me.0, "appStart": me.1.map(stamp), "writtenAt": stamp(now),
            "holder": holder_of(&apps.queue(1)), "slots": slots })
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

/// One pass of the rules over the store: frees the test apps cards can't keep, steps each test
/// app's turns, and gives free test apps to the cards waiting for one. Logs and queued
/// messages are written here. Returns whether a card changed or was logged on, and the cards
/// (with their test app) whose stopped session the pump starts for their turn (card #168).
pub fn run(store: &mut Store, now: DateTime<Utc>, limit: i64, shared: &Shared, live: &Live) -> (bool, Vec<(String, u32)>) {
    let mut wakes: Vec<(String, u32)> = vec![];
    let mut logs: Vec<(String, String)> = vec![];
    // Which holders still have a message waiting for their session (card #168).
    let mut live = live.clone();
    for slot in 1..=q::MAX_SLOTS {
        if let Some(id) = store.test_app_queue_for(slot).ok().and_then(|q| q.holder).and_then(|h| h.who.card_id) {
            if store.waiting(&id, "message").ok().flatten().is_some() {
                live.unheard.insert(id);
            }
        }
    }
    let live = &live;
    let mut messages: Vec<(String, String)> = vec![];
    let mut changed = false;
    for (c, slot, why) in store.stale_test_apps().unwrap_or_default() {
        if store.free_test_app(&c.id, &why, "the app").is_ok() {
            changed = true;
            // A card still testing hears it (one given the same test app later, a stack's card
            // in Human test); a card that moved on or was archived has nothing to hear.
            if let (Some(s), true) = (slot, q::TESTER_COLUMNS.contains(&c.column.as_str()) && !c.archived) {
                messages.push((c.id.clone(), freed_message(s, &why)));
            }
        }
    }
    for slot in 1..=q::MAX_SLOTS {
        let mut queue = store.test_app_queue_for(slot).unwrap_or_default();
        let st = step(&mut queue, now, limit, &shared.for_slot(slot), live, card_state(store), slot);
        if st.changed {
            let _ = store.save_test_app_queue_for(slot, &queue);
        }
        logs.extend(st.logs);
        messages.extend(st.messages);
        wakes.extend(st.wake.map(|id| (id, slot)));
        if let Some(id) = st.free {
            if let Some(m) = free_unstarted(store, &id) {
                messages.push((id, m));
            }
        }
    }
    if let Ok(ls) = store.step_slot_line(now, kanban_store::slot(), &shared.elsewhere()) {
        for (id, why) in ls.left {
            logs.push((id, format!("Left the line for a test app: {why}")));
        }
        for (id, n, slot) in ls.given {
            changed = true;
            let mut queue = store.test_app_queue_for(slot).unwrap_or_default();
            let seen = shared.for_slot(slot);
            let t = queue.take_now(Ask::tester(&id, n), now, |c| live.live.contains(c), |o| seen.blocks(o));
            let _ = store.save_test_app_queue_for(slot, &queue);
            messages.push((id, take_reply(&t, &format!("#{n}"), limit, Some(n), slot)));
        }
    }
    for (id, line) in &logs {
        let _ = store.add_log(id, line, "the app");
    }
    // Queued only: typed when the session is next free, or heard at its next start.
    for (id, text) in &messages {
        let _ = store.queue(id, "message", text);
    }
    (changed || !logs.is_empty() || !wakes.is_empty(), wakes)
}

/// Card #168: starts the session of a card whose test-app turn came while it was stopped, as
/// the Router's start does (its conversation resumed, else a new one in its worktree), under
/// the running-sessions cap. A conversation only in the desktop app is never started by the
/// app. If it can't start, a held turn ends and a place in line is passed over, logged.
fn wake(app: &AppHandle, id: &str, slot: u32, now: DateTime<Utc>) {
    let board = app.state::<Board>();
    let Ok(mut store) = board.0.lock() else { return };
    let Ok(c) = store.get(id) else { return };
    let on = q::slot_label(slot);
    // Its session may have started since the pump looked (Hugh's Start, the start queue).
    if app.state::<Arc<Sessions>>().is_live(id).is_some() {
        return;
    }
    let line = match wake_start(app, &mut store, &c, slot) {
        Ok(done) => format!("Its turn on {on} came while its session was stopped: {done}"),
        Err(e) => {
            let mut queue = store.test_app_queue_for(slot).unwrap_or_default();
            let ended = queue.wake_failed(id, now);
            let _ = store.save_test_app_queue_for(slot, &queue);
            if ended.is_some() {
                // Heard when its session next runs, as every other end of a turn is.
                let why = Ended::NoSession(format!("couldn't be started ({e})"));
                let _ = store.queue(id, "message", &ended_message(&why.text()));
                if let Some(m) = free_unstarted(&mut store, id) {
                    let _ = store.queue(id, "message", &m);
                }
            }
            let next = if ended.is_some() { "its turn ended and its test app is free again, for the next card waiting for one" } else { "it keeps its place, and the next in line gets the turn" };
            format!("Its turn on {on} came while its session was stopped, and the app couldn't start it ({e}): {next}")
        }
    };
    let _ = store.add_log(id, &line, "the app");
    drop(store);
    let _ = app.emit("cards-changed", ());
}

/// A card whose session couldn't be started for its turn can't use its test app: it is freed
/// (logged by the store), so the next card waiting for a test app gets it (card #168). Returns
/// the message for the card's session, heard when it next runs.
fn free_unstarted(store: &mut Store, id: &str) -> Option<String> {
    let why = "its session was stopped and couldn't be started for its turn";
    store.free_test_app(id, why, "the app").ok().flatten().map(|s| freed_message(s, why))
}

/// What a new session started for its turn hears first (card #168): it has no conversation to
/// carry on, so it is told why it runs and where to start.
pub fn woke_new_message(number: i64, column: &str, slot: u32) -> String {
    format!(
        "The app started this session for card #{number} because its turn on {} came while it had no session running. \
The card is in {column}: read it (kanban_get_card), its log and nextStep, and carry on its column's work from there.",
        q::slot_label(slot)
    )
}

fn wake_start(app: &AppHandle, store: &mut Store, c: &kanban_store::Card, slot: u32) -> Result<String, String> {
    if c.archived || !q::TESTER_COLUMNS.contains(&c.column.as_str()) {
        return Err(format!("the card is in {}", c.column));
    }
    let app_convo = store.latest_session(&c.id).ok().flatten();
    if app_convo.is_none() && c.fields.get("claudeSessionId").is_some() {
        return Err("its conversation is in the Claude desktop app, which the app never starts by itself; open the card and choose Resume in the app".into());
    }
    let mode = if app_convo.is_some() { "resume" } else { "new" };
    let begun = crate::begin_session(app, store, c, mode, "testapp")?;
    if mode == "new" {
        let _ = store.queue(&c.id, "message", &woke_new_message(c.number, &c.column, slot));
    }
    Ok(match begun {
        crate::Begun::Started(_) if mode == "resume" => "the app resumed its session; it gets the turn once the session runs".into(),
        crate::Begun::Started(_) => "the app started a new session; it gets the turn once the session runs".into(),
        crate::Begun::Queued(pos) => format!("the app asked to start its session, which waits under Hugh's cap (number {pos}); if it hasn't started within {} min, the turn moves on to the next in line", q::WAKE_GRACE_MINUTES),
        crate::Begun::Sizing => "the app asked to start its session, which waits for the card to be sized".into(),
    })
}

/// Every 2 s from the dispatch loop, and after each tool call or command.
pub fn pump(app: &AppHandle) {
    let now = Utc::now();
    let shared = look(&shared_dir(), &dsh_dir(), now);
    let live = live_of(app);
    let board = app.state::<Board>();
    let (apps, limit, changed, wakes) = {
        let Ok(mut store) = board.0.lock() else { return };
        let limit = store.test_app_limit().unwrap_or(q::DEFAULT_LIMIT);
        let (changed, wakes) = run(&mut store, now, limit, &shared, &live);
        (Apps::read(&store), limit, changed, wakes)
    };
    for (id, slot) in &wakes {
        wake(app, id, *slot, now);
    }
    publish(app, &apps, limit, &shared, &live, now);
    if changed {
        let _ = app.emit("cards-changed", ());
    }
}

/// Writes the turn file and sends "testapp" when either changed.
fn publish(app: &AppHandle, apps: &Apps, limit: i64, shared: &Shared, live: &Live, now: DateTime<Utc>) {
    static ME: std::sync::OnceLock<(u32, Option<DateTime<Utc>>)> = std::sync::OnceLock::new();
    let me = *ME.get_or_init(|| (std::process::id(), process_start(std::process::id())));
    let turn = turn_file(apps, limit, me, now);
    let status = status(apps, now, limit, shared, live);
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
    if let Some(slots) = scmp.get_mut("slots").and_then(Value::as_array_mut) {
        for s in slots {
            if let Some(h) = s.get_mut("holder").and_then(Value::as_object_mut) {
                h.remove("heldMinutes");
            }
        }
    }
    if last.status.as_ref() != Some(&scmp) {
        last.status = Some(scmp);
        let _ = app.emit("testapp", status);
    }
}

/// The current status, read fresh.
pub fn snapshot(app: &AppHandle) -> Result<Value, String> {
    let now = Utc::now();
    let shared = look(&shared_dir(), &dsh_dir(), now);
    let live = live_of(app);
    let board = app.state::<Board>();
    let store = board.0.lock().map_err(|e| e.to_string())?;
    let limit = store.test_app_limit().unwrap_or(q::DEFAULT_LIMIT);
    Ok(status(&Apps::read(&store), now, limit, &shared, &live))
}

// --- the tool -----------------------------------------------------------------------------

/// The install command for test app `slot`.
pub fn install_command(slot: u32) -> String {
    crate::platform::install_command(&q::slot_channel(slot))
}

/// The given message to a card's session: its own test app and its turn on it.
pub fn given_message(number: i64, limit: i64, slot: u32) -> String {
    let (label, port) = (q::slot_label(slot), kanban_store::devtools_port_for(slot));
    let not_test = if slot == 1 { String::new() } else { " (not -Channel test)".to_string() };
    format!(
        "It is your turn on your test app, {label} ({}), card #{number}. If your card needs its build installed first, run this in the background now: `{}`{not_test}. \
It waits for other installs and for Hugh, and says why; if {label} isn't installed yet, it creates it and copies the live board. \
Then use it for your own checks and your card-tester (CDP port {port}). {label} stays this card's while the card is In progress or Human test, so Hugh tests your build there. \
When you are done with it, release the turn: kanban_test_app with action \"release\" ({label} stays yours). \
The turn ends by itself when this card leaves In progress and Human test, or after {limit} min with nothing running.",
        kanban_store::product_name_for(slot),
        install_command(slot),
    )
}

pub fn ended_message(why: &str) -> String {
    format!("Your turn on the test app ended: {why}. If you still need the test app, take a new turn: kanban_test_app with action \"take\".")
}

/// The message to a card whose test app was taken from it.
pub fn freed_message(slot: u32, why: &str) -> String {
    format!(
        "Your test app, {}, was taken from this card: {why}. Another card's build may go there now. If you need a test app again, take a turn: kanban_test_app with action \"take\".",
        q::slot_label(slot)
    )
}

/// What a take or release told the asker.
fn take_reply(t: &Take, label: &str, limit: i64, number: Option<i64>, slot: u32) -> String {
    let app = q::slot_label(slot);
    match t {
        Take::Given => match number {
            Some(n) => given_message(n, limit, slot),
            None => format!("{app} is yours ({label}): install it now (`{}`). The turn ends when your install has started {app} again, when you release it, or after {limit} min with nothing running.", install_command(slot)),
        },
        Take::Renewed => format!("Your turn on {app} is still yours ({label}); its {limit} min start again."),
        Take::Waiting(p) => match number {
            Some(n) => format!(
                "Your test app is {app} ({}, CDP port {}), card #{n}; it stays this card's while the card is In progress or Human test. You are number {p} in line for its turn. \
The app tells this session when the turn is yours (a message, typed when you are next free). End your turn now; don't wait in a loop.",
                kanban_store::product_name_for(slot),
                kanban_store::devtools_port_for(slot)
            ),
            None => format!("Number {p} in line for {app} (an install goes ahead of every tester, as soon as the current turn ends). Check with kb testapp, or wait with kb testapp take --install --slot {slot} --wait."),
        },
    }
}

/// The reply to a card that waits for a test app: who has each.
fn slot_wait_reply(store: &Store, place: usize) -> String {
    let ceiling = store.test_slots_ceiling().unwrap_or(q::DEFAULT_SLOTS);
    let held: Vec<String> = store.slot_owners().unwrap_or_default().iter().map(|(s, c)| format!("{} #{}", q::slot_label(*s), c.number)).collect();
    let held = if held.is_empty() { String::new() } else { format!(" ({})", held.join(", ")) };
    format!(
        "Every test app up to {} is taken{held}. You are number {place} in line for one. The app tells this session when one is yours \
(a message, typed when you are next free). End your turn now; don't wait in a loop. Hugh can free one on the board, or raise the number of test apps in Settings.",
        q::slot_label(ceiling)
    )
}

/// The test app an outside caller names (`slot`, 1 to 10; Test when left out).
fn arg_slot(args: &Value) -> Result<u32, String> {
    let Some(v) = args.get("slot").filter(|v| !v.is_null()) else { return Ok(1) };
    let n = v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).ok_or("slot is a test app's number, 1 to 10")?;
    u32::try_from(n).ok().filter(|n| (1..=q::MAX_SLOTS).contains(n)).ok_or_else(|| format!("slot is 1 to {}, not {n}", q::MAX_SLOTS))
}

/// kanban_test_app. A card's session takes its own test app and its turn on it, and releases
/// the turn. A session outside the app takes install turns (owner, label, slot), may force a
/// release, and may free a test app.
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
    let shared = look(&shared_dir(), &dsh_dir(), now);
    let live = live_of(app);
    let reply = {
        let board = app.state::<Board>();
        let mut store = board.0.lock().map_err(|e| e.to_string())?;
        let own = crate::tools::caller_card(app, &store, caller);
        let limit = store.test_app_limit().unwrap_or(q::DEFAULT_LIMIT);
        act(&mut store, own.as_deref(), action, args, now, limit, &shared, &live)?
    };
    // Hands a released turn on, writes the turn file and tells the page.
    pump(app);
    let _ = app.emit("cards-changed", ());
    Ok(json!(reply))
}

/// Who an outside session's writes are by.
const OUTSIDE_BY: &str = "a session outside the app";

/// The tool's rules over the store, apart from files and sessions.
/// A take gives the turn now by the pump's own rule (`TestAppQueue::take_now`, card #149):
/// paused cards ahead are passed over, and `shared` blocks as it does in `step`.
#[allow(clippy::too_many_arguments)]
pub fn act(store: &mut Store, own: Option<&str>, action: &str, args: &Value, now: DateTime<Utc>, limit: i64, shared: &Shared, live: &Live) -> Result<String, String> {
    let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
    let install = args.get("install").and_then(Value::as_bool).unwrap_or(false) || args.get("kind").and_then(Value::as_str) == Some(INSTALL);
    let e = |e: kanban_store::Error| e.to_string();
    match (own, action) {
        (Some(id), "take") => {
            if install {
                return Err("a card's session takes a tester turn only; installs are taken from outside a card session (kb testapp take --install), by Hugh's own session".into());
            }
            let c = store.get(id).map_err(e)?;
            if let Some(why) = q::take_refused(&c.column, c.archived) {
                return Err(format!("refused: {why}"));
            }
            let by = format!("card #{}'s session", c.number);
            let slot = match store.claim_test_app(id, now, kanban_store::slot(), &shared.elsewhere()).map_err(|e| format!("refused: {e}"))? {
                SlotClaim::Waiting(p) => {
                    let _ = store.add_log(id, &format!("Waits for a test app: number {p} in line (every one is taken)"), &by);
                    return Ok(slot_wait_reply(store, p));
                }
                SlotClaim::Kept(s) | SlotClaim::Given(s) => s,
            };
            let mut queue = store.test_app_queue_for(slot).map_err(e)?;
            let ask = Ask::tester(id, c.number);
            let seen = shared.for_slot(slot);
            let t = queue.take_now(ask, now, |c| live.live.contains(c), |o| seen.blocks(o));
            // Its own session asked, so it runs: a later stop may be woken again (card #168).
            queue.ran(id);
            store.save_test_app_queue_for(slot, &queue).map_err(e)?;
            let app = q::slot_label(slot);
            let line = match &t {
                Take::Given => format!("Took its turn on {app}"),
                Take::Renewed => format!("Renewed its turn on {app}"),
                Take::Waiting(p) => format!("Waits for its turn on {app}: number {p} in line"),
            };
            let _ = store.add_log(id, &line, &by);
            Ok(take_reply(&t, &format!("#{}", c.number), limit, Some(c.number), slot))
        }
        (Some(id), "release") => {
            if force {
                return Err("a card's session can't force a release; it releases only its own turn".into());
            }
            let c = store.get(id).map_err(e)?;
            let slot = q::card_slot(&c).unwrap_or(1);
            let app = q::slot_label(slot);
            let mut queue = store.test_app_queue_for(slot).map_err(e)?;
            let r = queue.release(Some(id), &c.number.to_string(), TESTER);
            store.save_test_app_queue_for(slot, &queue).map_err(e)?;
            let mut line = store.slot_line().map_err(e)?;
            let left = line.leave(id);
            if left {
                store.save_slot_line(&line).map_err(e)?;
            }
            let keeps = if q::card_slot(&c).is_some() { format!(" {app} stays this card's test app while the card is In progress or Human test.") } else { String::new() };
            Ok(match r {
                Some(true) => {
                    let _ = store.add_log(id, &format!("Released its turn on {app}"), &format!("card #{}'s session", c.number));
                    format!("Released your turn on {app}.{keeps}")
                }
                Some(false) => format!("Left the line for {app}'s turn.{keeps}"),
                None if left => "Left the line for a test app.".into(),
                None => "This card has no turn on a test app and isn't in line.".into(),
            })
        }
        (Some(_), "free") => Err("a card's session can't free a test app: the app frees it when the card moves on, and Hugh can free it on the board".into()),
        (None, "take") => {
            if !install {
                return Err("from outside a card session, take an install turn: kb testapp take --install --owner <name> --label <label> [--slot N]".into());
            }
            let owner = args.get("owner").and_then(Value::as_str).map(str::trim).filter(|o| !o.is_empty()).ok_or("pass the install's owner (the KANBAN_LOCK_OWNER its install.ps1 runs with), for example --owner hugh")?;
            let label = args.get("label").and_then(Value::as_str).unwrap_or("");
            let slot = arg_slot(args)?;
            // A card's test app holds its build for Hugh: no install goes over it from outside.
            if let Some(c) = store.slot_owners().map_err(e)?.get(&slot) {
                return Err(format!("refused: {} holds card #{}'s build (its own test app). Free it first (kb testapp free --slot {slot}, or Free on the board), or use another test app.", q::slot_label(slot), c.number));
            }
            let ask = Ask::install(owner, label);
            let shown = ask.label.clone();
            let mut queue = store.test_app_queue_for(slot).map_err(e)?;
            let seen = shared.for_slot(slot);
            let t = queue.take_now(ask, now, |c| live.live.contains(c), |o| seen.blocks(o));
            store.save_test_app_queue_for(slot, &queue).map_err(e)?;
            Ok(take_reply(&t, &shown, limit, None, slot))
        }
        (None, "release") => {
            let slot = arg_slot(args)?;
            let app = q::slot_label(slot);
            let mut queue = store.test_app_queue_for(slot).map_err(e)?;
            if force {
                let reply = match queue.force() {
                    Some(t) => {
                        if let Some(id) = &t.who.card_id {
                            let _ = store.add_log(id, &format!("Its turn on {app} was released by a session outside the app"), "the app");
                            let _ = store.queue(id, "message", &ended_message("a session outside the app released it"));
                        }
                        format!("Released {}'s turn on {app}.", t.who.label)
                    }
                    None => format!("Nobody holds {app}."),
                };
                store.save_test_app_queue_for(slot, &queue).map_err(e)?;
                return Ok(reply);
            }
            let owner = args.get("owner").and_then(Value::as_str).map(str::trim).filter(|o| !o.is_empty()).unwrap_or("hugh");
            let r = queue.release(None, owner, INSTALL);
            store.save_test_app_queue_for(slot, &queue).map_err(e)?;
            Ok(match r {
                Some(true) => format!("Released {owner}'s install turn on {app}."),
                Some(false) => format!("Took {owner}'s install on {app} out of the line."),
                None => format!("{owner} has no install turn on {app} and isn't in line."),
            })
        }
        (None, "free") => {
            if args.get("slot").map_or(true, Value::is_null) {
                return Err("pass the test app to free: slot (kb testapp free --slot N)".into());
            }
            let slot = arg_slot(args)?;
            free_slot(store, slot, "a session outside the app freed it", OUTSIDE_BY)
        }
        (_, other) => Err(format!("action is take, release, free or status, not {other}")),
    }
}

/// Takes test app `slot` from the card that has it (its turn and place on it too), and tells
/// that card's session.
pub fn free_slot(store: &mut Store, slot: u32, why: &str, by: &str) -> Result<String, String> {
    let app = q::slot_label(slot);
    let owners = store.slot_owners().map_err(|e| e.to_string())?;
    let Some(c) = owners.get(&slot) else { return Ok(format!("{app} isn't any card's test app.")) };
    store.free_test_app(&c.id, why, by).map_err(|e| e.to_string())?;
    let _ = store.queue(&c.id, "message", &freed_message(slot, why));
    Ok(format!("Freed {app}: card #{} no longer has it.", c.number))
}

pub fn schema() -> Value {
    json!({ "name": "kanban_test_app",
            "description": "Test apps and their turns. take: your card's own test app (Test, Test 2, ...: the app gives it one, a new one if every lower one holds another card's build, and it stays the card's while the card is In progress or Human test) and a turn on it. You get the turn now, or a place in line and a message when it is yours; take again to renew it. The reply names the test app, its install command (install.ps1 -Channel test2) and its CDP port. Use it only during your turn, for your own checks and your card-tester. release: give the turn back (the test app stays your card's). status: each test app's card, turn, line and install. The turn ends by itself when your card leaves In progress and Human test, or after a time with nothing running.",
            "inputSchema": { "type": "object", "properties": {
                "action": { "type": "string", "enum": ["take", "release", "free", "status"] },
                "install": { "type": "boolean", "description": "Outside a card session only: an install turn (ahead of every tester)." },
                "owner": { "type": "string", "description": "Outside only: the install's owner, the KANBAN_LOCK_OWNER its install.ps1 runs with (default hugh)." },
                "label": { "type": "string", "description": "Outside only: what the board shows, for example wave 3." },
                "slot": { "type": "integer", "description": "Outside only: the test app, 1 (Test, the default) to 10 (Test 10). free needs it." },
                "force": { "type": "boolean", "description": "Outside only, with release: release whoever holds the turn." } }, "required": ["action"] } })
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

/// Hugh's Release on a test app's row: whoever holds its turn loses it.
#[tauri::command]
pub fn testapp_release(app: AppHandle, slot: Option<u32>) -> Result<Value, String> {
    let slot = slot.unwrap_or(1).clamp(1, q::MAX_SLOTS);
    {
        let board = app.state::<Board>();
        let mut store = board.0.lock().map_err(|e| e.to_string())?;
        let mut queue = store.test_app_queue_for(slot).map_err(|e| e.to_string())?;
        if let Some(t) = queue.force() {
            store.save_test_app_queue_for(slot, &queue).map_err(|e| e.to_string())?;
            if let Some(id) = &t.who.card_id {
                let _ = store.add_log(id, &format!("Hugh released its turn on {}", q::slot_label(slot)), "Hugh");
                let _ = store.queue(id, "message", &ended_message("Hugh released it"));
            }
        }
    }
    let _ = app.emit("cards-changed", ());
    changed(&app)
}

/// Hugh's Free on a test app's row: the card that has it loses it, and its session hears so.
#[tauri::command]
pub fn testapp_free(app: AppHandle, slot: u32) -> Result<Value, String> {
    if !(1..=q::MAX_SLOTS).contains(&slot) {
        return Err(format!("there is no test app {slot}"));
    }
    {
        let board = app.state::<Board>();
        let mut store = board.0.lock().map_err(|e| e.to_string())?;
        free_slot(&mut store, slot, "Hugh freed it", "Hugh")?;
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

/// A test app's page: Hugh used it (trusted input in a focused window). Each test app writes its
/// own file (card #147); the live app records nothing.
#[tauri::command]
pub fn testapp_input() -> Result<(), String> {
    if !kanban_store::is_test() {
        return Ok(());
    }
    write_atomic(&shared_dir().join(input_file(kanban_store::slot())), &json!({ "at": stamp(Utc::now()) }))
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
        Live { live: live.iter().map(|s| s.to_string()).collect(), busy: busy.iter().map(|s| s.to_string()).collect(), unheard: HashSet::new() }
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
        // a moved on to Blocked: its turn ends; b is paused, so the app starts b's session
        // (card #168), and once that hasn't run for WAKE_GRACE_MINUTES c gets it.
        let card2 = cols(&[("a", "Blocked"), ("b", "In progress"), ("c", "In progress")]);
        let s = step(&mut queue, t(1), 60, &Seen::default(), &lv(&["a", "c"], &[]), &card, 1);
        assert!(!s.changed, "Human test keeps the turn");
        let s = step(&mut queue, t(2), 60, &Seen::default(), &lv(&["a", "c"], &[]), &card2, 1);
        assert!(s.changed && queue.holder.is_none());
        assert_eq!(s.wake.as_deref(), Some("b"));
        assert!(s.logs.iter().any(|(id, l)| id == "a" && l.contains("moved to Blocked")));
        let s = step(&mut queue, t(2 + q::WAKE_GRACE_MINUTES), 60, &Seen::default(), &lv(&["a", "c"], &[]), &card2, 1);
        assert!(s.changed);
        assert_eq!(queue.holder.as_ref().unwrap().who.label, "#3");
        assert_eq!(queue.line.len(), 1, "b keeps its place");
        assert!(s.messages.iter().any(|(id, m)| id == "c" && m.contains("your turn")));
        assert!(!s.messages.iter().any(|(id, _)| id == "a"), "a left the testing columns: nothing to hear");
    }

    /// Card #168: a card whose turn comes while its session is stopped is woken once, holds
    /// its turn's place while its session starts, and hears its turn once it runs.
    #[test]
    fn a_stopped_card_whose_turn_comes_is_woken_and_gets_the_turn_once_its_session_runs() {
        let mut queue = TestAppQueue::default();
        queue.take(Ask::tester("a", 1), t(0), false);
        queue.take(Ask::tester("b", 2), t(0), false);
        queue.take(Ask::tester("c", 3), t(0), false);
        let card = cols(&[("a", "In progress"), ("b", "Human test"), ("c", "In progress")]);
        // a holds it and runs: nobody to wake.
        let s = step(&mut queue, t(1), 60, &Seen::default(), &lv(&["a", "c"], &[]), &card, 1);
        assert_eq!(s.wake, None);
        // a releases; b is first and stopped: woken, and c doesn't get the turn.
        queue.force();
        let s = step(&mut queue, t(2), 60, &Seen::default(), &lv(&["c"], &[]), &card, 1);
        assert_eq!(s.wake.as_deref(), Some("b"));
        assert!(s.changed && queue.holder.is_none());
        assert_eq!(slot_status(&queue, t(2), 60, &Seen::default(), &lv(&["c"], &[]))["line"][0]["waking"], true);
        let s = step(&mut queue, t(3), 60, &Seen::default(), &lv(&["c"], &[]), &card, 1);
        assert_eq!(s.wake, None, "woken once");
        assert!(queue.holder.is_none(), "c waits while b's session starts");
        // b's session runs: b gets the turn and its message.
        let s = step(&mut queue, t(4), 60, &Seen::default(), &lv(&["b", "c"], &[]), &card, 1);
        assert_eq!(queue.holder.as_ref().unwrap().who.label, "#2");
        assert!(s.messages.iter().any(|(id, m)| id == "b" && m.contains("your turn")));
        // A holder that heard its turn and then stopped (paused when idle) isn't woken.
        let mut queue = TestAppQueue::default();
        queue.take(Ask::tester("a", 1), t(0), false);
        assert_eq!(step(&mut queue, t(1), 60, &Seen::default(), &Live::default(), &card, 1).wake, None);
        // One given the turn while stopped (the slot line's take: its message waits) is woken, once.
        let unheard = Live { unheard: ["a".to_string()].into_iter().collect(), ..Live::default() };
        let s = step(&mut queue, t(1), 60, &Seen::default(), &unheard, &card, 1);
        assert_eq!(s.wake.as_deref(), Some("a"));
        assert_eq!(slot_status(&queue, t(1), 60, &Seen::default(), &Live::default())["holder"]["waking"], true);
        assert_eq!(step(&mut queue, t(2), 60, &Seen::default(), &unheard, &card, 1).wake, None);
        // Its session didn't come up within the grace: its turn ends, and it hears why.
        let s = step(&mut queue, t(1 + q::WAKE_GRACE_MINUTES), 60, &Seen::default(), &unheard, &card, 1);
        assert!(queue.holder.is_none());
        assert!(s.logs.iter().any(|(id, l)| id == "a" && l.contains("its session was stopped and didn't start within 5 min")), "{:?}", s.logs);
        assert!(s.messages.iter().any(|(id, m)| id == "a" && m.contains("Your turn on the test app ended")));
        // A new session started for its turn is told where to start.
        let m = woke_new_message(168, "Human test", 6);
        assert!(m.contains("card #168") && m.contains("Test 6") && m.contains("Human test") && m.contains("kanban_get_card"), "{m}");
        // A card that left the testing columns is never woken: it leaves the line instead.
        let mut queue = TestAppQueue::default();
        queue.take(Ask::tester("z", 9), t(0), false);
        queue.take(Ask::tester("a", 1), t(0), false);
        queue.force();
        let s = step(&mut queue, t(1), 60, &Seen::default(), &Live::default(), cols(&[("a", "Planning")]), 1);
        assert_eq!(s.wake, None);
        assert!(queue.line.is_empty());
    }

    #[test]
    fn a_busy_holder_or_its_running_tester_keeps_the_turn_past_the_limit() {
        let mut queue = TestAppQueue::default();
        queue.take(Ask::tester("a", 1), t(0), false);
        queue.take(Ask::tester("b", 2), t(0), false);
        let card = cols(&[("a", "In progress"), ("b", "In progress")]);
        step(&mut queue, t(50), 60, &Seen::default(), &lv(&["a", "b"], &["a"]), &card, 1);
        let soon = t(50) + Duration::seconds(30);
        assert!(!step(&mut queue, soon, 60, &Seen::default(), &lv(&["a", "b"], &["a"]), &card, 1).changed, "renewed at most once a minute");
        let s = step(&mut queue, t(100), 60, &Seen::default(), &lv(&["a", "b"], &[]), &card, 1);
        assert!(!s.changed, "renewed at 50, so not out until 110");
        let s = step(&mut queue, t(111), 60, &Seen::default(), &lv(&["a", "b"], &[]), &card, 1);
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
        step(&mut queue, t(1), 60, &Seen::default(), &lv(&["a"], &[]), &card, 1);
        assert_eq!(queue.holder.as_ref().unwrap().who.label, "wave 3", "not seen installing yet: kept");
        step(&mut queue, t(2), 60, &lock, &lv(&["a"], &[]), &card, 1);
        assert!(queue.holder.as_ref().unwrap().install_seen);
        // Someone else's install doesn't count as this one's.
        let other = Seen { lock: Some(Lock { owner: "hugh".into(), ..Default::default() }), lock_present: true, ..Default::default() };
        step(&mut queue, t(3), 60, &other, &lv(&["a"], &[]), &card, 1);
        assert!(queue.holder.is_none(), "router's install is done, and Hugh's install holds the lock: nobody yet");
        let s = step(&mut queue, t(4), 60, &Seen::default(), &lv(&["a"], &[]), &card, 1);
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
        assert!(!step(&mut queue, t(1), 60, &dsh, &lv(&["a", "b"], &[]), &card, 1).changed);
        let mine = Seen { dsh: Some(Dsh { owner: "2".into(), age_minutes: 3, dead: false, alive: false }), ..Default::default() };
        step(&mut queue, t(2), 60, &mine, &lv(&["a", "b"], &[]), &card, 1);
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
        assert!(!step(&mut q, t(1), 60, &hugh, &lv(&["a", "b"], &[]), &card, 1).changed);
        // Present but unreadable this instant: still an install.
        let blur = Seen { lock_present: true, ..Default::default() };
        assert!(!step(&mut q, t(1), 60, &blur, &lv(&["a", "b"], &[]), &card, 1).changed);
        // The live board's turn blocks the test board's queue, except for its own owner.
        let other = Seen { other: Some("2".into()), ..Default::default() };
        step(&mut q, t(2), 60, &other, &lv(&["a", "b"], &[]), &card, 1);
        assert_eq!(q.holder.as_ref().unwrap().who.label, "#2");
        // A killed run's DSH folder (dead pid) blocks nobody.
        let mut q = mk();
        let dead = Seen { dsh: Some(Dsh { owner: "router".into(), age_minutes: 5, dead: true, alive: false }), ..Default::default() };
        step(&mut q, t(3), 60, &dead, &lv(&["a", "b"], &[]), &card, 1);
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
        let s = step(&mut q, t(1), 60, &old, &lv(&["card-131"], &[]), &card, 1);
        assert_eq!(q.holder.as_ref().unwrap().who.label, "#131");
        assert!(s.messages.iter().any(|(id, m)| id == "card-131" && m.contains("your turn")), "its session is told");
        assert!(old.dsh.as_ref().unwrap().stale());
        // Just under the stale age, it still blocks; at it, it doesn't.
        let mut q = mk();
        assert!(!step(&mut q, t(1), 60, &dsh("108", DSH_STALE_MINUTES - 1, false, false), &lv(&["card-131"], &[]), &card, 1).changed);
        step(&mut q, t(2), 60, &dsh("108", DSH_STALE_MINUTES, false, false), &lv(&["card-131"], &[]), &card, 1);
        assert_eq!(q.holder.as_ref().unwrap().who.label, "#131");
    }

    #[test]
    fn a_fresh_dsh_folder_or_one_whose_install_still_runs_still_blocks() {
        let card = cols(&[("card-131", "In progress")]);
        let live = lv(&["card-131"], &[]);
        for (seen, what) in [(dsh("108", 3, false, false), "fresh, no pid line"), (dsh("108", 3410, false, true), "old, but its run still runs")] {
            let mut q = TestAppQueue::default();
            q.take(Ask::tester("card-131", 131), t(0), true);
            assert!(!step(&mut q, t(1), 60, &seen, &live, &card, 1).changed, "{what}");
            assert!(q.holder.is_none(), "{what}");
            assert!(!seen.dsh.as_ref().unwrap().stale(), "{what}");
        }
        // Its own owner isn't blocked by it.
        let mut q = TestAppQueue::default();
        q.take(Ask::tester("card-131", 131), t(0), true);
        step(&mut q, t(1), 60, &dsh("131", 3, false, true), &live, &card, 1);
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
            assert!(!step(&mut q, t(1), 60, &seen, &live, &card, 1).changed);
        }
        // An install turn held in the queue.
        let mut q = TestAppQueue::default();
        q.take(Ask::install("router", "wave 3"), t(0), false);
        q.take(Ask::tester("card-131", 131), t(0), false);
        let installing = Seen { lock: Some(Lock { owner: "router".into(), ..Default::default() }), lock_present: true, ..Default::default() };
        step(&mut q, t(1), 60, &installing, &live, &card, 1);
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
        let shared = |d: Seen| Shared { dsh: d.dsh, ..Default::default() };
        let take = |s: &mut Store, sh: &Shared| act(s, Some(&me), "take", &json!({}), t(1), 60, sh, &live).unwrap();
        // Blocked by a fresh DSH folder: in line. Asked again once it is stale: given.
        let r = take(&mut store, &shared(dsh("108", 3, false, false)));
        assert!(r.contains("number 1 in line"), "{r}");
        let r = take(&mut store, &shared(dsh("108", 3410, false, false)));
        assert!(r.contains("your turn"), "{r}");
        let slot = q::card_slot(&store.get(&me).unwrap()).unwrap();
        assert!(store.test_app_queue_for(slot).unwrap().line.is_empty());
        // Behind a card whose session isn't running, on the same test app.
        let mut queue = store.test_app_queue_for(slot).unwrap();
        queue.release(Some(&me), "", TESTER);
        queue.take(Ask::tester(&paused, 1), t(0), true);
        store.save_test_app_queue_for(slot, &queue).unwrap();
        let r = take(&mut store, &Shared::default());
        assert!(r.contains("your turn"), "{r}");
        assert_eq!(store.test_app_queue_for(slot).unwrap().line.len(), 1, "the paused card keeps its place");
    }

    #[test]
    fn an_install_turn_isn_t_done_while_the_lock_file_is_there() {
        let mut queue = TestAppQueue::default();
        queue.take(Ask::install("router", "wave 3"), t(0), false);
        queue.holder.as_mut().unwrap().install_seen = true;
        let blur = Seen { lock_present: true, ..Default::default() };
        step(&mut queue, t(1), 60, &blur, &Live::default(), cols(&[]), 1);
        assert!(queue.holder.is_some(), "unreadable for a moment is not done");
        let waiting = Seen { waits: vec![Lock { owner: "router".into(), ..Default::default() }], ..Default::default() };
        step(&mut queue, t(2), 60, &waiting, &Live::default(), cols(&[]), 1);
        assert!(queue.holder.is_some(), "its run is waiting to restart");
        step(&mut queue, t(3), 60, &Seen::default(), &Live::default(), cols(&[]), 1);
        assert!(queue.holder.is_none());
    }

    #[test]
    fn other_board_files_count_only_while_their_app_runs_old_and_new() {
        let me = std::process::id();
        let start = process_start(me).map(stamp);
        let now = Utc::now();
        let soon = stamp(now + Duration::minutes(5));
        // An old live file: its holder is on Test.
        let live = json!({ "channel": "live", "appPid": me, "appStart": start, "holder": { "owner": "7", "expiresAt": soon } }).to_string();
        assert_eq!(other_slots(&live, now), vec![(1, OtherSlot { card: None, holder: Some("7".into()) })]);
        // An old test app's file: its holder is on itself.
        let t3 = json!({ "channel": "test3", "appPid": me, "appStart": start, "holder": { "owner": "7", "expiresAt": soon } }).to_string();
        assert_eq!(other_slots(&t3, now)[0].0, 3);
        // A new file: test app by test app, cards too.
        let new = json!({ "channel": "test", "appPid": me, "appStart": start, "holder": null,
            "slots": { "test2": { "slot": 2, "card": 140, "holder": null }, "test4": { "slot": 4, "card": null, "holder": { "owner": "router", "expiresAt": soon } } } }).to_string();
        let got = other_slots(&new, now);
        assert!(got.contains(&(2, OtherSlot { card: Some(140), holder: None })));
        assert!(got.contains(&(4, OtherSlot { card: None, holder: Some("router".into()) })));
        let gone = json!({ "appPid": me, "appStart": "2020-01-01T00:00:00Z", "channel": "live", "holder": { "owner": "7", "expiresAt": soon } }).to_string();
        assert!(other_slots(&gone, now).is_empty());
        let expired = json!({ "channel": "live", "appPid": me, "appStart": start, "holder": { "owner": "7", "expiresAt": stamp(now - Duration::minutes(1)) } }).to_string();
        assert!(other_slots(&expired, now).is_empty());
        // Shared: elsewhere, and a test app's other board.
        let mut shared = Shared::default();
        for (s, o) in got {
            shared.others.entry(s).or_default().push(o);
        }
        assert_eq!(shared.elsewhere(), [2, 4].into_iter().collect());
        assert_eq!(shared.for_slot(4).other.as_deref(), Some("router"));
        assert!(shared.for_slot(2).other.is_none());
    }

    #[test]
    fn an_install_counts_only_for_its_own_test_app_unless_it_names_none() {
        let mut shared = Shared { lock: Some(Lock { owner: "140".into(), channel: Some("test2".into()), ..Default::default() }), lock_present: true, ..Default::default() };
        assert!(shared.for_slot(2).blocks("1"), "Test 2's install holds Test 2");
        assert!(!shared.for_slot(3).blocks("1"), "but not Test 3");
        shared.lock.as_mut().unwrap().channel = None;
        assert!(shared.for_slot(3).blocks("1"), "a lock from an older script holds every test app");
        assert!(shared.shown(1).lock.is_some() && shared.shown(3).lock.is_none(), "and shows on Test only");
        // Unreadable this instant: every test app waits.
        let blur = Shared { lock_present: true, ..Default::default() };
        assert!(blur.for_slot(5).blocks("1"));
        let waits = Shared { waits: vec![Lock { owner: "x".into(), channel: Some("test3".into()), ..Default::default() }], ..Default::default() };
        assert_eq!(waits.for_slot(3).waits.len(), 1);
        assert!(waits.for_slot(2).waits.is_empty());
        let input = Shared { inputs: [(3, t(0))].into_iter().collect(), ..Default::default() };
        assert!(input.for_slot(3).input.is_some() && input.for_slot(2).input.is_none());
        assert_eq!(input_file(1), "input-test.json");
        assert_eq!(input_file(3), "input-test3.json");
    }

    #[test]
    fn the_turn_messages_name_the_test_app_its_install_and_port_and_no_router() {
        let m = given_message(115, 60, 1);
        assert!(m.contains("card #115") && m.contains("CDP port 9334") && m.contains("60 min") && m.contains("-Channel test`"), "{m}");
        let m2 = given_message(147, 60, 4);
        assert!(m2.contains("Test 4") && m2.contains("-Channel test4") && m2.contains("CDP port 9337") && m2.contains("not -Channel test") && m2.contains("creates it"), "{m2}");
        for text in [m, m2, ended_message("Hugh released it"), freed_message(2, "Hugh freed it")] {
            assert!(!text.to_lowercase().contains("router") && !text.contains("wave"), "{text}");
        }
    }

    fn mk_card(store: &mut Store, col: &str) -> kanban_store::Card {
        let mut p = kanban_store::Patch::new();
        p.insert("title".into(), json!("x"));
        let c = store.create(None, p, "t").unwrap();
        if col == "Backlog" {
            c
        } else {
            store.move_by_hugh(&c.id, c.version, col, kanban_store::Patch::new()).unwrap().card
        }
    }

    #[test]
    fn tool_rules() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("b.db")).unwrap();
        let c = mk_card(&mut store, "In progress");
        let d = mk_card(&mut store, "Backlog");
        let e2 = mk_card(&mut store, "In progress");
        let call = |s: &mut Store, own: Option<&str>, action: &str, args: Value| act(s, own, action, &args, t(0), 60, &Shared::default(), &Live::default());
        // A card in Backlog is told which columns take a turn.
        let e = call(&mut store, Some(&d.id), "take", json!({})).unwrap_err();
        assert!(e.contains("In progress or Human test") && e.contains("Backlog"), "{e}");
        assert!(call(&mut store, Some(&c.id), "take", json!({ "install": true })).is_err());
        // The first card gets Test and its turn; the second Test 2 and its turn.
        let r = call(&mut store, Some(&c.id), "take", json!({})).unwrap();
        assert!(r.contains("your turn") && r.contains("Test (") && r.contains("CDP port 9334"), "{r}");
        let r = call(&mut store, Some(&e2.id), "take", json!({})).unwrap();
        assert!(r.contains("Test 2") && r.contains("-Channel test2") && r.contains("CDP port 9335"), "{r}");
        assert_eq!(kanban_store::card_slot(&store.get(&e2.id).unwrap()), Some(2));
        assert!(store.test_app_queue_for(2).unwrap().holds(Some(&e2.id), &e2.number.to_string(), TESTER));
        // Its release gives back the turn, not the test app.
        let r = call(&mut store, Some(&e2.id), "release", json!({})).unwrap();
        assert!(r.contains("Released your turn on Test 2") && r.contains("stays this card's"), "{r}");
        assert_eq!(kanban_store::card_slot(&store.get(&e2.id).unwrap()), Some(2));
        assert!(call(&mut store, Some(&c.id), "release", json!({ "force": true })).is_err());
        assert!(call(&mut store, Some(&c.id), "free", json!({ "slot": 2 })).is_err(), "a card's session frees nothing");
        // Outside: an install turn with an owner, on a test app; a tester take is refused.
        assert!(call(&mut store, None, "take", json!({})).is_err());
        assert!(call(&mut store, None, "take", json!({ "install": true })).is_err());
        // Test holds c's build: no outside install goes over it.
        let r = call(&mut store, None, "take", json!({ "install": true, "owner": "router", "label": "wave 3" })).unwrap_err();
        assert!(r.contains("Test holds card #"), "{r}");
        let r = call(&mut store, None, "take", json!({ "install": true, "owner": "router", "label": "wave 3", "slot": 3 })).unwrap();
        assert!(r.contains("Test 3 is yours") && r.contains("-Channel test3"), "{r}");
        assert!(call(&mut store, None, "take", json!({ "install": true, "owner": "router", "slot": 11 })).is_err());
        let r = call(&mut store, None, "release", json!({ "force": true })).unwrap();
        assert!(r.contains(&format!("#{}", c.number)), "{r}");
        // The forced card hears it, queued.
        assert!(store.waiting(&c.id, "message").unwrap().is_some());
        assert_eq!(call(&mut store, None, "release", json!({ "owner": "router" })).unwrap(), "router has no install turn on Test and isn't in line.");
        assert_eq!(call(&mut store, None, "release", json!({ "owner": "router", "slot": 3 })).unwrap(), "Released router's install turn on Test 3.");
        // Free: names its test app; the card loses it and hears so.
        assert!(call(&mut store, None, "free", json!({})).is_err());
        let r = call(&mut store, None, "free", json!({ "slot": 2 })).unwrap();
        assert_eq!(r, format!("Freed Test 2: card #{} no longer has it.", e2.number));
        assert_eq!(kanban_store::card_slot(&store.get(&e2.id).unwrap()), None);
        assert!(store.waiting(&e2.id, "message").unwrap().unwrap().payload.contains("was taken from this card"));
        assert_eq!(call(&mut store, None, "free", json!({ "slot": 2 })).unwrap(), "Test 2 isn't any card's test app.");
        assert!(call(&mut store, None, "dance", json!({})).is_err());
    }

    #[test]
    fn a_card_waits_for_a_test_app_only_at_the_ceiling_and_hears_when_one_is_given() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("b.db")).unwrap();
        store.set_page_setting("testapp.slots", "1").unwrap();
        let a = mk_card(&mut store, "In progress");
        let b = mk_card(&mut store, "In progress");
        let take = |s: &mut Store, own: &str| act(s, Some(own), "take", &json!({}), t(0), 60, &Shared::default(), &Live::default());
        take(&mut store, &a.id).unwrap();
        let r = take(&mut store, &b.id).unwrap();
        assert!(r.contains("Every test app up to Test is taken") && r.contains(&format!("Test #{}", a.number)) && r.contains("number 1 in line"), "{r}");
        // a goes to Ship it: the pump frees Test and gives it to b, whose session hears it.
        let a2 = store.get(&a.id).unwrap();
        store.move_by_hugh(&a.id, a2.version, "Ship it", kanban_store::Patch::new()).unwrap();
        assert!(run(&mut store, t(1), 60, &Shared::default(), &lv(&[&b.id], &[])).0);
        assert_eq!(kanban_store::card_slot(&store.get(&a.id).unwrap()), None);
        assert_eq!(kanban_store::card_slot(&store.get(&b.id).unwrap()), Some(1));
        assert!(store.test_app_queue_for(1).unwrap().holds(Some(&b.id), &b.number.to_string(), TESTER));
        let m = store.waiting(&b.id, "message").unwrap().unwrap().payload;
        assert!(m.contains("your turn on your test app, Test") && m.contains("CDP port 9334"), "{m}");
    }

    #[test]
    fn turns_on_one_test_app_never_hold_another() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("b.db")).unwrap();
        let a = mk_card(&mut store, "In progress");
        let b = mk_card(&mut store, "In progress");
        // An install into Test 2 runs; a gets Test and b Test 2.
        let shared = Shared { lock: Some(Lock { owner: "x".into(), channel: Some("test2".into()), ..Default::default() }), lock_present: true, ..Default::default() };
        let r = act(&mut store, Some(&a.id), "take", &json!({}), t(0), 60, &shared, &Live::default()).unwrap();
        assert!(r.contains("your turn") && r.contains("Test ("), "Test 2's install doesn't hold a turn on Test: {r}");
        let r = act(&mut store, Some(&b.id), "take", &json!({}), t(0), 60, &shared, &Live::default()).unwrap();
        assert!(r.contains("Test 2") && r.contains("number 1 in line for its turn"), "Test 2's own install holds its turn: {r}");
        assert_eq!(kanban_store::card_slot(&store.get(&b.id).unwrap()), Some(2));
    }

    #[test]
    fn status_shows_each_test_app_in_use_the_line_and_the_pending_restart() {
        let mut apps = Apps { ceiling: 10, ..Default::default() };
        let mut queue = TestAppQueue::default();
        queue.take(Ask::tester("a", 1), t(0), false);
        queue.take(Ask::tester("b", 2), t(0), false);
        queue.take(Ask::install("router", "wave 3"), t(0), false);
        apps.queues.insert(2, queue);
        apps.owners.insert(2, (1, "a".into(), "2026-11-06T03:00:00Z".into()));
        apps.owners.insert(5, (9, "z".into(), "2026-11-06T03:00:00Z".into()));
        apps.line.join("w", 12, t(1));
        let shared = Shared {
            waits: vec![Lock { owner: "router".into(), label: "wave 3".into(), phase: "waiting".into(), waiting_for: Some("the test app's turn: #1 (tester)".into()), channel: Some("test2".into()), ..Default::default() }],
            hold: Hold { until: Some(t(20)), allow_until: None },
            inputs: [(2, t(-3))].into_iter().collect(),
            dsh: Some(Dsh { owner: "105".into(), age_minutes: 70, dead: false, alive: false }),
            ..Default::default()
        };
        let s = status(&apps, t(46), 60, &shared, &lv(&["a"], &[]));
        let slots = s["slots"].as_array().unwrap();
        assert_eq!(slots.iter().map(|v| v["slot"].as_u64().unwrap()).collect::<Vec<_>>(), vec![2, 5], "only the test apps in use");
        let two = &slots[0];
        assert_eq!(two["label"], "Test 2");
        assert_eq!(two["port"], 9335);
        assert_eq!(two["card"]["number"], 1);
        assert_eq!(two["holder"]["label"], "#1");
        assert_eq!(two["holder"]["overdue"], true);
        assert_eq!(two["holder"]["expiresAt"], "2026-11-06T04:00:00Z");
        assert_eq!(two["line"][0]["label"], "wave 3");
        assert_eq!(two["line"][1]["live"], false);
        assert_eq!(two["install"]["phase"], "waiting");
        assert_eq!(two["hughInputRecent"], false, "3 min before 03:00 is 49 min before 03:46");
        assert_eq!(slots[1]["card"]["label"], "#9");
        assert!(slots[1]["holder"].is_null());
        assert_eq!(s["slotLine"][0]["label"], "#12");
        assert_eq!(s["pending"]["slotLabel"], "Test 2");
        assert_eq!(s["pending"]["label"], "wave 3");
        assert_eq!(s["hold"]["until"], "2026-11-06T03:20:00Z");
        assert_eq!(s["mkdir"]["stale"], true);
        assert_eq!(s["ceiling"], 10);
        let quiet = status(&Apps::default(), t(0), 60, &Shared::default(), &Live::default());
        assert!(quiet["slots"].as_array().unwrap().is_empty() && quiet["pending"].is_null());
    }

    #[test]
    fn the_turn_file_names_test_s_holder_and_each_test_app_in_use() {
        let mut apps = Apps::default();
        let mut queue = TestAppQueue::default();
        queue.take(Ask::tester("a", 115), t(0), false);
        apps.queues.insert(1, queue.clone());
        apps.queues.insert(3, queue);
        apps.owners.insert(2, (140, "c".into(), String::new()));
        let f = turn_file(&apps, 60, (42, Some(t(-10))), t(1));
        assert_eq!(f["holder"]["owner"], "115", "holder stays Test's, for older scripts");
        assert_eq!(f["holder"]["kind"], "tester");
        assert_eq!(f["holder"]["expiresAt"], "2026-11-06T04:00:00Z");
        assert_eq!(f["appPid"], 42);
        assert_eq!(f["appStart"], "2026-11-06T02:50:00Z");
        assert_eq!(f["slots"]["test2"]["card"], 140);
        assert!(f["slots"]["test2"]["holder"].is_null());
        assert_eq!(f["slots"]["test3"]["holder"]["owner"], "115");
        assert!(f["slots"]["test3"]["card"].is_null());
        assert!(f["slots"].get("test4").is_none());
        let empty = turn_file(&Apps::default(), 60, (42, None), t(1));
        assert!(empty["holder"].is_null() && empty["slots"].as_object().unwrap().is_empty());
    }

    #[test]
    fn no_outside_install_goes_over_a_card_s_test_app() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("b.db")).unwrap();
        let a = mk_card(&mut store, "In progress");
        act(&mut store, Some(&a.id), "take", &json!({}), t(0), 60, &Shared::default(), &Live::default()).unwrap();
        let e = act(&mut store, None, "take", &json!({ "install": true, "owner": "router", "slot": 1 }), t(0), 60, &Shared::default(), &Live::default()).unwrap_err();
        assert!(e.contains("Test holds card #") && e.contains("free --slot 1"), "{e}");
        assert!(act(&mut store, None, "take", &json!({ "install": true, "owner": "router", "slot": 2 }), t(0), 60, &Shared::default(), &Live::default()).is_ok());
    }

    #[test]
    fn a_card_still_testing_hears_when_the_pump_takes_its_test_app() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("b.db")).unwrap();
        let a = mk_card(&mut store, "In progress");
        let b = mk_card(&mut store, "In progress");
        store.give_test_app(&a.id, 2, t(0)).unwrap();
        store.give_test_app(&b.id, 2, t(1)).unwrap();
        run(&mut store, t(2), 60, &Shared::default(), &Live::default());
        assert_eq!(kanban_store::card_slot(&store.get(&b.id).unwrap()), None);
        let m = store.waiting(&b.id, "message").unwrap().unwrap().payload;
        assert!(m.contains("Test 2, was taken from this card"), "{m}");
        // A card that moved on hears nothing.
        let c = store.get(&a.id).unwrap();
        store.move_by_hugh(&a.id, c.version, "Ship it", kanban_store::Patch::new()).unwrap();
        run(&mut store, t(3), 60, &Shared::default(), &Live::default());
        assert!(store.waiting(&a.id, "message").unwrap().is_none());
    }

    /// Card #168: a card given a test app and its turn while stopped (the slot line's take) is
    /// woken; if its session hasn't started within the grace, its turn ends and its test app is
    /// freed, so the next card waiting for one gets it.
    #[test]
    fn a_stopped_card_whose_start_never_comes_frees_its_test_app_for_the_next() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().join("b.db")).unwrap();
        let a = mk_card(&mut store, "In progress");
        store.give_test_app(&a.id, 2, t(0)).unwrap();
        let mut queue = TestAppQueue::default();
        queue.take(Ask::tester(&a.id, a.number), t(0), false);
        store.save_test_app_queue_for(2, &queue).unwrap();
        store.queue(&a.id, "message", &given_message(a.number, 60, 2)).unwrap();
        // Stopped, its turn unheard: woken once.
        let (_, wakes) = run(&mut store, t(1), 60, &Shared::default(), &Live::default());
        assert_eq!(wakes, vec![(a.id.clone(), 2)]);
        assert_eq!(run(&mut store, t(2), 60, &Shared::default(), &Live::default()).1, vec![]);
        assert_eq!(kanban_store::card_slot(&store.get(&a.id).unwrap()), Some(2), "kept while its session starts");
        // Not up within the grace: the turn ends and Test 2 is free again.
        run(&mut store, t(1 + q::WAKE_GRACE_MINUTES), 60, &Shared::default(), &Live::default());
        assert!(store.test_app_queue_for(2).unwrap().holder.is_none());
        assert_eq!(kanban_store::card_slot(&store.get(&a.id).unwrap()), None);
        let log: Vec<String> = store.log(&a.id).unwrap().iter().map(|l| l.text.clone()).collect();
        assert!(log.iter().any(|l| l.contains("didn't start within 5 min")), "{log:?}");
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
