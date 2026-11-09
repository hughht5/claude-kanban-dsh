//! The Settings screen's commands: column prompts, models and efforts; card templates; and the
//! allow-listed settings (`launch.profiles`, `sessions.max_running`) the page reads and writes.

use crate::{changed, refused, Board, CmdError, CmdResult};
use kanban_store::{page_default, ColumnCfg, Template, TemplateInput};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

fn settings_changed(app: &AppHandle, what: &str) {
    let _ = app.emit("settings-changed", what.to_string());
}

/// Every column with its saved settings and its shipped default, in board order.
#[tauri::command]
pub fn list_columns(board: State<Board>) -> CmdResult<Vec<Value>> {
    let store = board.0.lock().map_err(CmdError::other)?;
    let cols = store.columns().map_err(CmdError::other)?;
    Ok(cols
        .into_iter()
        .map(|c| {
            let d = store.column_default(&c.name).ok();
            let is_default = d.as_ref().map_or(false, |d| d.prompt_text == c.prompt_text && d.model == c.model && d.effort == c.effort);
            json!({ "column": c, "default": d, "isDefault": is_default })
        })
        .collect())
}

#[tauri::command]
pub fn update_column(app: AppHandle, board: State<Board>, name: String, prompt: Option<String>, model: Option<String>, effort: Option<String>) -> CmdResult<ColumnCfg> {
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let c = store
        .update_column(&name, prompt.as_deref(), model.as_deref(), effort.as_deref())
        .map_err(|e| refused(&store, e))?;
    drop(store);
    settings_changed(&app, "columns");
    Ok(c)
}

#[tauri::command]
pub fn reset_column(app: AppHandle, board: State<Board>, name: String) -> CmdResult<ColumnCfg> {
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let c = store.reset_column(&name).map_err(CmdError::other)?;
    drop(store);
    settings_changed(&app, "columns");
    Ok(c)
}

#[tauri::command]
pub fn list_templates(board: State<Board>, archived: Option<bool>) -> CmdResult<Vec<Template>> {
    let store = board.0.lock().map_err(CmdError::other)?;
    store.templates(archived.unwrap_or(false)).map_err(CmdError::other)
}

#[tauri::command]
pub fn save_template(app: AppHandle, board: State<Board>, template: TemplateInput) -> CmdResult<Template> {
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let t = store.save_template(&template).map_err(|e| refused(&store, e))?;
    drop(store);
    settings_changed(&app, "templates");
    Ok(t)
}

/// Templates are archived, never removed; `on: false` restores one.
#[tauri::command]
pub fn archive_template(app: AppHandle, board: State<Board>, id: String, on: bool) -> CmdResult<Template> {
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let t = store.archive_template(&id, on).map_err(CmdError::other)?;
    drop(store);
    settings_changed(&app, "templates");
    // Cards made from it show its name; let the board refresh.
    changed(&app);
    Ok(t)
}

/// An allow-listed setting: its value (saved, else the default) and its default.
#[tauri::command]
pub fn get_setting(board: State<Board>, key: String) -> CmdResult<Value> {
    let store = board.0.lock().map_err(CmdError::other)?;
    let value = store.page_setting(&key).map_err(CmdError::other)?;
    let default = page_default(&key).map_err(CmdError::other)?;
    Ok(json!({ "key": key, "value": value, "default": default }))
}

/// Saves an allow-listed setting after checking it. Returns the value as stored.
#[tauri::command]
pub fn set_setting(app: AppHandle, board: State<Board>, key: String, value: String) -> CmdResult<String> {
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let (v, changed_now) = store.set_page_setting_logged(&key, &value, "Hugh (Settings)").map_err(|e| refused(&store, e))?;
    drop(store);
    if key == kanban_store::PRIVATE_DIR_KEY {
        crate::private::set(&v);
    }
    settings_changed(&app, &key);
    // Card #162: a flow default (a subagent, skip Human test) reaches the cards that follow it
    // at once; their sessions are told where it matters.
    if changed_now {
        crate::dispatch::after_default_change(&app, &key);
    }
    // The repos whose small changes ship with no human test (card #104): every change is told,
    // so one Hugh didn't make is seen. The same rule as the log's.
    if key == kanban_store::SMALL_REPOS_KEY && changed_now {
        let repos: Vec<String> = serde_json::from_str::<Vec<kanban_store::SmallRepo>>(&v).unwrap_or_default().into_iter().map(|r| r.repo).collect();
        let _ = app.emit("small-repos", json!({ "repos": repos }));
    }
    Ok(v)
}

/// The repos a card can be for, from the private folder's `repos.json` (see `private`).
#[tauri::command]
pub fn list_repos() -> CmdResult<Vec<crate::private::Repo>> {
    Ok(crate::private::repos())
}

/// The repo list as Settings › Sessions edits it (card #155): names, labels and folders as written,
/// refused when `repos.json` can't be read, so a Save never drops what wasn't shown.
#[tauri::command]
pub fn edit_repos() -> CmdResult<Vec<crate::private::RepoEdit>> {
    crate::private::edit_rows().map_err(CmdError::other)
}

/// Saves the repo list (Settings, card #155) as the private folder's `repos.json` and returns it
/// as `edit_repos` would. The Add card dropdown reads the file each time it opens.
#[tauri::command]
pub fn save_repos(repos: Vec<crate::private::RepoEdit>) -> CmdResult<Vec<crate::private::RepoEdit>> {
    crate::private::save_repos(&repos).map_err(CmdError::other)
}

/// The latest changes of an allow-listed setting, newest first (card #104).
#[tauri::command]
pub fn get_setting_log(board: State<Board>, key: String, limit: Option<usize>) -> CmdResult<Value> {
    let store = board.0.lock().map_err(CmdError::other)?;
    page_default(&key).map_err(CmdError::other)?;
    let rows = store.setting_log(&key, limit.unwrap_or(10).min(50)).map_err(CmdError::other)?;
    Ok(json!(rows))
}
