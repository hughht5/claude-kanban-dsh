//! The session tracker (card #117, item 1). Each claude the app starts on a PTY is a session
//! leader, and everything it starts keeps its session ID unless it calls setsid itself, even
//! after `nohup ... &` hands it to launchd. The app samples the user's processes every 5 s and
//! records each member of its claude sessions with its start time, so `kb reap` can end them
//! when the app's process ends, cleanly or in a crash.
//!
//! The record rules are pure and tested on every system; the sampler thread is macOS only.

use serde::{Deserialize, Serialize};

/// One process: its pid and its start time (microseconds since the epoch), so a pid number
/// that the system gives to another process later never matches.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Proc {
    pub pid: i32,
    pub start: u64,
}

/// The tracker's record, written to `<data>/run/pids-<app pid>.json`.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Record {
    pub app_pid: i32,
    /// The claude session leaders started this run.
    pub sessions: Vec<Proc>,
    /// Every recorded member of those sessions (the leaders too) that is still alive.
    pub procs: Vec<Proc>,
}

impl Record {
    /// A claude the app started. It is a session leader, so its session ID is its pid.
    pub fn add_session(&mut self, leader: Proc) {
        if !self.sessions.contains(&leader) {
            self.sessions.push(leader);
        }
        if !self.procs.contains(&leader) {
            self.procs.push(leader);
        }
    }

    /// One sample: (process, its session ID) for every process of the user, now.
    ///
    /// - A leader is "taken" when its pid number now belongs to another process (same pid, a
    ///   different start). A taken leader is dropped, so its session ID takes no new members.
    /// - An entry of `procs` that isn't in the sample (same pid and start) is dead and goes.
    /// - A sampled process whose session ID is a leader's pid is added.
    ///
    /// An empty sample means the listing failed (it always holds the caller), so it changes
    /// nothing.
    pub fn observe(&mut self, seen: &[(Proc, i32)]) {
        if seen.is_empty() {
            return;
        }
        self.sessions.retain(|l| !seen.iter().any(|(p, _)| p.pid == l.pid && p.start != l.start));
        self.procs.retain(|p| seen.iter().any(|(s, _)| s == p));
        for (p, sid) in seen {
            if self.sessions.iter().any(|l| l.pid == *sid) && !self.procs.contains(p) {
                self.procs.push(*p);
            }
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_json(s: &str) -> Option<Record> {
        serde_json::from_str(s).ok()
    }
}

/// The sampler: a thread that, every 5 s, lists the user's processes, applies them to the
/// record and writes the file (0600) when its JSON changed. It runs for the app's life.
#[cfg(target_os = "macos")]
pub fn start_sampler(file: std::path::PathBuf, record: std::sync::Arc<std::sync::Mutex<Record>>) {
    let _ = std::thread::Builder::new().name("session-tracker".into()).spawn(move || {
        let mut written = String::new();
        loop {
            let seen = crate::sys::all_procs();
            let json = {
                let mut r = record.lock().unwrap_or_else(|e| e.into_inner());
                r.observe(&seen);
                r.to_json()
            };
            if json != written && crate::sys::write_private(&file, json.as_bytes()).is_ok() {
                written = json;
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: i32, start: u64) -> Proc {
        Proc { pid, start }
    }

    #[test]
    fn members_join_by_session_id() {
        let mut r = Record { app_pid: 1, ..Default::default() };
        r.add_session(p(100, 10));
        r.add_session(p(100, 10));
        assert_eq!(r.sessions, vec![p(100, 10)]);
        assert_eq!(r.procs, vec![p(100, 10)], "the leader is in procs");
        r.observe(&[(p(1, 1), 1), (p(100, 10), 100), (p(101, 11), 100), (p(200, 20), 200)]);
        assert_eq!(r.procs, vec![p(100, 10), p(101, 11)], "another session isn't recorded");
        // The leader exits; its handed-off child keeps the session ID and stays recorded.
        r.observe(&[(p(1, 1), 1), (p(101, 11), 100), (p(102, 12), 100)]);
        assert_eq!(r.sessions, vec![p(100, 10)], "a dead leader stays (cheap)");
        assert_eq!(r.procs, vec![p(101, 11), p(102, 12)], "the dead leader leaves procs, a new member joins");
    }

    #[test]
    fn dead_entries_go_by_pid_and_start() {
        let mut r = Record::default();
        r.add_session(p(100, 10));
        r.observe(&[(p(100, 10), 100), (p(101, 11), 100)]);
        // pid 101 is now a different process (another start) in another session.
        r.observe(&[(p(100, 10), 100), (p(101, 99), 300)]);
        assert_eq!(r.procs, vec![p(100, 10)]);
    }

    #[test]
    fn a_taken_leader_takes_no_new_members() {
        let mut r = Record::default();
        r.add_session(p(100, 10));
        r.add_session(p(500, 50));
        r.observe(&[(p(100, 10), 100), (p(500, 50), 500)]);
        // Leader 100 died and its pid number went to an unrelated process (start 77).
        r.observe(&[(p(100, 77), 7), (p(103, 13), 100), (p(500, 50), 500)]);
        assert_eq!(r.sessions, vec![p(500, 50)], "the taken leader is dropped");
        assert_eq!(r.procs, vec![p(500, 50)], "nothing joins through a taken leader, and the stranger isn't recorded");
    }

    #[test]
    fn an_empty_sample_changes_nothing() {
        let mut r = Record::default();
        r.add_session(p(100, 10));
        let before = r.clone();
        r.observe(&[]);
        assert_eq!(r, before);
    }

    #[test]
    fn json_round_trip() {
        let mut r = Record { app_pid: 4242, ..Default::default() };
        r.add_session(p(100, 1_762_400_000_000_000));
        r.observe(&[(p(100, 1_762_400_000_000_000), 100), (p(101, 1_762_400_000_000_001), 100)]);
        let j = r.to_json();
        assert!(j.contains("\"appPid\":4242"));
        assert_eq!(Record::from_json(&j), Some(r));
        assert_eq!(Record::from_json("{}"), Some(Record::default()));
        assert_eq!(Record::from_json("nope"), None);
    }
}
