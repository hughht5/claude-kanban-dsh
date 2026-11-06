// Phase 1 store tests: ACCEPTANCE.md 1.4 to 1.15.

use kanban_store::{looks_secret, Error, Patch, Store};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn patch(v: Value) -> Patch {
    v.as_object().cloned().unwrap()
}

fn temp_board() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    (dir, path)
}

fn write_doc(dir: &Path, id: &str, doc: Value) {
    std::fs::write(dir.join(format!("{id}.json")), serde_json::to_string_pretty(&doc).unwrap()).unwrap();
}

fn sample_export(dir: &Path) {
    write_doc(dir, "a-1", json!({
        "title": "First", "number": 3, "column": "In progress", "summary": "s",
        "log": [{ "at": "2026-11-03T10:00:00Z", "text": "newer" }, { "at": "2026-11-03T09:00:00Z", "text": "older" }],
        "watcherNotifiedAt": "2026-11-03T09:30:00Z", "order": 2, "orderCol": "In progress"
    }));
    write_doc(dir, "b-2", json!({
        "title": "Second", "number": 20, "column": "Done", "archived": true,
        "archivedAt": "2026-11-03T11:00:00Z", "archivedBy": "Hugh (board)"
    }));
}

#[test]
fn t1_4_parallel_creates_get_distinct_numbers() {
    let (_dir, path) = temp_board();
    Store::open(&path).unwrap();
    let handles: Vec<_> = (0..50)
        .map(|i| {
            let path = path.clone();
            std::thread::spawn(move || {
                let mut s = Store::open(&path).unwrap();
                s.create(None, patch(json!({ "title": format!("t{i}") })), "test").unwrap().number
            })
        })
        .collect();
    let mut numbers: Vec<i64> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    numbers.sort();
    assert_eq!(numbers, (1..=50).collect::<Vec<_>>());
    assert_eq!(Store::open(&path).unwrap().next_number().unwrap(), 51);
}

#[test]
fn t1_5_import_seeds_next_number_and_numbers_are_not_reused() {
    let (dir, path) = temp_board();
    let export = dir.path().join("export");
    std::fs::create_dir(&export).unwrap();
    sample_export(&export);
    let mut s = Store::open(&path).unwrap();
    let r = s.import_dir(&export, false).unwrap();
    assert_eq!(r.next_number, 21, "highest number 20, archived card included");
    let c = s.create(None, patch(json!({ "title": "new" })), "test").unwrap();
    assert_eq!(c.number, 21);
    let b = s.get("b-2").unwrap();
    assert!(b.archived);
    assert_eq!(s.create(None, patch(json!({ "title": "next" })), "test").unwrap().number, 22);
}

#[test]
fn t1_6_stale_write_is_refused_and_changes_nothing() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let c = s.create(Some("x"), patch(json!({ "title": "t", "summary": "one" })), "test").unwrap();
    s.update("x", c.version, &patch(json!({ "summary": "two" })), "a").unwrap();
    match s.update("x", c.version, &patch(json!({ "summary": "three" })), "b") {
        Err(Error::Conflict { yours, current, .. }) => {
            assert_eq!(yours, 1);
            assert_eq!(current, 2);
        }
        other => panic!("expected a conflict, got {other:?}"),
    }
    assert_eq!(s.get("x").unwrap().fields["summary"], json!("two"));
}

#[test]
fn t1_7_every_write_adds_a_revision() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let c = s.create(Some("x"), patch(json!({ "title": "t" })), "test").unwrap();
    let c = s.update("x", c.version, &patch(json!({ "summary": "a" })), "t").unwrap();
    let c = s.update("x", c.version, &patch(json!({ "nextStep": "b" })), "t").unwrap();
    s.move_by_hugh("x", c.version, "Blocked", Patch::new()).unwrap();
    assert_eq!(s.revision_count("x").unwrap(), 4);
}

#[test]
fn t1_8_source_has_no_delete_statement() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let re = regex::Regex::new(r"(?i)\bdelete\s+from\b").unwrap();
    let mut hits = Vec::new();
    let mut stack = vec![root.join("crates"), root.join("src-tauri")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if p.is_dir() {
                if name != "target" && name != "node_modules" {
                    stack.push(p);
                }
            } else if (name.ends_with(".rs") || name.ends_with(".sql")) && !p.ends_with("tests/store.rs") {
                let text = std::fs::read_to_string(&p).unwrap_or_default();
                if re.is_match(&text) {
                    hits.push(p.display().to_string());
                }
            }
        }
    }
    assert!(hits.is_empty(), "DELETE FROM found in {hits:?}");
}

#[test]
fn t1_8_database_refuses_deletes_and_log_updates() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    s.create(Some("x"), patch(json!({ "title": "t" })), "test").unwrap();
    s.add_log("x", "hello", "test").unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    let del = ["cards", "card_log", "card_revisions"]
        .map(|t| conn.execute(&format!("{} FROM {t}", "DELETE"), []).is_err());
    assert_eq!(del, [true, true, true]);
    assert!(conn.execute("UPDATE card_log SET text = 'changed'", []).is_err());
    assert!(conn.execute("UPDATE card_revisions SET by = 'changed'", []).is_err());
}

#[test]
fn t1_9_secret_filter() {
    for bad in [
        "http://127.0.0.1:5173/login.html?ct=abc",
        "customToken here",
        "idToken: x",
        "eyJhbGciOiJIUzI1NiJ9",
        "sk_live_123",
        "sk_test_123",
        "the password is",
    ] {
        assert!(looks_secret(bad), "{bad} should be refused");
    }
    assert!(!looks_secret("http://127.0.0.1:5173/login.html?as=admin&next=/o/x"));

    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let c = s.create(Some("x"), patch(json!({ "title": "t" })), "test").unwrap();
    let bad = patch(json!({ "logins": [{ "label": "Admin", "url": "http://x/?ct=as-abc" }] }));
    assert!(matches!(s.update("x", c.version, &bad, "t"), Err(Error::Secret(_))));
    assert!(matches!(
        s.create(None, patch(json!({ "title": "my password" })), "t"),
        Err(Error::Secret(_))
    ));
}

#[test]
fn t1_10_archive_and_restore_keep_the_row() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let c = s.create(Some("x"), patch(json!({ "title": "t" })), "test").unwrap();
    let c = s.archive("x", c.version, true, "Hugh (board)").unwrap();
    assert!(c.archived);
    assert!(c.fields.contains_key("archivedAt"));
    assert_eq!(s.list(false).unwrap().len(), 0);
    assert_eq!(s.list(true).unwrap().len(), 1);
    let c = s.archive("x", c.version, false, "Hugh (board)").unwrap();
    assert!(!c.archived);
    assert!(!c.fields.contains_key("archivedAt"));
    let texts: Vec<String> = s.log("x").unwrap().into_iter().map(|e| e.text).collect();
    assert_eq!(texts, ["Hugh restored it", "Hugh archived it"]);
}

#[test]
fn t1_11_wal_and_backup_before_migration() {
    let (dir, path) = temp_board();
    let s = Store::open(&path).unwrap();
    assert_eq!(s.journal_mode().unwrap().to_lowercase(), "wal");
    assert_eq!(s.schema_version().unwrap(), kanban_store::SCHEMA_VERSION);
    let out = s.backup("manual").unwrap();
    assert!(out.exists());
    assert!(dir.path().join("backups").is_dir());
}

#[test]
fn t1_12_and_1_13_import_then_check_is_clean() {
    let (dir, path) = temp_board();
    let export = dir.path().join("export");
    std::fs::create_dir(&export).unwrap();
    sample_export(&export);
    let mut s = Store::open(&path).unwrap();
    let r = s.import_dir(&export, false).unwrap();
    assert_eq!(r.imported, 2);
    let a = s.get("a-1").unwrap();
    assert_eq!((a.number, a.column.as_str(), a.order), (3, "In progress", Some(2.0)));
    assert_eq!(s.log("a-1").unwrap()[0].text, "newer");
    let report = s.check_dir(&export).unwrap();
    assert!(report.clean(), "{:#?}", report.diffs);
    assert_eq!((report.cards, report.archived), (2, 1));
    assert_eq!(report.kept_unused.get("watcherNotifiedAt"), Some(&1));
}

#[test]
fn t1_13_check_reports_a_changed_field() {
    let (dir, path) = temp_board();
    let export = dir.path().join("export");
    std::fs::create_dir(&export).unwrap();
    sample_export(&export);
    let mut s = Store::open(&path).unwrap();
    s.import_dir(&export, false).unwrap();
    let a = s.get("a-1").unwrap();
    s.update("a-1", a.version, &patch(json!({ "summary": "changed" })), "t").unwrap();
    let report = s.check_dir(&export).unwrap();
    let fields: Vec<&str> = report.diffs.iter().map(|d| d.field.as_str()).collect();
    assert!(fields.contains(&"summary"));
    assert!(!report.clean());
}

#[test]
fn t1_14_a_bad_card_fails_the_whole_import() {
    let (dir, path) = temp_board();
    let export = dir.path().join("export");
    std::fs::create_dir(&export).unwrap();
    sample_export(&export);
    write_doc(&export, "c-3", json!({ "title": "Bad", "number": 4, "column": "Nowhere" }));
    let mut s = Store::open(&path).unwrap();
    let err = s.import_dir(&export, false).unwrap_err().to_string();
    assert!(err.contains("c-3"), "{err}");
    assert_eq!(s.list(true).unwrap().len(), 0, "nothing written");
}

#[test]
fn t1_15_second_import_needs_replace_and_keeps_history() {
    let (dir, path) = temp_board();
    let export = dir.path().join("export");
    std::fs::create_dir(&export).unwrap();
    sample_export(&export);
    let mut s = Store::open(&path).unwrap();
    s.import_dir(&export, false).unwrap();
    assert!(s.import_dir(&export, false).is_err());
    let r = s.import_dir(&export, true).unwrap();
    assert_eq!(r.replaced, 2);
    assert_eq!(s.revision_count("a-1").unwrap(), 2);
    assert_eq!(s.log("a-1").unwrap().len(), 2, "log lines are not duplicated");
}

#[test]
fn t1_20_move_rules_for_ship_it_and_planning() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let c = s.create(Some("x"), patch(json!({ "title": "t", "column": "Human test" })), "test").unwrap();
    let m = s.move_by_hugh("x", c.version, "Ship it", Patch::new()).unwrap();
    assert!(m.ship);
    assert_eq!(m.card.fields["ship"]["state"], json!("requested"));
    assert!(m.card.fields.contains_key("shipRequestedAt"));
    assert_eq!(m.card.fields["movedByHand"], json!(true));
    let m = s.move_by_hugh("x", m.card.version, "Human test", Patch::new()).unwrap();
    assert_eq!(m.card.fields["ship"]["state"], json!("cancelled"));
    assert!(!m.card.fields.contains_key("shipRequestedAt"));
    let m = s.move_by_hugh("x", m.card.version, "Planning", Patch::new()).unwrap();
    assert!(m.plan);
    assert_eq!(m.card.fields["plan"]["state"], json!("requested"));
    let m = s.move_by_hugh("x", m.card.version, "Backlog", Patch::new()).unwrap();
    assert_eq!(m.card.fields["plan"]["state"], json!("cancelled"));
    let texts: Vec<String> = s.log("x").unwrap().into_iter().map(|e| e.text).collect();
    assert!(texts.contains(&"Hugh moved it from Human test to Ship it".to_string()));
    assert!(texts.contains(&"Ship requested".to_string()));
}

#[test]
fn t3_columns_are_seeded_and_the_queue_keeps_one_of_each_kind() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let p = s.column("Planning").unwrap().unwrap();
    assert!(p.auto_spawn && p.mode == "immediate");
    assert!(p.prompt_text.unwrap().contains("{n}"));
    assert_eq!(s.column("Done").unwrap().unwrap().on_enter, "suspend");
    assert_eq!(s.column("Blocked").unwrap().unwrap().model.as_deref(), Some("claude-sonnet-5-5"));
    s.create(Some("x"), patch(json!({ "title": "t" })), "test").unwrap();
    s.queue("x", "message", "one").unwrap();
    s.queue("x", "message", "two").unwrap();
    s.queue("x", "move", "Blocked").unwrap();
    s.queue("x", "move", "Done").unwrap();
    assert_eq!(s.waiting("x", "message").unwrap().unwrap().payload, "one

two");
    let m = s.waiting("x", "move").unwrap().unwrap();
    assert_eq!(m.payload, "Done");
    s.done(m.id, "sent").unwrap();
    assert!(s.waiting("x", "move").unwrap().is_none());
}

fn session(s: &mut Store, id: &str, card: &str) {
    s.add_session(&kanban_store::SessionRow {
        id: id.into(),
        card_id: card.into(),
        claude_session_id: format!("convo-{id}"),
        account: "main".into(),
        cwd: "C:\\x".into(),
        model: None,
        effort: None,
        started_at: "2026-11-03T10:00:00Z".into(),
        status: None,
    })
    .unwrap();
}

fn event_count(path: &Path, session: &str) -> i64 {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.query_row("SELECT COUNT(*) FROM session_events WHERE session_id = ?1", [session], |r| r.get(0)).unwrap()
}

#[test]
fn t4_1_state_is_recorded_only_when_it_changes() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    s.create(Some("a"), patch(json!({ "title": "t" })), "test").unwrap();
    session(&mut s, "r1", "a");
    assert!(s.session_state("r1", "idle").unwrap());
    assert!(!s.session_state("r1", "idle").unwrap());
    assert!(s.session_state("r1", "working").unwrap());
    assert!(!s.session_state("r1", "working").unwrap());
    assert_eq!(s.last_state("r1").unwrap().as_deref(), Some("working"));
    // The start row, then idle and working.
    assert_eq!(event_count(&path, "r1"), 3);
    // State rows keep the session running.
    assert_eq!(s.running_sessions().unwrap(), vec!["r1".to_string()]);
    // Once stopped, a late hook adds nothing.
    s.session_event("r1", "suspended", Some("mid-turn (working)")).unwrap();
    assert!(!s.session_state("r1", "idle").unwrap());
    assert_eq!(event_count(&path, "r1"), 4);
}

#[test]
fn t4_1_to_4_3_which_sessions_were_mid_turn_at_the_last_stop() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    for c in ["a", "b", "c", "d", "e"] {
        s.create(Some(c), patch(json!({ "title": c })), "test").unwrap();
    }
    // The first run: nothing has stopped yet.
    assert!(s.begin_run().unwrap().is_empty());
    // a: quit mid-turn. b: quit idle. c: crashed while working. d: crashed while idle.
    session(&mut s, "a1", "a");
    session(&mut s, "b1", "b");
    session(&mut s, "c1", "c");
    session(&mut s, "d1", "d");
    s.session_event("a1", "suspended", Some("mid-turn (working)")).unwrap();
    s.session_event("b1", "suspended", Some("idle")).unwrap();
    s.session_state("c1", "idle").unwrap();
    s.session_state("c1", "working").unwrap();
    s.session_state("d1", "working").unwrap();
    s.session_state("d1", "idle").unwrap();
    // e: an older session went mid-turn but the card has a newer, idle one.
    session(&mut s, "e1", "e");
    s.session_event("e1", "suspended", Some("mid-turn (permission)")).unwrap();
    session(&mut s, "e2", "e");
    s.session_event("e2", "suspended", Some("idle")).unwrap();

    let mid: Vec<String> = s.begin_run().unwrap().into_iter().map(|r| r.id).collect();
    assert_eq!(mid, vec!["a1".to_string(), "c1".to_string()]);
    assert!(s.running_sessions().unwrap().is_empty());
    assert_eq!(s.latest_session("c").unwrap().unwrap().status.as_deref(), Some("orphaned"));
    assert_eq!(s.latest_session("d").unwrap().unwrap().status.as_deref(), Some("orphaned"));

    // The next start: a1 was resumed (a new session on card a) and quit idle. c1 was never
    // resumed, but its stop belongs to the run before, so it isn't picked again.
    session(&mut s, "a2", "a");
    s.session_event("a2", "suspended", Some("idle")).unwrap();
    assert!(s.begin_run().unwrap().is_empty());
}

#[test]
fn resume_offer_lists_the_sessions_idle_at_the_last_quit_until_answered() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    for c in ["a", "b", "c", "d", "e", "f", "g"] {
        s.create(Some(c), patch(json!({ "title": c })), "test").unwrap();
    }
    // An idle stop from a run before the last one is history, not offered. An older build
    // ran in between: it moved the run marker past it, but never wrote the offer's setting.
    session(&mut s, "old", "g");
    s.session_event("old", "suspended", Some("idle")).unwrap();
    // Two events so far: the old session's start and its stop.
    s.set_setting("app.run_started_seq", "2").unwrap();
    assert!(s.setting(kanban_store::RESUME_OFFER_KEY).unwrap().is_none());
    assert!(s.begin_run().unwrap().is_empty());
    assert!(s.resume_offer().unwrap().is_empty());

    // a: quit idle. b: quit mid-turn (resumes by itself). c: stopped by Hugh or Done.
    // d: crashed while idle. e: quit idle, but an older session; its card has a newer one
    // stopped by Hugh. f: copied from another board.
    for (id, card) in [("a1", "a"), ("b1", "b"), ("c1", "c"), ("d1", "d"), ("e1", "e"), ("f1", "f")] {
        session(&mut s, id, card);
    }
    s.session_event("a1", "suspended", Some("idle")).unwrap();
    s.session_event("b1", "suspended", Some("mid-turn (working)")).unwrap();
    s.session_event("c1", "suspended", None).unwrap();
    s.session_state("d1", "idle").unwrap();
    s.session_event("e1", "suspended", Some("idle")).unwrap();
    session(&mut s, "e2", "e");
    s.session_event("e2", "suspended", None).unwrap();
    s.session_event("f1", "suspended", Some("copied: running on the board this was copied from; not resumed here")).unwrap();

    let mid: Vec<String> = s.begin_run().unwrap().into_iter().map(|r| r.id).collect();
    assert_eq!(mid, vec!["b1".to_string()]);
    let offer = |s: &Store| s.resume_offer().unwrap().into_iter().map(|r| r.id).collect::<Vec<_>>();
    assert_eq!(offer(&s), vec!["a1".to_string(), "d1".to_string()]);

    // Unanswered, the offer survives another start, and a card resumed by hand leaves it.
    assert!(s.begin_run().unwrap().is_empty());
    assert_eq!(offer(&s), vec!["a1".to_string(), "d1".to_string()]);
    session(&mut s, "d2", "d");
    assert_eq!(offer(&s), vec!["a1".to_string()]);

    // Answered: nothing so far is offered again; the next quit offers its own.
    s.answer_resume_offer().unwrap();
    assert!(offer(&s).is_empty());
    s.session_event("d2", "suspended", Some("idle")).unwrap();
    s.begin_run().unwrap();
    assert_eq!(offer(&s), vec!["d2".to_string()]);
}

#[test]
fn t4_7_backups_keep_the_newest_30_and_one_a_day() {
    let (dir, path) = temp_board();
    let s = Store::open(&path).unwrap();
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    // 35 old backups, plus a file that isn't one.
    // Names only need to sort before today's stamp.
    for d in 1..=35 {
        std::fs::write(backups.join(format!("202601{d:02}T030000.000Z-daily.db")), b"x").unwrap();
    }
    std::fs::write(backups.join("notes.txt"), b"keep me").unwrap();
    let first = s.backup_if_none_today(kanban_store::KEEP_BACKUPS).unwrap();
    assert!(first.as_ref().unwrap().exists());
    let dbs = |d: &Path| std::fs::read_dir(d).unwrap().flatten().filter(|e| e.path().extension().map_or(false, |x| x == "db")).count();
    assert_eq!(dbs(&backups), 30);
    assert!(backups.join("notes.txt").exists());
    // The six oldest went; today's, the newest, stayed.
    assert!(!backups.join("20260106T030000.000Z-daily.db").exists());
    assert!(backups.join("20260107T030000.000Z-daily.db").exists());
    let today = first.unwrap();
    assert!(today.exists());
    // A second start on the same day takes none.
    assert!(s.backup_if_none_today(30).unwrap().is_none());
    assert_eq!(dbs(&backups), 30);
    // Pruning removes the oldest by name and returns them.
    let gone = kanban_store::prune_backups(&backups, 2).unwrap();
    assert_eq!(gone.len(), 28);
    assert!(today.exists() && backups.join("20260135T030000.000Z-daily.db").exists());
    assert_eq!(dbs(&backups), 2);
}

#[test]
fn prompts_keep_card_writes_in_the_app() {
    let (_dir, path) = temp_board();
    let s = Store::open(&path).unwrap();
    // The column prompts point at the board's own prompt files, which carry the rules.
    let file = |f: &str| std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../prompts").join(f)).unwrap();
    assert!(s.column("Planning").unwrap().unwrap().prompt_text.unwrap().contains("planning.md"));
    let plan = file("planning.md");
    assert!(plan.contains("kanban_*") && plan.contains("Never move a card into Ship it"), "{plan}");
    assert!(!plan.contains("SKILL.md"));
    let ship = file("ship-it.md");
    assert!(ship.contains("kanban_ship_result") && !ship.contains("ArtifactData"));
}

// The test channel: `kb copy-board`.

#[test]
fn copy_board_snapshots_a_board_in_use_and_refuses_to_overwrite() {
    let (dir, live) = temp_board();
    let mut s = Store::open(&live).unwrap();
    for c in ["a", "b", "c"] {
        s.create(Some(c), patch(json!({ "title": c })), "test").unwrap();
    }
    s.begin_run().unwrap();
    // a: stopped mid-turn just before the copy. c: still running and working.
    session(&mut s, "a1", "a");
    s.session_event("a1", "suspended", Some("mid-turn (working)")).unwrap();
    session(&mut s, "c1", "c");
    s.session_state("c1", "working").unwrap();
    // b: waiting under the cap, and its last session quit idle.
    s.queue_start("b", "new", "hugh").unwrap();
    session(&mut s, "b1", "b");
    s.session_event("b1", "suspended", Some("idle")).unwrap();

    // Copied while the live store is still open, as with the app running.
    let to = dir.path().join("test").join("board.db");
    let r = kanban_store::copy_board(&live, &to, false).unwrap();
    assert_eq!(r.cards, 3);
    assert_eq!(r.sessions_detached, 1);
    assert!(r.moved_aside.is_none());

    // The copy has the cards, and a start of the copy resumes nothing.
    let mut copy = Store::open(&to).unwrap();
    assert_eq!(copy.list(true).unwrap().len(), 3);
    assert!(copy.running_sessions().unwrap().is_empty());
    assert!(copy.begin_run().unwrap().is_empty());
    assert!(copy.resume_offer().unwrap().is_empty(), "the copy offers nothing that ran on the source");
    assert!(copy.start_queue().unwrap().is_empty(), "the copy starts nothing that waited on the source");
    drop(copy);
    // The source is untouched.
    assert_eq!(s.running_sessions().unwrap(), vec!["c1".to_string()]);

    // A second copy onto it is refused, and leaves the file as it was.
    let before = std::fs::read(&to).unwrap();
    let err = kanban_store::copy_board(&live, &to, false).unwrap_err();
    assert!(err.to_string().contains("--replace"), "{err}");
    assert_eq!(std::fs::read(&to).unwrap(), before);
    assert!(!to.with_extension("db.copying").exists());

    // With replace, the old copy is moved into backups, not lost.
    s.create(Some("d"), patch(json!({ "title": "d" })), "test").unwrap();
    let r = kanban_store::copy_board(&live, &to, true).unwrap();
    assert_eq!(r.cards, 4);
    let aside = r.moved_aside.unwrap();
    assert!(aside.starts_with(to.parent().unwrap().join("backups")));
    assert_eq!(Store::open(&aside).unwrap().list(true).unwrap().len(), 3);

    // Copying a board onto itself, or from nowhere, is refused.
    assert!(kanban_store::copy_board(&live, &live, true).is_err());
    assert!(kanban_store::copy_board(&dir.path().join("none.db"), &dir.path().join("x.db"), false).is_err());
}

#[test]
fn t5_deploy_complete_column_order_and_prompts_without_the_artifact() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let d = s.column("Deploy complete").unwrap().unwrap();
    assert_eq!(d.mode, "immediate");
    assert!(!d.auto_spawn);
    assert_eq!(d.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(d.effort.as_deref(), Some("medium"));
    assert!(d.prompt_text.unwrap().contains("deploy-complete.md"));
    let ship = s.column("Ship it").unwrap().unwrap().prompt_text.unwrap();
    assert!(ship.contains("ship-it.md") && !ship.contains("deploy-complete.md"));
    let conn = rusqlite::Connection::open(&path).unwrap();
    let names: Vec<String> = conn
        .prepare("SELECT name FROM columns ORDER BY position").unwrap()
        .query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
    assert_eq!(names, kanban_store::COLUMNS);
    let prompts: Vec<String> = conn
        .prepare("SELECT COALESCE(prompt_text, '') FROM columns").unwrap()
        .query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
    for p in &prompts {
        let l = p.to_lowercase();
        assert!(!l.contains("artifact") && !l.contains("work board") && !l.contains("skill"), "{p}");
    }
    let c = s.create(Some("x"), patch(json!({ "title": "t" })), "test").unwrap();
    s.update("x", c.version, &patch(json!({ "column": "Deploy complete" })), "test").unwrap();
    assert_eq!(s.get("x").unwrap().column, "Deploy complete");
}

#[test]
fn column_prompts_are_the_boards_own() {
    let (_dir, path) = temp_board();
    let s = Store::open(&path).unwrap();
    for (name, file) in [("Planning", "planning.md"), ("In progress", "in-progress.md"), ("Ship it", "ship-it.md"), ("Deploy complete", "deploy-complete.md")] {
        let p = s.column(name).unwrap().unwrap().prompt_text.unwrap();
        // Migration 13 (card #114) names the folder as {prompts}; the app fills it in.
        assert!(p.contains(&format!(r"{{prompts}}\{file}")), "{name}: {p}");
        let low = p.to_lowercase();
        assert!(!low.contains("kangentic") && !low.contains(r".claude\prompts\v2"), "{name}: {p}");
    }
    // Planning still opens with who the session is, so a new session gets no second header.
    assert!(s.column("Planning").unwrap().unwrap().prompt_text.unwrap().starts_with("You are the session for card #{n}"));
    // The prompt files themselves never send a session to Kangentic.
    for f in ["planning.md", "in-progress.md", "ship-it.md", "deploy-complete.md"] {
        let t = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../prompts").join(f)).unwrap();
        for line in t.lines().filter(|l| l.to_lowercase().contains("kangentic")) {
            assert!(line.contains("no Kangentic") || line.contains("never Kangentic"), "{f}: {line}");
        }
    }
}

// Card #71: labels on cards.
#[test]
fn labels_are_cleaned_and_checked() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    // On create: trimmed, inner spaces collapsed, empties dropped, repeats (any case) kept once.
    let c = s.create(Some("x"), patch(json!({ "title": "t", "labels": [" BUG ", "bug", "", "Jack  feedback"] })), "test").unwrap();
    assert_eq!(c.fields["labels"], json!(["BUG", "Jack feedback"]));
    // A comma-separated string, as `kb update --labels BUG,RCS` sends.
    let c = s.update("x", c.version, &patch(json!({ "labels": "RCS, ABC,rcs" })), "a").unwrap();
    assert_eq!(c.fields["labels"], json!(["RCS", "ABC"]));
    // The same labels again change nothing (no new version).
    let same = s.update("x", c.version, &patch(json!({ "labels": ["RCS", "ABC"] })), "a").unwrap();
    assert_eq!(same.version, c.version);
    // Too long, too many and the wrong shape are refused, and the card is left as it was.
    for bad in [json!(["x".repeat(25)]), json!((0..9).map(|i| format!("L{i}")).collect::<Vec<_>>()), json!([1, 2]), json!({ "a": 1 })] {
        match s.update("x", c.version, &patch(json!({ "labels": bad })), "a") {
            Err(Error::Invalid(_)) => {}
            other => panic!("expected the labels to be refused, got {other:?}"),
        }
    }
    assert_eq!(s.get("x").unwrap().fields["labels"], json!(["RCS", "ABC"]));
    // 24 characters and 8 labels are allowed.
    let c = s.update("x", c.version, &patch(json!({ "labels": ["y".repeat(24)] })), "a").unwrap();
    let c = s.update("x", c.version, &patch(json!({ "labels": (0..8).map(|i| format!("L{i}")).collect::<Vec<_>>() })), "a").unwrap();
    assert_eq!(c.fields["labels"].as_array().unwrap().len(), 8);
    // An empty list or null clears them: the field goes.
    let c = s.update("x", c.version, &patch(json!({ "labels": [] })), "a").unwrap();
    assert!(c.fields.get("labels").is_none());
    let c = s.update("x", c.version, &patch(json!({ "labels": "BUG" })), "a").unwrap();
    let c = s.update("x", c.version, &patch(json!({ "labels": null })), "a").unwrap();
    assert!(c.fields.get("labels").is_none());
    // A create with only empty labels stores none.
    let e = s.create(Some("y"), patch(json!({ "title": "t", "labels": [" "] })), "test").unwrap();
    assert!(e.fields.get("labels").is_none());
}
