//! The app as the dispatcher: what Hugh's moves and edits send to a card's session, when
//! queued text is typed, held moves, ship results, and the 7-day account choice.

use crate::{sessions::Sessions, Board};
use kanban_store::{Card, Patch, Store};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

// Stacks build in parallel (card #113): the code lives in stackrun.rs. after_child_merge is for
// card #100's merge record.
#[allow(unused_imports)]
pub use crate::stackrun::{after_child_merge, child_note, child_ready, merge_brief, stack_started, start_stack, waits_on};

/// Fields whose edit by Hugh is passed on to the card's session.
pub const TOLD: &[&str] = &["summary", "nextStep", "blockedReason", "modelOverride", "effortOverride", "permissionOverride", "accountPin", "autoTo", kanban_store::BLOCKED_KEY];
/// Card #105: Hugh's edits of the "Done when..." line and its checks are passed on too, except
/// in the columns where he is testing or it is shipping: the card holds his ticks there, and a
/// message would wake the session for nothing.
pub const TOLD_DONE_WHEN: &[&str] = &[kanban_store::DONE_WHEN_KEY, kanban_store::DONE_WHEN_CHECKS_KEY];
const DONE_WHEN_QUIET: [&str; 3] = ["Human test", "Ship it", "Deploy complete"];

/// The checks as numbered lines with their state, for a message (not raw JSON).
fn checks_text(v: Option<&Value>) -> String {
    let Some(list) = v.and_then(Value::as_array).filter(|l| !l.is_empty()) else { return "(cleared)".into() };
    let lines: Vec<String> = list
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let state = c.get("state").and_then(Value::as_str).unwrap_or("open");
            let text = c.get("text").and_then(Value::as_str).unwrap_or("");
            let note = c.get("note").and_then(Value::as_str).map(|n| format!(" ({n})")).unwrap_or_default();
            format!("  {}. [{state}] {text}{note}", i + 1)
        })
        .collect();
    format!("\n{}", lines.join("\n"))
}

/// The folder `{prompts}` names in the board's column prompts and templates (card #114): the
/// private folder's `prompts` if set (Settings > Private folder), else the prompts this build
/// carries (a test build: the checkout it was built from, `PROMPTS_DIR`, card #96). See
/// `private::prompts_dir`.
fn prompts_dir() -> std::path::PathBuf {
    crate::private::prompts_dir()
}

/// `text` with `{prompts}` filled in as `dir`. `{prompts}\name` takes the system's separator,
/// so on Windows the text reads exactly as the full path did before.
fn prompts_in(text: &str, dir: &std::path::Path) -> String {
    let dir = dir.display().to_string();
    text.replace("{prompts}\\", &format!("{dir}{}", std::path::MAIN_SEPARATOR)).replace("{prompts}", &dir)
}

/// `text` with `{prompts}` filled in.
pub fn with_prompts(text: &str) -> String {
    prompts_in(text, &prompts_dir())
}

/// `text` from the app itself with `{prompts}` filled in as `PROMPTS_DIR` (card #96): main's
/// prompts for a live build, the checkout it was built from for a test build.
fn app_prompts(text: &str) -> String {
    prompts_in(text, &prompts_dir())
}

/// A file in the prompts folder, for an app message: `{prompts}\name` as `app_prompts` fills it.
fn prompt_file(name: &str) -> String {
    app_prompts(&format!("{{prompts}}\\{name}"))
}

pub(crate) fn fill(template: &str, card: &Card, from: &str, reason: &str) -> String {
    // The folder first, so nothing from the card (its title, a reason) is taken for it.
    let template = with_prompts(template);
    let issue = card
        .fields
        .get("issue")
        .and_then(|i| i.get("number").or(Some(i)))
        .and_then(|n| n.as_i64())
        .map(|n| format!(", issue {n}"))
        .unwrap_or_default();
    let repo = card.fields.get("repo").and_then(Value::as_str).unwrap_or("no repo");
    let reason = if reason.is_empty() { String::new() } else { format!(": {reason}") };
    template
        .replace("{n}", &card.number.to_string())
        .replace("{title}", card.title())
        .replace("{repo}", repo)
        .replace("{issue}", &issue)
        .replace("{from}", from)
        .replace("{reason}", &reason)
}

/// Who the session is, for a session that doesn't know its card yet.
pub fn header(card: &Card) -> String {
    fill(
        "You are the session for card #{n}, \"{title}\" ({repo}{issue}). Read and update your card with the kanban_* tools (or `kb` in Bash). Never move a card into Ship it, never merge, never push to main.",
        card,
        "",
        "",
    )
}

pub fn dry(card: &Card, text: String) -> String {
    if card.fields.get("dryRun").and_then(Value::as_bool) == Some(true) {
        format!("Dry run: say what you would do and change nothing.\n\n{text}")
    } else {
        text
    }
}

/// Did the app move the card into its column for Hugh (card #76): a plan or ship it asked for
/// because he set the card to go on by itself, or to start when he added it.
fn auto_moved(card: &Card) -> bool {
    let req = |k: &str| {
        card.fields.get(k).map_or(false, |o| o.get("auto") == Some(&json!(true)) && matches!(o.get("state").and_then(Value::as_str), Some("requested" | "started" | "queued")))
    };
    (card.column == "Planning" && req("plan"))
        || (card.column == "Ship it" && req("ship"))
        // Card #162: the app's move for Hugh into Planning went to In progress (it skips Planning).
        || (card.column == "In progress" && card.planning_skipped() && card.fields.get("plan").and_then(|p| p.get("auto")) == Some(&json!(true)))
}

/// Did the app move the card into Planning for Hugh because the cards it waited on are ready
/// (card #109): he had put it in Blocked to start then.
fn waits_moved(card: &Card) -> bool {
    let into = |col: &str, states: &[&str]| {
        card.column == col
            && card.fields.get("plan").map_or(false, |p| {
                p.get(kanban_store::WAITED_KEY).and_then(Value::as_array).map_or(false, |a| !a.is_empty())
                    && p.get("state").and_then(Value::as_str).is_some_and(|s| states.contains(&s))
            })
    };
    // Card #162: into In progress instead, for a card that skips Planning.
    into("Planning", &["requested", "started"]) || into("In progress", &[kanban_store::PLAN_SKIPPED])
}

/// Did `card` (now in In progress) just come from `from` (Backlog or Blocked) by a move into
/// Planning that skipped it (card #162)? Whose choice it was: "card" or "default".
pub fn planning_skipped_from(card: &Card, from: &str) -> Option<kanban_store::Whose> {
    if card.column != "In progress" || !card.planning_skipped() || !matches!(from, "Backlog" | "Blocked") {
        return None;
    }
    Some(match card.fields.get("plan").and_then(|p| p.get("whose")).and_then(Value::as_str) {
        Some("card") => kanban_store::Whose::Card,
        _ => kanban_store::Whose::Default,
    })
}

/// "his choice for this card" or "his default in Settings", for a session's note (card #162).
fn whose_choice(w: kanban_store::Whose) -> &'static str {
    match w {
        kanban_store::Whose::Card => "his choice for this card",
        kanban_store::Whose::Default => "his default in Settings > Sessions",
    }
}

/// What a card's session is told about Hugh's flow options for the card (card #162): a Planning
/// he skipped (the card has just come into In progress from Backlog or Blocked), and a Human test
/// he skips (in In progress: the app ships it from Human test; in Ship it: this is his approval).
/// None when neither applies. `autoTo` wins over the skip, so a card with one is told that instead.
pub fn flow_note(store: &Store, card: &Card, from: &str) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(w) = planning_skipped_from(card, from) {
        parts.push(format!("{} (Planning skipped by {}).", SKIPPED_PLANNING, whose_choice(w)));
    }
    let (skip, w) = store.skip_of(card, kanban_store::SKIP_HUMAN_TEST_KEY);
    if skip && !card.archived && card.auto_to().is_none() {
        match card.column.as_str() {
            "In progress" => parts.push(format!("{} (Human test skipped by {}).", SKIPS_HUMAN_TEST, whose_choice(w))),
            "Ship it" if auto_moved(card) && !crate::goes_on::went_on(card) => parts.push(format!("{} (Human test skipped by {}).", SHIP_NO_HUMAN_TEST, whose_choice(w))),
            _ => {}
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// The In progress message's note for a card whose Planning Hugh skipped (card #162).
pub const SKIPPED_PLANNING: &str = "Hugh skipped Planning for this card, so it came straight to In progress: there is no plan and no plan critique. Read the card first (kanban_get_card). If this message doesn't name a worktree the app made for this card, make the card's own worktree and branch first, as the repo's Planning step does. Then write a short approach into the card's summary (kanban_update_card), and build it";
/// The In progress message's note for a card that skips Human test (card #162).
pub const SKIPS_HUMAN_TEST: &str = "Hugh set this card to skip Human test. Once you move it to Human test (after its subagent checks pass, with the hand-over as usual), the app moves it into Ship it for him and sends you the Ship it prompt. That is his approval to ship it with no human test, so make your own testing count";
/// The Ship it message's note for a card the app moved on because it skips Human test (card #162).
pub const SHIP_NO_HUMAN_TEST: &str = "This is Hugh's approval to ship it with no human test: he set the card to skip Human test. Ship it as the Ship it prompt says, then move it to Deploy complete";

/// What a card that goes on by itself (`autoTo`, Hugh's "Take it to") tells its session in the
/// column it is in. None for any other card, a stack's child, or a column with nothing to say.
pub fn auto_note(card: &Card) -> Option<String> {
    // Small-change auto-ship (card #104) sets no autoTo, so it is read first.
    if let Some(note) = crate::goes_on::ship_note(card) {
        return Some(note);
    }
    let t = card.auto_to()?;
    if card.stack_of().is_some() || card.archived {
        return None;
    }
    Some(match card.column.as_str() {
        "Planning" => format!("Hugh set this card to go on by itself to {t} (Take it to). When your plan is ready, its card-critic has passed it (the planning prompt's critic step, unless the app said the plan critique is off) and nothing in it is his to decide, move this card to In progress yourself (kanban_move_card) as the last thing in your turn, instead of waiting for him: the app then sends you the In progress prompt. If something is his to decide (a HUGH: line from the critic), ask it and leave the card in Planning: it waits for him. A card routed small gets one critique: if it fails, fix the plan, don't run the critic again, and leave the card for Hugh."),
        "In progress" if t == "Human test" => "Hugh set this card to go on by itself to Human test: hand it over there as usual (after its subagent test passes), and it stops there for him.".into(),
        "In progress" => "Hugh set this card to go on by itself to Deploy complete. Once you move it to Human test (after its subagent test passes, with the hand-over as usual), the app moves it into Ship it for him and sends you the Ship it prompt. That is his approval to ship it with no human test, so make your own testing count.".into(),
        "Ship it" if auto_moved(card) => "This is Hugh's approval to ship it with no human test: he set the card to go on by itself to Deploy complete. Ship it as the Ship it prompt says, then move it to Deploy complete.".into(),
        _ => return None,
    })
}

/// The message for Hugh's move of `card` (already moved) from `from`, or the app's move for him.
pub fn move_message(store: &Store, card: &Card, from: &str, new_session: bool) -> String {
    let to = &card.column;
    let mut text = if auto_moved(card) {
        let why = match card.auto_to() {
            Some(t) => format!("he set it to go on by itself to {t}"),
            None if crate::goes_on::went_on(card) => "the change was small and fully checked, and his small-change auto-ship is on for this repo".to_string(),
            // Card #162: the app moves a card into Ship it for Hugh with no autoTo when it skips Human test.
            None if to == "Ship it" => "he set it to skip Human test, which is his approval to ship it with no human test".to_string(),
            None if card.planning_skipped() => "he asked for it to start when he added it".to_string(),
            None => format!("he asked for it to start in {to} when he added it"),
        };
        format!("The app moved this card from {from} to {to} for Hugh: {why}.")
    } else if waits_moved(card) {
        format!("The app moved this card from {from} to {to} for Hugh: the cards it waited on are ready.")
    } else {
        format!("Hugh moved this card from {from} to {to}.")
    };
    let cfg = store.column(to).ok().flatten();
    if let Some(p) = cfg.as_ref().filter(|c| c.mode != "none").and_then(|c| c.prompt_text.clone()) {
        let reason = card.fields.get("blockedReason").and_then(Value::as_str).unwrap_or("");
        let prompt = fill(&p, card, from, reason);
        if new_session && !prompt.starts_with("You are the session") {
            text = format!("{}\n\n{text}", header(card));
        }
        text = format!("{text}\n\n{prompt}");
    } else if new_session {
        text = format!("{}\n\n{text}", header(card));
    }
    if from == "Human test" && to == "In progress" {
        text = format!("{text}\n\n{}", back_from_test());
    }
    if let Some(note) = subagents_note(store, card, to) {
        text = format!("{text}\n\n{note}");
    }
    // A repo that ships through its merge lane (card #160).
    if let Some(note) = crate::lane::lane_note(store, card, to) {
        text = format!("{text}\n\n{note}");
    }
    if let Some(note) = auto_note(card) {
        text = format!("{text}\n\n{note}");
    }
    // Card #162: Hugh's flow options (a Planning he skipped, a Human test he skips).
    if let Some(note) = flow_note(store, card, from) {
        text = format!("{text}\n\n{note}");
    }
    if let Some(note) = waits_moved(card).then(|| store.wait_note(card)).flatten() {
        text = format!("{text}\n\n{note}");
    }
    if let Some(notes) = build_notes_for(card, from) {
        text = format!("{text}\n\n{notes}");
    }
    dry(card, with_worktree_note(store, card, text))
}

/// The plan critic's build notes (card #144) for a card that has just come into In progress
/// from before its build (Planning or Backlog): not from Human test or Blocked, where the build
/// had already had them.
pub fn build_notes_for(card: &Card, from: &str) -> Option<String> {
    (card.column == "In progress" && matches!(from, "Planning" | "Backlog")).then(|| crate::critic::build_notes(card)).flatten()
}

/// What a card's session in `column` is told about the subagents run there that Hugh has off on
/// the card (card #151; card #97's plan critique note before it): one line each, saying whether it
/// is the card's own choice or the board's default it follows. None when they are all on.
pub fn subagents_note(store: &Store, card: &Card, column: &str) -> Option<String> {
    // Card #161: a card in Planning that isn't sized runs no card-router (Hugh's defaults).
    let not_sized = (column == "Planning" && card.column == "Planning").then(|| crate::presize::unsized_note(store, card)).flatten();
    let off = store.subagents_off_in(card, column);
    if off.is_empty() {
        return not_sized;
    }
    let lines: Vec<String> = off
        .iter()
        .map(|s| {
            let whose = if card.subagent_choice(s.key).is_some() { "off for this card" } else { "off by the board's default in Settings > Sessions" };
            format!("- {} ({whose}): {}", s.name, s.skip)
        })
        .collect();
    let text = format!("Hugh has these subagents off on this card. Don't run them; only he turns them back on (the card's details).\n{}", lines.join("\n"));
    Some(match not_sized {
        Some(u) => format!("{text}\n\n{u}"),
        None => text,
    })
}

/// The message for the app's own move of `card` (already moved) from `from`, when it moved the
/// card by a rule of Hugh's (card #98: a card routed small, its plan critique passed): `lead`
/// says why, then the column's prompt follows, as for his move.
pub fn app_move_message(store: &Store, card: &Card, from: &str, lead: &str) -> String {
    let to = &card.column;
    let mut text = format!("The app moved this card from {from} to {to}: {lead}");
    let cfg = store.column(to).ok().flatten();
    if let Some(p) = cfg.as_ref().filter(|c| c.mode != "none").and_then(|c| c.prompt_text.clone()) {
        text = format!("{text}\n\n{}", fill(&p, card, from, ""));
    }
    if let Some(note) = subagents_note(store, card, to) {
        text = format!("{text}\n\n{note}");
    }
    // A repo that ships through its merge lane (card #160).
    if let Some(note) = crate::lane::lane_note(store, card, to) {
        text = format!("{text}\n\n{note}");
    }
    if let Some(note) = auto_note(card) {
        text = format!("{text}\n\n{note}");
    }
    // Card #162: Hugh's flow options (a Planning he skipped, a Human test he skips).
    if let Some(note) = flow_note(store, card, from) {
        text = format!("{text}\n\n{note}");
    }
    // A stack's child builds itself, in parallel with its siblings (card #113).
    if let Ok(Some(lead)) = store.stack_lead(card) {
        text = format!("{text}\n\n{}", child_note(card, &lead));
    }
    if let Some(notes) = build_notes_for(card, from) {
        text = format!("{text}\n\n{notes}");
    }
    dry(card, with_worktree_note(store, card, text))
}

/// Card #146: what a session in the worktree the app made for its card is told, after any
/// column prompt that says to work in the shared checkout. None for any other card.
pub fn worktree_note(_store: &Store, card: &Card) -> Option<String> {
    crate::worktree::made_for(card, kanban_store::channel()).map(|p| crate::worktree::note(&p))
}

/// The card's worktree note (card #146), unless the message already waiting for its session
/// has it (a new session's first message does).
pub fn worktree_note_unsent(store: &Store, card: &Card) -> Option<String> {
    worktree_note(store, card).filter(|n| !store.waiting(&card.id, "message").ok().flatten().is_some_and(|w| w.payload.contains(n.as_str())))
}

/// `text` with the card's worktree note at the end, unless it is waiting already.
fn with_worktree_note(store: &Store, card: &Card, text: String) -> String {
    match worktree_note_unsent(store, card) {
        Some(n) => format!("{text}\n\n{n}"),
        None => text,
    }
}

/// What a card back in In progress from Human test is to do (card #93).
fn back_from_test() -> String {
    app_prompts(BACK_FROM_TEST)
}

const BACK_FROM_TEST: &str = "It came back from Human test, so something Hugh tested needs more work: his latest messages and the card's latest log lines say what. Fix that through the build-and-verify loop ({prompts}\\in-progress.md), run a fresh card-reviewer and card-tester if your session has them and Hugh hasn't turned them off for this card (the move to Human test needs a new pass from each one that is on), write a new hand-over into nextStep, and move the card back to Human test. If his message needs no change (a question, say), answer it, then still run them and move it back. If your session has the card-reproducer subagent: when his message reports a bug he saw, run it before the fix and again after it (the app records both runs on the card); else log \"Not a bug report: <why>\" on the card.";

/// The column prompts' folder in the checkout this build was made from (`build.rs`, card #96):
/// a test build from a worktree reads that worktree's prompts, a live build from main reads main's.
pub const PROMPTS_DIR: &str = env!("KANBAN_PROMPTS_DIR");

/// Does Hugh's message from the chat take `card` back to In progress first (card #93)? Only a
/// card in Human test that is not in a stack: a stack's cards move by the stack's rules (and
/// its children by Hugh's "Found a problem").
pub fn message_moves_back(card: &Card) -> bool {
    card.column == "Human test" && !card.archived && card.stack_of().is_none() && !card.is_lead()
}

/// Hugh's message `text` to a card his message just moved from Human test to In progress, with
/// a note that tells its session why the card moved and what to do.
pub fn moved_back_message(store: &Store, card: &Card, text: &str) -> String {
    let note = format!("Hugh's message below moved this card from Human test back to In progress (the app does that when he writes to a card in Human test). {}", back_from_test());
    let off = off_in_progress(store, card);
    dry(card, format!("{note}{off}\n\nHugh's message:\n\n{text}"))
}

/// The subagents Hugh has off on `card` for In progress (card #151), as a paragraph to add to a
/// message that sends it back there; "" when they are all on.
fn off_in_progress(store: &Store, card: &Card) -> String {
    subagents_note(store, card, "In progress").map(|n| format!("\n\n{n}")).unwrap_or_default()
}

/// What Hugh's Fail in Human test does to a card (card #102): an ordinary card goes back to In
/// progress by card #93's rule (`back_to_work`); a stack's lead goes back by "Found a problem"'s
/// steps. A stack's child isn't failed here: the board sends its Fail to `stack_problem`.
#[derive(Debug, Clone, PartialEq)]
pub enum FailPlan {
    MoveBack,
    LeadBack,
}

/// Checks Hugh's Fail on `card` with `note` (card #102), or says why it can't be done.
pub fn fail_plan(card: &Card, note: &str) -> Result<FailPlan, String> {
    if note.trim().is_empty() {
        return Err("say what's wrong: the note goes to the card's session".into());
    }
    if card.archived {
        return Err(format!("#{} is archived", card.number));
    }
    if card.column != "Human test" {
        return Err(format!("#{} isn't in Human test any more: it's in {} now", card.number, card.column));
    }
    if let Some(lead) = card.stack_of() {
        return Err(format!("#{} is in stack #{lead}: use Found a problem on it", card.number));
    }
    Ok(if card.is_lead() { FailPlan::LeadBack } else { FailPlan::MoveBack })
}

/// The log line for Hugh's Fail (card #102). Card #112's reproducer is told through its own
/// call; this line is the card's record of why it went back.
pub fn failed_log(note: &str) -> String {
    format!("Failed in Human test: {}", note.trim())
}

/// Hugh's Fail note, as the message to an ordinary card his Fail moved back to In progress:
/// what happened and what to do (card #93's text), then his note.
pub fn failed_message(store: &Store, card: &Card, note: &str) -> String {
    dry(card, format!("Hugh failed this card in Human test, which moved it back to In progress. {}{}\n\nHugh's note:\n\n{}", back_from_test(), off_in_progress(store, card), note.trim()))
}

/// Hugh's Fail note on a stack's lead, as the message to the lead's session: the lead is back in
/// In progress, as "Found a problem" does for a child.
pub fn lead_failed_message(store: &Store, card: &Card, note: &str) -> String {
    dry(
        card,
        format!(
            "Hugh tested stack #{n} \"{t}\" in Human test and failed it: {note}\n#{n} is back in In progress. Fix it through the build-and-verify loop ({ip}), run a fresh card-reviewer and card-tester if your session has them and they are on for this card, write a new hand-over into nextStep, and move it back to Human test.{off}",
            n = card.number,
            t = card.title(),
            note = note.trim(),
            ip = prompt_file("in-progress.md"),
            off = off_in_progress(store, card)
        ),
    )
}

/// The starting prompt of the template the card was made from (`templateId`), sent once, in a
/// new session's first message (see `opening`), ahead of the column prompt that a move adds.
/// An archived template still counts for the cards made from it.
pub fn template_prompt(store: &Store, card: &Card) -> Option<String> {
    let id = card.fields.get("templateId").and_then(Value::as_str)?;
    let t = store.template(id).ok().flatten()?;
    let p = t.start_prompt.trim();
    if p.is_empty() {
        return None;
    }
    Some(format!("This card was made from the \"{}\" template. {}", t.name, fill(p, card, "", "")))
}

/// A new session's first message: who it is, then its card's template starting prompt.
pub fn opening(store: &Store, card: &Card) -> String {
    let text = match template_prompt(store, card) {
        Some(start) => format!("{}\n\n{start}", header(card)),
        None => header(card),
    };
    match worktree_note(store, card) {
        Some(n) => format!("{text}\n\n{n}"),
        None => text,
    }
}

/// The message for a session's own held move of `card` (already moved), when the column it
/// went into runs its prompt at once, or it is In progress on a card that goes on by itself
/// (card #76: its session moved it there from Planning). None for any other column.
/// Card #162: a move into Planning that skipped it counts too (Hugh's skip took the card to
/// In progress); a card that only skips Human test does not: that is no approval to build.
pub fn held_move_message(store: &Store, card: &Card, from: &str) -> Option<String> {
    let cfg = store.column(&card.column).ok().flatten()?;
    let auto = card.column == "In progress" && (auto_note(card).is_some() || planning_skipped_from(card, from).is_some()) && cfg.mode != "none";
    if cfg.mode != "immediate" && !auto {
        return None;
    }
    let reason = card.fields.get("blockedReason").and_then(Value::as_str).unwrap_or("");
    let prompt = fill(cfg.prompt_text.as_deref()?, card, "", reason);
    let mut text = format!("Your move of this card to {} is done.

{prompt}", card.column);
    if let Some(note) = subagents_note(store, card, &card.column) {
        text = format!("{text}\n\n{note}");
    }
    if let Some(note) = crate::lane::lane_note(store, card, &card.column) {
        text = format!("{text}\n\n{note}");
    }
    if let Some(note) = auto_note(card) {
        text = format!("{text}\n\n{note}");
    }
    // Card #162: Hugh's flow options (a Planning he skipped, a Human test he skips).
    if let Some(note) = flow_note(store, card, from) {
        text = format!("{text}\n\n{note}");
    }
    if let Some(notes) = build_notes_for(card, from) {
        text = format!("{text}\n\n{notes}");
    }
    Some(dry(card, with_worktree_note(store, card, text)))
}

/// After a card moved by a session or the stack rules, or Hugh changed where it goes (card
/// #76): a card that reached the column Hugh set it to go on to stops going on; one that goes
/// to Deploy complete and is now in Human test is moved into Ship it for him, and its session
/// is sent the Ship it prompt (or it waits its turn behind another ship of its repo). A stack's
/// child never goes on by itself; its stack does. Card #162: one that skips Human test goes on
/// the same way, unless Hugh moved it there himself or "Take it to: Human test" stopped it there.
pub fn auto_progress(app: &AppHandle, store: &mut Store, card_id: &str) {
    let Ok(card) = store.get(card_id) else { return };
    if card.archived || card.stack_of().is_some() {
        return;
    }
    let by = kanban_store::AUTO_BY;
    let t = card.auto_to().map(str::to_string);
    if let Some(t) = t.as_deref().filter(|t| *t == card.column) {
        let mut patch = Patch::new();
        patch.insert(kanban_store::AUTO_KEY.into(), Value::Null);
        if store.update(&card.id, card.version, &patch, by).is_ok() {
            let _ = store.add_log(&card.id, &format!("Auto-progress done: it reached {t}, where Hugh set it to go"), by);
        }
        return;
    }
    if card.column != "Human test" {
        return;
    }
    let why = match t.as_deref() {
        Some("Deploy complete") => "he set it to go on by itself to Deploy complete, which is his approval to ship it with no human test".to_string(),
        // Take it to wins over the skip: it stops in Human test for him.
        Some(_) => return,
        None => match skip_human_test_why(store, &card) {
            Some(w) => w,
            None => return,
        },
    };
    // A stack ships whole: only once every card in it is ready (in Human test or later, each
    // through its own subagent test), whatever the board's rule for moving the stack.
    if card.is_lead() {
        let kids = store.stack_children(&card).unwrap_or_default();
        if !kanban_store::stack_ready(&card, &kids, "all") {
            return;
        }
    }
    match store.move_for_hugh(&card.id, card.version, "Ship it", &why) {
        Ok(_) => {
            after_stack_move(app, store, &card.id, by);
            let (a, id) = (app.clone(), card.id.clone());
            std::thread::spawn(move || after_move(&a, &id, "Human test"));
        }
        Err(e) => {
            let _ = store.add_log(&card.id, &format!("Couldn't move it on into Ship it for Hugh: {e}"), by);
        }
    }
}

/// Why `card`, in Human test with no `autoTo`, goes on into Ship it because it skips Human test
/// (card #162), if it does: its own skipHumanTest, else the default. Not when Hugh moved it into
/// Human test himself (his drag never skips, nor does his move from `kb` or his own session
/// outside the app), when its autoTo was cleared since it arrived (Take it to: Human test stopped
/// it there for him), or when no move put it there (an import).
pub fn skip_human_test_why(store: &Store, card: &Card) -> Option<String> {
    let (on, whose) = store.skip_of(card, kanban_store::SKIP_HUMAN_TEST_KEY);
    if !on || card.column != "Human test" || card.auto_to().is_some() {
        return None;
    }
    let a = store.arrival(&card.id, "Human test").ok().flatten()?;
    if hughs_own(&a.by) || a.auto_cleared {
        return None;
    }
    Some(format!("he set it to skip Human test ({}), which is his approval to ship it with no human test", whose.whose()))
}

/// Was a revision by `by` Hugh's own (card #162)? His board, panel or Settings ("Hugh (...)"),
/// and his `kb` or his own sessions outside the app (`tools::OUTSIDE_BY`). Not a card's session,
/// the Router, the stack rules, the app for him, or a session the app doesn't know.
pub fn hughs_own(by: &str) -> bool {
    by.starts_with("Hugh (") || by == crate::tools::OUTSIDE_BY
}

/// A card made into Planning or In progress by Hugh (the Add card form's "Take it to", or his
/// own session's `kb create`): the app moves it there for him from Backlog, so it starts as his
/// move would (a plan request, the column's session), with `why` in the log. The session
/// starts once the caller lets go of the board.
pub fn start_new(app: &AppHandle, store: &mut Store, card: &Card, column: &str, why: &str) -> Result<Card, String> {
    let m = store.move_for_hugh(&card.id, card.version, column, why).map_err(|e| e.to_string())?;
    let (a, id) = (app.clone(), card.id.clone());
    std::thread::spawn(move || after_move(&a, &id, "Backlog"));
    Ok(m.card)
}

/// The message for Hugh's edit of `changed` fields.
pub fn edit_message(card: &Card, changed: &[&str]) -> Option<String> {
    let quiet = DONE_WHEN_QUIET.contains(&card.column.as_str());
    let told: Vec<&str> = changed.iter().copied().filter(|k| TOLD.contains(k) || (!quiet && TOLD_DONE_WHEN.contains(k))).collect();
    if told.is_empty() {
        return None;
    }
    let mut lines = vec![format!("Hugh edited {} on this card:", told.join(", "))];
    for k in &told {
        if *k == kanban_store::DONE_WHEN_CHECKS_KEY {
            lines.push(format!("- {k}:{}", checks_text(card.fields.get(*k))));
            continue;
        }
        if *k == kanban_store::BLOCKED_KEY {
            let on = card.blocked_by();
            lines.push(if on.is_empty() {
                "- blockedBy: (cleared). It no longer waits on other cards.".into()
            } else {
                format!(
                    "- blockedBy: {}. This card waits on those cards. Don't start work that needs theirs before they are ready (Human test, or shipped for another repo): the app tells you then.",
                    on.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(", ")
                )
            });
            continue;
        }
        if *k == "autoTo" {
            lines.push(match card.auto_to() {
                Some(t) => format!("- autoTo: {t}. The card now goes on by itself to {t}: in Planning, once your plan is ready, its card-critic has passed it (unless the plan critique is off) and nothing in it is his to decide (no HUGH: line), move it to In progress yourself (a card routed small gets one critique: after a FAIL it waits for him); {}", if t == "Deploy complete" { "from Human test the app moves it into Ship it for him, which is his approval to ship it with no human test." } else { "it stops in Human test for him." }),
                None => "- autoTo: (cleared). The card no longer goes on by itself: wait for Hugh at each step as usual.".into(),
            });
            continue;
        }
        let v = card.fields.get(*k).map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()));
        lines.push(format!("- {k}: {}", v.unwrap_or_else(|| "(cleared)".into())));
        if k.ends_with("Override") || *k == "accountPin" {
            lines.push("  (this applies when the session next starts)".into());
        }
    }
    Some(lines.join("\n"))
}

/// After Hugh moved a card: stop it (Done), queue the message, or start a session. A stack's
/// lead moved into Planning or In progress then starts its children (card #113, R1).
pub fn after_move(app: &AppHandle, card_id: &str, from: &str) {
    // A card that went on by itself is checked again before its ship's prompt goes (card #104).
    if crate::goes_on::ship_start_refused(app, card_id) {
        return;
    }
    after_move_checked(app, card_id, from);
}

/// After the Router's move of a card (card #119, final-test F1): the card's own after-move, as
/// for Hugh's drag, so its session starts (a column that auto-spawns) or gets the column's
/// message. Not a stack lead's children: only Hugh's move starts those (card #113, R1).
pub fn after_router_move(app: &AppHandle, card_id: &str, from: &str) {
    if crate::goes_on::ship_start_refused(app, card_id) {
        return;
    }
    after_move_card(app, card_id, from);
}

/// `after_move` for a card whose small-change check has just passed (`goes_on::go_on`), so
/// its ship's start isn't checked a second time. A stack's lead then starts its children.
pub fn after_move_checked(app: &AppHandle, card_id: &str, from: &str) {
    after_move_card(app, card_id, from);
    let board = app.state::<Board>();
    let Ok(mut store) = board.0.lock() else { return };
    if store.get(card_id).map_or(false, |c| c.is_lead()) {
        start_stack(app, &mut store, card_id);
    }
}

/// After the waits poll moved a card from Blocked to Planning (card #109, `pump_waits`): the
/// card's own after-move only (its session starts). A stack's lead started this way doesn't
/// start its children: they follow the stack's rules, so they start when Hugh moves the lead
/// into Planning or In progress himself (card #113, R1). Router decision on the whole-build
/// review, N2.
pub fn after_wait_release(app: &AppHandle, card_id: &str) {
    after_move_card(app, card_id, "Blocked");
}

fn after_move_card(app: &AppHandle, card_id: &str, from: &str) {
    let live = app.state::<Arc<Sessions>>().inner().clone();
    let board = app.state::<Board>();
    let Ok(mut store) = board.0.lock() else { return };
    let Ok(card) = store.get(card_id) else { return };
    if card.column == "Human test" && from != "Human test" { crate::changes::on_entered_human_test(app, card_id); }
    let cfg = store.column(&card.column).ok().flatten();
    if cfg.as_ref().map(|c| c.on_enter.as_str()) == Some("suspend") {
        if let Ok(Some(_)) = store.unqueue_start(card_id) {
            let _ = store.add_log(card_id, "Took the waiting start out of the queue: the card's column stops sessions", "the app");
            let running = live.running_count();
            let _ = app.emit("session-queue", crate::queue_json(&store, running));
        }
        drop(store);
        if live.is_live(card_id).is_some() {
            let (app, id) = (app.clone(), card_id.to_string());
            std::thread::spawn(move || {
                let _ = app.state::<Arc<Sessions>>().stop(&app, &id);
            });
        }
        return;
    }
    if left_ship_for_stack(&card) {
        return;
    }
    // One ship at a time per repo (card #79): behind another card's ship, it waits in the
    // queue and its session hears nothing until `pump_ships` releases it.
    // `move_card` usually queued it already; this catches any other way into Ship it.
    if card.column == "Ship it" {
        let state = card.fields.get("ship").and_then(|s| s.get("state")).and_then(Value::as_str);
        if state == Some(kanban_store::SHIP_QUEUED) {
            return;
        }
        if state == Some("requested") {
            if let Ok(Some(b)) = store.ship_blocker(&card) {
                let _ = store.ship_wait(card_id, &b, from);
                drop(store);
                let _ = app.emit("cards-changed", ());
                return;
            }
        }
    }
    if live.is_live(card_id).is_some() {
        let text = move_message(&store, &card, from, false);
        let _ = store.queue(card_id, "message", &text);
        mark_handed(&mut store, &card);
        return;
    }
    if !cfg.as_ref().map_or(false, |c| c.auto_spawn) {
        return;
    }
    // A conversation that lives in the desktop app is resumed only when Hugh asks.
    let app_convo = store.latest_session(card_id).ok().flatten();
    if app_convo.is_none() && card.fields.get("claudeSessionId").is_some() {
        let _ = store.add_log(
            card_id,
            "Not started: this card's conversation is in the Claude desktop app. Open the card and choose Resume in the app, or Start a new session.",
            "the app",
        );
        drop(store);
        let _ = app.emit("cards-changed", ());
        return;
    }
    let mode = if app_convo.is_some() { "resume" } else { "new" };
    // Over the cap it waits in the queue; the message waits too, and is typed once it starts.
    match crate::begin_session(app, &mut store, &card, mode, "move") {
        Ok(begun) => {
            // A new session that started now has already been sent its header (start_session's
            // opening message), so the move's message leaves it out. A queued one gets it here,
            // since its message is waiting before it starts.
            let started = matches!(begun, crate::Begun::Started(_));
            let text = move_message(&store, &card, from, mode == "new" && !started);
            let _ = store.queue(card_id, "message", &text);
            mark_handed(&mut store, &card);
        }
        Err(e) => {
            let _ = store.add_log(card_id, &format!("Couldn't start a session: {e}"), "the app");
        }
    }
    drop(store);
    let _ = app.emit("cards-changed", ());
}

/// A plan or ship request that now has a session on it.
fn mark_handed(store: &mut Store, card: &Card) {
    // Read again: starting the session can have written the card since (card #113's mark).
    let fresh = store.get(&card.id).ok();
    let card = fresh.as_ref().unwrap_or(card);
    let mut patch = Patch::new();
    for key in ["plan", "ship"] {
        if let Some(obj) = card.fields.get(key).and_then(Value::as_object) {
            if obj.get("state").and_then(Value::as_str) == Some("requested") {
                let mut o = obj.clone();
                if key == "plan" {
                    o.insert("state".into(), json!("started"));
                }
                o.insert("detail".into(), json!("Handed to the card's session in the app"));
                if key == "ship" {
                    // From now it holds its repo's place in the ship queue (card #79).
                    o.insert("handedAt".into(), json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)));
                }
                patch.insert(key.into(), Value::Object(o));
            }
        }
    }
    if !patch.is_empty() {
        let _ = store.update(&card.id, card.version, &patch, "the app");
    }
}

pub fn after_edit(app: &AppHandle, card_id: &str, changed: &[&str]) {
    let board = app.state::<Board>();
    let Ok(mut store) = board.0.lock() else { return };
    // Hugh set where it goes: a card already there stops going on, and one waiting in Human
    // test to ship moves on. First, so the session is told how it stands after that.
    // Card #162: so does a card a session put in Human test that he now set to skip it.
    let auto = changed.contains(&kanban_store::AUTO_KEY) || changed.contains(&kanban_store::SKIP_HUMAN_TEST_KEY);
    if auto {
        auto_progress(app, &mut store, card_id);
    }
    let Ok(card) = store.get(card_id) else { return };
    let has_session = app.state::<Arc<Sessions>>().is_live(card_id).is_some() || store.latest_session(card_id).ok().flatten().is_some();
    if let (true, Some(text)) = (has_session, edit_message(&card, changed)) {
        let _ = store.queue(card_id, "message", &text);
    }
    // Card #162: Hugh changed whether it skips Planning or Human test (update_card logs it).
    if let (true, Some(text)) = (has_session, skips_changed_note(&store, &card, changed)) {
        let _ = store.queue(card_id, "message", &dry(&card, text));
    }
    // Card #151: Hugh changed the card's subagents (update_card logs it). Its session hears it,
    // so it runs or skips them from here on; a lead held for its own test, review or proof may
    // be free now, so its stack's rules run again.
    if changed.contains(&kanban_store::SUBAGENTS_KEY) {
        let (_, note) = subagents_changed(&store, &card);
        if has_session {
            let _ = store.queue(card_id, "message", &dry(&card, note));
        }
        if card.is_lead() {
            after_lead_verdict(app, &mut store, card_id, "Hugh (board)", &format!("Hugh changed #{}'s subagents", card.number));
            drop(store);
            let _ = app.emit("cards-changed", ());
            return;
        }
    }
    if auto {
        drop(store);
        let _ = app.emit("cards-changed", ());
    }
}

/// The log line and the session's note for Hugh's change of `card`'s subagents (card #151).
pub fn subagents_changed(store: &Store, card: &Card) -> (String, String) {
    let names = |on: bool| -> String {
        let l: Vec<&str> = kanban_store::SUBAGENTS.iter().filter(|s| store.subagent_on(card, s.key) == on).map(|s| s.name).collect();
        if l.is_empty() { "none".into() } else { l.join(", ") }
    };
    let (on, off) = (names(true), names(false));
    // Card #162: which are the card's own, and which follow Settings.
    let own: Vec<&str> = kanban_store::SUBAGENTS.iter().filter(|s| card.subagent_choice(s.key).is_some()).map(|s| s.name).collect();
    let whose = match own.len() {
        0 => " All follow the defaults in Settings > Sessions.".to_string(),
        n if n == kanban_store::SUBAGENTS.len() => String::new(),
        _ => format!(" This card's own: {}; the rest follow the defaults in Settings > Sessions.", own.join(", ")),
    };
    let line = format!("Hugh set its subagents. On: {on}. Off: {off}.{whose}");
    let note = format!(
        "Hugh changed this card's subagents. On: {on}. Off: {off}.{whose} From now on, run only the ones that are on, where the column prompt says; skip a step for one that is off. The app's holds follow the same choice: a move isn't held for a subagent that is off. Only Hugh changes this."
    );
    (line, note)
}

/// Which of the card's flow options (card #162: skipPlanning, skipHumanTest, subagents) differ
/// between `before` and `after`, in that order.
pub fn flow_changed(before: Option<&Card>, after: &Card) -> Vec<&'static str> {
    kanban_store::FLOW_FIELDS.iter().copied().filter(|k| before.and_then(|b| b.fields.get(*k)) != after.fields.get(*k)).collect()
}

/// The log line for Hugh's change of a skip option (card #162), as the card now holds it.
pub fn skip_line(store: &Store, card: &Card, key: &str) -> String {
    let name = skip_name(key);
    let rule = if key == kanban_store::SKIP_PLANNING_KEY { kanban_store::SKIP_PLANNING_RULE_KEY } else { kanban_store::SKIP_HUMAN_TEST_RULE_KEY };
    match card.skip_choice(key) {
        Some(b) => format!("Hugh set {name}: {} for this card.", on_off(b)),
        None => format!("Hugh set {name} back to the default in Settings ({}).", on_off(store.skip_default(rule))),
    }
}

fn skip_name(key: &str) -> &'static str {
    if key == kanban_store::SKIP_PLANNING_KEY { "Skip Planning" } else { "Skip Human test" }
}

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

/// Every log line for Hugh's change of the card's flow (card #162): one for the subagents, one
/// for each skip option that changed.
pub fn flow_log_lines(store: &Store, card: &Card, changed: &[&str]) -> Vec<String> {
    changed
        .iter()
        .map(|k| if *k == kanban_store::SUBAGENTS_KEY { subagents_changed(store, card).0 } else { skip_line(store, card, k) })
        .collect()
}

/// What the card's session is told when Hugh changed whether it skips Planning or Human test
/// (card #162). None when neither changed.
pub fn skips_changed_note(store: &Store, card: &Card, changed: &[&str]) -> Option<String> {
    let mut lines = Vec::new();
    for key in [kanban_store::SKIP_PLANNING_KEY, kanban_store::SKIP_HUMAN_TEST_KEY] {
        if !changed.contains(&key) {
            continue;
        }
        let (on, whose) = store.skip_of(card, key);
        let state = format!("{} ({})", on_off(on), whose_choice(whose));
        let what = match (key == kanban_store::SKIP_PLANNING_KEY, on) {
            (true, true) => "A move into Planning from Backlog or Blocked goes straight to In progress, with no plan or plan critique. A move back into Planning from a later column still plans it.",
            (true, false) => "It is planned in Planning as usual.",
            (false, true) => "When you move it to Human test (after its subagent checks pass, with the hand-over as usual), the app moves it into Ship it for him and sends you the Ship it prompt: that is his approval to ship it with no human test, so make your own testing count. Take it to: Human test still stops it there for him.",
            (false, false) => "It stops in Human test for him as usual.",
        };
        lines.push(format!("- {}: {state}. {what}", skip_name(key)));
    }
    if lines.is_empty() {
        return None;
    }
    let child = if card.stack_of().is_some() { "\nThis card is in a stack, so it never skips either: its lead's flow plans and ships it.\n" } else { "\n" };
    Some(format!("Hugh changed this card's flow:\n{}{child}Only Hugh changes this.", lines.join("\n")))
}

/// After Hugh changed a flow default in Settings > Sessions (card #162): each card that follows
/// that default (no choice of its own) and is in the column where it matters is told, if it has a
/// session: a subagent in the column that runs it (as `subagents_note` words it), skip Human test
/// in In progress. A stack lead that follows a subagent's default may be free of a hold now, so
/// its stack's rules run again. Each card told gets a log line.
pub fn after_default_change(app: &AppHandle, key: &str) {
    let sub = kanban_store::SUBAGENTS.iter().find(|s| s.setting == key);
    let skip_ht = key == kanban_store::SKIP_HUMAN_TEST_RULE_KEY;
    if sub.is_none() && !skip_ht {
        return;
    }
    let board = app.state::<Board>();
    let Ok(mut store) = board.0.lock() else { return };
    let cards = store.list(false).unwrap_or_default();
    let mut leads = Vec::new();
    for c in &cards {
        let (note, line) = match sub {
            Some(s) => {
                if c.subagent_choice(s.key).is_some() {
                    continue;
                }
                if c.is_lead() {
                    leads.push(c.id.clone());
                }
                if c.column != s.column {
                    continue;
                }
                let on = store.rule_on(s.setting);
                let what = if on { "run it where the column prompt says; the app's holds wait for it again.".to_string() } else { s.skip.to_string() };
                (
                    format!("Hugh changed the default for {} in Settings > Sessions: it is {} now, and this card follows the default. {}", s.name, on_off(on), what),
                    format!("Told its session: Hugh turned {} {} by default in Settings", s.name, on_off(on)),
                )
            }
            None => {
                if c.skip_choice(kanban_store::SKIP_HUMAN_TEST_KEY).is_some() || c.stack_of().is_some() || c.auto_to().is_some() || c.column != "In progress" {
                    continue;
                }
                let on = store.skip_default(kanban_store::SKIP_HUMAN_TEST_RULE_KEY);
                let what = if on { SKIPS_HUMAN_TEST } else { "It stops in Human test for him as usual" };
                (
                    format!("Hugh changed the default for Skip Human test in Settings > Sessions: it is {} now, and this card follows the default. {what}.", on_off(on)),
                    format!("Told its session: Hugh turned Skip Human test {} by default in Settings", on_off(on)),
                )
            }
        };
        if has_session(app, &store, &c.id) {
            let _ = store.queue(&c.id, "message", &dry(c, note));
            let _ = store.add_log(&c.id, &line, "the app");
        }
    }
    let cause = format!("Hugh changed a default in Settings ({key})");
    for id in leads {
        after_lead_verdict(app, &mut store, &id, "Hugh (Settings)", &cause);
    }
    // Card #162: turning Skip Human test on by default moves on the cards that follow it and wait
    // in Human test where a session put them, as his change on one card does (after_edit).
    if skip_ht {
        for id in default_skip_waiting(&store, &cards) {
            auto_progress(app, &mut store, &id);
        }
    }
    drop(store);
    let _ = app.emit("cards-changed", ());
}

/// The cards that go on into Ship it now the Skip Human test default is on (card #162): those
/// that follow the default and wait in Human test where a session or the stack rules put them
/// (`skip_human_test_why`). None while the default is off.
pub fn default_skip_waiting(store: &Store, cards: &[Card]) -> Vec<String> {
    if !store.skip_default(kanban_store::SKIP_HUMAN_TEST_RULE_KEY) {
        return Vec::new();
    }
    cards
        .iter()
        .filter(|c| !c.archived && c.skip_choice(kanban_store::SKIP_HUMAN_TEST_KEY).is_none() && skip_human_test_why(store, c).is_some())
        .map(|c| c.id.clone())
        .collect()
}

/// Why a session may not move `card` to `to`, if it may not: a stack's lead goes to Human test
/// only when its children are ready (by the board's rule); the app moves it then.
pub fn stack_move_refused(store: &Store, card: &Card, to: &str) -> Option<String> {
    if to != "Human test" || !card.is_lead() {
        return None;
    }
    let rule = store.stack_rule();
    let kids = store.stack_children(card).ok()?;
    if kanban_store::stack_ready(card, &kids, &rule) {
        return None;
    }
    let waiting: Vec<String> = kids
        .iter()
        .filter(|k| !k.archived && !kanban_store::STACK_READY.contains(&k.column.as_str()))
        .map(|k| format!("#{} ({})", k.number, k.column))
        .collect();
    let need = if rule == "first" { "at least one card in it is" } else { "every card in it is" };
    Some(format!(
        "card #{} leads a stack, and a stack goes to Human test only when {need} ready (in Human test or later). Not ready yet: {}. Move each child to Human test when it is ready; the app then moves the stack itself.",
        card.number,
        if waiting.is_empty() { "no cards in the stack".to_string() } else { waiting.join(", ") }
    ))
}

/// Why a card's session may not move `card` to `to`, if it may not (card #109): a card not
/// started yet (in Backlog or Blocked) that waits on cards that aren't ready goes into Planning or
/// later only once they are, or by Hugh's own move. A card already started moves as before (its
/// session was told of the wait), and Backlog, Blocked and Done are always allowed.
pub fn wait_move_refused(store: &Store, card: &Card, to: &str) -> Option<String> {
    if !["Backlog", "Blocked"].contains(&card.column.as_str()) || ["Backlog", "Blocked", "Done"].contains(&to) {
        return None;
    }
    let waiting: Vec<String> = card
        .blocked_by()
        .iter()
        .filter_map(|n| match store.get_by_number(*n) {
            Ok(b) if kanban_store::is_met(card, &b) => None,
            Ok(b) => Some(format!("#{} ({})", b.number, b.column)),
            Err(_) => Some(format!("#{n} (not on the board)")),
        })
        .collect();
    if waiting.is_empty() {
        return None;
    }
    let then = if card.stack_of().is_some() {
        "Move it on once they are: the app doesn't start a stack's card for you".to_string()
    } else {
        format!("It goes into {to} once they are (the app then starts it or tells Hugh), or when Hugh moves it himself. Say so in nextStep and stop")
    };
    Some(format!("card #{} waits on {}, not ready yet. {then}.", card.number, waiting.join(", ")))
}

/// A move made by a session: of another card (at once), or its own (held, applied here). The
/// stack rules follow it.
pub fn session_move(app: &AppHandle, store: &mut Store, card: &Card, to: &str, reason: &str, by: &str) -> Result<(), String> {
    session_move_to(app, store, card, to, reason, by).map(|_| ())
}

/// `session_move`, returning the column the card went to: In progress instead of Planning for a
/// card that skips Planning (card #162), as the store's own moves do.
pub fn session_move_to(app: &AppHandle, store: &mut Store, card: &Card, to: &str, reason: &str, by: &str) -> Result<String, String> {
    let skipped = kanban_store::planning_skip(card, to, store.skip_default(kanban_store::SKIP_PLANNING_RULE_KEY));
    let to = if skipped.is_some() { "In progress" } else { to };
    if let Some(why) = stack_move_refused(store, card, to) {
        let _ = store.add_log(&card.id, &format!("Refused a move to {to} by {by}: {why}"), "the app");
        return Err(why);
    }
    let mut patch = Patch::new();
    patch.insert("column".into(), json!(to));
    if let Some(w) = skipped {
        let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        patch.insert("plan".into(), json!({ "state": kanban_store::PLAN_SKIPPED, "at": at, "whose": w.word(), "from": card.column }));
    }
    store.update(&card.id, card.version, &patch, by).map_err(|e| e.to_string())?;
    let why = if reason.is_empty() { String::new() } else { format!(": {reason}") };
    store.add_log(&card.id, &format!("Moved from {} to {to} by {by}{why}", card.column), by).map_err(|e| e.to_string())?;
    if let Some(w) = skipped {
        let _ = store.add_log(&card.id, &kanban_store::skip_planning_line(w), by);
    }
    after_stack_move(app, store, &card.id, by);
    auto_progress(app, store, &card.id);
    // A stack's child (card #113): in Human test it is ready to merge, so its lead is woken;
    // moved by anyone but its own session, its own session hears why.
    if card.stack_of().is_some() {
        if let Ok(moved) = store.get(&card.id) {
            crate::stackrun::child_moved(app, store, &moved, &card.column, reason, by);
        }
        if to == "Human test" {
            child_ready(app, store, &card.id);
        }
    }
    Ok(to.to_string())
}

/// Is `card` built, for a join: in Human test or later, or with a PR.
pub fn built(card: &Card) -> bool {
    kanban_store::STACK_READY.contains(&card.column.as_str()) || card.fields.get("pr").map_or(false, |v| !v.is_null() && v != "")
}

/// The message to a stack's session when card `c` joined it. `l` and `c` are the two cards as
/// they were before the join; `kids` are the cards that came with `c`, when it led a stack of
/// its own. A stack in Ship it ships the card in the same merge (card #89).
pub fn join_message(l: &Card, c: &Card, kids: &[Card], j: &kanban_store::Joined, by: &str) -> String {
    let (n, t) = (c.number, c.title());
    let field = |card: &Card, k: &str| card.fields.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let (lb, cb) = (field(l, "branch"), field(c, "branch"));
    let lb_text = lb.as_ref().map(|b| format!("this stack's branch {b}")).unwrap_or_else(|| "this stack's branch".into());
    // A stack's cards build themselves in parallel (card #113): the lead merges each one once it
    // reaches Human test, and never builds it.
    let planning = prompt_file("planning.md");
    let plan_it = |m: i64| {
        format!(
            "#{m} plans and builds itself in its own session, in its own worktree on its own branch made from {lb_text}, and goes through the critic like any card ({planning}, its stack section). Don't build it here: once it reaches Human test the app sends you a merge brief for it"
        )
    };
    let mut parts = vec![if kids.is_empty() {
        format!("#{n} \"{t}\" joined this stack ({by}).")
    } else {
        let list: Vec<String> = kids.iter().map(|k| format!("#{}", k.number)).collect();
        format!("Stack #{n} \"{t}\" joined this stack ({by}), with its cards {}: you integrate them all now; each keeps its own session and nextStep.", list.join(", "))
    }];
    if !built(c) {
        parts.push(format!("It isn't built yet. {}.", plan_it(n)));
        return parts.join(" ");
    }
    let same = matches!((&cb, &lb), (Some(c), Some(l)) if c == l);
    let in_ship = l.column == "Ship it";
    // A stack built on main itself: merging into its branch is merging to main, which only its ship does.
    let on_main = matches!(lb.as_deref(), Some("main" | "master"));
    parts.push(if same {
        format!("It is on {} already, so there is nothing to merge.", lb.as_deref().unwrap_or_default())
    } else {
        let on = match &cb {
            Some(b) if kids.is_empty() => format!("branch {b}"),
            Some(b) => format!("branch {b}, which holds its cards' work so far"),
            None => "its own branch (see the card)".into(),
        };
        let pr = field(c, "pr").map(|p| format!(", PR {p}")).unwrap_or_default();
        if in_ship && on_main {
            format!("It is already built, on {on}{pr}. This stack's branch is {}, so that branch goes in with this stack's ship: merge it as part of the ship (resolve any conflicts and run the checks again), then close its PR in favour of this stack's, saying so on it.", lb.as_deref().unwrap_or_default())
        } else {
            format!("It is already built, on {on}{pr}. Merge that branch into {lb_text} (resolve any conflicts and run the checks again), then close its PR in favour of this stack's PR, saying so on it.")
        }
    });
    // A stack that joined while some of its cards were still being built (the "first" rule).
    let unbuilt: Vec<&Card> = kids.iter().filter(|k| !k.archived && !built(k)).collect();
    for k in &unbuilt {
        parts.push(format!("#{} isn't built yet. {}.", k.number, plan_it(k.number)));
    }
    if in_ship {
        parts.push(if j.unshipped.contains(&n) {
            format!("#{n} was waiting in Ship it itself. Hugh dropped it on this stack, which is his approval to ship it with the rest: its own ship request is cancelled, it is marked Passed, and it waits in Human test until this stack ships.")
        } else {
            format!("This stack is in Ship it, and Hugh dropped #{n} on it: that is his approval to ship it with the rest, so it is marked Passed and waits in Human test until the stack ships.")
        });
        let early = !same && !on_main;
        parts.push(if early {
            "Merge it in before this stack's PR is merged, so it all ships as one merge: one push to main and one deploy.".to_string()
        } else {
            "It all ships as one merge: one push to main and one deploy.".to_string()
        });
        let queued = l.fields.get("ship").and_then(|s| s.get("state")).and_then(Value::as_str) == Some(kanban_store::SHIP_QUEUED);
        parts.push(match (queued, early) {
            (true, false) => "The stack is waiting its turn in the ship queue: don't merge anything to main or ship until the app sends you the Ship it prompt.".to_string(),
            (true, true) => "The stack is waiting its turn in the ship queue: do the merge into the stack's branch now, but don't merge to main or ship until the app sends you the Ship it prompt.".to_string(),
            (false, _) => "If you already have the Ship it prompt, ship it with the rest; otherwise wait for it: the app sends it when the stack's turn to ship comes.".to_string(),
        });
        parts.push("Just before you merge, check kanban_stack_list: every card in the stack must be on your branch.".to_string());
    } else {
        parts.push(if kids.is_empty() {
            format!("Keep #{n} in Human test with its own how-to-test in its nextStep, and update this stack's dev stack so Hugh tests it with the rest.")
        } else {
            format!("Keep #{n} and its built cards in Human test, each with its own how-to-test in its nextStep, and update this stack's dev stack so Hugh tests them with the rest.")
        });
        if j.unshipped.contains(&n) {
            parts.push(format!("#{n} was in Ship it, which is Hugh's approval to ship it: its own ship request is cancelled, it is marked Passed, and it ships with this stack when Hugh moves the stack to Ship it."));
        }
    }
    parts.join(" ")
}

/// The message to the own session of card `c` (as it was before it joined stack `lead`).
pub fn stand_down_message(c: &Card, lead: i64, j: &kanban_store::Joined) -> String {
    let mut t = if c.is_lead() {
        format!("This card's stack joined stack #{lead}: that stack's session owns this card and its cards now. Stop work on them here; log anything its new owner should know with kanban_add_log.")
    } else {
        format!("This card joined stack #{lead}: that stack's session owns it now. Stop work on it here; log anything its new owner should know with kanban_add_log.")
    };
    if j.unshipped.contains(&c.number) {
        t.push_str(&format!(" Its own ship is cancelled: it ships with stack #{lead}, so don't ship it from here."));
    }
    t
}

/// Did `card` leave Ship it to ship with its stack (card #89)? Hugh can drop a card he just
/// moved into Ship it on a stack before the app has handed that move on; then the stack's
/// session owns it, and `after_move` has nothing to start or tell.
pub fn left_ship_for_stack(card: &Card) -> bool {
    let Some(lead) = card.stack_of() else { return false };
    let ship = card.fields.get("ship");
    card.column != "Ship it"
        && ship.and_then(|s| s.get("state")).and_then(Value::as_str) == Some("cancelled")
        && ship.and_then(|s| s.get("with")).and_then(Value::as_i64) == Some(lead)
}

/// Does the card have a session to tell (live now, or one that resumes)?
pub fn has_session(app: &AppHandle, store: &Store, card_id: &str) -> bool {
    app.state::<Arc<Sessions>>().is_live(card_id).is_some() || store.latest_session(card_id).ok().flatten().is_some()
}

/// The message to a lead's session when the stack rules moved its stack.
pub fn stack_move_message(store: &Store, m: &kanban_store::StackMove, trigger: Option<&Card>) -> String {
    let because = match (m.to.as_str(), trigger) {
        ("Human test", _) if store.stack_rule() == "first" => "a card in it is ready to test".to_string(),
        ("Human test", _) => "every card in it is ready to test".to_string(),
        (_, Some(t)) => format!("#{} \"{}\" went back to {}", t.number, t.title(), t.column),
        _ => "a card in it is no longer ready".to_string(),
    };
    let next = if m.to == "Human test" {
        "Hugh tests the stack now: each card's nextStep is its how-to-test, and the stack's branch, PR, dev stack and logins are on this card. Keep the dev stack up. Never move a card into Ship it."
    } else {
        "Finish the card that went back, then move it to Human test; the app moves the stack again once it is ready."
    };
    format!("Stack #{}: the app moved this stack from {} to {} because {because}. {next}", m.number, m.from, m.to)
}

/// A stack lead's own subagent verdict just passed (card #96): its ready stack may have been
/// waiting for it, so the stack rules run again, and a move is told to the lead's session as a
/// child's move would be.
pub fn after_lead_verdict(app: &AppHandle, store: &mut Store, lead_id: &str, by: &str, cause: &str) {
    let Ok(lead) = store.get(lead_id) else { return };
    if !lead.is_lead() {
        return;
    }
    let moves = match store.stack_recheck(lead.number, cause, by) {
        Ok(m) => m,
        Err(e) => {
            let _ = store.add_log(lead_id, &format!("Couldn't apply the stack rules: {e}"), "the app");
            return;
        }
    };
    for m in moves.iter().filter(|m| m.lead) {
        if has_session(app, store, &m.id) {
            let text = stack_move_message(store, m, None);
            let _ = store.queue(&m.id, "message", &text);
        }
        if m.to == "Human test" {
            auto_progress(app, store, &m.id);
        }
    }
}

/// After `card_id` moved (by anyone): applies the stack rules. A lead the rules moved hears
/// why in its session; a child carried into a column that stops sessions (Done) has its
/// session stopped. Children moved by the rules never get `after_move` (no session per child).
pub fn after_stack_move(app: &AppHandle, store: &mut Store, card_id: &str, by: &str) -> Vec<kanban_store::StackMove> {
    if store.get(card_id).map_or(false, |c| c.column == "Human test") { crate::changes::on_entered_human_test(app, card_id); }
    let moves = match store.stack_after_move(card_id, by) {
        Ok(m) => m,
        Err(e) => {
            let _ = store.add_log(card_id, &format!("Couldn't apply the stack rules: {e}"), "the app");
            return vec![];
        }
    };
    // A stack set to go on by itself to Deploy complete ships once every card in it is ready,
    // which can be a later child's move after the stack is already in Human test (the "first"
    // rule): so each child's move looks at its lead again.
    let lead_of_moved = store.get(card_id).ok().and_then(|c| c.stack_of()).and_then(|n| store.get_by_number(n).ok()).map(|l| l.id);
    if moves.is_empty() {
        if let Some(l) = &lead_of_moved {
            auto_progress(app, store, l);
        }
        return moves;
    }
    let trigger = store.get(card_id).ok();
    let live = app.state::<Arc<Sessions>>().inner().clone();
    for m in &moves {
        if m.to == "Human test" { crate::changes::on_entered_human_test(app, &m.id); }
        if m.lead {
            if has_session(app, store, &m.id) {
                let text = stack_move_message(store, m, trigger.as_ref());
                let _ = store.queue(&m.id, "message", &text);
            }
            if m.to == "Human test" {
                auto_progress(app, store, &m.id);
            }
        } else {
            let suspend = store.column(&m.to).ok().flatten().map_or(false, |c| c.on_enter == "suspend");
            if suspend && live.is_live(&m.id).is_some() {
                let (a, id) = (app.clone(), m.id.clone());
                std::thread::spawn(move || {
                    let _ = a.state::<Arc<Sessions>>().stop(&a, &id);
                });
            }
        }
    }
    if let Some(l) = &lead_of_moved {
        auto_progress(app, store, l);
    }
    let _ = app.emit("cards-changed", ());
    moves
}

pub fn ship_result(app: &AppHandle, store: &mut Store, card: &Card, state: &str, detail: &str, by: &str) -> Result<Value, String> {
    // Only a card Hugh put into Ship it is being shipped; a result for any other is a mistake.
    if card.column != "Ship it" {
        return Err(format!("card #{} isn't in Ship it (it's in {}), so there is no ship to report on", card.number, card.column));
    }
    let mut ship = card.fields.get("ship").and_then(Value::as_object).cloned().unwrap_or_default();
    let retries = ship.get("retries").and_then(Value::as_i64).unwrap_or(0);
    ship.insert("detail".into(), json!(detail));
    let mut patch = Patch::new();
    let reply;
    match state {
        "shipping" | "shipped" | "needs-hugh" => {
            ship.insert("state".into(), json!(state));
            ship.insert("retries".into(), json!(retries));
            reply = format!("Recorded: {state}.");
        }
        "failed" => {
            let n = retries + 1;
            ship.insert("retries".into(), json!(n));
            if n < 3 {
                ship.insert("state".into(), json!("failed"));
                patch.insert("column".into(), json!("In progress"));
                let _ = store.queue(
                    &card.id,
                    "message",
                    &format!("The ship failed (try {n} of 3): {detail}. Fix that failure first, then hand the card back for testing."),
                );
                reply = format!("Recorded the failure (try {n} of 3). The card is back in In progress.");
            } else {
                ship.insert("state".into(), json!("needs-hugh"));
                reply = "That is the third failure: the card stays in Ship it and waits for Hugh.".into();
            }
        }
        other => return Err(format!("unknown ship state {other}")),
    }
    let patch_moved = patch.contains_key("column");
    let recorded = ship.get("state").cloned().unwrap_or(json!(state));
    let tries = ship.get("retries").cloned().unwrap_or(json!(0));
    patch.insert("ship".into(), Value::Object(ship));
    store.update(&card.id, card.version, &patch, by).map_err(|e| e.to_string())?;
    store.add_log(&card.id, &format!("Ship {state}{}", if detail.is_empty() { String::new() } else { format!(": {detail}") }), by).map_err(|e| e.to_string())?;
    if patch_moved {
        after_stack_move(app, store, &card.id, by);
    }
    let _ = app.emit("cards-changed", ());
    // For the notifications. `state` is what was recorded (a third failure is needs-hugh).
    let _ = app.emit("ship-result", json!({ "card": card.id, "number": card.number, "state": recorded, "detail": detail, "retries": tries }));
    Ok(json!(reply))
}

pub const RESTARTED: &str = "The kanban app restarted while you were working. Carry on from where you were.";

/// After the restart recovery resumed `card`'s conversation `prev` (at once, or later from the
/// queue): queues RESTARTED. A session that had no transcript yet started afresh instead, and
/// gets its card's header.
pub fn restarted(store: &mut Store, card: &Card, prev: Option<&str>, new: &kanban_store::SessionRow) {
    if prev == Some(new.claude_session_id.as_str()) {
        let _ = store.queue(&card.id, "message", RESTARTED);
        let _ = store.add_log(&card.id, "Resumed after the app restarted, because it was mid-turn", "the app");
    }
}

/// After the app starts: resumes each session that was mid-turn when it last stopped, and
/// queues RESTARTED, typed once the session is ready. Idle sessions wait for Hugh. Over the
/// cap, the rest wait in the queue (oldest first) and resume as places come free.
pub fn resume_after_restart(app: &AppHandle, rows: &[kanban_store::SessionRow]) {
    let live = app.state::<Arc<Sessions>>().inner().clone();
    for row in rows {
        let board = app.state::<Board>();
        let Ok(mut store) = board.0.lock() else { return };
        let Ok(card) = store.get(&row.card_id) else { continue };
        // A card archived or in Done since keeps its session stopped.
        let done = store.column(&card.column).ok().flatten().map_or(false, |c| c.on_enter == "suspend");
        if card.archived || done || live.is_live(&card.id).is_some() {
            continue;
        }
        match crate::begin_session(app, &mut store, &card, "resume", "restart") {
            Ok(crate::Begun::Started(new)) => restarted(&mut store, &card, Some(&row.claude_session_id), &new),
            Ok(crate::Begun::Queued(_)) | Ok(crate::Begun::Sizing) => {}
            Err(e) => {
                let _ = store.add_log(&card.id, &format!("Couldn't resume after the app restarted: {e}"), "the app");
            }
        }
        drop(store);
        let _ = app.emit("cards-changed", ());
    }
}

pub fn on_hook(app: &AppHandle, row_id: &str, event: &str, body: &Value) {
    // The Router's question, kept for its toast before the hook sets its state (card #119, F7).
    crate::routersession::on_hook(app, row_id, event, body);
    // One of the app's subagents starting: a snapshot of Claude Code's config (card #96).
    if event == "PreToolUse" {
        crate::configwatch::on_pre_tool(row_id, body);
        // And of the work, for a verdict that may let a small change go on (card #104).
        crate::agents::on_pre_tool(app, row_id, body);
    }
    // A subagent finished: a card-tester's or card-reviewer's verdict goes on the card (cards
    // #76 and #96), and a change to Claude Code's config while it ran is reported. Other uses
    // of the hook still see it below.
    if event == "SubagentStop" {
        crate::configwatch::report(app, row_id, body);
        crate::agents::on_subagent_stop(app, row_id, body);
    }
    // Index the transcript the hook names (after a /clear, a new file) now (card #110).
    crate::search::on_hook(app, row_id, event, body);
    let live = app.state::<Arc<Sessions>>();
    let testing = live.testing_hook(row_id, event, body);
    if let Some((card, state, prompt)) = live.hook(row_id, event, body) {
        record_state(app, row_id, &state);
        if event == "Stop" {
            crate::needs::turn_ended(app, &card, body.get("last_assistant_message").and_then(Value::as_str));
        }
        // A permission prompt: what it asks to run, read from the transcript off this thread.
        if let (Some((seq, message)), Some(path)) = (prompt, body.get("transcript_path").and_then(Value::as_str)) {
            crate::needs::read_prompt_call(app, row_id, seq, &message, path);
        }
        let _ = app.emit("activity", json!({ "card": card, "state": state }));
    }
    // After the state, so a review ending with its turn goes straight from Testing to nothing.
    if let Some((card, on)) = testing {
        let _ = app.emit("testing", json!({ "card": card, "on": on }));
    }
}

/// Keeps the session's state in its events (a row only on a change), so that after a crash
/// the next start knows whether it was mid-turn.
fn record_state(app: &AppHandle, row_id: &str, state: &str) {
    let board = app.state::<Board>();
    if let Ok(mut store) = board.0.lock() {
        let _ = store.session_state(row_id, state);
    };
}

/// Ends session `row_id`'s turn if its transcript says it ended with no hook.
fn turn_check(app: &AppHandle, live: &Sessions, row_id: &str, path: &std::path::Path) {
    let Some(at) = crate::transcript::turn_ended_in(path) else { return };
    if let Some((card, state)) = live.turn_ended(row_id, &at) {
        record_state(app, row_id, &state);
        let _ = app.emit("activity", json!({ "card": card.clone(), "state": state }));
        let _ = app.emit("session-changed", card);
    }
}

/// Takes session `row_id` off a permission prompt Hugh has answered: no hook says so for most
/// tools, but the transcript shows the call's result, or Claude going on (card #107). The file
/// is read holding no lock.
fn prompt_check(app: &AppHandle, live: &Sessions, row_id: &str) {
    let Some((seq, call)) = live.prompt_pending(row_id) else { return };
    if !crate::transcript::call_settled_in(&call.path, &call.id, call.sub) {
        return;
    }
    if let Some((card, state)) = live.prompt_settled(row_id, seq) {
        record_state(app, row_id, &state);
        let _ = app.emit("activity", json!({ "card": card.clone(), "state": state }));
        let _ = app.emit("needs-changed", card);
    }
}

/// The statusLine's JSON: keeps each account's newest 5-hour and 7-day usage.
pub fn on_status(app: &AppHandle, row_id: &str, body: &Value) {
    crate::note_status(app, row_id, body); // the card panel's session footer
    crate::steps::report(app, row_id, body); // the card's step progress (card #106), read off this thread
    let live = app.state::<Arc<Sessions>>();
    if let Some((card, state)) = live.status_seen(row_id) {
        record_state(app, row_id, &state);
        let _ = app.emit("activity", json!({ "card": card, "state": state }));
    }
    // A turn that ended with no hook (interrupted in the terminal): the statusLine still
    // reports, and the transcript records the end. The timer in `run` checks again later.
    if let Some(path) = body.get("transcript_path").and_then(Value::as_str) {
        live.note_transcript(row_id, path);
        if live.on_turn(row_id) {
            prompt_check(app, &live, row_id);
            turn_check(app, &live, row_id, std::path::Path::new(path));
        }
    }
    let Some(limits) = body.get("rate_limits") else { return };
    let board = app.state::<Board>();
    let Ok(mut store) = board.0.lock() else { return };
    let account = store.session_account(row_id).ok().flatten().unwrap_or_else(|| "main".into());
    let pct = |k: &str| limits.get(k).and_then(|w| w.get("used_percentage")).and_then(Value::as_f64);
    let v = json!({ "seven_day": pct("seven_day"), "five_hour": pct("five_hour"), "at": chrono::Utc::now().to_rfc3339() });
    let key = format!("usage.{account}");
    let before = store.setting(&key).ok().flatten().and_then(|s| serde_json::from_str::<Value>(&s).ok());
    let _ = store.set_setting(&key, &v.to_string());
    drop(store);
    // The board's header shows the 7-day figures; it is told only when one changes.
    if before.as_ref().and_then(|b| b.get("seven_day")) != v.get("seven_day") {
        let _ = app.emit("usage-changed", account);
    }
}

/// The account for a new session: the card's pin, else the lower 7-day usage, else the card's
/// recorded account, else main. Also returns the reason, for the log.
pub fn pick_account(store: &Store, card: &Card) -> (String, String) {
    if let Some(pin) = card.fields.get("accountPin").and_then(Value::as_str) {
        return (pin.to_string(), "the card is pinned to it".into());
    }
    let week = |a: &str| -> Option<f64> {
        let v: Value = serde_json::from_str(&store.setting(&format!("usage.{a}")).ok()??).ok()?;
        v.get("seven_day")?.as_f64()
    };
    // Account 2 isn't supported on a Mac in this version (card #117): never picked for usage.
    if let (true, Some(m), Some(a2)) = (crate::platform::SECOND_ACCOUNT, week("main"), week("account2")) {
        let pick = if a2 < m { "account2" } else { "main" };
        return (pick.into(), format!("7-day usage: main {m:.0}%, Account 2 {a2:.0}%"));
    }
    match card.fields.get("account").and_then(Value::as_str) {
        Some("account2") => ("account2".into(), "the card's recorded account".into()),
        _ => ("main".into(), "no usage reported yet for both accounts".into()),
    }
}

/// Hands the next waiting ship of each repo with nothing shipping to its session, as Hugh's
/// move into Ship it would have (card #79). A ship ends however it ends (shipped, failed,
/// needs Hugh, moved out by anyone, archived, or a request no session picked up going stale),
/// so this looks every couple of seconds rather than at each of those places. With nothing
/// queued it costs one small query.
fn pump_ships(app: &AppHandle) {
    let board = app.state::<Board>();
    let released: Vec<(String, String)> = {
        let Ok(mut store) = board.0.lock() else { return };
        if !store.ship_any_queued().unwrap_or(false) {
            return;
        }
        if store.ship_cancel_strays().unwrap_or(0) > 0 {
            let _ = app.emit("cards-changed", ());
        }
        let due = store.ship_due().unwrap_or_default();
        due.iter().filter_map(|c| store.ship_release(&c.id).ok().map(|(c, from)| (c.id, from))).collect()
    };
    for (id, from) in released {
        let a = app.clone();
        std::thread::spawn(move || after_move(&a, &id, &from));
    }
}

/// Acts on the cards whose waits are all met, or that a card they waited on went back from
/// (card #109, `Store::settle_waits`), every couple of seconds as `pump_ships` does. With no
/// card waiting it costs one query on a small index. The board is let go before anything is
/// passed on: a started card's after-move runs on its own thread, Hugh's notification and a
/// session's message after the poll's writes.
fn pump_waits(app: &AppHandle) {
    let board = app.state::<Board>();
    let done = {
        let Ok(mut store) = board.0.lock() else { return };
        store.settle_waits(kanban_store::is_test()).unwrap_or_default()
    };
    if done.is_empty() {
        return;
    }
    let _ = app.emit("cards-changed", ());
    for o in done {
        match o.kind {
            kanban_store::WaitKind::Released => {
                let (a, id) = (app.clone(), o.id.clone());
                std::thread::spawn(move || after_wait_release(&a, &id));
            }
            kanban_store::WaitKind::TellHugh => {
                let _ = app.emit("notify", json!({ "card": o.id, "number": o.number, "text": o.text }));
            }
            kanban_store::WaitKind::TellSession | kanban_store::WaitKind::WentBack => {
                let Ok(mut store) = board.0.lock() else { continue };
                if has_session(app, &store, &o.id) {
                    let text = match store.get(&o.id) {
                        Ok(c) => dry(&c, o.text.clone()),
                        Err(_) => o.text.clone(),
                    };
                    let _ = store.queue(&o.id, "message", &text);
                }
            }
        }
    }
}

/// Card #154: a card's waiting message that may be typed mid-turn, as its whole text. Only
/// text sent with `send_to_session` (Hugh's words, the Router's or another session's
/// `kanban_session_send`) gets here. The app's own text (a column prompt, a notice) is added to
/// the same waiting message by `Store::queue`, which changes the text, so a recorded text that
/// no longer matches the waiting one means "wait for the turn to end" as before. In memory only:
/// after an app restart a waiting message is typed at the prompt, as it always was.
static MID_TURN_OK: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

/// Whether a message added to `before` (the card's waiting text, if any) may go in mid-turn:
/// yes with nothing waiting, else only when what waits was itself all mid-turn text.
pub fn mid_turn_rule(recorded: Option<&str>, before: Option<&str>) -> bool {
    match before {
        None => true,
        Some(b) => recorded == Some(b),
    }
}

/// `mid_turn_rule` for card `card_id`.
pub fn mid_turn_ok_after(card_id: &str, before: Option<&str>) -> bool {
    let recorded = MID_TURN_OK.lock().ok().and_then(|m| m.iter().find(|(c, _)| c == card_id).map(|(_, t)| t.clone()));
    mid_turn_rule(recorded.as_deref(), before)
}

/// Records `payload` as the card's waiting text that may go in mid-turn.
pub fn mark_mid_turn_ok(card_id: &str, payload: &str) {
    if let Ok(mut m) = MID_TURN_OK.lock() {
        m.retain(|(c, _)| c != card_id);
        m.push((card_id.to_string(), payload.to_string()));
    }
}

/// Forgets the card's mid-turn text (it was typed, or no longer waits).
pub fn forget_mid_turn_ok(card_id: &str) {
    if let Ok(mut m) = MID_TURN_OK.lock() {
        m.retain(|(c, _)| c != card_id);
    }
}

/// Types each waiting mid-turn message (card #154) into its session as soon as the session can
/// take it, mid-turn included (`Sessions::type_now`), instead of waiting for the turn to end.
/// Before, a message that couldn't go in at once (the session had just been typed into, or
/// something already waited) sat until the turn ended, often many minutes. A held move still
/// goes first, and a session being restarted onto its route gets nothing (card #98).
fn type_mid_turn(app: &AppHandle, live: &Arc<Sessions>) {
    let due: Vec<(String, String)> = match MID_TURN_OK.lock() {
        Ok(m) if !m.is_empty() => m.clone(),
        _ => return,
    };
    let board = app.state::<Board>();
    for (card_id, text) in due {
        let Ok(mut store) = board.0.lock() else { return };
        let waiting = store.waiting(&card_id, "message").ok().flatten();
        let Some(m) = waiting.filter(|m| m.payload == text) else {
            // Typed at the prompt by `free`, or the app's text joined it: nothing to do here.
            forget_mid_turn_ok(&card_id);
            continue;
        };
        if store.waiting(&card_id, "move").ok().flatten().is_some() || crate::router::restarting(&card_id) {
            continue;
        }
        if live.type_now(&card_id, &m.payload, true).unwrap_or(false) {
            let _ = store.done(m.id, "sent");
            forget_mid_turn_ok(&card_id);
            drop(store);
            let _ = app.emit("session-changed", card_id);
        }
    }
}

/// Types queued text into free sessions and applies held moves, every 400 ms.
pub fn run(app: AppHandle) {
    // The stuck-session monitor (card #99) runs on its own thread: its file reads never hold up the typing.
    crate::monitor::start(app.clone());
    let mut last_pause_check = std::time::Instant::now();
    let mut last_ship_check = std::time::Instant::now();
    let mut last_check_size = std::time::Instant::now();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(400));
        let live = app.state::<Arc<Sessions>>().inner().clone();
        for (card_id, step) in live.nudge() {
            let after = crate::sessions::RETYPE_AFTER.as_secs();
            use crate::sessions::Retry;
            let line = match step {
                Retry::Wait => continue,
                Retry::GiveUp => format!("The session didn't take a message after {} tries; the chat shows it with Send again", crate::sessions::TYPE_TRIES),
                Retry::Enter => format!("Pressed Enter again for a message in the session's prompt: Claude hadn't taken it {after} s after it was typed"),
                Retry::Retype => format!("Typed a message into the session again: Claude hadn't taken it {after} s after it was typed"),
            };
            if let Ok(mut store) = app.state::<Board>().0.lock() {
                let _ = store.add_log(&card_id, &line, "the app");
            }
            let _ = app.emit("session-changed", card_id);
        }
        if last_ship_check.elapsed() > Duration::from_secs(2) {
            last_ship_check = std::time::Instant::now();
            pump_ships(&app);
            // The merge lanes (card #160): one cheap count when no merge waits.
            crate::lane::tick(&app);
            pump_waits(&app);
            // Test-app turns (card #115); the first run is the start-up check.
            crate::testapp::pump(&app);
        }
        // How long each Human test card takes to check (card #103): git runs on its own thread.
        if last_check_size.elapsed() > Duration::from_secs(3) {
            last_check_size = std::time::Instant::now();
            crate::check_size::pump(&app);
        }
        if last_pause_check.elapsed() > Duration::from_secs(20) {
            last_pause_check = std::time::Instant::now();
            pause_idle(&app, &live);
            // Cards whose 10-minute small-change wait is over go on (card #104).
            crate::goes_on::tick(&app);
            // A turn interrupted in the terminal whose end no statusLine report caught.
            for (row_id, path) in live.quiet_turns(Duration::from_secs(10)) {
                turn_check(&app, &live, &row_id, &path);
            }
        }
        crate::drain_queue(&app);
        type_mid_turn(&app, &live);
        let free = live.free();
        // The Router (card #119): its events, its crash, and what is due when it is free. It is
        // not a card, so the card steps below skip it.
        crate::routersession::tick(&app, free.iter().any(|k| k == crate::routersession::KEY));
        for card_id in free {
            // The Router and the merge lanes (card #160) aren't cards: their own ticks type to them.
            if crate::sessions::is_app_key(&card_id) {
                continue;
            }
            // Its session is being restarted onto its route (card #98): type nothing into it, or
            // the text (such as the In progress prompt) is lost with the old session. It waits in
            // the queue for the resumed one.
            if crate::router::restarting(&card_id) {
                continue;
            }
            let board = app.state::<Board>();
            let Ok(mut store) = board.0.lock() else { continue };
            if let Ok(Some(m)) = store.waiting(&card_id, "move") {
                let to: Value = serde_json::from_str(&m.payload).unwrap_or_default();
                let mut col = to.get("column").and_then(Value::as_str).unwrap_or("").to_string();
                let reason = to.get("reason").and_then(Value::as_str).unwrap_or("").to_string();
                // The app's own held move (card #98: a card routed small going on to In
                // progress) says so; any other is the session's.
                let app_by = to.get("by").and_then(Value::as_str).filter(|b| *b == crate::router::BY).map(str::to_string);
                let mut from = String::new();
                let res = store.get(&card_id).map_err(|e| e.to_string()).and_then(|c| {
                    from = c.column.clone();
                    let by = app_by.clone().unwrap_or_else(|| format!("card #{}'s session (held until its turn ended)", c.number));
                    // The app's small-card move is checked again now (card #98): Hugh may have
                    // dragged the card, cleared its route or turned the rule off since.
                    if app_by.is_some() {
                        if let Some(why) = crate::router::go_on_blocker(&store, &c, true) {
                            let _ = store.add_log(&c.id, &format!("Dropped the app's move to {col}: {why}"), crate::router::BY);
                            return Err(why.to_string());
                        }
                    }
                    // Checked again now: the card may have moved since the session asked.
                    if let Some(why) = crate::agents::move_refused(&store, &c, &col) {
                        let _ = store.add_log(&c.id, &format!("Refused a move to {col} by {by}: its subagent checks haven't passed"), "the app");
                        return Err(why);
                    }
                    if let Some(why) = wait_move_refused(&store, &c, &col) {
                        let _ = store.add_log(&c.id, &format!("Refused a move to {col} by {by}: it waits on cards that aren't ready"), "the app");
                        return Err(why);
                    }
                    // Small-change auto-ship (card #104) checks only a session's own held move
                    // into Human test, and needs to know if Take it to was set before the move.
                    let had_auto = c.auto_to().is_some();
                    let moved = session_move_to(&app, &mut store, &c, &col, &reason, &by);
                    if moved.as_deref() == Ok("Human test") {
                        crate::goes_on::consider(&app, &c.id, had_auto);
                    }
                    moved
                });
                // Card #162: a move into Planning on a card that skips it went to In progress.
                let res = res.map(|to| {
                    col = to;
                });
                let _ = store.done(m.id, if res.is_ok() { "applied" } else { "failed" });
                if let (Err(_), Some(_)) = (&res, &app_by) {
                    // The app's own move that no longer applies: nothing for the session to do.
                } else if let Err(e) = &res {
                    let whose = if app_by.is_some() { format!("The app's move of this card to {col} ({reason})") } else { format!("Your move of this card to {col}") };
                    let _ = store.queue(&card_id, "message", &format!("{whose} was refused: {e}"));
                }
                // A held move into a column that runs at once (Deploy complete, after a ship)
                // gives the session that column's prompt, as Hugh's move would.
                // Not when the app has already moved it on (auto-progress into Ship it): that
                // move sends its own message.
                if res.is_ok() && app_by.is_some() {
                    // The app moved it, so the session gets the column's prompt as for Hugh's move,
                    // and the route is marked so it goes on once.
                    crate::router::went_on(&mut store, &card_id);
                    if let Some(c) = store.get(&card_id).ok().filter(|c| c.column == col) {
                        let text = app_move_message(&store, &c, &from, crate::router::GO_ON_LEAD);
                        let _ = store.queue(&card_id, "message", &text);
                    }
                } else if res.is_ok() {
                    if let Some(text) = store.get(&card_id).ok().filter(|c| c.column == col).and_then(|c| held_move_message(&store, &c, &from)) {
                        let _ = store.queue(&card_id, "message", &text);
                    }
                }
                // A stack's child that moved itself into Human test waits to be merged, and a
                // waiting child holds no session slot (card #113): its session stops too.
                let suspend = store.column(&col).ok().flatten().map_or(false, |c| c.on_enter == "suspend")
                    || (res.is_ok() && crate::stackrun::stops_after_move(&store, &card_id, &col));
                if suspend && res.is_ok() && !store.column(&col).ok().flatten().map_or(false, |c| c.on_enter == "suspend") {
                    let _ = store.add_log(&card_id, "Stopped its session: it waits for its stack's lead to merge it. The conversation is kept; it resumes when there is something to do.", "the app");
                }
                drop(store);
                let _ = app.emit("cards-changed", ());
                if suspend {
                    let _ = live.stop(&app, &card_id);
                }
                continue;
            }
            // A routed card's session is restarted onto its route in Planning and In progress,
            // and back once it leaves them (card #98); its waiting message follows the resume.
            if crate::router::restart_if_due(&app, &live, &mut store, &card_id) {
                drop(store);
                continue;
            }
            // A model or effort switch goes before the next message, so the message runs on it.
            if let Some((field, value, row_id)) = live.apply_switch(&card_id) {
                let _ = store.session_event(&row_id, "running", Some(&format!("switched {field} to {value}")));
                let _ = store.add_log(&card_id, &format!("Typed /{field} {value} into the session"), "the app");
                drop(store);
                let _ = app.emit("session-changed", card_id.clone());
                continue;
            }
            if let Ok(Some(m)) = store.waiting(&card_id, "message") {
                if live.send_message(&card_id, &m.payload).is_ok() {
                    let _ = store.done(m.id, "sent");
                }
            }
        }
    });
}

/// Pauses sessions left idle longer than Hugh's setting (`sessions.idle_pause_minutes`), as
/// Kangentic does: the session is stopped and recorded as suspended, its conversation kept, and
/// it resumes when Hugh sends it a message (`session_send`) or starts it. A session with
/// something waiting to be typed, or whose card Hugh is looking at, isn't paused.
fn pause_idle(app: &AppHandle, live: &Arc<Sessions>) {
    let board = app.state::<Board>();
    let mins = match board.0.lock() {
        Ok(s) => s.idle_pause_minutes().unwrap_or(kanban_store::IDLE_PAUSE_DEFAULT),
        Err(_) => return,
    };
    if mins <= 0 {
        return;
    }
    for card_id in live.idle_for(Duration::from_secs(mins as u64 * 60)) {
        if crate::notify::looking_at(app, &card_id) {
            continue;
        }
        {
            let Ok(mut store) = board.0.lock() else { return };
            let waiting = store.waiting(&card_id, "message").ok().flatten().is_some() || store.waiting(&card_id, "move").ok().flatten().is_some();
            if waiting {
                continue;
            }
            let _ = store.add_log(&card_id, &format!("Paused after {mins} minute{} idle. The conversation is kept; it resumes when you send it a message.", if mins == 1 { "" } else { "s" }), "the app");
        }
        let (a, id, l) = (app.clone(), card_id.clone(), live.clone());
        std::thread::spawn(move || {
            let _ = l.stop(&a, &id);
            let _ = a.emit("cards-changed", ());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_message_goes_in_mid_turn_only_when_all_that_waits_may() {
        // Nothing waiting: it may go in mid-turn (Hugh's words, the Router's send).
        assert!(mid_turn_rule(None, None));
        // The Router's second message joins its first, which was recorded: still mid-turn.
        assert!(mid_turn_rule(Some("From the Router: one"), Some("From the Router: one")));
        // A column prompt or notice waits (not recorded): the whole message waits for the turn end.
        assert!(!mid_turn_rule(None, Some("You are the session for card #7")));
        // The app's text joined a recorded message, so the waiting text changed: it waits too.
        assert!(!mid_turn_rule(Some("From the Router: one"), Some("From the Router: one\n\nA notice")));
    }

    #[test]
    fn the_mid_turn_record_is_per_card_and_forgotten() {
        mark_mid_turn_ok("c154a", "hello");
        assert!(mid_turn_ok_after("c154a", Some("hello")));
        assert!(!mid_turn_ok_after("c154b", Some("hello")));
        mark_mid_turn_ok("c154a", "hello\n\nagain");
        assert!(!mid_turn_ok_after("c154a", Some("hello")));
        assert!(mid_turn_ok_after("c154a", Some("hello\n\nagain")));
        forget_mid_turn_ok("c154a");
        assert!(!mid_turn_ok_after("c154a", Some("hello\n\nagain")));
        assert!(mid_turn_ok_after("c154a", None));
    }

    fn card(fields: Value, column: &str) -> Card {
        Card {
            id: "x".into(),
            number: 7,
            column: column.into(),
            order: None,
            order_col: None,
            archived: false,
            fields: fields.as_object().cloned().unwrap(),
            version: 1,
        }
    }

    /// A test build's column prompts name its own checkout's prompts; a live build's name
    /// main's, as before.
    #[test]
    fn test_builds_fill_in_their_own_prompts() {
        // No private folder in tests: the build's own prompts.
        assert_eq!(prompts_dir(), crate::private::prompts_dir());
        assert!(prompts_dir().join("planning.md").is_file(), "{}", prompts_dir().display());
    }

    /// `{prompts}` (card #114): filled in as the prompts folder, with the system's separator.
    #[test]
    fn prompts_fill_in_the_folder() {
        let dir = std::path::Path::new("base").join("prompts");
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(prompts_in(r"follow {prompts}\planning.md.", &dir), format!("follow {}{sep}planning.md.", dir.display()));
        assert_eq!(prompts_in("the folder {prompts}", &dir), format!("the folder {}", dir.display()));
        assert_eq!(prompts_in("no folder here", &dir), "no folder here");
        assert!(!back_from_test().contains("{prompts}") && back_from_test().contains(&prompt_file("in-progress.md")));
        // fill fills it in, and a card's own text is never taken for it.
        let c = card(json!({ "title": "{prompts} in a title" }), "Planning");
        let p = fill(r"#{n} {title}: follow {prompts}\planning.md", &c, "", "");
        assert!(p.starts_with("#7 {prompts} in a title: follow ") && p.ends_with(&with_prompts(r"{prompts}\planning.md")), "{p}");
    }

    /// On Windows every prompt reads exactly as the full path did before card #114.
    #[cfg(windows)]
    #[test]
    fn prompts_read_as_before_on_windows() {
        let dir = std::path::Path::new(r"C:\Users\someone\kanban-app\prompts");
        for (new, old) in [
            (r"Plan it now: follow {prompts}\planning.md.", r"Plan it now: follow C:\Users\someone\kanban-app\prompts\planning.md."),
            (r"Build it now: follow {prompts}\in-progress.md.", r"Build it now: follow C:\Users\someone\kanban-app\prompts\in-progress.md."),
            (r"then follow {prompts}\kanban-app.md: it", r"then follow C:\Users\someone\kanban-app\prompts\kanban-app.md: it"),
        ] {
            assert_eq!(prompts_in(new, dir), old);
        }
        let back = prompts_in(BACK_FROM_TEST, dir);
        assert!(back.contains(r"build-and-verify loop (C:\Users\someone\kanban-app\prompts\in-progress.md), run a fresh card-reviewer and card-tester"), "{back}");
    }

    #[test]
    fn t3_7_edit_message_names_the_fields() {
        let c = card(json!({ "title": "T", "summary": "new s", "nextStep": "do x" }), "In progress");
        let m = edit_message(&c, &["summary", "nextStep", "order"]).unwrap();
        assert!(m.starts_with("Hugh edited summary, nextStep on this card:"));
        assert!(m.contains("- summary: new s"));
        assert!(edit_message(&c, &["order", "orderCol"]).is_none());
    }

    #[test]
    fn hughs_edit_of_the_checks_is_told_readably() {
        // Card #105: the checks reach the session as numbered lines with their state, not JSON.
        let checks = json!([{ "text": "the line shows", "state": "met", "by": "Hugh (board)", "at": "t" }, { "text": "a tick is kept", "state": "failed", "note": "lost on reload" }]);
        let c = card(json!({ "title": "T", "doneWhen": "it shows", "doneWhenChecks": checks }), "In progress");
        let m = edit_message(&c, &["doneWhen", "doneWhenChecks"]).unwrap();
        assert!(m.starts_with("Hugh edited doneWhen, doneWhenChecks on this card:"), "{m}");
        assert!(m.contains("- doneWhen: it shows"), "{m}");
        assert!(m.contains("- doneWhenChecks:\n  1. [met] the line shows\n  2. [failed] a tick is kept (lost on reload)"), "{m}");
        assert!(!m.contains('{'), "{m}");
        let cleared = card(json!({ "title": "T" }), "Planning");
        assert!(edit_message(&cleared, &["doneWhenChecks"]).unwrap().contains("- doneWhenChecks:(cleared)"));
    }

    #[test]
    fn a_check_edit_in_human_test_is_not_told() {
        // Card #105, critic finding 6: Hugh ticking a check while he tests mustn't wake the session.
        for col in ["Human test", "Ship it", "Deploy complete"] {
            let c = card(json!({ "title": "T", "doneWhenChecks": [{ "text": "a", "state": "met" }] }), col);
            assert!(edit_message(&c, &["doneWhenChecks"]).is_none(), "{col}");
            assert!(edit_message(&c, &["doneWhen"]).is_none(), "{col}");
            // Other fields are told there as before, without the checks.
            let m = edit_message(&c, &["nextStep", "doneWhenChecks"]).unwrap();
            assert!(m.starts_with("Hugh edited nextStep on this card:") && !m.contains("doneWhenChecks"), "{col}: {m}");
        }
        for col in ["Planning", "In progress", "Blocked", "Backlog"] {
            let c = card(json!({ "title": "T", "doneWhenChecks": [{ "text": "a", "state": "met" }] }), col);
            assert!(edit_message(&c, &["doneWhenChecks"]).is_some(), "{col}");
        }
    }

    #[test]
    fn edit_message_explains_blocked_by() {
        let c = card(json!({ "title": "T", "blockedBy": [12, 40] }), "In progress");
        let m = edit_message(&c, &["blockedBy"]).unwrap();
        assert!(m.contains("- blockedBy: #12, #40. This card waits on those cards."), "{m}");
        let c = card(json!({ "title": "T" }), "In progress");
        assert!(edit_message(&c, &["blockedBy"]).unwrap().contains("(cleared)"));
    }

    // Card #109: a card Hugh put in Blocked is moved to Planning once the cards it waited on are
    // ready. Its session hears why, and where each of those cards is.
    #[test]
    fn a_card_released_by_its_waits_is_told_the_branches() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let p = |v: Value| v.as_object().cloned().unwrap();
        let a = s.create(None, p(json!({ "title": "Alpha", "column": "In progress", "repo": "hughht5/kanban-app", "branch": "dsh-a" })), "t").unwrap();
        let c = s.create(None, p(json!({ "title": "C", "repo": "hughht5/kanban-app", "blockedBy": [a.number] })), "t").unwrap();
        let c = s.move_by_hugh(&c.id, c.version, "Blocked", Patch::new()).unwrap().card;
        // A card session can't take it on while it waits; Blocked and Backlog are fine.
        let refused = wait_move_refused(&s, &c, "In progress").unwrap();
        assert!(refused.contains(&format!("waits on #{} (In progress)", a.number)), "{refused}");
        assert!(wait_move_refused(&s, &c, "Backlog").is_none());
        // A card already started moves as before: its session was told of the wait.
        let run = s.create(None, p(json!({ "title": "R", "column": "In progress", "repo": "hughht5/kanban-app", "blockedBy": [a.number] })), "t").unwrap();
        assert!(wait_move_refused(&s, &run, "Human test").is_none());
        let mut patch = Patch::new();
        patch.insert("column".into(), json!("Human test"));
        s.update(&a.id, a.version, &patch, "t").unwrap();
        assert!(wait_move_refused(&s, &c, "In progress").is_none(), "ready now");
        let out = s.settle_waits(false).unwrap();
        assert_eq!(out[0].kind, kanban_store::WaitKind::Released);
        let c = s.get(&c.id).unwrap();
        let m = move_message(&s, &c, "Blocked", false);
        assert!(m.starts_with("The app moved this card from Blocked to Planning for Hugh: the cards it waited on are ready."), "{m}");
        assert!(m.contains("on branch dsh-a. It is not on main yet"), "{m}");
        // Hugh's own move of another card says nothing about waits.
        let plain = card(json!({ "title": "T", "plan": { "state": "requested" } }), "Planning");
        assert!(move_message(&s, &plain, "Backlog", false).starts_with("Hugh moved this card"));
    }

    #[test]
    fn a_planning_card_that_isnt_sized_is_told_to_run_no_card_router() {
        // Card #161: Hugh's "If it's off, it should be off".
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let plain = card(json!({ "title": "T", "subagents": { "router": true } }), "Planning");
        // On, and not being sized (its session already runs, say): not sized later.
        let n = subagents_note(&s, &plain, "Planning").unwrap();
        assert!(n.contains("isn't sized: the app didn't size it before this session started") && n.contains("Don't run the card-router") && n.contains("no route and no restart"), "{n}");
        assert!(!n.contains("Hugh has these subagents off"), "{n}");
        // Off: the setting's reason, in the move, held-move and app-move messages.
        s.set_page_setting(kanban_store::SIZE_FIRST_KEY, "off").unwrap();
        let n = subagents_note(&s, &plain, "Planning").unwrap();
        assert!(n.contains("\"Size cards before their session starts\" is off"), "{n}");
        assert!(move_message(&s, &plain, "Backlog", false).contains("Don't run the card-router"));
        assert!(app_move_message(&s, &plain, "Backlog", "x").contains("Don't run the card-router"));
        // Not for a routed card, a stack's child, another column, or one with its card-router off
        // (that has its own line, never both).
        assert!(subagents_note(&s, &card(json!({ "title": "T", "route": { "size": "small" } }), "Planning"), "Planning").is_none());
        assert!(subagents_note(&s, &card(json!({ "title": "T", "stackOf": 3 }), "Planning"), "Planning").is_none());
        assert!(subagents_note(&s, &card(json!({ "title": "T" }), "In progress"), "In progress").is_none());
        let off = subagents_note(&s, &card(json!({ "title": "T", "subagents": { "router": false } }), "Planning"), "Planning").unwrap();
        assert!(off.contains("- card-router (off for this card)") && !off.contains("isn't sized"), "{off}");
        // With its critic off too: both notes, the off list first.
        let both = subagents_note(&s, &card(json!({ "title": "T", "subagents": { "router": true, "critic": false } }), "Planning"), "Planning").unwrap();
        assert!(both.find("- card-critic").unwrap() < both.find("isn't sized").unwrap(), "{both}");
    }

    #[test]
    fn a_session_is_told_which_subagents_are_off_in_its_column() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        // All on: nothing to say, in any column.
        // Routed, so card #161's "not sized" note doesn't come into it (its own test below).
        let all = card(json!({ "title": "T", "route": { "size": "normal" }, "subagents": { "router": true, "critic": true, "reproducer": true, "reviewer": true, "tester": true, "merger": true } }), "Planning");
        for col in ["Planning", "In progress", "Ship it", "Human test"] {
            assert!(subagents_note(&s, &all, col).is_none(), "{col}");
        }
        let some = card(json!({ "title": "T", "route": { "size": "normal" }, "subagents": { "router": true, "critic": false, "reproducer": true, "reviewer": false, "tester": true, "merger": false } }), "Planning");
        let n = subagents_note(&s, &some, "Planning").unwrap();
        assert!(n.contains("- card-critic (off for this card): skip the planning prompt's card-critic step. Your move to In progress isn't held for it."), "{n}");
        assert!(!n.contains("card-router") && !n.contains("card-reviewer"), "{n}");
        let n = subagents_note(&s, &some, "In progress").unwrap();
        assert!(n.contains("- card-reviewer (off for this card)") && !n.contains("card-tester") && !n.contains("card-critic"), "{n}");
        // Card #162: an old card's `merger: false` says nothing at ship (its lane merges it).
        assert!(subagents_note(&s, &some, "Ship it").is_none());
        assert!(subagents_note(&s, &some, "Human test").is_none());
        // A card made before #151 follows the board: card #97's critic-off note, from the default.
        s.set_page_setting(kanban_store::CRITIC_RULE_KEY, "off").unwrap();
        let old = card(json!({ "title": "T", "plan": { "state": "requested" } }), "Planning");
        let n = subagents_note(&s, &old, "Planning").unwrap();
        assert!(n.contains("- card-critic (off by the board's default in Settings > Sessions)"), "{n}");
        // In the message for Hugh's move into Planning, and not for a card with the critic on.
        assert!(move_message(&s, &old, "Backlog", false).contains("card-critic (off by the board's default"));
        assert!(!move_message(&s, &card(json!({ "title": "T", "subagents": { "critic": true } }), "Planning"), "Backlog", false).contains("card-critic (off"));
        // Hugh's change: the log line and the session's note name all five (card #162: no
        // card-merger, whatever an old card holds).
        let (line, note) = subagents_changed(&s, &some);
        assert_eq!(line, "Hugh set its subagents. On: card-router, card-reproducer, card-tester. Off: card-critic, card-reviewer.");
        assert!(note.starts_with("Hugh changed this card's subagents. On: card-router, card-reproducer, card-tester. Off: card-critic, card-reviewer.") && note.contains("Only Hugh changes this."), "{note}");
        assert!(!kanban_store::looks_secret(&line) && !kanban_store::looks_secret(&note));
    }

    /// Card #162: when a card that skips Human test goes on into Ship it. A session's move (or the
    /// stack rules) into Human test does; Hugh's own drag doesn't; Take it to: Human test wins; a
    /// stack's child never does; the default reaches a card with no choice of its own.
    #[test]
    fn skip_human_test_goes_on_only_after_a_sessions_move() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let p = |v: Value| v.as_object().cloned().unwrap();
        let to_test = |s: &mut Store, c: &Card, by: &str| s.update(&c.id, c.version, &p(json!({ "column": "Human test" })), by).unwrap();
        let c = s.create(None, p(json!({ "title": "T", "column": "In progress", "skipHumanTest": true })), "Hugh (board)").unwrap();
        assert!(skip_human_test_why(&s, &c).is_none(), "not in Human test");
        let c = to_test(&mut s, &c, "card #1's session (held until its turn ended)");
        let why = skip_human_test_why(&s, &c).unwrap();
        assert_eq!(why, "he set it to skip Human test (this card's own), which is his approval to ship it with no human test");
        // Off on the card: no.
        let off = s.update(&c.id, c.version, &p(json!({ "skipHumanTest": false })), "Hugh (board)").unwrap();
        assert!(skip_human_test_why(&s, &off).is_none());
        // The default, for a card with no choice of its own.
        let d = s.create(None, p(json!({ "title": "D", "column": "In progress" })), "Hugh (board)").unwrap();
        let d = to_test(&mut s, &d, "card #2's session");
        assert!(skip_human_test_why(&s, &d).is_none(), "default off");
        s.set_page_setting(kanban_store::SKIP_HUMAN_TEST_RULE_KEY, "on").unwrap();
        assert!(skip_human_test_why(&s, &d).unwrap().contains("(the default in Settings)"));
        // Hugh's own drag into Human test never skips.
        let h = s.create(None, p(json!({ "title": "H", "column": "In progress" })), "Hugh (board)").unwrap();
        let h = s.move_by_hugh(&h.id, h.version, "Human test", Patch::new()).unwrap().card;
        assert!(skip_human_test_why(&s, &h).is_none());
        // Nor does his move from kb or his own session outside the app (an unheld caller).
        let k = s.create(None, p(json!({ "title": "K", "column": "In progress" })), "Hugh (board)").unwrap();
        let k = to_test(&mut s, &k, crate::tools::OUTSIDE_BY);
        assert!(skip_human_test_why(&s, &k).is_none(), "Hugh's kb move");
        for by in ["Hugh (panel)", "Hugh (settings)", "Hugh (board)", crate::tools::OUTSIDE_BY] {
            assert!(hughs_own(by), "{by}");
        }
        for by in ["card #3's session", "card #3's session (held until its turn ended)", kanban_store::ROUTER_BY, kanban_store::AUTO_BY, crate::tools::UNKNOWN_BY, "the app"] {
            assert!(!hughs_own(by), "{by}");
        }
        // A session the app doesn't know is no Hugh: it skips.
        let u = s.create(None, p(json!({ "title": "U", "column": "In progress" })), "Hugh (board)").unwrap();
        let u = to_test(&mut s, &u, crate::tools::UNKNOWN_BY);
        assert!(skip_human_test_why(&s, &u).is_some());
        // Take it to: Human test wins: it stops there, also once its autoTo is cleared.
        let a = s.create(None, p(json!({ "title": "A", "column": "In progress", "autoTo": "Human test" })), "Hugh (board)").unwrap();
        let a = to_test(&mut s, &a, "card #3's session");
        assert!(skip_human_test_why(&s, &a).is_none(), "autoTo set");
        let a = s.update(&a.id, a.version, &p(json!({ "autoTo": null })), kanban_store::AUTO_BY).unwrap();
        assert!(skip_human_test_why(&s, &a).is_none(), "autoTo cleared on arrival: it stopped for Hugh");
        // A stack's child never goes on by itself.
        let lead = s.create(None, p(json!({ "title": "L" })), "t").unwrap();
        let kid = s.create_in_stack(p(json!({ "title": "K", "column": "In progress" })), lead.number, "t").unwrap();
        let kid = s.update(&kid.id, kid.version, &p(json!({ "skipHumanTest": true })), "Hugh (board)").unwrap();
        let kid = to_test(&mut s, &kid, "card #9's session");
        assert!(skip_human_test_why(&s, &kid).is_none());
    }

    /// Card #162: what the sessions are told about the card's flow.
    #[test]
    fn the_flow_notes_tell_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let p = |v: Value| v.as_object().cloned().unwrap();
        // Hugh's drag of a card that skips Planning: the In progress message says so.
        let c = s.create(None, p(json!({ "title": "T", "repo": "a/b", "skipPlanning": true })), "Hugh (board)").unwrap();
        let c = s.move_by_hugh(&c.id, c.version, "Planning", Patch::new()).unwrap().card;
        let m = move_message(&s, &c, "Backlog", false);
        assert!(m.starts_with("Hugh moved this card from Backlog to In progress."), "{m}");
        assert!(m.contains(&format!("{SKIPPED_PLANNING} (Planning skipped by his choice for this card).")), "{m}");
        assert!(SKIPPED_PLANNING.contains("no plan and no plan critique") && SKIPPED_PLANNING.contains("make the card's own worktree") && SKIPPED_PLANNING.contains("summary"));
        assert!(!m.contains("Plan it now"), "no Planning prompt");
        // Not from Human test (only the move that skipped says it), and the record is gone then.
        let c2 = s.update(&c.id, c.version, &p(json!({ "column": "Human test" })), "card #1's session").unwrap();
        assert!(c2.fields.get("plan").is_none(), "the skipped record goes with the next move");
        let back = s.move_by_hugh(&c2.id, c2.version, "In progress", Patch::new()).unwrap().card;
        assert!(!move_message(&s, &back, "Human test", false).contains(SKIPPED_PLANNING));
        // The app's start for Hugh (Add card's Take it to Planning), on the default.
        s.set_page_setting(kanban_store::SKIP_PLANNING_RULE_KEY, "on").unwrap();
        let d = s.create(None, p(json!({ "title": "D" })), "Hugh (board)").unwrap();
        let d = s.move_for_hugh(&d.id, d.version, "Planning", "he added it to be planned now").unwrap().card;
        let m = move_message(&s, &d, "Backlog", true);
        assert!(m.contains("The app moved this card from Backlog to In progress for Hugh: he asked for it to start when he added it."), "{m}");
        assert!(m.contains("(Planning skipped by his default in Settings > Sessions)"), "{m}");
        // Skip Human test: told in In progress, and in Ship it once the app moved it there.
        let h = s.create(None, p(json!({ "title": "H", "column": "In progress", "skipHumanTest": true })), "Hugh (board)").unwrap();
        let n = flow_note(&s, &h, "Planning").unwrap();
        assert_eq!(n, format!("{SKIPS_HUMAN_TEST} (Human test skipped by his choice for this card)."));
        let h = s.update(&h.id, h.version, &p(json!({ "column": "Human test" })), "card #3's session").unwrap();
        let shipped = s.move_for_hugh(&h.id, h.version, "Ship it", "he set it to skip Human test").unwrap().card;
        let m = move_message(&s, &shipped, "Human test", false);
        assert!(m.contains("for Hugh: he set it to skip Human test, which is his approval to ship it with no human test."), "{m}");
        assert!(m.contains(SHIP_NO_HUMAN_TEST), "{m}");
        // autoTo wins: a card with Take it to hears that instead.
        let a = card(json!({ "title": "A", "skipHumanTest": true, "autoTo": "Human test" }), "In progress");
        assert!(flow_note(&s, &a, "Planning").is_none());
        // A stack's child hears nothing of it.
        let k = card(json!({ "title": "K", "skipHumanTest": true, "stackOf": 4 }), "In progress");
        assert!(flow_note(&s, &k, "Planning").is_none());
        for t in [SKIPPED_PLANNING, SKIPS_HUMAN_TEST, SHIP_NO_HUMAN_TEST] {
            assert!(!kanban_store::looks_secret(t));
        }
    }

    /// Card #162: Hugh's change of the card's flow, logged and told.
    #[test]
    fn hughs_flow_change_is_logged_and_told() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let p = |v: Value| v.as_object().cloned().unwrap();
        let before = s.create(None, p(json!({ "title": "T" })), "Hugh (board)").unwrap();
        let after = s.update(&before.id, before.version, &p(json!({ "skipPlanning": true, "skipHumanTest": false, "subagents": { "critic": false } })), "Hugh (board)").unwrap();
        let ch = flow_changed(Some(&before), &after);
        assert_eq!(ch, ["skipPlanning", "skipHumanTest", "subagents"]);
        let lines = flow_log_lines(&s, &after, &ch);
        assert_eq!(lines[0], "Hugh set Skip Planning: on for this card.");
        assert_eq!(lines[1], "Hugh set Skip Human test: off for this card.");
        assert!(lines[2].ends_with("This card's own: card-critic; the rest follow the defaults in Settings > Sessions."), "{}", lines[2]);
        let n = skips_changed_note(&s, &after, &ch).unwrap();
        assert!(n.starts_with("Hugh changed this card's flow:\n- Skip Planning: on (his choice for this card). A move into Planning from Backlog or Blocked goes straight to In progress"), "{n}");
        assert!(n.contains("- Skip Human test: off (his choice for this card). It stops in Human test for him as usual.") && n.ends_with("Only Hugh changes this."), "{n}");
        assert!(skips_changed_note(&s, &after, &["subagents"]).is_none());
        // Back to the default.
        let back = s.update(&after.id, after.version, &p(json!({ "skipPlanning": null })), "Hugh (board)").unwrap();
        assert_eq!(flow_changed(Some(&after), &back), ["skipPlanning"]);
        assert_eq!(skip_line(&s, &back, "skipPlanning"), "Hugh set Skip Planning back to the default in Settings (off).");
        assert!(skips_changed_note(&s, &back, &["skipPlanning"]).unwrap().contains("- Skip Planning: off (his default in Settings > Sessions). It is planned in Planning as usual."));
        assert!(flow_changed(Some(&back), &back).is_empty());
        let none = card(json!({ "title": "N" }), "In progress");
        assert!(subagents_changed(&s, &none).0.ends_with("All follow the defaults in Settings > Sessions."));
    }

    #[test]
    fn t3_13_dry_run_prefix_and_fill() {
        let c = card(json!({ "title": "T", "dryRun": true, "repo": "acme/web", "issue": { "number": 12 } }), "Planning");
        assert!(dry(&c, "x".into()).starts_with("Dry run: say what you would do and change nothing."));
        assert_eq!(fill("#{n} {title} ({repo}{issue})", &c, "", ""), "#7 T (acme/web, issue 12)");
        // {prompts} is the prompts folder: the bundled one here, with no private folder set.
        let p = fill(r"follow {prompts}\planning.md", &c, "", "");
        assert!(!p.contains("{prompts}") && p.ends_with(r"prompts\planning.md"), "{p}");
        assert!(!back_from_test().contains("{prompts}"));
    }

    #[test]
    fn t5_held_move_into_deploy_complete_gets_its_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("b.db")).unwrap();
        let c = card(json!({ "title": "T" }), "Deploy complete");
        let m = held_move_message(&s, &c, "Planning").unwrap();
        assert!(m.starts_with("Your move of this card to Deploy complete is done."));
        assert!(m.contains("deploy-complete.md") && m.contains("Hugh moves it to Done"));
        assert!(held_move_message(&s, &card(json!({ "title": "T" }), "Human test"), "In progress").is_none());
        let h = header(&card(json!({ "title": "T" }), "Planning"));
        assert_eq!(h, "You are the session for card #7, \"T\" (no repo). Read and update your card with the kanban_* tools (or `kb` in Bash). Never move a card into Ship it, never merge, never push to main.");
    }

    /// Card #162: turning the Skip Human test default on moves on the cards that follow it and
    /// that a session put in Human test, as a change on one card does; not Hugh's own, not a card
    /// with its own choice, not one with Take it to.
    #[test]
    fn the_skip_default_moves_on_the_cards_that_follow_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let p = |v: Value| v.as_object().cloned().unwrap();
        let put = |s: &mut Store, f: Value, by: &str| {
            let c = s.create(None, p(f), "Hugh (board)").unwrap();
            s.update(&c.id, c.version, &p(json!({ "column": "Human test" })), by).unwrap()
        };
        let follows = put(&mut s, json!({ "title": "F", "column": "In progress" }), "card #1's session");
        let _own_off = put(&mut s, json!({ "title": "O", "column": "In progress", "skipHumanTest": false }), "card #2's session");
        let _hughs = put(&mut s, json!({ "title": "H", "column": "In progress" }), "Hugh (board)");
        let _auto = put(&mut s, json!({ "title": "A", "column": "In progress", "autoTo": "Human test" }), "card #3's session");
        let cards = s.list(false).unwrap();
        assert!(default_skip_waiting(&s, &cards).is_empty(), "default off");
        s.set_page_setting(kanban_store::SKIP_HUMAN_TEST_RULE_KEY, "on").unwrap();
        assert_eq!(default_skip_waiting(&s, &cards), vec![follows.id.clone()]);
    }

    /// Card #162: skip Human test alone is no approval to build. A Planning session's own move
    /// into In progress (deferred) gets no prompt for it; a Planning it skipped does.
    #[test]
    fn skip_human_test_alone_does_not_start_the_build() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let p = |v: Value| v.as_object().cloned().unwrap();
        assert_eq!(s.column("In progress").unwrap().unwrap().mode, "deferred");
        let h = card(json!({ "title": "H", "skipHumanTest": true }), "In progress");
        assert!(flow_note(&s, &h, "Planning").is_some(), "it is told of the skip, later");
        assert!(held_move_message(&s, &h, "Planning").is_none(), "skip Human test isn't approval to build");
        s.set_page_setting(kanban_store::SKIP_HUMAN_TEST_RULE_KEY, "on").unwrap();
        let d = card(json!({ "title": "D" }), "In progress");
        assert!(held_move_message(&s, &d, "Planning").is_none(), "nor is the default");
        // Take it to still is, and so is a Planning that was skipped.
        let a = card(json!({ "title": "A", "autoTo": "Human test" }), "In progress");
        assert!(held_move_message(&s, &a, "Planning").unwrap().starts_with("Your move of this card to In progress is done."));
        let c = s.create(None, p(json!({ "title": "S", "skipPlanning": true })), "Hugh (board)").unwrap();
        let c = s.move_by_hugh(&c.id, c.version, "Planning", Patch::new()).unwrap().card;
        assert_eq!(c.column, "In progress");
        let m = held_move_message(&s, &c, "Backlog").unwrap();
        assert!(m.contains(SKIPPED_PLANNING), "{m}");
    }

    // Card #98: the app's own move of a card routed small gives the In progress prompt, and says
    // why the card moved.
    #[test]
    fn the_apps_small_card_move_gives_the_in_progress_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("b.db")).unwrap();
        let c = card(json!({ "title": "T" }), "In progress");
        let m = app_move_message(&s, &c, "Planning", crate::router::GO_ON_LEAD);
        assert!(m.starts_with("The app moved this card from Planning to In progress: it was routed small and its plan critique passed"), "{m}");
        assert!(m.contains("in-progress.md"), "{m}");
        let dry = card(json!({ "title": "T", "dryRun": true }), "In progress");
        assert!(app_move_message(&s, &dry, "Planning", "x").starts_with("Dry run:"));
    }

    // Card #144: the plan critic's notes reach the build, by each way into In progress.
    #[test]
    fn the_in_progress_message_ends_with_the_critics_build_notes() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("b.db")).unwrap();
        let noted = |extra: Value| {
            let mut f = json!({ "title": "T", "critique": { "state": "passed", "round": 0, "started": true, "notes": ["name the scroller"] } });
            f.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap());
            card(f, "In progress")
        };
        let has = |m: &str| m.trim_end().ends_with("sharpening doneWhenChecks now (an edit in In progress needs no new critique).") && m.contains("Build notes from the plan critic") && m.contains("- name the scroller");
        // Hugh's move (and the Router's, which uses the same message), from Planning or Backlog.
        for from in ["Planning", "Backlog"] {
            let m = move_message(&s, &noted(json!({})), from, false);
            assert!(m.contains("in-progress.md") && has(&m), "{from}: {m}");
        }
        // The app's small-card move.
        let m = app_move_message(&s, &noted(json!({})), "Planning", crate::router::GO_ON_LEAD);
        assert!(has(&m), "{m}");
        // A Take it to card's own move.
        let m = held_move_message(&s, &noted(json!({ "autoTo": "Human test" })), "Planning").unwrap();
        assert!(m.contains("Build notes from the plan critic"), "{m}");
        // Not back from Human test, nor from Blocked (the build already had them).
        for from in ["Human test", "Blocked"] {
            assert!(!move_message(&s, &noted(json!({})), from, false).contains("Build notes"), "{from}");
        }
        assert!(held_move_message(&s, &noted(json!({ "autoTo": "Human test" })), "Blocked").unwrap().find("Build notes").is_none());
        // Not into another column, and nothing when there are no notes.
        let planning = card(json!({ "title": "T", "critique": { "state": "passed", "notes": ["x"] } }), "Planning");
        assert!(!move_message(&s, &planning, "Backlog", false).contains("Build notes"));
        assert!(!move_message(&s, &card(json!({ "title": "T", "critique": { "state": "passed" } }), "In progress"), "Planning", false).contains("Build notes"));
        // A dry run still says so first.
        assert!(move_message(&s, &noted(json!({ "dryRun": true })), "Planning", false).starts_with("Dry run:"));
    }

    #[test]
    fn a_card_that_went_on_by_itself_is_told_why_and_what_to_push() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("b.db")).unwrap();
        let went = card(json!({ "title": "T", "repo": "a/b", "ship": { "state": "requested", "auto": true }, "smallChange": { "state": "went on", "head": "abc123", "branch": "main" } }), "Ship it");
        let m = move_message(&s, &went, "Human test", false);
        assert!(m.starts_with("The app moved this card from Human test to Ship it for Hugh: the change was small and fully checked, and his small-change auto-ship is on for this repo."), "{m}");
        assert!(!m.contains("when he added it"), "{m}");
        let n = auto_note(&went).unwrap();
        assert!(n.contains("This is his approval to ship it with no human test") && n.contains("git push origin abc123:main"), "{n}");
        // A card that only waited, or stopped, is told nothing of the kind.
        let waited = card(json!({ "title": "T", "smallChange": { "state": "stopped", "head": "abc123" } }), "Ship it");
        assert!(auto_note(&waited).is_none());
    }

    #[test]
    fn auto_progress_notes_tell_the_session_what_to_do() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("b.db")).unwrap();
        let plan = card(json!({ "title": "T", "autoTo": "Human test" }), "Planning");
        let m = move_message(&s, &plan, "Backlog", false);
        assert!(m.starts_with("Hugh moved this card from Backlog to Planning."), "{m}");
        assert!(m.contains("move this card to In progress yourself"), "{m}");
        // The app's own move into Planning says so.
        let started = card(json!({ "title": "T", "autoTo": "Deploy complete", "plan": { "state": "requested", "auto": true } }), "Planning");
        assert!(move_message(&s, &started, "Backlog", false).starts_with("The app moved this card from Backlog to Planning for Hugh: he set it to go on by itself to Deploy complete."));
        let asked = card(json!({ "title": "T", "plan": { "state": "requested", "auto": true } }), "Planning");
        assert!(move_message(&s, &asked, "Backlog", false).starts_with("The app moved this card from Backlog to Planning for Hugh: he asked for it to start in Planning when he added it."));
        // A held move into In progress gets the In progress prompt, only on a card that goes on.
        let ip = card(json!({ "title": "T", "autoTo": "Deploy complete" }), "In progress");
        let h = held_move_message(&s, &ip, "Planning").unwrap();
        assert!(h.starts_with("Your move of this card to In progress is done.") && h.contains("in-progress.md") && h.contains("the app moves it into Ship it for him"), "{h}");
        assert!(held_move_message(&s, &card(json!({ "title": "T" }), "In progress"), "Planning").is_none());
        // A stack's child never goes on by itself.
        assert!(auto_note(&card(json!({ "title": "T", "autoTo": "Human test", "stackOf": 3 }), "Planning")).is_none());
        // The app's move into Ship it is Hugh's approval, and says so.
        let ship = card(json!({ "title": "T", "autoTo": "Deploy complete", "ship": { "state": "requested", "auto": true } }), "Ship it");
        let m = move_message(&s, &ship, "Human test", false);
        assert!(m.starts_with("The app moved this card from Human test to Ship it for Hugh: he set it to go on by itself to Deploy complete."), "{m}");
        assert!(m.contains("ship-it.md") && m.contains("approval to ship it with no human test"), "{m}");
        // Hugh's own move into Ship it reads as before.
        let hugh = card(json!({ "title": "T", "ship": { "state": "requested" } }), "Ship it");
        assert!(move_message(&s, &hugh, "Human test", false).starts_with("Hugh moved this card from Human test to Ship it."));
        // Hugh's edit of autoTo is told in words.
        let e = edit_message(&ip, &["autoTo"]).unwrap();
        assert!(e.contains("- autoTo: Deploy complete. The card now goes on by itself"), "{e}");
        let e = edit_message(&card(json!({ "title": "T" }), "Planning"), &["autoTo"]).unwrap();
        assert!(e.contains("no longer goes on by itself"), "{e}");
    }

    // Card #102: Hugh's Fail in Human test. Refused unless the card is in Human test, not
    // archived, not a stack's child, and the note says something.
    #[test]
    fn fail_in_human_test_refuses_what_it_cannot_fail() {
        let t = card(json!({ "title": "T" }), "Human test");
        for note in ["", "   \n"] {
            assert!(fail_plan(&t, note).unwrap_err().contains("say what's wrong"));
        }
        let e = fail_plan(&card(json!({ "title": "T" }), "In progress"), "x").unwrap_err();
        assert!(e.contains("isn't in Human test") && e.contains("In progress now"), "{e}");
        let e = fail_plan(&Card { archived: true, ..card(json!({ "title": "T" }), "Human test") }, "x").unwrap_err();
        assert!(e.contains("archived"), "{e}");
        let e = fail_plan(&card(json!({ "title": "T", "stackOf": 3 }), "Human test"), "x").unwrap_err();
        assert!(e.contains("stack #3") && e.contains("Found a problem"), "{e}");
    }

    #[test]
    fn fail_in_human_test_moves_an_ordinary_card_back_and_a_lead_by_the_stack_steps() {
        let t = card(json!({ "title": "T" }), "Human test");
        assert_eq!(fail_plan(&t, "grey button"), Ok(FailPlan::MoveBack));
        // The rule agrees with card #93's: an ordinary card is one a message moves back.
        assert!(message_moves_back(&t));
        let lead = card(json!({ "title": "Stack", "stack": [8, 9] }), "Human test");
        assert_eq!(fail_plan(&lead, "grey button"), Ok(FailPlan::LeadBack));
        assert_eq!(failed_log("  grey button \n"), "Failed in Human test: grey button");
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("b.db")).unwrap();
        let moved = card(json!({ "title": "T" }), "In progress");
        let m = failed_message(&s, &moved, " grey button ");
        assert!(m.starts_with("Hugh failed this card in Human test, which moved it back to In progress."), "{m}");
        assert!(!m.contains("Hugh's message below"), "{m}");
        assert!(m.contains("card-tester") && m.contains("in-progress.md") && m.ends_with("Hugh's note:\n\ngrey button"), "{m}");
        // Card #151: a card with its tester off is told to skip it, before Hugh's note.
        let off = card(json!({ "title": "T", "subagents": { "tester": false } }), "In progress");
        let m = failed_message(&s, &off, "grey button");
        assert!(m.contains("- card-tester (off for this card)") && m.ends_with("Hugh's note:\n\ngrey button"), "{m}");
        let l = lead_failed_message(&s, &Card { column: "In progress".into(), ..lead.clone() }, "grey button");
        assert!(l.starts_with("Hugh tested stack #7 \"Stack\" in Human test and failed it: grey button"), "{l}");
        assert!(l.contains("back in In progress") && l.contains("in-progress.md") && l.contains("card-tester"), "{l}");
        let dl = card(json!({ "title": "S", "stack": [8], "dryRun": true }), "In progress");
        assert!(lead_failed_message(&s, &dl, "x").starts_with("Dry run:"));
        assert!(lead_failed_message(&s, &card(json!({ "title": "S", "stack": [8], "subagents": { "reviewer": false } }), "In progress"), "x").contains("- card-reviewer (off for this card)"));
        assert!(failed_message(&s, &card(json!({ "title": "T", "dryRun": true }), "In progress"), "x").starts_with("Dry run:"));
    }

    // Card #93: a card back from Human test is told why, by Hugh's message or his move.
    #[test]
    fn a_message_to_a_card_in_human_test_takes_it_back_to_work() {
        let t = card(json!({ "title": "T" }), "Human test");
        assert!(message_moves_back(&t));
        for c in [
            card(json!({ "title": "T" }), "In progress"),
            card(json!({ "title": "T" }), "Ship it"),
            card(json!({ "title": "T", "stackOf": 3 }), "Human test"),
            card(json!({ "title": "T", "stack": [8, 9] }), "Human test"),
            Card { archived: true, ..card(json!({ "title": "T" }), "Human test") },
        ] {
            assert!(!message_moves_back(&c), "{} {:?}", c.column, c.fields);
        }
        let moved = card(json!({ "title": "T" }), "In progress");
        let sd = tempfile::tempdir().unwrap();
        let ss = Store::open(sd.path().join("b.db")).unwrap();
        let m = moved_back_message(&ss, &moved, "The button is still grey");
        assert!(!m.contains("subagents off"), "{m}");
        let off = card(json!({ "title": "T", "subagents": { "reviewer": false } }), "In progress");
        let mo = moved_back_message(&ss, &off, "x");
        assert!(mo.contains("- card-reviewer (off for this card)") && mo.ends_with("Hugh's message:\n\nx"), "{mo}");
        assert!(m.starts_with("Hugh's message below moved this card from Human test back to In progress"), "{m}");
        assert!(m.contains("card-tester") && m.contains("in-progress.md") && m.ends_with("Hugh's message:\n\nThe button is still grey"), "{m}");
        // A message he didn't mark as a bug may still be proved; else the session says why not (card #112).
        assert!(m.contains("card-reproducer") && m.contains("Not a bug report: <why>"), "{m}");
        let dry_card = card(json!({ "title": "T", "dryRun": true }), "In progress");
        assert!(moved_back_message(&ss, &dry_card, "x").starts_with("Dry run:"));
        // Hugh's own move back from Human test says the same; a move from elsewhere doesn't.
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("b.db")).unwrap();
        assert!(move_message(&s, &moved, "Human test", false).contains("It came back from Human test"));
        assert!(!move_message(&s, &moved, "Planning", false).contains("It came back from Human test"));
    }

    /// Card #146: a card in the worktree the app made for it is told where it builds, in its
    /// first message and in each move's message, once. Other cards hear nothing new.
    /// Cards #160, #162: Ship it's message for every card with a branch, on any repo, says to
    /// call kanban_merge, and never to run the card-merger; a card with no branch hears nothing
    /// of a merge. An old card's `merger: false` changes nothing.
    #[test]
    fn every_ship_with_a_branch_is_told_to_use_its_merge_lane() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("board.db")).unwrap();
        let mk = |s: &mut Store, f: Value| {
            let mut p = f.as_object().cloned().unwrap();
            p.insert("column".into(), json!("Ship it"));
            s.create(None, p, "Hugh").unwrap()
        };
        let none = mk(&mut s, json!({ "title": "T", "repo": "acme/web" }));
        assert!(!move_message(&s, &none, "Human test", false).contains("merge lane"), "no branch: nothing to merge");
        for f in [json!({ "title": "T", "repo": "acme/web", "branch": "card-2" }), json!({ "title": "T", "repo": "hughht5/kanban-app", "branch": "card-3", "subagents": { "merger": false } })] {
            let c = mk(&mut s, f);
            for m in [move_message(&s, &c, "Human test", false), app_move_message(&s, &c, "Human test", "x")] {
                assert!(m.contains("ships through its repo's merge lane") && m.contains("kanban_merge"), "{m}");
                assert!(!m.contains("card-merger"), "no ship path asks for the card-merger:\n{m}");
            }
        }
    }

    #[test]
    fn a_card_in_its_own_worktree_is_told_where_it_builds() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let co = dir.path().join("repo");
        std::fs::create_dir_all(co.join(".git")).unwrap();
        let list = json!([{ "repo": "a/b", "checkout": co.display().to_string() }]).to_string();
        let p = |v: Value| v.as_object().cloned().unwrap();
        let c = s.create(None, p(json!({ "title": "T", "repo": "a/b", "column": "Planning" })), "t").unwrap();
        // Before Hugh lists the repo, and before the card has its worktree: nothing.
        assert!(worktree_note(&s, &c).is_none());
        s.set_page_setting(kanban_store::OWN_WORKTREE_REPOS_KEY, &list).unwrap();
        assert!(worktree_note(&s, &c).is_none(), "no worktreePath yet");
        let wt = co.join(".claude").join("worktrees").join(crate::worktree::name(c.number, kanban_store::channel()));
        let mut patch = Patch::new();
        patch.insert("worktreePath".into(), json!(wt.display().to_string()));
        let c = s.update(&c.id, c.version, &patch, "the app").unwrap();
        let note = worktree_note(&s, &c).unwrap();
        assert!(note.contains(&wt.display().to_string()) && note.contains("never on main"), "{note}");
        assert!(opening(&s, &c).ends_with(&note));
        // A move's message ends with it, after the column prompt it overrides.
        let m = move_message(&s, &c, "Planning", false);
        assert!(m.ends_with(&note) && m.matches(&note).count() == 1, "{m}");
        // A new session's first message is waiting with it: the move's message doesn't repeat it.
        s.queue(&c.id, "message", &opening(&s, &c)).unwrap();
        assert_eq!(move_message(&s, &c, "Planning", false).matches(&note).count(), 0);
        assert_eq!(app_move_message(&s, &c, "Planning", "why").matches(&note).count(), 0);
        // The main checkout (the old way of building), or another repo: no note.
        let old = card(json!({ "title": "T", "repo": "a/b", "worktreePath": co.display().to_string() }), "In progress");
        assert!(worktree_note(&s, &old).is_none());
        assert!(!move_message(&s, &old, "Planning", false).contains("its own git worktree"));
        let other = card(json!({ "title": "T", "repo": "c/d", "worktreePath": wt.display().to_string() }), "In progress");
        assert!(worktree_note(&s, &other).is_none());
    }

    #[test]
    fn template_starting_prompt_opens_a_new_session() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("b.db")).unwrap();
        let c = card(json!({ "title": "Bug: login fails", "templateId": "bug" }), "Planning");
        let start = template_prompt(&s, &c).unwrap();
        assert!(start.starts_with("This card was made from the \"Bug\" template. This card is a bug."), "{start}");
        assert_eq!(opening(&s, &c), format!("{}\n\n{start}", header(&c)));
        // No template, or one that isn't on the board: just the header.
        let plain = card(json!({ "title": "T" }), "Planning");
        assert!(template_prompt(&s, &plain).is_none());
        assert_eq!(opening(&s, &plain), header(&plain));
        assert!(template_prompt(&s, &card(json!({ "title": "T", "templateId": "gone" }), "Planning")).is_none());
        // The move's message, which follows it, is unchanged.
        assert!(!move_message(&s, &c, "Backlog", true).contains("template"));
    }

    #[test]
    fn t3_11_account_choice() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let c = card(json!({ "title": "T" }), "Backlog");
        assert_eq!(pick_account(&s, &c).0, "main");
        s.set_setting("usage.main", r#"{"seven_day": 80}"#).unwrap();
        s.set_setting("usage.account2", r#"{"seven_day": 20}"#).unwrap();
        let (a, why) = pick_account(&s, &c);
        if crate::platform::SECOND_ACCOUNT {
            assert_eq!(a, "account2");
            assert!(why.contains("main 80%") && why.contains("Account 2 20%"));
        } else {
            // On a Mac account 2 is never picked for usage (card #117).
            assert_eq!(a, "main", "{why}");
        }
        let pinned = card(json!({ "title": "T", "accountPin": "main" }), "Backlog");
        assert_eq!(pick_account(&s, &pinned).0, "main");
    }

    fn numbered(n: i64, fields: Value, column: &str) -> Card {
        let mut c = card(fields, column);
        c.number = n;
        c.id = format!("card-{n}");
        c
    }

    #[test]
    fn join_message_for_a_stack_waiting_in_ship_it() {
        let l = numbered(53, json!({ "title": "Lead", "branch": "lead-53", "ship": { "state": "queued" } }), "Ship it");
        let c = numbered(81, json!({ "title": "Menu", "branch": "canvas-menu-81", "pr": "https://github.com/x/y/pull/1860", "ship": { "state": "queued" } }), "Ship it");
        let j = kanban_store::Joined { unshipped: vec![81], came_with: vec![] };
        let m = join_message(&l, &c, &[], &j, "Hugh (board)");
        assert!(m.starts_with("#81 \"Menu\" joined this stack (Hugh (board))."), "{m}");
        assert!(m.contains("#81 was waiting in Ship it itself") && m.contains("its own ship request is cancelled, it is marked Passed"), "{m}");
        assert_eq!(m.matches("approval").count(), 1, "said once: {m}");
        assert!(m.contains("on branch canvas-menu-81, PR https://github.com/x/y/pull/1860. Merge that branch into this stack's branch lead-53"), "{m}");
        assert!(m.contains("one merge: one push to main and one deploy"), "{m}");
        assert!(m.contains("don't merge to main or ship until the app sends you the Ship it prompt"), "{m}");
        assert!(m.contains("check kanban_stack_list"), "{m}");
        assert!(!m.contains("dev stack"), "nobody tests a stack in Ship it again: {m}");
        // Asked for and about to be handed: the prompt is on its way, or there.
        let asked = numbered(53, json!({ "title": "Lead", "branch": "lead-53", "ship": { "state": "requested" } }), "Ship it");
        let m = join_message(&asked, &numbered(81, json!({ "title": "Menu", "branch": "b" }), "Human test"), &[], &kanban_store::Joined::default(), "Hugh (board)");
        assert!(m.contains("If you already have the Ship it prompt, ship it with the rest; otherwise wait for it"), "{m}");
        assert!(!m.contains("ship request is cancelled"), "{m}");
    }

    #[test]
    fn join_message_outside_ship_it_and_on_the_same_branch() {
        let l = numbered(10, json!({ "title": "Lead", "branch": "main" }), "Human test");
        let c = numbered(11, json!({ "title": "App card", "branch": "main" }), "Ship it");
        let j = kanban_store::Joined { unshipped: vec![11], came_with: vec![] };
        let m = join_message(&l, &c, &[], &j, "Hugh (board)");
        assert!(m.contains("It is on main already, so there is nothing to merge."), "{m}");
        assert!(m.contains("update this stack's dev stack") && m.contains("ships with this stack when Hugh moves the stack to Ship it"), "{m}");
        // In Ship it, on the same branch: no merge to do, only the wait.
        let waiting = numbered(10, json!({ "title": "Lead", "branch": "main", "ship": { "state": "queued" } }), "Ship it");
        let m = join_message(&waiting, &numbered(11, json!({ "title": "App card", "branch": "main" }), "Human test"), &[], &kanban_store::Joined::default(), "Hugh (board)");
        assert!(m.contains("nothing to merge") && m.contains("It all ships as one merge") && !m.contains("do the merge"), "{m}");
        // A stack on main, a card on its own branch: the merge goes in with the ship, never before it.
        let m = join_message(&waiting, &numbered(12, json!({ "title": "Branch card", "branch": "fix-12" }), "Human test"), &[], &kanban_store::Joined::default(), "Hugh (board)");
        assert!(m.contains("This stack's branch is main, so that branch goes in with this stack's ship"), "{m}");
        assert!(m.contains("don't merge anything to main or ship until the app sends you the Ship it prompt") && !m.contains("do the merge into the stack's branch now"), "{m}");
        // Not built yet: plan and build it, as before.
        let m = join_message(&l, &numbered(12, json!({ "title": "New" }), "Backlog"), &[], &kanban_store::Joined::default(), "card #5's session");
        assert!(m.contains("It isn't built yet") && m.contains("#12 plans and builds itself") && m.contains("made from this stack's branch main") && m.contains("merge brief"), "{m}");
    }

    #[test]
    fn join_message_for_a_whole_stack() {
        let l = numbered(1, json!({ "title": "Lead", "branch": "one" }), "Human test");
        let c = numbered(2, json!({ "title": "Other stack", "branch": "two", "stack": [3, 4] }), "Human test");
        let kids = [numbered(3, json!({ "title": "a" }), "Human test"), numbered(4, json!({ "title": "b" }), "Human test")];
        let j = kanban_store::Joined { unshipped: vec![], came_with: vec![3, 4] };
        let m = join_message(&l, &c, &kids, &j, "Hugh (board)");
        assert!(m.starts_with("Stack #2 \"Other stack\" joined this stack (Hugh (board)), with its cards #3, #4: you integrate them all now"), "{m}");
        assert!(m.contains("on branch two, which holds its cards' work so far"), "{m}");
        assert!(!m.contains("isn't built yet"), "{m}");
        // Under the "first" rule a stack in Human test can still have a card being built.
        let kids = [numbered(3, json!({ "title": "a" }), "Human test"), numbered(4, json!({ "title": "b" }), "In progress")];
        let m = join_message(&l, &c, &kids, &j, "Hugh (board)");
        assert!(m.contains("#4 isn't built yet. #4 plans and builds itself") && !m.contains("#3 isn't built"), "{m}");
        let s = stand_down_message(&c, 1, &j);
        assert!(s.starts_with("This card's stack joined stack #1: that stack's session owns this card and its cards now."), "{s}");
    }

    #[test]
    fn stand_down_says_a_cancelled_ship_is_not_to_be_shipped() {
        let c = numbered(81, json!({ "title": "Menu" }), "Ship it");
        let j = kanban_store::Joined { unshipped: vec![81], came_with: vec![] };
        let s = stand_down_message(&c, 53, &j);
        assert!(s.starts_with("This card joined stack #53") && s.ends_with("so don't ship it from here."), "{s}");
        assert!(!stand_down_message(&c, 53, &kanban_store::Joined::default()).contains("ship"), "only a cancelled ship is mentioned");
    }

    #[test]
    fn after_move_leaves_a_card_that_left_ship_it_for_its_stack() {
        let joined = numbered(81, json!({ "title": "M", "stackOf": 53, "ship": { "state": "cancelled", "with": 53, "detail": "Ships with stack #53" } }), "Human test");
        assert!(left_ship_for_stack(&joined));
        // A plain stack card, a card whose ship was cancelled some other way or for another stack, and a lone card are left to after_move.
        assert!(!left_ship_for_stack(&numbered(5, json!({ "title": "M", "stackOf": 53 }), "Planning")));
        assert!(!left_ship_for_stack(&numbered(6, json!({ "title": "M", "stackOf": 53, "ship": { "state": "cancelled", "detail": "Ships with stack #53" } }), "Human test")));
        assert!(!left_ship_for_stack(&numbered(8, json!({ "title": "M", "stackOf": 53, "ship": { "state": "cancelled", "with": 9 } }), "Human test")));
        assert!(!left_ship_for_stack(&numbered(7, json!({ "title": "M", "ship": { "state": "cancelled", "with": 53 } }), "Human test")));
    }
}
