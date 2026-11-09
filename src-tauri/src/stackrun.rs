//! Stacks build in parallel (card #113, 6 Nov 2026).
//!
//! A stack's children are ordinary card sessions: each plans, builds and tests itself in its own
//! worktree, on its own branch made from the stack's branch. The lead builds nothing: it is the
//! integrator. When a child reaches Human test the app wakes the lead with a merge brief; the
//! lead's card-merger (card #100's "stack" job) brings the child's branch into the stack's
//! branch, and `after_child_merge` acts on its verdict.
//!
//! Hugh's move of a lead into Planning or In progress approves every start and resume of
//! sessions in that stack while the lead stays in Planning, In progress or Human test (the
//! router's decision R1, PLAN.md). Children still go through the critic: the app never moves a
//! child into In progress.
//!
//! Re-exported from `dispatch`, so callers write `dispatch::child_ready` and so on.

use crate::sessions::Sessions;
use kanban_store::{Card, Patch, Store, STACK_READY};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

/// Lead columns in which Hugh's approval of the stack holds (R1).
pub const STARTED: [&str; 3] = ["Planning", "In progress", "Human test"];
/// A lead's session effort when the card has no effortOverride: it only integrates.
pub const LEAD_EFFORT: &str = "low";
/// Who the app's own stack moves and messages are by.
#[allow(dead_code)]
pub const BY: &str = "the app (stack)";

/// The R3 merge, run in the lead's worktree: detach at the child's head, rebase onto the stack's
/// branch, then fast-forward the stack's branch to the result. The child's branch is never
/// checked out or rewritten here (it is checked out in the child's own worktree, and git refuses
/// a branch another worktree has). `{rebased}` is the SHA the third step prints. The brief and
/// the scratch-repo test both use this list.
pub const MERGE_STEPS: [&str; 5] = [
    "git switch --detach {child}",
    "git rebase {stack}",
    "git rev-parse HEAD",
    "git switch {stack}",
    "git merge --ff-only {rebased}",
];

/// The merge steps for one child, as the brief gives them (`{rebased}` stays a placeholder).
pub fn merge_steps(stack: &str, child: &str) -> Vec<String> {
    MERGE_STEPS.iter().map(|s| s.replace("{stack}", stack).replace("{child}", child)).collect()
}

fn text(card: &Card, key: &str) -> Option<String> {
    card.fields.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// The cards `card` waits on: card #109's `blockedBy` (a list of card numbers). Empty on a
/// build without #109, so every child counts as waiting on nothing there.
pub fn waits_on(card: &Card) -> Vec<i64> {
    card.fields.get("blockedBy").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default()
}

/// Has Hugh started stack `lead` (R1): the lead is in Planning, In progress or Human test.
pub fn stack_started(lead: &Card) -> bool {
    !lead.archived && STARTED.contains(&lead.column.as_str())
}

/// Is one wait of a child met? A sibling in the same stack must be merged into the stack's
/// branch (R4); any other card must be in Human test or later (card #109's rule).
fn wait_met(store: &Store, lead: &Card, n: i64) -> bool {
    let Ok(c) = store.get_by_number(n) else { return true };
    if c.stack_of() == Some(lead.number) {
        return STACK_READY.contains(&c.column.as_str()) && kanban_store::stack_merged(&c, lead);
    }
    STACK_READY.contains(&c.column.as_str())
}

/// Does `child` still wait on a card that isn't merged (a sibling) or met (any other)?
pub fn waiting(store: &Store, child: &Card, lead: &Card) -> bool {
    waits_on(child).into_iter().any(|n| !wait_met(store, lead, n))
}

/// A child's stage, for kanban_stack_list and the board (card #116).
pub fn stage(store: &Store, child: &Card, lead: &Card) -> &'static str {
    kanban_store::stack_stage(child, lead, waiting(store, child, lead))
}

/// The worktree a child makes for itself: its repo's checkout folder with `-<n>`, as
/// kanban-app.md says (C:\Users\someone\kanban-app-<n>).
fn child_worktree(child: &Card, lead: &Card) -> String {
    let repo = text(child, "repo").or_else(|| text(lead, "repo"));
    format!("{}-{}", crate::sessions::repo_folder(repo.as_deref()).display(), child.number)
}

/// What a stack child's session is told in every message about a move or a start: how it
/// builds in a parallel stack.
pub fn child_note(child: &Card, lead: &Card) -> String {
    let n = child.number;
    let (l, lt) = (lead.number, lead.title());
    let stack = text(lead, "branch");
    let sb = stack.clone().unwrap_or_else(|| "the stack's branch (on the lead card)".into());
    let own = text(child, "branch").filter(|b| Some(b) != stack.as_ref());
    let branch = own.unwrap_or_else(|| format!("{}-{n}", stack.as_deref().unwrap_or("stack")));
    let lw = text(lead, "worktreePath").map(|w| format!(" in {w}")).unwrap_or_default();
    format!(
        "This card is in stack #{l} \"{lt}\", which builds its cards in parallel (card #113). You build only this card, in your own worktree on your own branch: {wt}, on branch {branch}, made from {sb} (the stack's branch, checked out by the lead{lw}). Record worktreePath and branch on this card. Don't commit to {sb}, don't open a PR and don't bring up a dev stack: the lead does those once for the stack. When your card-reviewer and card-tester pass on your branch, write your own how-to-test into nextStep and move this card to Human test: your session then stops, and the lead merges your branch into {sb}. If you are told the stack's branch moved, rebase your branch onto it before you finish. Planning: once your plan is ready and your card-critic (if your session has it) has passed, move this card to In progress yourself if stack #{l} is In progress; otherwise wait for Hugh or the router.",
        wt = child_worktree(child, lead),
    )
}

/// The brief for the lead's card-merger, for child `child` that reached Human test.
pub fn merge_brief(child: &Card, lead: &Card) -> String {
    let n = child.number;
    let stack = text(lead, "branch").unwrap_or_else(|| "<the stack's branch>".into());
    let cb = text(child, "branch").unwrap_or_else(|| format!("<#{n}'s branch, on its card>"));
    let wt = text(lead, "worktreePath").unwrap_or_else(|| "<your worktree>".into());
    let steps: Vec<String> = merge_steps(&stack, &cb).iter().enumerate().map(|(i, s)| format!("{}. {s}", i + 1)).collect();
    format!(
        "#{n} \"{t}\" in this stack passed its card-reviewer and card-tester and is in Human test, on branch {cb}. Merge it into this stack's branch {stack}: run the card-merger subagent (its \"stack\" job, outside Ship it), and brief it with these facts.\n- CARD: #{n}. The stack's lead: #{l}. The worktree to work in: {wt} (it has {stack} checked out).\n- In that worktree, run these steps in order. Step 3 prints the SHA that step 5 uses as {{rebased}}.\n{steps}\n- Never check out or rewrite {cb}: #{n}'s own worktree has it. Never merge to main, never push.\n- On a conflict, resolve it if the right answer is plain; otherwise run git rebase --abort and git switch {stack}, and report FAIL.\n- Then run the repo's tests in {wt}. Report CARD: #{n} and VERDICT: PASS or FAIL, as the merger does.\nThe app reads the merger's verdict. On PASS it reruns the stack rules and starts any card that waited on #{n}. On FAIL it sends #{n} back to its own session to rebase and fix. You don't move #{n} yourself.",
        t = child.title(),
        l = lead.number,
        steps = steps.join("\n"),
    )
}

/// Queues `text` for `card`'s session, then starts or resumes the session if it isn't running
/// (R1: callers use it only for a card in a stack Hugh started). Typed when the session is free,
/// never mid-turn. A card that has never had a session gets its header first (a new session
/// that finds a message waiting sends only its template's prompt). The start runs on its own
/// thread, once the caller lets go of the board, and only in a column that starts sessions
/// (auto-spawn), as AGENTS.md's hard rule has it with R1's exception.
pub fn wake(app: &AppHandle, store: &mut Store, card: &Card, text: &str) {
    let live = app.state::<Arc<Sessions>>();
    let running = live.is_live(&card.id).is_some();
    let fresh = !running && store.latest_session(&card.id).ok().flatten().is_none();
    let text = if fresh { format!("{}\n\n{text}", crate::dispatch::header(card)) } else { text.to_string() };
    let _ = store.queue(&card.id, "message", &crate::dispatch::dry(card, text));
    if running {
        return;
    }
    let (a, id) = (app.clone(), card.id.clone());
    std::thread::spawn(move || start_now(&a, &id));
}

/// The start half of `wake`, under its own lock of the board.
fn start_now(app: &AppHandle, card_id: &str) {
    let board = app.state::<crate::Board>();
    let Ok(mut store) = board.0.lock() else { return };
    let Ok(card) = store.get(card_id) else { return };
    let live = app.state::<Arc<Sessions>>();
    if card.archived || live.is_live(&card.id).is_some() || store.start_queue().map(|q| q.position(&card.id).is_some()).unwrap_or(false) {
        return;
    }
    if !store.column(&card.column).ok().flatten().map_or(false, |c| c.auto_spawn) {
        let _ = store.add_log(&card.id, &format!("A message waits for its session; {} doesn't start sessions, so it starts when Hugh opens it", card.column), "the app");
        return;
    }
    // A conversation that lives in the desktop app is resumed only when Hugh asks.
    let app_convo = store.latest_session(&card.id).ok().flatten();
    if app_convo.is_none() && card.fields.get("claudeSessionId").is_some() {
        return;
    }
    let mode = if app_convo.is_some() { "resume" } else { "new" };
    if let Err(e) = crate::begin_session(app, &mut store, &card, mode, "move") {
        let _ = store.add_log(&card.id, &format!("Couldn't start its session for the stack: {e}"), "the app");
    }
    drop(store);
    let _ = app.emit("session-changed", card_id.to_string());
    let _ = app.emit("cards-changed", ());
}

/// Marks a stack child whose own session starts: it builds itself (card #113).
pub fn mark_own_build(store: &mut Store, card_id: &str) {
    let Ok(c) = store.get(card_id) else { return };
    if c.stack_of().is_none() || kanban_store::stack_own_build(&c) {
        return;
    }
    let mut p = Patch::new();
    p.insert(kanban_store::STACK_OWN_BUILD_KEY.into(), json!(true));
    if store.update(&c.id, c.version, &p, "the app").is_ok() {
        let _ = store.add_log(&c.id, "Builds itself in its own session (card #113): its lead merges it into the stack's branch once it passes", "the app");
    }
}

/// Does a lead run on the lead's effort in its column: Planning and In progress, where it
/// integrates. Other columns (Ship it) keep their own.
pub fn lead_effort_applies(card: &Card) -> bool {
    card.is_lead() && ["Planning", "In progress"].contains(&card.column.as_str())
}

/// The column prompt for `card` in its column, if the column has one.
fn column_prompt(store: &Store, card: &Card) -> Option<String> {
    let cfg = store.column(&card.column).ok().flatten()?;
    if cfg.mode == "none" {
        return None;
    }
    let p = cfg.prompt_text?;
    let reason = card.fields.get("blockedReason").and_then(Value::as_str).unwrap_or("");
    Some(crate::dispatch::fill(&p, card, "", reason))
}

/// Starts (or resumes) a stack child's session with `intro`, its column's prompt and its note.
fn start_child(app: &AppHandle, store: &mut Store, child: &Card, lead: &Card, intro: &str) {
    let mut parts = vec![intro.to_string()];
    if let Some(p) = column_prompt(store, child) {
        parts.push(p);
    }
    // Card #151: the subagents Hugh has off on the child, for its column.
    if let Some(n) = crate::dispatch::subagents_note(store, child, &child.column) {
        parts.push(n);
    }
    parts.push(child_note(child, lead));
    wake(app, store, child, &parts.join("\n\n"));
}

/// Moves `child` from Backlog to Planning for Hugh, and starts its session (R1).
fn plan_child(app: &AppHandle, store: &mut Store, child: &Card, lead: &Card, why: &str) {
    let child = if child.column == "Backlog" {
        match store.move_for_hugh(&child.id, child.version, "Planning", why) {
            Ok(m) => m.card,
            Err(e) => {
                let _ = store.add_log(&child.id, &format!("Couldn't start it for stack #{}: {e}", lead.number), "the app");
                return;
            }
        }
    } else {
        child.clone()
    };
    start_child(app, store, &child, lead, &format!("The app started this card: {why}."));
}

/// Hugh moved stack lead `lead_id` into Planning or In progress (R1): each child that waits on
/// nothing and has never had a session starts. One in Backlog goes to Planning first. A child
/// that waits on a sibling starts when that sibling is merged (`after_child_merge`).
pub fn start_stack(app: &AppHandle, store: &mut Store, lead_id: &str) {
    let Ok(lead) = store.get(lead_id) else { return };
    let live = app.state::<Arc<Sessions>>().inner().clone();
    let fresh = |k: &Card| live.is_live(&k.id).is_none() && store.latest_session(&k.id).ok().flatten().is_none() && k.fields.get("claudeSessionId").is_none();
    let due = starts_for(store, &lead, &fresh);
    // A child that planned while the lead was in Planning waits in Planning for this move.
    let told = moves_on_for(store, &lead, &fresh);
    let why = format!("its stack #{} started (Hugh moved the lead to {})", lead.number, lead.column);
    for k in due {
        plan_child(app, store, &k, &lead, &why);
    }
    for k in told {
        let _ = store.add_log(&k.id, &format!("Told it that its stack's lead #{} is In progress, so it can move on (card #113)", lead.number), "the app");
        wake(app, store, &k, &moves_on_note(&lead));
    }
    // A child that reached Human test while the stack wasn't started missed its brief.
    for k in store.stack_children(&lead).unwrap_or_default() {
        if k.column == "Human test" && !k.archived && !kanban_store::stack_merged(&k, &lead) {
            child_ready(app, store, &k.id);
        }
    }
    let _ = app.emit("cards-changed", ());
}

/// The children of `lead` that `start_stack` starts: the lead is in Planning or In progress,
/// and each is live, in Backlog, Planning or In progress, waits on nothing, and is `fresh` (has
/// never had a session).
pub fn starts_for(store: &Store, lead: &Card, fresh: &dyn Fn(&Card) -> bool) -> Vec<Card> {
    if !lead.is_lead() || lead.archived || !["Planning", "In progress"].contains(&lead.column.as_str()) {
        return vec![];
    }
    store
        .stack_children(lead)
        .unwrap_or_default()
        .into_iter()
        .filter(|k| !k.archived && ["Backlog", "Planning", "In progress"].contains(&k.column.as_str()) && fresh(k) && !waiting(store, k, lead))
        .collect()
}

/// The children of `lead` that hear the lead is now In progress: each is live, in Planning,
/// builds itself, waits on nothing and has had a session (`fresh` is false). Their plans may
/// already have passed the critic, and planning.md has them wait for this move.
pub fn moves_on_for(store: &Store, lead: &Card, fresh: &dyn Fn(&Card) -> bool) -> Vec<Card> {
    if !lead.is_lead() || lead.archived || lead.column != "In progress" {
        return vec![];
    }
    store
        .stack_children(lead)
        .unwrap_or_default()
        .into_iter()
        .filter(|k| !k.archived && k.column == "Planning" && kanban_store::stack_own_build(k) && !fresh(k) && !waiting(store, k, lead))
        .collect()
}

/// The message to a planned child when Hugh moves its lead into In progress.
pub fn moves_on_note(lead: &Card) -> String {
    format!(
        "Your stack's lead #{} is now In progress. If your plan passed the critic, move this card to In progress now, as the last thing in your turn. If it hasn't passed yet, finish the critique first (planning.md, its stack section).",
        lead.number
    )
}

/// What a merge PASS of child `n` does to its siblings.
#[derive(Debug, Clone, PartialEq)]
pub enum AfterMerge {
    /// It waited on `n` and now waits on nothing: it starts, from the stack's branch (R4).
    Start(i64),
    /// It is still building with a session: it hears that the stack's branch moved.
    BaseMoved(i64),
}

/// The siblings' part of a merge PASS of child `n` in `lead`'s stack. `idle`: the card has no
/// running or queued session; `has_session`: it has a session to tell.
pub fn after_merge_plan(store: &Store, lead: &Card, n: i64, idle: &dyn Fn(&Card) -> bool, has_session: &dyn Fn(&Card) -> bool) -> Vec<AfterMerge> {
    let started = stack_started(lead);
    let mut out = vec![];
    for k in store.stack_children(lead).unwrap_or_default() {
        if k.archived || k.number == n {
            continue;
        }
        let col = k.column.as_str();
        if waits_on(&k).contains(&n) && started && idle(&k) && ["Backlog", "Planning", "In progress"].contains(&col) && !waiting(store, &k, lead) {
            out.push(AfterMerge::Start(k.number));
        } else if ["Planning", "In progress"].contains(&col) && has_session(&k) {
            out.push(AfterMerge::BaseMoved(k.number));
        }
    }
    out
}

/// A card created or added into a started stack: it starts as the stack's other children did
/// (in Backlog, Planning or In progress, waiting on nothing, never had a session), or, already
/// built in Human test, its lead gets the brief to merge it. A card that joins with a session of
/// its own builds itself from now on.
pub fn joined_started_stack(app: &AppHandle, store: &mut Store, child_id: &str) {
    let Ok(child) = store.get(child_id) else { return };
    let Ok(Some(lead)) = store.stack_lead(&child) else { return };
    if crate::dispatch::has_session(app, store, &child.id) {
        mark_own_build(store, &child.id);
    }
    if !["Planning", "In progress"].contains(&lead.column.as_str()) || lead.archived {
        return;
    }
    if child.column == "Human test" {
        child_ready(app, store, &child.id);
        return;
    }
    let live = app.state::<Arc<Sessions>>().inner().clone();
    let fresh = |k: &Card| live.is_live(&k.id).is_none() && store.latest_session(&k.id).ok().flatten().is_none() && k.fields.get("claudeSessionId").is_none();
    if starts_for(store, &lead, &fresh).iter().any(|k| k.id == child.id) {
        plan_child(app, store, &child, &lead, &format!("it joined stack #{}, which has started", lead.number));
    }
}

/// A card that leads a stack now, with a running session and no effortOverride, switches to
/// the lead's effort when it is next free (the session already on it is left alone).
pub fn lead_effort(app: &AppHandle, store: &mut Store, lead_n: i64) {
    let Ok(l) = store.get_by_number(lead_n) else { return };
    if !lead_effort_applies(&l) || l.fields.get("effortOverride").and_then(Value::as_str).is_some() {
        return;
    }
    let live = app.state::<Arc<Sessions>>();
    if live.is_live(&l.id).is_none() {
        return;
    }
    if let Ok(w) = live.request_switch(&l.id, None, Some(LEAD_EFFORT.into())) {
        if w.effort.is_some() {
            let _ = store.add_log(&l.id, &format!("It leads a stack now, so its session switches to effort {LEAD_EFFORT} when it is next free (card #113)"), "the app");
        }
    }
}

/// A stack child reached Human test: wake the lead with the merge brief (R1). Nothing for a
/// child already merged, or a stack Hugh hasn't started.
pub fn child_ready(app: &AppHandle, store: &mut Store, child_id: &str) {
    let Ok(child) = store.get(child_id) else { return };
    let Ok(Some(lead)) = store.stack_lead(&child) else { return };
    if child.column != "Human test" || child.archived || !stack_started(&lead) || kanban_store::stack_merged(&child, &lead) {
        return;
    }
    let _ = store.add_log(&lead.id, &format!("#{} is ready to merge: the app asked this card's session to merge it", child.number), "the app");
    wake(app, store, &lead, &merge_brief(&child, &lead));
}

/// Is `card` a stack child whose own held move into `col` (a ready column) leaves it waiting,
/// so its session stops (item 6)?
pub fn stops_after_move(store: &Store, card_id: &str, col: &str) -> bool {
    store.get(card_id).map_or(false, |c| c.stack_of().is_some() && c.column == col && STACK_READY.contains(&col))
}

/// After the app recorded the card-merger's verdict on stack child `child_id` (card #100 calls
/// this). PASS: the stack rules run again (the stack may be ready), each sibling whose waits are
/// now all merged starts from the stack's branch (R4), and siblings still building hear that the
/// base moved. FAIL: the child goes back to In progress and its session is told to rebase.
#[allow(dead_code)] // called by card #100's merge record once it lands; tested directly until then
pub fn after_child_merge(app: &AppHandle, store: &mut Store, child_id: &str) {
    let Ok(child) = store.get(child_id) else { return };
    let Ok(Some(lead)) = store.stack_lead(&child) else { return };
    let merge = child.fields.get("merge");
    let state = merge.and_then(|m| m.get("state")).and_then(Value::as_str).unwrap_or("");
    let detail = merge.and_then(|m| m.get("detail")).and_then(Value::as_str).unwrap_or("").to_string();
    let sb = text(&lead, "branch").unwrap_or_else(|| "the stack's branch".into());
    let n = child.number;
    match state {
        "passed" => {
            crate::dispatch::after_lead_verdict(app, store, &lead.id, BY, &format!("#{n} was merged"));
            let lead = store.get(&lead.id).unwrap_or(lead);
            let live = app.state::<Arc<Sessions>>().inner().clone();
            let queue = store.start_queue().unwrap_or_default();
            let idle = |k: &Card| live.is_live(&k.id).is_none() && queue.position(&k.id).is_none();
            let told = |k: &Card| live.is_live(&k.id).is_some() || store.latest_session(&k.id).ok().flatten().is_some();
            let plan = after_merge_plan(store, &lead, n, &idle, &told);
            for step in plan {
                let (AfterMerge::Start(m) | AfterMerge::BaseMoved(m)) = step.clone();
                let Ok(k) = store.get_by_number(m) else { continue };
                match step {
                    AfterMerge::Start(_) => plan_child(app, store, &k, &lead, &format!("#{n}, which it waited on, is merged into the stack's branch {sb}")),
                    AfterMerge::BaseMoved(_) => {
                        let note = format!("Stack #{}'s branch {sb} moved: #{n} was merged into it. Before you move this card to Human test, rebase your branch onto {sb} in your worktree and run your tests again.", lead.number);
                        let _ = store.queue(&k.id, "message", &crate::dispatch::dry(&k, note));
                    }
                }
            }
        }
        "failed" => {
            let mut p = Patch::new();
            let from = child.column.clone();
            if from != "In progress" {
                p.insert("column".into(), json!("In progress"));
                if store.update(&child.id, child.version, &p, BY).is_ok() {
                    let _ = store.add_log(&child.id, &format!("Moved from {from} to In progress by {BY}: its merge into {sb} failed"), BY);
                    crate::dispatch::after_stack_move(app, store, &child.id, BY);
                }
            }
            let why = if detail.is_empty() { "see the merger's report in the lead's session".to_string() } else { detail };
            let text = format!("The merge of this card's branch into stack #{}'s branch {sb} failed: {why}. In your worktree, rebase your branch onto {sb}, resolve the conflicts, run your card-reviewer and card-tester again, and move this card back to Human test. The lead merges it again then.", lead.number);
            if let Ok(c) = store.get(&child.id) {
                if stack_started(&lead) {
                    wake(app, store, &c, &text);
                } else {
                    let _ = store.queue(&c.id, "message", &crate::dispatch::dry(&c, text));
                }
            }
        }
        _ => {
            let _ = store.add_log(&lead.id, &format!("The merger's verdict on #{n} didn't count ({}): run it again", if state.is_empty() { "no verdict" } else { state }), "the app");
        }
    }
    let _ = app.emit("cards-changed", ());
}

/// A move of stack child `card` (already moved, from `from`) by a session other than its own,
/// or by the app: its own session is told who moved it and why, and woken within R1 when the
/// move gives it work (Planning or In progress).
pub fn child_moved(app: &AppHandle, store: &mut Store, card: &Card, from: &str, reason: &str, by: &str) {
    let Ok(Some(lead)) = store.stack_lead(card) else { return };
    if by.starts_with(&format!("card #{}'s session", card.number)) || !kanban_store::stack_own_build(card) {
        return;
    }
    let why = if reason.is_empty() { String::new() } else { format!(": {reason}") };
    let mut text = format!("{by} moved this card from {from} to {}{why}.", card.column);
    if let Some(p) = column_prompt(store, card) {
        text = format!("{text}\n\n{p}");
    }
    // Card #151: the subagents Hugh has off on the child, for its column.
    if let Some(n) = crate::dispatch::subagents_note(store, card, &card.column) {
        text = format!("{text}\n\n{n}");
    }
    text = format!("{text}\n\n{}", child_note(card, &lead));
    if ["Planning", "In progress"].contains(&card.column.as_str()) && stack_started(&lead) {
        wake(app, store, card, &text);
    } else if crate::dispatch::has_session(app, store, &card.id) {
        let _ = store.queue(&card.id, "message", &crate::dispatch::dry(card, text));
    }
}

/// Detach (finding 11): refused for a merged child; it is in the stack's branch already.
pub fn detach_refused(child: &Card, lead: &Card) -> Option<String> {
    (kanban_store::stack_own_build(child) && kanban_store::stack_merged(child, lead) && !child.archived).then(|| {
        format!(
            "#{} is merged into stack #{}'s branch already, so it can't be detached: send it back with Found a problem, or let it ship with the stack",
            child.number, lead.number
        )
    })
}

/// What a detached child's own session is told (it keeps its session and branch).
pub fn detached_note(child: &Card, lead: &Card) -> String {
    let sb = text(lead, "branch").unwrap_or_else(|| "the stack's branch".into());
    format!(
        "This card was detached from stack #{}: it is its own card now, and ships on its own. Your branch was made from {sb}, so it carries the stack's other commits. In your worktree, rebase it onto origin/main keeping only this card's own \"(card #{})\" commits (for example git rebase --onto origin/main {sb}), run your checks again, and hand it over as its column's prompt says.",
        lead.number, child.number
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    fn card(number: i64, column: &str, fields: Value) -> Card {
        Card { id: format!("card-{number}"), number, column: column.into(), order: None, order_col: None, archived: false, fields: fields.as_object().cloned().unwrap(), version: 1 }
    }

    #[test]
    fn waits_read_blocked_by_and_nothing_else() {
        assert_eq!(waits_on(&card(5, "Backlog", json!({ "blockedBy": [3, 4] }))), vec![3, 4]);
        assert!(waits_on(&card(5, "Backlog", json!({}))).is_empty());
        assert!(waits_on(&card(5, "Backlog", json!({ "blockedBy": "3" }))).is_empty());
    }

    #[test]
    fn started_means_planning_in_progress_or_human_test() {
        for (col, on) in [("Backlog", false), ("Planning", true), ("In progress", true), ("Human test", true), ("Ship it", false), ("Blocked", false)] {
            assert_eq!(stack_started(&card(1, col, json!({ "stack": [2] }))), on, "{col}");
        }
    }

    #[test]
    fn the_childs_note_names_its_worktree_branch_and_base() {
        let lead = card(40, "In progress", json!({ "title": "L", "stack": [41], "branch": "dsh-order", "worktreePath": "C:\\Users\\someone\\kanban-app-order" }));
        let kid = card(41, "Planning", json!({ "title": "K", "stackOf": 40, "repo": "hughht5/kanban-app" }));
        let n = child_note(&kid, &lead);
        // The repo's folder (repos.json, else home) plus the child's number.
        let want = format!("{}-41,", crate::sessions::repo_folder(Some("hughht5/kanban-app")).display());
        assert!(n.contains(&want), "{n}");
        assert!(n.contains("on branch dsh-order-41, made from dsh-order"), "{n}");
        assert!(n.contains("don't bring up a dev stack"));
        // A child that has its own branch already keeps it.
        let kid = card(41, "In progress", json!({ "title": "K", "stackOf": 40, "branch": "mine" }));
        assert!(child_note(&kid, &lead).contains("on branch mine, made from dsh-order"));
    }

    #[test]
    fn the_merge_brief_gives_the_r3_steps_in_the_leads_worktree() {
        let lead = card(40, "In progress", json!({ "title": "L", "stack": [41], "branch": "dsh-order", "worktreePath": "C:\\w\\lead" }));
        let kid = card(41, "Human test", json!({ "title": "K", "stackOf": 40, "branch": "dsh-order-41" }));
        let b = merge_brief(&kid, &lead);
        assert!(b.contains("1. git switch --detach dsh-order-41"), "{b}");
        assert!(b.contains("2. git rebase dsh-order"));
        assert!(b.contains("5. git merge --ff-only {rebased}"));
        assert!(b.contains("C:\\w\\lead") && b.contains("CARD: #41") && b.contains("Never merge to main, never push"));
    }

    #[test]
    fn detach_is_refused_only_for_a_merged_child() {
        let lead = card(40, "In progress", json!({ "title": "L", "stack": [41], "branch": "s" }));
        assert!(detach_refused(&card(41, "Human test", json!({ "stackOwnBuild": true, "merge": { "state": "passed" } })), &lead).is_some());
        assert!(detach_refused(&card(41, "Human test", json!({ "stackOwnBuild": true, "merge": { "state": "failed" } })), &lead).is_none());
        // A child its lead built the old way detaches as before.
        assert!(detach_refused(&card(41, "Human test", json!({})), &lead).is_none());
        assert!(detach_refused(&card(41, "In progress", json!({})), &lead).is_none());
        assert!(detached_note(&card(41, "In progress", json!({})), &lead).contains("git rebase --onto origin/main s"));
    }

    /// A board with lead #L (branch "s", in `lead_col`) and children made from `kids`
    /// (title, column, extra fields). Returns the store, the lead's number and the children's.
    fn stack(dir: &tempfile::TempDir, lead_col: &str, kids: &[(&str, &str, Value)]) -> (Store, i64, Vec<i64>) {
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let lead = s.create(None, json!({ "title": "lead", "branch": "s", "column": "Backlog" }).as_object().cloned().unwrap(), "test").unwrap();
        let mut nums = vec![];
        for (t, col, extra) in kids {
            let c = s.create_in_stack(json!({ "title": t }).as_object().cloned().unwrap(), lead.number, "test").unwrap();
            let mut p: Patch = extra.as_object().cloned().unwrap_or_default();
            p.insert("column".into(), json!(col));
            s.update(&c.id, c.version, &p, "test").unwrap();
            nums.push(c.number);
        }
        let l = s.get_by_number(lead.number).unwrap();
        let mut p = Patch::new();
        p.insert("column".into(), json!(lead_col));
        s.update(&l.id, l.version, &p, "test").unwrap();
        (s, lead.number, nums)
    }

    #[test]
    fn a_started_stack_starts_each_fresh_child_that_waits_on_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (s, l, k) = stack(&dir, "Planning", &[("a", "Backlog", json!({})), ("b", "Backlog", json!({})), ("c", "Planning", json!({})), ("d", "Human test", json!({}))]);
        // b waits on a, which isn't merged yet (R4).
        let b = s.get_by_number(k[1]).unwrap();
        let mut s = s;
        let mut p = Patch::new();
        p.insert("blockedBy".into(), json!([k[0]]));
        s.update(&b.id, b.version, &p, "test").unwrap();
        let lead = s.get_by_number(l).unwrap();
        let due: Vec<i64> = starts_for(&s, &lead, &|_| true).iter().map(|c| c.number).collect();
        assert_eq!(due, vec![k[0], k[2]], "a and c start; b waits on a; d is built");
        // A child that has had a session isn't started again.
        let c_id = s.get_by_number(k[2]).unwrap().id;
        let due: Vec<i64> = starts_for(&s, &lead, &|c: &Card| c.id != c_id).iter().map(|c| c.number).collect();
        assert_eq!(due, vec![k[0]]);
        // A stack Hugh hasn't started starts nothing (R1).
        let dir2 = tempfile::tempdir().unwrap();
        let (s2, l2, _) = stack(&dir2, "Backlog", &[("a", "Backlog", json!({}))]);
        assert!(starts_for(&s2, &s2.get_by_number(l2).unwrap(), &|_| true).is_empty());
    }

    #[test]
    fn a_planned_child_hears_when_the_lead_goes_into_in_progress() {
        let dir = tempfile::tempdir().unwrap();
        let own = json!({"stackOwnBuild": true});
        let (s, l, k) = stack(&dir, "Planning", &[("a", "Planning", own.clone()), ("b", "Planning", json!({})), ("c", "In progress", own.clone()), ("d", "Planning", own.clone())]);
        let mut s = s;
        // d waits on a, which isn't merged.
        let d = s.get_by_number(k[3]).unwrap();
        let mut p = Patch::new();
        p.insert("blockedBy".into(), json!([k[0]]));
        s.update(&d.id, d.version, &p, "test").unwrap();
        let had = |_: &Card| false;
        // The lead in Planning: nobody moves on yet.
        assert!(moves_on_for(&s, &s.get_by_number(l).unwrap(), &had).is_empty());
        let lead = s.get_by_number(l).unwrap();
        let lead = s.move_for_hugh(&lead.id, lead.version, "In progress", "test").unwrap().card;
        let told: Vec<i64> = moves_on_for(&s, &lead, &had).iter().map(|c| c.number).collect();
        assert_eq!(told, vec![k[0]], "a only: b is built the old way, c is In progress, d waits");
        // A child with no session yet is started by starts_for, not told.
        assert!(moves_on_for(&s, &lead, &|_| true).is_empty());
        assert!(moves_on_note(&lead).contains("move this card to In progress"));
    }

    #[test]
    fn a_child_waits_on_a_sibling_until_it_is_merged_then_starts_and_the_rest_hear_the_base_moved() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, l, k) = stack(
            &dir,
            "In progress",
            &[
                ("a", "Human test", json!({ "branch": "s-a" })),
                ("b", "Backlog", json!({})),
                ("c", "In progress", json!({ "branch": "s-c" })),
                ("d", "In progress", json!({ "branch": "s-d" })),
            ],
        );
        let b = s.get_by_number(k[1]).unwrap();
        let mut p = Patch::new();
        p.insert("blockedBy".into(), json!([k[0]]));
        s.update(&b.id, b.version, &p, "test").unwrap();
        let lead = s.get_by_number(l).unwrap();
        let (a, b) = (s.get_by_number(k[0]).unwrap(), s.get_by_number(k[1]).unwrap());
        // In Human test but not merged: b still waits, and a is "to-merge".
        assert!(waiting(&s, &b, &lead));
        assert_eq!(stage(&s, &a, &lead), "to-merge");
        assert_eq!(stage(&s, &b, &lead), "waiting");
        let mut p = Patch::new();
        p.insert("merge".into(), json!({ "state": "passed" }));
        s.update(&a.id, a.version, &p, "test").unwrap();
        let a = s.get_by_number(k[0]).unwrap();
        assert!(!waiting(&s, &b, &lead));
        assert_eq!(stage(&s, &a, &lead), "merged");
        assert_eq!(stage(&s, &b, &lead), "building");
        // c has a session, d hasn't: only c hears the base moved; b starts.
        let c_id = s.get_by_number(k[2]).unwrap().id;
        let plan = after_merge_plan(&s, &lead, k[0], &|_| true, &|c: &Card| c.id == c_id);
        assert_eq!(plan, vec![AfterMerge::Start(k[1]), AfterMerge::BaseMoved(k[2])]);
        // Not in a stack Hugh has started: nothing starts.
        let mut idle_lead = lead.clone();
        idle_lead.column = "Backlog".into();
        assert_eq!(after_merge_plan(&s, &idle_lead, k[0], &|_| true, &|_| false), vec![]);
    }

    #[test]
    fn a_waiting_child_whose_blocker_is_outside_the_stack_waits_for_human_test() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, l, k) = stack(&dir, "In progress", &[("a", "Backlog", json!({}))]);
        let other = s.create(None, json!({ "title": "other", "column": "In progress" }).as_object().cloned().unwrap(), "test").unwrap();
        let a = s.get_by_number(k[0]).unwrap();
        let mut p = Patch::new();
        p.insert("blockedBy".into(), json!([other.number]));
        s.update(&a.id, a.version, &p, "test").unwrap();
        let (lead, a) = (s.get_by_number(l).unwrap(), s.get_by_number(k[0]).unwrap());
        assert!(waiting(&s, &a, &lead));
        let mut p = Patch::new();
        p.insert("column".into(), json!("Human test"));
        s.update(&other.id, other.version, &p, "test").unwrap();
        assert!(!waiting(&s, &a, &lead));
    }

    #[test]
    fn a_child_stops_only_after_its_own_move_into_a_ready_column() {
        let dir = tempfile::tempdir().unwrap();
        let (s, _, k) = stack(&dir, "In progress", &[("a", "Human test", json!({})), ("b", "In progress", json!({}))]);
        let (a, b) = (s.get_by_number(k[0]).unwrap(), s.get_by_number(k[1]).unwrap());
        assert!(stops_after_move(&s, &a.id, "Human test"));
        assert!(!stops_after_move(&s, &b.id, "In progress"));
        let solo = Card { id: "x".into(), ..a.clone() };
        assert!(!stops_after_move(&s, &solo.id, "Human test"), "a card not on the board, or not in a stack, never stops here");
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git").current_dir(dir).args(args).output().expect("git runs");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn commit(dir: &Path, file: &str, body: &str, msg: &str) {
        std::fs::write(dir.join(file), body).unwrap();
        git(dir, &["add", file]);
        git(dir, &["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q", "-m", msg]);
    }

    /// R3 on a scratch repo with two worktrees: the lead's (the stack's branch checked out) and a
    /// child's (its own branch checked out). The steps the brief gives bring the child's work
    /// onto the stack's branch, which had moved on since the child branched, and the child's own
    /// branch is left as it was.
    #[test]
    fn the_merge_steps_work_with_the_childs_branch_checked_out_elsewhere() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let root = std::env::temp_dir().join(format!("kanban-113-{}", uuid::Uuid::new_v4()));
        let lead = root.join("lead");
        std::fs::create_dir_all(&lead).unwrap();
        git(&lead, &["init", "-q", "-b", "stack"]);
        commit(&lead, "base.txt", "base\n", "base");
        let kid = root.join("lead-41");
        git(&lead, &["worktree", "add", "-q", "-b", "stack-41", kid.to_str().unwrap()]);
        commit(&kid, "kid.txt", "kid\n", "child work (card #41)");
        let kid_head = git(&kid, &["rev-parse", "HEAD"]);
        // A sibling was merged meanwhile: the stack's branch moved on.
        commit(&lead, "sib.txt", "sib\n", "sibling work (card #42)");
        let mut rebased = String::new();
        for step in merge_steps("stack", "stack-41") {
            let step = step.replace("{rebased}", &rebased);
            let args: Vec<&str> = step.split_whitespace().skip(1).collect();
            let out = git(&lead, &args);
            if args == ["rev-parse", "HEAD"] {
                rebased = out;
            }
        }
        assert_eq!(git(&lead, &["rev-parse", "--abbrev-ref", "HEAD"]), "stack");
        assert_eq!(git(&lead, &["rev-parse", "HEAD"]), rebased);
        assert!(lead.join("kid.txt").exists() && lead.join("sib.txt").exists());
        assert_eq!(git(&lead, &["log", "-1", "--format=%s"]), "child work (card #41)");
        // The child's branch and worktree are untouched.
        assert_eq!(git(&kid, &["rev-parse", "HEAD"]), kid_head);
        assert_eq!(git(&kid, &["rev-parse", "--abbrev-ref", "HEAD"]), "stack-41");
        let _ = git(&lead, &["worktree", "remove", "--force", kid.to_str().unwrap()]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
