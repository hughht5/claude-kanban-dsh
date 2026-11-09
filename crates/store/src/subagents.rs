//! Hugh's choice of the app's subagents for each card (card #151, three-way since card #162). The
//! five (`router, critic, reproducer, reviewer, tester`) each have a board default: the
//! Settings switches, one setting each (four of them the board-wide rules they replace, so the
//! critic stays off where Hugh had turned it off). A card's `subagents` holds only Hugh's own
//! overrides, `{critic: false}`: a key it leaves out follows the Settings default as it is now, so
//! a later change of the default reaches it. Hugh's Add card may send overrides; his card details,
//! and his own sessions outside the app (kb), change them, null putting one back to the default.
//! Card sessions and the Router can't: kanban_update_card refuses the field from them, and
//! kanban_create_card doesn't copy it. Migration 17 (card #162) took the values #151 stamped on
//! every card off where they equal the default, so those follow Settings from then on.
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

/// The card field holding Hugh's overrides: `{critic: false, tester: true}`. A key it leaves out
/// follows the board's default.
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

/// Checks a choice: an object whose keys are among the five and whose values are true, false, or
/// null (back to the board's default, card #162). It may name only some of them (an update
/// changes those). Returns it as given.
pub fn clean_subagents(v: &Value) -> Result<Map<String, Value>> {
    let keys = || SUBAGENTS.iter().map(|s| s.key).collect::<Vec<_>>().join(", ");
    let Value::Object(m) = v else {
        return Err(Error::Invalid(format!("subagents is an object of true, false or null (the default) for any of {}, like {{\"tester\": false}}", keys())));
    };
    // Card #162: a retired key from an older page is dropped, not refused.
    let m: &Map<String, Value> = &m.iter().filter(|(k, _)| !RETIRED.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
    for (k, x) in m {
        if subagent(k).filter(|s| s.key == k).is_none() {
            return Err(Error::Invalid(format!("\"{k}\" isn't one of the subagents ({})", keys())));
        }
        if !x.is_boolean() && !x.is_null() {
            return Err(Error::Invalid(format!("subagents.{k} is true, false or null (the default)")));
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

/// A new card's overrides (card #162): only Hugh's given choice (`given`, checked), its nulls left
/// out. None (no field) when nothing is left: every subagent then follows the board's default.
pub(crate) fn for_new_card(given: Option<&Value>) -> Result<Option<Value>> {
    let Some(v) = given else { return Ok(None) };
    merged(None, v)
}

/// An update's overrides on top of the card's (`old`): the keys it names change, a null one goes
/// back to the default (its key goes), the rest stay. None when no override is left: the field
/// goes, as on a card that never had one.
pub(crate) fn merged(old: Option<&Value>, patch: &Value) -> Result<Option<Value>> {
    let mut all = old.and_then(Value::as_object).cloned().unwrap_or_default();
    // A retired key (the card-merger's, card #162) goes with any change: nothing reads it.
    all.retain(|k, _| !RETIRED.contains(&k.as_str()));
    for (k, v) in clean_subagents(patch)? {
        if v.is_null() {
            all.remove(&k);
        } else {
            all.insert(k, v);
        }
    }
    Ok((!all.is_empty()).then_some(Value::Object(all)))
}

/// Migration 17 (card #162): before it, every card made since card #151 had all six (then) stamped
/// from the defaults of the day, so a later change of a default never reached it. On each card
/// not archived, a stamped value equal to the board's default now goes (it follows the default
/// from here on); one that differs stays as Hugh's override. Each card changed is an ordinary
/// write with its revision, by `MIGRATION_BY`. Running it again changes nothing.
pub(crate) fn unstamp_defaults(tx: &rusqlite::Transaction) -> Result<usize> {
    let rule_on = |key: &str| -> Result<bool> {
        let v: Option<String> = rusqlite::OptionalExtension::optional(tx.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)))?;
        Ok(v.as_deref() != Some("off"))
    };
    let mut defaults = Map::new();
    for s in SUBAGENTS {
        defaults.insert(s.key.to_string(), Value::Bool(rule_on(s.setting)?));
    }
    let ids: Vec<String> = {
        let mut st = tx.prepare("SELECT id FROM cards WHERE archived = 0 AND json_valid(fields) AND json_type(fields, '$.subagents') = 'object' ORDER BY number")?;
        let rows = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        rows
    };
    let mut changed = 0;
    for id in ids {
        let Some(mut card) = crate::cards::get_tx(tx, &id)? else { continue };
        let Some(own) = card.fields.get(SUBAGENTS_KEY).and_then(Value::as_object) else { continue };
        // A retired key (the card-merger's, card #162) goes too: nothing reads it now.
        let same: Map<String, Value> = own.iter().filter(|(k, v)| defaults.get(*k) == Some(*v) || RETIRED.contains(&k.as_str())).map(|(k, _)| (k.clone(), Value::Null)).collect();
        if same.is_empty() {
            continue;
        }
        let mut patch = crate::Patch::new();
        patch.insert(SUBAGENTS_KEY.into(), Value::Object(same));
        let keys = crate::cards::apply(&mut card, &patch)?;
        if keys.is_empty() {
            continue;
        }
        let version = card.version;
        crate::cards::write_tx(tx, &mut card, version, &keys, MIGRATION_BY)?;
        changed += 1;
    }
    Ok(changed)
}

/// Who migration 17's writes are recorded as.
pub const MIGRATION_BY: &str = "the app (migration 17: subagents follow Settings, card #162)";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_choice_names_only_the_five_with_true_or_false() {
        assert!(clean_subagents(&json!({ "tester": false, "router": true })).is_ok());
        assert!(clean_subagents(&json!({})).is_ok());
        assert!(clean_subagents(&json!({ "tester": null })).is_ok(), "null: back to the default");
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
        assert_eq!(merged(Some(&old), &json!({ "tester": false })).unwrap(), Some(json!({ "router": true, "critic": false, "tester": false })));
        // Card #162: an old card's stored `merger` goes with any change, and a new one is dropped.
        assert_eq!(merged(Some(&json!({ "merger": true })), &json!({ "merger": false, "critic": true })).unwrap(), Some(json!({ "critic": true })));
        assert_eq!(merged(Some(&json!({ "merger": true, "tester": false })), &json!({ "merger": null })).unwrap(), Some(json!({ "tester": false })));
        assert_eq!(merged(None, &json!({ "merger": false })).unwrap(), None);
        assert!(merged(Some(&old), &json!({ "tester": 0 })).is_err());
        // Card #162: null puts one back to the default; with none left the field goes.
        assert_eq!(merged(Some(&old), &json!({ "critic": null, "router": null })).unwrap(), Some(json!({ "tester": true })));
        assert_eq!(merged(Some(&json!({ "critic": false })), &json!({ "critic": null })).unwrap(), None);
        assert_eq!(merged(None, &json!({ "critic": null })).unwrap(), None);
        assert_eq!(for_new_card(None).unwrap(), None);
        assert_eq!(for_new_card(Some(&json!({ "tester": false, "critic": null }))).unwrap(), Some(json!({ "tester": false })));
        assert_eq!(for_new_card(Some(&json!({}))).unwrap(), None);
    }
}
