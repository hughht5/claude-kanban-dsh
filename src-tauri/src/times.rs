//! Time in each column (card #111): the panel's "Time in columns" and the column headers'
//! medians. Read-only; the work is in the store (`crates/store/src/times.rs`).

use crate::{Board, CmdError, CmdResult};
use kanban_store::{CardTimes, ColumnMedian};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::State;

/// The board's medians are worked out at most this often: they read every card's history.
const MEDIANS_FOR: Duration = Duration::from_secs(5 * 60);
static MEDIANS: Mutex<Option<(Instant, Vec<ColumnMedian>)>> = Mutex::new(None);

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// One card's time in each column (every column but Done), up to now.
#[tauri::command]
pub fn card_times(board: State<Board>, card_id: String) -> CmdResult<CardTimes> {
    let store = board.0.lock().map_err(CmdError::other)?;
    store.card_times(&card_id, now_ms()).map_err(CmdError::other)
}

/// The board's median per column: finished visits of the last 30 days, dry-run cards left
/// out. Kept for `MEDIANS_FOR`, so opening cards doesn't read the whole board each time.
#[tauri::command]
pub fn column_medians(board: State<Board>) -> CmdResult<Vec<ColumnMedian>> {
    if let Ok(cache) = MEDIANS.lock() {
        if let Some((at, m)) = cache.as_ref() {
            if at.elapsed() < MEDIANS_FOR {
                return Ok(m.clone());
            }
        }
    }
    let m = {
        let store = board.0.lock().map_err(CmdError::other)?;
        store.column_medians(now_ms()).map_err(CmdError::other)?
    };
    if let Ok(mut cache) = MEDIANS.lock() {
        *cache = Some((Instant::now(), m.clone()));
    }
    Ok(m)
}
