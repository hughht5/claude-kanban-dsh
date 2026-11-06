//! How long a Human test card takes to check (card #103): "30-second look" or "Needs real
//! testing", worked out from the card's diff and recorded as its `checkSize` field.
//!
//! - **When:** every move into Human test removes `checkSize` (the store's
//!   `RESET_ON_HUMAN_TEST`), and `pump`, called from the dispatch loop every few seconds, works
//!   out the cards that have none (`Store::check_size_due`) on a worker thread. Git never runs
//!   while the board is locked. A card whose result wasn't written waits `RETRY_AFTER` before
//!   its next try; each git call stops after `GIT_LIMIT`; the result is written only over the
//!   version it was worked out from, so a stale one never lands after a move.
//! - **The diff:** Windows git with no window, only for a checkout on a Windows drive (never one
//!   inside WSL), read-only (`GIT_OPTIONAL_LOCKS=0`, no fsmonitor, no external diff or
//!   textconv). A card on its own branch: the branch against the nearer of main and
//!   origin/main, plus a check for uncommitted work in its checkout. A card on main (this app's
//!   cards): the commits whose message carries "(card #N)", and its stack children's numbers.
//! - **The rule** (`verdict`): a 30-second look is only a change you judge by reading or by
//!   looking: styles, markup, images and docs, small, with nothing to sign in to. Anything else
//!   needs real testing, with every reason listed. The router's guess (`route.check`, card #98)
//!   can only make it stricter.
//! - **Hidden text:** the store refuses text its secret filter matches, and a file path can match
//!   it. Such a path is hidden everywhere; if a write is still refused, a minimal record goes in.

use kanban_store::{looks_secret, Card, Patch, Store, CHECK_SIZE_KEY};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::Read as _;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// Who the app's writes of `checkSize` are recorded as.
pub const BY: &str = "the app (diff check)";
/// A 30-second look changes at most this many lines, and files, of UI.
pub const UI_LINES: i64 = 80;
pub const UI_FILES: usize = 5;
/// ... and at most this many lines of docs (tests don't count).
pub const DOCS_LINES: i64 = 150;
/// The longest one git call may take before it is stopped.
pub const GIT_LIMIT: Duration = Duration::from_secs(20);
/// How long a card whose result wasn't written waits before its next try.
pub const RETRY_AFTER: Duration = Duration::from_secs(60);
/// Cards worked out per round of the loop.
const BATCH: usize = 5;
/// Files kept in the record (the totals cover them all).
const MAX_PATHS: usize = 30;
/// A path, or other text, the board's secret filter would refuse.
pub const HIDDEN: &str = "(a path hidden: it looks like a secret)";

/// What a changed file is, for the rule. First match wins (`kind_of`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Tests,
    Behaviour,
    Docs,
    Signin,
    Data,
    Build,
    Code,
    Ui,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Tests => "tests",
            Kind::Behaviour => "behaviour",
            Kind::Docs => "docs",
            Kind::Signin => "signin",
            Kind::Data => "data",
            Kind::Build => "build",
            Kind::Code => "code",
            Kind::Ui => "ui",
        }
    }
}

const DOC_EXT: &[&str] = &["md", "mdx", "rst", "adoc"];
const SCRIPT_EXT: &[&str] = &["js", "mjs", "cjs", "jsx", "ts", "tsx", "mts", "cts", "html", "htm", "vue", "svelte"];
/// Styles and pictures. HTML is code: a `<script>` line in it switches a feature on or off.
const UI_EXT: &[&str] = &["css", "scss", "less", "svg"];
const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "ico", "bmp", "avif"];
const BUILD_NAMES: &[&str] = &[
    "cargo.toml", "cargo.lock", "package.json", "package-lock.json", "yarn.lock", "pnpm-lock.yaml",
    "build.rs", "makefile", "docker-compose.yml", "docker-compose.yaml", ".gitignore", ".gitattributes",
];
const BUILD_EXT: &[&str] = &["ps1", "psm1", "sh", "bash", "bat", "cmd", "nsh", "tf", "lock"];
const BUILD_DIRS: &[&str] = &["scripts", ".github", ".husky", "infra", "terraform"];
const TEST_DIRS: &[&str] = &["tests", "test", "__tests__", "e2e", "fixtures", "spec"];
const BEHAVIOUR_DIRS: &[&str] = &["prompts", ".claude", "skills", ".agents"];
const BEHAVIOUR_NAMES: &[&str] = &["agents.md", "claude.md", "skill.md"];

/// Does a path name a sign-in file? By its words (split on anything not a letter or digit):
/// "auth..." (not "author", "authors"; "authorize" is), "oauth", "login", "logout", "signin", "sso", "token" or a word
/// ending in it ("customtoken", "idtoken"; not design "tokens"), "passw...", "credential...",
/// "secret...". "sign-in" and "log_in" are joined first.
pub fn signin_path(p: &str) -> bool {
    let p = p.to_ascii_lowercase().replace("sign-in", "signin").replace("sign_in", "signin").replace("log-in", "login").replace("log_in", "login");
    p.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).any(|w| {
        (w.starts_with("auth") && !(w.starts_with("author") && !w.starts_with("authoriz")))
            || w.contains("oauth")
            || w.contains("login")
            || w.contains("logout")
            || w.contains("signin")
            || w == "sso"
            || w.ends_with("token")
            || w.starts_with("passw")
            || w.starts_with("credential")
            || w.starts_with("secret")
    })
}

/// The kind of the file at `path` (as git prints it, forward slashes). `binary` is git's "-"
/// line count: a binary file that isn't an image is code.
pub fn kind_of(path: &str, binary: bool) -> Kind {
    let p = path.replace('\\', "/").to_ascii_lowercase();
    let segs: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
    let name = segs.last().copied().unwrap_or("");
    let dirs = &segs[..segs.len().saturating_sub(1)];
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let first = dirs.first().copied().unwrap_or("");

    // A test file's name: "a.test.ts", "a.spec.js", "a_test.go", "test_a.py" (not "tauri.test.conf.json").
    let src = (SCRIPT_EXT.contains(&ext) && !["html", "htm"].contains(&ext)) || ["py", "rs", "go"].contains(&ext);
    let test_name = src && (name.contains(".test.") || name.contains(".spec.") || name.contains("_test.") || (name.starts_with("test_") && ext == "py"));
    if dirs.iter().any(|d| TEST_DIRS.contains(d)) || test_name {
        return Kind::Tests;
    }
    if dirs.iter().any(|d| BEHAVIOUR_DIRS.contains(d)) || BEHAVIOUR_NAMES.contains(&name) {
        return Kind::Behaviour;
    }
    let doc_dir = ["docs", "vault", "design"].contains(&first);
    let doc_name = (name.starts_with("license") || name.starts_with("changelog")) && (ext.is_empty() || DOC_EXT.contains(&ext) || ext == "txt");
    if DOC_EXT.contains(&ext) || doc_name || (doc_dir && (ext == "txt" || IMAGE_EXT.contains(&ext))) {
        return Kind::Docs;
    }
    if signin_path(&p) {
        return Kind::Signin;
    }
    let data = ["sql", "prisma"].contains(&ext)
        || dirs.iter().any(|d| d.contains("migration") || *d == "db" || *d == "database")
        || name.starts_with("schema")
        || p.starts_with("crates/store/src/")
        || first == "pipelines"
        || name == "firestore.rules"
        || name == "firestore.indexes.json";
    if data {
        return Kind::Data;
    }
    let build = BUILD_NAMES.contains(&name)
        || name.starts_with("requirements")
        || name == "cmakelists.txt"
        || name == "pyproject.toml"
        || name == "go.mod"
        || name == "go.sum"
        || BUILD_EXT.contains(&ext)
        || dirs.iter().any(|d| BUILD_DIRS.contains(d))
        || name.starts_with("dockerfile")
        || name.starts_with(".env")
        || name.starts_with("tsconfig")
        || name.starts_with("vite.config")
        || name.starts_with("webpack")
        || (name.starts_with("tauri") && ext == "json");
    if build {
        return Kind::Build;
    }
    if SCRIPT_EXT.contains(&ext) {
        return Kind::Code;
    }
    if IMAGE_EXT.contains(&ext) || (UI_EXT.contains(&ext) && !binary) {
        return Kind::Ui;
    }
    Kind::Code
}

/// One changed file.
#[derive(Clone, Debug, PartialEq)]
pub struct FileStat {
    pub path: String,
    pub added: i64,
    pub removed: i64,
    pub binary: bool,
}

/// `git --numstat` output (`added\tremoved\tpath`, `-\t-\tpath` for a binary file), summed per
/// path, in the order first seen. Other lines are skipped. A path that looks like a secret is
/// hidden here, so nothing built from it can hold it.
pub fn parse_numstat(out: &str) -> Vec<FileStat> {
    let mut files: Vec<FileStat> = Vec::new();
    for line in out.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(a), Some(r), Some(path)) = (parts.next(), parts.next(), parts.next()) else { continue };
        let binary = a == "-" && r == "-";
        let (Ok(a), Ok(r)) = (if binary { Ok(0) } else { a.parse::<i64>() }, if binary { Ok(0) } else { r.parse::<i64>() }) else { continue };
        let mut path = path.trim().to_string();
        if path.len() >= 2 && path.starts_with('"') && path.ends_with('"') {
            path = path[1..path.len() - 1].to_string();
        }
        if path.is_empty() {
            continue;
        }
        if looks_secret(&path) {
            path = HIDDEN.to_string();
        }
        match files.iter_mut().find(|f| f.path == path && path != HIDDEN) {
            Some(f) => {
                f.added += a;
                f.removed += r;
                f.binary |= binary;
            }
            None => files.push(FileStat { path, added: a, removed: r, binary }),
        }
    }
    files
}

/// What reading a card's changes gave.
#[derive(Debug, Default, Clone)]
pub struct Reading {
    /// The files, when the diff was read.
    pub files: Option<Vec<FileStat>>,
    /// Where they came from ("branch dsh-103 against main"), or why there are none.
    pub from: String,
    /// Its own checkout has uncommitted changes to tracked files.
    pub uncommitted: bool,
    /// The card's `branch` and its checkout's, when they differ.
    pub mismatch: Option<(String, String)>,
    /// Git failed or timed out: worth trying again before writing a verdict.
    pub transient: bool,
    /// The card numbers whose commits were read (the lead's and its children's).
    pub numbers: Vec<i64>,
}

/// What the rule needs besides the diff.
#[derive(Debug, Default, Clone)]
pub struct Context {
    /// Why checking it needs a sign-in, if it does (empty: it doesn't).
    pub login: Vec<String>,
    /// The router's guess of the check time (`route.check`, card #98).
    pub seed_check: Option<String>,
    /// The card's repo.
    pub repo: String,
    /// The repo's top-level folders a user signs in to, from the private folder's `repos.json`
    /// (`loginDirs`): a change there needs a sign-in to check.
    pub login_dirs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub quick: bool,
    pub why: String,
    pub reasons: Vec<String>,
}

/// "−" with the count, as the board shows it.
fn plus_minus(a: i64, r: i64) -> String {
    format!("+{a} \u{2212}{r}")
}

fn some_paths(files: &[&FileStat]) -> String {
    let mut s: Vec<String> = files.iter().take(3).map(|f| if f.binary { f.path.clone() } else { format!("{} {}", f.path, plus_minus(f.added, f.removed)) }).collect();
    if files.len() > 3 {
        s.push(format!("and {} more", files.len() - 3));
    }
    s.join(", ")
}

/// Minutes in a guess like "5 min", "30 min+" or "30 sec".
pub fn guess_minutes(check: &str) -> Option<f64> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(\d+(?:\.\d+)?)\s*([a-z]*)").expect("guess pattern"));
    let c = check.to_ascii_lowercase();
    let m = re.captures(&c)?;
    let n: f64 = m[1].parse().ok()?;
    let unit = &m[2];
    Some(if unit.starts_with('s') { n / 60.0 } else if unit.starts_with('h') { n * 60.0 } else { n })
}


/// The rule. Every reason a card isn't a 30-second look is listed; the first is the headline.
pub fn verdict(r: &Reading, cx: &Context) -> Verdict {
    let mut reasons: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let files = match &r.files {
        None => {
            reasons.push(format!("the app couldn't read its changes: {}", r.from));
            &[][..]
        }
        Some(f) if f.is_empty() => {
            reasons.push("nothing to check: the diff has no files".into());
            &[][..]
        }
        Some(f) => &f[..],
    };
    if let Some((card, checkout)) = &r.mismatch {
        reasons.push(format!("the card says branch {card} but its checkout is on {checkout}"));
    }
    if r.uncommitted {
        reasons.push("its checkout has uncommitted changes, so the diff isn't the whole change".into());
    }
    let kinded: Vec<(Kind, &FileStat)> = files.iter().map(|f| (kind_of(&f.path, f.binary), f)).collect();
    let of = |k: Kind| kinded.iter().filter(|(x, _)| *x == k).map(|(_, f)| *f).collect::<Vec<_>>();
    for (k, what) in [
        (Kind::Behaviour, "changes how card sessions or agents behave"),
        (Kind::Signin, "touches sign-in"),
        (Kind::Data, "touches data or a migration"),
        (Kind::Build, "changes the build, scripts or dependencies"),
        (Kind::Code, "changes code you check by using it"),
    ] {
        let fs = of(k);
        if !fs.is_empty() {
            reasons.push(format!("{what}: {}", some_paths(&fs)));
        }
    }
    let lines = |fs: &[&FileStat]| fs.iter().map(|f| f.added + f.removed).sum::<i64>();
    let ui = of(Kind::Ui);
    if lines(&ui) > UI_LINES {
        reasons.push(format!("{} lines of UI change (a 30-second look is at most {UI_LINES})", lines(&ui)));
    }
    if ui.len() > UI_FILES {
        reasons.push(format!("{} UI files change (a 30-second look is at most {UI_FILES})", ui.len()));
    }
    let docs = of(Kind::Docs);
    if lines(&docs) > DOCS_LINES {
        reasons.push(format!("{} lines of docs to read (a 30-second look is at most {DOCS_LINES})", lines(&docs)));
    }
    let mut login = cx.login.clone();
    if !cx.login_dirs.is_empty() {
        let top = |f: &&FileStat| f.path.split('/').next().unwrap_or("").to_string();
        let areas: Vec<&FileStat> = files.iter().filter(|f| cx.login_dirs.iter().any(|d| d.eq_ignore_ascii_case(&top(f)))).collect();
        if !areas.is_empty() {
            login.push(format!("it changes an area of {} you sign in to ({})", cx.repo, some_paths(&areas)));
        }
    }
    for l in login {
        reasons.push(format!("needs a sign-in to check: {l}"));
    }
    if let Some(g) = cx.seed_check.as_deref().map(str::trim).filter(|g| !g.is_empty()) {
        match guess_minutes(g) {
            Some(m) if m >= 30.0 => reasons.push(format!("the router expected {g} to check it")),
            _ => notes.push(format!("the router guessed {g}")),
        }
    }
    if !reasons.is_empty() {
        let why = reasons[0].clone();
        reasons.extend(notes);
        return Verdict { quick: false, why, reasons };
    }
    let (a, rm): (i64, i64) = files.iter().fold((0, 0), |(a, r), f| (a + f.added, r + f.removed));
    let mut what: Vec<&str> = Vec::new();
    if !ui.is_empty() {
        what.push("styles or images");
    }
    if !docs.is_empty() {
        what.push("docs");
    }
    if !of(Kind::Tests).is_empty() {
        what.push("tests");
    }
    let why = format!("{} file{}, {}: {} only", files.len(), if files.len() == 1 { "" } else { "s" }, plus_minus(a, rm), what.join(" and "));
    let mut reasons = vec![why.clone()];
    reasons.extend(notes);
    Verdict { quick: true, why, reasons }
}

/// The `checkSize` record.
pub fn record(r: &Reading, cx: &Context, v: &Verdict, at: &str) -> Value {
    let diff = r.files.as_ref().map(|files| {
        let mut kinds: BTreeMap<&str, (usize, i64)> = BTreeMap::new();
        for f in files {
            let e = kinds.entry(kind_of(&f.path, f.binary).name()).or_default();
            e.0 += 1;
            e.1 += f.added + f.removed;
        }
        let kinds: Map<String, Value> = kinds.into_iter().map(|(k, (n, l))| (k.to_string(), json!({ "files": n, "lines": l }))).collect();
        let paths: Vec<Value> = files
            .iter()
            .take(MAX_PATHS)
            .map(|f| json!({ "path": f.path, "added": f.added, "removed": f.removed, "kind": kind_of(&f.path, f.binary).name() }))
            .collect();
        json!({
            "from": r.from,
            "files": files.len(),
            "added": files.iter().map(|f| f.added).sum::<i64>(),
            "removed": files.iter().map(|f| f.removed).sum::<i64>(),
            "kinds": kinds,
            "paths": paths,
            "more": files.len().saturating_sub(MAX_PATHS),
        })
    });
    let mut rec = json!({
        "class": if v.quick { "quick" } else { "real" },
        "why": v.why,
        "reasons": v.reasons,
        "diff": diff.unwrap_or(Value::Null),
        "login": !cx.login.is_empty() || v.reasons.iter().any(|x| x.starts_with("needs a sign-in")),
        "seed": cx.seed_check.as_ref().map(|c| json!({ "check": c })).unwrap_or(Value::Null),
        "cards": r.numbers,
        "at": at,
    });
    hide_secrets(&mut rec);
    rec
}

/// Any string in `v` the board's secret filter would refuse becomes `HIDDEN`.
fn hide_secrets(v: &mut Value) {
    match v {
        Value::String(s) if looks_secret(s) => *s = HIDDEN.to_string(),
        Value::Array(a) => a.iter_mut().for_each(hide_secrets),
        Value::Object(o) => o.values_mut().for_each(hide_secrets),
        _ => {}
    }
}

/// The record that goes in when the full one is refused: real, with no details.
pub fn minimal_record(at: &str) -> Value {
    json!({ "class": "real", "why": "details hidden", "reasons": ["details hidden"], "diff": Value::Null, "login": false, "seed": Value::Null, "at": at })
}

/// The card's log line for a record.
pub fn log_line(rec: &Value) -> String {
    let class = if rec["class"] == "quick" { "30-second look" } else { "Needs real testing" };
    let line = format!("Time to check: {class}: {}", rec["why"].as_str().unwrap_or(""));
    if looks_secret(&line) {
        format!("Time to check: {class}: details hidden")
    } else {
        line
    }
}

// ---- git ----

/// Runs `cmd`, stopping it after `limit`. Its standard output on success.
fn run_limited(mut cmd: Command, limit: Duration) -> Result<String, String> {
    {
        use crate::platform::NoWindow;
        cmd.no_window();
    }
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| format!("couldn't run git: {e}"))?;
    let mut out = child.stdout.take().ok_or("no output")?;
    let reader = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = out.read_to_end(&mut s);
        String::from_utf8_lossy(&s).into_owned()
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let text = reader.join().unwrap_or_default();
                return if status.success() { Ok(text) } else { Err(format!("git failed ({status})")) };
            }
            Ok(None) if start.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("git took longer than {} s", limit.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return Err(format!("git failed: {e}")),
        }
    }
}

fn git(dir: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["-c", "core.fsmonitor=false", "-c", "core.quotepath=off", "-C", dir])
        .args(args);
    run_limited(cmd, GIT_LIMIT)
}

/// A branch name safe to hand git: no option, no revision syntax.
pub fn branch_ok(b: &str) -> bool {
    !b.is_empty() && !b.starts_with('-') && !b.contains("..") && b.chars().all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c))
}

/// The regex for a card's commits: "(card #N)", "(card #N as shipped)", "(card #N, #M)".
pub fn card_grep(n: i64) -> String {
    format!(r"\(card #{n}[) ,]")
}

fn resolves(dir: &str, rev: &str) -> bool {
    git(dir, &["rev-parse", "--verify", "--quiet", &format!("{rev}^{{commit}}")]).map_or(false, |o| !o.trim().is_empty())
}

/// The base to diff `branch` against: of main and origin/main (or master), the one whose
/// merge-base with the branch is newest. Its name and that merge-base.
fn nearest_base(dir: &str, branch: &str) -> Option<(String, String)> {
    let mut best: Option<(String, String)> = None;
    for cand in ["main", "origin/main", "master", "origin/master"] {
        if cand == branch || !resolves(dir, cand) {
            continue;
        }
        let Ok(mb) = git(dir, &["merge-base", cand, branch]) else { continue };
        let mb = mb.trim().to_string();
        if mb.is_empty() {
            continue;
        }
        best = match best {
            None => Some((cand.to_string(), mb)),
            Some((n, b)) => {
                // The later merge-base is nearer: the old one is its ancestor.
                if b != mb && git(dir, &["merge-base", "--is-ancestor", &b, &mb]).is_ok() {
                    Some((cand.to_string(), mb))
                } else {
                    Some((n, b))
                }
            }
        };
    }
    best
}

/// `git log <range> --numstat` of the commits marked with any of `numbers`: (commits, files).
fn marked_commits(dir: &str, range: &str, numbers: &[i64]) -> Result<(usize, Vec<FileStat>), String> {
    let mut args: Vec<String> = vec!["log".into(), range.into(), "--no-merges".into(), "-E".into()];
    for n in numbers {
        args.push(format!("--grep={}", card_grep(*n)));
    }
    args.extend(["--numstat".into(), "--no-renames".into(), "--no-ext-diff".into(), "--no-textconv".into(), "--format=%x01".into(), "--".into()]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = git(dir, &refs)?;
    let commits = out.lines().filter(|l| l.starts_with('\u{1}')).count();
    Ok((commits, parse_numstat(&out)))
}

fn marked_label(numbers: &[i64]) -> String {
    numbers.iter().map(|n| format!("(card #{n})")).collect::<Vec<_>>().join(" or ")
}

/// Reads a card's changes from its checkout. `numbers` are the card's own number and, for a
/// stack's lead, its children's.
/// - On main or master: the commits marked with those numbers.
/// - On a branch: the commits since it left the nearer of main and origin/main. When some of them
///   are marked with the card's numbers, only those count, so a branch made from another
///   unmerged branch (as the DSH branches are) isn't charged with its parent's work. Otherwise
///   the whole diff from the merge-base.
pub fn read(worktree: Option<&str>, card_branch: Option<&str>, numbers: &[i64]) -> Reading {
    let base = Reading { numbers: numbers.to_vec(), ..Default::default() };
    let none = |why: &str| Reading { files: None, from: why.to_string(), ..base.clone() };
    let failed = |why: String, mismatch: Option<(String, String)>| Reading { files: None, from: why, transient: true, mismatch, ..base.clone() };
    let Some(dir) = worktree.map(str::trim).filter(|w| !w.is_empty()) else { return none("the card has no worktree") };
    if crate::stack::in_wsl(dir) {
        return none("its checkout is inside WSL, which the app doesn't open");
    }
    if !std::path::Path::new(dir).is_dir() {
        return none("its worktree folder isn't there");
    }
    let head = match git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        Ok(h) => h.trim().to_string(),
        Err(e) if e.contains("longer than") => return failed(format!("git didn't answer ({e})"), None),
        Err(e) => return none(&format!("its worktree isn't a git checkout ({e})")),
    };
    let card_branch = card_branch.map(str::trim).filter(|b| !b.is_empty());
    let mut mismatch = None;
    let branch = if head != "HEAD" && !head.is_empty() {
        if let Some(cb) = card_branch {
            if cb != head {
                mismatch = Some((cb.to_string(), head.clone()));
            }
        }
        head.clone()
    } else if let Some(cb) = card_branch {
        cb.to_string()
    } else {
        return none("its checkout isn't on a branch");
    };
    if !branch_ok(&branch) {
        return Reading { mismatch, ..none("its branch name isn't one the app will pass to git") };
    }
    let label = marked_label(numbers);
    if branch == "main" || branch == "master" {
        // A card built on main: its own commits, by number. Uncommitted work isn't checked: the
        // main checkout is shared by many cards.
        return match marked_commits(dir, &branch, numbers) {
            Err(e) => failed(format!("git log failed: {e}"), mismatch),
            Ok((0, _)) => Reading { mismatch, ..none(&format!("no commits on {branch} mention {label}")) },
            Ok((n, files)) => Reading {
                files: Some(files),
                from: format!("{n} commit{} on {branch} marked {label}", if n == 1 { "" } else { "s" }),
                mismatch,
                ..base.clone()
            },
        };
    }
    let Some((base_name, mb)) = nearest_base(dir, &branch) else { return Reading { mismatch, ..none("the app couldn't find main to compare the branch with") } };
    // Uncommitted work counts only when the checkout is on the branch read.
    let uncommitted = branch == head && git(dir, &["status", "--porcelain", "--untracked-files=no"]).map_or(false, |o| !o.trim().is_empty());
    match marked_commits(dir, &format!("{mb}..{branch}"), numbers) {
        Err(e) => return failed(format!("git log failed: {e}"), mismatch),
        Ok((n, files)) if n > 0 => {
            return Reading {
                files: Some(files),
                from: format!("{n} commit{} on branch {branch} marked {label}, since it left {base_name}", if n == 1 { "" } else { "s" }),
                uncommitted,
                mismatch,
                ..base.clone()
            }
        }
        Ok(_) => {}
    }
    match git(dir, &["diff", "--numstat", "--no-renames", "--no-ext-diff", "--no-textconv", &mb, &branch, "--"]) {
        Ok(out) => Reading { files: Some(parse_numstat(&out)), from: format!("branch {branch} against {base_name}"), uncommitted, mismatch, ..base.clone() },
        Err(e) => failed(format!("git diff failed: {e}"), mismatch),
    }
}

// ---- the card ----

/// Why checking `card` needs a sign-in: its logins, its dev stack, or its hand-over's routing
/// line ("Route: login", not "no-login"). A repo's sign-in folders are checked in `verdict`.
pub fn login_reasons(card: &Card, log: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let logins = card.fields.get("logins").and_then(Value::as_array).map_or(0, |a| a.len());
    if logins > 0 {
        out.push(format!("it has {logins} login{} to a dev stack", if logins == 1 { "" } else { "s" }));
    }
    if card.fields.get("devStack").and_then(Value::as_str).map_or(false, |s| !s.trim().is_empty()) {
        out.push("it has a dev stack to sign in to".into());
    }
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?i)\broute:\s*login\b").expect("route pattern"));
    if log.iter().any(|l| re.is_match(l)) {
        out.push("its hand-over routed it as a login card".into());
    }
    out
}

/// One card to work out, read under the board lock.
struct Job {
    id: String,
    version: i64,
    worktree: Option<String>,
    branch: Option<String>,
    numbers: Vec<i64>,
    cx: Context,
}

fn job(store: &Store, card: &Card) -> Job {
    let s = |k: &str| card.fields.get(k).and_then(Value::as_str).map(str::to_string);
    let mut numbers = vec![card.number];
    if card.is_lead() {
        numbers.extend(store.stack_children(card).unwrap_or_default().iter().map(|c| c.number));
    }
    let log: Vec<String> = store.log(&card.id).unwrap_or_default().into_iter().map(|e| e.text).collect();
    Job {
        id: card.id.clone(),
        version: card.version,
        worktree: s("worktreePath"),
        branch: s("branch"),
        numbers,
        cx: Context {
            login: login_reasons(card, &log),
            seed_check: card.fields.get("route").and_then(|r| r.get("check")).and_then(Value::as_str).map(str::to_string),
            login_dirs: crate::private::login_dirs(&s("repo").unwrap_or_default()),
            repo: s("repo").unwrap_or_default(),
        },
    }
}

/// What `write` did.
#[derive(Debug, PartialEq)]
pub enum Wrote {
    Written,
    /// The card changed while git ran: try again next round.
    Stale,
    /// The store refused both records, or failed.
    Failed,
}

/// Writes `rec` over the card's `version`, with its log line; the minimal record if the store
/// refuses it. A card that changed meanwhile, left Human test or got a record is `Stale`.
pub fn write(store: &mut Store, id: &str, version: i64, rec: Value) -> Wrote {
    let Ok(card) = store.get(id) else { return Wrote::Failed };
    if card.version != version || card.column != "Human test" || card.fields.contains_key(CHECK_SIZE_KEY) {
        return Wrote::Stale;
    }
    let at = rec["at"].as_str().unwrap_or("").to_string();
    for rec in [rec, minimal_record(&at)] {
        let mut p = Patch::new();
        p.insert(CHECK_SIZE_KEY.into(), rec.clone());
        if store.update(id, version, &p, BY).is_ok() {
            let _ = store.add_log(id, &log_line(&rec), BY);
            return Wrote::Written;
        }
    }
    Wrote::Failed
}

static RUNNING: AtomicBool = AtomicBool::new(false);

/// Clears `RUNNING` however the worker ends, a panic included.
struct Running;
impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
    }
}

/// Git failures in a row before the "couldn't read" verdict is written anyway.
const TRANSIENT_TRIES: u32 = 3;

/// Per card: when it was last tried without a result written, and how many times in a row.
fn tried() -> &'static Mutex<HashMap<String, (Instant, u32)>> {
    static T: OnceLock<Mutex<HashMap<String, (Instant, u32)>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The card numbers a lead's check covers now: its own and its children's.
fn stack_numbers(card: &Card) -> Vec<i64> {
    let mut n = vec![card.number];
    if let Some(kids) = card.fields.get("stack").and_then(Value::as_array) {
        n.extend(kids.iter().filter_map(Value::as_i64));
    }
    n
}

/// A lead whose stack's cards changed since its check was worked out: its record is removed, so
/// the loop works it out again.
fn clear_changed_stacks(store: &mut Store) {
    for card in store.check_size_leads().unwrap_or_default() {
        let had: Vec<i64> = card.fields.get(CHECK_SIZE_KEY).and_then(|c| c.get("cards")).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default();
        let mut now = stack_numbers(&card);
        let mut was = had.clone();
        now.sort_unstable();
        was.sort_unstable();
        if now != was {
            let mut p = Patch::new();
            p.insert(CHECK_SIZE_KEY.into(), Value::Null);
            if store.update(&card.id, card.version, &p, BY).is_ok() {
                let _ = store.add_log(&card.id, "Time to check: the stack's cards changed, so it is worked out again", BY);
            }
        }
    }
}

/// The dispatch loop's step: works out the Human test cards that have no `checkSize`, on a
/// worker thread, one batch at a time.
pub fn pump(app: &AppHandle) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let guard = Running;
    let jobs: Vec<Job> = {
        let board = app.state::<crate::Board>();
        let Ok(mut store) = board.0.lock() else { return };
        clear_changed_stacks(&mut store);
        let wait = tried().lock().map(|t| t.clone()).unwrap_or_default();
        // Every due card is listed (Human test is short), so cards waiting to retry never
        // crowd out a newer one.
        store
            .check_size_due(10_000)
            .unwrap_or_default()
            .iter()
            .filter(|c| wait.get(&c.id).map_or(true, |(at, _)| at.elapsed() > RETRY_AFTER))
            .take(BATCH)
            .map(|c| job(&store, c))
            .collect()
    };
    if jobs.is_empty() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let _guard = guard;
        let mut wrote = false;
        for j in jobs {
            // Git runs here, with the board let go.
            let reading = read(j.worktree.as_deref(), j.branch.as_deref(), &j.numbers);
            let tries = tried().lock().ok().and_then(|t| t.get(&j.id).map(|x| x.1)).unwrap_or(0);
            if reading.transient && tries + 1 < TRANSIENT_TRIES {
                if let Ok(mut t) = tried().lock() {
                    t.insert(j.id.clone(), (Instant::now(), tries + 1));
                }
                continue;
            }
            let v = verdict(&reading, &j.cx);
            let rec = record(&reading, &j.cx, &v, &kanban_now());
            let res = match app.state::<crate::Board>().0.lock() {
                Ok(mut store) => write(&mut store, &j.id, j.version, rec),
                Err(_) => Wrote::Failed,
            };
            if let Ok(mut t) = tried().lock() {
                match res {
                    Wrote::Written => {
                        t.remove(&j.id);
                    }
                    // Changed while git ran: next round, with no wait.
                    Wrote::Stale => {
                        t.remove(&j.id);
                    }
                    Wrote::Failed => {
                        t.insert(j.id.clone(), (Instant::now(), tries + 1));
                    }
                }
            }
            wrote |= res == Wrote::Written;
        }
        if wrote {
            let _ = app.emit("cards-changed", ());
        }
    });
}

fn kanban_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Hugh's "Work it out again": removes the card's `checkSize`, so the loop works it out afresh
/// within seconds.
#[tauri::command]
pub fn check_size_again(app: AppHandle, board: tauri::State<crate::Board>, id: String) -> Result<(), String> {
    let mut store = board.0.lock().map_err(|e| e.to_string())?;
    let card = store.get(&id).map_err(|e| e.to_string())?;
    if card.column != "Human test" {
        return Err("only a card in Human test has a time to check".into());
    }
    let mut p = Patch::new();
    p.insert(CHECK_SIZE_KEY.into(), Value::Null);
    store.update(&id, card.version, &p, "Hugh (board)").map_err(|e| e.to_string())?;
    drop(store);
    if let Ok(mut t) = tried().lock() {
        t.remove(&id);
    }
    let _ = app.emit("cards-changed", ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fs(path: &str, a: i64, r: i64) -> FileStat {
        FileStat { path: path.into(), added: a, removed: r, binary: false }
    }
    fn reading(files: Vec<FileStat>) -> Reading {
        Reading { files: Some(files), from: "branch b against main".into(), ..Default::default() }
    }
    fn cx() -> Context {
        Context { repo: "hughht5/kanban-app".into(), ..Default::default() }
    }

    #[test]
    fn kinds() {
        for p in ["prompts/planning.md", ".claude/skills/x/SKILL.md", "skills/kanban/SKILL.md", "AGENTS.md", "CLAUDE.md", "sub/SKILL.md", ".claude/agents/r.md", ".agents/skills/a.md"] {
            assert_eq!(kind_of(p, false), Kind::Behaviour, "{p}");
        }
        for p in ["README.md", "docs/x.png", "vault/Features/Login.md", "design/stacked-cards/SPEC.md", "docs/notes.txt", "LICENSE"] {
            assert_eq!(kind_of(p, false), Kind::Docs, "{p}");
        }
        for p in ["crates/store/tests/auto.rs", "e2e/tests/a.spec.ts", "server/x.test.ts", "fixtures/m.json", "a_test.go"] {
            assert_eq!(kind_of(p, false), Kind::Tests, "{p}");
        }
        for p in ["ui/board.js", "ui/x.mjs", "frontends/dashboard/src/A.tsx", "server/src/a.ts", "src-tauri/src/dispatch.rs", "docs/run.js", "docs/conf.py", "design/x.rs", "ui/index.html", "notes.txt"] {
            assert_eq!(kind_of(p, false), Kind::Code, "{p}");
        }
        for p in ["ui/board.css", "ui/icon.svg", "public/logo.png", "a.scss", "ui/author-card.css", "ui/tokens.css"] {
            assert_eq!(kind_of(p, false), Kind::Ui, "{p}");
        }
        for p in ["server/src/auth/session.ts", "ui/login.css", "crates/store/src/secret.rs", "x/oauth.js", "server/customToken.ts", "a/sign-in.css", "x/authorize.ts"] {
            assert_eq!(kind_of(p, false), Kind::Signin, "{p}");
        }
        for p in ["crates/store/src/schema11.sql", "crates/store/src/cards.rs", "server/migrations/0001.ts", "pipelines/transforms/a.py", "firestore.rules"] {
            assert_eq!(kind_of(p, false), Kind::Data, "{p}");
        }
        for p in ["Cargo.toml", "Cargo.lock", "package.json", "yarn.lock", "src-tauri/build.rs", "scripts/install.ps1", ".github/workflows/ci.yml", "Dockerfile", "src-tauri/tauri.test.conf.json", "tools/run.sh", "requirements.txt", "CMakeLists.txt"] {
            assert_eq!(kind_of(p, false), Kind::Build, "{p}");
        }
        // A binary file that isn't an image is code; an image is UI.
        assert_eq!(kind_of("ui/vendor/thing.wasm", true), Kind::Code);
        assert_eq!(kind_of("ui/font.woff2", true), Kind::Code);
        assert_eq!(kind_of("ui/shot.png", true), Kind::Ui);
    }

    #[test]
    fn numstat_is_summed_and_binaries_read() {
        let out = "\u{1}\n3\t1\tui/a.css\n\u{1}\n2\t0\tui/a.css\n-\t-\tui/x.bin\n\"odd name.css\"\nbad line\n1\t1\t\"q a.css\"\n";
        let f = parse_numstat(out);
        assert_eq!(f[0], fs("ui/a.css", 5, 1));
        assert!(f[1].binary && f[1].path == "ui/x.bin");
        assert_eq!(f[2].path, "q a.css");
        assert_eq!(f.len(), 3);
    }

    #[test]
    fn a_small_styles_and_docs_change_is_a_30_second_look() {
        let v = verdict(&reading(vec![fs("ui/board.css", 20, 4), fs("README.md", 10, 0), fs("crates/store/tests/a.rs", 300, 0)]), &cx());
        assert!(v.quick, "{v:?}");
        assert_eq!(v.why, "3 files, +330 \u{2212}4: styles or images and docs and tests only");
    }

    #[test]
    fn code_and_scripts_need_real_testing() {
        let v = verdict(&reading(vec![fs("ui/board.js", 3, 1)]), &cx());
        assert!(!v.quick);
        assert_eq!(v.why, "changes code you check by using it: ui/board.js +3 \u{2212}1");
        let v = verdict(&reading(vec![fs("prompts/in-progress.md", 2, 0)]), &cx());
        assert!(!v.quick && v.why.starts_with("changes how card sessions or agents behave"));
    }

    #[test]
    fn the_limits() {
        let v = verdict(&reading(vec![fs("ui/a.css", 50, 31)]), &cx());
        assert!(!v.quick && v.why.contains("81 lines of UI"));
        assert!(verdict(&reading(vec![fs("ui/a.css", 50, 30)]), &cx()).quick);
        let six: Vec<FileStat> = (0..6).map(|i| fs(&format!("ui/{i}.css"), 1, 0)).collect();
        assert!(verdict(&reading(six), &cx()).why.contains("6 UI files"));
        assert!(!verdict(&reading(vec![fs("README.md", 151, 0)]), &cx()).quick);
        assert!(verdict(&reading(vec![fs("README.md", 150, 0)]), &cx()).quick);
    }

    #[test]
    fn empty_unread_uncommitted_and_mismatched_need_real_testing() {
        assert_eq!(verdict(&reading(vec![]), &cx()).why, "nothing to check: the diff has no files");
        let none = Reading { files: None, from: "the card has no worktree".into(), ..Default::default() };
        assert_eq!(verdict(&none, &cx()).why, "the app couldn't read its changes: the card has no worktree");
        let mut r = reading(vec![fs("ui/a.css", 1, 0)]);
        r.uncommitted = true;
        assert!(!verdict(&r, &cx()).quick);
        let mut r = reading(vec![fs("ui/a.css", 1, 0)]);
        r.mismatch = Some(("dsh-1".into(), "dsh-2".into()));
        assert_eq!(verdict(&r, &cx()).why, "the card says branch dsh-1 but its checkout is on dsh-2");
    }

    fn card(fields: Value) -> Card {
        Card { id: "c".into(), number: 5, column: "Human test".into(), order: None, order_col: None, archived: false, fields: fields.as_object().cloned().unwrap(), version: 1 }
    }

    #[test]
    fn login_class() {
        assert_eq!(login_reasons(&card(json!({ "logins": [{}, {}] })), &[]), vec!["it has 2 logins to a dev stack"]);
        assert_eq!(login_reasons(&card(json!({ "devStack": "http://127.0.0.1:4732" })), &[]).len(), 1);
        assert_eq!(login_reasons(&card(json!({})), &["Handed over. Route: login (frontends)".into()]).len(), 1);
        assert!(login_reasons(&card(json!({})), &["Route: no-login".into(), "No logins and no dev stack".into()]).is_empty());
        let mut c = cx();
        c.login = vec!["it has a dev stack to sign in to".into()];
        let v = verdict(&reading(vec![fs("vault/a.md", 1, 0)]), &c);
        assert!(!v.quick && v.why.starts_with("needs a sign-in to check"));
        // A diff in a folder the repo's repos.json entry lists as one you sign in to.
        let c = Context { repo: "acme/web".into(), login_dirs: vec!["frontends".into(), "server".into()], ..Default::default() };
        assert!(!verdict(&reading(vec![fs("frontends/dashboard/a.css", 1, 0)]), &c).quick);
        assert!(verdict(&reading(vec![fs("vault/Features/a.md", 1, 0)]), &c).quick);
    }

    #[test]
    fn the_router_can_only_make_it_stricter() {
        let mut c = cx();
        c.seed_check = Some("30 min+".into());
        let v = verdict(&reading(vec![fs("ui/a.css", 1, 0)]), &c);
        assert!(!v.quick && v.why == "the router expected 30 min+ to check it");
        c.seed_check = Some("5 min".into());
        let v = verdict(&reading(vec![fs("ui/a.css", 1, 0)]), &c);
        assert!(v.quick && v.reasons.contains(&"the router guessed 5 min".to_string()));
        // A quick guess never makes an unread diff quick.
        let none = Reading { files: None, from: "x".into(), ..Default::default() };
        assert!(!verdict(&none, &c).quick);
        assert_eq!(guess_minutes("30 sec"), Some(0.5));
        assert_eq!(guess_minutes("1 h"), Some(60.0));
        assert_eq!(guess_minutes("soon"), None);
    }

    #[test]
    fn card_numbers_match_exactly() {
        let re = Regex::new(&card_grep(103)).unwrap();
        assert!(re.is_match("Show it (card #103)") && re.is_match("(card #103 as shipped)") && re.is_match("(card #103, #104)"));
        assert!(!re.is_match("(card #1030)"));
        let re10 = Regex::new(&card_grep(10)).unwrap();
        assert!(!re10.is_match("(card #103)") && re10.is_match("(card #10)"));
        assert!(Regex::new(&card_grep(79)).unwrap().is_match("Merge origin/main (card #79 as shipped) into main"));
    }

    #[test]
    fn secret_looking_text_is_hidden_everywhere() {
        let files = parse_numstat("4\t0\tserver/auth/password-reset.ts\n1\t0\tui/a.css\n");
        assert_eq!(files[0].path, HIDDEN);
        let r = reading(files);
        let v = verdict(&r, &cx());
        let rec = record(&r, &cx(), &v, "2026-11-06T00:00:00.000Z");
        let text = rec.to_string();
        assert!(!looks_secret(&text), "{text}");
        assert!(!looks_secret(&log_line(&rec)));
        // Text from elsewhere that looks secret is hidden in the record too.
        let r = Reading { files: None, from: "branch ct=abc against main".into(), ..Default::default() };
        let rec = record(&r, &cx(), &verdict(&r, &cx()), "t");
        assert!(!looks_secret(&rec.to_string()));
        assert_eq!(log_line(&json!({ "class": "real", "why": "x password y" })), "Time to check: Needs real testing: details hidden");
    }

    #[test]
    fn the_record_has_the_shape_card_104_reads() {
        let r = reading(vec![fs("ui/a.css", 2, 1), fs("README.md", 3, 0)]);
        let rec = record(&r, &cx(), &verdict(&r, &cx()), "t");
        assert_eq!(rec["class"], "quick");
        assert_eq!(rec["diff"]["files"], 2);
        assert_eq!(rec["diff"]["kinds"]["ui"], json!({ "files": 1, "lines": 3 }));
        assert_eq!(rec["diff"]["paths"][1], json!({ "path": "README.md", "added": 3, "removed": 0, "kind": "docs" }));
        assert_eq!(rec["diff"]["more"], 0);
        assert_eq!(minimal_record("t")["class"], "real");
    }

    #[test]
    fn writes_only_over_the_version_read_and_redoes_a_changed_stack() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("board.db")).unwrap();
        let mk = |s: &mut Store| s.create(None, json!({ "title": "T", "column": "Human test" }).as_object().cloned().unwrap(), "t").unwrap();
        let c = mk(&mut s);
        let rec = json!({ "class": "real", "why": "x", "cards": [c.number], "at": "t" });
        // The card changed while git ran: stale, nothing written.
        let mut p = Patch::new();
        p.insert("summary".into(), json!("new"));
        let c2 = s.update(&c.id, c.version, &p, "t").unwrap();
        assert_eq!(write(&mut s, &c.id, c.version, rec.clone()), Wrote::Stale);
        assert!(!s.get(&c.id).unwrap().fields.contains_key(CHECK_SIZE_KEY));
        assert_eq!(write(&mut s, &c.id, c2.version, rec.clone()), Wrote::Written);
        assert_eq!(s.log(&c.id).unwrap()[0].text, "Time to check: Needs real testing: x");
        // A record already there isn't written over.
        let v = s.get(&c.id).unwrap().version;
        assert_eq!(write(&mut s, &c.id, v, rec), Wrote::Stale);
        // A lead whose stack gained a card is worked out again; one that didn't change keeps it.
        let lead = mk(&mut s);
        let kid = mk(&mut s);
        let lead = s.stack_add(lead.number, kid.number, "t").unwrap();
        assert_eq!(write(&mut s, &lead.id, lead.version, json!({ "class": "quick", "why": "y", "cards": [lead.number, kid.number], "at": "t" })), Wrote::Written);
        clear_changed_stacks(&mut s);
        assert!(s.get(&lead.id).unwrap().fields.contains_key(CHECK_SIZE_KEY));
        let other = mk(&mut s);
        s.stack_add(lead.number, other.number, "t").unwrap();
        clear_changed_stacks(&mut s);
        assert!(!s.get(&lead.id).unwrap().fields.contains_key(CHECK_SIZE_KEY));
    }

    #[test]
    fn branch_names_are_checked() {
        assert!(branch_ok("dsh-103") && branch_ok("feat/login-v2") && branch_ok("issue_1.2"));
        assert!(!branch_ok("-x") && !branch_ok("a..b") && !branch_ok("a b") && !branch_ok("a;b") && !branch_ok(""));
    }

    #[test]
    fn a_slow_command_is_stopped() {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 10"]);
        let t = Instant::now();
        let r = run_limited(cmd, Duration::from_millis(800));
        assert!(r.unwrap_err().contains("longer than"));
        assert!(t.elapsed() < Duration::from_secs(8));
    }

    // Git on a temporary repo: branch mode with the nearer base, card-commit mode, uncommitted
    // work and the branch mismatch. Skipped where git isn't installed.
    fn sh(dir: &std::path::Path, args: &[&str]) {
        let ok = Command::new("git").current_dir(dir).args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"]).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success();
        assert!(ok, "git {args:?}");
    }
    fn put(dir: &std::path::Path, name: &str, text: &str) {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn reads_a_real_repo() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        let ds = d.to_str().unwrap();
        sh(d, &["init", "-q", "-b", "main"]);
        put(d, "ui/a.css", "a\n");
        sh(d, &["add", "."]);
        sh(d, &["commit", "-q", "-m", "base"]);
        // Card commits on main.
        put(d, "ui/a.css", "a\nb\n");
        sh(d, &["commit", "-q", "-am", "Style (card #103)"]);
        put(d, "README.md", "r\n");
        sh(d, &["add", "."]);
        sh(d, &["commit", "-q", "-m", "Docs (card #10)"]);
        let r = read(Some(ds), Some("main"), &[103]);
        assert_eq!(r.files.as_ref().unwrap(), &vec![fs("ui/a.css", 1, 0)], "{}", r.from);
        assert_eq!(r.from, "1 commit on main marked (card #103)");
        assert!(read(Some(ds), Some("main"), &[999]).files.is_none());
        // Uncommitted work on main isn't held against a card on main (a shared checkout).
        put(d, "ui/a.css", "changed\n");
        assert!(!read(Some(ds), Some("main"), &[103]).uncommitted);
        sh(d, &["checkout", "-q", "--", "ui/a.css"]);

        // A branch: diffed against main from where it left, not main's later commits.
        sh(d, &["checkout", "-q", "-b", "dsh-1"]);
        put(d, "ui/b.css", "x\ny\n");
        sh(d, &["add", "."]);
        sh(d, &["commit", "-q", "-m", "b"]);
        sh(d, &["checkout", "-q", "main"]);
        put(d, "src.rs", "later\n");
        sh(d, &["add", "."]);
        sh(d, &["commit", "-q", "-m", "later on main"]);
        sh(d, &["checkout", "-q", "dsh-1"]);
        let r = read(Some(ds), Some("dsh-1"), &[1]);
        assert_eq!(r.files.as_ref().unwrap(), &vec![fs("ui/b.css", 2, 0)], "{}", r.from);
        assert_eq!(r.from, "branch dsh-1 against main");
        assert!(!r.uncommitted && r.mismatch.is_none());
        assert!(verdict(&r, &cx()).quick);
        // Uncommitted work in its own checkout, and a card naming another branch.
        put(d, "ui/b.css", "x\n");
        let r = read(Some(ds), Some("dsh-9"), &[1]);
        assert!(r.uncommitted);
        assert_eq!(r.mismatch, Some(("dsh-9".into(), "dsh-1".into())));
        assert!(!verdict(&r, &cx()).quick);
        sh(d, &["checkout", "-q", "--", "ui/b.css"]);
        // A branch made from another unmerged branch: only the card's own marked commits count.
        sh(d, &["checkout", "-q", "-b", "dsh-2"]);
        put(d, "ui/c.css", "c
");
        sh(d, &["add", "."]);
        sh(d, &["commit", "-q", "-m", "Mine (card #2)"]);
        let r = read(Some(ds), Some("dsh-2"), &[2]);
        assert_eq!(r.files.as_ref().unwrap(), &vec![fs("ui/c.css", 1, 0)], "{}", r.from);
        assert_eq!(r.from, "1 commit on branch dsh-2 marked (card #2), since it left main");
        assert_eq!(r.numbers, vec![2]);
        // With no marked commit, the whole diff from main (the parent's work too).
        let r = read(Some(ds), Some("dsh-2"), &[77]);
        assert_eq!(r.files.as_ref().unwrap().len(), 2, "{}", r.from);
        // A branch named like a folder isn't mistaken for a path.
        sh(d, &["branch", "ui"]);
        sh(d, &["checkout", "-q", "ui"]);
        assert!(read(Some(ds), Some("ui"), &[2]).files.is_some());
        // No checkout.
        assert!(read(None, Some("x"), &[1]).files.is_none());
        assert!(read(Some(r"C:\no\such\folder\here"), None, &[1]).files.is_none());
        assert!(read(Some(r"\\wsl$\Ubuntu\home\x"), None, &[1]).from.contains("WSL"));
    }
}
