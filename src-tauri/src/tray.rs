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
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
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
            // The window is first shown with SetWindowPos (place.rs), which the window
            // framework doesn't see, so it still thinks the window is hidden and its hide()
            // does nothing. Hide it directly too.
            if let Ok(h) = window.hwnd() {
                use windows::Win32::Foundation::HWND;
                use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
                unsafe {
                    let _ = ShowWindow(HWND(h.0 as *mut core::ffi::c_void), SW_HIDE);
                }
            }
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
fn quit_app(app: &AppHandle) {
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
    }
}

/// Waits on a named Windows event so `scripts\install.ps1` can ask for a clean quit, the same
/// way tray > Quit does. Its name has a `Test` suffix in the test build, so each channel's
/// install quits only its own app.
fn wait_for_quit_signal(app: AppHandle) {
    use windows::core::HSTRING;
    use windows::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject, INFINITE};
    let Ok(event) = (unsafe { CreateEventW(None, true, false, &HSTRING::from(kanban_store::quit_event())) }) else { return };
    // The event is manual-reset and named, so if the copy before this one is still shutting
    // down, it is still set from that copy's quit. Clear it, or this copy would quit at once.
    let _ = unsafe { ResetEvent(event) };
    let raw = event.0 as isize;
    std::thread::spawn(move || {
        let event = windows::Win32::Foundation::HANDLE(raw as _);
        unsafe { WaitForSingleObject(event, INFINITE) };
        quit_app(&app);
    });
}
