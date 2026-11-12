//! A card's flow (card #162): Hugh's per-card options for the steps a card goes through, each with
//! a board default in Settings > Sessions.
//!
//! - `skipPlanning`: a move into Planning from Backlog or Blocked goes to In progress instead (the
//!   store's moves, `move_as`, and a session's or the Router's move, `dispatch::session_move`).
//!   No plan request, no Planning session, no plan critique. Hugh's move into Planning from a later
//!   column still plans.
//! - `skipHumanTest`: when a session's move or the stack rules put the card in Human test, the
//!   app moves it on into Ship it for Hugh (`dispatch::auto_progress`), as "Take it to: Deploy
//!   complete" does. Hugh's own drag into Human test never skips, and "Take it to: Human test"
//!   wins (it stops there for him).
//! - `skipPlanReview` (card #195): a card session's or the Router's move from Planning into In
//!   progress (or later) goes there as asked, instead of stopping in Plan review for Hugh to
//!   approve the plan (`plan_review_stop`). Off by default: a plan waits for Hugh. Skipping
//!   Planning skips Plan review too (Backlog straight to In progress).
//!
//! Each is three-way on the card: true, false, or absent (follow the default as it is now). A
//! stack's child ignores both: its lead's flow plans and ships it. Only Hugh sets them: his card
//! details, Add card, and his own sessions outside the app (kb); never a card session or the
//! Router (kanban_update_card refuses them there). The subagents (`subagents.rs`) are the third
//! kind of flow option, three-way the same way.

use crate::{Card, Error, Result, Store, SUBAGENTS, SUBAGENTS_KEY};
use rusqlite::OptionalExtension;
use serde_json::{json, Map, Value};

/// The card field: true skips Planning for this card, false never does, absent follows the default.
pub const SKIP_PLANNING_KEY: &str = "skipPlanning";
/// The card field: true skips Human test for this card, false never does, absent follows the default.
pub const SKIP_HUMAN_TEST_KEY: &str = "skipHumanTest";
/// The board's default for skipping Planning: "on" or "off" (the default).
pub const SKIP_PLANNING_RULE_KEY: &str = "sessions.skip_planning";
/// The board's default for skipping Human test: "on" or "off" (the default).
pub const SKIP_HUMAN_TEST_RULE_KEY: &str = "sessions.skip_human_test";
/// The card field (card #195): true skips Plan review for this card, false never does, absent
/// follows the default.
pub const SKIP_PLAN_REVIEW_KEY: &str = "skipPlanReview";
/// The board's default for skipping Plan review: "on" or "off" (the default).
pub const SKIP_PLAN_REVIEW_RULE_KEY: &str = "sessions.skip_plan_review";
/// The column a finished plan waits in for Hugh (card #195).
pub const PLAN_REVIEW: &str = "Plan review";
/// The skip options, each with its board default, in the order the flow shows them.
pub const SKIPS: [(&str, &str); 3] = [
    (SKIP_PLANNING_KEY, SKIP_PLANNING_RULE_KEY),
    (SKIP_PLAN_REVIEW_KEY, SKIP_PLAN_REVIEW_RULE_KEY),
    (SKIP_HUMAN_TEST_KEY, SKIP_HUMAN_TEST_RULE_KEY),
];
/// The card fields only Hugh sets: the skips and the subagents.
pub const FLOW_FIELDS: [&str; 4] = [SKIP_PLANNING_KEY, SKIP_PLAN_REVIEW_KEY, SKIP_HUMAN_TEST_KEY, SUBAGENTS_KEY];

/// Is `key` one of the skip fields?
pub fn is_skip(key: &str) -> bool {
    SKIPS.iter().any(|(k, _)| *k == key)
}

/// The board-default setting for skip field `key`, if it is one.
pub fn skip_rule(key: &str) -> Option<&'static str> {
    SKIPS.iter().find(|(k, _)| *k == key).map(|(_, r)| *r)
}

/// The columns a move from Planning stops in Plan review on its way to (card #195): into the
/// build or straight past it.
pub const PAST_PLAN_REVIEW: [&str; 3] = ["In progress", "Human test", "Deploy complete"];
/// The state of a card's `plan` record when its move into Planning went to In progress instead.
pub const PLAN_SKIPPED: &str = "skipped";

/// Checks a skip field's value: true or false, or null for none (back to the default).
pub fn clean_skip(key: &str, v: &Value) -> Result<Option<Value>> {
    match v {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(json!(b))),
        _ => Err(Error::Invalid(format!("{key} is true, false or null (the default in Settings)"))),
    }
}

/// Where a flow option's value comes from: the card's own choice, or the board's default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Whose {
    Card,
    Default,
}

impl Whose {
    pub fn word(self) -> &'static str {
        match self {
            Whose::Card => "card",
            Whose::Default => "default",
        }
    }
    /// For a log line or a session's note.
    pub fn whose(self) -> &'static str {
        match self {
            Whose::Card => "this card's own",
            Whose::Default => "the default in Settings",
        }
    }
}

impl Card {
    /// The card's own choice for a skip field (`SKIP_PLANNING_KEY`, `SKIP_HUMAN_TEST_KEY`), if any.
    pub fn skip_choice(&self, key: &str) -> Option<bool> {
        self.fields.get(key).and_then(Value::as_bool)
    }

    /// Did this card's last move into Planning go to In progress instead (card #162)?
    pub fn planning_skipped(&self) -> bool {
        self.fields.get("plan").and_then(|p| p.get("state")).and_then(Value::as_str) == Some(PLAN_SKIPPED)
    }
}

/// Where a card arrived in a column (`Store::arrival`): who moved it there, and whether its
/// `autoTo` was cleared since (it stopped there for Hugh).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrival {
    pub by: String,
    pub auto_cleared: bool,
}

impl Store {
    /// The board's default for a skip option (`SKIP_PLANNING_RULE_KEY`, ...): on only if Hugh
    /// turned it on.
    pub fn skip_default(&self, rule: &str) -> bool {
        self.setting(rule).ok().flatten().as_deref() == Some("on")
    }

    /// A skip option on `card` and where it comes from: its own choice, else the default. A
    /// stack's child never skips (its lead's flow covers it), whatever it says.
    pub fn skip_of(&self, card: &Card, key: &str) -> (bool, Whose) {
        let Some(rule) = SKIPS.iter().find(|(k, _)| *k == key).map(|(_, r)| *r) else { return (false, Whose::Default) };
        match card.skip_choice(key) {
            Some(b) => (b && card.stack_of().is_none(), Whose::Card),
            None => (self.skip_default(rule) && card.stack_of().is_none(), Whose::Default),
        }
    }

    /// Does a move of `card` into Planning go to In progress instead?
    pub fn skips_planning(&self, card: &Card) -> bool {
        self.skip_of(card, SKIP_PLANNING_KEY).0
    }

    /// Does `card` skip Plan review (card #195): its own choice, else the default?
    pub fn skips_plan_review(&self, card: &Card) -> bool {
        self.skip_of(card, SKIP_PLAN_REVIEW_KEY).0
    }

    /// Whether a card session's or the Router's move of `card` to `to` stops in Plan review
    /// instead (card #195), and whose choice that is: a move from Planning into In progress or
    /// later, on a card that doesn't skip Plan review, with no Take it to (Hugh's "go on by
    /// itself" approves the plan too), that isn't a stack's child (its lead's approval covers it).
    pub fn plan_review_stop(&self, card: &Card, to: &str) -> Option<Whose> {
        if card.column != "Planning" || !PAST_PLAN_REVIEW.contains(&to) || card.stack_of().is_some() || card.auto_to().is_some() {
            return None;
        }
        let (skip, whose) = self.skip_of(card, SKIP_PLAN_REVIEW_KEY);
        (!skip).then_some(whose)
    }

    /// Does `card` go on from Human test to Ship it by itself (when a session's move or the stack
    /// rules put it there)?
    pub fn skips_human_test(&self, card: &Card) -> bool {
        self.skip_of(card, SKIP_HUMAN_TEST_KEY).0
    }

    /// The card's flow as it holds now (`kanban_get_card`'s and `kb card`'s `flow`): for each of
    /// skipPlanning, skipPlanReview, skipHumanTest and the subagents, its value, whether it is the card's own
    /// or the default, and the default itself.
    pub fn flow_of(&self, card: &Card) -> Value {
        let mut out = Map::new();
        for (key, rule) in SKIPS {
            let (on, from) = self.skip_of(card, key);
            out.insert(key.into(), json!({ "value": on, "from": from.word(), "default": self.skip_default(rule) }));
        }
        let mut subs = Map::new();
        for s in SUBAGENTS {
            let from = if card.subagent_choice(s.key).is_some() { Whose::Card } else { Whose::Default };
            subs.insert(s.key.into(), json!({ "value": self.subagent_on(card, s.key), "from": from.word(), "default": self.rule_on(s.setting) }));
        }
        out.insert(SUBAGENTS_KEY.into(), Value::Object(subs));
        if card.stack_of().is_some() {
            out.insert("note".into(), json!("A stack's card never skips Planning or Human test, and never stops in Plan review: its lead's flow plans, approves and ships it."));
        }
        Value::Object(out)
    }

    /// How `card` last arrived in `column`: who wrote the newest revision that set it there, and
    /// whether a later revision cleared its `autoTo`. None when no revision did (an import).
    pub fn arrival(&self, id: &str, column: &str) -> Result<Option<Arrival>> {
        let at: Option<(i64, Option<String>)> = self
            .conn
            .query_row(
                "SELECT seq, by FROM card_revisions WHERE card_id = ?1
                   AND CASE WHEN json_valid(changed) THEN json_extract(changed, '$.column') END = ?2
                 ORDER BY seq DESC LIMIT 1",
                rusqlite::params![id, column],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((seq, by)) = at else { return Ok(None) };
        let auto_cleared: bool = self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM card_revisions WHERE card_id = ?1 AND seq > ?2
               AND json_valid(changed) AND json_type(changed, '$.autoTo') = 'null')",
            rusqlite::params![id, seq],
            |r| r.get(0),
        )?;
        Ok(Some(Arrival { by: by.unwrap_or_default(), auto_cleared }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skip_is_true_false_or_null() {
        assert_eq!(clean_skip(SKIP_PLANNING_KEY, &json!(true)).unwrap(), Some(json!(true)));
        assert_eq!(clean_skip(SKIP_PLANNING_KEY, &json!(false)).unwrap(), Some(json!(false)));
        assert_eq!(clean_skip(SKIP_PLANNING_KEY, &Value::Null).unwrap(), None);
        for bad in [json!("on"), json!(1), json!({}), json!([])] {
            assert!(clean_skip(SKIP_HUMAN_TEST_KEY, &bad).is_err(), "{bad}");
        }
        assert_eq!(FLOW_FIELDS, ["skipPlanning", "skipPlanReview", "skipHumanTest", "subagents"]);
        assert!(is_skip(SKIP_PLAN_REVIEW_KEY) && !is_skip(SUBAGENTS_KEY));
        assert_eq!(skip_rule(SKIP_PLAN_REVIEW_KEY), Some(SKIP_PLAN_REVIEW_RULE_KEY));
    }
}
