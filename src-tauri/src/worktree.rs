//! A card's own git worktree (card #146). For the repos Hugh lists in Settings > Sessions
//! ("Build each card in its own worktree", `sessions.own_worktree_repos`), the app makes each
//! card's worktree when the card's first session starts, records it on the card (`worktreePath`,
//! `branch`), and starts the session there. Several cards of one repo then build at once, each
//! on its own branch, and none of them works in the shared checkout.
//!
//! - The folder is `<checkout>\.claude\worktrees\<name>`: inside the checkout, which Claude Code
//!   already trusts, so a session started there is never asked "Do you trust this folder?"
//!   (6 Nov 2026). The app keeps `.claude/worktrees/` out of the checkout's `git status`.
//! - The name, which is also the branch, is `card-<n>` on the live board and `<channel>-card-<n>`
//!   on a test board (`test-card-<n>`, `test3-card-<n>`): a test board's card numbers run ahead
//!   of the live board's, and must never take a live card's branch.
//! - The branch starts from `origin/main` (else `main`) as this machine last saw it. The app
//!   doesn't fetch: the board is locked while a session starts. The ship's merger rebases the
//!   branch onto the latest `origin/main` (card #100).
//! - A stack's cards keep their own flow (card #113), and a card that already has a
//!   `worktreePath` keeps it.

use kanban_store::{Card, OwnWorktreeRepo};
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long all of `make`'s git commands together may take. The board is locked while a session
/// starts, so this caps how long a start can hold it (a worktree of this repo takes well under a
/// second).
const MAKE_LIMIT: Duration = Duration::from_secs(15);

/// Where the app makes card worktrees, under the checkout.
const UNDER: [&str; 2] = [".claude", "worktrees"];

/// The worktree the app makes, or made, for a card.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub repo: String,
    pub checkout: PathBuf,
    pub path: PathBuf,
    pub branch: String,
}

/// What `make` did.
#[derive(Debug, Clone, PartialEq)]
pub struct Made {
    /// "made", "made on its existing branch" or "found".
    pub how: &'static str,
    /// The branch it started from, for a new branch.
    pub base: Option<String>,
    pub head: String,
}

/// The name of card `n`'s worktree and branch on `channel`.
pub fn name(n: i64, channel: &str) -> String {
    match channel.trim() {
        "" | "live" => format!("card-{n}"),
        c => format!("{c}-card-{n}"),
    }
}

fn text<'a>(card: &'a Card, key: &str) -> Option<&'a str> {
    card.fields.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

/// A card the app makes worktrees for, by kind: not archived and not in a stack.
fn eligible(card: &Card) -> bool {
    !card.archived && card.stack_of().is_none() && !card.is_lead()
}

/// The worktree the app should make for `card` as its session starts, if any: a card of a
/// listed repo with no `worktreePath` yet.
pub fn plan_for(card: &Card, list: &[OwnWorktreeRepo], channel: &str) -> Option<Plan> {
    if !eligible(card) || text(card, "worktreePath").is_some() {
        return None;
    }
    let repo = text(card, "repo")?;
    let e = list.iter().find(|r| r.repo.eq_ignore_ascii_case(repo))?;
    let branch = name(card.number, channel);
    let checkout = PathBuf::from(&e.checkout);
    let path = UNDER.iter().fold(checkout.clone(), |p, part| p.join(part)).join(&branch);
    Some(Plan { repo: e.repo.clone(), checkout, path, branch })
}

/// A path for comparing: `/` for `\`, no trailing separator, lower case (the file systems of
/// both Windows and macOS compare names without case by default).
fn norm(s: &str) -> String {
    s.trim().replace('\\', "/").trim_end_matches('/').to_ascii_lowercase()
}

fn same_path(a: &str, b: &str) -> bool {
    norm(a) == norm(b)
}

/// The worktree the app made for `card`, if the card is in it: its `worktreePath` is
/// `<checkout>\.claude\worktrees\<name>` and its branch (if set) is `<name>`. Read from the card
/// alone, so a card already in its worktree keeps its note if Hugh later changes the list.
pub fn made_for(card: &Card, channel: &str) -> Option<Plan> {
    if !eligible(card) {
        return None;
    }
    let wt = text(card, "worktreePath")?;
    let branch = name(card.number, channel);
    if text(card, "branch").is_some_and(|b| b != branch) {
        return None;
    }
    let tail = format!("/{}/{}/{branch}", UNDER[0], UNDER[1]);
    let n = norm(wt);
    if !n.ends_with(&tail) || n.len() == tail.len() {
        return None;
    }
    // Cut the same number of characters off the path as written.
    let written = wt.trim().trim_end_matches(['\\', '/']);
    let checkout = PathBuf::from(&written[..written.len() - tail.len()]);
    Some(Plan { repo: text(card, "repo").unwrap_or_default().to_string(), checkout, path: PathBuf::from(written), branch })
}

/// What a session in its app-made worktree is told with its first message and every move
/// (card #146): where it works, and that this replaces the prompts' shared-checkout rules.
pub fn note(plan: &Plan) -> String {
    let repo = if plan.repo.is_empty() { "this repo".to_string() } else { plan.repo.clone() };
    format!(
        "This card builds in its own git worktree, {wt}, on its own branch {b}. The app made it from {co} (card #146), so other cards of {repo} build at the same time, each in its own. Work, build, test and commit only in {wt}: never in {co} and never on main, and don't change this card's worktreePath or branch. Where a prompt says to work on main in the shared checkout, or that one card of this repo builds at a time, this replaces it. The branch tracks nothing: if your column's prompt says to push, push {b} only (git push -u origin {b}), never main.",
        wt = plan.path.display(),
        b = plan.branch,
        co = plan.checkout.display(),
    )
}

/// What a new session is told when the app couldn't make its worktree (card #146): how to make
/// it, and to work only there.
pub fn failed_note(plan: &Plan, why: &str) -> String {
    format!(
        "This card should build in its own git worktree, but the app couldn't make it ({why}). Make it yourself: git -C \"{co}\" worktree prune, then git -C \"{co}\" worktree add --no-track -b {b} \"{wt}\" origin/main (leave out --no-track -b {b} if that branch is already there). Then record worktreePath and branch on this card, and work, build, test and commit only in {wt}: never in {co} and never on main. If that fails too, say so on the card and stop.",
        co = plan.checkout.display(),
        b = plan.branch,
        wt = plan.path.display(),
    )
}

/// What a new session is told when the app made its worktree but couldn't write it on the card.
pub fn unrecorded_note(plan: &Plan, why: &str) -> String {
    format!(
        "The app made this card's own git worktree, {wt}, on branch {b}, but couldn't record it on the card ({why}). Record worktreePath {wt} and branch {b} on this card yourself, then work, build, test and commit only in {wt}: never in {co} and never on main.",
        wt = plan.path.display(),
        b = plan.branch,
        co = plan.checkout.display(),
    )
}

/// Runs git commands for one `make`, all within `MAKE_LIMIT`.
struct Git {
    until: Instant,
}

impl Git {
    fn new(limit: Duration) -> Self {
        Git { until: Instant::now() + limit }
    }

    /// Runs git in `dir`, with no window, in the time left. Its output, or its error text.
    fn run(&self, dir: &Path, args: &[&str]) -> Result<String, String> {
        let what = args.iter().find(|a| !a.starts_with('-')).copied().unwrap_or("");
        if Instant::now() >= self.until {
            return Err(format!("git {what} wasn't run: the worktree took longer than {} s", MAKE_LIMIT.as_secs()));
        }
        let mut cmd = Command::new("git");
        cmd.args(["-c", "core.fsmonitor=false", "-c", "core.quotepath=off", "-C"])
            .arg(dir)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        {
            use crate::platform::NoWindow;
            cmd.no_window();
        }
        let mut child = cmd.spawn().map_err(|e| format!("couldn't run git: {e}"))?;
        let read = |pipe: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut s = Vec::new();
                if let Some(mut p) = pipe {
                    let _ = p.read_to_end(&mut s);
                }
                String::from_utf8_lossy(&s).trim().to_string()
            })
        };
        let out = read(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
        let err = read(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if Instant::now() >= self.until => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("git {what} took longer than the {} s the app allows", MAKE_LIMIT.as_secs()));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(15)),
                Err(e) => return Err(format!("git failed: {e}")),
            }
        };
        let (out, err) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
        if status.success() {
            Ok(out)
        } else {
            let first = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("no message").trim();
            Err(format!("git {what} failed: {first}"))
        }
    }

    fn has_ref(&self, dir: &Path, r: &str) -> bool {
        self.run(dir, &["rev-parse", "--verify", "--quiet", r]).is_ok()
    }
}

/// Is `path` a worktree of `checkout` with `branch` checked out (`git worktree list`)? One git
/// still has locked (a `worktree add` stopped part way) is an error, never "found".
fn is_its_worktree(g: &Git, checkout: &Path, path: &Path, branch: &str) -> Result<bool, String> {
    let list = g.run(checkout, &["worktree", "list", "--porcelain"])?;
    let want = path.display().to_string();
    // Entries are separated by a blank line.
    for entry in list.split("\n\n") {
        let mut lines = entry.lines().map(str::trim);
        let Some(p) = lines.next().and_then(|l| l.strip_prefix("worktree ")) else { continue };
        if !same_path(p, &want) {
            continue;
        }
        let rest: Vec<&str> = lines.collect();
        if rest.iter().any(|l| *l == "locked" || l.starts_with("locked ")) {
            return Err(format!("{want} is half made (git has it locked); remove it with git worktree remove --force \"{want}\", then start the card again"));
        }
        return Ok(rest.iter().any(|l| *l == format!("branch refs/heads/{branch}")));
    }
    Ok(false)
}

/// Keeps the card worktrees out of the checkout's `git status`: if git doesn't ignore them
/// already (this repo's .gitignore has `.claude/`), adds `/.claude/worktrees/` to the
/// checkout's own `info/exclude`, which git never commits.
fn keep_out_of_status(g: &Git, checkout: &Path, branch: &str) -> Result<(), String> {
    let probe = format!("{}/{}/{branch}/x", UNDER[0], UNDER[1]);
    if g.run(checkout, &["check-ignore", "-q", "--no-index", &probe]).is_ok() {
        return Ok(());
    }
    let rel = g.run(checkout, &["rev-parse", "--git-path", "info/exclude"])?;
    let file = if Path::new(&rel).is_absolute() { PathBuf::from(&rel) } else { checkout.join(&rel) };
    let old = std::fs::read_to_string(&file).unwrap_or_default();
    let line = format!("/{}/{}/", UNDER[0], UNDER[1]);
    if old.lines().any(|l| l.trim() == line) {
        return Ok(());
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("couldn't make {}: {e}", dir.display()))?;
    }
    let sep = if old.is_empty() || old.ends_with('\n') { "" } else { "\n" };
    std::fs::write(&file, format!("{old}{sep}# Card worktrees the kanban app makes (card #146)\n{line}\n"))
        .map_err(|e| format!("couldn't write {}: {e}", file.display()))
}

/// Makes `plan`'s worktree, or finds it made. Touches nothing that isn't the card's own: a
/// folder already there that isn't this card's worktree is left alone and refused. Before it
/// adds one, `git worktree prune` forgets worktrees whose folders were deleted by hand (only
/// git's own records of them), so a card branch whose folder went gets a worktree again. A card
/// that still names the missing folder in its worktreePath isn't planned again: clear the field first.
pub fn make(plan: &Plan) -> Result<Made, String> {
    make_within(plan, MAKE_LIMIT)
}

/// `make` with its own time budget (the tests give a slow, busy machine longer).
fn make_within(plan: &Plan, limit: Duration) -> Result<Made, String> {
    let g = Git::new(limit);
    let co = &plan.checkout;
    if !co.join(".git").exists() {
        return Err(format!("{} isn't a git checkout", co.display()));
    }
    if !crate::check_size::branch_ok(&plan.branch) {
        return Err(format!("\"{}\" isn't a branch name the app will pass to git", plan.branch));
    }
    let wt = plan.path.display().to_string();
    if plan.path.exists() {
        if is_its_worktree(&g, co, &plan.path, &plan.branch)? {
            let head = g.run(&plan.path, &["rev-parse", "--short", "HEAD"])?;
            return Ok(Made { how: "found", base: None, head });
        }
        return Err(format!("{wt} is already there and isn't this card's worktree on {}, so the app left it alone", plan.branch));
    }
    keep_out_of_status(&g, co, &plan.branch)?;
    g.run(co, &["worktree", "prune"])?;
    let (how, base) = if g.has_ref(co, &format!("refs/heads/{}", plan.branch)) {
        g.run(co, &["worktree", "add", "--quiet", &wt, &plan.branch])?;
        ("made on its existing branch", None)
    } else {
        let base = ["origin/main", "main"]
            .into_iter()
            .find(|b| g.has_ref(co, &format!("{b}^{{commit}}")))
            .ok_or_else(|| format!("{} has neither origin/main nor main to start the branch from", co.display()))?;
        // --no-track: the card's branch must never have main as its upstream, so a plain
        // `git push` can't reach main.
        g.run(co, &["worktree", "add", "--quiet", "--no-track", "-b", &plan.branch, &wt, base])?;
        ("made", Some(base.to_string()))
    };
    let head = g.run(&plan.path, &["rev-parse", "--short", "HEAD"])?;
    Ok(Made { how, base, head })
}

// ---- The merge lane's own worktree (card #160) ----

/// The name of a repo's merge-lane worktree on `channel`: `merge-lane` on the live board,
/// `<channel>-merge-lane` on a test board, so two channels never share one.
pub fn lane_name(channel: &str) -> String {
    match channel.trim() {
        "" | "live" => "merge-lane".into(),
        c => format!("{c}-merge-lane"),
    }
}

/// Where a repo's merge lane works: `<checkout>\.claude\worktrees\<lane_name>`, inside the
/// checkout Claude Code already trusts.
pub fn lane_path(checkout: &Path, channel: &str) -> PathBuf {
    UNDER.iter().fold(checkout.to_path_buf(), |p, part| p.join(part)).join(lane_name(channel))
}

/// Is `path` a worktree of `checkout` (any HEAD, a branch or detached)? A locked one is an error.
fn is_a_worktree(g: &Git, checkout: &Path, path: &Path) -> Result<bool, String> {
    let list = g.run(checkout, &["worktree", "list", "--porcelain"])?;
    let want = path.display().to_string();
    for entry in list.split("\n\n") {
        let mut lines = entry.lines().map(str::trim);
        let Some(p) = lines.next().and_then(|l| l.strip_prefix("worktree ")) else { continue };
        if !same_path(p, &want) {
            continue;
        }
        if lines.any(|l| l == "locked" || l.starts_with("locked ")) {
            return Err(format!("{want} is half made (git has it locked); remove it with git worktree remove --force \"{want}\""));
        }
        return Ok(true);
    }
    Ok(false)
}

/// Makes the merge lane's worktree at `path`, detached at `origin/main` (else `main`), or finds
/// it made (it is kept between jobs, so its build folders stay warm). Never makes or moves a
/// branch. A folder there that isn't a worktree of `checkout` is left alone and refused.
pub fn make_lane(checkout: &Path, path: &Path) -> Result<Made, String> {
    make_lane_within(checkout, path, MAKE_LIMIT)
}

fn make_lane_within(checkout: &Path, path: &Path, limit: Duration) -> Result<Made, String> {
    let g = Git::new(limit);
    if !checkout.join(".git").exists() {
        return Err(format!("{} isn't a git checkout", checkout.display()));
    }
    let wt = path.display().to_string();
    if path.exists() {
        if is_a_worktree(&g, checkout, path)? {
            let head = g.run(path, &["rev-parse", "--short", "HEAD"])?;
            return Ok(Made { how: "found", base: None, head });
        }
        return Err(format!("{wt} is already there and isn't a worktree of {}, so the app left it alone", checkout.display()));
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("merge-lane").to_string();
    keep_out_of_status(&g, checkout, &name)?;
    g.run(checkout, &["worktree", "prune"])?;
    let base = ["origin/main", "main"]
        .into_iter()
        .find(|b| g.has_ref(checkout, &format!("{b}^{{commit}}")))
        .ok_or_else(|| format!("{} has neither origin/main nor main to start the merge lane from", checkout.display()))?;
    g.run(checkout, &["worktree", "add", "--quiet", "--detach", &wt, base])?;
    let head = g.run(path, &["rev-parse", "--short", "HEAD"])?;
    Ok(Made { how: "made", base: Some(base.to_string()), head })
}

/// Runs one git command in `dir` within `MAKE_LIMIT`, with no window: its output, or its error.
/// For the merge lane's checks, which run with no lock of the board held.
pub fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    Git::new(MAKE_LIMIT).run(dir, args)
}

/// Whether `sha` names a commit in `dir`'s repo, and its full SHA if so.
pub fn commit_of(dir: &Path, sha: &str) -> Option<String> {
    if sha.is_empty() || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    git(dir, &["rev-parse", "--verify", "--quiet", &format!("{sha}^{{commit}}")]).ok().filter(|s| !s.is_empty())
}

/// Whether commit `head` is on top of commit `onto` (`git merge-base --is-ancestor onto head`),
/// both checked to be commits first, so any other git failure reads as "no".
pub fn on_top_of(dir: &Path, onto: &str, head: &str) -> Result<bool, String> {
    let o = commit_of(dir, onto).ok_or_else(|| format!("{onto} isn't a commit in {}", dir.display()))?;
    let h = commit_of(dir, head).ok_or_else(|| format!("{head} isn't a commit in {}", dir.display()))?;
    Ok(git(dir, &["merge-base", "--is-ancestor", &o, &h]).is_ok())
}

/// The card's log line for a made (or found) worktree.
pub fn made_log(plan: &Plan, m: &Made) -> String {
    match (&m.base, m.how) {
        (Some(base), _) => format!("Made its own worktree {} on a new branch {}, from {base} at {} (card #146)", plan.path.display(), plan.branch, m.head),
        (None, "found") => format!("Found its own worktree {} on {} at {}, and uses it (card #146)", plan.path.display(), plan.branch, m.head),
        (None, _) => format!("Made its own worktree {} on its existing branch {}, at {} (card #146)", plan.path.display(), plan.branch, m.head),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The tests' git budget: they run beside many others.
    const SLOW: Duration = Duration::from_secs(300);

    fn card(number: i64, fields: Value) -> Card {
        Card { id: format!("card-{number}"), number, column: "Planning".into(), order: None, order_col: None, archived: false, fields: fields.as_object().cloned().unwrap(), version: 1 }
    }

    fn list(checkout: &str) -> Vec<OwnWorktreeRepo> {
        vec![OwnWorktreeRepo { repo: "hughht5/claude-kanban-dsh".into(), checkout: checkout.into() }]
    }

    #[test]
    fn names_are_card_n_on_live_and_carry_a_test_channel() {
        assert_eq!(name(146, "live"), "card-146");
        assert_eq!(name(146, ""), "card-146");
        assert_eq!(name(146, "test"), "test-card-146");
        assert_eq!(name(9, "test3"), "test3-card-9");
    }

    #[test]
    fn a_listed_card_with_no_worktree_gets_one_inside_the_checkout() {
        let l = list(r"C:\w\repo");
        let p = plan_for(&card(7, json!({ "repo": "HughHT5/Claude-Kanban-DSH" })), &l, "live").unwrap();
        assert_eq!(p.branch, "card-7");
        assert_eq!(p.checkout, PathBuf::from(r"C:\w\repo"));
        assert_eq!(p.path, PathBuf::from(r"C:\w\repo").join(".claude").join("worktrees").join("card-7"));
        assert_eq!(plan_for(&card(7, json!({ "repo": "hughht5/claude-kanban-dsh", "worktreePath": "  " })), &l, "test").unwrap().branch, "test-card-7");
    }

    #[test]
    fn no_plan_for_other_cards() {
        let l = list(r"C:\w\repo");
        let none = [
            json!({ "repo": "acme/web" }),
            json!({}),
            json!({ "repo": "hughht5/claude-kanban-dsh", "worktreePath": r"C:\w\repo" }),
            json!({ "repo": "hughht5/claude-kanban-dsh", "stackOf": 145 }),
            json!({ "repo": "hughht5/claude-kanban-dsh", "stack": [146, 147] }),
        ];
        for f in none {
            assert!(plan_for(&card(7, f.clone()), &l, "live").is_none(), "{f}");
        }
        let mut gone = card(7, json!({ "repo": "hughht5/claude-kanban-dsh" }));
        gone.archived = true;
        assert!(plan_for(&gone, &l, "live").is_none());
        assert!(plan_for(&card(7, json!({ "repo": "hughht5/claude-kanban-dsh" })), &[], "live").is_none(), "an empty list is off");
    }

    #[test]
    fn made_for_knows_the_apps_own_folder_in_any_spelling() {
        let mine = card(7, json!({ "repo": "hughht5/claude-kanban-dsh", "worktreePath": "c:/W/repo/.claude/worktrees/card-7/" }));
        assert_eq!(made_for(&mine, "live").map(|p| p.branch), Some("card-7".into()));
        let exact = card(7, json!({ "repo": "hughht5/claude-kanban-dsh", "worktreePath": PathBuf::from(r"C:\w\repo").join(".claude").join("worktrees").join("card-7").display().to_string() }));
        assert!(made_for(&exact, "live").is_some());
        // The main checkout itself (the old way of building), or another folder: not the app's.
        assert!(made_for(&card(7, json!({ "repo": "hughht5/claude-kanban-dsh", "worktreePath": r"C:\w\repo" })), "live").is_none());
        assert!(made_for(&card(7, json!({ "repo": "hughht5/claude-kanban-dsh" })), "live").is_none());
        // A test board's card 7 isn't in live card 7's folder.
        assert!(made_for(&exact, "test").is_none());
    }

    #[test]
    fn the_notes_name_the_folder_branch_and_the_rule_they_replace() {
        let p = Plan { repo: "a/b".into(), checkout: PathBuf::from(r"C:\w\repo"), path: PathBuf::from(r"C:\w\repo\.claude\worktrees\card-7"), branch: "card-7".into() };
        let n = note(&p);
        for must in [r"C:\w\repo\.claude\worktrees\card-7", "card-7", r"never in C:\w\repo", "never on main", "don't change this card's worktreePath", "one card of this repo builds at a time", "push card-7 only"] {
            assert!(n.contains(must), "{must}: {n}");
        }
        let f = failed_note(&p, "git worktree failed: boom");
        for must in ["git worktree failed: boom", r#"git -C "C:\w\repo" worktree add --no-track -b card-7 "C:\w\repo\.claude\worktrees\card-7" origin/main"#, "record worktreePath and branch", "never on main"] {
            assert!(f.contains(must), "{must}: {f}");
        }
    }

    // On a scratch repo with an origin: the git side.

    fn sh(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git").current_dir(dir).args(args).output().expect("git runs");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn commit(dir: &Path, file: &str, msg: &str) {
        std::fs::write(dir.join(file), msg).unwrap();
        sh(dir, &["add", file]);
        sh(dir, &["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q", "-m", msg]);
    }

    /// A checkout cloned from a bare origin, with one commit on origin's main, a local commit on
    /// main not pushed yet (as a card built the old way leaves), and uncommitted work.
    fn scratch() -> Option<(tempfile::TempDir, PathBuf)> {
        Command::new("git").arg("--version").output().ok()?;
        let t = tempfile::tempdir().unwrap();
        let seed = t.path().join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        sh(&seed, &["init", "-q", "-b", "main"]);
        commit(&seed, "base.txt", "base");
        sh(t.path(), &["clone", "-q", "--bare", seed.to_str().unwrap(), "origin.git"]);
        sh(t.path(), &["clone", "-q", t.path().join("origin.git").to_str().unwrap(), "repo"]);
        let co = t.path().join("repo");
        commit(&co, "local.txt", "local only");
        std::fs::write(co.join("base.txt"), "someone's uncommitted work").unwrap();
        Some((t, co))
    }

    fn state(co: &Path) -> (String, String, String) {
        (sh(co, &["rev-parse", "HEAD"]), sh(co, &["rev-parse", "--abbrev-ref", "HEAD"]), sh(co, &["status", "--porcelain"]))
    }

    #[test]
    fn two_cards_get_two_worktrees_and_the_checkout_is_untouched() {
        let Some((_t, co)) = scratch() else { return };
        let before = state(&co);
        let l = vec![OwnWorktreeRepo { repo: "a/b".into(), checkout: co.display().to_string() }];
        let mut heads = vec![];
        for n in [7, 8] {
            let p = plan_for(&card(n, json!({ "repo": "a/b" })), &l, "live").unwrap();
            let m = make_within(&p, SLOW).unwrap();
            assert_eq!((m.how, m.base.as_deref()), ("made", Some("origin/main")));
            assert_eq!(sh(&p.path, &["rev-parse", "--abbrev-ref", "HEAD"]), format!("card-{n}"));
            // From origin/main: the checkout's unpushed local commit isn't on the card's branch.
            assert!(!p.path.join("local.txt").exists());
            assert_eq!(std::fs::read_to_string(p.path.join("base.txt")).unwrap(), "base");
            assert!(made_log(&p, &m).starts_with(&format!("Made its own worktree {} on a new branch card-{n}, from origin/main at ", p.path.display())));
            heads.push(m.head);
        }
        assert_eq!(heads[0], heads[1]);
        // The checkout's HEAD, branch and status are as they were: the worktrees are kept out of
        // its status by its info/exclude.
        assert_eq!(state(&co), before);
        let list = sh(&co, &["worktree", "list"]);
        assert!(list.contains("card-7") && list.contains("card-8"), "{list}");
    }

    #[test]
    fn a_made_worktree_is_found_again_and_an_existing_branch_is_used() {
        let Some((_t, co)) = scratch() else { return };
        let l = vec![OwnWorktreeRepo { repo: "a/b".into(), checkout: co.display().to_string() }];
        let p = plan_for(&card(7, json!({ "repo": "a/b" })), &l, "live").unwrap();
        make_within(&p, SLOW).unwrap();
        commit(&p.path, "work.txt", "card work (card #7)");
        let again = make_within(&p, SLOW).unwrap();
        assert_eq!((again.how, again.base), ("found", None));
        assert_eq!(again.head, sh(&p.path, &["rev-parse", "--short", "HEAD"]));
        // The folder went (removed by hand) but the branch stayed: a new worktree on that branch.
        sh(&co, &["worktree", "remove", p.path.to_str().unwrap()]);
        let back = make_within(&p, SLOW).unwrap();
        assert_eq!((back.how, back.base), ("made on its existing branch", None));
        assert!(p.path.join("work.txt").exists(), "the card's own commit is there");
    }

    #[test]
    fn a_folder_that_isnt_the_cards_worktree_is_refused_and_left_alone() {
        let Some((_t, co)) = scratch() else { return };
        let l = vec![OwnWorktreeRepo { repo: "a/b".into(), checkout: co.display().to_string() }];
        let p = plan_for(&card(7, json!({ "repo": "a/b" })), &l, "live").unwrap();
        std::fs::create_dir_all(&p.path).unwrap();
        std::fs::write(p.path.join("keep.txt"), "mine").unwrap();
        let before = state(&co);
        let e = make_within(&p, SLOW).unwrap_err();
        assert!(e.contains("isn't this card's worktree") && e.contains("left it alone"), "{e}");
        assert_eq!(std::fs::read_to_string(p.path.join("keep.txt")).unwrap(), "mine");
        assert!(!sh(&co, &["branch", "--list", "card-7"]).contains("card-7"), "no branch made");
        assert_eq!(state(&co).0, before.0);
        // A folder that isn't a checkout at all.
        let mut bad = p.clone();
        bad.checkout = co.parent().unwrap().join("nowhere");
        assert!(make_within(&bad, SLOW).unwrap_err().contains("isn't a git checkout"));
    }

    /// Review fixes: a folder deleted by hand comes back (prune), the branch tracks nothing, and
    /// a half-made (locked) worktree is never "found".
    #[test]
    fn a_folder_deleted_by_hand_comes_back_and_the_branch_tracks_nothing() {
        let Some((_t, co)) = scratch() else { return };
        let l = vec![OwnWorktreeRepo { repo: "a/b".into(), checkout: co.display().to_string() }];
        let p = plan_for(&card(7, json!({ "repo": "a/b" })), &l, "live").unwrap();
        make_within(&p, SLOW).unwrap();
        // No upstream: a plain push can't reach main.
        assert!(Command::new("git").current_dir(&p.path).args(["rev-parse", "--abbrev-ref", "@{u}"]).output().unwrap().status.code() != Some(0));
        std::fs::remove_dir_all(&p.path).unwrap();
        let back = make_within(&p, SLOW).unwrap();
        assert_eq!(back.how, "made on its existing branch");
        assert!(p.path.join("base.txt").exists());
        sh(&co, &["worktree", "lock", p.path.to_str().unwrap()]);
        let e = make_within(&p, SLOW).unwrap_err();
        assert!(e.contains("half made") && e.contains("worktree remove --force"), "{e}");
    }

    #[test]
    fn made_for_reads_the_card_alone() {
        let wt = PathBuf::from(r"C:\w\repo").join(".claude").join("worktrees").join("card-7");
        let c = card(7, json!({ "repo": "a/b", "worktreePath": wt.display().to_string(), "branch": "card-7" }));
        let p = made_for(&c, "live").unwrap();
        assert_eq!((p.path, p.checkout, p.branch.as_str()), (wt.clone(), PathBuf::from(r"C:\w\repo"), "card-7"));
        // Another branch on the card, or a stack's card: not the app's.
        assert!(made_for(&card(7, json!({ "worktreePath": wt.display().to_string(), "branch": "dsh-145-7" })), "live").is_none());
        assert!(made_for(&card(7, json!({ "worktreePath": wt.display().to_string(), "stackOf": 5 })), "live").is_none());
        // The note still names the repo politely without one.
        assert!(note(&made_for(&card(7, json!({ "worktreePath": wt.display().to_string() })), "live").unwrap()).contains("other cards of this repo"));
    }

    #[test]
    fn with_no_origin_it_starts_from_main() {
        let Some((t, _co)) = scratch() else { return };
        let seed = t.path().join("seed");
        let l = vec![OwnWorktreeRepo { repo: "a/b".into(), checkout: seed.display().to_string() }];
        let p = plan_for(&card(3, json!({ "repo": "a/b" })), &l, "test2").unwrap();
        let m = make_within(&p, SLOW).unwrap();
        assert_eq!(m.base.as_deref(), Some("main"));
        assert_eq!(sh(&p.path, &["rev-parse", "--abbrev-ref", "HEAD"]), "test2-card-3");
    }

    #[test]
    fn the_lanes_worktree_is_per_channel_inside_the_checkout() {
        assert_eq!(lane_name("live"), "merge-lane");
        assert_eq!(lane_name(""), "merge-lane");
        assert_eq!(lane_name("test3"), "test3-merge-lane");
        let p = lane_path(Path::new(r"C:\w\repo"), "test");
        assert_eq!(p, PathBuf::from(r"C:\w\repo").join(".claude").join("worktrees").join("test-merge-lane"));
    }

    /// Card #160: the lane's worktree is made detached at origin/main, found again, and makes no
    /// branch; the checkout's own branch and work are untouched.
    #[test]
    fn the_lanes_worktree_is_detached_found_again_and_makes_no_branch() {
        let Some((_t, co)) = scratch() else { return };
        let before = state(&co);
        let branches = || sh(&co, &["branch", "--list", "--format=%(refname:short)"]);
        let b0 = branches();
        let path = lane_path(&co, "live");
        let m = make_lane_within(&co, &path, SLOW).unwrap();
        assert_eq!((m.how, m.base.as_deref()), ("made", Some("origin/main")));
        assert_eq!(sh(&path, &["rev-parse", "--abbrev-ref", "HEAD"]), "HEAD", "detached");
        assert_eq!(sh(&path, &["rev-parse", "HEAD"]), sh(&co, &["rev-parse", "origin/main"]));
        assert_eq!(branches(), b0, "no branch made");
        assert_eq!(state(&co), before, "the checkout is untouched");
        let again = make_lane_within(&co, &path, SLOW).unwrap();
        assert_eq!(again.how, "found");
        // A folder that isn't a worktree is refused and left alone.
        let other = lane_path(&co, "test");
        std::fs::create_dir_all(&other).unwrap();
        assert!(make_lane_within(&co, &other, SLOW).unwrap_err().contains("left it alone"));
        // The ancestry check the app makes on a PASS.
        let main = sh(&co, &["rev-parse", "origin/main"]);
        let local = sh(&co, &["rev-parse", "main"]);
        assert!(on_top_of(&co, &main, &local).unwrap(), "main's local commit is on top of origin/main");
        assert!(!on_top_of(&co, &local, &main).unwrap());
        assert!(on_top_of(&co, &main, "0000000").is_err(), "not a commit");
        assert_eq!(commit_of(&co, &main[..7]).as_deref(), Some(main.as_str()));
        assert!(commit_of(&co, "HEAD~1").is_none(), "only a SHA");
    }
}
