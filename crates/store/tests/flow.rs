// Card #162: a card's flow options. Skip Planning and skip Human test, three-way on the card with a
// default in Settings, and migration 17, which takes the subagent values #151 stamped on every card
// off where they equal the default.

use kanban_store::{page_default, Card, Patch, Store, CRITIC_RULE_KEY, PAGE_SETTINGS, PLAN_SKIPPED, RULE_KEYS, SKIP_HUMAN_TEST_KEY, SKIP_HUMAN_TEST_RULE_KEY, SKIP_PLANNING_KEY, SKIP_PLANNING_RULE_KEY, SUBAGENTS_KEY, TEST_RULE_KEY, UNSTAMP_BY};
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

fn plan_state(c: &Card) -> Option<&str> {
    c.fields.get("plan").and_then(|p| p.get("state")).and_then(Value::as_str)
}

fn log_has(s: &Store, c: &Card, text: &str) -> bool {
    s.log(&c.id).unwrap().iter().any(|l| l.text.contains(text))
}

#[test]
fn the_two_defaults_are_page_settings_off_by_default() {
    let (_d, mut s) = board();
    for key in [SKIP_PLANNING_RULE_KEY, SKIP_HUMAN_TEST_RULE_KEY] {
        assert!(PAGE_SETTINGS.contains(&key) && !RULE_KEYS.contains(&key), "{key}: a page setting, not an on-by-default rule");
        assert_eq!(page_default(key).unwrap(), "off");
        assert_eq!(s.page_setting(key).unwrap(), "off");
        assert!(!s.skip_default(key));
        assert!(s.set_page_setting(key, "maybe").is_err());
        assert_eq!(s.set_page_setting_by(key, "on", "Hugh (Settings)").unwrap(), "on");
        assert!(s.skip_default(key));
        assert_eq!(s.setting_log(key, 5).unwrap().len(), 1, "{key}: a change is logged");
    }
}

#[test]
fn a_skip_is_three_way_on_the_card() {
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T", "skipPlanning": true, "skipHumanTest": null })), "Hugh (board)").unwrap();
    assert_eq!(c.fields.get(SKIP_PLANNING_KEY), Some(&json!(true)));
    assert!(c.fields.get(SKIP_HUMAN_TEST_KEY).is_none(), "null: no field, the default");
    assert!(s.skips_planning(&c) && !s.skips_human_test(&c));
    // The default reaches a card with no choice of its own, and not one with its own.
    s.set_page_setting(SKIP_HUMAN_TEST_RULE_KEY, "on").unwrap();
    assert!(s.skips_human_test(&c));
    let c = s.update(&c.id, c.version, &patch(json!({ "skipHumanTest": false })), "Hugh (board)").unwrap();
    assert!(!s.skips_human_test(&c), "his own false beats the default");
    let c = s.update(&c.id, c.version, &patch(json!({ "skipHumanTest": null, "skipPlanning": null })), "Hugh (board)").unwrap();
    assert!(c.fields.get(SKIP_HUMAN_TEST_KEY).is_none() && c.fields.get(SKIP_PLANNING_KEY).is_none());
    assert!(s.skips_human_test(&c) && !s.skips_planning(&c), "both follow the defaults again");
    // Only true, false or null.
    for bad in [json!("on"), json!(1), json!({})] {
        assert!(s.update(&c.id, c.version, &patch(json!({ "skipPlanning": bad })), "Hugh (board)").is_err(), "{bad}");
        assert!(s.create(None, patch(json!({ "title": "X", "skipHumanTest": bad })), "Hugh (board)").is_err(), "{bad}");
    }
    // A stack's child never skips, whatever it or the default says.
    let lead = s.create(None, patch(json!({ "title": "L" })), "test").unwrap();
    let kid = s.create_in_stack(patch(json!({ "title": "K" })), lead.number, "test").unwrap();
    let kid = s.update(&kid.id, kid.version, &patch(json!({ "skipPlanning": true, "skipHumanTest": true })), "Hugh (board)").unwrap();
    assert!(!s.skips_planning(&kid) && !s.skips_human_test(&kid));
    assert!(s.skips_human_test(&s.get(&lead.id).unwrap()), "its lead follows the default");
}

#[test]
fn the_flow_object_says_each_value_and_whose_it_is() {
    let (_d, mut s) = board();
    s.set_page_setting(CRITIC_RULE_KEY, "off").unwrap();
    let c = s.create(None, patch(json!({ "title": "T", "skipPlanning": true, "subagents": { "tester": false } })), "Hugh (board)").unwrap();
    let f = s.flow_of(&c);
    assert_eq!(f["skipPlanning"], json!({ "value": true, "from": "card", "default": false }));
    assert_eq!(f["skipHumanTest"], json!({ "value": false, "from": "default", "default": false }));
    assert_eq!(f["subagents"]["tester"], json!({ "value": false, "from": "card", "default": true }));
    assert_eq!(f["subagents"]["critic"], json!({ "value": false, "from": "default", "default": false }));
    assert_eq!(f["subagents"]["router"], json!({ "value": true, "from": "default", "default": true }));
    assert_eq!(f["subagents"].as_object().unwrap().len(), 5);
    assert!(f.get("note").is_none());
}

/// Hugh's drag, his Take it to (move_for_hugh) and a card released by its waits all go from
/// Backlog or Blocked straight to In progress when it skips Planning: no plan request.
#[test]
fn a_move_into_planning_from_backlog_or_blocked_goes_to_in_progress() {
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T", "skipPlanning": true })), "Hugh (board)").unwrap();
    let m = s.move_by_hugh(&c.id, c.version, "Planning", Patch::new()).unwrap();
    assert!(m.skipped_planning && !m.plan);
    assert_eq!(m.card.column, "In progress");
    assert_eq!(plan_state(&m.card), Some(PLAN_SKIPPED));
    assert_eq!(m.card.fields["plan"]["whose"], "card");
    assert!(m.card.fields.get("planRequestedAt").is_none());
    assert!(log_has(&s, &m.card, "Skipped Planning: Hugh's choice (this card's own)"));
    assert!(!log_has(&s, &m.card, "Plan requested"));
    assert_eq!(m.card.fields["test"]["state"], "needed", "In progress's own resets ran");
    // The default, on the app's move for Hugh (Add card's Take it to Planning).
    s.set_page_setting(SKIP_PLANNING_RULE_KEY, "on").unwrap();
    let d = s.create(None, patch(json!({ "title": "D" })), "Hugh (board)").unwrap();
    let m = s.move_for_hugh(&d.id, d.version, "Planning", "he added it to be planned now").unwrap();
    assert_eq!(m.card.column, "In progress");
    assert_eq!(m.card.fields["plan"], json!({ "state": "skipped", "at": m.card.fields["plan"]["at"], "whose": "default", "from": "Backlog", "auto": true }));
    assert!(log_has(&s, &m.card, "Skipped Planning: Hugh's choice (the default in Settings)"));
    // From Blocked, by its waits.
    let w = s.create(None, patch(json!({ "title": "W" })), "Hugh (board)").unwrap();
    let w = s.move_by_hugh(&w.id, w.version, "Blocked", Patch::new()).unwrap().card;
    let m = s.move_after_waits(&w.id, w.version, &[d.number]).unwrap();
    assert_eq!(m.card.column, "In progress");
    assert_eq!(m.card.fields["plan"]["waitedOn"], json!([d.number]));
}

#[test]
fn planning_still_plans_where_the_rule_doesnt_reach() {
    let (_d, mut s) = board();
    // A card that says false, with the default on.
    s.set_page_setting(SKIP_PLANNING_RULE_KEY, "on").unwrap();
    let no = s.create(None, patch(json!({ "title": "N", "skipPlanning": false })), "Hugh (board)").unwrap();
    let m = s.move_by_hugh(&no.id, no.version, "Planning", Patch::new()).unwrap();
    assert!(m.plan && !m.skipped_planning && m.card.column == "Planning");
    // Hugh's move into Planning from a later column plans it.
    let c = s.create(None, patch(json!({ "title": "T", "skipPlanning": true })), "Hugh (board)").unwrap();
    let c = s.move_by_hugh(&c.id, c.version, "Planning", Patch::new()).unwrap().card;
    assert_eq!(c.column, "In progress");
    let m = s.move_by_hugh(&c.id, c.version, "Planning", Patch::new()).unwrap();
    assert!(m.plan && m.card.column == "Planning" && plan_state(&m.card) == Some("requested"));
    let h = s.move_by_hugh(&m.card.id, m.card.version, "Human test", Patch::new()).unwrap().card;
    let m = s.move_by_hugh(&h.id, h.version, "Planning", Patch::new()).unwrap();
    assert_eq!(m.card.column, "Planning");
    // A stack's child is planned by its lead's flow.
    let lead = s.create(None, patch(json!({ "title": "L" })), "test").unwrap();
    let kid = s.create_in_stack(patch(json!({ "title": "K", "column": "Backlog" })), lead.number, "test").unwrap();
    let kid = s.update(&kid.id, kid.version, &patch(json!({ "skipPlanning": true })), "Hugh (board)").unwrap();
    let m = s.move_by_hugh(&kid.id, kid.version, "Planning", Patch::new()).unwrap();
    assert_eq!(m.card.column, "Planning");
    // A move straight to In progress is just that.
    let d = s.create(None, patch(json!({ "title": "D", "skipPlanning": true })), "Hugh (board)").unwrap();
    let m = s.move_by_hugh(&d.id, d.version, "In progress", Patch::new()).unwrap();
    assert!(!m.skipped_planning && plan_state(&m.card).is_none());
}

/// How a card arrived in a column: who moved it, and whether its autoTo was cleared since.
#[test]
fn arrival_names_the_mover_and_a_cleared_auto_to() {
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T", "column": "In progress", "autoTo": "Human test" })), "Hugh (board)").unwrap();
    assert!(s.arrival(&c.id, "Human test").unwrap().is_none());
    let c = s.update(&c.id, c.version, &patch(json!({ "column": "Human test" })), "card #1's session").unwrap();
    let a = s.arrival(&c.id, "Human test").unwrap().unwrap();
    assert_eq!((a.by.as_str(), a.auto_cleared), ("card #1's session", false));
    let c = s.update(&c.id, c.version, &patch(json!({ "autoTo": null })), "the app (Hugh's Take it to)").unwrap();
    assert!(s.arrival(&c.id, "Human test").unwrap().unwrap().auto_cleared);
    let c = s.move_by_hugh(&c.id, c.version, "In progress", Patch::new()).unwrap().card;
    let c = s.move_by_hugh(&c.id, c.version, "Human test", Patch::new()).unwrap().card;
    let a = s.arrival(&c.id, "Human test").unwrap().unwrap();
    assert_eq!((a.by.as_str(), a.auto_cleared), ("Hugh (board)", false));
}

/// A board with cards stamped the #151 way, as an older build left them, at version 16.
fn stamped_board(path: &std::path::Path) -> (String, String, String) {
    let stamp = |critic: bool, tester: bool| json!({ "router": true, "critic": critic, "reproducer": true, "reviewer": true, "tester": tester, "merger": true });
    let mut s = Store::open(path).unwrap();
    s.set_page_setting(CRITIC_RULE_KEY, "off").unwrap();
    let a = s.create(None, patch(json!({ "title": "All defaults" })), "test").unwrap();
    let a = s.update(&a.id, a.version, &patch(json!({ "subagents": stamp(false, true) })), "test").unwrap();
    let b = s.create(None, patch(json!({ "title": "Tester off" })), "test").unwrap();
    let b = s.update(&b.id, b.version, &patch(json!({ "subagents": stamp(false, false) })), "test").unwrap();
    let z = s.create(None, patch(json!({ "title": "Archived" })), "test").unwrap();
    let z = s.update(&z.id, z.version, &patch(json!({ "subagents": stamp(false, true) })), "test").unwrap();
    s.archive(&z.id, z.version, true, "test").unwrap();
    drop(s);
    let c = Connection::open(path).unwrap();
    c.pragma_update(None, "user_version", 16).unwrap();
    (a.id, b.id, z.id)
}

#[test]
fn migration_17_unstamps_the_defaults_and_keeps_the_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    let (a, b, z) = stamped_board(&path);
    let revs = |s: &Store, id: &str| s.revision_count(id).unwrap();
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), kanban_store::SCHEMA_VERSION);
    assert!(std::fs::read_dir(s.backups_dir()).unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains("pre-migration")), "a backup first");
    let ca = s.get(&a).unwrap();
    assert!(ca.fields.get(SUBAGENTS_KEY).is_none(), "every value was the default: the field goes");
    assert_eq!(ca.fields["updatedBy"], UNSTAMP_BY);
    let cb = s.get(&b).unwrap();
    assert_eq!(cb.fields[SUBAGENTS_KEY], json!({ "tester": false }), "the one that differs stays");
    let cz = s.get(&z).unwrap();
    assert_eq!(cz.fields[SUBAGENTS_KEY]["router"], true, "an archived card is left as it is");
    // Each change is a revision of its own, and the card follows Settings from now on.
    let (ra, rb, rz) = (revs(&s, &a), revs(&s, &b), revs(&s, &z));
    s.set_page_setting(TEST_RULE_KEY, "off").unwrap();
    assert!(!s.subagent_on(&s.get(&a).unwrap(), "tester"));
    s.set_page_setting(TEST_RULE_KEY, "on").unwrap();
    drop(s);
    // Run again: nothing more changes.
    Connection::open(&path).unwrap().pragma_update(None, "user_version", 16).unwrap();
    let s = Store::open(&path).unwrap();
    assert_eq!((revs(&s, &a), revs(&s, &b), revs(&s, &z)), (ra, rb, rz), "idempotent");
    assert_eq!(s.get(&b).unwrap().fields[SUBAGENTS_KEY], json!({ "tester": false }));
}

/// Migration 17 changes only `subagents` (card #162): a card left in Done unarchived (an older
/// build's, or an import's) stays unarchived and in Done, and every other field is as it was.
#[test]
fn migration_17_changes_nothing_but_the_subagents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    let (a, _b, _z) = stamped_board(&path);
    let c = Connection::open(&path).unwrap();
    c.execute("UPDATE cards SET col = 'Done' WHERE id = ?1", [&a]).unwrap();
    let before: Value = c.query_row("SELECT fields FROM cards WHERE id = ?1", [&a], |r| r.get::<_, String>(0)).map(|t| serde_json::from_str(&t).unwrap()).unwrap();
    drop(c);
    let s = Store::open(&path).unwrap();
    let after = s.get(&a).unwrap();
    assert_eq!(after.column, "Done");
    assert!(!after.archived, "not archived by the migration");
    assert!(after.fields.get(SUBAGENTS_KEY).is_none(), "the stamped defaults went");
    let strip = |v: &serde_json::Map<String, Value>| {
        let mut m = v.clone();
        for k in [SUBAGENTS_KEY, "updatedAt", "updatedBy"] {
            m.remove(k);
        }
        m
    };
    assert_eq!(strip(&after.fields), strip(before.as_object().unwrap()), "nothing else changed");
    assert!(!log_has(&s, &after, "Archived"), "no archive line");
    // Its revision names only the subagents.
    let c = Connection::open(&path).unwrap();
    let changed: String = c.query_row("SELECT changed FROM card_revisions WHERE card_id = ?1 ORDER BY seq DESC LIMIT 1", [&a], |r| r.get(0)).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&changed).unwrap(), json!({ "subagents": null }));
}

/// An older build (one migration short) opens a board at version 17 as it is: a card with no
/// subagents field reads them from Settings, as #151's cards made before it did.
#[test]
fn a_board_at_version_17_opens_on_an_older_build() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    {
        let mut s = Store::open(&path).unwrap();
        s.create(None, patch(json!({ "title": "T", "skipPlanning": true })), "test").unwrap();
    }
    Connection::open(&path).unwrap().pragma_update(None, "user_version", 99).unwrap();
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), 99, "never migrated down");
    assert_eq!(s.list(false).unwrap().len(), 1);
}
