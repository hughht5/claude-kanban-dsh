// Card #151: each card's own choice of the app's subagents, from the board's defaults.

use kanban_store::{page_default, Card, Patch, Store, CRITIC_RULE_KEY, MERGER_RULE_KEY, PAGE_SETTINGS, REPRO_RULE_KEY, REVIEW_RULE_KEY, ROUTER_RULE_KEY, RULE_KEYS, SUBAGENTS, SUBAGENTS_KEY, TEST_RULE_KEY};
use serde_json::{json, Value};

fn patch(v: Value) -> Patch {
    v.as_object().cloned().unwrap()
}

fn board() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("board.db")).unwrap();
    (dir, s)
}

fn subs(c: &Card) -> Value {
    c.fields.get(SUBAGENTS_KEY).cloned().unwrap_or(Value::Null)
}

/// A card as one made before card #151 reads: no `subagents` field.
fn old_card() -> Card {
    Card { id: "c9".into(), number: 9, column: "In progress".into(), order: None, order_col: None, archived: false, fields: patch(json!({ "title": "Old" })), version: 1 }
}

#[test]
fn a_new_card_gets_the_boards_defaults() {
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T" })), "test").unwrap();
    assert_eq!(subs(&c), json!({ "router": true, "critic": true, "reproducer": true, "reviewer": true, "tester": true, "merger": true }));
    // Hugh's critic off (as on the live board) is the critic's default for a new card.
    s.set_page_setting(CRITIC_RULE_KEY, "off").unwrap();
    s.set_page_setting(MERGER_RULE_KEY, "off").unwrap();
    let c2 = s.create(None, patch(json!({ "title": "T2" })), "test").unwrap();
    assert_eq!(subs(&c2)["critic"], false);
    assert_eq!(subs(&c2)["merger"], false);
    assert_eq!(subs(&c2)["tester"], true);
    // A default changed later doesn't change a card made before it.
    assert_eq!(subs(&s.get(&c.id).unwrap())["critic"], true);
    assert!(s.subagent_on(&c, "critic") && !s.subagent_on(&c2, "critic"));
    // Through a stack too: a child is made by `create`.
    let lead = s.create(None, patch(json!({ "title": "L" })), "test").unwrap();
    let kid = s.create_in_stack(patch(json!({ "title": "K" })), lead.number, "test").unwrap();
    assert_eq!(subs(&kid)["critic"], false);
}

#[test]
fn hughs_choice_on_add_card_wins_for_the_ones_it_names() {
    let (_d, mut s) = board();
    s.set_page_setting(CRITIC_RULE_KEY, "off").unwrap();
    let c = s.create(None, patch(json!({ "title": "T", "subagents": { "tester": false, "critic": true } })), "Hugh (board)").unwrap();
    assert_eq!(subs(&c), json!({ "router": true, "critic": true, "reproducer": true, "reviewer": true, "tester": false, "merger": true }));
    // A bad choice is refused, and no card is made.
    for bad in [json!({ "card-tester": false }), json!({ "tester": "off" }), json!("tester"), json!(null)] {
        assert!(s.create(None, patch(json!({ "title": "X", "subagents": bad })), "Hugh (board)").is_err(), "{bad}");
    }
    assert_eq!(s.list(true).unwrap().len(), 1);
}

#[test]
fn an_update_changes_the_named_ones_and_is_kept_in_the_revisions() {
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T" })), "test").unwrap();
    let c = s.update(&c.id, c.version, &patch(json!({ "subagents": { "reviewer": false } })), "Hugh (board)").unwrap();
    assert_eq!(subs(&c)["reviewer"], false);
    assert_eq!(subs(&c)["tester"], true);
    assert!(!s.subagent_on(&c, "reviewer") && s.subagent_on(&c, "card-tester"));
    // The same value again is no change (no new version).
    let v = c.version;
    let c = s.update(&c.id, c.version, &patch(json!({ "subagents": { "reviewer": false } })), "Hugh (board)").unwrap();
    assert_eq!(c.version, v);
    // Refused: an unknown one, a value that isn't true or false, or none at all.
    for bad in [json!({ "planner": false }), json!({ "tester": 1 }), json!(null), json!([])] {
        assert!(s.update(&c.id, c.version, &patch(json!({ "subagents": bad })), "Hugh (board)").is_err(), "{bad}");
    }
    assert!(s.revision_count(&c.id).unwrap() >= 2);
}

#[test]
fn a_card_made_before_card_151_follows_the_defaults() {
    let (_d, mut s) = board();
    let old = old_card();
    assert!(SUBAGENTS.iter().all(|a| old.subagent_choice(a.key).is_none() && s.subagent_on(&old, a.key)));
    s.set_page_setting(TEST_RULE_KEY, "off").unwrap();
    s.set_page_setting(ROUTER_RULE_KEY, "off").unwrap();
    assert!(!s.subagent_on(&old, "tester") && !s.subagent_on(&old, "router") && s.subagent_on(&old, "reviewer"));
    assert_eq!(s.subagents_off_in(&old, "In progress").iter().map(|a| a.name).collect::<Vec<_>>(), ["card-tester"]);
    assert_eq!(s.subagents_off_in(&old, "Planning").iter().map(|a| a.name).collect::<Vec<_>>(), ["card-router"]);
    assert!(s.subagents_off_in(&old, "Ship it").is_empty());
    // Partly chosen: the rest still follow the defaults.
    let mut part = old_card();
    part.fields.insert(SUBAGENTS_KEY.into(), json!({ "tester": true }));
    assert!(s.subagent_on(&part, "tester") && !s.subagent_on(&part, "router"));
    // An unknown name is off, never a hold by accident.
    assert!(!s.subagent_on(&old, "card-planner"));
}

#[test]
fn the_six_defaults_are_page_settings_on_by_default() {
    let (_d, mut s) = board();
    for key in [ROUTER_RULE_KEY, CRITIC_RULE_KEY, REPRO_RULE_KEY, REVIEW_RULE_KEY, TEST_RULE_KEY, MERGER_RULE_KEY] {
        assert!(PAGE_SETTINGS.contains(&key) && RULE_KEYS.contains(&key), "{key}");
        assert_eq!(page_default(key).unwrap(), "on", "{key}");
        assert_eq!(s.page_setting(key).unwrap(), "on");
        assert!(s.set_page_setting(key, "maybe").is_err());
        assert_eq!(s.set_page_setting_by(key, "off", "Hugh (Settings)").unwrap(), "off");
        assert_eq!(s.setting_log(key, 5).unwrap().len(), 1, "{key}: a change is logged");
    }
    assert_eq!(SUBAGENTS.iter().map(|a| a.setting).collect::<Vec<_>>(), [ROUTER_RULE_KEY, CRITIC_RULE_KEY, REPRO_RULE_KEY, REVIEW_RULE_KEY, TEST_RULE_KEY, MERGER_RULE_KEY]);
    assert!(s.subagent_defaults().values().all(|v| v == &json!(false)));
}
