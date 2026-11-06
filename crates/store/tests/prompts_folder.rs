// Card #114: migration 13 (schema13.sql) makes the shipped column prompts and the "Kanban app"
// template name their folder as {prompts}, which the app fills in (dispatch::with_prompts).
// Migration 11 is card #104's and 12 is card #109's. The tests put a board back to schema 12
// with the old full paths, then let the store migrate it (and apply schema13.sql directly to
// show it is safe to run twice).

use kanban_store::Store;
use rusqlite::Connection;
use std::path::PathBuf;

const SCHEMA13: &str = include_str!("../src/schema13.sql");
const OLD: &str = r"C:\Users\someone\kanban-app\prompts\";
const NEW: &str = r"{prompts}\";
const CHANGED: [(&str, &str); 4] =
    [("Planning", "planning.md"), ("In progress", "in-progress.md"), ("Ship it", "ship-it.md"), ("Deploy complete", "deploy-complete.md")];

/// A board made by the store, then put back to schema 12 with the shipped prompts' old full
/// paths, as a board was before migration 13.
fn board() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    drop(Store::open(&path).unwrap());
    let conn = Connection::open(&path).unwrap();
    conn.execute("UPDATE columns SET prompt_text = replace(prompt_text, ?1, ?2)", [NEW, OLD]).unwrap();
    conn.execute("UPDATE templates SET start_prompt = replace(start_prompt, ?1, ?2) WHERE id = 'kanban-app'", [NEW, OLD]).unwrap();
    conn.pragma_update(None, "user_version", 12).unwrap();
    (dir, path)
}

/// Opens the board with the store, which runs migration 13 on it.
fn migrate(path: &PathBuf) {
    let s = Store::open(path).unwrap();
    assert_eq!(s.schema_version().unwrap(), kanban_store::SCHEMA_VERSION);
}

fn prompts(conn: &Connection) -> Vec<(String, Option<String>)> {
    let mut st = conn.prepare("SELECT name, prompt_text FROM columns ORDER BY name").unwrap();
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect()
}

fn template(conn: &Connection) -> String {
    conn.query_row("SELECT start_prompt FROM templates WHERE id = 'kanban-app'", [], |r| r.get(0)).unwrap()
}

#[test]
fn shipped_prompts_name_their_folder_as_prompts() {
    let (_dir, path) = board();
    let conn = Connection::open(&path).unwrap();
    let before = prompts(&conn);
    let tpl_before = template(&conn);
    for (name, file) in CHANGED {
        let p = before.iter().find(|(n, _)| n == name).and_then(|(_, p)| p.clone()).unwrap();
        assert!(p.contains(&format!("{OLD}{file}")), "{name} before: {p}");
    }
    assert!(tpl_before.contains(&format!("{OLD}kanban-app.md")));

    migrate(&path);
    let after = prompts(&conn);
    for ((name, old), (_, new)) in before.iter().zip(&after) {
        let (old, new) = (old.as_deref().unwrap_or(""), new.as_deref().unwrap_or(""));
        // Only the folder changes, so filling {prompts} back in gives the old text exactly.
        assert_eq!(new, old.replace(OLD, NEW), "{name}");
        assert!(!new.contains(OLD), "{name}: {new}");
        if let Some((_, file)) = CHANGED.iter().find(|(n, _)| n == name) {
            assert!(new.contains(&format!("{NEW}{file}")), "{name}: {new}");
        } else {
            assert_eq!(new, old, "{name} is not a column migration 13 changes");
        }
    }
    let tpl = template(&conn);
    assert_eq!(tpl, tpl_before.replace(OLD, NEW));
    assert!(tpl.contains(r"{prompts}\kanban-app.md"), "{tpl}");

    // Twice is the same as once, and the store opens the board as before.
    conn.execute_batch(SCHEMA13).unwrap();
    assert_eq!(prompts(&conn), after);
    drop(conn);
    let s = Store::open(&path).unwrap();
    assert!(s.column("Planning").unwrap().unwrap().prompt_text.unwrap().ends_with(r"follow {prompts}\planning.md."));
}

#[test]
fn an_edited_prompt_is_left_alone() {
    let (_dir, path) = board();
    // Edited on the schema-12 board (opening it with the store would migrate it first).
    let conn = Connection::open(&path).unwrap();
    conn.execute("UPDATE columns SET prompt_text = ?1 WHERE name = 'Planning'", [r"Plan #{n}: follow C:\Users\someone\kanban-app\prompts\planning.md, carefully."]).unwrap();
    conn.execute("UPDATE templates SET start_prompt = start_prompt || ' Also: be brief.' WHERE id = 'kanban-app'", []).unwrap();
    let tpl_edited = template(&conn);
    migrate(&path);
    let after = prompts(&conn);
    let planning = after.iter().find(|(n, _)| n == "Planning").and_then(|(_, p)| p.clone()).unwrap();
    assert_eq!(planning, r"Plan #{n}: follow C:\Users\someone\kanban-app\prompts\planning.md, carefully.");
    assert_eq!(template(&conn), tpl_edited);
    let building = after.iter().find(|(n, _)| n == "In progress").and_then(|(_, p)| p.clone()).unwrap();
    assert_eq!(building, r"Build it now: follow {prompts}\in-progress.md.", "an unedited column still changes");
}
