//! The macOS side of the platform layer (card #117), with the names `windows.rs` has. The
//! logic that needs no Tauri or AppKit (the process record, the reaper, the login-shell PATH,
//! the update helper's script, the install lock) is in the `kanban-unix` crate, which can be
//! checked for macOS on any machine; this file is the glue to Tauri and AppKit.
//!
//! Nobody has run this on a real Mac yet: CI on GitHub's Mac runners is the proof, and
//! ACCEPTANCE.md lists what only a real Mac can show.

use crate::place::Rect;
use kanban_unix::{env, sys, text, track};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Listener, Manager, RunEvent, Runtime, WebviewWindow, Window};

/// Account 2 (`CLAUDE_CONFIG_DIR=~/.claude-max2`) isn't supported on a Mac in this version:
/// Claude Code keeps its sign-in in the Keychain there, and nobody has checked that a second
/// config folder gets its own entry (router decision R2: such a card refuses to start).
pub const SECOND_ACCOUNT: bool = false;

/// Dev stacks need WSL, so none on a Mac in this version (router decision D6); a `/`
/// checkout path is an ordinary local folder here.
pub const DEV_STACKS: bool = false;

/// On a Mac a click on the menu-bar icon opens its menu, as Mac menu-bar icons do.
pub const TRAY_LEFT_CLICK_SHOWS: bool = false;

// --- Start-up (main.rs's first line) -------------------------------------------------------

/// Before anything else starts, and before any thread: the open-files limit, PATH and LANG
/// from the login shell (a Mac app started from Finder gets launchd's bare ones), private data
/// folders, and an App Nap activity so a hidden window doesn't slow the hooks and timers.
pub fn at_start() {
    let imported = env::import_login_env();
    let data = kanban_store::data_dir();
    let _ = sys::private_dir(&data);
    if let Some(home) = kanban_store::home_var() {
        let _ = sys::private_dir(&home.join(".claude-kanban-dsh-shared"));
    }
    let mut line = format!(
        "{} login shell: {}; PATH: {}; LANG: {}; open files: {} -> {}",
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        imported.shell,
        imported.path,
        std::env::var("LANG").unwrap_or_default(),
        imported.nofile.0,
        imported.nofile.1
    );
    if let Some(note) = &imported.note {
        line.push_str(&format!(" ({note})"));
    }
    line.push('\n');
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(data.join("macos.log")) {
        let _ = f.write_all(line.as_bytes());
    }
    hold_off_app_nap();
}

/// App Nap slows a hidden app's timers and I/O. The activity is held for the app's life.
fn hold_off_app_nap() {
    use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
    let info = NSProcessInfo::processInfo();
    let activity = info.beginActivityWithOptions_reason(
        NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
        &NSString::from_str("Card sessions run while the window is hidden"),
    );
    std::mem::forget(activity);
}

/// The builder's macOS settings. A background start must not activate the app: tao activates
/// it at launch unless told not to, and neither `open -g` nor `orderBack` stops that. And the
/// app menu, with Quit replaced so that Cmd+Q asks first when sessions run (`ask_quit`).
pub fn builder<R: Runtime>(b: tauri::Builder<R>) -> tauri::Builder<R> {
    let background = std::env::args().any(|a| a == "--background");
    b.activate_ignoring_other_apps(!background).menu(app_menu)
}

const QUIT_ASK: &str = "quit-ask";

/// The standard Mac menus (Edit gives Cmd+C, V, X, A and Z in text boxes), Quit our own.
fn app_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let name = app.package_info().name.clone();
    let quit = MenuItem::with_id(app, QUIT_ASK, format!("Quit {name}"), true, Some("CmdOrCtrl+Q"))?;
    let app_menu = Submenu::with_items(
        app,
        &name,
        true,
        &[
            &PredefinedMenuItem::about(app, None, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let view = Submenu::with_items(app, "View", true, &[&PredefinedMenuItem::fullscreen(app, None)?])?;
    let window = Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;
    Menu::with_items(app, &[&app_menu, &edit, &view, &window])
}

// --- Run events ----------------------------------------------------------------------------

pub fn on_run_event(app: &AppHandle, event: &RunEvent) {
    match event {
        RunEvent::Ready => listen_for_quit_answers(app),
        // A Dock click with the window hidden brings it back.
        RunEvent::Reopen { has_visible_windows, .. } if !*has_visible_windows => crate::tray::show_main(app),
        RunEvent::MenuEvent(e) if e.id.as_ref() == QUIT_ASK => ask_quit(app),
        _ => {}
    }
}

// --- Cmd+Q (router decision D4; critic finding 12) ------------------------------------------

/// When the page was last asked, and whether it said it showed the question.
static ASKED: Mutex<Option<Instant>> = Mutex::new(None);
static SHOWN: AtomicBool = AtomicBool::new(false);
const PAGE_WAIT: Duration = Duration::from_millis(3000);

/// Cmd+Q: with no session running it quits at once. Otherwise the page asks "Quit and suspend
/// N sessions?" (ui/quit.js); if the page hasn't said within 3 s that it showed the question
/// (hung, or still loading), a native alert asks the same.
fn ask_quit(app: &AppHandle) {
    let running = app.state::<Arc<crate::sessions::Sessions>>().running_count();
    match text::quit_step(running, None, false) {
        text::QuitStep::Quit => crate::tray::quit_app(app),
        _ => {
            let at = Instant::now();
            if let Ok(mut a) = ASKED.lock() {
                *a = Some(at);
            }
            SHOWN.store(false, Ordering::SeqCst);
            crate::tray::show_main(app);
            let _ = app.emit("confirm-quit", serde_json::json!({ "running": running }));
            let a = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(PAGE_WAIT);
                let still = ASKED.lock().ok().and_then(|g| *g) == Some(at);
                let ago = at.elapsed().as_millis() as u64;
                if still && text::quit_step(running, Some(ago), SHOWN.load(Ordering::SeqCst)) == text::QuitStep::NativeAlert {
                    let b = a.clone();
                    let _ = a.run_on_main_thread(move || native_quit_alert(&b, running));
                }
            });
        }
    }
}

fn listen_for_quit_answers(app: &AppHandle) {
    app.listen("quit-shown", |_| SHOWN.store(true, Ordering::SeqCst));
    let a = app.clone();
    app.listen("quit-answer", move |e| {
        if let Ok(mut g) = ASKED.lock() {
            *g = None;
        }
        answer_quit(&a, e.payload().trim_matches('"'));
    });
}

fn answer_quit(app: &AppHandle, answer: &str) {
    match answer {
        "quit" => crate::tray::quit_app(app),
        "hide" => {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.hide();
            }
        }
        _ => {}
    }
}

/// The same question as the page's, as a native alert. Runs on the main thread.
fn native_quit_alert(app: &AppHandle, running: usize) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSAlert;
    use objc2_foundation::NSString;
    let Some(mtm) = MainThreadMarker::new() else { return };
    if let Ok(mut g) = ASKED.lock() {
        *g = None;
    }
    let name = app.package_info().name.clone();
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&format!("Quit {name}?")));
    let sessions = if running == 1 { "1 card session is".to_string() } else { format!("{running} card sessions are") };
    alert.setInformativeText(&NSString::from_str(&format!(
        "{sessions} running. Quitting suspends them, and the ones mid-turn resume when the app opens again."
    )));
    alert.addButtonWithTitle(&NSString::from_str("Quit"));
    alert.addButtonWithTitle(&NSString::from_str("Hide window"));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    let answer = match alert.runModal() {
        1000 => "quit",
        1001 => "hide",
        _ => "cancel",
    };
    answer_quit(app, answer);
}

// --- Card sessions end with the app ---------------------------------------------------------

/// The record of every process in the card sessions' process groups, and the reaper that ends
/// them when the app's process ends, cleanly or in a crash (the Mac's stand-in for the Job
/// Object). Each claude leads its own session (portable-pty calls setsid), and `nohup` and `&`
/// keep the session ID, so a dev server claude backgrounds is recorded even after its shell
/// has gone. Only a process that calls setsid itself escapes (README).
pub struct Job {
    record: Arc<Mutex<track::Record>>,
    file: PathBuf,
}

impl Job {
    pub fn new() -> Result<Job, String> {
        let pid = std::process::id() as i32;
        let dir = kanban_store::data_dir().join("run");
        sys::private_dir(&dir).map_err(|e| format!("the run folder: {e}"))?;
        let file = dir.join(format!("pids-{pid}.json"));
        let record = Arc::new(Mutex::new(track::Record { app_pid: pid, ..Default::default() }));
        let json = record.lock().map(|r| r.to_json()).unwrap_or_default();
        sys::write_private(&file, json.as_bytes()).map_err(|e| format!("the process record: {e}"))?;
        track::start_sampler(file.clone(), record.clone());
        start_reaper(pid, &file);
        Ok(Job { record, file })
    }

    /// Records claude (a session leader), so its session's processes are tracked from now on.
    /// The record is written at once, not at the sampler's next turn, so a crash straight after
    /// a start still ends that session. A claude that has already gone has nothing to record:
    /// the session's own exit handling reports it.
    pub fn assign(&self, pid: u32) -> Result<(), String> {
        let pid = pid as i32;
        let Some(start) = sys::process_start(pid) else { return Ok(()) };
        let json = match self.record.lock() {
            Ok(mut r) => {
                r.add_session(track::Proc { pid, start });
                r.to_json()
            }
            Err(_) => return Ok(()),
        };
        if let Err(e) = sys::write_private(&self.file, json.as_bytes()) {
            eprintln!("the process record: {e}");
        }
        Ok(())
    }
}

/// `kb reap <pid> <record>`, in a process group of its own so a signal to the app's group
/// doesn't reach it. In kb, not this binary, so a quit by this app's name never matches it.
fn start_reaper(app_pid: i32, file: &Path) {
    use std::os::unix::process::CommandExt;
    let Some(kb) = kb_beside_app() else {
        eprintln!("no kb beside the app: card sessions' leftover processes won't be ended at exit");
        return;
    };
    let started = std::process::Command::new(kb)
        .arg("reap")
        .arg(app_pid.to_string())
        .arg(file)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn();
    if let Err(e) = started {
        eprintln!("couldn't start kb reap: {e}");
    }
}

fn kb_beside_app() -> Option<PathBuf> {
    let p = std::env::current_exe().ok()?.parent()?.join("kb");
    p.is_file().then_some(p)
}

/// Waits for claude to exit without reaping it (Sessions reaps it when it drops the session),
/// so portable-pty never signals a pid the system has given to another process.
pub fn exit_waiter(pid: u32) -> Option<impl FnOnce() + Send> {
    Some(move || sys::wait_exit(pid as i32))
}

// --- Helper processes: nothing to hide on a Mac ---------------------------------------------

pub trait NoWindow {
    fn no_window(&mut self) -> &mut Self;
}

impl NoWindow for std::process::Command {
    fn no_window(&mut self) -> &mut Self {
        self
    }
}

// --- Where Claude Code and Chrome are -------------------------------------------------------

/// The native install, the old local install, Homebrew, the desktop app's copies, then PATH
/// (taken from the login shell at start). Each is asked its version; the newest is used.
pub fn claude_candidates(home: &Path) -> Vec<PathBuf> {
    let desktop = home.join("Library").join("Application Support").join("Claude").join("claude-code");
    let dirs: Vec<PathBuf> = std::fs::read_dir(desktop).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    let path = std::env::var("PATH").ok();
    text::claude_candidates(home, path.as_deref(), &dirs)
}

pub fn no_home() -> PathBuf {
    PathBuf::from("/")
}

pub fn chrome_paths() -> Vec<PathBuf> {
    let app = Path::new("Google Chrome.app").join("Contents").join("MacOS").join("Google Chrome");
    let mut places = vec![Path::new("/Applications").join(&app)];
    if let Some(home) = kanban_store::home_var() {
        places.push(home.join("Applications").join(&app));
    }
    places
}

// --- Notifications (router decision D7; critic finding 20) ----------------------------------

/// Notifications waiting for their click now, at most `MAX_WAITING`.
static WAITING: AtomicUsize = AtomicUsize::new(0);
const MAX_WAITING: usize = 16;
/// A waiter whose notification is still in Notification Center after this is ended.
const WAIT_LIMIT: Duration = Duration::from_secs(60 * 60);

/// Each card's newest notification (its number in posting order, and its title), so a new
/// one replaces it. Titles start with the card's number, so two cards never share one.
struct Posted {
    newest: HashMap<String, (u64, String)>,
    /// Waited-on notifications, oldest first: (posted at, card, number).
    waiting: Vec<(Instant, String, u64)>,
    next: u64,
}

fn posted() -> &'static Mutex<Posted> {
    static POSTED: OnceLock<Mutex<Posted>> = OnceLock::new();
    POSTED.get_or_init(|| {
        // One sweeper for every waiter: once a minute, a card's newest notification still
        // waiting after an hour is taken out of Notification Center, which ends its waiter.
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_secs(60));
            let due: Vec<String> = match posted().lock() {
                Ok(mut p) => {
                    let mut due = Vec::new();
                    let p = &mut *p;
                    p.waiting.retain(|(at, card, n)| {
                        if at.elapsed() < WAIT_LIMIT {
                            return true;
                        }
                        // Only if it is still the card's newest: an older one went when it was replaced.
                        if let Some((newest, head)) = p.newest.get(card) {
                            if newest == n {
                                due.push(head.clone());
                            }
                        }
                        false
                    });
                    due
                }
                Err(_) => continue,
            };
            for head in due {
                remove_delivered(&head);
            }
        });
        Mutex::new(Posted { newest: HashMap::new(), waiting: Vec::new(), next: 0 })
    })
}

/// Shows a notification with mac-notification-sys, the library the notification plugin itself
/// uses (as windows.rs uses the plugin's WinRT library), because the plugin drops the click.
/// A click brings the window forward and opens the card. The card's older notification is
/// removed first, which also ends its waiter, so waiters never pile up.
pub fn toast(app: &AppHandle, card_id: &str, head: &str, body: &str) {
    static APP_SET: OnceLock<()> = OnceLock::new();
    APP_SET.get_or_init(|| {
        // An app run from target/ isn't a bundle macOS knows, so its notifications are posted
        // as Terminal's, the plugin's own rule.
        let in_bundle = std::env::current_exe().map(|e| e.to_string_lossy().contains(".app/Contents/MacOS/")).unwrap_or(false);
        let id = if in_bundle { app.config().identifier.clone() } else { "com.apple.Terminal".to_string() };
        let _ = mac_notification_sys::set_application(&id);
    });
    let wait = WAITING.fetch_add(1, Ordering::SeqCst) < MAX_WAITING;
    if !wait {
        WAITING.fetch_sub(1, Ordering::SeqCst);
    }
    let older = match posted().lock() {
        Ok(mut p) => {
            p.next += 1;
            let n = p.next;
            if wait {
                p.waiting.push((Instant::now(), card_id.to_string(), n));
            }
            p.newest.insert(card_id.to_string(), (n, head.to_string())).map(|(_, h)| h)
        }
        Err(_) => None,
    };
    if let Some(older) = older {
        remove_delivered(&older);
    }
    let (a, card, head, body) = (app.clone(), card_id.to_string(), head.to_string(), body.to_string());
    std::thread::spawn(move || {
        let result = mac_notification_sys::Notification::new().title(&head).message(&body).wait_for_click(wait).send();
        if wait {
            WAITING.fetch_sub(1, Ordering::SeqCst);
        }
        match result {
            Ok(mac_notification_sys::NotificationResponse::Click) => {
                crate::tray::show_main(&a);
                // No card (card #104's small-change notices): a click shows the board only.
                if !card.is_empty() {
                    let _ = a.emit("open-card", card);
                }
            }
            Ok(_) => {}
            Err(e) => eprintln!("notification: {e}"),
        }
    });
}

/// Takes this app's delivered notifications with `title` out of Notification Center.
/// (NSUserNotification is deprecated, but it is what mac-notification-sys posts with.)
#[allow(deprecated)]
fn remove_delivered(title: &str) {
    use objc2_foundation::NSUserNotificationCenter;
    let center = NSUserNotificationCenter::defaultUserNotificationCenter();
    for n in center.deliveredNotifications().iter() {
        if n.title().is_some_and(|t| t.to_string() == title) {
            center.removeDeliveredNotification(&n);
        }
    }
}

// --- The board window -----------------------------------------------------------------------

fn rect_of(m: &tauri::Monitor, work: bool) -> Rect {
    if work {
        let a = m.work_area();
        Rect { x: a.position.x, y: a.position.y, w: a.size.width as i32, h: a.size.height as i32 }
    } else {
        Rect { x: m.position().x, y: m.position().y, w: m.size().width as i32, h: m.size().height as i32 }
    }
}

pub fn work_area(w: &WebviewWindow) -> Rect {
    w.current_monitor()
        .ok()
        .flatten()
        .or_else(|| w.primary_monitor().ok().flatten())
        .map(|m| rect_of(&m, true))
        .unwrap_or(Rect { x: 0, y: 0, w: 1440, h: 900 })
}

pub fn virtual_screen(w: &WebviewWindow) -> Rect {
    let screens: Vec<Rect> = w.available_monitors().unwrap_or_default().iter().map(|m| rect_of(m, false)).collect();
    if screens.is_empty() {
        return work_area(w);
    }
    let (x, y) = (screens.iter().map(|r| r.x).min().unwrap_or(0), screens.iter().map(|r| r.y).min().unwrap_or(0));
    let right = screens.iter().map(|r| r.x + r.w).max().unwrap_or(1440);
    let bottom = screens.iter().map(|r| r.y + r.h).max().unwrap_or(900);
    Rect { x, y, w: right - x, h: bottom - y }
}

/// Puts the window at `r` and shows it: in front with focus, or with `background` behind every
/// other window without activating the app. Runs on the main thread.
pub fn place_window(w: &WebviewWindow, r: Rect, background: bool) {
    let _ = w.set_size(tauri::PhysicalSize::new(r.w.max(1) as u32, r.h.max(1) as u32));
    let _ = w.set_position(tauri::PhysicalPosition::new(r.x, r.y));
    if background {
        if let Ok(p) = w.ns_window() {
            // The NSWindow tao made for this window, alive as long as `w` is.
            let win: &objc2_app_kit::NSWindow = unsafe { &*(p as *const objc2_app_kit::NSWindow) };
            win.orderBack(None);
        }
    } else {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Hiding needs nothing more than the framework's own hide on a Mac.
pub fn hide_window(_window: &Window) {}

// --- The menu-bar icon ----------------------------------------------------------------------

/// The live app's icon is a template (black and alpha), so it suits light and dark menu bars;
/// the test app keeps its coloured icon, so the two are told apart. A click opens the menu.
pub fn tray_style<R: Runtime>(b: TrayIconBuilder<R>) -> TrayIconBuilder<R> {
    let b = b.show_menu_on_left_click(true);
    if kanban_store::is_test() {
        b
    } else {
        b.icon(tauri::include_image!("./icons/tray-template.png")).icon_as_template(true)
    }
}

/// The number of cards that need Hugh, beside the menu-bar icon (none when it is 0).
pub fn tray_count<R: Runtime>(tray: &TrayIcon<R>, n: usize) {
    let _ = tray.set_title(if n == 0 { None } else { Some(n.to_string()) });
}

// --- The install script's quit (critic finding 28 of 114.md: SIGTERM, not a named event) ----

/// SIGTERM (scripts/install.sh), SIGINT (Ctrl+C in a terminal) or SIGHUP (that terminal
/// closing) quit once, as tray > Quit does.
pub fn on_quit_signal(app: AppHandle, quit: fn(&AppHandle)) {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    let Ok(mut signals) = signal_hook::iterator::Signals::new([SIGTERM, SIGINT, SIGHUP]) else { return };
    std::thread::spawn(move || {
        if signals.forever().next().is_some() {
            quit(&app);
        }
        // Later signals are taken and ignored while the sessions stop.
        for _ in signals.forever() {}
    });
}

// --- Restart now -----------------------------------------------------------------------------

/// The `.app` folder `p` is in, or `p` itself.
fn bundle_of(p: &Path) -> PathBuf {
    p.ancestors().find(|a| a.extension().is_some_and(|x| x == "app")).map(PathBuf::from).unwrap_or_else(|| p.to_path_buf())
}

/// A sh script that waits for the app (pid) to exit, swaps in the staged .app (a ditto zip),
/// keeps the old one as update/previous.app and puts it back if the move fails, records the
/// result in restart-result.json beside the log, and opens the app in front. `background` (a
/// test app's Restart now, card #135) isn't used on a Mac yet: the app still opens in front.
pub fn update_helper(pid: u32, installer: &Path, install_dir: &Path, app_exe: &Path, log: &Path, _background: bool) -> String {
    let result = log.with_file_name("restart-result.json");
    text::update_helper_script(pid, installer, install_dir, &bundle_of(app_exe), log, &result)
}

pub fn spawn_detached(script: &str) -> Result<(), String> {
    sys::spawn_detached_sh(script).map_err(|e| format!("couldn't start the update helper: {e}"))
}

// --- Files ----------------------------------------------------------------------------------

/// `file:///Users/x.png` is `/Users/x.png`: the slash stays.
pub fn file_url_path(path: &str) -> &str {
    text::file_url_path(path)
}

/// A file only this user can read (api.json and the session settings hold the API token).
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    sys::write_private(path, bytes)
}

/// A resolved path as a string: on macOS it is already an ordinary one (card #125).
pub fn plain_path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Whether `p` is in the system's own settings folders under `home`, which a card's proof never
/// reads (card #125): ~/Library (keychains, application data, cookies). Both paths are resolved.
pub fn in_settings_folder(home: &Path, p: &Path) -> bool {
    p.starts_with(home.join("Library"))
}

// --- The shared test-app lock files (testapp.rs) --------------------------------------------

/// Nothing to set: a Unix file is opened whoever else has it open.
pub fn share_all(_o: &mut std::fs::OpenOptions) {}

/// A lock file's writer is checked by its pid and start time, as on Windows (card #117).
pub const PID_LOCKS: bool = true;

/// The start time of process `pid`, if it is running (libproc, through kanban-unix).
pub fn process_start(pid: u32) -> Option<chrono::DateTime<chrono::Utc>> {
    let us = sys::process_start(i32::try_from(pid).ok()?)?;
    chrono::DateTime::from_timestamp_micros(i64::try_from(us).ok()?)
}

/// Whether someone holds `path`'s flock (install.sh holds install.lock's on its own
/// descriptor). On a Mac the file stays after an install, so its presence says nothing; the
/// lock does. A file that isn't there or can't be opened isn't held.
pub fn file_lock_held(path: &Path) -> Option<bool> {
    use std::os::fd::AsRawFd;
    let Ok(f) = std::fs::File::open(path) else { return Some(false) };
    // Taking it shows it was free; closing the file at once lets it go again.
    Some(!matches!(sys::flock_nb(f.as_raw_fd()), Ok(true)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// install.lock is held only while someone holds its flock (critic N1, card #117).
    #[test]
    fn a_lock_file_is_held_only_while_flocked() {
        use std::os::fd::AsRawFd;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("install.lock");
        assert_eq!(file_lock_held(&p), Some(false), "no file");
        std::fs::write(&p, "{}").unwrap();
        assert_eq!(file_lock_held(&p), Some(false), "a file left behind");
        let holder = std::fs::OpenOptions::new().read(true).write(true).open(&p).unwrap();
        assert_eq!(sys::flock_nb(holder.as_raw_fd()).unwrap(), true);
        assert_eq!(file_lock_held(&p), Some(true), "held");
        drop(holder);
        assert_eq!(file_lock_held(&p), Some(false), "let go");
        let me = std::process::id();
        assert!(process_start(me).is_some(), "our own start time");
    }

    #[test]
    fn bundle_of_finds_the_app_folder() {
        let exe = Path::new("/Users/h/Applications/Claude Kanban DSH.app/Contents/MacOS/claude-kanban-dsh");
        assert_eq!(bundle_of(exe), Path::new("/Users/h/Applications/Claude Kanban DSH.app"));
        assert_eq!(bundle_of(Path::new("/Users/h/target/release/claude-kanban-dsh")), Path::new("/Users/h/target/release/claude-kanban-dsh"));
    }

    #[test]
    fn the_quit_alert_and_page_share_one_rule() {
        assert_eq!(text::quit_step(0, None, false), text::QuitStep::Quit);
        assert_eq!(text::quit_step(2, None, false), text::QuitStep::AskPage);
        assert_eq!(text::quit_step(2, Some(PAGE_WAIT.as_millis() as u64), false), text::QuitStep::NativeAlert);
    }
}

/// The command a card session runs to install its build into test app `channel` (card #147).
pub fn install_command(channel: &str) -> String {
    format!("sh scripts/install.sh --channel {channel}")
}
