// Card #104: the repos whose small, fully checked cards go on by themselves, and the
// append-only log of every change to the page's settings (migration 11).

use kanban_store::{page_default, SmallRepo, Store, PAGE_SETTINGS, SMALL_REPOS_KEY, TEST_RULE_KEY};
use serde_json::json;
use std::path::PathBuf;

fn temp_board() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    (dir, path)
}

/// A folder that looks like a git checkout (it has a .git), for the list's check.
fn checkout(dir: &tempfile::TempDir, name: &str) -> String {
    let p = dir.path().join(name);
    std::fs::create_dir_all(p.join(".git")).unwrap();
    p.display().to_string()
}

#[test]
fn the_repo_list_is_empty_by_default_and_on_the_page() {
    let (_dir, path) = temp_board();
    let s = Store::open(&path).unwrap();
    assert!(PAGE_SETTINGS.contains(&SMALL_REPOS_KEY));
    assert_eq!(page_default(SMALL_REPOS_KEY).unwrap(), "[]");
    assert_eq!(s.page_setting(SMALL_REPOS_KEY).unwrap(), "[]");
    assert!(s.small_repos().is_empty());
    assert!(s.small_repo_for("hughht5/kanban-app").is_none());
}

#[test]
fn the_repo_list_is_checked_and_found_by_repo_in_any_case() {
    let (dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let co = checkout(&dir, "repo");
    let good = json!([{ "repo": " hughht5/kanban-app ", "checkout": format!(" {co} "), "branch": " main " }]).to_string();
    let stored = s.set_page_setting(SMALL_REPOS_KEY, &good).unwrap();
    let want = SmallRepo { repo: "hughht5/kanban-app".into(), checkout: co.clone(), branch: "main".into() };
    assert_eq!(serde_json::from_str::<Vec<SmallRepo>>(&stored).unwrap(), vec![want.clone()]);
    assert_eq!(s.small_repos(), vec![want.clone()]);
    assert_eq!(s.small_repo_for("HughHT5/Kanban-App"), Some(want.clone()));
    assert!(s.small_repo_for("acme/web").is_none());
    assert!(s.small_repo_for("").is_none());

    let not_git = dir.path().join("plain");
    std::fs::create_dir_all(&not_git).unwrap();
    let bads = [
        "not json".to_string(),
        json!({ "repo": "a/b" }).to_string(),
        json!([{ "repo": "kanban-app", "checkout": co, "branch": "main" }]).to_string(),
        json!([{ "repo": "a/b/c", "checkout": co, "branch": "main" }]).to_string(),
        json!([{ "repo": "a/..", "checkout": co, "branch": "main" }]).to_string(),
        json!([{ "repo": "a/b", "checkout": "relative\\path", "branch": "main" }]).to_string(),
        json!([{ "repo": "a/b", "checkout": not_git.display().to_string(), "branch": "main" }]).to_string(),
        json!([{ "repo": "a/b", "checkout": co, "branch": "" }]).to_string(),
        json!([{ "repo": "a/b", "checkout": co, "branch": "-f" }]).to_string(),
        json!([{ "repo": "a/b", "checkout": co, "branch": "a..b" }]).to_string(),
        json!([{ "repo": "a/b", "checkout": co, "branch": "main; rm" }]).to_string(),
        json!([{ "repo": "a/b", "checkout": co, "branch": "main" }, { "repo": "A/B", "checkout": co, "branch": "main" }]).to_string(),
    ];
    for bad in &bads {
        assert!(s.set_page_setting(SMALL_REPOS_KEY, bad).is_err(), "{bad}");
    }
    // A refused value leaves the list as it was.
    assert_eq!(s.small_repos(), vec![want]);
    // An empty list turns it off.
    assert_eq!(s.set_page_setting(SMALL_REPOS_KEY, "[]").unwrap(), "[]");
    assert!(s.small_repos().is_empty());
}

#[test]
fn a_saved_list_that_no_longer_parses_counts_as_none() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    s.set_setting(SMALL_REPOS_KEY, "{broken").unwrap();
    assert!(s.small_repos().is_empty());
    assert!(s.small_repo_for("a/b").is_none());
}

#[test]
fn every_change_of_a_setting_is_logged_with_who_made_it() {
    let (dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let co = checkout(&dir, "repo");
    let list = json!([{ "repo": "a/b", "checkout": co, "branch": "main" }]).to_string();
    s.set_page_setting_by(SMALL_REPOS_KEY, &list, "Hugh (Settings)").unwrap();
    // The same value again is no change, so no line.
    s.set_page_setting_by(SMALL_REPOS_KEY, &list, "Hugh (Settings)").unwrap();
    s.set_page_setting_by(SMALL_REPOS_KEY, "[]", "Hugh (Settings)").unwrap();
    // Other settings are logged too, as "the app" when no one is named.
    s.set_page_setting(TEST_RULE_KEY, "off").unwrap();
    let log = s.setting_log(SMALL_REPOS_KEY, 10).unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!((log[0].value.as_str(), log[0].by.as_str()), ("[]", "Hugh (Settings)"));
    assert!(log[1].value.contains("a/b"));
    assert!(log[0].at >= log[1].at);
    let t = s.setting_log(TEST_RULE_KEY, 10).unwrap();
    assert_eq!((t.len(), t[0].value.as_str(), t[0].by.as_str()), (1, "off", "the app"));
    assert_eq!(s.setting_log(SMALL_REPOS_KEY, 1).unwrap().len(), 1);
    // A refused value is not logged.
    assert!(s.set_page_setting_by(SMALL_REPOS_KEY, "nope", "Hugh (Settings)").is_err());
    assert_eq!(s.setting_log(SMALL_REPOS_KEY, 10).unwrap().len(), 2);
    // It survives a reopen.
    drop(s);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.setting_log(SMALL_REPOS_KEY, 10).unwrap().len(), 2);
}

#[test]
fn migration_11_is_append_only_safe_to_run_twice_and_backed_up() {
    let (dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), kanban_store::SCHEMA_VERSION);
    s.set_page_setting_by(TEST_RULE_KEY, "off", "Hugh (Settings)").unwrap();
    drop(s);
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        let del = format!("{} {} settings_log", "DELETE", "FROM");
        assert!(conn.execute(&del, []).is_err(), "a delete must be refused");
        assert!(conn.execute("UPDATE settings_log SET value = 'on'", []).is_err(), "an update must be refused");
        // Put the board back to schema 10, as if an older build made it.
        conn.pragma_update(None, "user_version", 10).unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), kanban_store::SCHEMA_VERSION);
    // The log is kept, and a backup was taken before the migration.
    assert_eq!(s.setting_log(TEST_RULE_KEY, 10).unwrap().len(), 1);
    let backups: Vec<_> = std::fs::read_dir(dir.path().join("backups")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains("pre-migration")).collect();
    assert!(!backups.is_empty());
}

#[test]
fn a_board_from_a_newer_build_still_opens_with_the_new_key_and_fields() {
    let (dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let co = checkout(&dir, "repo");
    s.set_page_setting(SMALL_REPOS_KEY, &json!([{ "repo": "a/b", "checkout": co, "branch": "main" }]).to_string()).unwrap();
    let c = s
        .create(None, json!({ "title": "T", "column": "Ship it", "smallChange": { "state": "went on", "wentOnAt": "2026-11-06T10:00:00.000Z" }, "smallOptOut": true }).as_object().cloned().unwrap(), "t")
        .unwrap();
    drop(s);
    // A later build has moved the board on; this one opens it and reads what it knows.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), 99);
    let c = s.get(&c.id).unwrap();
    assert_eq!(c.fields["smallChange"]["state"], "went on");
    assert_eq!(s.small_repos().len(), 1);
}

#[test]
fn a_card_waiting_to_go_on_stops_waiting_when_it_leaves_human_test() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let mk = |s: &mut Store| {
        s.create(None, json!({ "title": "T", "column": "Human test", "smallChange": { "state": "waiting", "goesAt": "2026-11-06T10:10:00.000Z" } }).as_object().cloned().unwrap(), "t").unwrap()
    };
    // Hugh's move.
    let c = mk(&mut s);
    let c = s.move_by_hugh(&c.id, c.version, "In progress", Default::default()).unwrap().card;
    assert_eq!(c.fields["smallChange"]["state"], "stopped");
    assert!(c.fields["smallChange"]["reason"].as_str().unwrap().contains("left Human test (moved to In progress)"));
    assert_eq!(c.fields["smallChange"]["goesAt"], "2026-11-06T10:10:00.000Z");
    // Any other move (a session's update of its column).
    let c = mk(&mut s);
    let mut p = kanban_store::Patch::new();
    p.insert("column".into(), json!("Blocked"));
    let c = s.update(&c.id, c.version, &p, "t").unwrap();
    assert_eq!(c.fields["smallChange"]["state"], "stopped");
    // Other states, and edits that don't move it, are left alone.
    let c = mk(&mut s);
    let mut p = kanban_store::Patch::new();
    p.insert("summary".into(), json!("x"));
    let c = s.update(&c.id, c.version, &p, "t").unwrap();
    assert_eq!(c.fields["smallChange"]["state"], "waiting");
    let went = s.create(None, json!({ "title": "T", "column": "Ship it", "smallChange": { "state": "went on", "wentOnAt": "x" } }).as_object().cloned().unwrap(), "t").unwrap();
    let mut p = kanban_store::Patch::new();
    p.insert("column".into(), json!("In progress"));
    let went = s.update(&went.id, went.version, &p, "t").unwrap();
    // One that went on and left Ship it unshipped is no longer the app's ship, but it keeps
    // wentOnAt, so it never goes on by itself again.
    assert_eq!(went.fields["smallChange"]["state"], "stopped");
    assert!(went.fields["smallChange"]["reason"].as_str().unwrap().contains("left Ship it (moved to In progress) before it shipped"));
    assert_eq!(went.fields["smallChange"]["wentOnAt"], "x");
    // Shipped: on into Deploy complete, it stays as it went.
    let ok = s.create(None, json!({ "title": "T", "column": "Ship it", "smallChange": { "state": "went on", "wentOnAt": "x" } }).as_object().cloned().unwrap(), "t").unwrap();
    let mut p = kanban_store::Patch::new();
    p.insert("column".into(), json!("Deploy complete"));
    let ok = s.update(&ok.id, ok.version, &p, "t").unwrap();
    assert_eq!(ok.fields["smallChange"]["state"], "went on");
    // The app's own move writes the record with it, in one change.
    let w = mk(&mut s);
    let mut extra = kanban_store::Patch::new();
    extra.insert("smallChange".into(), json!({ "state": "went on", "wentOnAt": "z" }));
    let m = s.move_for_hugh_with(&w.id, w.version, "Ship it", "small and checked", extra).unwrap();
    assert_eq!((m.card.column.as_str(), m.card.fields["smallChange"]["state"].as_str()), ("Ship it", Some("went on")));
    assert_eq!(m.card.fields["ship"]["auto"], json!(true));
    // The same patch can set it itself (the app's move into Ship it).
    let c = mk(&mut s);
    let mut p = kanban_store::Patch::new();
    p.insert("column".into(), json!("Ship it"));
    p.insert("smallChange".into(), json!({ "state": "went on", "wentOnAt": "y" }));
    let c = s.update(&c.id, c.version, &p, "t").unwrap();
    assert_eq!(c.fields["smallChange"]["state"], "went on");
}
