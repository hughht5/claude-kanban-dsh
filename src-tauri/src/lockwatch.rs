//! A mutex that knows who holds it (card #122).
//!
//! `Watched<T>` wraps `std::sync::Mutex<T>`. Each `lock()` records its call site (via
//! `#[track_caller]`), its thread and the time. A watchdog thread looks at every watched lock
//! once a second and writes to `locks.log` in the data folder (and stderr):
//!
//! - a hold longer than `HOLD_REPORT` while it is still held, with the holder's call site and
//!   the call sites of the threads waiting for it, again every `REPEAT` while it lasts;
//! - a lock order cycle (thread A holds X and waits for Y, thread B holds Y and waits for X):
//!   "deadlock", with every site in the cycle;
//! - when a guard is dropped after more than `HOLD_REPORT`, how long it was held, and where.
//!
//! The bookkeeping is a leaf lock held only for a few instructions, never across user code.

use std::collections::HashMap;
use std::panic::Location;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, TryLockError};
use std::time::{Duration, Instant};

/// A hold over this long is logged with its call site.
pub const HOLD_REPORT: Duration = Duration::from_secs(5);
/// While it lasts, a long hold is logged again this often.
const REPEAT: Duration = Duration::from_secs(30);

type Site = &'static Location<'static>;

#[derive(Clone)]
struct Hold {
    at: Site,
    since: Instant,
    thread: u64,
    thread_name: String,
    /// When the watchdog last wrote about this hold.
    reported: Option<Instant>,
}

#[derive(Clone)]
struct Wait {
    at: Site,
    since: Instant,
    thread: u64,
    thread_name: String,
}

#[derive(Default)]
struct Slot {
    holder: Option<Hold>,
    waiters: Vec<Wait>,
}

struct Entry {
    name: &'static str,
    slot: Arc<Mutex<Slot>>,
}

fn registry() -> &'static Mutex<Vec<Entry>> {
    static R: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(Vec::new()))
}

fn log_path() -> &'static Mutex<Option<PathBuf>> {
    static P: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(None))
}

/// A small number for this thread, stable for its life (std's ThreadId has no stable integer).
fn thread_no() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    thread_local!(static NO: u64 = NEXT.fetch_add(1, Ordering::Relaxed));
    NO.with(|n| *n)
}

fn thread_label() -> String {
    std::thread::current().name().unwrap_or("unnamed").to_string()
}

fn site(s: Site) -> String {
    format!("{}:{}", s.file(), s.line())
}

fn slot_lock(slot: &Mutex<Slot>) -> MutexGuard<'_, Slot> {
    slot.lock().unwrap_or_else(|e| e.into_inner())
}

/// Writes one line to `locks.log` (when `start` has named the folder) and stderr.
pub fn log(line: &str) {
    let stamped = format!("{} {line}", chrono_like_now());
    eprintln!("lockwatch: {stamped}");
    let path = log_path().lock().ok().and_then(|p| p.clone());
    if let Some(path) = path {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{stamped}");
        }
    }
}

fn chrono_like_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Names the folder for `locks.log` and starts the watchdog (once).
pub fn start(dir: PathBuf) {
    if let Ok(mut p) = log_path().lock() {
        *p = Some(dir.join("locks.log"));
    }
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        let _ = std::thread::Builder::new().name("lockwatch".into()).spawn(|| loop {
            std::thread::sleep(Duration::from_secs(1));
            for line in scan(Instant::now()) {
                log(&line);
            }
        });
    });
}

/// One look at every watched lock: the lines to log now.
fn scan(now: Instant) -> Vec<String> {
    scan_as(now, false)
}

/// `every`: report every long hold, whether or not reported already, and mark none.
fn scan_as(now: Instant, every: bool) -> Vec<String> {
    let mut out = Vec::new();
    let reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    // name, holder thread, waiters' threads, for the cycle check.
    let mut held_by: HashMap<usize, (u64, Site)> = HashMap::new();
    let mut waits_for: HashMap<u64, (usize, Site)> = HashMap::new();
    for (i, e) in reg.iter().enumerate() {
        let mut s = slot_lock(&e.slot);
        for w in &s.waiters {
            waits_for.insert(w.thread, (i, w.at));
        }
        let waiters: Vec<String> =
            s.waiters.iter().map(|w| format!("{} (thread {} {}, {}s)", site(w.at), w.thread, w.thread_name, now.duration_since(w.since).as_secs())).collect();
        if let Some(h) = s.holder.as_mut() {
            held_by.insert(i, (h.thread, h.at));
            let held = now.duration_since(h.since);
            let due = match h.reported {
                _ if every => held >= HOLD_REPORT,
                None => held >= HOLD_REPORT,
                Some(t) => now.duration_since(t) >= REPEAT,
            };
            if due {
                if !every {
                    h.reported = Some(now);
                }
                out.push(format!(
                    "{} held {}s by {} (thread {} {}); waiting: {}",
                    e.name,
                    held.as_secs(),
                    site(h.at),
                    h.thread,
                    h.thread_name,
                    if waiters.is_empty() { "none".to_string() } else { waiters.join(", ") }
                ));
            }
        }
    }
    // A cycle: lock -> its holder thread -> the lock that thread waits for -> ...
    let mut seen_cycles = std::collections::HashSet::new();
    for &start in held_by.keys() {
        let mut path = vec![start];
        let mut at = start;
        while let Some((thread, _)) = held_by.get(&at) {
            let Some((next, _)) = waits_for.get(thread) else { break };
            if *next == start {
                let mut key = path.clone();
                key.sort();
                if seen_cycles.insert(key) {
                    let parts: Vec<String> = path
                        .iter()
                        .map(|l| {
                            let (t, h) = held_by[l];
                            let w = waits_for.get(&t).map(|(n, s)| format!("waits for {} at {}", reg[*n].name, site(s))).unwrap_or_default();
                            format!("{} held by thread {} at {}, which {}", reg[*l].name, t, site(h), w)
                        })
                        .collect();
                    out.push(format!("DEADLOCK: {}", parts.join("; ")));
                }
                break;
            }
            if path.contains(next) || path.len() > reg.len() {
                break;
            }
            path.push(*next);
            at = *next;
        }
    }
    out
}

/// `scan`, for tests elsewhere in the crate.
#[cfg(test)]
pub fn scan_for_test(now: Instant) -> Vec<String> {
    scan_as(now, true)
}

/// A lock that was poisoned by a panic while held.
#[derive(Debug)]
pub struct Poisoned(&'static str);

impl std::fmt::Display for Poisoned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the {} lock was poisoned", self.0)
    }
}

impl std::error::Error for Poisoned {}

/// A `Mutex<T>` that records who holds it, for the watchdog.
pub struct Watched<T> {
    name: &'static str,
    inner: Mutex<T>,
    slot: Arc<Mutex<Slot>>,
}

impl<T> Watched<T> {
    pub fn new(name: &'static str, value: T) -> Self {
        let slot = Arc::new(Mutex::new(Slot::default()));
        if let Ok(mut r) = registry().lock() {
            r.push(Entry { name, slot: slot.clone() });
        }
        Watched { name, inner: Mutex::new(value), slot }
    }

    /// Like `Mutex::lock`, recording the caller.
    #[track_caller]
    pub fn lock(&self) -> Result<WatchedGuard<'_, T>, Poisoned> {
        let at = Location::caller();
        let thread = thread_no();
        let guard = match self.inner.try_lock() {
            Ok(g) => g,
            Err(TryLockError::Poisoned(_)) => return Err(Poisoned(self.name)),
            Err(TryLockError::WouldBlock) => {
                slot_lock(&self.slot).waiters.push(Wait { at, since: Instant::now(), thread, thread_name: thread_label() });
                let got = self.inner.lock();
                slot_lock(&self.slot).waiters.retain(|w| w.thread != thread);
                got.map_err(|_| Poisoned(self.name))?
            }
        };
        slot_lock(&self.slot).holder =
            Some(Hold { at, since: Instant::now(), thread, thread_name: thread_label(), reported: None });
        Ok(WatchedGuard { guard: Some(guard), owner: self })
    }
}

impl<T> Drop for Watched<T> {
    fn drop(&mut self) {
        if let Ok(mut r) = registry().lock() {
            r.retain(|e| !Arc::ptr_eq(&e.slot, &self.slot));
        }
    }
}

/// The guard `Watched::lock` gives; derefs to `T`.
pub struct WatchedGuard<'a, T> {
    guard: Option<MutexGuard<'a, T>>,
    owner: &'a Watched<T>,
}

impl<T> std::ops::Deref for WatchedGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.guard.as_ref().expect("guard present until drop")
    }
}

impl<T> std::ops::DerefMut for WatchedGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.guard.as_mut().expect("guard present until drop")
    }
}

impl<T> Drop for WatchedGuard<'_, T> {
    fn drop(&mut self) {
        let hold = slot_lock(&self.owner.slot).holder.take();
        drop(self.guard.take());
        if let Some(h) = hold {
            let held = h.since.elapsed();
            if held >= HOLD_REPORT {
                log(&format!("{} was held {:.1}s by {} (thread {} {})", self.owner.name, held.as_secs_f64(), site(h.at), h.thread, h.thread_name));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    #[test]
    fn records_the_holder_and_clears_it() {
        let w = Watched::new("t1", 5);
        {
            let g = w.lock().unwrap();
            assert_eq!(*g, 5);
            let s = slot_lock(&w.slot);
            let h = s.holder.as_ref().unwrap();
            assert!(h.at.file().ends_with("lockwatch.rs"));
        }
        assert!(slot_lock(&w.slot).holder.is_none());
    }

    #[test]
    fn a_long_hold_is_reported_with_its_site_and_waiters() {
        let w = Arc::new(Watched::new("t2", ()));
        let g = w.lock().unwrap();
        let w2 = w.clone();
        let t = std::thread::spawn(move || {
            let _g = w2.lock().unwrap();
        });
        while slot_lock(&w.slot).waiters.is_empty() {
            std::thread::yield_now();
        }
        let lines = scan(Instant::now() + HOLD_REPORT + Duration::from_millis(10));
        let line = lines.iter().find(|l| l.starts_with("t2 held")).expect("reported");
        assert!(line.contains("lockwatch.rs:"), "{line}");
        assert!(!line.contains("waiting: none"), "{line}");
        // Reported once, then again only after REPEAT.
        let again = scan(Instant::now() + HOLD_REPORT + Duration::from_secs(1));
        assert!(!again.iter().any(|l| l.starts_with("t2 held")));
        drop(g);
        t.join().unwrap();
    }

    #[test]
    fn a_lock_order_cycle_is_reported_as_a_deadlock() {
        let a = Arc::new(Watched::new("cycA", ()));
        let b = Arc::new(Watched::new("cycB", ()));
        let bar = Arc::new(Barrier::new(2));
        // Thread 1 holds A and waits for B; this thread holds B and is seen to "wait" for A by
        // a thread that blocks on it. To keep the test from really deadlocking, thread 2 waits
        // on A while thread 1 (holding A) waits on B held here; we release B afterwards.
        let gb = b.lock().unwrap();
        let (a1, b1, bar1) = (a.clone(), b.clone(), bar.clone());
        let t1 = std::thread::spawn(move || {
            let _ga = a1.lock().unwrap();
            bar1.wait();
            let _gb = b1.lock().unwrap();
        });
        bar.wait();
        // Pretend this thread waits for A: register a waiter by locking A from a helper that
        // carries this thread's number is not possible, so mark it directly.
        slot_lock(&a.slot).waiters.push(Wait { at: Location::caller(), since: Instant::now(), thread: thread_no(), thread_name: thread_label() });
        while slot_lock(&b.slot).waiters.is_empty() {
            std::thread::yield_now();
        }
        let lines = scan(Instant::now());
        assert!(lines.iter().any(|l| l.starts_with("DEADLOCK") && l.contains("cycA") && l.contains("cycB")), "{lines:?}");
        slot_lock(&a.slot).waiters.clear();
        drop(gb);
        t1.join().unwrap();
    }
}
