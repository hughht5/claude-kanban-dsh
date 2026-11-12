// Card #195: the Plan review column between Planning and In progress, and the skipPlanReview flow
// option (three-way on the card, a default in Settings, off).

use kanban_store::{page_default, Patch, Store, COLUMNS, PAGE_SETTINGS, PLAN_REVIEW, SCHEMA_VERSION, SKIP_PLAN_REVIEW_KEY, SKIP_PLAN_REVIEW_RULE_KEY};
use rusqlite::Connection;
use serde_json::{json, Value};

fn patch(v: Value) -> Patch {
    v.as_object().cloned().unwrap()
}

fn board() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("board.db")).unwrap();
    (dir, s)
}

fn order(path: &std::path::Path) -> Vec<(String, i64)> {
    let c = Connection::open(path).unwrap();
    let mut q = c.prepare("SELECT name, position FROM columns ORDER BY position, name").unwrap();
    let rows: Vec<(String, i64)> = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect();
    rows
}

fn fields(path: &std::path::Path, id: &str) -> Value {
    let c = Connection::open(path).unwrap();
    let t: String = c.query_row("SELECT fields FROM cards WHERE id = ?1", [id], |r| r.get(0)).unwrap();
    serde_json::from_str(&t).unwrap()
}

#[test]
fn the_column_sits_between_planning_and_in_progress_and_runs_no_session() {
    assert_eq!(&COLUMNS[2..5], ["Planning", PLAN_REVIEW, "In progress"]);
    let (_d, s) = board();
    let names: Vec<String> = s.columns().unwrap().into_iter().map(|c| c.name).collect();
    assert_eq!(names, COLUMNS);
    let c = s.column(PLAN_REVIEW).unwrap().unwrap();
    assert_eq!((c.mode.as_str(), c.auto_spawn, c.on_enter.as_str()), ("none", false, "suspend"));
    assert!(c.prompt_text.is_none(), "nothing is typed into a session there");
    let d = s.column_default(PLAN_REVIEW).unwrap();
    assert_eq!((d.mode.as_str(), d.on_enter.as_str()), ("none", "suspend"));
}

#[test]
fn migration_18_adds_the_column_with_a_backup_changes_no_card_and_runs_twice() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    let id = {
        let mut s = Store::open(&path).unwrap();
        s.create(None, patch(json!({ "title": "T", "column": "In progress" })), "test").unwrap().id
    };
    // Back to the columns as version 17 had them: no Plan review row (renamed out of the way and
    // put last, since nothing is deleted), the later columns one place up.
    {
        let c = Connection::open(&path).unwrap();
        c.execute_batch(
            "UPDATE columns SET name = 'gone', position = 99 WHERE name = 'Plan review';
             UPDATE columns SET position = position - 1 WHERE position > 3 AND position < 99;",
        )
        .unwrap();
        c.pragma_update(None, "user_version", 17).unwrap();
    }
    let before = fields(&path, &id);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
    assert!(
        std::fs::read_dir(s.backups_dir()).unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains("pre-migration")),
        "a backup first"
    );
    drop(s);
    let mut want: Vec<(String, i64)> = ["Backlog", "Blocked", "Planning", PLAN_REVIEW, "In progress", "Human test", "Ship it", "Deploy complete", "Done"]
        .iter()
        .enumerate()
        .map(|(i, n)| (n.to_string(), i as i64))
        .collect();
    want.push(("gone".to_string(), 100)); // the stand-in row moves up with the rest
    assert_eq!(order(&path), want);
    assert_eq!(before, fields(&path, &id), "no card changes");
    // Run again: nothing moves.
    Connection::open(&path).unwrap().pragma_update(None, "user_version", 17).unwrap();
    drop(Store::open(&path).unwrap());
    assert_eq!(order(&path), want, "idempotent");
}

#[test]
fn an_older_build_opens_a_board_with_a_card_in_plan_review() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    {
        let mut s = Store::open(&path).unwrap();
        let c = s.create(None, patch(json!({ "title": "T", "column": "Planning" })), "test").unwrap();
        s.move_by_hugh(&c.id, c.version, PLAN_REVIEW, Patch::new()).unwrap();
    }
    Connection::open(&path).unwrap().pragma_update(None, "user_version", 99).unwrap();
    let s = Store::open(&path).unwrap();
    assert_eq!(s.list(false).unwrap()[0].column, PLAN_REVIEW);
}

#[test]
fn skip_plan_review_is_three_way_with_a_default_off() {
    let (_d, mut s) = board();
    assert!(PAGE_SETTINGS.contains(&SKIP_PLAN_REVIEW_RULE_KEY));
    assert_eq!(page_default(SKIP_PLAN_REVIEW_RULE_KEY).unwrap(), "off");
    assert!(s.set_page_setting(SKIP_PLAN_REVIEW_RULE_KEY, "maybe").is_err());
    let c = s.create(None, patch(json!({ "title": "T", "skipPlanReview": true })), "Hugh (board)").unwrap();
    assert_eq!(c.fields.get(SKIP_PLAN_REVIEW_KEY), Some(&json!(true)));
    assert!(s.skips_plan_review(&c));
    let c = s.update(&c.id, c.version, &patch(json!({ "skipPlanReview": null })), "Hugh (board)").unwrap();
    assert!(c.fields.get(SKIP_PLAN_REVIEW_KEY).is_none() && !s.skips_plan_review(&c));
    s.set_page_setting(SKIP_PLAN_REVIEW_RULE_KEY, "on").unwrap();
    assert!(s.skips_plan_review(&c), "follows the default");
    let c = s.update(&c.id, c.version, &patch(json!({ "skipPlanReview": false })), "Hugh (board)").unwrap();
    assert!(!s.skips_plan_review(&c), "the card's own choice wins");
    assert!(s.update(&c.id, c.version, &patch(json!({ "skipPlanReview": "on" })), "Hugh (board)").is_err());
    let f = s.flow_of(&c);
    assert_eq!(f["skipPlanReview"], json!({ "value": false, "from": "card", "default": true }));
}

#[test]
fn a_move_from_planning_into_the_build_stops_in_plan_review() {
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T", "column": "Planning" })), "test").unwrap();
    for to in ["In progress", "Human test", "Deploy complete"] {
        assert!(s.plan_review_stop(&c, to).is_some(), "{to}");
    }
    for to in ["Backlog", "Blocked", PLAN_REVIEW, "Done"] {
        assert!(s.plan_review_stop(&c, to).is_none(), "{to}");
    }
    // Only from Planning: Backlog straight to In progress (skip Planning) skips it too.
    let b = s.create(None, patch(json!({ "title": "B" })), "test").unwrap();
    assert!(s.plan_review_stop(&b, "In progress").is_none());
    // The card's skip, the default, and Take it to each let it through.
    let own = s.create(None, patch(json!({ "title": "O", "column": "Planning", "skipPlanReview": true })), "Hugh (board)").unwrap();
    assert!(s.plan_review_stop(&own, "In progress").is_none());
    let auto = s.create(None, patch(json!({ "title": "A", "column": "Planning", "autoTo": "Human test" })), "Hugh (board)").unwrap();
    assert!(s.plan_review_stop(&auto, "In progress").is_none(), "Take it to approves the plan");
    s.set_page_setting(SKIP_PLAN_REVIEW_RULE_KEY, "on").unwrap();
    assert!(s.plan_review_stop(&c, "In progress").is_none());
    s.set_page_setting(SKIP_PLAN_REVIEW_RULE_KEY, "off").unwrap();
    // A stack's child never stops: its lead's approval covers it.
    let lead = s.create(None, patch(json!({ "title": "L", "column": "Planning" })), "test").unwrap();
    let kid = s.create(None, patch(json!({ "title": "K", "column": "Planning" })), "test").unwrap();
    s.stack_add(lead.number, kid.number, "test").unwrap();
    let kid = s.get(&kid.id).unwrap();
    assert!(s.plan_review_stop(&kid, "In progress").is_none());
    assert!(s.plan_review_stop(&s.get(&lead.id).unwrap(), "In progress").is_some(), "the lead does");
}

#[test]
fn hughs_move_out_of_plan_review_approves_or_sends_back() {
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T", "column": "Planning" })), "test").unwrap();
    let c = s.move_by_hugh(&c.id, c.version, PLAN_REVIEW, Patch::new()).unwrap().card;
    let back = s.move_by_hugh(&c.id, c.version, "Planning", Patch::new()).unwrap();
    assert!(back.plan, "back to Planning is a fresh plan request");
    let c = s.move_by_hugh(&back.card.id, back.card.version, PLAN_REVIEW, Patch::new()).unwrap().card;
    let on = s.move_by_hugh(&c.id, c.version, "In progress", Patch::new()).unwrap();
    assert_eq!(on.card.column, "In progress");
}
