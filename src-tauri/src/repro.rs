//! The card-reproducer (Hugh, card #112): proof of a reported bug, and later of its fix, so a
//! card comes back to Hugh with evidence instead of another round of testing by hand.
//!
//! A bug report on a card (Hugh's "Found a problem", "Report a bug" or, with card #102, Fail in
//! Human test, or a card-tester FAIL with a "- BUG:" line) goes through `mark_needed`, which sets the card's
//! `repro` to needed. The session then runs the `card-reproducer` subagent before it fixes
//! anything; the app reads its report when it finishes (`on_stop`) and keeps its steps and a
//! copy of its screenshot as the "before". After the fix the session runs it again; that run's
//! steps and shot are the "after". A card session's move to Human test waits (`gate`) until the
//! latest verdict is FIXED or NOT REPRODUCED, while Hugh's rule is on.
//!
//! One open cycle per card: a report that arrives while one is open joins it, and the record
//! starts again from needed (`needed_record`), so the next first run covers every open report.
//! A proof already made moves to `earlier` and stays on the card. A card-reviewer's BUG line
//! isn't a report: the reviewer reads the diff before it is installed, so there is nothing yet to
//! reproduce, and the session fixes it in the review loop; the tester's BUG lines come from the
//! installed app.

use crate::agents::{target_card, tool_calls, Agent, OnStop};
use kanban_store::{mask_secrets, repro_open, Card, Patch, Store, REPRO_KEY};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};

/// The subagent's name, as the session calls it (`subagent_type`) and the hook reports it.
pub const REPRODUCER: &str = "card-reproducer";
/// Who its records are made as.
pub const BY: &str = "the app (card-reproducer subagent)";
// The card field is `repro` (`kanban_store::REPRO_KEY`): `{state, at, agentId, detail,
// reports, test, before, after, earlier}`. State is "needed", "reproduced", "not-reproduced",
// "fixed" or "unclear".
/// The most bug reports one cycle keeps, the earlier rounds' proofs it keeps, and the longest
/// note, steps and detail.
const MAX_REPORTS: usize = 20;
const MAX_EARLIER: usize = 5;
const MAX_NOTE: usize = 600;
const MAX_STEPS: usize = 4000;

const DESCRIPTION: &str = "Proves a reported bug on one card on Hugh's Claude Kanban DSH board, and later proves its fix. Run it when the card's repro is needed: once before you fix anything, and again after the fix is in the test app. Brief it with the card number, where the change is, and for the second run the fix commit and the test you added. It changes no code and ends with CARD and VERDICT lines that the app records on the card.";

const PROMPT: &str = "You are card-reproducer. You prove a reported bug on one card on Hugh's Claude Kanban DSH board. After the fix, you prove that the bug is gone. The session that builds the card briefs you. Do not trust what the brief says. Find out for yourself.

1. Read the card. Use kanban_get_card with the card number from the brief. Its repro field has the bug reports (reports) and the steps of the first run (before).
2. Find which run this is. If repro has no before, this is the first run. If repro has a before, this is the second run, after the fix.
3. Use the real thing: the app, the page or the command where the bug shows. Reading the code is not enough.
   - For the Claude Kanban DSH app, use the test app (Claude Kanban DSH Test). Drive it with node tools/cdp.mjs and CDP_PORT=9334, as AGENTS.md (Driving the UI) says. If port 9334 does not answer, restart only the test app with the DevTools port. Always use --background. Never use port 9333. Never quit, restart or install the live app.
   - Use dry-run cards on the test board. Archive each card that you make.
   - For another repo, use the card's dev stack in a headless browser.
4. First run:
   a. Reproduce each report. Write the steps that show the bug.
   b. Save a screenshot that shows the bug (node tools/cdp.mjs shot <full path>).
   c. Write a test that fails because of the bug: the file it goes in, its name and its code. Put it in your report. Do not add it yourself.
5. Second run:
   a. Make sure that the app you test has the fix. For the test app, get its build commit: node tools/cdp.mjs eval \"window.__TAURI__.core.invoke('update_status').then(s=>s.running.commit)\". Then run git merge-base --is-ancestor <the fix commit from the brief> <the build commit>. For a dev stack, use the HEAD of its checkout. If the app does not have the fix, stop. End with VERDICT: UNCLEAR and the line \"- the test app isn't on the fix (<the build commit>)\".
   b. Do the steps of the first run again (repro.before.steps). Save a screenshot.
   c. Run the test that the session added. The brief gives its name.
   d. Say FIXED only if the bug is gone and the test passes. Else say REPRODUCED.
6. Change nothing. Do not edit, commit, push, merge or deploy. Do not change or move a card on Hugh's board. Do not run kb or claude config.

End your reply with these lines, and nothing after them:
CARD: #<the card's number>
VERDICT: REPRODUCED, NOT REPRODUCED, FIXED or UNCLEAR (one of them)
- one line for each thing to know: what you saw; for NOT REPRODUCED, what you tried
STEPS:
1. <each step, in order>
SHOT: <the screenshot's full path, or none>
TEST: <the test's name>
The app reads these lines and records them on the card.";

pub const AGENT: Agent = Agent {
    name: REPRODUCER,
    description: DESCRIPTION,
    prompt: PROMPT,
    // The router's call (card #112): the reproducer runs at the session's own effort.
    effort: None,
    edits: false,
    also_denied: &[],
    on_stop: OnStop::Custom(on_stop),
};

/// The reproducer's verdict word.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Word {
    Reproduced,
    NotReproduced,
    Fixed,
    Unclear,
}

/// What a reproducer's report says, from its closing lines.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Report {
    /// The card it names (`CARD: #12`), if any.
    pub card: Option<i64>,
    /// None when there is no VERDICT line with one of the four words.
    pub verdict: Option<Word>,
    /// The "- " lines after the verdict.
    pub notes: Vec<String>,
    /// The numbered lines after `STEPS:`, one per line.
    pub steps: Vec<String>,
    pub shot: Option<String>,
    pub test: Option<String>,
}

fn word_of(rest: &str) -> Option<Word> {
    let w: String = rest.trim().trim_matches(|c| c == '*' || c == '`').replace(['_', '-'], " ").to_ascii_uppercase();
    let w: String = w.split_whitespace().collect::<Vec<_>>().join(" ");
    let w = w.trim_start_matches(|c: char| !c.is_ascii_alphabetic());
    if w.starts_with("NOT REPRODUCED") {
        Some(Word::NotReproduced)
    } else if w.starts_with("REPRODUCED") {
        Some(Word::Reproduced)
    } else if w.starts_with("FIXED") {
        Some(Word::Fixed)
    } else if w.starts_with("UNCLEAR") {
        Some(Word::Unclear)
    } else {
        None
    }
}

/// A path as a report may write it: in backticks or quotes, as a Markdown image, or a file URL.
fn clean_path(s: &str) -> Option<String> {
    let mut p = s.trim();
    if let Some(i) = p.find("](") {
        p = p[i + 2..].trim_end_matches(')');
    }
    let p = p.trim().trim_matches(|c| c == '`' || c == '"' || c == '\'' || c == '<' || c == '>').trim();
    let p = match p.get(..8) {
        Some(head) if head.eq_ignore_ascii_case("file:///") => &p[8..],
        _ => p,
    };
    (!p.is_empty() && !p.eq_ignore_ascii_case("none")).then(|| p.to_string())
}

#[derive(PartialEq)]
enum Section {
    None,
    Notes,
    Steps,
}

/// Reads the report's closing lines. The last of each wins (a report may quote an earlier one).
pub fn parse_report(text: &str) -> Report {
    let mut r = Report::default();
    let mut sec = Section::None;
    for raw in text.lines() {
        let l = raw.trim().trim_matches(|c| c == '*' || c == '`' || c == '_').trim();
        let up = l.to_ascii_uppercase();
        let key = |k: &str| up.starts_with(k).then(|| l[k.len()..].trim_start_matches(|c| c == '*' || c == '`' || c == '_').trim());
        if let Some(rest) = key("CARD:") {
            let digits: String = rest.trim_start_matches(|c: char| c == '#' || c.is_whitespace()).chars().take_while(char::is_ascii_digit).collect();
            if let Ok(n) = digits.parse() {
                r.card = Some(n);
            }
            sec = Section::None;
        } else if let Some(rest) = key("VERDICT:") {
            if let Some(w) = word_of(rest) {
                r.verdict = Some(w);
                r.notes.clear();
                sec = Section::Notes;
            } else {
                sec = Section::None;
            }
        } else if let Some(rest) = key("STEPS:") {
            r.steps.clear();
            if !rest.is_empty() {
                r.steps.push(rest.to_string());
            }
            sec = Section::Steps;
        } else if let Some(rest) = key("SHOT:") {
            r.shot = clean_path(rest);
            sec = Section::None;
        } else if let Some(rest) = key("TEST:") {
            let t = rest.trim_matches(|c| c == '`' || c == '"').trim();
            r.test = (!t.is_empty() && !t.eq_ignore_ascii_case("none")).then(|| t.to_string());
            sec = Section::None;
        } else if sec == Section::Notes {
            if let Some(f) = raw.trim().strip_prefix("- ").or_else(|| raw.trim().strip_prefix("* ")) {
                r.notes.push(f.trim().to_string());
            }
        } else if sec == Section::Steps && !raw.trim().is_empty() {
            r.steps.push(raw.trim().to_string());
        }
    }
    r
}

fn cut(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn state_of(rec: Option<&Value>) -> &str {
    rec.and_then(|r| r.get("state")).and_then(Value::as_str).unwrap_or("")
}

/// Is a cycle open: a report not yet proved fixed or not reproduced? (`kanban_store::repro_open`
/// says the same of a card; a test keeps them in step.)
fn open(state: &str) -> bool {
    matches!(state, "needed" | "reproduced" | "unclear")
}

/// Starts a cycle again: its "before" (with its "after" and test) moves to `earlier`, the last
/// MAX_EARLIER of them, so a proof already made stays on the card.
fn archive(o: &mut Map<String, Value>) {
    let (before, after, test) = (o.remove("before"), o.remove("after"), o.remove("test"));
    let Some(before) = before.filter(Value::is_object) else { return };
    let mut e = json!({ "state": o.get("state").cloned().unwrap_or(Value::Null), "reports": o.get("reports").cloned().unwrap_or_else(|| json!([])), "before": before });
    if let Some(a) = after {
        e["after"] = a;
    }
    if let Some(t) = test {
        e["test"] = t;
    }
    let mut list = o.get("earlier").and_then(Value::as_array).cloned().unwrap_or_default();
    list.push(e);
    let skip = list.len().saturating_sub(MAX_EARLIER);
    o.insert("earlier".into(), json!(list.split_off(skip)));
}

/// The record a new bug report gives: needed, with the open cycle's reports and this one. Any
/// before and after go to `earlier` (`archive`), so the next run is a first run for every open
/// report, and the proof made so far stays on the card.
pub fn needed_record(prior: Option<&Value>, source: &str, note: &str, at: &str) -> Value {
    let mut rec = prior.filter(|p| p.is_object()).cloned().unwrap_or_else(|| json!({}));
    let was_open = open(state_of(Some(&rec)));
    let o = rec.as_object_mut().expect("an object");
    archive(o);
    let mut reports: Vec<Value> = if was_open { o.get("reports").and_then(Value::as_array).cloned().unwrap_or_default() } else { vec![] };
    reports.push(json!({ "at": at, "source": source, "note": cut(&mask_secrets(note.trim()), MAX_NOTE) }));
    let skip = reports.len().saturating_sub(MAX_REPORTS);
    o.insert("reports".into(), json!(reports.split_off(skip)));
    o.insert("state".into(), json!("needed"));
    o.insert("at".into(), json!(at));
    o.remove("detail");
    o.remove("agentId");
    rec
}

/// The steps a session follows for a card whose repro is open (ASD-STE100: the reader is a
/// Claude session).
fn steps(n: i64) -> String {
    format!("If your session has the {REPRODUCER} subagent (the Agent tool lists it), do these steps:
1. Run it before you fix anything (subagent_type \"{REPRODUCER}\"). Brief it with the card number (#{n}) and where the change is. It reproduces the bug in the test app and drafts a failing test.
2. Add its test. See the test fail. Then fix the bug.
3. Install the fix in the test app. Run the {REPRODUCER} again. Give it the fix commit and the test's name.
4. Move the card to Human test only when its latest verdict is FIXED. If it says NOT REPRODUCED for a report from Hugh, put one question for him in blockedReason with choices, and move the card to Blocked. For a card-tester's report, log what it tried and go on.
If your session has no {REPRODUCER} subagent (it started before this rule), say so on the card and ask Hugh to turn the rule off or restart the session.")
}

/// What the session is told when a report sets its card's repro to needed.
pub fn ask_note(n: i64) -> String {
    format!("The app set the repro of card #{n} to needed (a bug report). {}", steps(n))
}

/// The bug-report sources, by the id callers pass: what the log calls it, and who it is by.
pub fn source_label(source: &str) -> (String, &'static str) {
    match source {
        "found-a-problem" => ("Hugh's Found a problem".into(), crate::HUGH),
        "human-test-fail" => ("Hugh's Fail in Human test".into(), crate::HUGH),
        "report-a-bug" => ("Hugh's Report a bug".into(), crate::HUGH),
        "card-tester" | "card-reviewer" => (format!("the {source}'s FAIL"), BY),
        other => (other.to_string(), BY),
    }
}

/// A bug report on `card`: sets its repro to needed (joining an open cycle), unless Hugh turned
/// the rule off. Returns the card as saved and whether repro is now needed. It logs one line
/// that says what happens to the report, not the note: the caller logs the note itself (Found a
/// problem, the FAIL line, Report a bug, card #102's "Failed in Human test"). It doesn't move the
/// card or send anything: a caller that tells the session adds `ask_note` when it returns true.
/// Every trigger comes through here, by `source`: "found-a-problem", "human-test-fail" (card
/// #102's Fail), "report-a-bug" and "card-tester" (a FAIL with a BUG: line). Keep this
/// signature: card #102 calls it.
pub fn mark_needed(store: &mut Store, card: &Card, source: &str, note: &str) -> Result<(Card, bool), String> {
    let (label, by) = source_label(source);
    // Card #151: Hugh's choice of the reproducer. A stack's card follows its lead's, whose
    // session runs it for the stack (as `stack_problem` asks it to).
    let lead = card.stack_of().and_then(|_| store.stack_lead(card).ok().flatten());
    let whose = lead.as_ref().unwrap_or(card);
    if !store.subagent_on(whose, "reproducer") {
        let off = match &lead {
            Some(l) => format!("its stack's lead #{} has the card-reproducer off", l.number),
            None => "Hugh has the card-reproducer off for this card".to_string(),
        };
        store.add_log(&card.id, &format!("Bug report ({label}): {off}, so it isn't asked for and nothing is held."), by).map_err(|e| e.to_string())?;
        return Ok((card.clone(), false));
    }
    // A stack's card doesn't install the test app, so it can't be held for a proof: the lead's
    // card-reproducer proves it on the stack's install (the router's decision, whole-build
    // review S8). The caller tells the sessions with `child_note`.
    if card.stack_of().is_some() {
        store.add_log(&card.id, &format!("Bug report ({label}): this is a stack's card, so it isn't held; the stack's lead proves the bug with its card-reproducer after it installs the stack."), by).map_err(|e| e.to_string())?;
        return Ok((card.clone(), false));
    }
    let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let mut patch = Patch::new();
    patch.insert(REPRO_KEY.into(), needed_record(card.fields.get(REPRO_KEY), source, note, &at));
    let saved = store.update(&card.id, card.version, &patch, by).map_err(|e| e.to_string())?;
    store.add_log(&card.id, &format!("Bug report ({label}): the card-reproducer is to prove it before and after the fix."), by).map_err(|e| e.to_string())?;
    Ok((saved, true))
}

/// Why Hugh's "Report a bug" can't go on `card`, if it can't. It is for a card being built (In
/// progress). In Human test, card #102's Fail is the bug report (the router's call); a stack's
/// card has "Found a problem", which goes to the session leading the stack.
pub fn report_refused(card: &Card, text: &str) -> Option<String> {
    if text.trim().is_empty() {
        return Some("say what the bug is".into());
    }
    if card.archived {
        return Some("this card is archived".into());
    }
    if card.stack_of().is_some() {
        return Some("a stack's card takes a bug report through Found a problem, in the stack's overlay".into());
    }
    match card.column.as_str() {
        "In progress" => None,
        "Human test" => Some("a card in Human test takes a bug report through Fail (or Found a problem on a stack's card), not Report a bug".into()),
        c => Some(format!("Report a bug is for a card in In progress; this one is in {c}")),
    }
}

/// What a bug report on stack card #`n` tells the session that gets it (ASD-STE100): the child's
/// own session when it builds itself (`own_build`), else the lead's session, which builds it.
pub fn child_note(n: i64, own_build: bool) -> String {
    if own_build {
        format!("Card #{n} is a stack's card, so the app does not hold it for the {REPRODUCER}. Do not install the test app. Fix the bug as usual. Put the steps that show the bug in your hand-over. The stack's lead proves the fix on the stack's install: if its session has the {REPRODUCER}, it runs it with CARD: #{n} after it installs.")
    } else {
        format!("Card #{n} is a card in your stack, so the app does not hold it for the {REPRODUCER}. If your session has the {REPRODUCER} subagent: after you install the stack's build with the fix, run it with CARD: #{n} and the steps that show the bug. The app records its result on #{n}.")
    }
}

/// The message Hugh's "Report a bug" sends to the session.
pub fn bug_message(n: i64, text: &str, needed: bool) -> String {
    let head = format!("Hugh reported a bug on card #{n} (Report a bug):\n\n{}", text.trim());
    if needed {
        format!("{head}\n\n{}", ask_note(n))
    } else {
        format!("{head}\n\nHugh's reproducer rule is off: fix it as usual.")
    }
}

/// The BUG: lines of a check's FAIL (`- BUG: ...`, as `parse_verdict` keeps them, Markdown bold
/// or backticks around the word allowed), joined.
pub fn bug_lines(failures: &[String]) -> Option<String> {
    let marks = |c: char| c == '*' || c == '_' || c == '`';
    let bugs: Vec<&str> = failures
        .iter()
        .filter_map(|f| {
            let t = f.trim().trim_start_matches(marks).trim_start();
            t.get(..4).filter(|h| h.eq_ignore_ascii_case("BUG:")).map(|_| t[4..].trim_start_matches(marks).trim())
        })
        .filter(|b| !b.is_empty())
        .collect();
    (!bugs.is_empty()).then(|| bugs.join("; "))
}

/// What the app records for a reproducer's report: the new `repro` value and the log line.
/// `shot` is the kept copy of its screenshot, if any. The value is None when no bug report is
/// open on the card and the run proved nothing (unclear): a stray run doesn't make a hold.
pub fn record_of(prior: Option<&Value>, r: &Report, tools: Option<usize>, agent_id: &str, at: &str, shot: Option<&str>) -> (Option<Value>, String) {
    // What it stores is masked first (the router's DSH rule 5): a step may hold a sign-in link
    // or a token, and the board would refuse the whole record.
    let r = &Report { notes: r.notes.iter().map(|x| mask_secrets(x)).collect(), steps: r.steps.iter().map(|x| mask_secrets(x)).collect(), test: r.test.as_deref().map(mask_secrets), ..r.clone() };
    let mut rec = prior.filter(|p| p.is_object()).cloned().unwrap_or_else(|| json!({ "reports": [] }));
    // A run on a closed cycle (proved fixed, or not reproduced) starts it again: its reports
    // stay, and its proof goes to `earlier`.
    let was_open = open(state_of(Some(&rec)));
    if !was_open {
        archive(rec.as_object_mut().expect("an object"));
    }
    let has_before = rec.get("before").is_some_and(Value::is_object);
    let notes = r.notes.join("; ");
    let said = |fallback: &str| if notes.is_empty() { fallback.to_string() } else { notes.clone() };
    let first_step = r.steps.first().cloned().unwrap_or_default();
    use Word::*;
    let (state, slot, detail, line) = match (r.verdict, tools) {
        (Some(_), Some(0)) => ("unclear", None, "the reproducer gave a verdict but ran no tools, so it didn't try anything".to_string(), None),
        (None, _) => ("unclear", None, "the reproducer ended with no VERDICT line (REPRODUCED, NOT REPRODUCED, FIXED or UNCLEAR)".to_string(), None),
        (Some(Unclear), _) => ("unclear", None, said("the reproducer couldn't tell"), None),
        (Some(Reproduced), _) if has_before => {
            let d = said(&first_step);
            ("reproduced", Some("after"), d.clone(), Some(format!("Still reproduced after the fix: {d}")))
        }
        (Some(Reproduced), _) => {
            let d = said(&first_step);
            ("reproduced", Some("before"), d.clone(), Some(format!("Reproduced: {d}")))
        }
        (Some(NotReproduced), _) if has_before => ("unclear", None, "a second run says FIXED or REPRODUCED, not NOT REPRODUCED".to_string(), None),
        (Some(NotReproduced), _) => {
            let d = said("see the reproducer's report in the session");
            ("not-reproduced", Some("before"), d.clone(), Some(format!("Not reproduced: {d}")))
        }
        (Some(Fixed), _) if has_before => {
            let t = r.test.as_deref().map(|t| format!(", and {t} passes")).unwrap_or_default();
            ("fixed", Some("after"), notes.clone(), Some(format!("Fixed: the reproducer reran the steps{t}")))
        }
        (Some(Fixed), _) => ("unclear", None, "FIXED with no before run to compare: run the card-reproducer before the fix".to_string(), None),
    };
    let detail = cut(&detail, MAX_NOTE);
    if !was_open && slot.is_none() {
        return (None, cut(&format!("Reproducer run not recorded: no bug report is open on the card, and the run proved nothing ({detail})"), 700));
    }
    let line = cut(&line.unwrap_or_else(|| format!("Reproducer unclear: {detail}")), 700);
    let o = rec.as_object_mut().expect("an object");
    o.insert("state".into(), json!(state));
    o.insert("at".into(), json!(at));
    o.insert("agentId".into(), json!(agent_id));
    if detail.is_empty() {
        o.remove("detail");
    } else {
        o.insert("detail".into(), json!(detail));
    }
    if let Some(slot) = slot {
        let mut run = json!({ "at": at, "agentId": agent_id, "steps": cut(&r.steps.join("\n"), MAX_STEPS) });
        if let Some(s) = shot {
            run["shot"] = json!(s);
        }
        o.insert(slot.into(), run);
        if let Some(t) = &r.test {
            o.insert("test".into(), json!(cut(t, 200)));
        }
    }
    (Some(rec), line)
}

/// Copies a reproducer's screenshot into the data folder's `repro\<card id>\`, so it outlasts
/// temp. Only an image file on a local drive, by its full path, within the attachment cap
/// (the chat's own checks). Returns the copy's path.
pub fn copy_shot(data: &Path, card_id: &str, at: &str, agent_id: &str, src: &str) -> Result<PathBuf, String> {
    let (from, _) = crate::local_image(src)?;
    let ext = from.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let safe = |s: &str| s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect::<String>();
    let dir = data.join("repro").join(safe(card_id));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let to = dir.join(format!("{}-{}.{ext}", safe(at), safe(&cut(agent_id, 24))));
    std::fs::copy(&from, &to).map_err(|e| e.to_string())?;
    Ok(to)
}

/// A `SubagentStop` hook for the card-reproducer, from session `row_id`.
pub fn on_stop(app: &AppHandle, row_id: &str, body: &Value) {
    let text = body.get("last_assistant_message").and_then(Value::as_str).unwrap_or("");
    let agent_id = body.get("agent_id").and_then(Value::as_str).unwrap_or("");
    let tools = body.get("agent_transcript_path").and_then(Value::as_str).and_then(tool_calls);
    let r = parse_report(text);
    let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let board = app.state::<crate::Board>();
    // Which card it is for, under the lock for a moment.
    let card_id = {
        let Ok(mut store) = board.0.lock() else { return };
        let Some(own) = crate::agents::own_or_named(app, &store, row_id, r.card) else { return };
        match target_card(&store, &own, r.card) {
            Ok(c) => c.id,
            Err(why) => {
                let _ = store.add_log(&own.id, &format!("Not recorded: the {REPRODUCER} subagent reported on {why}"), BY);
                return;
            }
        }
    };
    // The screenshot is copied off the lock: it can be up to the attachment cap.
    let (shot, shot_err) = match r.shot.as_deref() {
        Some(s) => match copy_shot(&kanban_store::data_dir(), &card_id, &at, agent_id, s) {
            Ok(p) => (Some(p.display().to_string()), None),
            Err(e) => (None, Some(format!("The reproducer's screenshot wasn't kept ({e}): {}", mask_secrets(&cut(s, 300))))),
        },
        None => (None, None),
    };
    let Ok(mut store) = board.0.lock() else { return };
    if let Some(e) = shot_err {
        let _ = store.add_log(&card_id, &e, BY);
    }
    // Read again: the card may have changed while the shot was copied.
    let Ok(card) = store.get(&card_id) else { return };
    let (rec, line) = record_of(card.fields.get(REPRO_KEY), &r, tools, agent_id, &at, shot.as_deref());
    let saved = match rec {
        Some(rec) => {
            let mut patch = Patch::new();
            patch.insert(REPRO_KEY.into(), rec);
            match store.update(&card.id, card.version, &patch, BY) {
                Ok(c) => c,
                Err(e) => {
                    let _ = store.add_log(&card.id, &format!("Couldn't record the {REPRODUCER} subagent's verdict: {e}"), BY);
                    return;
                }
            }
        }
        None => card.clone(),
    };
    let _ = store.add_log(&card.id, &line, BY);
    // A stack held back for its lead's own bug may be ready now.
    if saved.is_lead() && repro_open(&card) && !repro_open(&saved) {
        crate::dispatch::after_lead_verdict(app, &mut store, &saved.id, BY, &format!("#{}'s reported bug is proved", saved.number));
    }
    drop(store);
    let _ = app.emit("cards-changed", ());
}

/// Why a card's session may not move `card` to `to`: into Human test (or on past it) from any
/// column before it, while a bug report on it isn't yet proved fixed or not reproduced, and
/// Hugh's rule is on.
pub fn gate(store: &Store, card: &Card, to: &str) -> Option<String> {
    if !crate::tester::BEFORE_TEST.contains(&card.column.as_str()) || !crate::tester::INTO_TEST.contains(&to) || !store.subagent_on(card, "reproducer") {
        return None;
    }
    // A stack's card is never held for it: the lead proves it on the stack's install (S8).
    if card.stack_of().is_some() {
        return None;
    }
    let rec = card.fields.get(REPRO_KEY);
    let state = state_of(rec);
    if !open(state) {
        return None;
    }
    let n = card.number;
    let what = match state {
        "needed" => {
            let last = rec.and_then(|r| r.get("reports")).and_then(Value::as_array).and_then(|a| a.last()).and_then(|x| x.get("note")).and_then(Value::as_str).unwrap_or("");
            format!("Card #{n} has a bug report that the {REPRODUCER} hasn't reproduced yet{}.", if last.is_empty() { String::new() } else { format!(": {}", cut(last, 300)) })
        }
        "reproduced" => format!("The bug on card #{n} is reproduced, and no run of the {REPRODUCER} has proved the fix yet."),
        _ => format!("The latest run of the {REPRODUCER} on card #{n} didn't count: {}.", rec.and_then(|r| r.get("detail")).and_then(Value::as_str).unwrap_or("no verdict")),
    };
    Some(format!("{what} (Hugh's rule: a reported bug is proved fixed before Human test.) {}", steps(n)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanban_store::REPRO_RULE_KEY;

    /// `record_of` where the run is recorded.
    fn rec_of(prior: Option<&Value>, r: &Report, tools: Option<usize>, agent_id: &str, at: &str, shot: Option<&str>) -> (Value, String) {
        let (rec, line) = record_of(prior, r, tools, agent_id, at, shot);
        (rec.unwrap_or_else(|| panic!("not recorded: {line}")), line)
    }

    const FIRST: &str = "I opened the test board.\n\n**CARD:** #112\n**VERDICT: REPRODUCED**\n- the chip says Tested on a failed card\nSTEPS:\n1. Open the test app.\n2. Look at card #5.\nSHOT: `C:\\Temp\\before.png`\nTEST: chip_shows_failed";

    #[test]
    fn the_report_is_read_from_its_closing_lines() {
        let r = parse_report(FIRST);
        assert_eq!(r.card, Some(112));
        assert_eq!(r.verdict, Some(Word::Reproduced));
        assert_eq!(r.notes, vec!["the chip says Tested on a failed card"]);
        assert_eq!(r.steps, vec!["1. Open the test app.", "2. Look at card #5."]);
        assert_eq!(r.shot.as_deref(), Some("C:\\Temp\\before.png"));
        assert_eq!(r.test.as_deref(), Some("chip_shows_failed"));
        // The four words, in any case and with the separators reports use.
        for (t, w) in [("VERDICT: NOT REPRODUCED", Word::NotReproduced), ("verdict: not_reproduced", Word::NotReproduced), ("VERDICT: Not-Reproduced", Word::NotReproduced), ("VERDICT: FIXED", Word::Fixed), ("VERDICT: unclear", Word::Unclear), ("VERDICT: `REPRODUCED`", Word::Reproduced)] {
            assert_eq!(parse_report(t).verdict, Some(w), "{t}");
        }
        assert_eq!(parse_report("VERDICT: PASS").verdict, None);
        assert_eq!(parse_report("All fine.").verdict, None);
        // The last verdict wins; a Markdown image and "none" are read as paths should be.
        let r = parse_report("VERDICT: REPRODUCED\n- x\nRan it again.\nVERDICT: FIXED\nSHOT: ![after](C:/t/a.png)\nTEST: none");
        assert_eq!((r.verdict, r.notes.len(), r.shot.as_deref(), r.test), (Some(Word::Fixed), 0, Some("C:/t/a.png"), None));
        assert_eq!(parse_report("SHOT: none").shot, None);
        assert_eq!(parse_report("SHOT: file:///C:/t/a.png").shot.as_deref(), Some("C:/t/a.png"));
    }

    #[test]
    fn a_bug_report_opens_or_joins_a_cycle() {
        let r = needed_record(None, "Hugh's Report a bug", "the chip is wrong", "t1");
        assert_eq!(r["state"], "needed");
        assert_eq!(r["reports"].as_array().unwrap().len(), 1);
        // A report while one is open joins it, and the record starts again from needed.
        let open = json!({ "state": "reproduced", "reports": [{ "note": "a" }], "before": { "steps": "1. x" }, "test": "t" });
        let r = needed_record(Some(&open), "the card-tester's FAIL", "b", "t2");
        assert_eq!(r["state"], "needed");
        assert_eq!(r["reports"].as_array().unwrap().iter().map(|x| x["note"].as_str().unwrap()).collect::<Vec<_>>(), vec!["a", "b"]);
        assert!(r.get("before").is_none() && r.get("after").is_none() && r.get("test").is_none());
        // The proof made so far stays on the card, in earlier.
        assert_eq!(r["earlier"][0]["before"]["steps"], "1. x");
        assert_eq!((r["earlier"][0]["state"].as_str(), r["earlier"][0]["test"].as_str()), (Some("reproduced"), Some("t")));
        // A closed cycle (fixed, not reproduced) is history: a new report starts a new one.
        for closed in ["fixed", "not-reproduced"] {
            let r = needed_record(Some(&json!({ "state": closed, "reports": [{ "note": "a" }] })), "s", "c", "t3");
            assert_eq!(r["reports"].as_array().unwrap().len(), 1, "{closed}");
        }
        // It keeps the last 20 reports.
        let mut rec = needed_record(None, "s", "0", "t");
        for i in 1..30 {
            rec = needed_record(Some(&rec), "s", &i.to_string(), "t");
        }
        let notes = rec["reports"].as_array().unwrap();
        assert_eq!((notes.len(), notes[0]["note"].as_str()), (20, Some("10")));
    }

    #[test]
    fn a_new_round_keeps_the_last_rounds_proof() {
        let fixed = json!({ "state": "fixed", "reports": [{ "note": "A" }], "before": { "steps": "1. a" }, "after": { "steps": "1. a" }, "test": "pins_a" });
        let r = needed_record(Some(&fixed), "card-tester", "B", "t");
        assert_eq!(r["reports"].as_array().unwrap().iter().map(|x| x["note"].as_str().unwrap()).collect::<Vec<_>>(), vec!["B"]);
        let e = &r["earlier"][0];
        assert_eq!((e["state"].as_str(), e["reports"][0]["note"].as_str(), e["after"]["steps"].as_str(), e["test"].as_str()), (Some("fixed"), Some("A"), Some("1. a"), Some("pins_a")));
        // No before, nothing to keep; and only the last five rounds.
        assert!(needed_record(Some(&json!({ "state": "needed", "reports": [] })), "s", "x", "t").get("earlier").is_none());
        let mut rec = fixed.clone();
        for i in 0..8 {
            rec = needed_record(Some(&rec), "s", &i.to_string(), "t");
            rec["state"] = json!("fixed");
            rec["before"] = json!({ "steps": i.to_string() });
        }
        let earlier = needed_record(Some(&rec), "s", "z", "t")["earlier"].as_array().unwrap().clone();
        assert_eq!((earlier.len(), earlier[4]["before"]["steps"].as_str()), (5, Some("7")));
    }

    #[test]
    fn a_stray_run_makes_no_hold() {
        // No report open: a run that proves nothing isn't recorded, so nothing is held.
        for (prior, text) in [(None, "VERDICT: UNCLEAR
- no reports on the card"), (None, "Nothing to say."), (Some(json!({ "state": "fixed", "before": { "steps": "1" } })), "VERDICT: FIXED"), (None, "VERDICT: NOT REPRODUCED")] {
            let (rec, line) = record_of(prior.as_ref(), &parse_report(text), Some(0), "a", "t", None);
            assert!(rec.is_none(), "{text}");
            assert!(line.starts_with("Reproducer run not recorded: no bug report is open"), "{line}");
        }
        // But a run that reproduces a bug opens a round (the session ran it on Hugh's message).
        let (rec, _) = record_of(None, &parse_report(FIRST), Some(3), "a", "t", None);
        assert_eq!(rec.unwrap()["state"], "reproduced");
        // And NOT REPRODUCED with tools run, on no round, is kept: it holds nothing.
        let (rec, _) = record_of(None, &parse_report("VERDICT: NOT REPRODUCED
- tried it"), Some(3), "a", "t", None);
        assert!(!open(state_of(rec.as_ref())));
    }

    #[test]
    fn the_app_and_the_store_agree_on_an_open_round() {
        for st in ["needed", "reproduced", "unclear", "fixed", "not-reproduced", "other"] {
            let c = card(json!({ "title": "T", "repro": { "state": st } }), "In progress");
            assert_eq!(repro_open(&c), open(st), "{st}");
        }
        assert!(!repro_open(&card(json!({ "title": "T" }), "In progress")));
    }

    #[test]
    fn bug_lines_are_the_fails_that_set_repro() {
        assert_eq!(bug_lines(&["BUG: Esc doesn't close it".into(), "a wording nit".into(), "bug: the chip is grey".into()]).as_deref(), Some("Esc doesn't close it; the chip is grey"));
        assert_eq!(bug_lines(&["no test for the migration".into()]), None);
        assert_eq!(bug_lines(&[]), None);
        // Markdown around the word, as models write it.
        assert_eq!(bug_lines(&["**BUG:** the chip stays grey".into(), "`BUG:` x".into(), "_BUG: y_".into()]).as_deref(), Some("the chip stays grey; x; y_"));
        assert_eq!(bug_lines(&["**BUG:**".into()]), None);
    }

    #[test]
    fn the_first_verdict_is_the_before_and_later_ones_the_after() {
        let needed = needed_record(None, "s", "the chip is wrong", "t0");
        let (rec, line) = rec_of(Some(&needed), &parse_report(FIRST), Some(9), "a1", "t1", Some("D:\\data\\repro\\c\\1.png"));
        assert_eq!(rec["state"], "reproduced");
        assert_eq!(rec["before"]["steps"], "1. Open the test app.\n2. Look at card #5.");
        assert_eq!(rec["before"]["shot"], "D:\\data\\repro\\c\\1.png");
        assert_eq!(rec["test"], "chip_shows_failed");
        assert_eq!(rec["reports"].as_array().unwrap().len(), 1);
        assert_eq!(line, "Reproduced: the chip says Tested on a failed card");
        let (fixed, line) = rec_of(Some(&rec), &parse_report("CARD: #112\nVERDICT: FIXED\nSTEPS:\n1. Open the test app.\n2. Look at card #5.\nSHOT: none\nTEST: chip_shows_failed"), Some(5), "a2", "t2", None);
        assert_eq!(fixed["state"], "fixed");
        assert_eq!(fixed["before"], rec["before"]);
        assert_eq!(fixed["after"]["agentId"], "a2");
        assert!(fixed["after"].get("shot").is_none());
        assert_eq!(line, "Fixed: the reproducer reran the steps, and chip_shows_failed passes");
        // Still there after the fix: the after says so, and the before stays.
        let (still, line) = rec_of(Some(&rec), &parse_report("VERDICT: REPRODUCED\n- still grey"), Some(5), "a3", "t3", None);
        assert_eq!((still["state"].as_str(), still["before"] == rec["before"], line.as_str()), (Some("reproduced"), true, "Still reproduced after the fix: still grey"));
        assert!(still["after"].is_object());
    }

    #[test]
    fn what_isnt_proof_is_unclear() {
        let needed = needed_record(None, "s", "x", "t0");
        // FIXED with no before run.
        let (rec, line) = rec_of(Some(&needed), &parse_report("VERDICT: FIXED"), Some(3), "a", "t", None);
        assert_eq!(rec["state"], "unclear");
        assert!(line.starts_with("Reproducer unclear: FIXED with no before run"), "{line}");
        assert!(rec.get("after").is_none());
        // No tools run, no verdict line, and the commit check's UNCLEAR.
        assert_eq!(rec_of(Some(&needed), &parse_report(FIRST), Some(0), "a", "t", None).0["state"], "unclear");
        assert_eq!(rec_of(Some(&needed), &parse_report("Looks fine."), Some(4), "a", "t", None).0["state"], "unclear");
        let (rec, _) = rec_of(Some(&needed), &parse_report("VERDICT: UNCLEAR\n- the test app isn't on the fix (abc123)"), Some(4), "a", "t", None);
        assert_eq!((rec["state"].as_str(), rec["detail"].as_str()), (Some("unclear"), Some("the test app isn't on the fix (abc123)")));
        // NOT REPRODUCED on a second run isn't one of its answers.
        let (rec1, _) = rec_of(Some(&needed), &parse_report(FIRST), Some(9), "a", "t", None);
        assert_eq!(rec_of(Some(&rec1), &parse_report("VERDICT: NOT REPRODUCED"), Some(4), "a", "t", None).0["state"], "unclear");
        // An unclear run keeps the before, so the next run is still the second.
        let (u, _) = rec_of(Some(&rec1), &parse_report("VERDICT: UNCLEAR\n- stale"), Some(4), "a", "t", None);
        assert!(u["before"].is_object());
        assert_eq!(rec_of(Some(&u), &parse_report("VERDICT: FIXED"), Some(4), "a", "t", None).0["state"], "fixed");
    }

    #[test]
    fn not_reproduced_and_a_closed_cycle() {
        let needed = needed_record(None, "s", "x", "t0");
        let (nr, line) = rec_of(Some(&needed), &parse_report("VERDICT: NOT REPRODUCED\n- tried the chip on 3 cards"), Some(6), "a", "t", None);
        assert_eq!(nr["state"], "not-reproduced");
        assert!(nr["before"].is_object());
        assert_eq!(line, "Not reproduced: tried the chip on 3 cards");
        // A rerun after it closed (Hugh gave more detail) is a first run again; reports stay.
        let (again, _) = rec_of(Some(&nr), &parse_report(FIRST), Some(6), "b", "t2", None);
        assert_eq!((again["state"].as_str(), again["before"]["agentId"].as_str()), (Some("reproduced"), Some("b")));
        assert_eq!(again["reports"].as_array().unwrap().len(), 1);
        assert_eq!(again["earlier"][0]["state"], "not-reproduced");
        // A run with no report at all still records.
        assert_eq!(rec_of(None, &parse_report(FIRST), Some(2), "a", "t", None).0["state"], "reproduced");
    }

    fn card(fields: Value, column: &str) -> Card {
        Card { id: "c9".into(), number: 9, column: column.into(), order: None, order_col: None, archived: false, fields: fields.as_object().cloned().unwrap(), version: 1 }
    }

    #[test]
    fn the_move_to_human_test_waits_for_proof() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let with = |state: &str, col: &str| card(json!({ "title": "T", "repro": { "state": state, "reports": [{ "note": "the chip is wrong" }], "detail": "stale build" } }), col);
        let why = gate(&s, &with("needed", "In progress"), "Human test").unwrap();
        assert!(why.contains("hasn't reproduced yet: the chip is wrong") && why.contains("subagent_type \"card-reproducer\""), "{why}");
        assert!(gate(&s, &with("reproduced", "In progress"), "Deploy complete").unwrap().contains("no run of the card-reproducer has proved the fix"));
        assert!(gate(&s, &with("unclear", "Blocked"), "Human test").unwrap().contains("didn't count: stale build"));
        for ok in ["fixed", "not-reproduced"] {
            assert!(gate(&s, &with(ok, "In progress"), "Human test").is_none(), "{ok}");
        }
        // No report, other moves, and a card already in Human test are free.
        assert!(gate(&s, &card(json!({ "title": "T" }), "In progress"), "Human test").is_none());
        for to in ["Done", "Blocked", "Planning", "In progress"] {
            assert!(gate(&s, &with("needed", "In progress"), to).is_none(), "{to}");
        }
        assert!(gate(&s, &with("needed", "Human test"), "Deploy complete").is_none());
        // Hugh's rule off lets it through.
        s.set_page_setting(REPRO_RULE_KEY, "off").unwrap();
        assert!(gate(&s, &with("needed", "In progress"), "Human test").is_none());
    }

    #[test]
    fn what_is_stored_has_its_secrets_hidden() {
        let needed = needed_record(None, "report-a-bug", "see https://x/dev-sign-in.html?as=admin", "t");
        assert!(!kanban_store::looks_secret(&needed.to_string()), "{needed}");
        assert!(!needed.to_string().contains("dev-sign-in"), "{needed}");
        let (rec, _) = rec_of(Some(&needed), &parse_report("VERDICT: REPRODUCED\n- the password box is blank\nSTEPS:\n1. Open http://127.0.0.1:3006/dev-sign-in.html?as=admin\n2. Send Authorization: Bearer abc.def\nTEST: t"), Some(2), "a", "t", None);
        let all = rec.to_string();
        assert!(!kanban_store::looks_secret(&all) && !all.contains("dev-sign-in") && !all.contains("abc.def"), "{all}");
        assert!(rec["before"]["steps"].as_str().unwrap().starts_with("1. Open "));
    }

    #[test]
    fn a_stacks_card_isnt_held_and_its_lead_proves_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let lead = s.create(None, json!({ "title": "lead", "column": "In progress" }).as_object().cloned().unwrap(), "test").unwrap();
        let child = s.create_in_stack(json!({ "title": "child" }).as_object().cloned().unwrap(), lead.number, "test").unwrap();
        for src in ["found-a-problem", "human-test-fail", "card-tester"] {
            let c = s.get(&child.id).unwrap();
            let (c, needed) = mark_needed(&mut s, &c, src, "the chip is wrong").unwrap();
            assert!(!needed, "{src}");
            assert!(c.fields.get(REPRO_KEY).is_none(), "{src}");
        }
        assert!(s.log(&child.id).unwrap().iter().any(|e| e.text.contains("this is a stack's card, so it isn't held")));
        // So nothing holds the child's move, even with a round the lead's run opened on it.
        assert!(gate(&s, &s.get(&child.id).unwrap(), "Human test").is_none());
        let held = card(json!({ "title": "T", "stackOf": lead.number, "repro": { "state": "reproduced" } }), "In progress");
        assert!(gate(&s, &held, "Human test").is_none());
        let own = child_note(7, true);
        assert!(own.contains("Do not install the test app") && own.contains("CARD: #7"), "{own}");
        let lead_built = child_note(7, false);
        assert!(lead_built.contains("after you install the stack's build") && lead_built.contains("CARD: #7"), "{lead_built}");
    }

    #[test]
    fn mark_needed_follows_the_rule() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let c = s.create(None, json!({ "title": "T", "column": "In progress" }).as_object().cloned().unwrap(), "test").unwrap();
        let (c, on) = mark_needed(&mut s, &c, "report-a-bug", "the chip is wrong").unwrap();
        assert!(on);
        assert_eq!(c.fields[REPRO_KEY]["state"], "needed");
        assert_eq!(c.fields[REPRO_KEY]["reports"][0]["source"], "report-a-bug");
        // A second report joins the open cycle.
        let (c, _) = mark_needed(&mut s, &c, "card-tester", "Esc doesn't close it").unwrap();
        assert_eq!(c.fields[REPRO_KEY]["reports"].as_array().unwrap().len(), 2);
        // Card #151: the board's default off is for new cards; this one keeps its reproducer.
        s.set_page_setting(REPRO_RULE_KEY, "off").unwrap();
        assert!(mark_needed(&mut s, &c, "found-a-problem", "again").unwrap().1);
        // Its own choice off: the report is only logged.
        let c = s.get(&c.id).unwrap();
        let c = s.update(&c.id, c.version, &json!({ "subagents": { "reproducer": false } }).as_object().cloned().unwrap(), "Hugh (board)").unwrap();
        let (c2, on) = mark_needed(&mut s, &c, "found-a-problem", "once more").unwrap();
        assert!(!on);
        assert_eq!(c2.fields[REPRO_KEY]["reports"].as_array().unwrap().len(), 3, "off changes nothing");
        assert_eq!(s.get(&c.id).unwrap().fields[REPRO_KEY], c.fields[REPRO_KEY]);
        assert!(s.log(&c.id).unwrap().iter().any(|l| l.text.contains("Hugh has the card-reproducer off for this card")));
        // A card made now has it off from the start, and its gate holds nothing.
        let n = s.create(None, json!({ "title": "N", "column": "In progress" }).as_object().cloned().unwrap(), "test").unwrap();
        assert!(!mark_needed(&mut s, &n, "report-a-bug", "x").unwrap().1);
    }

    // Card #151: a stack's card follows its lead's choice of the reproducer, as stack_problem does.
    #[test]
    fn a_stack_card_follows_its_leads_reproducer() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        let p = |v: Value| v.as_object().cloned().unwrap();
        let lead = s.create(None, p(json!({ "title": "L", "column": "In progress" })), "test").unwrap();
        let kid = s.create_in_stack(p(json!({ "title": "K", "column": "In progress", "subagents": { "reproducer": false } })), lead.number, "test").unwrap();
        // Lead on, child off: the stack's line, never "off".
        assert!(!mark_needed(&mut s, &kid, "found-a-problem", "x").unwrap().1);
        let log: Vec<String> = s.log(&kid.id).unwrap().into_iter().map(|l| l.text).collect();
        assert!(log.iter().any(|l| l.contains("this is a stack's card")) && !log.iter().any(|l| l.contains("off")), "{log:?}");
        // Lead off: the child's log says so.
        let lead = s.get(&lead.id).unwrap();
        s.update(&lead.id, lead.version, &p(json!({ "subagents": { "reproducer": false } })), "Hugh (board)").unwrap();
        let kid = s.get(&kid.id).unwrap();
        assert!(!mark_needed(&mut s, &kid, "found-a-problem", "y").unwrap().1);
        let n = lead.number;
        assert!(s.log(&kid.id).unwrap().iter().any(|l| l.text.contains(&format!("its stack's lead #{n} has the card-reproducer off"))));
    }

    #[test]
    fn report_a_bug_is_for_a_card_being_built() {
        let c = |col: &str| card(json!({ "title": "T" }), col);
        assert!(report_refused(&c("In progress"), "the chip is wrong").is_none());
        assert!(report_refused(&c("In progress"), "  ").unwrap().contains("say what the bug is"));
        assert!(report_refused(&c("Human test"), "x").unwrap().contains("through Fail"));
        for col in ["Backlog", "Planning", "Blocked", "Ship it", "Done"] {
            assert!(report_refused(&c(col), "x").is_some(), "{col}");
        }
        assert!(report_refused(&card(json!({ "title": "T", "stackOf": 4 }), "In progress"), "x").unwrap().contains("Found a problem"));
        let mut arch = c("In progress");
        arch.archived = true;
        assert!(report_refused(&arch, "x").is_some());
        let m = bug_message(9, " the chip is wrong ", true);
        assert!(m.starts_with("Hugh reported a bug on card #9 (Report a bug):\n\nthe chip is wrong\n\n") && m.contains("subagent_type \"card-reproducer\""), "{m}");
        assert!(!bug_message(9, "x", false).contains("card-reproducer"));
    }

    #[test]
    fn each_source_is_named_and_recorded_as_its_own() {
        assert_eq!(source_label("found-a-problem"), ("Hugh's Found a problem".to_string(), crate::HUGH));
        assert_eq!(source_label("human-test-fail").1, crate::HUGH);
        assert_eq!(source_label("report-a-bug").1, crate::HUGH);
        assert_eq!(source_label("card-tester"), ("the card-tester's FAIL".to_string(), BY));
        assert_eq!(source_label("card-reviewer").0, "the card-reviewer's FAIL");
        assert!(ask_note(7).contains("card #7") && ask_note(7).contains("subagent_type \"card-reproducer\""));
    }

    #[test]
    fn a_shot_is_kept_only_if_it_is_a_local_image() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("shot.png");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\n").unwrap();
        let data = dir.path().join("data");
        let kept = copy_shot(&data, "card-9", "2026-11-06T02:00:00.000Z", "a1", &png.display().to_string()).unwrap();
        assert!(kept.starts_with(data.join("repro").join("card-9")) && kept.exists());
        assert_eq!(kept.extension().unwrap(), "png");
        let txt = dir.path().join("notes.txt");
        std::fs::write(&txt, b"x").unwrap();
        assert!(copy_shot(&data, "card-9", "t", "a", &txt.display().to_string()).is_err());
        assert!(copy_shot(&data, "card-9", "t", "a", "shot.png").is_err());
        assert!(copy_shot(&data, "card-9", "t", "a", r"\\host\share\a.png").is_err());
        assert!(copy_shot(&data, "card-9", "t", "a", &dir.path().join("gone.png").display().to_string()).is_err());
    }

    #[test]
    fn the_prompt_says_how_to_prove_the_fix() {
        assert!(PROMPT.contains("git merge-base --is-ancestor") && PROMPT.contains("update_status") && PROMPT.contains("the test app isn't on the fix"));
        assert!(PROMPT.contains("VERDICT: REPRODUCED, NOT REPRODUCED, FIXED or UNCLEAR") && PROMPT.contains("STEPS:") && PROMPT.contains("SHOT:") && PROMPT.contains("TEST:"));
        assert!(PROMPT.contains("CDP_PORT=9334") && PROMPT.contains("Never use port 9333") && PROMPT.contains("--background"));
        assert_eq!(AGENT.effort, None);
        assert!(!AGENT.edits);
    }
}
