// Settings screen: column config, templates, the browser-profile map and the page's settings.

use kanban_store::{default_profiles, profile_for, Error, Profile, Store, TemplateInput, COLUMNS, MAX_RUNNING_KEY, PROFILES_KEY};
use serde_json::json;
use std::path::PathBuf;

fn temp_board() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    (dir, path)
}

#[test]
fn column_config_updates_and_resets() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let cols = s.columns().unwrap();
    let names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, COLUMNS);
    let shipped = s.column("Planning").unwrap().unwrap();
    assert!(shipped.prompt_text.as_deref().unwrap().starts_with("You are the session for card #{n}"));

    let c = s.update_column("Planning", Some("Plan #{n} carefully."), Some("claude-sonnet-5-5"), Some("medium")).unwrap();
    assert_eq!(c.prompt_text.as_deref(), Some("Plan #{n} carefully."));
    assert_eq!((c.model.as_deref(), c.effort.as_deref()), (Some("claude-sonnet-5-5"), Some("medium")));
    // Mode and session rules aren't touched.
    assert_eq!((c.mode.as_str(), c.auto_spawn), (shipped.mode.as_str(), shipped.auto_spawn));
    // It survives a reopen.
    drop(s);
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.column("Planning").unwrap().unwrap().prompt_text.as_deref(), Some("Plan #{n} carefully."));

    // An empty prompt clears it; bad efforts, model names and tokens are refused.
    assert_eq!(s.update_column("Backlog", Some("  "), Some(""), Some("low")).unwrap().prompt_text, None);
    assert!(matches!(s.update_column("Planning", None, None, Some("huge")), Err(Error::Invalid(_))));
    assert!(matches!(s.update_column("Planning", None, Some("two words"), None), Err(Error::Invalid(_))));
    assert!(matches!(s.update_column("Planning", Some("use sk_live_abc"), None, None), Err(Error::Secret(_))));
    assert!(matches!(s.update_column("Nowhere", None, None, None), Err(Error::Invalid(_))));
    // The shipped prompt mentions passwords and ct= links (to forbid them); it still saves.
    s.update_column("Planning", shipped.prompt_text.as_deref(), Some("claude-opus-5-5"), Some("high")).unwrap();

    // Reset puts back the values the migrations ship, for every column.
    s.update_column("Planning", Some("x"), Some("m"), Some("low")).unwrap();
    let r = s.reset_column("Planning").unwrap();
    assert_eq!((r.prompt_text, r.model, r.effort), (shipped.prompt_text.clone(), shipped.model.clone(), shipped.effort.clone()));
    for name in COLUMNS {
        let d = s.column_default(name).unwrap();
        let now = s.reset_column(name).unwrap();
        assert_eq!((now.prompt_text, now.model, now.effort), (d.prompt_text, d.model, d.effort), "{name}");
    }
    assert!(s.column_default("Deploy complete").unwrap().prompt_text.unwrap().contains("deploy-complete.md"));
    assert_eq!(s.column_default("Blocked").unwrap().model.as_deref(), Some("claude-sonnet-5-5"));
    assert!(s.column_default("Nowhere").is_err());
}

#[test]
fn templates_seeded_editable_and_archived_not_deleted() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let t = s.templates(false).unwrap();
    let names: Vec<&str> = t.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Bug", "Customer request", "Small fix", "Kanban app"]);
    for x in &t {
        assert!(!x.title_prefix.is_empty() && !x.body.is_empty() && !x.start_prompt.is_empty(), "{}", x.name);
    }
    assert_eq!(t[0].id, "bug");

    // Add, then edit.
    let added = s
        .save_template(&TemplateInput { id: None, name: "Small fix".into(), title_prefix: "Tweak: ".into(), body: "b".into(), start_prompt: "p".into() })
        .unwrap();
    assert_eq!(added.id, "small-fix-2");
    assert_eq!(added.position, 4);
    let edited = s
        .save_template(&TemplateInput { id: Some("bug".into()), name: "Bug report".into(), title_prefix: "Bug: ".into(), body: "new".into(), start_prompt: "Repro first.".into() })
        .unwrap();
    assert_eq!((edited.name.as_str(), edited.body.as_str()), ("Bug report", "new"));
    assert!(matches!(s.save_template(&TemplateInput { name: " ".into(), ..Default::default() }), Err(Error::Invalid(_))));
    assert!(matches!(
        s.save_template(&TemplateInput { name: "x".into(), start_prompt: "token eyJhbGciOiJIUzI1".into(), ..Default::default() }),
        Err(Error::Secret(_))
    ));
    assert!(matches!(s.save_template(&TemplateInput { id: Some("nope".into()), name: "x".into(), ..Default::default() }), Err(Error::Invalid(_))));

    // Archive hides it from the picker but keeps it; restore brings it back.
    s.archive_template("small-fix", true).unwrap();
    assert!(!s.templates(false).unwrap().iter().any(|t| t.id == "small-fix"));
    assert!(s.templates(true).unwrap().iter().any(|t| t.id == "small-fix" && t.archived));
    assert!(s.template("small-fix").unwrap().unwrap().archived);
    s.archive_template("small-fix", false).unwrap();
    assert_eq!(s.templates(false).unwrap().len(), 5);

    // The database refuses to remove one.
    drop(s);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let del = format!("{} FROM templates WHERE id = 'bug'", "DELETE");
    assert!(conn.execute(&del, []).is_err());
}

#[test]
fn migration6_is_safe_to_run_twice() {
    let (_dir, path) = temp_board();
    Store::open(&path).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(include_str!("../src/schema6.sql")).unwrap();
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM templates", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 4);
}

#[test]
fn profile_map_default_and_lookup() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    // The default is the map launch.rs had in code.
    let d = s.launch_profiles().unwrap();
    assert_eq!(d, default_profiles());
    assert_eq!(d.len(), 2);
    assert_eq!(profile_for(&d, "Chrome · work (Profile 1)", "http://127.0.0.1:5173/x"), Some("Profile 1"));
    assert_eq!(profile_for(&d, "Chrome · Default (personal)", "https://x"), Some("Default"));
    assert_eq!(profile_for(&d, "Firefox", "https://x"), None);
    let shown: Vec<Profile> = serde_json::from_str(&s.page_setting(PROFILES_KEY).unwrap()).unwrap();
    assert_eq!(shown, d);

    // Saved rows replace it; a URL pattern matches when the label doesn't.
    let rows = json!([
        { "name": "Chrome · work (Profile 1)", "urlPattern": "", "profileDir": "Profile 1" },
        { "name": "Admin sign-ins", "urlPattern": "http://127.0.0.1:*?as=admin*", "profileDir": "Profile 3" }
    ]);
    s.set_page_setting(PROFILES_KEY, &rows.to_string()).unwrap();
    let p = s.launch_profiles().unwrap();
    assert_eq!(p.len(), 2);
    assert_eq!(profile_for(&p, "", "http://127.0.0.1:5173/login.html?as=admin&next=/o/a"), Some("Profile 3"));
    assert_eq!(profile_for(&p, "Chrome · Default (personal)", "https://x"), None);

    // Paths, empty folders, empty names and bad JSON are refused, and the saved map is unchanged.
    for bad in [
        json!([{ "name": "x", "profileDir": "..\\..\\Windows" }]).to_string(),
        json!([{ "name": "x", "profileDir": "" }]).to_string(),
        json!([{ "name": "", "profileDir": "Default" }]).to_string(),
        "not json".to_string(),
    ] {
        assert!(s.set_page_setting(PROFILES_KEY, &bad).is_err(), "{bad}");
    }
    assert_eq!(s.launch_profiles().unwrap(), p);
    // A stored value that no longer parses falls back to the default.
    s.set_setting(PROFILES_KEY, "{broken").unwrap();
    assert_eq!(s.launch_profiles().unwrap(), default_profiles());
}

#[test]
fn page_settings_are_allow_listed() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.page_setting(MAX_RUNNING_KEY).unwrap(), "10");
    assert_eq!(s.max_running().unwrap(), 10);
    assert_eq!(s.set_page_setting(MAX_RUNNING_KEY, " 4 ").unwrap(), "4");
    assert_eq!(s.max_running().unwrap(), 4);
    for bad in ["0", "101", "three", "2.5"] {
        assert!(s.set_page_setting(MAX_RUNNING_KEY, bad).is_err(), "{bad}");
    }
    assert_eq!(s.page_setting(MAX_RUNNING_KEY).unwrap(), "4");
    // Anything off the list is refused both ways.
    for key in ["usage.main", "app.run_started_seq", "api.token"] {
        assert!(s.page_setting(key).is_err(), "{key}");
        assert!(s.set_page_setting(key, "1").is_err(), "{key}");
    }
}

// Card #99: how often the monitor looks for stuck sessions, 0 for off.
#[test]
fn the_stuck_check_is_ten_minutes_a_whole_number_and_can_be_off() {
    use kanban_store::{page_default, STUCK_CHECK_DEFAULT, STUCK_CHECK_KEY};
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    assert_eq!(STUCK_CHECK_KEY, "sessions.stuck_check_minutes");
    assert_eq!(s.stuck_check_minutes().unwrap(), STUCK_CHECK_DEFAULT);
    assert_eq!(s.page_setting(STUCK_CHECK_KEY).unwrap(), "10");
    assert_eq!(page_default(STUCK_CHECK_KEY).unwrap(), "10");
    assert_eq!(s.set_page_setting(STUCK_CHECK_KEY, " 25 ").unwrap(), "25");
    assert_eq!(s.stuck_check_minutes().unwrap(), 25);
    assert_eq!(s.set_page_setting(STUCK_CHECK_KEY, "0").unwrap(), "0");
    assert_eq!(s.stuck_check_minutes().unwrap(), 0);
    assert_eq!(s.set_page_setting(STUCK_CHECK_KEY, "1440").unwrap(), "1440");
    for bad in ["-1", "1441", "ten", "2.5", ""] {
        assert!(matches!(s.set_page_setting(STUCK_CHECK_KEY, bad), Err(Error::Invalid(_))), "{bad}");
    }
    assert_eq!(s.stuck_check_minutes().unwrap(), 1440, "a refused value leaves the old one");
    // A value someone put in the settings table by hand that isn't a number reads as the default.
    s.set_setting(STUCK_CHECK_KEY, "soon").unwrap();
    assert_eq!(s.stuck_check_minutes().unwrap(), STUCK_CHECK_DEFAULT);
}
