// Card #151: each card's own choice of the app's subagents, from the board's defaults.

use kanban_store::{page_default, Card, Patch, Store, CRITIC_RULE_KEY, PAGE_SETTINGS, REPRO_RULE_KEY, REVIEW_RULE_KEY, ROUTER_RULE_KEY, RULE_KEYS, SUBAGENTS, SUBAGENTS_KEY, TEST_RULE_KEY};
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

/// Card #162: a new card keeps no stamped copy of the defaults, so a later change of a default
/// in Settings reaches it.
#[test]
fn a_new_card_follows_the_boards_defaults_as_they_change() {
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T" })), "test").unwrap();
    assert_eq!(subs(&c), Value::Null, "no stamped values");
    assert!(SUBAGENTS.iter().all(|a| s.subagent_on(&c, a.key)));
    // Hugh's critic off (as on the live board) holds for it from then on.
    s.set_page_setting(CRITIC_RULE_KEY, "off").unwrap();
    let c = s.get(&c.id).unwrap();
    assert!(!s.subagent_on(&c, "critic") && s.subagent_on(&c, "tester"));
    let c2 = s.create(None, patch(json!({ "title": "T2" })), "test").unwrap();
    assert_eq!(subs(&c2), Value::Null);
    assert!(!s.subagent_on(&c2, "critic"));
    s.set_page_setting(CRITIC_RULE_KEY, "on").unwrap();
    assert!(s.subagent_on(&s.get(&c2.id).unwrap(), "critic"));
    // Through a stack too: a child is made by `create`.
    let lead = s.create(None, patch(json!({ "title": "L" })), "test").unwrap();
    let kid = s.create_in_stack(patch(json!({ "title": "K" })), lead.number, "test").unwrap();
    assert_eq!(subs(&kid), Value::Null);
}

#[test]
fn hughs_choice_on_add_card_keeps_only_his_overrides() {
    let (_d, mut s) = board();
    s.set_page_setting(CRITIC_RULE_KEY, "off").unwrap();
    // Only what he chose is kept; a null (Default) is no override.
    let c = s.create(None, patch(json!({ "title": "T", "subagents": { "tester": false, "critic": true, "router": null } })), "Hugh (board)").unwrap();
    assert_eq!(subs(&c), json!({ "tester": false, "critic": true }));
    assert!(s.subagent_on(&c, "critic") && !s.subagent_on(&c, "tester") && s.subagent_on(&c, "router"));
    let none = s.create(None, patch(json!({ "title": "N", "subagents": {} })), "Hugh (board)").unwrap();
    assert_eq!(subs(&none), Value::Null, "nothing chosen: no field");
    // A bad choice is refused, and no card is made.
    for bad in [json!({ "card-tester": false }), json!({ "tester": "off" }), json!("tester"), json!(true)] {
        assert!(s.create(None, patch(json!({ "title": "X", "subagents": bad })), "Hugh (board)").is_err(), "{bad}");
    }
    assert_eq!(s.list(true).unwrap().len(), 2);
}

/// Card #162: three-way. A key set to null goes back to the default; with no override left the
/// field goes; null for the whole field clears them all. Each change is a revision.
#[test]
fn an_override_goes_back_to_the_default_with_null() {
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T", "subagents": { "critic": false, "tester": true } })), "Hugh (board)").unwrap();
    let r0 = s.revision_count(&c.id).unwrap();
    let c = s.update(&c.id, c.version, &patch(json!({ "subagents": { "critic": null } })), "Hugh (board)").unwrap();
    assert_eq!(subs(&c), json!({ "tester": true }));
    assert!(s.subagent_on(&c, "critic"), "back to the default (on)");
    s.set_page_setting(TEST_RULE_KEY, "off").unwrap();
    assert!(s.subagent_on(&c, "tester"), "an override on stays on whatever the default");
    let c = s.update(&c.id, c.version, &patch(json!({ "subagents": { "tester": null } })), "Hugh (board)").unwrap();
    assert_eq!(subs(&c), Value::Null, "no override left: no field");
    assert!(!s.subagent_on(&c, "tester"), "follows the default (off) now");
    // Null again changes nothing.
    let v = c.version;
    let c = s.update(&c.id, c.version, &patch(json!({ "subagents": { "tester": null } })), "Hugh (board)").unwrap();
    assert_eq!(c.version, v);
    let c = s.update(&c.id, c.version, &patch(json!({ "subagents": { "reviewer": false, "router": false } })), "Hugh (board)").unwrap();
    let c = s.update(&c.id, c.version, &patch(json!({ "subagents": null })), "Hugh (board)").unwrap();
    assert_eq!(subs(&c), Value::Null, "null for the field: all back to the defaults");
    assert_eq!(s.revision_count(&c.id).unwrap(), r0 + 4);
}

#[test]
fn an_update_changes_the_named_ones_and_is_kept_in_the_revisions() {
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "T" })), "test").unwrap();
    let c = s.update(&c.id, c.version, &patch(json!({ "subagents": { "reviewer": false } })), "Hugh (board)").unwrap();
    assert_eq!(subs(&c), json!({ "reviewer": false }), "only the override is kept");
    assert!(!s.subagent_on(&c, "reviewer") && s.subagent_on(&c, "card-tester"));
    // The same value again is no change (no new version).
    let v = c.version;
    let c = s.update(&c.id, c.version, &patch(json!({ "subagents": { "reviewer": false } })), "Hugh (board)").unwrap();
    assert_eq!(c.version, v);
    // Refused: an unknown one, a value that isn't true, false or null, or not an object.
    for bad in [json!({ "planner": false }), json!({ "tester": 1 }), json!("off"), json!([])] {
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
fn the_five_defaults_are_page_settings_on_by_default() {
    let (_d, mut s) = board();
    for key in [ROUTER_RULE_KEY, CRITIC_RULE_KEY, REPRO_RULE_KEY, REVIEW_RULE_KEY, TEST_RULE_KEY] {
        assert!(PAGE_SETTINGS.contains(&key) && RULE_KEYS.contains(&key), "{key}");
        assert_eq!(page_default(key).unwrap(), "on", "{key}");
        assert_eq!(s.page_setting(key).unwrap(), "on");
        assert!(s.set_page_setting(key, "maybe").is_err());
        assert_eq!(s.set_page_setting_by(key, "off", "Hugh (Settings)").unwrap(), "off");
        assert_eq!(s.setting_log(key, 5).unwrap().len(), 1, "{key}: a change is logged");
    }
    assert_eq!(SUBAGENTS.iter().map(|a| a.setting).collect::<Vec<_>>(), [ROUTER_RULE_KEY, CRITIC_RULE_KEY, REPRO_RULE_KEY, REVIEW_RULE_KEY, TEST_RULE_KEY]);
    assert!(s.subagent_defaults().values().all(|v| v == &json!(false)));
}

/// Card #162: the card-merger's switch (`sessions.merger_at_ship`) is gone. A board that saved it
/// keeps the value (an older build still reads it), and nothing here reads it; an old card's
/// `merger` choice is ignored, and an older page's is dropped.
#[test]
fn the_card_mergers_switch_and_choice_are_gone_and_ignored() {
    let (_d, mut s) = board();
    let old_key = "sessions.merger_at_ship";
    assert!(!PAGE_SETTINGS.contains(&old_key) && !RULE_KEYS.contains(&old_key));
    assert!(s.set_page_setting(old_key, "off").is_err(), "the page can't set it now");
    s.set_setting(old_key, "off").unwrap();
    assert_eq!(s.setting(old_key).unwrap().as_deref(), Some("off"), "the saved value stays");
    let c = s.create(None, patch(json!({ "title": "T", "subagents": { "merger": false, "tester": true } })), "Hugh (board)").unwrap();
    assert!(subs(&c).get("merger").is_none(), "{}", subs(&c));
    // A card made before #162, with the merger off: still five on, nothing about a merger.
    let mut old = old_card();
    old.fields.insert(SUBAGENTS_KEY.into(), json!({ "merger": false }));
    assert!(SUBAGENTS.iter().all(|a| s.subagent_on(&old, a.key)));
    assert!(!s.subagent_on(&old, "merger"));
    assert!(s.subagents_off_in(&old, "Ship it").is_empty());
    assert!(!s.subagents_of(&old).contains_key("merger"));
}
