//! Windows toasts for what needs Hugh: a permission prompt, a finished turn in Planning or
//! In progress, `kanban_notify`, a ship result, a crash and a ship request from outside.
//! None for the card whose panel is open in a focused window. Clicking one brings the
//! window forward and opens that card's panel (the page handles `open-card`).
//!
//! The notification plugin is registered, but its desktop side drops the toast's click, so
//! the toasts are shown here with `tauri-winrt-notification` (the library the plugin uses).

use crate::Board;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{App, AppHandle, Emitter, Listener, Manager, State};
use tauri_winrt_notification::Toast;

/// The card whose panel is open, as the page last said, and each card's last activity state.
#[derive(Default)]
pub struct Notices {
    panel: Mutex<Option<String>>,
    states: Mutex<HashMap<String, String>>,
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
        let (number, title, column) = lookup(app, &card);
        if state == "permission" && prev.as_deref() != Some("permission") {
            toast(app, &card, &format!("#{number} needs you: permission prompt"), &title);
        } else if state == "idle" && prev.as_deref() == Some("working") && (column == "Planning" || column == "In progress") {
            toast(app, &card, &format!("#{number} finished its turn"), &title);
        }
    });
    on(app, "notify", |app, p| {
        let card = s(p, "card");
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
    on(app, "session-crashed", |app, p| {
        let card = s(p, "card");
        let (number, title, _) = lookup(app, &card);
        toast(app, &card, &format!("#{}'s session crashed", n(p, number)), &title);
    });
    on(app, "ship-request", |app, p| {
        let card = s(p, "card");
        let (number, title, _) = lookup(app, &card);
        let title = Some(s(p, "title")).filter(|t| !t.is_empty()).unwrap_or(title);
        toast(app, &card, &format!("Ship #{}?", n(p, number)), &title);
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

/// Is Hugh looking at this card's panel in a focused window?
pub(crate) fn looking_at(app: &AppHandle, card_id: &str) -> bool {
    let open = app.state::<Notices>().panel.lock().ok().and_then(|p| p.clone());
    let focused = app.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
    focused && open.as_deref() == Some(card_id)
}

/// The toast's app id: the installed app's own (its Start menu shortcut carries it), or
/// PowerShell's for a build run from `target`, the same rule the notification plugin uses.
fn app_id(app: &AppHandle) -> String {
    let dir = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.display().to_string())).unwrap_or_default();
    let sep = std::path::MAIN_SEPARATOR;
    if dir.ends_with(&format!("{sep}target{sep}debug")) || dir.ends_with(&format!("{sep}target{sep}release")) {
        Toast::POWERSHELL_APP_ID.to_string()
    } else {
        app.config().identifier.clone()
    }
}

fn toast(app: &AppHandle, card_id: &str, head: &str, body: &str) {
    if looking_at(app, card_id) {
        return;
    }
    let id = app_id(app);
    // The test build says so on every toast, so its toasts are never taken for the live app's.
    let head = if kanban_store::is_test() { format!("(test) {head}") } else { head.to_string() };
    let (a, card, body) = (app.clone(), card_id.to_string(), body.to_string());
    std::thread::spawn(move || {
        let clicked = a.clone();
        let result = Toast::new(&id)
            .title(&head)
            .text1(&body)
            .on_activated(move |_| {
                crate::tray::show_main(&clicked);
                let _ = clicked.emit("open-card", card.clone());
                Ok(())
            })
            .show();
        if let Err(e) = result {
            eprintln!("toast: {e}");
        }
    });
}
