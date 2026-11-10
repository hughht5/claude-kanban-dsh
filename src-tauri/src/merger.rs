//! The card-merger (Hugh, card #100; card #162): every card session gets a `card-merger`
//! subagent (`agents.rs`), for merges onto a branch that isn't main. A stack's lead runs it to
//! bring each finished child onto the stack's branch (its "stack" job, card #113, briefed by
//! `stackrun::merge_brief`), and the Router could run it to integrate (card #119). It rebases,
//! resolves the conflicts, reruns the repo's tests and reports PASS or FAIL with the SHAs. It
//! never pushes.
//!
//! It no longer does a ship's merge (card #162): every ship's merge, on every repo, goes
//! through the repo's merge lane (`lane.rs`). Its prompt refuses a ship, and its report on a card
//! in Ship it is not recorded, so it can never overwrite a lane job. The lane's verdict is
//! recorded with this file's `record_of`.
//!
//! The app records its report on the card (`merge`, or `integrate` for the Router's job), with a
//! log line and a chip.
//!
//! **Its pushes are refused by the shell guard (`guard.rs`), not a deny list:** a rule like
//! `Bash(git push:*)` in an agent's `disallowedTools` takes the whole tool away. `push_in` is
//! the merger's own rule there: `git ... push`, `gh pr merge` and `gh api`. A shell still runs
//! anything in a form the guard doesn't read, so that is a guard rail too: its report gives
//! origin's SHA before and after, and the app flags a report where origin moved.

use crate::agents::{parse_verdict, target_card, tool_calls, Agent, OnStop};
use crate::guard::{commands, invocation, subcommand};
use crate::Board;
use kanban_store::{Patch, MERGE_KEY};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

/// The subagent's name, as the session calls it (`subagent_type`) and the hook reports it.
pub const MERGER: &str = "card-merger";
/// Who its reports are recorded as.
pub const BY: &str = "the app (card-merger subagent)";

const DESCRIPTION: &str = "Brings one finished child of a stack onto the stack's branch on Hugh's Claude Kanban DSH board (its \"stack\" job, card #113): run it as a stack's lead when the app's message says so, and wait for its result. Brief it with the job, the child's card number, the lead's worktree, the stack's branch and the child's head SHA. It rebases, resolves conflicts and reruns the tests. It never pushes, and never does a ship's merge: every ship goes through its repo's merge lane (kanban_merge). It ends with CARD, VERDICT, HEAD and ORIGIN lines that the app records on the card.";

const PROMPT: &str = "You are card-merger. On Hugh's Claude Kanban DSH board you bring finished work onto a branch that isn't main. The session that runs you briefs you. You never push. You never do a ship's merge: every ship's merge goes through its repo's merge lane (card #162).

There are two jobs. The brief names the job. If it names no job, the job is \"stack\".
- Job \"stack\" (card #113): a stack's lead briefs you to bring one finished child of the stack onto the stack's branch. Do steps 1 to 6, then the STACK JOB below.
- Job \"integrate\" (card #119): the Router briefs you (MODE: integrate). Bring one card's finished branch onto the integration branch. Do the INTEGRATE JOB below only.
If the brief asks for a ship (a card in Ship it, or a branch to bring up to date with origin/main before a push), stop. Change nothing. Report VERDICT: FAIL with the line \"- ships go through the repo's merge lane (kanban_merge), not the card-merger\".

1. Read the card. Use kanban_get_card with the card number from the brief. The card must be a child of a stack (kanban_stack_list), in the column \"Human test\", with its test and its review met (card #176). A test is met when test.state is \"passed\", or when the card's subagents.tester is false: Hugh switched the card-tester off for it. A review is met when review.state is \"passed\", or when the card's subagents.reviewer is false. A switched-off subagent counts as met, as the app's own checks count it. If not, stop. Change nothing. Report VERDICT: FAIL with the reason.
2. Go to the worktree that the brief names. If git status --porcelain shows work that is not committed, stop. Change nothing. Report FAIL and name the files.
3. Resolve each conflict. Keep the work of both sides. Use the repo's own merge rules: the rules in your brief, else the rules in the repo's CLAUDE.md or AGENTS.md (for example a version that is the base's version plus one, or a changelog where you keep both sides, newest first). Apply a version rule also when there is no conflict.
   - If you cannot resolve a conflict safely, stop and report FAIL. Name the file.
   Commit every change that you make. A change that is not part of a rebase (for example the version) is its own commit, with \"(card #<n>)\" in its message. Before you report PASS, git status --porcelain must be clean in the worktree.
4. Run the repo's checks that do not need a dev stack:
   - The commands that your brief names, if it names them.
   - This kanban app's own repo: cargo test --workspace (first export PATH=\"$HOME/.cargo/bin:$PATH\").
   - Another repo: the test command in its README or CLAUDE.md.
   A check that fails is a FAIL. Give the command and the error.
5. If the result is FAIL: run git rebase --abort if a rebase is in progress. Then put the branch you changed back to the SHA you recorded (git checkout <branch>, then git reset --hard <sha>). Before each git reset --hard, run git status --porcelain again. If it shows a file that you did not change, another person works in that checkout: do not reset. Report FAIL and say that the checkout needs Hugh. Leave the checkout as it was.
6. Do not push. Do not run git push, gh pr merge or gh api (the app refuses them). Do not merge a PR. Never change the branch main. Do not change or move a card. Do not run kb or claude config. Do not install, quit or restart an app.

STACK JOB. The brief names the lead's worktree, the stack's branch and the child's head SHA.
   a. In the lead's worktree, do step 2. Then run git fetch origin, and record HEAD of the stack's branch and origin/<the stack's branch> (\"none\" if it does not exist).
   b. Run git checkout --detach <the child's head SHA>. Then rebase it onto the stack's branch: git rebase <the stack's branch>. Do not make merge commits. Resolve conflicts as in step 3, and commit as step 3 says.
   c. Fast-forward the stack's branch to the result: git checkout <the stack's branch>, then git merge --ff-only <the rebased SHA>.
   d. Do not check out, rebase or reset the child's own branch. Do not change main. Do not push.
   e. Run the checks of step 4. If the result is FAIL, do step 5 for the stack's branch (back to its HEAD from a).
   f. Run git fetch origin again, and record origin/<the stack's branch> again. Report as below: CARD is the child's number, HEAD is the stack's branch, ORIGIN is origin/<the stack's branch> before (from a) and after.

INTEGRATE JOB. The brief names the card, the card's branch, the integration branch and the integration worktree.
   a. Read the card (kanban_get_card). Its test and its review must be met, as step 1 says (a subagent switched off for the card counts as met), and it must not be in Ship it or Deploy complete. If not, stop. Change nothing. Report VERDICT: FAIL with the reason.
   b. Go to the integration worktree. Do step 2. Make the lock: mkdir \"$(git rev-parse --git-common-dir)/kanban-integrate.lock\". If the folder is there already, another merge runs: stop and report FAIL. Remove the lock (rmdir) before you report, PASS or FAIL.
   c. git checkout <the integration branch>. Record its HEAD SHA, and origin/<the integration branch> (\"none\" if it does not exist).
   d. git merge --no-ff <the card's branch> -m \"Integrate <the card's branch> (card #<n>)\". In this job merge commits are allowed. Resolve conflicts as in step 3. If you cannot resolve a conflict safely, run git merge --abort and report FAIL. Name the file.
   e. Never change main. Never rebase. Never push.
   f. Run cargo test --workspace -j 4 (first export PATH=\"$HOME/.cargo/bin:$PATH\"). If a test fails, report FAIL with the first failure. Then, after the check of step 5, git reset --hard <the HEAD from c>.
   g. Report as below, with one more line before CARD: MODE: integrate. Also add a line TESTS: pass, or TESTS: fail <the first failure>. HEAD is the integration branch. ORIGIN is origin/<the integration branch> before and after.

Report what you did, each conflict and how you resolved it, and each check with its result. Then end your reply with these lines, and nothing after them:
CARD: #<the card's number>
VERDICT: PASS
HEAD: <the SHA of the branch you brought the work onto, now>
ORIGIN: <origin/<that branch> before> <origin/<that branch> after>
- <one line for each thing you changed: a rebase, a conflict and how you resolved it, a version change>
If the result is not a pass, write VERDICT: FAIL, and make each \"- \" line a thing that failed. The app reads these lines to record the merge on the card.";

pub const AGENT: Agent = Agent {
    name: MERGER,
    description: DESCRIPTION,
    prompt: PROMPT,
    // The router's call (6 Nov 2026, card #113): it runs at high, whatever the session's effort
    // (a stack lead's session runs at low).
    effort: Some("high"),
    // It resolves conflicts, so it edits files.
    edits: true,
    // Nothing more: a command rule here would take its shell away (see the top). `guard`
    // refuses its pushes instead.
    also_denied: &[],
    on_stop: OnStop::Custom(on_stop),
};

/// git's options that take the next word as their value (lower case: `-C` and `-c` both).
const GIT_VALUED: [&str; 6] = ["-c", "--git-dir", "--work-tree", "--namespace", "--config-env", "--exec-path"];
/// gh's global options that take a value (`-R` lower-cased).
const GH_VALUED: [&str; 3] = ["-r", "--repo", "--hostname"];

/// What push a shell command makes, if any: git's subcommand `push` (past its options, as in
/// `git -C <dir> push`; through `sh -c "..."`, `$( )` or a full path to git.exe), `gh api` or
/// `gh pr merge` (past `-R <repo>`). Words inside quotes are a command's argument, so a commit
/// message that says "push" is not a push.
pub fn push_in(command: &str) -> Option<&'static str> {
    for words in commands(&command.to_ascii_lowercase()) {
        match invocation(&words) {
            Some(("git", rest)) if subcommand(rest, &GIT_VALUED).map(|(_, s)| s) == Some("push") => return Some("git push"),
            Some(("gh", rest)) => match subcommand(rest, &GH_VALUED) {
                Some((_, "api")) => return Some("gh api"),
                Some((j, "pr")) if subcommand(&rest[j + 1..], &GH_VALUED).map(|(_, s)| s) == Some("merge") => return Some("gh pr merge"),
                _ => {}
            },
            _ => {}
        }
    }
    None
}

/// The card field of the integrate job's record (card #119), apart from the ship's `merge`.
pub const INTEGRATE_KEY: &str = "integrate";

/// Whether a report is from the integrate job: a "MODE: integrate" line.
pub fn is_integrate(text: &str) -> bool {
    text.lines().any(|l| {
        let l = l.replace(['*', '`'], "").trim().to_ascii_lowercase();
        l.strip_prefix("mode:").is_some_and(|m| m.trim().trim_matches(|c| c == '*' || c == '`') == "integrate")
    })
}

/// Whether a report's TESTS line says the tests failed.
pub fn tests_failed(text: &str) -> bool {
    text.lines().any(|l| {
        let l = l.replace(['*', '`'], "").trim().to_ascii_lowercase();
        l.strip_prefix("tests:").is_some_and(|t| t.trim().starts_with("fail"))
    })
}

/// A merger's report: its verdict, the SHAs and the "- " lines.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub card: Option<i64>,
    pub passed: Option<bool>,
    pub head: Option<String>,
    /// origin/<branch> before and after ("none" when it didn't exist).
    pub origin: Option<(String, String)>,
    pub lines: Vec<String>,
}

/// A commit SHA (7 to 64 hex digits), lower case.
pub(crate) fn sha(s: &str) -> Option<String> {
    let s = s.trim().trim_matches(|c| c == '`' || c == '*' || c == '<' || c == '>');
    (s.len() >= 7 && s.len() <= 64 && s.chars().all(|c| c.is_ascii_hexdigit())).then(|| s.to_ascii_lowercase())
}

/// A SHA, or "none" for a branch origin doesn't have yet.
pub(crate) fn sha_or_none(s: &str) -> Option<String> {
    let t = s.trim().trim_matches(|c| c == '`' || c == '*' || c == '"');
    if t.eq_ignore_ascii_case("none") {
        Some("none".into())
    } else {
        sha(t)
    }
}

/// Reads the closing lines of a merger's report: CARD and VERDICT as every agent's, the last
/// `HEAD: <sha>` and `ORIGIN: <before> <after>`, and the `- ` lines after the VERDICT line.
pub fn parse_report(text: &str) -> Report {
    let v = parse_verdict(text);
    let mut r = Report { card: v.card, passed: v.passed, head: None, origin: None, lines: vec![] };
    for raw in text.lines() {
        let l = raw.trim().trim_matches(|c| c == '*' || c == '`' || c == '_').trim();
        let up = l.to_ascii_uppercase();
        if up.starts_with("VERDICT:") {
            r.lines.clear();
        } else if up.starts_with("HEAD:") {
            r.head = sha(&l[5..]);
        } else if up.starts_with("ORIGIN:") {
            // "<before> <after>", with anything around or between them ("->", "→", "(before)"):
            // the line's SHAs and "none"s, in order. It counts only when there are two.
            let parts: Vec<String> = l[7..].split(|c: char| !c.is_ascii_alphanumeric()).filter_map(sha_or_none).collect();
            r.origin = match parts.as_slice() {
                [a, b] => Some((a.clone(), b.clone())),
                _ => None,
            };
        } else if let Some(x) = raw.trim().strip_prefix("- ").or_else(|| raw.trim().strip_prefix("* ")) {
            r.lines.push(x.trim().to_string());
        }
    }
    r
}

/// What the app records for a report: the `merge` field and the log line. A PASS counts only
/// with tools run, a HEAD, and origin where it was: else it is "unclear", never a pass.
pub fn record_of(r: &Report, tools: Option<usize>, agent_id: &str, at: &str) -> (Value, String) {
    let changed = r.lines.join("; ");
    let (state, detail) = match (r.passed, moved(r)) {
        // A push while it ran is reported whatever its verdict (or none): the session must not
        // trust it.
        (_, Some((a, b))) => ("unclear", format!("origin moved while the merger ran ({a} to {b}): something pushed. The merger may not push, and only one card ships at a time per repo")),
        (Some(true), None) if tools == Some(0) => ("unclear", "the merger said PASS but ran no tools, so it didn't merge anything".to_string()),
        (Some(true), None) if r.head.is_none() => ("unclear", "the merger said PASS with no HEAD line, so there is no commit to push".to_string()),
        (Some(true), None) if r.origin.is_none() => ("unclear", "the merger said PASS with no ORIGIN line, so nobody can tell whether it pushed".to_string()),
        (Some(true), None) => ("passed", changed),
        (Some(false), None) => ("failed", if changed.is_empty() { "see the merger's report in the session".into() } else { changed }),
        (None, _) => ("unclear", "the merger ended with no VERDICT line, so it isn't a pass".to_string()),
    };
    // What it quotes never reaches the card if it is a secret (the router's rule).
    let detail: String = kanban_store::mask_secrets(&detail).chars().take(600).collect();
    let line = match state {
        "passed" => format!("Merger: PASS{}{}", r.head.as_ref().map(|h| format!(" at {}", short(h))).unwrap_or_default(), if detail.is_empty() { String::new() } else { format!(": {detail}") }),
        "failed" => format!("Merger: FAIL: {detail}"),
        _ => format!("Merger unclear: {detail}"),
    };
    let line: String = line.chars().take(700).collect();
    let mut rec = json!({ "state": state, "at": at, "agentId": agent_id });
    if !detail.is_empty() {
        rec["detail"] = json!(detail);
    }
    if let Some(h) = &r.head {
        rec["head"] = json!(h);
    }
    if let Some((a, b)) = &r.origin {
        rec["origin"] = json!({ "before": a, "after": b });
    }
    (rec, line)
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

/// origin's SHAs before and after, when they differ: something pushed while the merger ran.
fn moved(r: &Report) -> Option<(&str, &str)> {
    r.origin.as_ref().filter(|(a, b)| a != b).map(|(a, b)| (a.as_str(), b.as_str()))
}

/// Why a card-merger's report on a card in `column` with lane-job state `merge` isn't recorded
/// (card #162): a ship's merge is its lane's, so a card in Ship it, or with a lane job waiting
/// or running, keeps its `merge` record as the lane writes it.
pub fn not_a_ship(column: &str, merge: Option<&str>) -> Option<String> {
    if column == "Ship it" {
        return Some("a card in Ship it: every ship's merge goes through its repo's merge lane (kanban_merge)".into());
    }
    merge.filter(|m| kanban_store::MERGE_WAITING.contains(m)).map(|m| format!("a card whose merge lane job is {m}"))
}

/// A card-merger finished in session `row_id`: its report goes on the card it names (the
/// session's own card or one in its stack). An origin that moved also tells Hugh.
fn on_stop(app: &AppHandle, row_id: &str, body: &Value) {
    let text = body.get("last_assistant_message").and_then(Value::as_str).unwrap_or("");
    let agent_id = body.get("agent_id").and_then(Value::as_str).unwrap_or("");
    let tools = body.get("agent_transcript_path").and_then(Value::as_str).and_then(tool_calls);
    let board = app.state::<Board>();
    let Ok(mut store) = board.0.lock() else { return };
    let r = parse_report(text);
    let Some(own) = crate::agents::own_or_named(app, &store, row_id, r.card) else { return };
    let card = match target_card(&store, &own, r.card) {
        Ok(c) => c,
        Err(why) => {
            let _ = store.add_log(&own.id, &format!("Not recorded: the card-merger subagent reported on {why}"), BY);
            return;
        }
    };
    let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let (mut rec, mut line) = record_of(&r, tools, agent_id, &at);
    // The Router's integrate job (card #119) has its own field, so the ship record and its chip
    // are never touched. A failed test run is a FAIL whatever the verdict line says.
    let integrate = is_integrate(text);
    if integrate {
        if tests_failed(text) && rec["state"] == "passed" {
            rec["state"] = json!("failed");
        }
        line = format!("Integrate: {line}");
    }
    // Card #162: a ship's merge is its lane's. A report on a card in Ship it, or on one whose
    // lane job waits or runs, would overwrite that job, so it isn't recorded.
    if let Some(why) = (!integrate).then(|| not_a_ship(&card.column, kanban_store::merge_state(&card))).flatten() {
        let _ = store.add_log(&card.id, &format!("Not recorded: the card-merger subagent reported on {why}"), BY);
        return;
    }
    let mut patch = Patch::new();
    patch.insert(if integrate { INTEGRATE_KEY } else { MERGE_KEY }.into(), rec.clone());
    if let Err(e) = store.update(&card.id, card.version, &patch, BY) {
        let _ = store.add_log(&card.id, &format!("Couldn't record the card-merger subagent's report: {e}"), BY);
        return;
    }
    let _ = store.add_log(&card.id, &line, BY);
    // A stack child's merge (the stack job, card #113), PASS or FAIL: the stack's rules go on.
    if card.id != own.id && card.stack_of().is_some() && !integrate {
        after_child_merge(app, &mut store, &card.id);
    }
    let pushed = moved(&r).is_some();
    drop(store);
    if pushed {
        let _ = app.emit("notify", json!({ "card": card.id, "number": card.number, "text": line }));
    }
    let _ = app.emit("cards-changed", ());
}

/// Where card #113's stack rules take over after a stack child's merge is recorded. #113 writes
/// `crate::dispatch::after_child_merge(app, store, child_id)`: when its branch and this one are
/// merged, this body becomes that one call. Until then a child's merge is only recorded.
fn after_child_merge(_app: &AppHandle, _store: &mut kanban_store::Store, _child_id: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b";
    const B: &str = "0f9e8d7c6b5a4f3e2d1c0b9a8f7e6d5c4b3a2f1e";

    #[test]
    fn a_report_gives_its_verdict_shas_and_changes() {
        let text = format!("Rebased onto origin/main.\n- not this line\n\n**CARD:** #100\n**VERDICT: PASS**\nHEAD: `{A}`\nORIGIN: {B} {B}\n- rebased dsh-100 onto origin/main (3 commits)\n- server/package.json: 3.202.1 (main's 3.202.0 plus one)\n");
        let r = parse_report(&text);
        assert_eq!(r.card, Some(100));
        assert_eq!(r.passed, Some(true));
        assert_eq!(r.head.as_deref(), Some(A));
        assert_eq!(r.origin, Some((B.to_string(), B.to_string())));
        assert_eq!(r.lines, vec!["rebased dsh-100 onto origin/main (3 commits)", "server/package.json: 3.202.1 (main's 3.202.0 plus one)"]);
        // An arrow between the SHAs, a branch origin didn't have, upper case and short SHAs.
        let r = parse_report("CARD: 5\nVERDICT: PASS\nHEAD: ABC1234\nORIGIN: none -> abc1234");
        assert_eq!((r.head.as_deref(), r.origin), (Some("abc1234"), Some(("none".into(), "abc1234".into()))));
        for line in ["ORIGIN: abc1234 → def5678", "ORIGIN: abc1234→def5678", "ORIGIN: abc1234 (before) def5678 (after)", "**ORIGIN:** `abc1234` -> `def5678`"] {
            assert_eq!(parse_report(&format!("VERDICT: PASS\n{line}")).origin, Some(("abc1234".into(), "def5678".into())), "{line}");
        }
        // Not SHAs: no HEAD, no ORIGIN. One SHA, or three, isn't a before and after.
        let r = parse_report("VERDICT: PASS\nHEAD: none\nORIGIN: main origin/main");
        assert_eq!((r.head, r.origin), (None, None));
        assert_eq!(parse_report("VERDICT: PASS\nORIGIN: abc1234").origin, None);
        assert_eq!(parse_report("VERDICT: PASS\nORIGIN: abc1234 def5678 0123456").origin, None);
        // A FAIL's lines are what failed.
        let r = parse_report("CARD: #7\nVERDICT: FAIL\n- vault/Bugs.md: conflict I couldn't resolve safely\n- cargo test: 2 failed");
        assert_eq!((r.passed, r.lines.len()), (Some(false), 2));
    }

    fn pass() -> Report {
        Report { card: Some(1), passed: Some(true), head: Some(A.into()), origin: Some((B.into(), B.into())), lines: vec!["rebased".into()] }
    }

    #[test]
    fn only_a_full_report_is_a_pass() {
        let (rec, line) = record_of(&pass(), Some(12), "a1", "t");
        assert_eq!(rec, json!({ "state": "passed", "at": "t", "agentId": "a1", "detail": "rebased", "head": A, "origin": { "before": B, "after": B } }));
        assert_eq!(line, "Merger: PASS at 1a2b3c4: rebased");
        assert_eq!(record_of(&pass(), None, "a1", "t").0["state"], "passed"); // transcript unreadable: trust it
        let unclear = |r: Report, tools: Option<usize>, says: &str| {
            let (rec, line) = record_of(&r, tools, "a1", "t");
            assert_eq!(rec["state"], "unclear", "{says}");
            assert!(rec["detail"].as_str().unwrap().contains(says), "{rec}");
            assert!(line.starts_with("Merger unclear: "), "{line}");
        };
        unclear(pass(), Some(0), "ran no tools");
        unclear(Report { head: None, ..pass() }, Some(3), "no HEAD line");
        unclear(Report { origin: None, ..pass() }, Some(3), "no ORIGIN line");
        unclear(Report { passed: None, ..pass() }, Some(3), "no VERDICT line");
        // origin moved: something pushed, whatever the verdict says.
        unclear(Report { origin: Some((B.into(), A.into())), ..pass() }, Some(3), "origin moved");
        unclear(Report { passed: Some(false), origin: Some(("none".into(), A.into())), ..pass() }, Some(3), "origin moved");
        unclear(Report { passed: None, origin: Some((B.into(), A.into())), ..pass() }, Some(3), "origin moved");
        let (rec, line) = record_of(&Report { passed: Some(false), lines: vec!["a".into(), "b".into()], ..pass() }, Some(3), "a1", "t");
        assert_eq!((rec["state"].as_str(), rec["detail"].as_str()), (Some("failed"), Some("a; b")));
        assert_eq!(line, "Merger: FAIL: a; b");
        let (rec, _) = record_of(&Report { passed: Some(false), lines: vec![], head: None, origin: None, card: None }, Some(3), "a1", "t");
        assert_eq!(rec["detail"], "see the merger's report in the session");
        assert!(rec.get("head").is_none() && rec.get("origin").is_none());
    }

    // The end of a real merger's report, from the live trial on a scratch repo (6 Nov 2026).
    #[test]
    fn a_real_reports_lines_are_read() {
        let text = "**After:** a second `git fetch origin` shows origin/card-1 still at a382d88, so it must be force-pushed.\n\nCARD: #1\nVERDICT: PASS\nHEAD: 5a97225d07ce2254f32657234db7a9c72ebb4b92\nORIGIN: a382d887041811cdea93fefa9edb5f558f0678ec a382d887041811cdea93fefa9edb5f558f0678ec\n- rebased card-1 onto origin/main (cf28975): a382d88 -> 379e54c\n- conflict in app.txt resolved: main's size (size = 2) and the card's colour (colour = green)\n- conflict in Changelog.md resolved: kept both entries, newest first (1.1.0 card #1, then 1.0.1, then 1.0.0)\n- rebased drop-2 (card #2) onto card-1: 84096f1 -> 5a97225, no conflicts\n- fast-forwarded card-1 to drop-2 (5a97225)\n";
        let r = parse_report(text);
        assert_eq!((r.card, r.passed, r.lines.len()), (Some(1), Some(true), 5));
        let (rec, line) = record_of(&r, Some(30), "a1", "t");
        assert_eq!(rec["state"], "passed");
        assert_eq!(rec["head"], "5a97225d07ce2254f32657234db7a9c72ebb4b92");
        assert!(line.starts_with("Merger: PASS at 5a97225: rebased card-1 onto origin/main"), "{line}");
    }

    #[test]
    fn the_merger_edits_but_may_not_push_or_change_cards() {
        let v: Value = serde_json::from_str(&crate::agents::agents_json()).unwrap();
        let d = &v[MERGER];
        assert_eq!(d["model"], "inherit");
        assert_eq!(d["effort"], "high", "the router's call: the merger runs at high");
        let denied: Vec<&str> = d["disallowedTools"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        for tool in ["mcp__kanban__kanban_ship_result", "mcp__kanban-app__kanban_ship_result", "mcp__kanban__kanban_move_card", "mcp__kanban-app__kanban_update_card"] {
            assert!(denied.contains(&tool), "the merger may use {tool}");
        }
        for tool in ["Edit", "Write"] {
            assert!(!denied.contains(&tool), "the merger can't resolve conflicts without {tool}");
        }
        // No rule of its own names a shell: one would take that whole shell away, and it needs
        // one for git. Its pushes are refused by `guard` instead.
        assert!(AGENT.also_denied.is_empty());
        assert!(!denied.iter().any(|t| t.starts_with("PowerShell")), "the merger would have no PowerShell");
        // It reads the card and its stack.
        assert!(!denied.iter().any(|t| t.ends_with("kanban_get_card") || t.ends_with("kanban_stack_list")));
        assert_eq!(crate::agents::find(MERGER).map(|a| a.name), Some(MERGER));
    }

    #[test]
    fn every_way_of_pushing_is_seen() {
        for cmd in [
            "git push origin card-1",
            "git push --force-with-lease origin dsh-100",
            "git -C C:/Users/x/work push origin card-1:fence-2",
            "cd C:/Users/x/work && git push origin card-1",
            "cd C:/Users/x/work; git push",
            "sh -c \"git push origin card-1\"",
            "bash -lc 'cd ~/web && git push'",
            "echo $(git push)",
            "& \"C:\\Program Files\\Git\\cmd\\git.exe\" push",
            "GIT_TRACE=1 git -c core.askPass= push",
            "git status\ngit push",
            "git -C \"C:/Program Files/x\" push",
            "git --git-dir=x/.git push",
            "powershell -Command \"git push origin main\"",
        ] {
            assert_eq!(push_in(cmd), Some("git push"), "{cmd}");
        }
        assert_eq!(push_in("gh api repos/hughht5/kanban-app/merges -f base=main"), Some("gh api"));
        assert_eq!(push_in("gh -R hughht5/kanban-app api user"), Some("gh api"));
        assert_eq!(push_in("gh pr merge 12 --merge"), Some("gh pr merge"));
        assert_eq!(push_in("cd x && gh  pr  merge --squash"), Some("gh pr merge"));
        assert_eq!(push_in("gh --repo x pr merge 12"), Some("gh pr merge"));
        assert_eq!(push_in("gh pr -R x merge 12"), Some("gh pr merge"));
        // The merger's own work runs: a word "push" that isn't git's subcommand is no push.
        for cmd in [
            "git fetch origin",
            "git rebase origin/main",
            "git rebase origin/main push-fix",
            "git status --porcelain",
            "git log --oneline origin/main..HEAD",
            "git log --grep push",
            "git commit -m \"Bump the version so the push lands (card #100)\"",
            "git commit -am 'push notes (card #1)'",
            "gh pr view 12 --json mergeable",
            "gh pr list --search merge",
            "cargo test --workspace",
            "git stash push -m x",
            "echo push",
            "git reset --hard abc1234",
            "git -C push status",
        ] {
            assert_eq!(push_in(cmd), None, "{cmd}");
        }
    }

    #[test]
    fn its_prompt_asks_for_the_lines_the_app_reads() {
        for needed in ["CARD:", "VERDICT: PASS", "HEAD:", "ORIGIN:", "git rebase --abort", "(card #<n>)", "plus one", "kanban_stack_list", "You never push", "Commit every change", "do not reset", "STACK JOB", "Job \"stack\"", "git checkout --detach", "git merge --ff-only", "Do not check out, rebase or reset the child's own branch"] {
            assert!(PROMPT.contains(needed), "the prompt doesn't say {needed}");
        }
    }

    /// Card #176: the merger's rule matches the app's own checks. A child whose reviewer (or
    /// tester) is switched off for the card counts as met, in both jobs; the old rule that
    /// asked for review.state "passed" alone is gone.
    #[test]
    fn a_switched_off_reviewer_or_tester_counts_as_met() {
        for must in ["subagents.reviewer is false", "subagents.tester is false", "counts as met", "review.state is \"passed\"", "test.state is \"passed\""] {
            assert!(PROMPT.contains(must), "the prompt doesn't say {must}");
        }
        assert!(!PROMPT.contains("review.state \"passed\". If not"), "the old rule is still there");
        assert!(!PROMPT.contains("test.state and review.state must be \"passed\""), "the integrate job's old rule is still there");
        let integrate = &PROMPT[PROMPT.find("INTEGRATE JOB.").unwrap()..];
        assert!(integrate.contains("switched off for the card counts as met"));
    }

    /// Card #162: the card-merger no longer does a ship. Its description and prompt say so and
    /// never speak of ship time; it refuses a ship's brief; and its report on a card in Ship it,
    /// or on one with a lane job waiting or running, isn't recorded.
    #[test]
    fn the_merger_does_no_ship() {
        for t in [DESCRIPTION, PROMPT] {
            assert!(!t.contains("at ship time") && !t.contains("before the push") && !t.contains("Job \"ship\""), "{t}");
            assert!(t.contains("merge lane"), "{t}");
        }
        assert!(DESCRIPTION.contains("stack") && !DESCRIPTION.contains("Run it in Ship it"));
        assert!(PROMPT.contains("If the brief asks for a ship") && PROMPT.contains("not the card-merger"));
        assert!(!PROMPT.contains("SHIP OF AN INTEGRATION BRANCH"));
        assert!(not_a_ship("Ship it", None).is_some_and(|w| w.contains("merge lane")));
        for st in ["queued", "running"] {
            assert!(not_a_ship("Human test", Some(st)).is_some(), "{st}");
        }
        for st in [None, Some("passed"), Some("failed"), Some("cancelled")] {
            assert_eq!(not_a_ship("Human test", st), None, "a stack child's merge is recorded: {st:?}");
        }
    }

    #[test]
    fn a_secret_in_the_report_never_reaches_the_card() {
        let r = Report { passed: Some(false), lines: vec!["the check called the API with Authorization: Bearer abc.DEF-123_x and got 401".into()], ..pass() };
        let (rec, line) = record_of(&r, Some(3), "a1", "t");
        for text in [rec["detail"].as_str().unwrap(), line.as_str()] {
            assert!(!text.contains("abc.DEF-123_x"), "{text}");
            assert!(text.contains("[hidden]"), "{text}");
        }
    }

    /// Cards #160, #162: the merge lane's step is both ship prompts' only merge step: ask with
    /// kanban_merge (needs-hugh when it finds no checkout), and on a PASS check the branch, reset
    /// to the lane's commit and check the range-diff before the push (or main's fast-forward).
    /// No card-merger, no condition on the app's note, no rebase of the session's own.
    #[test]
    fn both_ship_prompts_merge_only_through_the_lane() {
        let ship_it = include_str!("../../prompts/ship-it.md");
        let kanban_app = include_str!("../../prompts/kanban-app.md");
        for (name, text) in [("ship-it.md", ship_it), ("kanban-app.md", kanban_app)] {
            let plain = text.replace(['*', '`'], "").to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
            for must in ["kanban_merge", "git range-diff", "git reset --hard", "git status --porcelain", "git rev-parse HEAD", "needs-hugh"] {
                assert!(text.contains(must), "{name}'s lane step doesn't say {must}");
            }
            for must in ["every ship's merge goes through", "merge lane", "found no checkout for the repo", "don't rebase or merge the branch yourself"] {
                assert!(plain.contains(must), "{name} doesn't say {must}");
            }
            assert!(!text.contains("card-merger"), "{name} still names the card-merger");
            assert!(!plain.contains("if the app's note says this repo ships through its merge lane"), "{name}'s lane step is still conditional");
            assert!(!plain.contains("git rebase origin/main"), "{name} still has the session rebase itself");
        }
        assert!(ship_it.contains("--force-with-lease"));
        assert!(ship_it.replace(['*', '`'], "").contains("If the card has no branch"), "ship-it.md has a no-branch path");
        assert!(kanban_app.contains("--ff-only"));
    }

    /// Card #160: the lane's own rules say it never pushes and works detached, and never moves a
    /// branch, so a FAIL leaves every branch where it was.
    #[test]
    fn the_merge_lane_rules_never_push_and_work_detached() {
        let lane = include_str!("../../prompts/merge-lane.md");
        let plain = lane.replace(['*', '`'], "");
        for must in ["Never push", "detached", "git checkout --detach", "git branch -f", "update-ref", "kanban_merge_result"] {
            assert!(plain.contains(must), "merge-lane.md doesn't say {must}");
        }
    }

    #[test]
    fn the_integrate_job_is_its_own_mode_and_record() {
        assert!(is_integrate("did it\nMODE: integrate\nCARD: #4\nVERDICT: PASS"));
        assert!(is_integrate("**MODE:** `integrate`"));
        assert!(!is_integrate("CARD: #4\nVERDICT: PASS"));
        assert!(tests_failed("TESTS: fail agents::x panicked"));
        assert!(!tests_failed("TESTS: pass"));
        assert_ne!(INTEGRATE_KEY, MERGE_KEY, "the ship record is never touched");
        for must in ["INTEGRATE JOB", "merge commits are allowed", "Never change main", "kanban-integrate.lock", "MODE: integrate", "TESTS:"] {
            assert!(PROMPT.contains(must), "{must}");
        }
    }
}
