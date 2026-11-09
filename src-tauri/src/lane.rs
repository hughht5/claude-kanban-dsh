//! The merge lanes (card #160): one Claude Code session the app owns per repo Hugh lists in
//! Settings › Sessions ("Ship through one merge lane per repo", `sessions.merge_lane_repos`). It
//! does the ship's merge for every card of that repo, one at a time, in its own detached
//! worktree, and keeps its conversation across ships, so it resolves conflicts with what it
//! learnt merging the earlier ones. The card's own session still pushes and ships.
//!
//! - Built like the Router (card #119): not a card; key `lane:<repo>` in the live session map;
//!   outside the cap and the start queue; never paused by the card idle rule; main account; its
//!   rules are its system prompt (`prompts/merge-lane.md`); no subagents; AskUserQuestion denied;
//!   its guard is `guard::lane_refusal`; its tools are `lanetools.rs`.
//! - Its jobs are the cards' `merge` records: `kanban_merge` (a card's session, in Ship it)
//!   writes `queued`; `tick` starts or resumes the lane, types the brief and writes `running`;
//!   `kanban_merge_result` (the lane) writes the verdict with `merger::record_of`. So an app
//!   restart loses nothing.
//! - It starts only for a merge a ship asked for (Hugh's move into Ship it approved that ship),
//!   and stops 15 minutes after its last job with nothing waiting. The conversation is kept.
//! - The pure decisions are `decide`, `merge_refused` and the message builders; `tick` gathers
//!   the facts and acts.

use crate::sessions::{Plan, Sessions};
use crate::Board;
use kanban_store::{lane_key, lane_repo, Card, MergeLaneRepo, Patch, Store, MERGE_KEY};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// The lane's rules: its system prompt, appended to Claude Code's own.
pub const PROMPT: &str = include_str!("../../prompts/merge-lane.md");
/// Who the app's records of a lane's work are by (its verdicts, its job's states).
pub const BY: &str = "the app (merge lane)";
/// Who the lane's own log lines on its job's card are by.
pub const LOG_BY: &str = "the merge lane";
/// A lane with no job stops after this long idle. The next job resumes its conversation.
pub const IDLE_STOP: Duration = Duration::from_secs(15 * 60);
/// No report this long after the brief: the merge is unclear.
pub const TIMEOUT: chrono::Duration = chrono::Duration::minutes(60);
/// How many earlier merges of the repo each brief lists.
pub const HISTORY: usize = 5;
/// How many starts a waiting job gets before the app gives up on the lane.
pub const MAX_STARTS: u32 = 2;
/// The longest a `rules` or `checks` text from the card's session may be.
const TEXT_MAX: usize = 4000;

/// Whether a live-map key is a merge lane's.
pub fn is_lane_key(key: &str) -> bool {
    lane_repo(key).is_some()
}

/// What the app holds for each lane between ticks, in memory only: everything a lane acts on
/// is on the board.
#[derive(Default, Debug)]
pub struct Mem {
    /// A start (or resume) is under way on its own thread.
    pub starting: bool,
    /// Starts made for the job now waiting, since its last brief.
    pub starts: u32,
    /// Messages for the lane, typed when it is free (a resume note, a dropped job).
    pub pending: VecDeque<String>,
}

pub struct Lanes(pub Mutex<HashMap<String, Mem>>);

impl Lanes {
    pub fn new() -> Lanes {
        Lanes(Mutex::new(HashMap::new()))
    }
}

/// The lane's worktree for a listed repo on this channel.
pub fn worktree_of(entry: &MergeLaneRepo) -> PathBuf {
    crate::worktree::lane_path(Path::new(&entry.checkout), kanban_store::channel())
}

// ---- A job, as the card's `merge` record holds it ----

/// A lane job read from a card.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub number: i64,
    pub running: bool,
    pub lane: String,
    pub base: String,
    pub branch: String,
    pub worktree: String,
    pub branches: Vec<String>,
    pub rules: Option<String>,
    pub checks: Option<String>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub resumes: i64,
    /// The card is in Ship it and not archived.
    pub in_ship_it: bool,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

/// The lane job on `card`, if its `merge` record is one still owed a verdict.
pub fn job_of(card: &Card) -> Option<Job> {
    let m = card.fields.get(MERGE_KEY)?;
    let state = m.get("state").and_then(Value::as_str)?;
    if !kanban_store::MERGE_WAITING.contains(&state) {
        return None;
    }
    Some(Job {
        number: card.number,
        running: state == "running",
        lane: s(m, "lane"),
        base: s(m, "base"),
        branch: s(m, "branch"),
        worktree: s(m, "worktree"),
        branches: m.get("branches").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default(),
        rules: m.get("rules").and_then(Value::as_str).map(str::to_string),
        checks: m.get("checks").and_then(Value::as_str).map(str::to_string),
        started_at: m.get("startedAt").and_then(Value::as_str).and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|t| t.with_timezone(&chrono::Utc)),
        resumes: m.get("resumes").and_then(Value::as_i64).unwrap_or(0),
        in_ship_it: card.column == "Ship it" && !card.archived,
    })
}

// ---- The decision ----

/// What `tick` knows about one lane at one pass.
#[derive(Debug, Clone, PartialEq)]
pub struct Facts {
    /// The repo is still on Hugh's list.
    pub listed: bool,
    /// Its session is running (or a start is under way).
    pub live: bool,
    /// Ready, idle and nothing being typed: a message may be typed now.
    pub free: bool,
    /// How long it has sat idle, when it is.
    pub idle_for: Option<Duration>,
    /// Its job: the running one, else the oldest waiting.
    pub job: Option<Job>,
    /// A message waits to be typed to it.
    pub pending: bool,
    /// Starts made for the waiting job since its last brief.
    pub starts: u32,
    pub now: chrono::DateTime<chrono::Utc>,
}

/// What `tick` does for one lane.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Nothing,
    /// The job's card left Ship it (or was archived): drop the job.
    Cancel(String),
    /// Start the lane (or resume its last conversation) for a waiting job.
    Start,
    /// The lane ended during its job (a crash, Hugh's Stop, an app restart): resume it once.
    Resume,
    /// Type the waiting job's brief.
    Brief,
    /// Type the first waiting message.
    Type,
    /// No report within `TIMEOUT` of the brief: unclear.
    TimeOut,
    /// The job can't be done: unclear, with the reason.
    GiveUp(String),
    /// No job and idle for `IDLE_STOP`: stop the lane (its conversation is kept).
    Stop,
}

/// The one thing to do for a lane now.
pub fn decide(f: &Facts) -> Action {
    let Some(j) = &f.job else {
        if f.live && f.free && f.pending {
            return Action::Type;
        }
        if f.live && f.free && f.idle_for.is_some_and(|d| d >= IDLE_STOP) {
            return Action::Stop;
        }
        return Action::Nothing;
    };
    if !j.in_ship_it {
        return Action::Cancel("the card left Ship it".into());
    }
    if !f.listed {
        return Action::GiveUp("the repo is no longer on the merge-lane list in Settings".into());
    }
    if j.running {
        if j.started_at.is_some_and(|t| f.now - t >= TIMEOUT) {
            return Action::TimeOut;
        }
        if !f.live {
            return if j.resumes < 1 { Action::Resume } else { Action::GiveUp("the merge lane ended twice during this merge".into()) };
        }
        return if f.free && f.pending { Action::Type } else { Action::Nothing };
    }
    if !f.live {
        return if f.starts >= MAX_STARTS { Action::GiveUp(format!("the merge lane didn't start ({} tries)", f.starts)) } else { Action::Start };
    }
    if !f.free {
        return Action::Nothing;
    }
    // A dropped job's note goes first, so the lane is clean for the next one.
    if f.pending {
        Action::Type
    } else {
        Action::Brief
    }
}

// ---- kanban_merge: a card's session asks for its merge ----

/// What `merge_refused` checks, read from the card and the board.
#[derive(Debug, Clone, PartialEq)]
pub struct Ask<'a> {
    pub column: &'a str,
    pub archived: bool,
    /// `kanban_store::ship_started` said its ship has started.
    pub ship_started: bool,
    pub listed: bool,
    pub merger_on: bool,
    pub branch: Option<&'a str>,
    pub worktree: Option<&'a str>,
    /// Its current `merge` state, if any.
    pub merge_state: Option<&'a str>,
}

/// Why a card's `kanban_merge` is refused, if it is. The uncommitted-work check needs git and
/// comes after.
pub fn merge_refused(a: &Ask) -> Option<String> {
    if a.archived {
        return Some("the card is archived".into());
    }
    if a.column != "Ship it" {
        return Some(format!("the card is in {}, not Ship it: only a ship asks for a merge", a.column));
    }
    if !a.ship_started {
        return Some("the card's ship hasn't started (it waits its turn in the ship queue): wait for the Ship it prompt".into());
    }
    if !a.listed {
        return Some("this card's repo doesn't ship through a merge lane (Settings › Sessions): use the card-merger subagent, as the Ship it prompt says".into());
    }
    if !a.merger_on {
        return Some("Hugh has the merger off for this card: no merge, by the lane or the card-merger".into());
    }
    if a.branch.is_none() {
        return Some("the card has no branch: set `branch` on the card first".into());
    }
    if a.worktree.is_none() {
        return Some("the card has no worktreePath: set it to the folder its branch is checked out in".into());
    }
    if let Some(st) = a.merge_state.filter(|s| kanban_store::MERGE_WAITING.contains(s)) {
        return Some(format!("a merge for this card is already {} in the merge lane: end your turn, the app sends you its verdict", if st == "running" { "running" } else { "waiting" }));
    }
    None
}

fn text_arg(args: &Value, k: &str) -> Result<Option<String>, String> {
    let v = match args.get(k) {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(t)) => t.trim().to_string(),
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n"),
        Some(_) => return Err(format!("`{k}` is text (or a list of lines)")),
    };
    if v.is_empty() {
        return Ok(None);
    }
    if v.chars().count() > TEXT_MAX {
        return Err(format!("`{k}` is longer than {TEXT_MAX} characters"));
    }
    if kanban_store::looks_secret(&v) || kanban_store::looks_like_token(&v) || kanban_store::mask_secrets(&v) != v {
        return Err(format!("`{k}` looks like it holds a token, key or password; secrets never go on the board"));
    }
    Ok(Some(v))
}

/// The other branches of the ship (`branches`): plain branch names.
fn branches_arg(args: &Value) -> Result<Vec<String>, String> {
    let Some(v) = args.get("branches").filter(|v| !v.is_null()) else { return Ok(vec![]) };
    let a = v.as_array().ok_or("`branches` is a list of branch names")?;
    let mut out = vec![];
    for b in a {
        let b = b.as_str().map(str::trim).ok_or("`branches` is a list of branch names")?;
        if !crate::check_size::branch_ok(b) {
            return Err(format!("\"{b}\" isn't a branch name"));
        }
        out.push(b.to_string());
    }
    Ok(out)
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// `kanban_merge`: the card's own session asks its repo's lane for the ship's merge. The board
/// is let go while git reads the card's worktree.
pub fn request(app: &AppHandle, caller: Option<&str>, args: &Value) -> Result<Value, String> {
    let branches = branches_arg(args)?;
    let rules = text_arg(args, "rules")?;
    let checks = text_arg(args, "checks")?;
    let board = app.state::<Board>();
    let (card_id, worktree) = {
        let store = board.0.lock().map_err(|e| e.to_string())?;
        let own = crate::tools::caller_card(app, &store, caller).ok_or("kanban_merge is for a card's own session, for its own card's ship")?;
        let card = store.get(&own).map_err(|e| e.to_string())?;
        if args.get("card").and_then(Value::as_i64).is_some_and(|n| n != card.number) {
            return Err(format!("kanban_merge asks for your own card's merge (#{}) only", card.number));
        }
        check(&store, &card)?;
        let wt = card.fields.get("worktreePath").and_then(Value::as_str).unwrap_or("").trim().to_string();
        (card.id, wt)
    };
    // Outside the board's lock: the card's worktree must be clean, and its HEAD is the base.
    let dir = Path::new(&worktree);
    let dirty = crate::worktree::git(dir, &["status", "--porcelain"]).map_err(|e| format!("refused: couldn't read the git status of {worktree}: {e}"))?;
    if !dirty.trim().is_empty() {
        let files: Vec<&str> = dirty.lines().take(5).collect();
        return Err(format!("refused: your worktree has work that isn't committed ({}). Commit it, then ask again", files.join(", ").trim()));
    }
    let base = crate::worktree::git(dir, &["rev-parse", "HEAD"]).map_err(|e| format!("refused: couldn't read HEAD in {worktree}: {e}"))?;
    let mut store = board.0.lock().map_err(|e| e.to_string())?;
    let card = store.get(&card_id).map_err(|e| e.to_string())?;
    // Read again: the card may have moved while git ran.
    let entry = check(&store, &card)?;
    let branch = card.fields.get("branch").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let key = lane_key(&entry.repo);
    let mut rec = json!({ "state": "queued", "lane": key, "base": base, "at": now(), "branch": branch, "worktree": worktree, "by": format!("card #{}'s session", card.number) });
    if !branches.is_empty() {
        rec["branches"] = json!(branches);
    }
    if let Some(r) = &rules {
        rec["rules"] = json!(r);
    }
    if let Some(c) = &checks {
        rec["checks"] = json!(c);
    }
    let mut patch = Patch::new();
    patch.insert(MERGE_KEY.into(), rec);
    store.update(&card.id, card.version, &patch, BY).map_err(|e| e.to_string())?;
    let _ = store.add_log(&card.id, &format!("Asked the merge lane for {}'s merge: {branch} at {}{}", entry.repo, short(&base), if branches.is_empty() { String::new() } else { format!(", with {}", branches.join(", ")) }), BY);
    drop(store);
    let _ = app.emit("cards-changed", ());
    Ok(json!(format!("The merge lane has it ({branch} at {}). End your turn: the app sends you its verdict.", short(&base))))
}

/// `merge_refused` for `card` as it is on the board now; its repo's list entry when allowed.
fn check(store: &Store, card: &Card) -> Result<MergeLaneRepo, String> {
    let text = |k: &str| card.fields.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
    let entry = text("repo").and_then(|r| store.merge_lane_repo_for(r));
    let ask = Ask {
        column: &card.column,
        archived: card.archived,
        ship_started: kanban_store::ship_started(card).is_some(),
        listed: entry.is_some(),
        merger_on: store.subagent_on(card, "merger"),
        branch: text("branch"),
        worktree: text("worktreePath"),
        merge_state: kanban_store::merge_state(card),
    };
    if let Some(why) = merge_refused(&ask) {
        return Err(format!("refused: {why}"));
    }
    entry.ok_or_else(|| "refused: no merge lane for this repo".into())
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

// ---- The messages ----

/// The brief the lane is typed for `job` on `card`: the card, its branch and base, the ship's
/// other branches, the rules, the checks, and the repo's last merges. A dry-run card's brief
/// starts with the dry-run line (`dispatch::dry`).
pub fn brief(card: &Card, job: &Job, history: &[Card], lane_worktree: &Path) -> String {
    let repo = card.fields.get("repo").and_then(Value::as_str).unwrap_or("");
    let mut t = format!("MERGE JOB for card #{}, \"{}\" ({repo}).\n", card.number, card.title());
    t.push_str(&format!("- Card: #{}. Branch: {}. Base (the branch's HEAD when the card asked): {}.\n", card.number, job.branch, job.base));
    t.push_str(&format!("- The card's own worktree is {}: read only for you, never work in it.\n", job.worktree));
    t.push_str(&format!(
        "- Other branches of the ship, rebased on top in this order: {}. A stack's cards ship on its branch: read kanban_stack_list for a card that leads one.\n",
        if job.branches.is_empty() { "none".to_string() } else { job.branches.join(", ") }
    ));
    t.push_str(&format!("- Merge rules: {}\n", job.rules.as_deref().unwrap_or("none given: use the repo's CLAUDE.md or AGENTS.md.")));
    t.push_str(&format!("- Checks: {}\n", job.checks.as_deref().unwrap_or("none given: the repo's own test command (README or CLAUDE.md).")));
    if history.is_empty() {
        t.push_str("- Earlier merges in this repo: none recorded yet.\n");
    } else {
        t.push_str("- Earlier merges in this repo, newest first:\n");
        for h in history {
            let m = h.fields.get(MERGE_KEY).cloned().unwrap_or(Value::Null);
            let state = m.get("state").and_then(Value::as_str).unwrap_or("?");
            let head = m.get("head").and_then(Value::as_str).map(|h| format!(" at {}", short(h))).unwrap_or_default();
            let detail = m.get("detail").and_then(Value::as_str).map(|d| format!(": {}", crate::transcript::cut_words(d, 240))).unwrap_or_default();
            t.push_str(&format!("  - #{} \"{}\": {state}{head}{detail}\n", h.number, crate::transcript::cut_words(h.title(), 80)));
        }
    }
    t.push_str(&format!(
        "Do the ship job as your rules say, in your own worktree {} only. Log each step on card #{} with kanban_add_log. End with one kanban_merge_result call for card #{}, then end your turn.",
        lane_worktree.display(),
        card.number,
        card.number
    ));
    crate::dispatch::dry(card, t)
}

/// The note typed to a lane resumed after it ended during card `n`'s merge.
pub fn resume_note(n: i64) -> String {
    format!("The app resumed you: you stopped during card #{n}'s merge. Look at your worktree first (git status, git log -3, whether a rebase is in progress). Then finish #{n}'s merge or start it again from step 3, and report with kanban_merge_result. If you can't finish it, report FAIL with the reason.")
}

/// The note typed to a lane whose job was dropped.
pub fn drop_note(n: i64, why: &str) -> String {
    format!("Drop card #{n}'s merge: {why}. Stop work on it, run git rebase --abort if a rebase is in progress, make no report for it, and wait for the next job.")
}

/// What the card's session is told when its merge has a verdict: on a PASS, the checks and the
/// reset that bring its branch to the lane's commit before step 1a's PASS; else needs-hugh.
pub fn verdict_message(n: i64, state: &str, detail: &str, base: &str, head: Option<&str>, onto: Option<&str>, lines: &[String], worktree: &str) -> String {
    let changed = if lines.is_empty() { String::new() } else { format!("\nWhat it did:\n{}", lines.iter().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n")) };
    match (state, head, onto) {
        ("passed", Some(head), Some(onto)) => format!(
            "The merge lane passed card #{n}'s merge: your branch's commits (base {base}) are rebased onto origin/main at {onto}, as commit {head}. Nothing is pushed, and your branch hasn't moved yet.{changed}\n\nNow, in your worktree {worktree}:\n1. Check that git status --porcelain is empty and git rev-parse HEAD is {base}. If not, your branch moved since you asked: report kanban_ship_result needs-hugh with what you found, and stop.\n2. Run git reset --hard {head}.\n3. Run git range-diff {onto} {base} {head}. Each of your own commits must be there, intact, apart from the conflict resolutions listed above. If one is missing or changed in another way, report needs-hugh with what range-diff shows, and stop.\n4. Then go on as step 1a of the Ship it prompt says after the merger's PASS (the push with --force-with-lease, and the rest of the ship). Don't run the card-merger subagent."
        ),
        _ => format!(
            "The merge lane's verdict on card #{n}'s merge: {} ({detail}). Your branch is as it was: the lane moves no branch.{changed}\n\nReport kanban_ship_result needs-hugh with that reason, and stop. Hugh decides what happens next.",
            if state == "failed" { "FAIL" } else { "unclear" }
        ),
    }
}

/// Card #160's note on the Ship it message for a card whose repo ships through its merge lane.
/// None for any other card, or one Hugh has the merger off on (`subagents_note` says so).
pub fn lane_note(store: &Store, card: &Card, column: &str) -> Option<String> {
    if column != "Ship it" || !store.subagent_on(card, "merger") {
        return None;
    }
    let repo = card.fields.get("repo").and_then(Value::as_str)?;
    let entry = store.merge_lane_repo_for(repo)?;
    Some(format!(
        "{} ships through its merge lane (card #160). In step 1a, don't run the card-merger subagent. Commit your work first, then call kanban_merge: with branches for a card Hugh dropped on this one to ship as one, and the repo's merge rules and check commands as you would have briefed the merger (rules, checks). Then end your turn. The lane merges in its own worktree and leaves your branch alone. The app sends you its verdict and every line of what it changed, and says what to do next.",
        entry.repo
    ))
}

// ---- Start, stop, status ----

/// A plan for a lane's session: new, or resuming `resume` (its conversation id).
pub fn plan(store: &Store, api: Option<crate::api::ApiInfo>, base: &Path, resume: Option<String>, cwd: &str) -> Plan {
    let (model, effort) = store.lane_model_effort();
    let row_id = uuid::Uuid::new_v4().to_string();
    Plan {
        session_id: resume.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        resume,
        // A resume needs the account it ran on.
        account: "main".into(),
        cwd: cwd.into(),
        model,
        effort,
        // As the Router: its guard refuses what it must never run.
        permission_mode: "bypassPermissions".into(),
        settings_path: base.join(&row_id).join("settings.json"),
        row_id,
        api,
        todo_tools: false,
        router: false,
        lane: true,
    }
}

/// Starts `entry`'s lane, or resumes its last conversation on this channel when its transcript
/// is here. Makes its worktree first (git, capped at 15 s, with no lock of the board held).
pub fn start(app: &AppHandle, entry: &MergeLaneRepo) -> Result<String, String> {
    let key = lane_key(&entry.repo);
    let live = app.state::<Arc<Sessions>>().inner().clone();
    if live.is_live(&key).is_some() {
        return Ok("running".into());
    }
    let path = worktree_of(entry);
    let made = crate::worktree::make_lane(Path::new(&entry.checkout), &path)?;
    let api = app.state::<crate::Api>().0.clone();
    let base = crate::board_path().parent().map(|p| p.join("sessions")).unwrap_or_default();
    let board = app.state::<Board>();
    let mut store = board.0.lock().map_err(|e| e.to_string())?;
    let last = store.latest_lane_session(&entry.repo, kanban_store::channel()).map_err(|e| e.to_string())?;
    let resume = last
        .as_ref()
        .filter(|r| Path::new(&r.cwd).is_dir() && crate::transcript::find(&r.account, &r.claude_session_id).is_some())
        .map(|r| (r.claude_session_id.clone(), r.cwd.clone()));
    let (resume_id, run_in) = match resume {
        Some((id, c)) => (Some(id), c),
        None => (None, path.display().to_string()),
    };
    let p = plan(&store, api, &base, resume_id.clone(), &run_in);
    let row = live.start_as(app, &key, 0, &p)?;
    store.add_lane_session(&row.id, &entry.repo, &row.claude_session_id, kanban_store::channel(), &p.account, &p.cwd, &p.model, &p.effort).map_err(|e| e.to_string())?;
    let how = if resume_id.is_some() { "Resumed the merge lane's conversation" } else { "Started a new merge lane" };
    let line = format!("{how} for {} ({} {}, in {}, {} at {})", entry.repo, p.model, p.effort, p.cwd, if made.how == "made" { "its worktree made" } else { "its worktree" }, made.head);
    if let Some(job) = store.lane_jobs(&key).ok().and_then(|j| j.into_iter().next()) {
        let _ = store.add_log(&job.id, &line, BY);
    }
    drop(store);
    let _ = app.emit("lanes-changed", ());
    let _ = app.emit("cards-changed", ());
    Ok(how.into())
}

/// A lane's state as Settings shows it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub repo: String,
    pub checkout: String,
    pub worktree: String,
    /// "off", "starting", "idle", "merging" or "down".
    pub state: String,
    /// The card it merges now, or waits to start for.
    pub card: Option<i64>,
    pub model: String,
    pub effort: String,
    pub live: bool,
}

/// The word for a lane's state: off (not running, nothing waiting), starting, idle, merging (a
/// job's brief is typed), or down (a job waits on a lane that ended, or the session failed).
pub fn state_word(live: bool, session_state: &str, ready: bool, job: Option<&Job>) -> &'static str {
    match (live, job) {
        (false, None) => "off",
        (false, Some(j)) if j.running => "down",
        (false, Some(_)) => "starting",
        (true, _) if session_state == "failed" => "down",
        (true, _) if session_state == "starting" || !ready => "starting",
        (true, Some(j)) if j.running => "merging",
        _ => "idle",
    }
}

pub fn status(app: &AppHandle) -> Vec<Status> {
    let live = app.state::<Arc<Sessions>>();
    let (list, jobs, (model, effort)) = {
        let board = app.state::<Board>();
        let Ok(store) = board.0.lock() else { return vec![] };
        (store.merge_lane_repos(), store.lane_jobs_all().unwrap_or_default(), store.lane_model_effort())
    };
    let starting: BTreeSet<String> = app.try_state::<Lanes>().and_then(|l| l.0.lock().ok().map(|m| m.iter().filter(|(_, v)| v.starting).map(|(k, _)| k.clone()).collect())).unwrap_or_default();
    list.iter()
        .map(|e| {
            let key = lane_key(&e.repo);
            let job = jobs.iter().filter_map(job_of).find(|j| j.lane == key);
            let w = live.watch_one(&key);
            let state = match &w {
                Some(w) => state_word(true, &w.state, w.ready, job.as_ref()),
                None if starting.contains(&key) => "starting",
                None => state_word(false, "", false, job.as_ref()),
            };
            Status {
                repo: e.repo.clone(),
                checkout: e.checkout.clone(),
                worktree: worktree_of(e).display().to_string(),
                state: state.into(),
                card: job.map(|j| j.number),
                model: model.clone(),
                effort: effort.clone(),
                live: w.is_some(),
            }
        })
        .collect()
}

/// Hugh's Stop of a lane. Its conversation is kept; a job it was on is resumed once.
pub fn stop(app: &AppHandle, repo: &str, by: &str) -> Result<Vec<Status>, String> {
    let key = lane_key(repo);
    let live = app.state::<Arc<Sessions>>().inner().clone();
    if live.is_live(&key).is_some() {
        if let Ok(mut store) = app.state::<Board>().0.lock() {
            if let Some(c) = store.lane_jobs(&key).ok().and_then(|j| j.into_iter().find(|c| kanban_store::merge_state(c) == Some("running"))) {
                let _ = store.add_log(&c.id, &format!("{by} stopped the merge lane during this merge: the app resumes it once"), BY);
            }
        }
        live.stop(app, &key)?;
    }
    let _ = app.emit("lanes-changed", ());
    Ok(status(app))
}

#[tauri::command]
pub fn lane_status(app: AppHandle) -> Vec<Status> {
    status(&app)
}

#[tauri::command]
pub fn lane_stop(app: AppHandle, repo: String) -> Result<Vec<Status>, String> {
    stop(&app, &repo, "Hugh (settings)")
}

// ---- Rows ----

/// The lane key of session row `row_id`, if it is a lane's. A live row answers from the session
/// map with no board lock; only a row not live is looked up in the store.
pub fn key_of_row(app: &AppHandle, row_id: &str) -> Option<String> {
    match app.try_state::<Arc<Sessions>>().and_then(|l| l.card_of_row(row_id)) {
        Some(key) => is_lane_key(&key).then_some(key),
        None => app.try_state::<Board>().and_then(|b| b.0.lock().ok().and_then(|s| s.lane_of_row(row_id).ok().flatten())).map(|r| lane_key(&r)),
    }
}

/// The card lane `key` merges now (its running job), if any.
pub fn running_job(store: &Store, key: &str) -> Option<Card> {
    store.lane_jobs(key).ok()?.into_iter().find(|c| kanban_store::merge_state(c) == Some("running"))
}

// ---- The verdict ----

/// `kanban_merge_result` from lane `key` (session `row_id`): checks it is for its running job,
/// checks a PASS's head is a commit on top of onto (git, with the board let go), records it as
/// the card-merger's report is recorded, and wakes the card's session with the verdict.
pub fn record_result(app: &AppHandle, key: &str, row_id: &str, args: &Value) -> Result<Value, String> {
    let n = args.get("card").and_then(Value::as_i64).ok_or("pass the card's number as `card`")?;
    let verdict = args.get("verdict").and_then(Value::as_str).map(|v| v.trim().to_ascii_uppercase()).ok_or("pass verdict: \"PASS\" or \"FAIL\"")?;
    let passed = match verdict.as_str() {
        "PASS" => true,
        "FAIL" => false,
        v => return Err(format!("verdict is \"PASS\" or \"FAIL\", not {v}")),
    };
    let head = args.get("head").and_then(Value::as_str).and_then(crate::merger::sha);
    let onto = args.get("onto").and_then(Value::as_str).and_then(crate::merger::sha);
    let origin = match (args.get("originBefore").and_then(Value::as_str).and_then(crate::merger::sha_or_none), args.get("originAfter").and_then(Value::as_str).and_then(crate::merger::sha_or_none)) {
        (Some(a), Some(b)) => Some((a, b)),
        _ => None,
    };
    let lines: Vec<String> = match args.get("lines") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(|l| l.trim().trim_start_matches("- ").to_string()).filter(|l| !l.is_empty()).collect(),
        Some(Value::String(t)) => t.lines().map(|l| l.trim().trim_start_matches("- ").to_string()).filter(|l| !l.is_empty()).collect(),
        _ => vec![],
    };
    let board = app.state::<Board>();
    let worktree = {
        let store = board.0.lock().map_err(|e| e.to_string())?;
        let job = running_job(&store, key).ok_or("you have no merge job now: wait for the next MERGE JOB message")?;
        if job.number != n {
            return Err(format!("your job now is card #{}, not #{n}", job.number));
        }
        app.state::<Arc<Sessions>>().is_live(key).map(|r| r.cwd).unwrap_or_default()
    };
    // A PASS needs a real commit on top of onto: checked here, holding no lock.
    let problem = match (passed, &head, &onto) {
        (true, Some(h), Some(o)) => match crate::worktree::on_top_of(Path::new(&worktree), o, h) {
            Ok(true) => None,
            Ok(false) => Some(format!("the merge lane said PASS, but {} isn't on top of origin/main's {}", short(h), short(o))),
            Err(e) => Some(format!("the merge lane said PASS, but the app couldn't check its commit: {e}")),
        },
        (true, _, None) => Some("the merge lane said PASS with no onto SHA, so nobody can tell what it rebased onto".to_string()),
        _ => None,
    };
    let report = crate::merger::Report { card: Some(n), passed: Some(passed), head: head.clone(), origin, lines: lines.clone() };
    let at = now();
    let (mut rec, line) = crate::merger::record_of(&report, None, row_id, &at);
    // The card-merger's words, said of the lane.
    let mut line = line.replacen("Merger", "Merge lane", 1).replace("the merger ", "the merge lane ");
    if let Some(d) = rec["detail"].as_str().map(|d| d.replace("the merger ", "the merge lane ")) {
        rec["detail"] = json!(d);
    }
    if let (Some(p), "passed") = (&problem, rec["state"].as_str().unwrap_or("")) {
        rec["state"] = json!("unclear");
        rec["detail"] = json!(p);
        line = format!("Merge lane unclear: {p}");
    }
    if let Some(o) = &onto {
        rec["onto"] = json!(o);
    }
    let moved = report.origin.as_ref().is_some_and(|(a, b)| a != b);
    let mut store = board.0.lock().map_err(|e| e.to_string())?;
    let card = store.get_by_number(n).map_err(|e| e.to_string())?;
    // Read again: the card may have left Ship it (and its job been cancelled) meanwhile.
    let job = job_of(&card).filter(|j| j.running && j.lane == key).ok_or_else(|| format!("card #{n}'s merge isn't running in this lane any more (it was cancelled or timed out): drop it"))?;
    finish(app, &mut store, &card, &job, rec, &line, &lines, moved);
    Ok(json!(format!("Recorded card #{n}'s merge as {}. The card's session takes it from here. Wait for the next job.", line)))
}

/// Writes a verdict on `card` for its lane `job`, logs it, wakes the card's session with what to
/// do, and tells Hugh when origin moved or the merge is unclear.
#[allow(clippy::too_many_arguments)]
fn finish(app: &AppHandle, store: &mut Store, card: &Card, job: &Job, mut rec: Value, line: &str, lines: &[String], tell: bool) {
    rec["lane"] = json!(job.lane);
    rec["base"] = json!(job.base);
    rec["branch"] = json!(job.branch);
    if let Some(t) = job.started_at {
        rec["startedAt"] = json!(t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    }
    rec["by"] = json!(BY);
    let mut patch = Patch::new();
    patch.insert(MERGE_KEY.into(), rec.clone());
    if let Err(e) = store.update(&card.id, card.version, &patch, BY) {
        let _ = store.add_log(&card.id, &format!("Couldn't record the merge lane's verdict: {e}"), BY);
        return;
    }
    let _ = store.add_log(&card.id, line, BY);
    let state = rec["state"].as_str().unwrap_or("unclear").to_string();
    let detail = rec["detail"].as_str().unwrap_or("").to_string();
    let text = verdict_message(card.number, &state, &detail, &job.base, rec["head"].as_str(), rec["onto"].as_str(), lines, &job.worktree);
    if let Ok(fresh) = store.get(&card.id) {
        crate::stackrun::wake(app, store, &fresh, &text);
    }
    if tell || state == "unclear" {
        let _ = app.emit("notify", json!({ "card": card.id, "number": card.number, "text": line }));
    }
    let _ = app.emit("cards-changed", ());
    let _ = app.emit("lanes-changed", ());
}

/// An unclear verdict the app gives itself (a time-out, a lane that can't go on).
fn give_up(app: &AppHandle, key: &str, number: i64, why: &str) {
    let board = app.state::<Board>();
    let Ok(mut store) = board.0.lock() else { return };
    let Ok(card) = store.get_by_number(number) else { return };
    let Some(job) = job_of(&card).filter(|j| j.lane == key) else { return };
    let detail: String = kanban_store::mask_secrets(why).chars().take(600).collect();
    let rec = json!({ "state": "unclear", "at": now(), "detail": detail });
    finish(app, &mut store, &card, &job, rec, &format!("Merge lane unclear: {detail}"), &[], true);
}

// ---- The tick ----

/// Every dispatcher pass with the ship queue (every 2 s, from `dispatch::run`): for each lane,
/// the facts and then `decide`'s action. One cheap count when no merge waits and no lane runs.
pub fn tick(app: &AppHandle) {
    let Some(lanes) = app.try_state::<Lanes>() else { return };
    let live = app.state::<Arc<Sessions>>().inner().clone();
    let watches: HashMap<String, crate::sessions::Watch> = live.watch().into_iter().filter(|w| is_lane_key(&w.card_id)).map(|w| (w.card_id.clone(), w)).collect();
    let mem_keys: Vec<String> = lanes.0.lock().map(|m| m.iter().filter(|(_, v)| v.starting || !v.pending.is_empty()).map(|(k, _)| k.clone()).collect()).unwrap_or_default();
    let (list, jobs) = {
        let board = app.state::<Board>();
        let Ok(store) = board.0.lock() else { return };
        let waiting = store.lane_jobs_waiting().unwrap_or(0);
        if waiting == 0 && watches.is_empty() && mem_keys.is_empty() {
            return;
        }
        (store.merge_lane_repos(), if waiting > 0 { store.lane_jobs_all().unwrap_or_default() } else { vec![] })
    };
    let free: BTreeSet<String> = if watches.is_empty() { BTreeSet::new() } else { live.free().into_iter().filter(|k| is_lane_key(k)).collect() };
    let jobs: Vec<Job> = jobs.iter().filter_map(job_of).collect();
    let mut keys: BTreeSet<String> = list.iter().map(|e| lane_key(&e.repo)).filter(|k| jobs.iter().any(|j| &j.lane == k)).collect();
    keys.extend(watches.keys().cloned());
    keys.extend(mem_keys);
    keys.extend(jobs.iter().map(|j| j.lane.clone()).filter(|k| is_lane_key(k)));
    let now = chrono::Utc::now();
    for key in keys {
        let entry = list.iter().find(|e| lane_key(&e.repo) == key).cloned();
        let job = jobs.iter().find(|j| j.lane == key).cloned();
        let (starting, pending, starts) = lanes.0.lock().map(|m| m.get(&key).map(|v| (v.starting, !v.pending.is_empty(), v.starts)).unwrap_or_default()).unwrap_or_default();
        let w = watches.get(&key);
        let facts = Facts {
            listed: entry.is_some(),
            live: w.is_some() || starting,
            free: free.contains(&key) && !starting,
            idle_for: w.filter(|w| w.state == "idle").map(|w| w.in_state),
            job: job.clone(),
            pending,
            starts,
            now,
        };
        act(app, &lanes, &key, decide(&facts), job.as_ref(), entry.as_ref());
    }
}

fn mem<R>(lanes: &Lanes, key: &str, f: impl FnOnce(&mut Mem) -> R) -> Option<R> {
    lanes.0.lock().ok().map(|mut m| f(m.entry(key.to_string()).or_default()))
}

fn act(app: &AppHandle, lanes: &Lanes, key: &str, action: Action, job: Option<&Job>, entry: Option<&MergeLaneRepo>) {
    match (action, job) {
        (Action::Nothing, _) => {}
        (Action::Cancel(why), Some(j)) => {
            {
                let board = app.state::<Board>();
                let Ok(mut store) = board.0.lock() else { return };
                let Ok(card) = store.get_by_number(j.number) else { return };
                let mut rec = card.fields.get(MERGE_KEY).cloned().unwrap_or(json!({}));
                rec["state"] = json!("cancelled");
                rec["detail"] = json!(why);
                rec["at"] = json!(now());
                let mut patch = Patch::new();
                patch.insert(MERGE_KEY.into(), rec);
                if store.update(&card.id, card.version, &patch, BY).is_ok() {
                    let _ = store.add_log(&card.id, &format!("Merge lane: cancelled this card's merge: {why}"), BY);
                }
            }
            if j.running {
                mem(lanes, key, |m| m.pending.push_back(drop_note(j.number, &why)));
            }
            mem(lanes, key, |m| m.starts = 0);
            let _ = app.emit("cards-changed", ());
            let _ = app.emit("lanes-changed", ());
        }
        (a @ (Action::Start | Action::Resume), Some(j)) => {
            let Some(entry) = entry.cloned() else { return };
            if a == Action::Resume {
                let board = app.state::<Board>();
                let Ok(mut store) = board.0.lock() else { return };
                let Ok(card) = store.get_by_number(j.number) else { return };
                let mut rec = card.fields.get(MERGE_KEY).cloned().unwrap_or(json!({}));
                rec["resumes"] = json!(j.resumes + 1);
                let mut patch = Patch::new();
                patch.insert(MERGE_KEY.into(), rec);
                if store.update(&card.id, card.version, &patch, BY).is_err() {
                    return;
                }
                let _ = store.add_log(&card.id, "The merge lane ended during this merge (a crash, a Stop or an app restart): the app resumes it once", BY);
                // The job again with the note, for a lane whose conversation couldn't be resumed.
                let repo = card.fields.get("repo").and_then(Value::as_str).unwrap_or("").to_string();
                let history = store.recent_merges(&repo, HISTORY, Some(&card.id)).unwrap_or_default();
                let job_text = brief(&card, j, &history, &worktree_of(&entry));
                drop(store);
                mem(lanes, key, |m| m.pending.push_back(format!("{}\n\nThe job, as it was given:\n{job_text}", resume_note(j.number))));
            }
            mem(lanes, key, |m| {
                m.starting = true;
                m.starts += 1;
            });
            let (a, k, n) = (app.clone(), key.to_string(), j.number);
            std::thread::spawn(move || {
                let res = start(&a, &entry);
                if let Some(l) = a.try_state::<Lanes>() {
                    mem(&l, &k, |m| m.starting = false);
                }
                if let Err(e) = res {
                    if let Ok(mut store) = a.state::<Board>().0.lock() {
                        if let Ok(c) = store.get_by_number(n) {
                            let _ = store.add_log(&c.id, &format!("Couldn't start the merge lane: {e}"), BY);
                        }
                    }
                    let _ = a.emit("cards-changed", ());
                }
            });
        }
        (Action::Brief, Some(j)) => {
            let live = app.state::<Arc<Sessions>>().inner().clone();
            let lane_wt = live.is_live(key).map(|r| PathBuf::from(r.cwd)).unwrap_or_default();
            let text = {
                let board = app.state::<Board>();
                let Ok(store) = board.0.lock() else { return };
                let Ok(card) = store.get_by_number(j.number) else { return };
                let repo = card.fields.get("repo").and_then(Value::as_str).unwrap_or("").to_string();
                let history = store.recent_merges(&repo, HISTORY, Some(&card.id)).unwrap_or_default();
                brief(&card, j, &history, &lane_wt)
            };
            if live.send_message(key, &text).is_err() {
                return;
            }
            let board = app.state::<Board>();
            let Ok(mut store) = board.0.lock() else { return };
            let _ = store.note_app_message(key, &text);
            if let Ok(card) = store.get_by_number(j.number) {
                let mut rec = card.fields.get(MERGE_KEY).cloned().unwrap_or(json!({}));
                rec["state"] = json!("running");
                rec["startedAt"] = json!(now());
                let mut patch = Patch::new();
                patch.insert(MERGE_KEY.into(), rec);
                if store.update(&card.id, card.version, &patch, BY).is_ok() {
                    let _ = store.add_log(&card.id, "Merge lane: briefed it with this card's merge", BY);
                }
            }
            drop(store);
            mem(lanes, key, |m| m.starts = 0);
            let _ = app.emit("cards-changed", ());
            let _ = app.emit("lanes-changed", ());
        }
        (Action::Type, _) => {
            let Some(text) = mem(lanes, key, |m| m.pending.pop_front()).flatten() else { return };
            let live = app.state::<Arc<Sessions>>().inner().clone();
            if live.send_message(key, &text).is_err() {
                mem(lanes, key, |m| m.pending.push_front(text));
            } else if let Ok(mut store) = app.state::<Board>().0.lock() {
                let _ = store.note_app_message(key, &text);
            }
        }
        (Action::TimeOut, Some(j)) => {
            let why = format!("the merge lane didn't report in {} min", TIMEOUT.num_minutes());
            give_up(app, key, j.number, &why);
            mem(lanes, key, |m| m.pending.push_back(drop_note(j.number, &why)));
        }
        (Action::GiveUp(why), Some(j)) => {
            give_up(app, key, j.number, &why);
            mem(lanes, key, |m| {
                m.starts = 0;
                m.pending.push_back(drop_note(j.number, &why));
            });
        }
        (Action::Stop, _) => {
            let live = app.state::<Arc<Sessions>>().inner().clone();
            let _ = live.stop(app, key);
            mem(lanes, key, |m| m.pending.clear());
            let _ = app.emit("lanes-changed", ());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(running: bool) -> Job {
        Job {
            number: 7,
            running,
            lane: "lane:acme/web".into(),
            base: "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b".into(),
            branch: "card-7".into(),
            worktree: "C:/co/.claude/worktrees/card-7".into(),
            branches: vec![],
            rules: None,
            checks: None,
            started_at: None,
            resumes: 0,
            in_ship_it: true,
        }
    }

    fn facts(j: Option<Job>) -> Facts {
        Facts { listed: true, live: false, free: false, idle_for: None, job: j, pending: false, starts: 0, now: chrono::Utc::now() }
    }

    #[test]
    fn a_waiting_job_starts_the_lane_then_is_briefed_when_it_is_free() {
        let f = facts(Some(job(false)));
        assert_eq!(decide(&f), Action::Start);
        assert_eq!(decide(&Facts { live: true, ..f.clone() }), Action::Nothing, "starting: wait");
        assert_eq!(decide(&Facts { live: true, free: true, ..f.clone() }), Action::Brief);
        // A dropped job's note goes before the next brief.
        assert_eq!(decide(&Facts { live: true, free: true, pending: true, ..f.clone() }), Action::Type);
        // A lane that never comes up: given up after its tries.
        assert_eq!(decide(&Facts { starts: 1, ..f.clone() }), Action::Start);
        assert!(matches!(decide(&Facts { starts: MAX_STARTS, ..f.clone() }), Action::GiveUp(w) if w.contains("didn't start")));
    }

    #[test]
    fn a_running_job_times_out_after_60_minutes_and_a_lane_that_ends_is_resumed_once() {
        let now = chrono::Utc::now();
        let mut j = job(true);
        j.started_at = Some(now - chrono::Duration::minutes(59));
        let f = Facts { live: true, ..facts(Some(j.clone())) };
        assert_eq!(decide(&f), Action::Nothing, "working on it");
        assert_eq!(decide(&Facts { free: true, pending: true, ..f.clone() }), Action::Type, "a resume note");
        j.started_at = Some(now - chrono::Duration::minutes(60));
        assert_eq!(decide(&Facts { job: Some(j.clone()), ..f.clone() }), Action::TimeOut);
        j.started_at = Some(now - chrono::Duration::minutes(5));
        assert_eq!(decide(&Facts { live: false, job: Some(j.clone()), ..f.clone() }), Action::Resume);
        j.resumes = 1;
        assert!(matches!(decide(&Facts { live: false, job: Some(j.clone()), ..f.clone() }), Action::GiveUp(w) if w.contains("ended twice")));
    }

    #[test]
    fn a_card_that_leaves_ship_it_cancels_its_job_and_an_unlisted_repo_gives_up() {
        for running in [false, true] {
            let mut j = job(running);
            j.in_ship_it = false;
            assert!(matches!(decide(&Facts { live: true, free: true, ..facts(Some(j)) }), Action::Cancel(w) if w.contains("left Ship it")));
        }
        assert!(matches!(decide(&Facts { listed: false, ..facts(Some(job(false))) }), Action::GiveUp(w) if w.contains("no longer on the merge-lane list")));
    }

    #[test]
    fn an_idle_lane_with_no_job_stops_after_15_minutes() {
        let f = Facts { live: true, free: true, idle_for: Some(Duration::from_secs(14 * 60)), ..facts(None) };
        assert_eq!(decide(&f), Action::Nothing);
        assert_eq!(decide(&Facts { idle_for: Some(IDLE_STOP), ..f.clone() }), Action::Stop);
        assert_eq!(decide(&Facts { pending: true, idle_for: Some(IDLE_STOP), ..f.clone() }), Action::Type, "its note first");
        assert_eq!(decide(&Facts { free: false, idle_for: Some(IDLE_STOP), ..f.clone() }), Action::Nothing);
        assert_eq!(decide(&facts(None)), Action::Nothing, "off and nothing waits");
        // A job keeps it running, however long it waits.
        assert_eq!(decide(&Facts { job: Some(job(true)), idle_for: Some(IDLE_STOP * 4), ..f }), Action::Nothing);
    }

    fn ask<'a>() -> Ask<'a> {
        Ask { column: "Ship it", archived: false, ship_started: true, listed: true, merger_on: true, branch: Some("card-7"), worktree: Some("C:/w"), merge_state: None }
    }

    #[test]
    fn kanban_merge_is_refused_saying_why() {
        assert_eq!(merge_refused(&ask()), None);
        for (a, says) in [
            (Ask { column: "Human test", ..ask() }, "not Ship it"),
            (Ask { archived: true, ..ask() }, "archived"),
            (Ask { ship_started: false, ..ask() }, "hasn't started"),
            (Ask { listed: false, ..ask() }, "doesn't ship through a merge lane"),
            (Ask { merger_on: false, ..ask() }, "merger off"),
            (Ask { branch: None, ..ask() }, "no branch"),
            (Ask { worktree: None, ..ask() }, "no worktreePath"),
            (Ask { merge_state: Some("queued"), ..ask() }, "already waiting"),
            (Ask { merge_state: Some("running"), ..ask() }, "already running"),
        ] {
            let why = merge_refused(&a).unwrap_or_else(|| panic!("{a:?} isn't refused"));
            assert!(why.contains(says), "{why}");
        }
        // A finished or cancelled merge doesn't stop a new one.
        for st in ["passed", "failed", "unclear", "cancelled"] {
            assert_eq!(merge_refused(&Ask { merge_state: Some(st), ..ask() }), None, "{st}");
        }
    }

    #[test]
    fn the_rules_checks_and_branches_are_checked() {
        assert_eq!(text_arg(&json!({}), "rules").unwrap(), None);
        assert_eq!(text_arg(&json!({ "rules": ["keep both changelog sides", " ", "version +1"] }), "rules").unwrap().as_deref(), Some("keep both changelog sides\nversion +1"));
        assert!(text_arg(&json!({ "checks": "curl -H 'Authorization: Bearer abc.DEF-123_x'" }), "checks").is_err());
        assert!(text_arg(&json!({ "rules": "x".repeat(TEXT_MAX + 1) }), "rules").is_err());
        assert!(text_arg(&json!({ "rules": 5 }), "rules").is_err());
        assert_eq!(branches_arg(&json!({ "branches": ["card-9", "drop-2"] })).unwrap(), vec!["card-9", "drop-2"]);
        assert!(branches_arg(&json!({ "branches": ["--force"] })).is_err());
        assert!(branches_arg(&json!({ "branches": "card-9" })).is_err());
        assert!(branches_arg(&json!({})).unwrap().is_empty());
    }

    fn card(n: i64, fields: Value) -> Card {
        Card { id: format!("card-{n}"), number: n, column: "Ship it".into(), order: None, order_col: None, archived: false, fields: fields.as_object().cloned().unwrap(), version: 3 }
    }

    #[test]
    fn the_brief_names_the_job_the_history_and_the_dry_run() {
        let c = card(7, json!({ "title": "Add a thing", "repo": "acme/web" }));
        let mut j = job(false);
        j.branches = vec!["drop-9".into()];
        j.rules = Some("keep both sides of Changelog.md".into());
        let h = vec![card(5, json!({ "title": "Earlier", "merge": { "state": "passed", "head": "abcdef1234567", "detail": "rebased card-5; Changelog.md: kept both" } }))];
        let b = brief(&c, &j, &h, Path::new("C:/co/.claude/worktrees/merge-lane"));
        for must in ["MERGE JOB for card #7", "Add a thing", "card-7", &j.base, "drop-9", "keep both sides of Changelog.md", "#5 \"Earlier\": passed at abcdef1: rebased card-5", "kanban_merge_result", "merge-lane", "read only for you"] {
            assert!(b.contains(must), "the brief doesn't say {must}:\n{b}");
        }
        assert!(!b.starts_with("Dry run"));
        let none = brief(&c, &job(false), &[], Path::new("C:/x"));
        assert!(none.contains("none recorded yet") && none.contains("none given"));
        let dry = brief(&card(7, json!({ "title": "T", "dryRun": true })), &job(false), &[], Path::new("C:/x"));
        assert!(dry.starts_with("Dry run: say what you would do and change nothing."));
    }

    #[test]
    fn the_verdict_message_tells_the_session_the_pass_steps_or_needs_hugh() {
        let base = "1111111111111111111111111111111111111111";
        let head = "2222222222222222222222222222222222222222";
        let onto = "3333333333333333333333333333333333333333";
        let p = verdict_message(7, "passed", "rebased", base, Some(head), Some(onto), &["rebased card-7 onto origin/main".into()], "C:/w");
        for must in ["git status --porcelain", &format!("git rev-parse HEAD is {base}"), &format!("git reset --hard {head}"), &format!("git range-diff {onto} {base} {head}"), "needs-hugh", "step 1a", "--force-with-lease", "- rebased card-7 onto origin/main", "C:/w"] {
            assert!(p.contains(must), "{must}:\n{p}");
        }
        for st in ["failed", "unclear"] {
            let m = verdict_message(7, st, "conflict in app.txt", base, None, None, &[], "C:/w");
            assert!(m.contains("needs-hugh") && m.contains("conflict in app.txt") && !m.contains("reset --hard"), "{m}");
        }
        // A PASS without the SHAs to reset to is no PASS to act on.
        assert!(verdict_message(7, "passed", "x", base, None, Some(onto), &[], "C:/w").contains("needs-hugh"));
    }

    #[test]
    fn the_state_word_says_off_starting_idle_merging_or_down() {
        assert_eq!(state_word(false, "", false, None), "off");
        assert_eq!(state_word(false, "", false, Some(&job(false))), "starting");
        assert_eq!(state_word(false, "", false, Some(&job(true))), "down");
        assert_eq!(state_word(true, "starting", false, Some(&job(false))), "starting");
        assert_eq!(state_word(true, "idle", true, None), "idle");
        assert_eq!(state_word(true, "idle", true, Some(&job(false))), "idle");
        assert_eq!(state_word(true, "working", true, Some(&job(true))), "merging");
        assert_eq!(state_word(true, "failed", true, Some(&job(true))), "down");
    }

    #[test]
    fn a_job_is_read_from_the_merge_record() {
        let c = card(7, json!({ "merge": { "state": "running", "lane": "lane:acme/web", "base": "abc1234", "branch": "card-7", "worktree": "C:/w", "branches": ["d-1"], "startedAt": "2026-11-09T10:00:00.000Z", "resumes": 1 } }));
        let j = job_of(&c).unwrap();
        assert!(j.running && j.in_ship_it);
        assert_eq!((j.lane.as_str(), j.base.as_str(), j.branches.clone(), j.resumes), ("lane:acme/web", "abc1234", vec!["d-1".to_string()], 1));
        assert!(j.started_at.is_some());
        assert!(job_of(&card(7, json!({ "merge": { "state": "passed" } }))).is_none());
        assert!(job_of(&card(7, json!({}))).is_none());
        let mut gone = card(7, json!({ "merge": { "state": "queued", "lane": "lane:a/b" } }));
        gone.column = "In progress".into();
        assert!(!job_of(&gone).unwrap().in_ship_it);
    }

    /// The lane's verdict is recorded by `merger::record_of`, as the card-merger's: a PASS with
    /// an origin that moved is unclear.
    #[test]
    fn the_verdict_reuses_the_mergers_record() {
        let r = crate::merger::Report { card: Some(7), passed: Some(true), head: Some("2222222".into()), origin: Some(("none".into(), "none".into())), lines: vec!["rebased".into()] };
        let (rec, line) = crate::merger::record_of(&r, None, "row-1", "t");
        assert_eq!(rec["state"], "passed");
        assert_eq!(line.replacen("Merger", "Merge lane", 1), "Merge lane: PASS at 2222222: rebased");
        let moved = crate::merger::Report { origin: Some(("1111111".into(), "3333333".into())), ..r };
        assert_eq!(crate::merger::record_of(&moved, None, "row-1", "t").0["state"], "unclear");
    }

    #[test]
    fn the_rules_say_what_the_lane_must_and_must_not_do() {
        for must in ["kanban_merge_result", "Never push", "detached", "git checkout --detach", "git rebase --abort", "(card #<n>)", "plus one", "kanban_add_log", "git branch -f", "update-ref", "Dry run", "drop a job", "stopped during a job", "cargo test --workspace", "onto", "originBefore"] {
            assert!(PROMPT.contains(must), "merge-lane.md doesn't say {must}");
        }
        assert!(PROMPT.len() < 9000, "the system prompt is {} characters", PROMPT.len());
    }

    #[test]
    fn the_lane_note_is_for_a_listed_repo_in_ship_it_with_the_merger_on() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("board.db")).unwrap();
        let co = dir.path().join("co");
        std::fs::create_dir_all(co.join(".git")).unwrap();
        let mut p = Patch::new();
        p.insert("title".into(), json!("T"));
        p.insert("repo".into(), json!("acme/web"));
        p.insert("column".into(), json!("Ship it"));
        let c = s.create(None, p, "Hugh").unwrap();
        assert_eq!(lane_note(&s, &c, "Ship it"), None, "not listed");
        s.set_page_setting(kanban_store::MERGE_LANE_REPOS_KEY, &json!([{ "repo": "acme/web", "checkout": co.display().to_string() }]).to_string()).unwrap();
        let n = lane_note(&s, &c, "Ship it").expect("a note for a listed repo");
        assert!(n.contains("kanban_merge") && n.contains("don't run the card-merger") && n.contains("end your turn"), "{n}");
        assert_eq!(lane_note(&s, &c, "In progress"), None, "only Ship it");
        let mut off = Patch::new();
        off.insert(kanban_store::SUBAGENTS_KEY.into(), json!({ "merger": false }));
        let c = s.update(&c.id, c.version, &off, "Hugh (board)").unwrap();
        assert_eq!(lane_note(&s, &c, "Ship it"), None, "the merger is off: no lane either");
    }

    #[test]
    fn a_lane_plan_is_the_main_account_bypass_and_the_lane_kind() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("board.db")).unwrap();
        let p = plan(&s, None, Path::new("C:/s"), Some("c1".into()), "C:/co/.claude/worktrees/merge-lane");
        assert!(p.lane && !p.router && !p.todo_tools);
        assert_eq!((p.account.as_str(), p.permission_mode.as_str(), p.model.as_str(), p.effort.as_str()), ("main", "bypassPermissions", "claude-opus-5-5", "high"));
        assert_eq!(p.resume.as_deref(), Some("c1"));
        assert_eq!(p.session_id, "c1");
        assert!(p.settings_path.ends_with("settings.json"));
    }
}
