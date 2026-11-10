//! Toasts for what needs Hugh: a permission prompt, a finished turn in Planning or
//! In progress, `kanban_notify`, a ship result, a crash and a ship request from outside.
//! None for the card whose panel is open in a focused window. Clicking one brings the
//! window forward and opens that card's panel (the page handles `open-card`).
//!
//! The toast itself is shown by the platform layer (platform/windows.rs: with
//! `tauri-winrt-notification`, because the notification plugin drops a desktop toast's click).

use crate::Board;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{App, AppHandle, Listener, Manager, State};

/// The card whose panel is open, as the page last said, and each card's last activity state.
#[derive(Default)]
pub struct Notices {
    panel: Mutex<Option<String>>,
    states: Mutex<HashMap<String, String>>,
    /// The Router panel is open (card #119).
    router: Mutex<bool>,
}

/// The page calls this when a card's panel opens (with its id) and closes (with null). Both
/// count as Hugh reading the card: the one opened, and the one closed (he saw it until then).
#[tauri::command]
pub fn panel_opened(app: AppHandle, notices: State<Notices>, card_id: Option<String>) {
    let before = match notices.panel.lock() {
        Ok(mut p) => std::mem::replace(&mut *p, card_id.clone()),
        Err(_) => return,
    };
    for id in [before, card_id].into_iter().flatten() {
        crate::needs::mark_read(&app, &id);
    }
}

pub fn setup(app: &App) {
    app.manage(Notices::default());
    on(app, "activity", |app, p| {
        let card = s(p, "card");
        let state = s(p, "state");
        let notices = app.state::<Notices>();
        let prev = notices.states.lock().ok().and_then(|mut m| m.insert(card.clone(), state.clone()));
        // The Router (card #119, final-test F7): its own words, and the question itself.
        if card == crate::routersession::KEY {
            if state == "permission" && prev.as_deref() != Some("permission") {
                let q = crate::routersession::open_question(app).unwrap_or_else(|| "a question or a permission prompt: open the Router panel".into());
                toast(app, &card, "The Router asks you", &q);
            }
            return;
        }
        // A merge lane (card #160) asks nothing (AskUserQuestion is denied it) and is no card:
        // what Hugh needs to know goes on its job's card.
        if crate::sessions::is_app_key(&card) {
            return;
        }
        let (number, title, column) = lookup(app, &card);
        if state == "permission" && prev.as_deref() != Some("permission") {
            // A question dialog says its question (card #119, re-test 2 F7); a real permission
            // prompt keeps its heading.
            let wait = app.state::<std::sync::Arc<crate::sessions::Sessions>>().waits().remove(&card);
            let (head, body) = permission_toast(number, &title, wait.as_ref());
            toast(app, &card, &head, &body);
        } else if state == "idle" && prev.as_deref() == Some("working") && (column == "Planning" || column == "In progress") {
            toast(app, &card, &format!("#{number} finished its turn"), &title);
        }
    });
    on(app, "notify", |app, p| {
        let card = s(p, "card");
        if card == crate::routersession::KEY {
            toast(app, &card, "Router", &s(p, "text"));
            return;
        }
        if let Some(repo) = kanban_store::lane_repo(&card) {
            toast(app, &card, &format!("Merge lane ({repo})"), &s(p, "text"));
            return;
        }
        let (number, title, _) = lookup(app, &card);
        toast(app, &card, &format!("#{}: {}", n(p, number), s(p, "text")), &title);
    });
    on(app, "ship-result", |app, p| {
        let card = s(p, "card");
        let (number, title, _) = lookup(app, &card);
        let number = n(p, number);
        let detail = s(p, "detail");
        let head = match s(p, "state").as_str() {
            "shipped" if detail.is_empty() => format!("#{number} shipped"),
            "shipped" => format!("#{number} shipped ({detail})"),
            "failed" => match ship_retries(app, &card) {
                Some(r) if r < 3 => format!("#{number}: ship failed (retry {r} of 3)"),
                _ => format!("#{number}: ship needs you"),
            },
            "needs-hugh" => format!("#{number}: ship needs you"),
            _ => return,
        };
        toast(app, &card, &head, &title);
    });
    // A change to the small-change repo list (card #104). No card: a click shows the board.
    on(app, "small-repos", |app, p| {
        let repos: Vec<String> = p.get("repos").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
        let body = if repos.is_empty() { "Off: every card stops in Human test for you.".to_string() } else { format!("Small, fully checked cards in {} now go on to Ship it with no human test.", repos.join(", ")) };
        toast(app, "", "Small-change auto-ship changed", &body);
    });
    on(app, "session-crashed", |app, p| {
        let card = s(p, "card");
        let (number, title, _) = lookup(app, &card);
        toast(app, &card, &format!("#{}'s session crashed", n(p, number)), &title);
    });
    on(app, "ship-request", |app, p| {
        let card = s(p, "card");
        let (number, title, _) = lookup(app, &card);
        let title = Some(s(p, "title")).filter(|t| !t.is_empty()).unwrap_or(title);
        // Card #184: who asked, and that only his click in the app ships it.
        let by = Some(s(p, "by")).filter(|t| !t.is_empty()).unwrap_or_else(|| "an agent".into());
        toast(app, &card, &format!("Ship #{}?", n(p, number)), &format!("{title}. Asked by {by}: click Ship on the board within 10 minutes."));
    });
}

/// Listens for `event` and passes its JSON payload on, off the emitting thread.
fn on(app: &App, event: &str, f: fn(&AppHandle, &Value)) {
    let a = app.handle().clone();
    listen_off_thread(app, event, move |payload| {
        if let Ok(p) = serde_json::from_str::<Value>(payload) {
            f(&a, &p);
        }
    });
}

type Job = Box<dyn FnOnce() + Send>;

/// Tauri runs a Rust listener on the thread that emits, and that thread may hold the board
/// lock (a ship result, `kanban_notify`, a restart resume) or be the main thread. The
/// handlers here take the board lock and wait on the main thread (the tray tooltip, window
/// focus), which deadlocked the app. So they run on one worker thread, in the order emitted.
pub fn listen_off_thread(app: &App, event: &str, f: impl Fn(&str) + Send + Sync + 'static) {
    static WORKER: std::sync::OnceLock<Mutex<std::sync::mpsc::Sender<Job>>> = std::sync::OnceLock::new();
    let tx = WORKER
        .get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<Job>();
            std::thread::spawn(move || {
                for job in rx {
                    job();
                }
            });
            Mutex::new(tx)
        })
        .lock()
        .map(|t| t.clone());
    let Ok(tx) = tx else { return };
    let f = std::sync::Arc::new(f);
    app.listen(event, move |e| {
        let (f, payload) = (f.clone(), e.payload().to_string());
        let _ = tx.send(Box::new(move || f(&payload)));
    });
}

fn s(p: &Value, k: &str) -> String {
    p.get(k).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// The payload's card number, else the store's.
fn n(p: &Value, fallback: i64) -> i64 {
    p.get("number").and_then(Value::as_i64).unwrap_or(fallback)
}

/// The card's number, title and column, from the store.
fn lookup(app: &AppHandle, card_id: &str) -> (i64, String, String) {
    let board = app.state::<Board>();
    let Ok(store) = board.0.lock() else { return (0, String::new(), String::new()) };
    match store.get(card_id) {
        Ok(c) => (c.number, c.title().to_string(), c.column.clone()),
        Err(_) => (0, String::new(), String::new()),
    }
}

fn ship_retries(app: &AppHandle, card_id: &str) -> Option<i64> {
    let board = app.state::<Board>();
    let store = board.0.lock().ok()?;
    let card = store.get(card_id).ok()?;
    card.fields.get("ship")?.get("retries")?.as_i64()
}

/// The card whose panel is open in the page now, focus or not (card #119, final-test F2): the
/// Router leaves that card alone while Hugh has it open, even with a game in front.
pub(crate) fn panel_open(app: &AppHandle) -> Option<String> {
    app.try_state::<Notices>().and_then(|n| n.panel.lock().ok().and_then(|p| p.clone()))
}

/// Whether `open` (the open panel's card id) is `card_id`.
pub fn panel_holds(open: Option<&str>, card_id: &str) -> bool {
    open == Some(card_id)
}

/// The Router panel is open in the page (card #119, F7): its toasts wait, as a card's do.
#[tauri::command]
pub fn router_panel_opened(notices: State<Notices>, open: bool) {
    if let Ok(mut r) = notices.router.lock() {
        *r = open;
    }
}

/// Whether the main window has focus, as its last `Focused` event said (card #122).
static FOCUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Keeps `FOCUSED` current; called from the app's window-event handler.
pub fn on_window_event(w: &tauri::Window, e: &tauri::WindowEvent) {
    if let tauri::WindowEvent::Focused(on) = e {
        if w.label() == "main" {
            FOCUSED.store(*on, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// Is Hugh looking at this card's panel in a focused window?
///
/// Card #122: this never asks the window (`is_focused()`), because that waits for the main
/// thread, and callers hold the board lock (the dispatcher, the Router's tools). The main
/// thread's commands wait for the board, so the two waited for each other and the app hung.
pub(crate) fn looking_at(app: &AppHandle, card_id: &str) -> bool {
    let notices = app.state::<Notices>();
    // The Router panel is its own (card #119): a card panel open beside it doesn't count for it.
    let open = if card_id == crate::routersession::KEY {
        notices.router.lock().ok().filter(|r| **r).map(|_| card_id.to_string())
    } else {
        notices.panel.lock().ok().and_then(|p| p.clone())
    };
    // Card #122: never ask the window here (it waits for the main thread under the board lock).
    FOCUSED.load(std::sync::atomic::Ordering::Relaxed) && open.as_deref() == Some(card_id)
}

/// The heading and body of a card's "needs you" toast: a question dialog's first question
/// ("#n asks you"), else a permission prompt with what it asks to run, else the card's title.
pub fn permission_toast(number: i64, title: &str, wait: Option<&crate::sessions::Waiting>) -> (String, String) {
    if let Some(a) = wait.and_then(|w| w.questions.as_ref()).and_then(crate::needs::question_ask) {
        let body = if title.is_empty() { a.text } else { format!("{}\n{title}", a.text) };
        return (format!("#{number} asks you"), body);
    }
    let what = wait.and_then(|w| w.prompt.as_deref()).map(str::trim).filter(|p| !p.is_empty());
    let body = match what {
        Some(p) if !title.is_empty() => format!("{}\n{title}", crate::needs::shown(p)),
        Some(p) => crate::needs::shown(p),
        None => title.to_string(),
    };
    (format!("#{number} needs you: permission prompt"), body)
}

fn toast(app: &AppHandle, card_id: &str, head: &str, body: &str) {
    if looking_at(app, card_id) {
        return;
    }
    // The test build says so on every toast, so its toasts are never taken for the live app's.
    let head = if kanban_store::is_test() { format!("({}) {head}", kanban_store::label_for(kanban_store::slot())) } else { head.to_string() };
    crate::platform::toast(app, card_id, &head, body);
}

#[cfg(test)]
mod tests {
    /// Card #122: a window getter (`is_focused()`, `outer_size()`, ...) waits for the main
    /// thread. Called holding a lock from another thread, it hung the app. Only the window-event
    /// handler and start-up code (on the main thread: place.rs, platform/macos.rs) may call one.
    #[test]
    fn no_window_getter_off_the_main_thread() {
        const GETTERS: [&str; 10] = [
            ".is_focused()", ".is_visible()", ".is_minimized()", ".is_maximized()", ".is_fullscreen()",
            ".outer_position()", ".outer_size()", ".inner_size()", ".inner_position()", ".scale_factor()",
        ];
        const MAIN_THREAD_ONLY: [&str; 2] = ["place.rs", "macos.rs"];
        let mut found = vec![];
        let mut dirs = vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(d) = dirs.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    dirs.push(p);
                    continue;
                }
                let name = p.file_name().unwrap().to_string_lossy().to_string();
                if !name.ends_with(".rs") || MAIN_THREAD_ONLY.contains(&name.as_str()) {
                    continue;
                }
                let text = std::fs::read_to_string(&p).unwrap();
                for (n, line) in text.lines().enumerate() {
                    if line.trim_start().starts_with("//") || line.contains("\".") {
                        // a comment, or the GETTERS list above
                        continue;
                    }
                    if GETTERS.iter().any(|g| line.contains(g)) {
                        found.push(format!("{name}:{}", n + 1));
                    }
                }
            }
        }
        assert!(found.is_empty(), "window getters off the main thread: {found:?}");
    }

    /// Never take Hugh's focus (AGENTS.md; card #104's notifications go through here): a toast
    /// shows the window only when Hugh clicks it, and nothing here focuses a window.
    #[test]
    fn a_toast_never_takes_focus_unless_clicked() {
        let src = include_str!("notify.rs");
        let here: String = src.split("#[cfg(test)]").next().unwrap().to_string();
        assert!(!here.contains("set_focus") && !here.contains("SetForegroundWindow"), "a toast must not focus a window");
        assert!(here.contains("crate::platform::toast("), "toasts go through the platform module (card #114)");
        // The toast itself is in the platform module (card #114): its `toast` function.
        let win = include_str!("platform/windows.rs");
        let start = win.find("pub fn toast(").unwrap();
        let code = &win[start..start + win[start..].find("\n}").unwrap()];
        assert!(!code.contains("set_focus") && !code.contains("SetForegroundWindow"), "a toast must not focus a window");
        let shows = code.matches("show_main(").count();
        let clicks = code.matches(".on_activated(").count();
        assert!(shows >= 1 && shows == clicks, "show_main only in a click handler: {shows} vs {clicks}");
        let at = code.find(".on_activated(").unwrap();
        assert!(code[at..].find("show_main(").unwrap() < 200, "show_main sits inside on_activated");
        // The small-change notices use these toasts.
        assert!(here.contains("\"small-repos\""));
    }

    /// Re-test 2 F7: a card's question toast says its question (a real AskUserQuestion input),
    /// not the card's title alone or "permission prompt".
    #[test]
    fn a_cards_question_toast_says_the_question() {
        let input = serde_json::json!({ "questions": [{ "header": "Tea", "question": "R3: Should the scratch note be about green tea or black tea?", "multiSelect": false, "options": [{ "label": "Green tea" }, { "label": "Black tea" }] }] });
        let w = crate::sessions::Waiting { questions: crate::transcript::ask_questions(&input), prompt: None, failure: None };
        let (head, body) = super::permission_toast(194, "R3-test: ask a question (dry run)", Some(&w));
        assert_eq!(head, "#194 asks you");
        assert!(body.starts_with("R3: Should the scratch note be about green tea or black tea?"), "{body}");
        // A real permission prompt keeps its heading and says what it asks to run.
        let p = crate::sessions::Waiting { questions: None, prompt: Some("Bash: cargo test".into()), failure: None };
        let (head, body) = super::permission_toast(7, "A card", Some(&p));
        assert_eq!(head, "#7 needs you: permission prompt");
        assert!(body.starts_with("Bash: cargo test") && body.ends_with("A card"), "{body}");
        // Nothing known: the title, as before.
        assert_eq!(super::permission_toast(7, "A card", None).1, "A card");
    }
}
