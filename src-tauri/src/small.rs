//! Card #104: small changes that go on by themselves.
//!
//! When a card's session moves its card into Human test, the app asks this module whether the
//! change is small, fully checked and safe enough to go on into Ship it with no human test.
//! `git_facts` collects what git says about the checkout on Hugh's list; `decide` is pure and
//! judges those facts against the card's tester and reviewer verdicts. When in doubt it refuses.

use kanban_store::{looks_secret, Card, SmallRepo};
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{mpsc, OnceLock};
use std::time::{Duration, Instant};

/// Added + removed lines, all files.
#[allow(dead_code)]
pub const LINES_MAX: usize = 30;
/// Bytes of the added + removed line text, all files.
#[allow(dead_code)]
pub const BYTES_MAX: usize = 3000;
#[allow(dead_code)]
pub const FILES_MAX: usize = 5;
#[allow(dead_code)]
pub const COMMITS_MAX: usize = 20;

/// How long one git command may run before the app kills it.
#[cfg(not(test))]
const GIT_TIMEOUT: Duration = Duration::from_secs(5);
/// The tests run many git commands side by side, often on a busy machine, where starting git
/// alone can take seconds; they check the facts, not the machine's speed.
#[cfg(test)]
const GIT_TIMEOUT: Duration = Duration::from_secs(60);

/// Before every subcommand, so the user's or the repo's config can't change what git shows.
/// `NUL` works as `core.attributesFile` with Git for Windows (checked with git 2.55: a later
/// `-c core.attributesFile=NUL` turns an earlier attributes file off, and git reads it as empty).
const GIT_SAFE: [&str; 12] = [
    "-c", "core.attributesFile=NUL",
    "-c", "core.quotepath=false",
    "-c", "core.fsmonitor=false",
    "-c", "core.untrackedCache=false",
    "-c", "diff.external=",
    "-c", "color.ui=never",
];

/// Taken out of the child's environment: each can point git at another repo, index or config.
const GIT_ENV_DROP: [&str; 9] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_EXTERNAL_DIFF",
];

/// The options every diff-tree call gets.
const DIFF_SAFE: [&str; 5] = ["-r", "--no-renames", "--no-ext-diff", "--no-textconv", "--no-color"];

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub head: String,
    pub status_hash: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct Commit {
    pub sha: String,
    pub subject: String,
    pub message: String,
    pub parents: usize,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct FileChange {
    /// The new path, forward slashes, as git prints it.
    pub path: String,
    /// The raw status letter: "M", "A", "D", "T", ...
    pub status: String,
    pub old_mode: String,
    pub new_mode: String,
    /// numstat printed "-".
    pub binary: bool,
    pub added: usize,
    pub removed: usize,
    /// The text of each "+" line, without the "+".
    pub added_lines: Vec<String>,
    /// The text of each "-" line, without the "-".
    pub removed_lines: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct Facts {
    /// The top of the checkout, as `git rev-parse --show-toplevel` prints it.
    pub checkout: String,
    /// `git symbolic-ref --short -q HEAD`; "" when HEAD is detached.
    pub branch_now: String,
    pub head: String,
    pub status_hash: String,
    /// `git status` printed nothing.
    pub status_clean: bool,
    /// The sha of origin/<branch>.
    pub base: String,
    /// base..HEAD, newest first.
    pub commits: Vec<Commit>,
    /// The diff base..HEAD.
    pub files: Vec<FileChange>,
    /// Why git's view may be false, if any.
    pub tamper: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Running git
// ---------------------------------------------------------------------------------------------

/// What one git command printed, and how it ended.
struct GitOut {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Runs `git <GIT_SAFE> <args>` in `checkout`, with no console window, a clean environment and a
/// 5-second timeout. `input`, if any, goes to git's stdin; otherwise stdin is null. Only a spawn
/// error or a timeout is an Err here; the exit code is the caller's to judge.
fn git_run(checkout: &str, args: &[&str], input: Option<&[u8]>) -> Result<GitOut, String> {
    let what = args.first().copied().unwrap_or("git");
    let mut cmd = Command::new("git");
    cmd.current_dir(checkout).args(GIT_SAFE);
    if let Some(v) = system_autocrlf() {
        cmd.arg("-c").arg(format!("core.autocrlf={v}"));
    }
    cmd.args(args)
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for k in GIT_ENV_DROP {
        cmd.env_remove(k);
    }
    {
        use crate::platform::NoWindow;
        cmd.no_window();
    }
    let mut child = cmd.spawn().map_err(|e| format!("git {what} couldn't start: {e}"))?;

    // Feed stdin and drain stdout and stderr on their own threads, so a full pipe never stalls
    // the child while this thread waits for it.
    if let (Some(bytes), Some(mut stdin)) = (input, child.stdin.take()) {
        let bytes = bytes.to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
    }
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        let (tx, rx) = mpsc::channel();
        if let Some(mut p) = pipe {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = p.read_to_end(&mut buf);
                let _ = tx.send(buf);
            });
        }
        rx
    };
    let out_rx = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err_rx = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));

    let deadline = Instant::now() + GIT_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("git {what} took more than {} seconds", GIT_TIMEOUT.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => {
                let _ = child.kill();
                return Err(format!("git {what} couldn't be watched: {e}"));
            }
        }
    };
    // The pipes close when git exits; a grandchild holding one open must not hang the app.
    let left = deadline.saturating_duration_since(Instant::now()) + Duration::from_secs(1);
    let stdout = out_rx
        .recv_timeout(left)
        .map_err(|_| format!("git {what}'s output didn't finish"))?;
    let stderr = err_rx.recv_timeout(Duration::from_millis(500)).unwrap_or_default();
    Ok(GitOut { code: status.code(), stdout, stderr })
}

/// The system config's `core.autocrlf` (Git for Windows sets `true` there), read once. The checks
/// run with `GIT_CONFIG_NOSYSTEM=1`, which would drop it, and then a CRLF checkout whose files
/// were touched looks changed to `git status`. Only this one key is carried over, and only one
/// of git's own values.
fn system_autocrlf() -> Option<&'static str> {
    static V: std::sync::OnceLock<Option<&'static str>> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        let mut cmd = Command::new("git");
        cmd.args(["config", "--system", "--get", "core.autocrlf"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        for k in GIT_ENV_DROP {
            cmd.env_remove(k);
        }
        {
            use crate::platform::NoWindow;
            cmd.no_window();
        }
        let out = cmd.output().ok()?;
        match String::from_utf8_lossy(&out.stdout).trim().to_ascii_lowercase().as_str() {
            "true" => Some("true"),
            "false" => Some("false"),
            "input" => Some("input"),
            _ => None,
        }
    })
}

/// Runs git and returns its stdout; a non-zero exit is an Err with git's first error line.
fn git(checkout: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    git_with(checkout, args, None)
}

fn git_with(checkout: &str, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let out = git_run(checkout, args, input)?;
    if out.code == Some(0) {
        return Ok(out.stdout);
    }
    let what = args.first().copied().unwrap_or("git");
    let err = String::from_utf8_lossy(&out.stderr);
    let first = err.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("no message");
    Err(format!("git {what} failed ({first})"))
}

/// stdout as trimmed text.
fn git_text(checkout: &str, args: &[&str]) -> Result<String, String> {
    Ok(String::from_utf8_lossy(&git(checkout, args)?).trim().to_string())
}

/// `git status` as the app reads it, its bytes untouched.
fn status_bytes(checkout: &str) -> Result<Vec<u8>, String> {
    git(
        checkout,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--ignore-submodules=none"],
    )
}

/// The blob hash of the status bytes: `git hash-object --stdin`, with no --path.
fn status_hash(checkout: &str, status: &[u8]) -> Result<String, String> {
    let h = git_with(checkout, &["hash-object", "--stdin"], Some(status))?;
    let h = String::from_utf8_lossy(&h).trim().to_string();
    if h.is_empty() {
        return Err("git hash-object printed no hash".into());
    }
    Ok(h)
}

/// HEAD and a hash of `git status` in `checkout`. The verdict records store these.
#[allow(dead_code)]
pub fn snapshot(checkout: &str) -> Result<Snapshot, String> {
    let head = git_text(checkout, &["rev-parse", "HEAD"])?;
    let status = status_bytes(checkout)?;
    Ok(Snapshot { head, status_hash: status_hash(checkout, &status)? })
}

/// Why git's view of this checkout may be false, if anything says so.
fn tamper_of(checkout: &str) -> Result<Option<String>, String> {
    let dirs = git_text(checkout, &["rev-parse", "--git-dir", "--git-common-dir"])?;
    for dir in dirs.lines().map(str::trim).filter(|d| !d.is_empty()) {
        let dir = Path::new(checkout).join(dir); // an absolute `dir` replaces the checkout
        if dir.join("info").join("attributes").exists() {
            return Ok(Some("the repo has an info/attributes file, which can change how git shows a diff".into()));
        }
        if dir.join("info").join("grafts").exists() {
            return Ok(Some("the repo has an info/grafts file, which can change its history".into()));
        }
    }
    let replace = git(checkout, &["for-each-ref", "--format=%(refname)", "refs/replace/"])?;
    if !String::from_utf8_lossy(&replace).trim().is_empty() {
        return Ok(Some("the repo has replace refs, which can make git show other commits".into()));
    }
    Ok(None)
}

/// base..head, newest first. Past COMMITS_MAX the messages aren't read: decide refuses anyway.
fn commits_of(checkout: &str, base: &str, head: &str) -> Result<Vec<Commit>, String> {
    let range = format!("{base}..{head}");
    let list = git_text(checkout, &["rev-list", "--parents", &range])?;
    let rows: Vec<Vec<&str>> = list.lines().map(|l| l.split_whitespace().collect()).filter(|r: &Vec<&str>| !r.is_empty()).collect();
    let read = rows.len() <= COMMITS_MAX;
    let mut out = Vec::new();
    for r in rows {
        let sha = r[0].to_string();
        let (subject, message) = if read {
            let raw = git(checkout, &["cat-file", "commit", &sha])?;
            let text = String::from_utf8_lossy(&raw).into_owned();
            let message = match text.find("\n\n") {
                Some(i) => text[i + 2..].to_string(),
                None => String::new(),
            };
            let subject = message.lines().next().unwrap_or("").trim().to_string();
            (subject, message)
        } else {
            (String::new(), String::new())
        };
        out.push(Commit { sha, subject, message, parents: r.len() - 1 });
    }
    Ok(out)
}

/// Splits git's -z output into its non-empty fields.
fn z_fields(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|b| *b == 0)
        .filter(|f| !f.is_empty())
        .map(|f| String::from_utf8_lossy(f).into_owned())
        .collect()
}

/// The diff base..head: status and modes from --raw, counts from --numstat, the line text from
/// the patch. Each view is checked against the others; any mismatch is an Err.
fn files_of(checkout: &str, base: &str, head: &str) -> Result<Vec<FileChange>, String> {
    let with = |extra: &[&str]| {
        let mut a: Vec<&str> = vec!["diff-tree"];
        a.extend(DIFF_SAFE);
        a.extend(extra);
        a.extend([base, head, "--"]);
        git(checkout, &a)
    };

    // --raw -z: ":old_mode new_mode old_sha new_sha status" NUL path NUL, per file.
    let raw = z_fields(&with(&["--raw", "-z"])?);
    let mut files: Vec<FileChange> = Vec::new();
    let mut it = raw.into_iter();
    while let Some(meta) = it.next() {
        let parts: Vec<&str> = meta.trim_start_matches(':').split_whitespace().collect();
        let path = it.next().ok_or("git's file list ended early")?;
        if !meta.starts_with(':') || parts.len() != 5 {
            return Err(format!("git printed a file line it shouldn't ({meta})"));
        }
        files.push(FileChange {
            path,
            status: parts[4].to_string(),
            old_mode: parts[0].to_string(),
            new_mode: parts[1].to_string(),
            binary: false,
            added: 0,
            removed: 0,
            added_lines: Vec::new(),
            removed_lines: Vec::new(),
        });
    }

    // --numstat -z: "added TAB removed TAB path" NUL, per file; "-" for a binary file.
    let num = z_fields(&with(&["--numstat", "-z"])?);
    if num.len() != files.len() {
        return Err("git's file counts disagree".into());
    }
    for (f, row) in files.iter_mut().zip(num) {
        let mut p = row.splitn(3, '\t');
        let (a, r, path) = (p.next().unwrap_or(""), p.next().unwrap_or(""), p.next().unwrap_or(""));
        if path != f.path {
            return Err(format!("git's file lists disagree about {}", f.path));
        }
        if a == "-" || r == "-" {
            f.binary = true;
        } else {
            f.added = a.parse().map_err(|_| format!("git printed a bad count for {}", f.path))?;
            f.removed = r.parse().map_err(|_| format!("git printed a bad count for {}", f.path))?;
        }
    }

    // -p -U0: one "diff --git" section per file, in the same order as --raw.
    let patch = with(&["-p", "-U0", "--src-prefix=a/", "--dst-prefix=b/"])?;
    let mut sections: Vec<Vec<&[u8]>> = Vec::new();
    for line in patch.split(|b| *b == b'\n') {
        if line.starts_with(b"diff --git ") {
            sections.push(vec![line]);
        } else if let Some(s) = sections.last_mut() {
            s.push(line);
        } else if !line.is_empty() {
            return Err("git's patch didn't start with a file".into());
        }
    }
    if sections.len() != files.len() {
        return Err("git's patch and file list disagree".into());
    }
    for (f, lines) in files.iter_mut().zip(sections) {
        let header = format!("diff --git a/{0} b/{0}", f.path);
        if lines[0] != header.as_bytes() {
            return Err(format!("git's patch and file list disagree about {}", f.path));
        }
        let mut in_hunks = false;
        for line in &lines[1..] {
            if line.starts_with(b"@@") {
                in_hunks = true;
            } else if !in_hunks || line.starts_with(b"\\") {
                // The header ("---", "+++", "index", modes) and "\ No newline at end of file".
            } else if let Some(t) = line.strip_prefix(b"+") {
                f.added_lines.push(String::from_utf8_lossy(t).into_owned());
            } else if let Some(t) = line.strip_prefix(b"-") {
                f.removed_lines.push(String::from_utf8_lossy(t).into_owned());
            }
        }
        if !f.binary && (f.added_lines.len() != f.added || f.removed_lines.len() != f.removed) {
            return Err(format!("git's line counts for {} don't match its patch", f.path));
        }
    }
    Ok(files)
}

/// Collects the facts for `checkout`, on `branch` (from Hugh's list entry). Local refs only:
/// it never fetches.
#[allow(dead_code)]
pub fn git_facts(checkout: &str, branch: &str) -> Result<Facts, String> {
    let top = git_text(checkout, &["rev-parse", "--show-toplevel"])?;
    let head = git_text(checkout, &["rev-parse", "HEAD"])?;
    let sym = git_run(checkout, &["symbolic-ref", "--short", "-q", "HEAD"], None)?;
    let branch_now = match sym.code {
        Some(0) => String::from_utf8_lossy(&sym.stdout).trim().to_string(),
        Some(1) => String::new(), // detached
        _ => return Err("git symbolic-ref failed".into()),
    };
    let status = status_bytes(checkout)?;
    let status_clean = status.is_empty();
    let status_hash = status_hash(checkout, &status)?;
    let tamper = tamper_of(checkout)?;

    let want = format!("origin/{branch}^{{commit}}");
    let base = git_text(checkout, &["rev-parse", "--verify", "--quiet", &want])
        .map_err(|_| format!("the checkout has no origin/{branch}"))?;
    if base.is_empty() {
        return Err(format!("the checkout has no origin/{branch}"));
    }
    // The diff base..HEAD is only the change when HEAD sits on top of origin/<branch>.
    let behind = git_text(checkout, &["rev-list", &format!("{head}..{base}")])?;
    if !behind.is_empty() {
        return Err(format!("origin/{branch} has commits that HEAD doesn't"));
    }
    let commits = commits_of(checkout, &base, &head)?;
    let files = files_of(checkout, &base, &head)?;

    // HEAD must not move while the facts are collected.
    if git_text(checkout, &["rev-parse", "HEAD"])? != head {
        return Err("HEAD moved while the app was checking it".into());
    }
    Ok(Facts { checkout: top, branch_now, head, status_hash, status_clean, base, commits, files, tamper })
}

// ---------------------------------------------------------------------------------------------
// Deciding
// ---------------------------------------------------------------------------------------------

/// True when the card never goes on by itself, so the app checks nothing and logs nothing.
pub fn skip(card: &Card) -> bool {
    let auto_to = card.fields.get("autoTo").and_then(Value::as_str).is_some_and(|s| !s.is_empty());
    let opt_out = card.fields.get("smallOptOut").and_then(Value::as_bool) == Some(true);
    card.archived || card.stack_of().is_some() || card.is_lead() || auto_to || opt_out
}

/// A path to compare: lower case, "\" as "/", no trailing "/".
fn norm_path(p: &str) -> String {
    p.trim().replace('\\', "/").to_lowercase().trim_end_matches('/').to_string()
}

fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

/// Checks one verdict (`test` or `review`) against the checkout and the facts.
fn check_verdict(card: &Card, key: &str, entry: &SmallRepo, facts: &Facts) -> Result<(), String> {
    let who = if key == "test" { "the tester" } else { "the reviewer" };
    let v = card.fields.get(key).cloned().unwrap_or(Value::Null);
    let s = |k: &str| v.get(k).and_then(Value::as_str);
    if s("state") != Some("passed") {
        return Err(format!("{who} hasn't passed it"));
    }
    if !s("agentId").is_some_and(|a| !a.trim().is_empty()) {
        return Err(format!("{who}'s pass names no agent"));
    }
    if !v.get("tools").and_then(Value::as_f64).is_some_and(|t| t > 0.0) {
        return Err(format!("{who}'s pass shows no tool use"));
    }
    // Unpinned (card #104): the work changed while the agent ran, so its pass is for no one state.
    if v.get("placeMoved").and_then(Value::as_bool) == Some(true) {
        return Err(format!("the work changed while {who} ran, so its pass doesn't cover the final commit"));
    }
    if s("checkout").is_none() || s("head").is_none() {
        return Err(format!("{who}'s pass wasn't pinned to a commit (it started before the repo was listed, or its start wasn't seen)"));
    }
    if s("checkout").map(norm_path) != Some(norm_path(&entry.checkout)) {
        return Err(format!("{who}'s pass was for another checkout"));
    }
    if facts.head.is_empty() || s("head") != Some(facts.head.as_str()) {
        return Err(format!("{who}'s pass was for an older commit"));
    }
    if s("statusHash") != Some(facts.status_hash.as_str()) {
        return Err(format!("{who}'s pass was for the checkout in another state"));
    }
    if key == "review" {
        match s("scope") {
            Some("full") => {}
            Some(other) => return Err(format!("the review was narrowed by its brief (SCOPE: {other})")),
            None => return Err("the review doesn't say it covered the whole change (no SCOPE)".into()),
        }
    }
    Ok(())
}

fn added_line_rules() -> &'static [(Regex, &'static str)] {
    static RULES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    RULES.get_or_init(|| {
        [
            (r"(?i)DELETE\s+FROM", "an SQL delete"),
            (r"(?i)\bDROP\s+(TABLE|INDEX|VIEW|TRIGGER|COLUMN|DATABASE)\b", "an SQL drop"),
            (r"(?i)ALTER\s+TABLE", "an SQL table change"),
            (r"(?i)sign-?in", "a sign-in"),
            (r"(?i)token", "a token"),
            (r"(?i)secret", "a secret"),
            (r"(?i)api[ _-]?key", "an API key"),
            (r"(?i)bearer", "a bearer"),
        ]
        .into_iter()
        .map(|(p, what)| (Regex::new(p).expect("small-change line rule"), what))
        .collect()
    })
}

/// A CSS line that sets a property able to hide, move or cover things. The property counts only
/// at the start of a declaration, so "background-position:" and "align-content:" don't match. A
/// property name alone at the end of a line counts too, in case its ":" is on the next line.
fn css_rule() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)(?:^|[\s;{])(display|visibility|pointer-events|position|z-index|opacity|content)\s*(?::|$)")
            .expect("small-change CSS rule")
    })
}

const PATH_WORDS: [&str; 37] = [
    "auth", "sign", "login", "logout", "oauth", "token", "secret", "credential", "cookie", "session",
    "permission", "key", "migrat", "schema", "sql", "store", "db", "database", "data", "seed",
    "fixture", "env", "config", "setting", "deploy", "infra", "docker", "lock", "package.json",
    "cargo", "build", "billing", "payment", "stripe", "iam", "prompt", "password",
];
const PATH_NAMES: [&str; 4] = ["agents.md", "claude.md", "skill.md", ".mcp.json"];
const PATH_SEGMENTS: [&str; 9] = ["agents", "commands", "skills", "hooks", "vendor", "prompts", "scripts", ".github", ".claude"];

/// Ok(evidence) when the card may go on; Err(reason) with the first reason it may not. The
/// reason reads after "Stops in Human test for Hugh: ".
#[allow(dead_code)]
pub fn decide(card: &Card, entry: &SmallRepo, facts: &Facts, css_evidence: bool) -> Result<Value, String> {
    // 1. Once only.
    if card.fields.get("smallChange").and_then(|s| s.get("wentOnAt")).is_some_and(Value::is_string) {
        return Err("it went on by itself once already, so it stops for Hugh this time".into());
    }

    // 2. The verdicts.
    check_verdict(card, "test", entry, facts)?;
    check_verdict(card, "review", entry, facts)?;

    // 3. The checkout.
    if norm_path(&facts.checkout) != norm_path(&entry.checkout) {
        return Err(format!("git's checkout ({}) isn't the one on Hugh's list", facts.checkout));
    }
    if facts.branch_now.is_empty() {
        return Err(format!("the checkout's HEAD is detached, not on {}", entry.branch));
    }
    if facts.branch_now != entry.branch {
        return Err(format!("the checkout is on {}, not {}", facts.branch_now, entry.branch));
    }
    if let Some(t) = &facts.tamper {
        return Err(format!("git's view may be false: {t}"));
    }
    if !facts.status_clean {
        return Err("the checkout has changes that aren't committed".into());
    }

    // 4. The commits.
    if facts.commits.is_empty() {
        return Err(format!("it has no commits on top of origin/{}", entry.branch));
    }
    if facts.commits.len() > COMMITS_MAX {
        return Err(format!("it has {} commits, more than {COMMITS_MAX}", facts.commits.len()));
    }
    let tag = format!("(card #{})", card.number);
    for c in &facts.commits {
        if c.parents > 1 {
            return Err(format!("commit {} ({}) is a merge commit", short(&c.sha), c.subject));
        }
        if !c.message.contains(&tag) {
            return Err(format!("commit {} ({}) doesn't name the card as {tag}", short(&c.sha), c.subject));
        }
    }

    // 5. The size.
    let n = facts.files.len();
    if n == 0 {
        return Err("it changes no files".into());
    }
    if n > FILES_MAX {
        return Err(format!("it changes {n} files, more than {FILES_MAX}"));
    }
    let lines: usize = facts.files.iter().map(|f| f.added + f.removed).sum();
    if lines > LINES_MAX {
        return Err(format!("it changes {lines} lines, more than {LINES_MAX}"));
    }
    let bytes: usize = facts
        .files
        .iter()
        .flat_map(|f| f.added_lines.iter().chain(&f.removed_lines))
        .map(String::len)
        .sum();
    if bytes > BYTES_MAX {
        return Err(format!("its changed lines hold {bytes} bytes, more than {BYTES_MAX}"));
    }
    for f in &facts.files {
        match f.status.as_str() {
            "M" => {}
            "A" => return Err(format!("{} is a new file", f.path)),
            "D" => return Err(format!("{} is deleted", f.path)),
            "T" => return Err(format!("{} changes type", f.path)),
            s => return Err(format!("{} isn't a plain edit (git status {s})", f.path)),
        }
        if f.old_mode != f.new_mode {
            return Err(format!("{} changes its file mode", f.path));
        }
        if f.new_mode != "100644" {
            return Err(format!("{} isn't a plain file (mode {})", f.path, f.new_mode));
        }
        if f.binary {
            return Err(format!("{} is a binary file", f.path));
        }
    }

    // 6. The kind of each file.
    let mut kinds = Vec::new();
    for f in &facts.files {
        let p = f.path.to_lowercase();
        let kind = if p.ends_with(".md") || p.ends_with(".rst") {
            "docs"
        } else if p.ends_with(".css") {
            "css"
        } else {
            return Err(format!("{} isn't a docs or CSS file", f.path));
        };
        kinds.push(kind);
    }

    // 7. The paths that are off limits.
    for f in &facts.files {
        let p = f.path.to_lowercase();
        if let Some(w) = PATH_WORDS.iter().find(|w| p.contains(**w)) {
            return Err(format!("{} is off limits (its path has \"{w}\")", f.path));
        }
        if let Some(s) = p.split('/').find(|s| PATH_SEGMENTS.contains(s)) {
            return Err(format!("{} is off limits (it's under {s}/)", f.path));
        }
        let name = p.rsplit('/').next().unwrap_or("");
        if PATH_NAMES.contains(&name) {
            return Err(format!("{} is off limits (a file agents read as instructions)", f.path));
        }
    }

    // 8. The added lines, all files. The reason names the kind of match, never the line itself,
    // which may hold a secret.
    for f in &facts.files {
        for line in &f.added_lines {
            if let Some((_, what)) = added_line_rules().iter().find(|(re, _)| re.is_match(line)) {
                return Err(format!("an added line in {} mentions {what}", f.path));
            }
            if looks_secret(line) {
                return Err(format!("an added line in {} looks like it holds a secret", f.path));
            }
        }
    }

    // 9. CSS: no line, added or removed, that can hide, move or cover things.
    let mut any_css = false;
    for (f, kind) in facts.files.iter().zip(&kinds) {
        if *kind != "css" {
            continue;
        }
        any_css = true;
        for (sign, line) in f.added_lines.iter().map(|l| ("an added", l)).chain(f.removed_lines.iter().map(|l| ("a removed", l))) {
            if let Some(m) = css_rule().captures(line) {
                return Err(format!("{sign} line in {} sets {} in CSS", f.path, m[1].to_lowercase()));
            }
            if line.to_lowercase().contains("@import") {
                return Err(format!("{sign} line in {} imports more CSS", f.path));
            }
        }
    }
    if any_css && !css_evidence {
        return Err("a CSS change needs #101's before-and-after screenshots".into());
    }

    // 10. Pass.
    let verdict = |key: &str| {
        let v = card.fields.get(key).cloned().unwrap_or(Value::Null);
        let mut o = json!({ "agentId": v["agentId"], "tools": v["tools"], "head": v["head"] });
        if key == "review" {
            o["scope"] = v["scope"].clone();
        }
        o
    };
    Ok(json!({
        "checkout": entry.checkout,
        "branch": entry.branch,
        "base": facts.base,
        "head": facts.head,
        "commits": facts.commits.iter().map(|c| json!({ "sha": c.sha, "subject": c.subject })).collect::<Vec<_>>(),
        "files": facts.files.iter().zip(&kinds).map(|(f, k)| json!({
            "path": f.path, "kind": k, "added": f.added, "removed": f.removed,
        })).collect::<Vec<_>>(),
        "lines": lines,
        "bytes": bytes,
        "test": verdict("test"),
        "review": verdict("review"),
    }))
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "1111111111111111111111111111111111111111";
    const BASE: &str = "0000000000000000000000000000000000000000";
    const HASH: &str = "2222222222222222222222222222222222222222";

    fn card(fields: Value) -> Card {
        Card {
            id: "c104".into(),
            number: 104,
            column: "Human test".into(),
            order: None,
            order_col: None,
            archived: false,
            fields: fields.as_object().cloned().unwrap_or_default(),
            version: 1,
        }
    }

    fn verdict(scope: bool) -> Value {
        let mut v = json!({
            "state": "passed", "agentId": "agent-1", "tools": 7,
            "checkout": "C:\\Users\\x\\Repo\\", "head": HEAD, "statusHash": HASH,
        });
        if scope {
            v["scope"] = json!("full");
        }
        v
    }

    fn good_card() -> Card {
        card(json!({ "title": "Fix a typo", "test": verdict(false), "review": verdict(true) }))
    }

    fn entry() -> SmallRepo {
        SmallRepo { repo: "hughht5/kanban-app".into(), checkout: "C:\\Users\\x\\repo".into(), branch: "main".into() }
    }

    fn file(path: &str, added: &[&str], removed: &[&str]) -> FileChange {
        FileChange {
            path: path.into(),
            status: "M".into(),
            old_mode: "100644".into(),
            new_mode: "100644".into(),
            binary: false,
            added: added.len(),
            removed: removed.len(),
            added_lines: added.iter().map(|s| s.to_string()).collect(),
            removed_lines: removed.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn commit(n: usize, message: &str) -> Commit {
        Commit {
            sha: format!("{n:07}abcdef0123456789abcdef0123456789"),
            subject: message.lines().next().unwrap_or("").into(),
            message: message.into(),
            parents: 1,
        }
    }

    fn good_facts() -> Facts {
        Facts {
            checkout: "C:/Users/x/repo".into(),
            branch_now: "main".into(),
            head: HEAD.into(),
            status_hash: HASH.into(),
            status_clean: true,
            base: BASE.into(),
            commits: vec![commit(1, "Fix a typo in the README (card #104)\n")],
            files: vec![file("README.md", &["The board has five columns."], &["The board has four columns."])],
            tamper: None,
        }
    }

    fn why(card: &Card, facts: &Facts) -> String {
        decide(card, &entry(), facts, false).expect_err("should stop for Hugh")
    }

    fn why_files(files: Vec<FileChange>) -> String {
        let mut f = good_facts();
        f.files = files;
        why(&good_card(), &f)
    }

    fn assert_has(reason: &str, want: &str) {
        assert!(reason.contains(want), "reason \"{reason}\" should contain \"{want}\"");
    }

    // ---- decide: the passing case and once only -------------------------------------------

    #[test]
    fn docs_change_passes() {
        let ev = decide(&good_card(), &entry(), &good_facts(), false).expect("should go on");
        assert_eq!(ev["files"][0]["kind"], "docs");
    }

    #[test]
    fn went_on_once_already() {
        let mut c = good_card();
        c.fields.insert("smallChange".into(), json!({ "wentOnAt": "2026-11-06T10:00:00Z" }));
        assert_has(&why(&c, &good_facts()), "went on by itself once already");
        // A wentOnAt that isn't a string doesn't count.
        c.fields.insert("smallChange".into(), json!({ "wentOnAt": null }));
        assert!(decide(&c, &entry(), &good_facts(), false).is_ok());
    }

    // ---- decide: the verdicts --------------------------------------------------------------

    fn with_verdict(key: &str, field: &str, value: Option<Value>) -> Card {
        let mut c = good_card();
        let v = c.fields.get_mut(key).unwrap().as_object_mut().unwrap();
        match value {
            Some(x) => v.insert(field.into(), x),
            None => v.remove(field),
        };
        c
    }

    #[test]
    fn verdict_fields_wrong() {
        for (key, who) in [("test", "the tester"), ("review", "the reviewer")] {
            let cases: Vec<(&str, Option<Value>, &str)> = vec![
                ("state", Some(json!("failed")), "hasn't passed it"),
                ("state", None, "hasn't passed it"),
                ("agentId", Some(json!("")), "names no agent"),
                ("agentId", None, "names no agent"),
                ("tools", Some(json!(0)), "shows no tool use"),
                ("tools", None, "shows no tool use"),
                ("tools", Some(json!("7")), "shows no tool use"),
                ("checkout", Some(json!("C:\\Users\\x\\other")), "another checkout"),
                ("head", Some(json!(BASE)), "an older commit"),
                ("statusHash", Some(json!("3333")), "another state"),
            ];
            for (field, value, want) in cases {
                let r = why(&with_verdict(key, field, value.clone()), &good_facts());
                assert_has(&r, want);
                assert!(r.starts_with(who), "{key}.{field}={value:?}: {r}");
            }
            // No verdict at all.
            let mut c = good_card();
            c.fields.remove(key);
            assert_has(&why(&c, &good_facts()), &format!("{who} hasn't passed it"));
        }
    }

    #[test]
    fn an_unpinned_verdict_says_why() {
        for (key, who) in [("test", "the tester"), ("review", "the reviewer")] {
            // The work changed while it ran (seen on the test app, card #130).
            let r = why(&with_verdict(key, "placeMoved", Some(json!(true))), &good_facts());
            assert_eq!(r, format!("the work changed while {who} ran, so its pass doesn't cover the final commit"));
            for field in ["checkout", "head"] {
                let r = why(&with_verdict(key, field, None), &good_facts());
                assert!(r.starts_with(who) && r.contains("wasn't pinned to a commit"), "{key}.{field}: {r}");
            }
        }
    }

    #[test]
    fn review_scope_narrowed_or_absent() {
        let r = why(&with_verdict("review", "scope", Some(json!("narrowed"))), &good_facts());
        assert_eq!(r, "the review was narrowed by its brief (SCOPE: narrowed)");
        let r = why(&with_verdict("review", "scope", None), &good_facts());
        assert_has(&r, "no SCOPE");
        // The tester's verdict needs no scope.
        assert!(decide(&with_verdict("test", "scope", None), &entry(), &good_facts(), false).is_ok());
    }

    // ---- decide: the checkout --------------------------------------------------------------

    #[test]
    fn checkout_wrong() {
        let mut f = good_facts();
        f.checkout = "C:/Users/x/elsewhere".into();
        assert_has(&why(&good_card(), &f), "isn't the one on Hugh's list");

        let mut f = good_facts();
        f.branch_now = "dsh-104".into();
        assert_eq!(why(&good_card(), &f), "the checkout is on dsh-104, not main");

        let mut f = good_facts();
        f.branch_now = String::new();
        assert_has(&why(&good_card(), &f), "detached");

        let mut f = good_facts();
        f.tamper = Some("the repo has replace refs".into());
        assert_has(&why(&good_card(), &f), "git's view may be false: the repo has replace refs");

        let mut f = good_facts();
        f.status_clean = false;
        assert_eq!(why(&good_card(), &f), "the checkout has changes that aren't committed");
    }

    // ---- decide: the commits ---------------------------------------------------------------

    #[test]
    fn commits_wrong() {
        let mut f = good_facts();
        f.commits.clear();
        assert_has(&why(&good_card(), &f), "no commits");

        let mut f = good_facts();
        f.commits = (0..21).map(|n| commit(n, "x (card #104)")).collect();
        assert_has(&why(&good_card(), &f), "21 commits, more than 20");
        f.commits.pop();
        assert!(decide(&good_card(), &entry(), &f, false).is_ok(), "20 commits are allowed");

        let mut f = good_facts();
        f.commits[0].parents = 2;
        assert_has(&why(&good_card(), &f), "is a merge commit");

        let mut f = good_facts();
        f.commits.push(commit(2, "Tidy the docs\n\nNo card here."));
        let r = why(&good_card(), &f);
        assert_has(&r, "commit 0000002 (Tidy the docs) doesn't name the card as (card #104)");

        let mut f = good_facts();
        f.commits = vec![commit(1, "Tidy (card #10)")];
        assert_has(&why(&good_card(), &f), "doesn't name the card");
        f.commits = vec![commit(1, "Tidy (card #1040)")];
        assert_has(&why(&good_card(), &f), "doesn't name the card");
        // The tag may sit in the body.
        f.commits = vec![commit(1, "Tidy\n\nPart of (card #104).")];
        assert!(decide(&good_card(), &entry(), &f, false).is_ok());
    }

    // ---- decide: the size ------------------------------------------------------------------

    #[test]
    fn size_wrong() {
        assert_eq!(why_files(vec![]), "it changes no files");

        let six = (0..6).map(|n| file(&format!("doc{n}.md"), &["a"], &[])).collect();
        assert_has(&why_files(six), "6 files, more than 5");
        let five: Vec<_> = (0..5).map(|n| file(&format!("doc{n}.md"), &["a"], &[])).collect();
        let mut f = good_facts();
        f.files = five;
        assert!(decide(&good_card(), &entry(), &f, false).is_ok(), "5 files are allowed");

        let a: Vec<&str> = vec!["x"; 16];
        let r: Vec<&str> = vec!["y"; 15];
        assert_has(&why_files(vec![file("README.md", &a, &r)]), "31 lines, more than 30");
        let mut f = good_facts();
        f.files = vec![file("README.md", &a, &r[..14])];
        assert!(decide(&good_card(), &entry(), &f, false).is_ok(), "30 lines are allowed");

        let big = "b".repeat(1500);
        let one = "c".to_string();
        let r = why_files(vec![file("README.md", &[big.as_str(), big.as_str()], &[one.as_str()])]);
        assert_has(&r, "3001 bytes, more than 3000");
        let mut f = good_facts();
        f.files = vec![file("README.md", &[big.as_str(), big.as_str()], &[])];
        assert!(decide(&good_card(), &entry(), &f, false).is_ok(), "3000 bytes are allowed");
    }

    #[test]
    fn status_mode_and_binary_wrong() {
        for (status, want) in [("A", "is a new file"), ("D", "is deleted"), ("T", "changes type"), ("U", "isn't a plain edit")] {
            let mut f = file("README.md", &["a"], &[]);
            f.status = status.into();
            assert_has(&why_files(vec![f]), want);
        }
        let mut f = file("README.md", &["a"], &[]);
        f.new_mode = "100755".into();
        assert_has(&why_files(vec![f]), "changes its file mode");

        let mut f = file("README.md", &["a"], &[]);
        f.old_mode = "120000".into();
        f.new_mode = "120000".into();
        assert_has(&why_files(vec![f]), "isn't a plain file");

        let mut f = file("README.md", &[], &[]);
        f.binary = true;
        assert_eq!(why_files(vec![f]), "README.md is a binary file");
    }

    // ---- decide: the kind of file and the paths --------------------------------------------

    #[test]
    fn kind_wrong() {
        assert_eq!(why_files(vec![file("notes.txt", &["a"], &[])]), "notes.txt isn't a docs or CSS file");
        assert_eq!(
            why_files(vec![file("src-tauri/src/x.rs", &["a"], &[])]),
            "src-tauri/src/x.rs isn't a docs or CSS file"
        );
        // A good file first doesn't hide a bad one after it.
        assert_has(&why_files(vec![file("README.md", &["a"], &[]), file("ui/app.js", &["a"], &[])]), "ui/app.js");
        // .rst and upper-case extensions are docs.
        let mut f = good_facts();
        f.files = vec![file("docs/Guide.RST", &["a"], &[]), file("NOTES.MD", &["a"], &[])];
        assert!(decide(&good_card(), &entry(), &f, false).is_ok());
    }

    #[test]
    fn segments_off_limits() {
        for p in ["agents/x.md", "skills/kanban/README.md", ".claude/notes.md", "a/commands/x.md", "hooks/x.md",
                  "ui/vendor/x.css", "prompts/x.md", "scripts/x.md", ".github/x.md"] {
            let r = why_files(vec![file(p, &["a"], &[])]);
            assert_has(&r, p);
            assert_has(&r, "off limits");
        }
    }

    #[test]
    fn words_and_names_off_limits() {
        for w in PATH_WORDS {
            let p = format!("docs/{w}-notes.md");
            let r = why_files(vec![file(&p, &["a"], &[])]);
            assert_has(&r, &p);
            assert_has(&r, "off limits");
        }
        assert_has(&why_files(vec![file("docs/Prompt-Guide.md", &["a"], &[])]), "\"prompt\"");
        for name in ["AGENTS.md", "CLAUDE.md", "docs/SKILL.md", "ui/Agents.md"] {
            assert_has(&why_files(vec![file(name, &["a"], &[])]), "a file agents read as instructions");
        }
        // .mcp.json isn't docs, so it stops a step earlier; it stops all the same.
        assert_eq!(why_files(vec![file(".mcp.json", &["a"], &[])]), ".mcp.json isn't a docs or CSS file");
    }

    // ---- decide: the added lines -----------------------------------------------------------

    #[test]
    fn added_lines_refused() {
        let sql_del = concat!("DELETE", " FROM cards");
        let sql_del_lower = format!("{} {}", "delete", "from x");
        for line in [
            sql_del, sql_del_lower.as_str(), "DROP TABLE cards", "drop index i", "Drop View v",
            "DROP TRIGGER t", "DROP COLUMN c", "DROP DATABASE d", "ALTER TABLE cards",
            "use the sign-in page", "signin", "a token", "the TOKEN", "a secret", "api key", "API_KEY", "apikey",
            "api-key", "Bearer abc", "the password is here", "?ct=abc", "eyJhbGciOiJIUzI1NiJ9",
        ] {
            let r = why_files(vec![file("README.md", &[line], &[])]);
            assert_has(&r, "an added line in README.md");
        }
        // The reason never quotes the line, which may hold the secret itself.
        let r = why_files(vec![file("README.md", &["the password is hunter2"], &[])]);
        assert!(!r.contains("hunter2"), "{r}");
        // Removed lines aren't checked for these words.
        let mut f = good_facts();
        f.files = vec![file("README.md", &[], &["the old token note"])];
        assert!(decide(&good_card(), &entry(), &f, false).is_ok());
    }

    #[test]
    fn only_sql_drop_counts() {
        let mut f = good_facts();
        f.files = vec![file("README.md", &["Drop a card on a Ship it card to ship them as one."], &[])];
        assert!(decide(&good_card(), &entry(), &f, false).is_ok());
        assert_has(&why_files(vec![file("README.md", &["DROP TABLE cards"], &[])]), "an SQL drop");
    }

    // ---- decide: CSS -----------------------------------------------------------------------

    #[test]
    fn css_properties_refused() {
        for prop in ["display", "visibility", "pointer-events", "position", "z-index", "opacity", "content"] {
            for line in [format!("  {prop}: none;"), format!(".a{{{prop}:none}}"), format!("color: red; {}: x", prop.to_uppercase())] {
                let added = decide(&good_card(), &entry(), &{
                    let mut f = good_facts();
                    f.files = vec![file("ui/app.css", &[line.as_str()], &[])];
                    f
                }, true);
                assert_has(&added.expect_err("added"), &format!("an added line in ui/app.css sets {prop}"));
                let removed = decide(&good_card(), &entry(), &{
                    let mut f = good_facts();
                    f.files = vec![file("ui/app.css", &[], &[line.as_str()])];
                    f
                }, true);
                assert_has(&removed.expect_err("removed"), &format!("a removed line in ui/app.css sets {prop}"));
            }
        }
        // A property whose ":" is on the next line.
        let mut f = good_facts();
        f.files = vec![file("ui/app.css", &["  display"], &[])];
        assert!(decide(&good_card(), &entry(), &f, true).is_err());
        // @import pulls in CSS the check never sees.
        f.files = vec![file("ui/app.css", &["@import url(x.css);"], &[])];
        assert_has(&decide(&good_card(), &entry(), &f, true).unwrap_err(), "imports more CSS");
    }

    #[test]
    fn css_lookalikes_pass() {
        let mut f = good_facts();
        f.files = vec![file(
            "ui/app.css",
            &["  background-position: 0 0;", "  align-content: center;", "  color: #333;"],
            &["  background-position: 1px 0;", "  justify-content: start;"],
        )];
        assert!(decide(&good_card(), &entry(), &f, true).is_ok());
    }

    #[test]
    fn css_needs_evidence() {
        let mut f = good_facts();
        f.files = vec![file("ui/app.css", &["  color: #333;"], &["  color: #444;"])];
        assert_eq!(why(&good_card(), &f), "a CSS change needs #101's before-and-after screenshots");
        let ev = decide(&good_card(), &entry(), &f, true).expect("with evidence");
        assert_eq!(ev["files"][0]["kind"], "css");
        // Docs need no screenshots.
        assert!(decide(&good_card(), &entry(), &good_facts(), false).is_ok());
    }

    // ---- decide: the evidence --------------------------------------------------------------

    #[test]
    fn evidence_shape() {
        let mut f = good_facts();
        f.files.push(file("ui/app.css", &["  color: #333;"], &[]));
        let ev = decide(&good_card(), &entry(), &f, true).unwrap();
        let c1 = commit(1, "x");
        assert_eq!(
            ev,
            json!({
                "checkout": "C:\\Users\\x\\repo",
                "branch": "main",
                "base": BASE,
                "head": HEAD,
                "commits": [{ "sha": c1.sha, "subject": "Fix a typo in the README (card #104)" }],
                "files": [
                    { "path": "README.md", "kind": "docs", "added": 1, "removed": 1 },
                    { "path": "ui/app.css", "kind": "css", "added": 1, "removed": 0 },
                ],
                "lines": 3,
                "bytes": "The board has five columns.".len() + "The board has four columns.".len() + "  color: #333;".len(),
                "test": { "agentId": "agent-1", "tools": 7, "head": HEAD },
                "review": { "agentId": "agent-1", "tools": 7, "head": HEAD, "scope": "full" },
            })
        );
    }

    // ---- skip ------------------------------------------------------------------------------

    #[test]
    fn skip_conditions() {
        assert!(!skip(&good_card()), "a plain card isn't skipped");
        let mut c = good_card();
        c.archived = true;
        assert!(skip(&c), "archived");
        for (k, v, skipped) in [
            ("stackOf", json!(96), true),
            ("stack", json!([97, 98]), true),
            ("stack", json!([]), false),
            ("autoTo", json!("Deploy complete"), true),
            ("autoTo", json!(""), false),
            ("smallOptOut", json!(true), true),
            ("smallOptOut", json!(false), false),
        ] {
            let mut c = good_card();
            c.fields.insert(k.into(), v.clone());
            assert_eq!(skip(&c), skipped, "{k} = {v}");
        }
    }

    // ---- the source itself -----------------------------------------------------------------

    #[test]
    fn source_has_no_sql_delete_text() {
        let re = Regex::new(r"(?i)\bdelete\s+from\b").unwrap();
        assert!(!re.is_match(include_str!("small.rs")));
    }

    // ---- git_facts and snapshot on a real repo ---------------------------------------------

    /// Runs the test's own git: identity given, no system or global config.
    fn sh(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", dir.join("no-such-gitconfig"))
            .stdin(Stdio::null())
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A bare origin and a clone of it, with one commit pushed to origin main. Returns the
    /// temp dir and the clone's path.
    fn repo() -> (tempfile::TempDir, std::path::PathBuf) {
        let t = tempfile::tempdir().unwrap();
        sh(t.path(), &["init", "-q", "--bare", "-b", "main", "origin.git"]);
        sh(t.path(), &["clone", "-q", "origin.git", "work"]);
        let w = t.path().join("work");
        sh(&w, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        std::fs::write(w.join("README.md"), "# Title\n\nThe board has four columns.\n").unwrap();
        std::fs::write(w.join("notes.txt"), "keep\n").unwrap();
        sh(&w, &["add", "."]);
        sh(&w, &["commit", "-q", "-m", "First"]);
        sh(&w, &["push", "-q", "origin", "main"]);
        (t, w)
    }

    fn edit_readme(w: &Path) {
        std::fs::write(w.join("README.md"), "# Title\n\nThe board has five columns.\n").unwrap();
        sh(w, &["commit", "-q", "-am", "Fix the column count (card #104)"]);
    }

    fn s(p: &Path) -> &str {
        p.to_str().unwrap()
    }

    #[test]
    fn git_facts_of_one_docs_commit() {
        let (_t, w) = repo();
        let base = sh(&w, &["rev-parse", "HEAD"]);
        edit_readme(&w);
        let f = git_facts(s(&w), "main").expect("facts");
        assert_eq!(norm_path(&f.checkout), norm_path(s(&w)));
        assert_eq!(f.branch_now, "main");
        assert_eq!(f.head, sh(&w, &["rev-parse", "HEAD"]));
        assert_eq!(f.base, base);
        assert!(f.status_clean);
        // The empty blob: `git status` printed nothing.
        assert_eq!(f.status_hash, "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
        assert_eq!(f.tamper, None);
        assert_eq!(f.commits.len(), 1);
        assert_eq!(f.commits[0].sha, f.head);
        assert_eq!(f.commits[0].subject, "Fix the column count (card #104)");
        assert!(f.commits[0].message.contains("(card #104)"));
        assert_eq!(f.commits[0].parents, 1);
        assert_eq!(
            f.files,
            vec![FileChange {
                path: "README.md".into(),
                status: "M".into(),
                old_mode: "100644".into(),
                new_mode: "100644".into(),
                binary: false,
                added: 1,
                removed: 1,
                added_lines: vec!["The board has five columns.".into()],
                removed_lines: vec!["The board has four columns.".into()],
            }]
        );

        // And decide lets it go on, with verdicts for this very checkout state.
        let mut c = good_card();
        for k in ["test", "review"] {
            let v = c.fields.get_mut(k).unwrap();
            v["checkout"] = json!(s(&w));
            v["head"] = json!(f.head);
            v["statusHash"] = json!(f.status_hash);
        }
        let e = SmallRepo { repo: "t/t".into(), checkout: s(&w).into(), branch: "main".into() };
        let ev = decide(&c, &e, &f, false).expect("should go on");
        assert_eq!(ev["lines"], 2);
    }

    #[test]
    fn git_facts_reads_several_files_and_no_newline() {
        let (_t, w) = repo();
        std::fs::write(w.join("README.md"), "# Title\n\nThe board has four columns.").unwrap(); // no newline
        std::fs::write(w.join("notes.txt"), "keep\nmore\n").unwrap();
        sh(&w, &["commit", "-q", "-am", "Two (card #104)"]);
        let f = git_facts(s(&w), "main").unwrap();
        assert_eq!(f.files.len(), 2);
        assert_eq!(f.files[0].path, "README.md");
        assert_eq!(f.files[0].added_lines, vec!["The board has four columns."]);
        assert_eq!(f.files[0].removed_lines, vec!["The board has four columns."]);
        assert_eq!(f.files[1].path, "notes.txt");
        assert_eq!((f.files[1].added, f.files[1].removed), (1, 0));
        assert_eq!(f.files[1].added_lines, vec!["more"]);
    }

    #[test]
    fn snapshot_matches_git_facts_and_sees_untracked_files() {
        let (_t, w) = repo();
        edit_readme(&w);
        let f = git_facts(s(&w), "main").unwrap();
        let snap = snapshot(s(&w)).unwrap();
        assert_eq!(snap, Snapshot { head: f.head.clone(), status_hash: f.status_hash.clone() });
        assert_eq!(serde_json::to_value(&snap).unwrap()["statusHash"], json!(f.status_hash));

        std::fs::write(w.join("new.md"), "x\n").unwrap();
        let snap2 = snapshot(s(&w)).unwrap();
        assert_eq!(snap2.head, snap.head);
        assert_ne!(snap2.status_hash, snap.status_hash);
        let f2 = git_facts(s(&w), "main").unwrap();
        assert!(!f2.status_clean);
        assert_eq!(f2.status_hash, snap2.status_hash);
    }

    #[test]
    fn modified_tracked_file_is_not_clean() {
        let (_t, w) = repo();
        edit_readme(&w);
        std::fs::write(w.join("notes.txt"), "changed\n").unwrap();
        assert!(!git_facts(s(&w), "main").unwrap().status_clean);
    }

    #[test]
    fn info_attributes_is_tamper() {
        let (_t, w) = repo();
        edit_readme(&w);
        let dir = w.join(".git").join("info");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("attributes"), "*.md -diff\n").unwrap();
        let f = git_facts(s(&w), "main").unwrap();
        assert!(f.tamper.as_deref().unwrap_or("").contains("info/attributes"), "{:?}", f.tamper);
    }

    #[test]
    fn replace_ref_is_tamper() {
        let (_t, w) = repo();
        let first = sh(&w, &["rev-parse", "HEAD"]);
        edit_readme(&w);
        sh(&w, &["replace", "HEAD", &first]);
        let f = git_facts(s(&w), "main").unwrap();
        assert!(f.tamper.as_deref().unwrap_or("").contains("replace refs"), "{:?}", f.tamper);
        // git's view ignores the replacement, so the real change still shows.
        assert_eq!(f.files.len(), 1);
        assert_eq!(f.commits[0].subject, "Fix the column count (card #104)");
    }

    #[test]
    fn diff_external_changes_nothing_and_runs_nothing() {
        let (t, w) = repo();
        edit_readme(&w);
        let clean = git_facts(s(&w), "main").unwrap();
        let marker = t.path().join("external-ran");
        let marker_sh = s(&marker).replace('\\', "/");
        sh(&w, &["config", "diff.external", &format!("echo ran > \"{marker_sh}\"")]);
        let f = git_facts(s(&w), "main").unwrap();
        assert!(!marker.exists(), "the external diff ran");
        assert_eq!(f.files, clean.files);
        assert_eq!(f.commits, clean.commits);
        // The setting is live: the porcelain diff does run it.
        let _ = Command::new("git").current_dir(&w).args(["diff", "HEAD~1", "HEAD"]).stdin(Stdio::null()).output();
        assert!(marker.exists(), "the test's external diff should run under plain git diff");
    }

    #[test]
    fn missing_origin_branch_is_err() {
        let (_t, w) = repo();
        let e = git_facts(s(&w), "nope").unwrap_err();
        assert_has(&e, "origin/nope");
    }

    #[test]
    fn head_behind_origin_is_err() {
        let (_t, w) = repo();
        edit_readme(&w);
        sh(&w, &["push", "-q", "origin", "main"]);
        sh(&w, &["reset", "-q", "--hard", "HEAD~1"]);
        assert_has(&git_facts(s(&w), "main").unwrap_err(), "commits that HEAD doesn't");
    }

    #[test]
    fn detached_head_has_no_branch() {
        let (_t, w) = repo();
        edit_readme(&w);
        sh(&w, &["checkout", "-q", "--detach"]);
        assert_eq!(git_facts(s(&w), "main").unwrap().branch_now, "");
    }
}
