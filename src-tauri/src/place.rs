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

/// A saved place is used only if it is a sensible size and its title bar is on a screen.
pub fn usable(saved: Option<Rect>, screen: Rect) -> Option<Rect> {
    let r = saved?;
    let on_screen = r.x + 100 > screen.x && r.x < screen.x + screen.w - 100 && r.y >= screen.y - 10 && r.y < screen.y + screen.h - 40;
    (r.w >= 600 && r.h >= 400 && on_screen).then_some(r)
}

pub fn initial(w: &WebviewWindow) -> Rect {
    let saved = std::fs::read_to_string(file()).ok().and_then(|t| serde_json::from_str(&t).ok());
    usable(saved, crate::platform::virtual_screen(w)).unwrap_or_else(|| crate::platform::work_area(w))
}

/// Puts the window at `r` and shows it: behind everything and not activated with
/// `background`, else in front. Runs on the main thread.
pub fn place(w: &WebviewWindow, r: Rect, background: bool) {
    crate::platform::place_window(w, r, background);
    PLACED.store(true, Ordering::SeqCst);
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
