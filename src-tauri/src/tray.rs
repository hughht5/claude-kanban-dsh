//! The tray: closing the window hides it while the sessions keep running; the tray menu has
//! Show and Quit, and the tooltip says how many sessions need Hugh.

use crate::sessions::Sessions;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Window, WindowEvent};

const TRAY: &str = "main";

pub fn setup(app: &App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut tray = TrayIconBuilder::with_id(TRAY)
        .tooltip(kanban_store::app_name())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() {
            "show" => show_main(app),
            "quit" => quit_app(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                // On a Mac the click opens the menu instead (card #117).
                if crate::platform::TRAY_LEFT_CLICK_SHOWS {
                    show_main(tray.app_handle());
                }
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    // The menu-bar icon's own look on a Mac; nothing changes on Windows.
    let tray = crate::platform::tray_style(tray);
    tray.build(app)?;

    for event in ["activity", "session-changed", "needs-changed", "cards-changed"] {
        let a = app.handle().clone();
        // Off the emitting thread: setting the tooltip waits on the main thread.
        crate::notify::listen_off_thread(app, event, move |_| refresh_tooltip(&a));
    }
    // Unread replies and crashes from before this start count from the first moment.
    {
        let a = app.handle().clone();
        std::thread::spawn(move || refresh_tooltip(&a));
    }
    wait_for_quit_signal(app.handle().clone());
    Ok(())
}

/// Closing the window only hides it; Quit is in the tray menu.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        if window.label() == "main" {
            api.prevent_close();
            let _ = window.hide();
            // Windows: the framework may not know the window is shown (platform/windows.rs).
            crate::platform::hide_window(window);
        }
    }
}

/// Brings the board window back and to the front.
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Stops every session (each is recorded as suspended), then exits. The stops take a
/// moment each, so they run off the event loop.
pub(crate) fn quit_app(app: &AppHandle) {
    let a = app.clone();
    std::thread::spawn(move || {
        a.state::<Arc<Sessions>>().stop_all(&a);
        a.exit(0);
    });
}

/// "Claude Kanban DSH · N need you" ("Claude Kanban DSH (test) · ..." in the test build), counting the
/// cards that need Hugh: a permission prompt, a failed session or an unread reply (needs.rs).
fn refresh_tooltip(app: &AppHandle) {
    let n = crate::needs::current(app).len();
    let name = kanban_store::app_name();
    let text = if n == 0 { name.to_string() } else { format!("{name} · {n} need you") };
    if let Some(tray) = app.tray_by_id(TRAY) {
        let _ = tray.set_tooltip(Some(text));
        crate::platform::tray_count(&tray, n);
    }
}

/// Lets the install script ask for a clean quit, the same way tray > Quit does; each channel's
/// install quits only its own app (platform/windows.rs: a named event).
fn wait_for_quit_signal(app: AppHandle) {
    crate::platform::on_quit_signal(app, quit_app);
}
