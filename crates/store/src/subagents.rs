//! Hugh's choice of the app's subagents for each card (card #151). Every card carries its own
//! on/off for each of the five (`subagents` = `{router, critic, reviewer, tester, reproducer}`),
//! set from the board's defaults when it is made: the Settings switches, one setting
//! each (four of them the board-wide rules they replace, so the critic stays off where Hugh had
//! turned it off). Hugh's Add card may send its own choice; his card details change it. Card
//! sessions, the Router and `kb` can't: the field isn't in kanban_update_card's list, and
//! kanban_create_card doesn't copy it. A card made before #151 has no field and follows the
//! board's defaults as they are now, as every card did before.
//!
//! The app's holds read the card's choice (`Store::subagent_on`): the plan critique before In
//! progress, the reviewer, tester and reproducer before Human test, a stack lead's own test and
//! review. The router holds nothing; its session is told to skip it.
//!
//! Card #162: the card-merger is no longer one of them (there were six). Every ship's merge goes
//! through its repo's merge lane (`lane.rs`); a stack's lead still merges each child with the
//! card-merger (card #113's merge job). A card made before #162 may hold a `merger` key: it is
//! ignored, and a choice that names it has it dropped (`RETIRED`), not refused.

use crate::{Card, Error, Result, Store, CRITIC_RULE_KEY, REPRO_RULE_KEY, REVIEW_RULE_KEY, TEST_RULE_KEY};
use serde_json::{Map, Value};

/// The card field holding the choice: `{router: true, critic: false, ...}`.
pub const SUBAGENTS_KEY: &str = "subagents";
/// The board's default for the card-router on a new card: does its session route it first in
/// Planning? "on" (the default, as before card #151) or "off".
pub const ROUTER_RULE_KEY: &str = "sessions.router_in_planning";
/// Keys a card or an older page may still hold that are no longer the card's subagents: the
/// card-merger's (card #162). A choice that names one has it dropped.
pub const RETIRED: [&str; 1] = ["merger"];

/// One of the five, in the order the board shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subagent {
    /// Its key in the card's `subagents`.
    pub key: &'static str,
    /// Its name, as a session calls it (`subagent_type`).
    pub name: &'static str,
    /// The setting holding the board's default for a new card.
    pub setting: &'static str,
    /// The column whose session runs it; the app tells that session when it is off.
    pub column: &'static str,
    /// What its session skips when it is off, for the session's note.
    pub skip: &'static str,
}

pub const SUBAGENTS: [Subagent; 5] = [
    Subagent { key: "router", name: "card-router", setting: ROUTER_RULE_KEY, column: "Planning", skip: "skip the planning prompt's card-router step (route it first) and go on with the plan." },
    Subagent { key: "critic", name: "card-critic", setting: CRITIC_RULE_KEY, column: "Planning", skip: "skip the planning prompt's card-critic step. Your move to In progress isn't held for it." },
    Subagent { key: "reproducer", name: "card-reproducer", setting: REPRO_RULE_KEY, column: "In progress", skip: "don't run it. A bug report on the card is logged, and doesn't hold your move to Human test." },
    Subagent { key: "reviewer", name: "card-reviewer", setting: REVIEW_RULE_KEY, column: "In progress", skip: "skip the subagent review. Your move to Human test isn't held for it." },
    Subagent { key: "tester", name: "card-tester", setting: TEST_RULE_KEY, column: "In progress", skip: "skip the subagent test, and test it yourself. Your move to Human test isn't held for it." },
];

/// The subagent with this key ("tester"), or this name ("card-tester").
pub fn subagent(key_or_name: &str) -> Option<&'static Subagent> {
    SUBAGENTS.iter().find(|s| s.key == key_or_name || s.name == key_or_name)
}

/// Checks a choice: an object whose keys are among the six and whose values are true or false.
/// It may name only some of them (an update changes those). Returns it as given.
pub fn clean_subagents(v: &Value) -> Result<Map<String, Value>> {
    let keys = || SUBAGENTS.iter().map(|s| s.key).collect::<Vec<_>>().join(", ");
    let Value::Object(m) = v else {
        return Err(Error::Invalid(format!("subagents is an object of true or false for each of {}, like {{\"tester\": false}}", keys())));
    };
    // Card #162: a retired key from an older page is dropped, not refused.
    let m: &Map<String, Value> = &m.iter().filter(|(k, _)| !RETIRED.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
    for (k, x) in m {
        if subagent(k).filter(|s| s.key == k).is_none() {
            return Err(Error::Invalid(format!("\"{k}\" isn't one of the subagents ({})", keys())));
        }
        if !x.is_boolean() {
            return Err(Error::Invalid(format!("subagents.{k} is true or false")));
        }
    }
    Ok(m.clone())
}

impl Card {
    /// The card's own choice for one subagent (`key`, "tester"), if it has one.
    pub fn subagent_choice(&self, key: &str) -> Option<bool> {
        self.fields.get(SUBAGENTS_KEY).and_then(|m| m.get(key)).and_then(Value::as_bool)
    }
}

impl Store {
    /// The board's defaults for a new card: each subagent on unless Hugh turned its switch off.
    pub fn subagent_defaults(&self) -> Map<String, Value> {
        SUBAGENTS.iter().map(|s| (s.key.to_string(), Value::Bool(self.rule_on(s.setting)))).collect()
    }

    /// Does `card` have subagent `key` ("critic") on? Its own choice, else the board's default
    /// (a card made before card #151). An unknown key is off.
    pub fn subagent_on(&self, card: &Card, key: &str) -> bool {
        let Some(s) = subagent(key) else { return false };
        card.subagent_choice(s.key).unwrap_or_else(|| self.rule_on(s.setting))
    }

    /// Every subagent's state on `card`, in the board's order: its own choice, else the default.
    pub fn subagents_of(&self, card: &Card) -> Map<String, Value> {
        SUBAGENTS.iter().map(|s| (s.key.to_string(), Value::Bool(self.subagent_on(card, s.key)))).collect()
    }

    /// The subagents run in `column` that are off on `card`.
    pub fn subagents_off_in(&self, card: &Card, column: &str) -> Vec<&'static Subagent> {
        SUBAGENTS.iter().filter(|s| s.column == column && !self.subagent_on(card, s.key)).collect()
    }
}

/// A new card's choice: Hugh's (`given`, checked), with the board's defaults for the rest.
pub(crate) fn for_new_card(store: &Store, given: Option<&Value>) -> Result<Value> {
    let mut all = store.subagent_defaults();
    if let Some(v) = given {
        all.extend(clean_subagents(v)?);
    }
    Ok(Value::Object(all))
}

/// An update's choice on top of the card's (`old`): the keys it names change, the rest stay.
pub(crate) fn merged(old: Option<&Value>, patch: &Value) -> Result<Value> {
    let mut all = old.and_then(Value::as_object).cloned().unwrap_or_default();
    all.extend(clean_subagents(patch)?);
    Ok(Value::Object(all))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_choice_names_only_the_six_with_true_or_false() {
        assert!(clean_subagents(&json!({ "tester": false, "router": true })).is_ok());
        assert!(clean_subagents(&json!({})).is_ok());
        for bad in [json!({ "card-tester": false }), json!({ "tester": "off" }), json!({ "planner": true }), json!(null), json!(["tester"]), json!(true)] {
            assert!(clean_subagents(&bad).is_err(), "{bad}");
        }
        assert_eq!(SUBAGENTS.iter().map(|s| s.key).collect::<Vec<_>>(), ["router", "critic", "reproducer", "reviewer", "tester"]);
        assert!(subagent("card-planner").is_none());
        // Card #162: the card-merger is no card subagent now. An older page's `merger` is dropped.
        assert!(subagent("card-merger").is_none() && subagent("merger").is_none());
        assert_eq!(Value::Object(clean_subagents(&json!({ "merger": false, "tester": true })).unwrap()), json!({ "tester": true }));
    }

    /// The page keeps its own copy of the table, with the labels (`ui/subagents.js`): the same
    /// keys, names and settings, in the same order.
    #[test]
    fn the_pages_table_is_this_one() {
        let js = include_str!("../../../ui/subagents.js");
        let mut at = 0;
        for s in SUBAGENTS {
            let row = format!("{{card:\"{}\",name:\"{}\",key:\"{}\"", s.key, s.name, s.setting);
            let i = js[at..].find(&row).unwrap_or_else(|| panic!("ui/subagents.js has no row {row} after the last one"));
            at += i + row.len();
        }
        for (field, key) in [("test", "tester"), ("review", "reviewer"), ("critique", "critic"), ("repro", "reproducer")] {
            assert!(js.contains(&format!("{field}:\"{key}\"")), "SUBAGENT_OF_CHECK lacks {field}");
        }
        // Card #162: the page has no card-merger row.
        assert!(!js.contains("card:\"merger\"") && !js.contains("sessions.merger_at_ship"), "ui/subagents.js still has the card-merger");
    }

    #[test]
    fn an_update_changes_only_the_keys_it_names() {
        let old = json!({ "router": true, "critic": false, "tester": true });
        assert_eq!(merged(Some(&old), &json!({ "tester": false })).unwrap(), json!({ "router": true, "critic": false, "tester": false }));
        // Card #162: an old card's stored `merger` stays as it was (ignored); a new one is dropped.
        assert_eq!(merged(Some(&json!({ "merger": true })), &json!({ "merger": false, "critic": true })).unwrap(), json!({ "merger": true, "critic": true }));
        assert!(merged(Some(&old), &json!({ "tester": 0 })).is_err());
    }
}
