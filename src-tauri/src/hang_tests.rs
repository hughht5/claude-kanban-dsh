//! Card #122: a stress model of the test-app hang, with the app's own lock type.
//!
//! The real hang: the dispatcher (every 400 ms) held the board lock and called
//! `notify::looking_at`, which asked the window `is_focused()`. That waits for the main thread.
//! The main thread was running a sync command (`needs_you`, `list_cards`, ...) that waited for
//! the board. Neither went on; every API request then queued on the board for ever.
//!
//! Here a thread stands in for the main thread (it answers window getters and runs commands that
//! lock the board), six threads poll the board as API requests do, a thread pushes UI events,
//! and a dispatcher holds the board while it checks focus: the old way (a round trip to the main
//! thread) hangs, the new way (an atomic, as `notify::looking_at` now does) never does.

use crate::lockwatch::{self, Watched};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

enum Msg {
    /// A window getter: the main thread answers it.
    Getter(mpsc::Sender<bool>),
    /// A sync command: the main thread locks the board.
    Command,
}

/// Runs the load for up to `run`, or until the main thread has run `enough` commands. Returns
/// whether the dispatcher's focus check ever waited past `stuck` (the hang), and how many
/// commands the main thread ran.
fn load(name: &'static str, old_way: bool, run: Duration, stuck: Duration, enough: u64) -> (bool, u64, Arc<Watched<u64>>) {
    let board = Arc::new(Watched::new(name, 0u64));
    let stop = Arc::new(AtomicBool::new(false));
    let commands = Arc::new(AtomicU64::new(0));
    let focused = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<Msg>();

    // The main thread.
    let main = {
        let (board, stop, commands) = (board.clone(), stop.clone(), commands.clone());
        std::thread::Builder::new()
            .name("main".into())
            .spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match rx.recv_timeout(Duration::from_millis(20)) {
                        Ok(Msg::Getter(reply)) => {
                            let _ = reply.send(false);
                        }
                        Ok(Msg::Command) => {
                            let mut b = board.lock().unwrap();
                            *b += 1;
                            commands.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(_) => {}
                    }
                }
            })
            .unwrap()
    };
    // UI events, each making the page call a command.
    let ui = {
        let (tx, stop) = (tx.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let _ = tx.send(Msg::Command);
                std::thread::sleep(Duration::from_millis(1));
            }
        })
    };
    // Six sessions' hooks and the poller's tools.
    let api: Vec<_> = (0..6)
        .map(|_| {
            let (board, stop) = (board.clone(), stop.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if let Ok(mut b) = board.lock() {
                        *b += 1;
                    }
                    std::thread::sleep(Duration::from_micros(300));
                }
            })
        })
        .collect();
    // The dispatcher: holds the board while it asks whether Hugh is looking.
    let hung = Arc::new(AtomicBool::new(false));
    let report = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let dispatcher = {
        let (board, stop, hung, focused, tx, report) = (board.clone(), stop.clone(), hung.clone(), focused.clone(), tx.clone(), report.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                {
                    let _store = board.lock().unwrap();
                    let _looking = if old_way {
                        let (reply_tx, reply_rx) = mpsc::channel();
                        let _ = tx.send(Msg::Getter(reply_tx));
                        match reply_rx.recv_timeout(stuck) {
                            Ok(f) => f,
                            Err(_) => {
                                // The real one waits for ever; the test stops here, after
                                // the watchdog's view: the board held here, the main thread waiting.
                                *report.lock().unwrap() = lockwatch::scan_for_test(Instant::now() + lockwatch::HOLD_REPORT);
                                hung.store(true, Ordering::Relaxed);
                                stop.store(true, Ordering::Relaxed);
                                false
                            }
                        }
                    } else {
                        focused.load(Ordering::Relaxed)
                    };
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        })
    };
    let until = Instant::now() + run;
    while Instant::now() < until && !stop.load(Ordering::Relaxed) && commands.load(Ordering::Relaxed) < enough {
        std::thread::sleep(Duration::from_millis(10));
    }
    stop.store(true, Ordering::Relaxed);
    dispatcher.join().unwrap();
    main.join().unwrap();
    ui.join().unwrap();
    for t in api {
        t.join().unwrap();
    }
    if hung.load(Ordering::Relaxed) {
        let report = report.lock().unwrap();
        let line = report.iter().find(|l| l.starts_with(name)).unwrap_or_else(|| panic!("{report:?}"));
        assert!(line.contains("hang_tests.rs") && line.contains(" main, "), "{line}");
    }
    (hung.load(Ordering::Relaxed), commands.load(Ordering::Relaxed), board)
}

#[test]
fn the_old_focus_check_under_the_board_lock_hangs() {
    let (hung, _, _) = load("hang-old", true, Duration::from_secs(10), Duration::from_millis(500), u64::MAX);
    assert!(hung, "the old way should hang under this load");
}

/// The proof is that the main thread keeps getting the board, not how fast: on a busy machine
/// (other cards' builds beside `cargo test`) it once ran 12 commands in 2 s (card #171). So the
/// load runs until the main thread has run 100 commands, with a minute's room; a hang stops it at
/// the first one, however long it is given.
#[test]
fn the_focus_flag_under_the_board_lock_never_hangs() {
    let (hung, commands, board) = load("hang-new", false, Duration::from_secs(60), Duration::from_millis(500), 101);
    assert!(!hung);
    assert!(commands > 100, "the main thread kept running commands: {commands} in a minute");
    assert!(*board.lock().unwrap() > commands);
}
