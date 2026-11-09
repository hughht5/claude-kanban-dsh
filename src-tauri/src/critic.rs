//! The plan critique before building (Hugh, card #97): every card session gets a `card-critic`
//! subagent. At the end of Planning the session briefs it with the card; it attacks the plan
//! (the card's summary) and ends PASS or FAIL. The app records its verdict on the card it names
//! (`critique`, by `agents.rs`'s shared code), and a card session's move into In progress is
//! held until that card's critique has passed, while Hugh's rule is on. Hugh's own moves never
//! are. The resets (into Planning, a summary edit, `started`) are the store's (`cards.rs`).

use crate::agents::{not_passed, Agent, Check, OnStop};
use kanban_store::{routed_small, Card, Store, CRITIQUE_KEY, ONE_FAILED_KEY, TEST_KEY};
use serde_json::Value;

/// The subagent's name, as the session calls it (`subagent_type`) and the hook reports it.
pub const CRITIC: &str = "card-critic";

const DESCRIPTION: &str = "Independent critic of one card's plan on Hugh's Claude Kanban DSH board. Run it, and wait for its result, at the end of Planning, before you say the plan is ready or move the card on: brief it with the card number and where the code is, not your conclusions. It checks the plan against the card and the code, changes nothing, and ends with CARD and VERDICT lines that the app records on the card.";

const PROMPT: &str = "You are card-critic, an independent critic of one card's plan on Hugh's Claude Kanban DSH board. The plan is not built yet. Check it yourself: don't take the brief's word for anything.

1. Read the card: kanban_get_card with its number (from the brief). Its summary is the plan; doneWhen and doneWhenChecks are its acceptance checks.
2. Read the code the plan names (the brief says where the checkout is) and the repo's AGENTS.md or CLAUDE.md.
3. A blocker is only: a deliverable the card asks for that the plan drops; a plan that, built as written, would come out wrong in a way the build wouldn't catch (a wrong assumption about the code: give file:line); a step that is unsafe or breaks a hard rule (live, data, secrets).
4. Everything else is a note: test steps and wording, the number or wording of the checks, small gaps in the test harness, details the build will find as it goes, style. Notes never fail a plan. Keep them few and short.
5. A question that is Hugh's to decide (scope, work the card didn't ask for, a trade-off the card doesn't settle) is a line starting \"HUGH:\". It holds the card for him.
6. Change nothing: don't edit files, commit, or change or move any card.

Report what you found, with evidence. Then end your reply with these lines, and nothing after them:
CARD: #<the card's number>
VERDICT: PASS
or, if there is a blocker, VERDICT: FAIL. After the VERDICT line, one line per point, each starting \"- \": after a FAIL the blockers, after a PASS the notes (none if there are none), and any HUGH: lines. The app records them on the card, and the notes go to the build. A PASS lets the card's session move it on.";

/// How the app records the critic's verdict (`agents::record_check`).
pub const CHECK: Check = Check {
    agent: CRITIC,
    key: CRITIQUE_KEY,
    by: "the app (card-critic subagent)",
    label: "Plan critique",
    who: "critic",
    verb: "critique",
    reset_in: "Planning",
    cap: Some(3),
    evidence: false,
};

pub const AGENT: Agent = Agent {
    name: CRITIC,
    description: DESCRIPTION,
    prompt: PROMPT,
    effort: Some("high"),
    edits: false,
    also_denied: &[],
    on_stop: OnStop::Check(&CHECK),
};

/// The columns a card's plan is critiqued before leaving, and where to.
const BEFORE_BUILD: [&str; 3] = ["Backlog", "Planning", "Blocked"];
/// Into In progress, or straight on past it (which would skip the build's start, so a stale
/// test or review pass can't carry an unchecked plan through).
const INTO_BUILD: [&str; 3] = ["In progress", "Human test", "Deploy complete"];

/// Why a card's session may not move `card` to `to`, if the plan critique holds it: into In
/// progress (or past it) from Backlog, Planning or Blocked only once the critique passed. From
/// Blocked it is free when the build had already started: `started` on the record (set on every
/// move into In progress), or, with no record at all, a subagent test field (set on every move
/// into In progress since card #76), for a card blocked mid-build before this rule.
pub fn gate(store: &Store, card: &Card, to: &str) -> Option<String> {
    let rec = card.fields.get(CRITIQUE_KEY);
    let built = match rec {
        Some(r) => r.get("started").and_then(Value::as_bool) == Some(true),
        None => card.fields.contains_key(TEST_KEY),
    };
    if card.column == "Blocked" && built {
        return None;
    }
    // Card #144's holds, where today's applies (the rule on, from before the build into it):
    // a question for Hugh, and a card routed small whose one critique failed.
    let required = store.subagent_on(card, "critic");
    if required && BEFORE_BUILD.contains(&card.column.as_str()) && INTO_BUILD.contains(&to) {
        if rec.and_then(|r| r.get("hugh")).and_then(Value::as_bool) == Some(true) {
            return Some(format!(
                "card #{n}'s plan critic asked Hugh something (a HUGH: line in its latest critique), so only Hugh moves it on. Ask him in your message, put the question in nextStep, and leave the card in Planning. Fix any other findings in the plan too. Once he has answered, put his answer in the plan: on a card not routed small that makes a fresh critique needed, and its pass lets you move the card again.",
                n = card.number
            ));
        }
        if one_failed(card) {
            return Some(format!(
                "card #{n} is routed small, so it gets one plan critique, and that critique failed (Hugh's rule, card #144). Fix the plan in the summary for its findings, don't run the critic again (its findings are kept as the build's notes), and tell Hugh the plan is ready: only he moves it on.",
                n = card.number
            ));
        }
    }
    let latest = not_passed(&CHECK, card, to, &BEFORE_BUILD, &INTO_BUILD, required)?;
    Some(format!(
        "card #{n}'s plan hasn't passed a plan critique since it last came into Planning (Hugh's rule: every plan is checked by a subagent before building).{latest} Do these steps:
1. Run the card-critic subagent: the Agent tool, subagent_type \"{CRITIC}\". Wait for its result; if it runs in the background, wait for its notification.
2. Brief it with the card number (#{n}) and where the code is (the checkout and branch). Do not give your conclusions.
3. Fix the plan in the card's summary and run it again. A finding marked HUGH: is his to decide: ask him, and leave the card in Planning.
4. Move the card when its latest verdict is PASS. Any edit to the summary after a pass needs a fresh pass, except on a card routed small (one critique). After its third FAIL, the app moves the card to Blocked (not a card routed small: after its FAIL it waits for Hugh).
If your session has no card-critic subagent (it started before this rule), say so on the card and leave the card for Hugh to move on, or ask him to turn the rule off or restart the session.",
        n = card.number
    ))
}

/// Whether `card` is routed small and its one plan critique failed (card #144,
/// `agents::one_critique` marks it): only Hugh moves it on.
pub fn one_failed(card: &Card) -> bool {
    routed_small(card) && card.fields.get(CRITIQUE_KEY).and_then(|r| r.get(ONE_FAILED_KEY)).and_then(Value::as_bool) == Some(true)
}

/// The critic's notes for the build (card #144), for the end of the In progress message: a
/// PASS's notes, or a small card's FAIL findings. None when there are none.
pub fn build_notes(card: &Card) -> Option<String> {
    let rec = card.fields.get(CRITIQUE_KEY)?;
    let notes: Vec<&str> = rec.get("notes")?.as_array()?.iter().filter_map(Value::as_str).filter(|n| !n.trim().is_empty()).collect();
    if notes.is_empty() {
        return None;
    }
    let list: String = notes.iter().map(|n| format!("- {n}\n")).collect();
    Some(if one_failed(card) {
        format!("Build notes from the plan critic: the findings of this card's one plan critique (it is routed small, and the critique failed). The plan was to be fixed for them before Hugh moved it on; check the build covers each one, and say in your hand-over how:\n{list}")
    } else {
        format!("Build notes from the plan critic (its pass came with these notes):\n{list}They don't hold the build and need no new plan critique. Handle each one as you build, or say in your hand-over why not. A note about the acceptance checks means sharpening doneWhenChecks now (an edit in In progress needs no new critique).")
    }.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn card(fields: Value, column: &str) -> Card {
        Card { id: "c4".into(), number: 4, column: column.into(), order: None, order_col: None, archived: false, fields: fields.as_object().cloned().unwrap(), version: 1 }
    }

    #[test]
    fn the_move_into_in_progress_waits_for_a_plan_that_passed() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        // From Planning or Backlog: no record counts as needed.
        for from in ["Planning", "Backlog"] {
            let why = gate(&s, &card(json!({ "title": "T" }), from), "In progress").unwrap();
            assert!(why.contains("card #4's plan hasn't passed a plan critique") && why.contains("subagent_type \"card-critic\"") && why.contains("has no card-critic subagent"), "{why}");
        }
        let failed = card(json!({ "title": "T", "critique": { "state": "failed", "detail": "it deletes log rows", "round": 1 } }), "Planning");
        assert!(gate(&s, &failed, "In progress").unwrap().contains("Its latest critique failed: it deletes log rows."));
        let unclear = card(json!({ "title": "T", "critique": { "state": "unclear", "detail": "no VERDICT line" } }), "Planning");
        assert!(gate(&s, &unclear, "In progress").unwrap().contains("didn't count: no VERDICT line"));
        assert!(gate(&s, &card(json!({ "title": "T", "critique": { "state": "passed" } }), "Planning"), "In progress").is_none());
        // Only the move into In progress is held.
        for to in ["Blocked", "Backlog", "Done"] {
            assert!(gate(&s, &card(json!({ "title": "T" }), "Planning"), to).is_none(), "{to}");
        }
        // Blocked: held before the build started, free once it had (blocked mid-build), and
        // free with no record (blocked before this rule).
        assert!(gate(&s, &card(json!({ "title": "T", "critique": { "state": "failed", "round": 3 } }), "Blocked"), "In progress").is_some());
        assert!(gate(&s, &card(json!({ "title": "T", "critique": { "state": "needed", "round": 0 } }), "Blocked"), "In progress").is_some());
        assert!(gate(&s, &card(json!({ "title": "T", "critique": { "state": "passed" } }), "Blocked"), "In progress").is_none());
        assert!(gate(&s, &card(json!({ "title": "T", "critique": { "state": "needed", "started": true } }), "Blocked"), "In progress").is_none());
        // No record: blocked mid-build before this rule (it has a test field) goes through; a
        // card that never reached In progress (made straight into Planning or Blocked) is held.
        assert!(gate(&s, &card(json!({ "title": "T", "test": { "state": "needed" } }), "Blocked"), "In progress").is_none());
        assert!(gate(&s, &card(json!({ "title": "T" }), "Blocked"), "In progress").is_some());
        // Straight on past In progress is held too: stale test and review passes can't carry
        // an unchecked plan to Human test.
        let stale = card(json!({ "title": "T", "test": { "state": "passed" }, "review": { "state": "passed" }, "critique": { "state": "needed", "round": 0 } }), "Planning");
        assert!(gate(&s, &stale, "Human test").is_some() && gate(&s, &stale, "Deploy complete").is_some());
        assert!(crate::agents::move_refused(&s, &stale, "Human test").is_some());
        // Hugh can turn it off: for one card (card #151) ...
        let own_off = card(json!({ "title": "T", "subagents": { "critic": false }, "critique": { "state": "failed", "round": 1 } }), "Planning");
        assert!(gate(&s, &own_off, "In progress").is_none());
        // ... or as the board's default, which a card made before #151 follows.
        s.set_page_setting(kanban_store::CRITIC_RULE_KEY, "off").unwrap();
        assert!(gate(&s, &failed, "In progress").is_none());
        // A card with the critic on is still held, whatever the default.
        let own_on = card(json!({ "title": "T", "subagents": { "critic": true } }), "Planning");
        assert!(gate(&s, &own_on, "In progress").is_some());
    }

    #[test]
    fn the_gate_is_one_of_the_apps_and_names_the_critic_first() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("b.db")).unwrap();
        // Through agents.rs, as tools.rs and dispatch.rs check it.
        let why = crate::agents::move_refused(&s, &card(json!({ "title": "T" }), "Planning"), "In progress").unwrap();
        assert!(why.contains("plan critique") && !why.contains("subagent review"), "{why}");
        assert!(crate::agents::move_refused(&s, &card(json!({ "title": "T", "critique": { "state": "passed" } }), "Planning"), "In progress").is_none());
    }

    #[test]
    fn the_critic_runs_at_high_and_marks_hughs_calls() {
        assert_eq!(AGENT.effort, Some("high"));
        assert!(!AGENT.edits);
        assert_eq!((CHECK.key, CHECK.reset_in, CHECK.cap), ("critique", "Planning", Some(3)));
        assert!(PROMPT.contains("VERDICT: PASS") && PROMPT.contains("CARD:") && PROMPT.contains("HUGH:"));
        assert!(crate::agents::find("card-critic").is_some_and(|a| a.name == CRITIC));
        let v: Value = serde_json::from_str(&crate::agents::agents_json()).unwrap();
        assert_eq!(v[CRITIC]["effort"], "high");
        assert!(PROMPT.len() < 2_000, "{}", PROMPT.len());
        // The refusal no longer says a summary edit always needs a fresh pass (card #144).
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("b.db")).unwrap();
        let why = gate(&s, &card(json!({ "title": "T" }), "Planning"), "In progress").unwrap();
        assert!(why.contains("except on a card routed small") && why.contains("after its FAIL it waits for Hugh"), "{why}");
    }

    // Card #144: the gentler critic.
    #[test]
    fn the_prompt_blocks_only_on_real_problems() {
        for s in [
            "A blocker is only",
            "the plan drops",
            "come out wrong in a way the build wouldn't catch",
            "unsafe or breaks a hard rule (live, data, secrets)",
            "Everything else is a note",
            "test steps and wording",
            "the number or wording of the checks",
            "gaps in the test harness",
            "Notes never fail a plan",
            "after a PASS the notes",
            "\"HUGH:\"",
            "doneWhenChecks",
        ] {
            assert!(PROMPT.contains(s), "{s}");
        }
        assert!(!PROMPT.contains("vague or untestable check is a finding"));
        assert!(!kanban_store::looks_like_token(PROMPT) && !PROMPT.to_ascii_lowercase().contains("password"));
    }

    fn small(critique: Value, column: &str) -> Card {
        card(json!({ "title": "T", "route": { "size": "small" }, "critique": critique }), column)
    }

    #[test]
    fn a_small_card_gets_one_critique_and_waits_for_hugh_after_a_fail() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        // After a PASS it goes on, as any card.
        assert!(gate(&s, &small(json!({ "state": "passed", "round": 0 }), "Planning"), "In progress").is_none());
        // After its FAIL (marked oneFailed): held, whatever came next (a plan edit keeps the
        // record; a critique run again that passes).
        for rec in [json!({ "state": "failed", "round": 1, "oneFailed": true }), json!({ "state": "passed", "round": 1, "oneFailed": true })] {
            let why = gate(&s, &small(rec.clone(), "Planning"), "In progress").expect("held");
            assert!(why.contains("routed small, so it gets one plan critique") && why.contains("only he moves it on"), "{rec}: {why}");
            assert!(gate(&s, &small(rec.clone(), "Planning"), "Human test").is_some());
        }
        // A FAIL from before the card was routed small (Route again) doesn't count as its one
        // critique: a later pass lets it through.
        assert!(gate(&s, &small(json!({ "state": "passed", "round": 1 }), "Planning"), "In progress").is_none());
        // A normal card: today's rules (a later pass lets it through).
        let normal = |rec: Value| card(json!({ "title": "T", "route": { "size": "normal" }, "critique": rec }), "Planning");
        assert!(gate(&s, &normal(json!({ "state": "passed", "round": 2 })), "In progress").is_none());
        assert!(gate(&s, &normal(json!({ "state": "failed", "round": 1 })), "In progress").unwrap().contains("hasn't passed a plan critique"));
        // A stack's card isn't one: a lead or a child routed small works as a normal card.
        let lead = card(json!({ "title": "T", "route": { "size": "small" }, "stack": [5], "critique": { "state": "passed", "round": 1, "oneFailed": true } }), "Planning");
        let child = card(json!({ "title": "T", "route": { "size": "small" }, "stackOf": 3, "critique": { "state": "passed", "round": 1, "oneFailed": true } }), "Planning");
        assert!(gate(&s, &lead, "In progress").is_none() && gate(&s, &child, "In progress").is_none());
        // Blocked mid-build goes back freely.
        assert!(gate(&s, &small(json!({ "state": "failed", "round": 1, "oneFailed": true, "started": true }), "Blocked"), "In progress").is_none());
        // Only moves into the build are held, and Hugh can turn the rule off.
        assert!(gate(&s, &small(json!({ "state": "failed", "round": 1, "oneFailed": true }), "Planning"), "Blocked").is_none());
        s.set_page_setting(kanban_store::CRITIC_RULE_KEY, "off").unwrap();
        assert!(gate(&s, &small(json!({ "state": "failed", "round": 1, "oneFailed": true }), "Planning"), "In progress").is_none());
    }

    #[test]
    fn a_question_for_hugh_holds_the_card_even_on_a_pass() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let asks = |column: &str, extra: Value| {
            let mut rec = json!({ "state": "passed", "round": 0, "hugh": true, "notes": ["HUGH: keep the old chip?"] });
            rec.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap());
            card(json!({ "title": "T", "critique": rec }), column)
        };
        for col in ["Planning", "Backlog", "Blocked"] {
            let why = gate(&s, &asks(col, json!({})), "In progress").expect(col);
            assert!(why.contains("asked Hugh something") && why.contains("only Hugh moves it on") && why.contains("makes a fresh critique needed"), "{why}");
        }
        assert!(crate::agents::move_refused(&s, &asks("Planning", json!({})), "Human test").is_some());
        // Not a card blocked mid-build, not a move elsewhere, not with the rule off.
        assert!(gate(&s, &asks("Blocked", json!({ "started": true })), "In progress").is_none());
        assert!(gate(&s, &asks("Planning", json!({})), "Backlog").is_none());
        s.set_page_setting(kanban_store::CRITIC_RULE_KEY, "off").unwrap();
        assert!(gate(&s, &asks("Planning", json!({})), "In progress").is_none());
    }

    #[test]
    fn the_build_gets_the_critics_notes() {
        let pass = card(json!({ "title": "T", "critique": { "state": "passed", "round": 0, "notes": ["name the scroller", "check 3 says it twice"] } }), "In progress");
        let n = build_notes(&pass).unwrap();
        assert!(n.starts_with("Build notes from the plan critic (its pass came with these notes):\n- name the scroller\n- check 3 says it twice\n"), "{n}");
        assert!(n.contains("need no new plan critique") && n.contains("sharpening doneWhenChecks"), "{n}");
        // A normal card that failed once, then passed with notes: still notes, not findings.
        let later = card(json!({ "title": "T", "route": { "size": "normal" }, "critique": { "state": "passed", "round": 1, "notes": ["n"] } }), "In progress");
        assert!(build_notes(&later).unwrap().starts_with("Build notes from the plan critic (its pass"));
        // A small card's FAIL findings: the plan was to be fixed for them.
        let failed = small(json!({ "state": "failed", "round": 1, "oneFailed": true, "notes": ["it drops the docs"] }), "In progress");
        let n = build_notes(&failed).unwrap();
        assert!(n.contains("findings of this card's one plan critique") && n.ends_with("- it drops the docs"), "{n}");
        for rec in [json!({ "state": "passed" }), json!({ "state": "passed", "notes": [] }), json!({ "state": "passed", "notes": ["  "] })] {
            assert!(build_notes(&card(json!({ "title": "T", "critique": rec }), "In progress")).is_none());
        }
        assert!(build_notes(&card(json!({ "title": "T" }), "In progress")).is_none());
    }

    #[test]
    fn a_small_cards_fail_is_its_notes_and_never_caps_it() {
        use crate::agents::{capped, one_critique, parse_verdict, record_of};
        let v = parse_verdict("CARD: #4\nVERDICT: FAIL\n- it drops the docs\n- HUGH: keep the old chip?");
        let c = small(json!({ "state": "failed", "round": 2 }), "Planning");
        let (mut rec, _) = record_of(&CHECK, &v, Some(3), "a1", "t", 2);
        assert_eq!(rec["round"], 3);
        assert!(one_critique(&CHECK, &c, &v, &mut rec), "the rule applies, so record_check skips the cap");
        assert!(capped(&CHECK, &rec, "Planning"), "the round alone would have capped it");
        // Its findings are its notes, its HUGH: question isn't (it holds the card instead).
        assert_eq!((rec["notes"].clone(), rec["oneFailed"].clone(), rec["hugh"].clone()), (json!(["it drops the docs"]), json!(true), json!(true)));
        // The critic run again anyway: the mark stays, and the first FAIL's notes come first.
        let failed_card = small(rec.clone(), "Planning");
        let again = parse_verdict("VERDICT: PASS\n- a new note");
        let (mut rec2, _) = record_of(&CHECK, &again, Some(3), "a2", "t2", 3);
        one_critique(&CHECK, &failed_card, &again, &mut rec2);
        assert_eq!((rec2["state"].as_str(), rec2["oneFailed"].clone(), rec2["notes"].clone()), (Some("passed"), json!(true), json!(["it drops the docs", "a new note"])));
        let bare = parse_verdict("VERDICT: PASS");
        let (mut rec3, _) = record_of(&CHECK, &bare, Some(3), "a3", "t3", 3);
        one_critique(&CHECK, &failed_card, &bare, &mut rec3);
        assert_eq!(rec3["notes"], json!(["it drops the docs"]));
        // A normal card: no notes from a FAIL, no mark, and the cap applies.
        let n = card(json!({ "title": "T", "route": { "size": "normal" } }), "Planning");
        let (mut rec, _) = record_of(&CHECK, &v, Some(3), "a1", "t", 2);
        assert!(!one_critique(&CHECK, &n, &v, &mut rec));
        assert!(rec.get("notes").is_none() && rec.get("oneFailed").is_none());
        // Another agent's FAIL on a small card: not the critic's rule, and no hugh flag.
        let (mut rec, _) = record_of(&crate::tester::CHECK, &v, Some(3), "a1", "t", 0);
        assert!(!one_critique(&crate::tester::CHECK, &c, &v, &mut rec));
        assert!(rec.get("hugh").is_none());
        // A small card's first PASS keeps its own notes, with no mark.
        let v = parse_verdict("VERDICT: PASS\n- a note");
        let fresh = small(json!({ "state": "needed", "round": 0 }), "Planning");
        let (mut rec, _) = record_of(&CHECK, &v, Some(3), "a1", "t", 0);
        one_critique(&CHECK, &fresh, &v, &mut rec);
        assert_eq!(rec["notes"], json!(["a note"]));
        assert!(rec.get("oneFailed").is_none());
    }

    #[test]
    fn the_third_fail_caps_only_in_planning() {
        let rec = json!({ "state": "failed", "round": 3 });
        assert!(crate::agents::capped(&CHECK, &rec, "Planning"));
        // Already Blocked, or moved on by Hugh: no second trip to Blocked.
        for col in ["Blocked", "In progress", "Backlog"] {
            assert!(!crate::agents::capped(&CHECK, &rec, col), "{col}");
        }
        assert!(!crate::agents::capped(&CHECK, &json!({ "state": "failed", "round": 2 }), "Planning"));
    }
}
