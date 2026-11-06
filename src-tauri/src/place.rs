//! Where the board window opens: where Hugh last left it (`window.json` in the data folder),
//! or, the first time, filling the screen's work area (the whole screen less the taskbar).
//! It is placed without activating it, so a background start never takes focus; any other start
//! (Hugh's own, or the restart after Restart now) brings it to the front.

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{WebviewWindow, WindowEvent};

/// Set once the window has been placed, so the framework's own first size isn't saved.
static PLACED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

fn file() -> std::path::PathBuf {
    kanban_store::data_dir().join("window.json")
}

/// The screen's work area, in physical pixels.
fn work_area() -> Rect {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
    let mut r = RECT::default();
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut r as *mut RECT as *mut core::ffi::c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) };
    if ok.is_ok() && r.right > r.left {
        Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
    } else {
        Rect { x: 0, y: 0, w: 1440, h: 900 }
    }
}

/// The whole desktop across every screen.
fn virtual_screen() -> Rect {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};
    unsafe {
        Rect {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            w: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            h: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

/// A saved place is used only if it is a sensible size and its title bar is on a screen.
pub fn usable(saved: Option<Rect>, screen: Rect) -> Option<Rect> {
    let r = saved?;
    let on_screen = r.x + 100 > screen.x && r.x < screen.x + screen.w - 100 && r.y >= screen.y - 10 && r.y < screen.y + screen.h - 40;
    (r.w >= 600 && r.h >= 400 && on_screen).then_some(r)
}

pub fn initial() -> Rect {
    let saved = std::fs::read_to_string(file()).ok().and_then(|t| serde_json::from_str(&t).ok());
    usable(saved, virtual_screen()).unwrap_or_else(work_area)
}

/// Puts the window at `r` and shows it: behind everything and not activated with
/// `background`, else in front. Runs on the main thread.
pub fn place(w: &WebviewWindow, r: Rect, background: bool) {
    if let Ok(h) = w.hwnd() {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, HWND_BOTTOM, HWND_TOP, SWP_NOACTIVATE, SWP_SHOWWINDOW};
        let hwnd = HWND(h.0 as *mut core::ffi::c_void);
        let (after, flags) = if background { (HWND_BOTTOM, SWP_SHOWWINDOW | SWP_NOACTIVATE) } else { (HWND_TOP, SWP_SHOWWINDOW) };
        unsafe {
            let _ = SetWindowPos(hwnd, after, r.x, r.y, r.w, r.h, flags);
        }
        if !background {
            bring_forward(hwnd);
        }
    }
    PLACED.store(true, Ordering::SeqCst);
}

/// Makes the window the foreground window. Windows refuses `SetForegroundWindow` to a process
/// that wasn't started by the foreground one, which is how the app comes back after Restart now
/// (a hidden helper starts it once the old copy has gone), so a refused call only flashes the
/// taskbar. Then it taps Alt first and asks again, as the window framework does when it creates
/// a focused window: Windows counts the tap as the user's own input. (Not `AttachThreadInput`:
/// sharing a hung app's input queue would hang the board's UI thread.)
pub fn bring_forward(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VK_LMENU, VK_MENU,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};
    unsafe {
        if GetForegroundWindow() == hwnd || SetForegroundWindow(hwnd).as_bool() {
            return;
        }
        let scan = MapVirtualKeyW(u32::from(VK_MENU.0), MAPVK_VK_TO_VSC) as u16;
        let key = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_LMENU, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
        };
        let taps = [key(KEYEVENTF_EXTENDEDKEY), key(KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP)];
        SendInput(&taps, std::mem::size_of::<INPUT>() as i32);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Remembers the window's place after Hugh moves or resizes it (not while it is minimised
/// or maximised).
pub fn on_event(w: &tauri::Window, e: &WindowEvent) {
    if !matches!(e, WindowEvent::Moved(_) | WindowEvent::Resized(_)) || !PLACED.load(Ordering::SeqCst) || w.label() != "main" {
        return;
    }
    if w.is_minimized().unwrap_or(false) || w.is_maximized().unwrap_or(false) {
        return;
    }
    let (Ok(p), Ok(s)) = (w.outer_position(), w.outer_size()) else { return };
    let r = Rect { x: p.x, y: p.y, w: s.width as i32, h: s.height as i32 };
    if r.x <= -30000 || r.w < 200 {
        return;
    }
    if let Ok(t) = serde_json::to_string(&r) {
        let _ = std::fs::write(file(), t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_place_is_kept_only_when_on_screen_and_big_enough() {
        let screen = Rect { x: 0, y: 0, w: 2560, h: 1440 };
        let ok = Rect { x: 100, y: 50, w: 1400, h: 900 };
        assert_eq!(usable(Some(ok), screen), Some(ok));
        assert_eq!(usable(Some(Rect { x: 5000, ..ok }), screen), None);
        assert_eq!(usable(Some(Rect { y: -500, ..ok }), screen), None);
        assert_eq!(usable(Some(Rect { w: 300, ..ok }), screen), None);
        assert_eq!(usable(None, screen), None);
    }
}
