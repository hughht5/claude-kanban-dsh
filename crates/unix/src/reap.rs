//! `kb reap <app pid> <record file>` (card #117 item 1): started by the app in its own session.
//! When the app's process ends, cleanly or in a crash, it ends every process the tracker
//! recorded whose start time still matches (TERM, then KILL after 3 s) and removes the record.

use crate::sys;
use crate::track::Record;
use std::path::Path;
use std::time::{Duration, Instant};

/// Wait for the app to end, then end its sessions' processes. Always returns 0: a missing or
/// unreadable record means there is nothing to end.
pub fn run(app_pid: i32, file: &Path) -> i32 {
    // SAFETY: setting a signal to be ignored has no preconditions. A quit that signals the
    // app's group or terminal must not stop the reaper before it has done its work.
    unsafe {
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
        libc::signal(libc::SIGINT, libc::SIG_IGN);
        libc::signal(libc::SIGTERM, libc::SIG_IGN);
    }
    sys::watch_exit(app_pid);
    let Some(mut record) = std::fs::read_to_string(file).ok().and_then(|s| Record::from_json(&s)) else {
        return 0;
    };
    // One last sample: members started since the tracker's last one (up to 5 s ago) join too.
    record.observe(&sys::all_procs());
    let me = std::process::id() as i32;
    let targets: Vec<_> = record.procs.iter().copied().filter(|p| p.pid != me && p.pid != app_pid && p.pid > 1 && sys::alive(p)).collect();
    for p in &targets {
        // SAFETY: kill only sends a signal; the start time was checked just before.
        unsafe { libc::kill(p.pid, libc::SIGTERM) };
    }
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until && targets.iter().any(sys::alive) {
        std::thread::sleep(Duration::from_millis(100));
    }
    for p in targets.iter().filter(|p| sys::alive(p)) {
        // SAFETY: as above.
        unsafe { libc::kill(p.pid, libc::SIGKILL) };
    }
    let _ = std::fs::remove_file(file);
    0
}
