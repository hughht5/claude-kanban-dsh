//! What changed on each card (card #108): git figures for the card's checkout, the files its
//! session handed over to Hugh (`changesOutputs`), and the figures the app saved when the card
//! entered Human test (`changesSaved`), for the card's panel and the board.
//!
//! Git is only read, never written: no fetch, no optional locks (`GIT_OPTIONAL_LOCKS=0`), no
//! prompts, no console window, and a kill after 10 s. All git work runs on one worker thread,
//! one job at a time: a panel job goes to the front of the queue, the board's jobs to the back,
//! one per card. Nothing here holds the board's lock while git runs.
//!
//! Which figures: the card's branch against the newest commit it shares with main/master
//! ("branch"); else, when the branch is merged, is main/master or isn't known, the commits whose
//! message names the card ("commits"); else what was saved at Human test ("saved").

use crate::Board;
use kanban_store::{looks_secret, output_path_problem, Card, Error, Patch, Store, OUTPUTS_KEY, SAVED_KEY};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// Most files in a list (the rest are counted in `more`; totals count them all).
const MAX_FILES: usize = 300;
/// Most files kept in `changesSaved`.
const MAX_SAVED_FILES: usize = 100;
/// Most commits in a list (`commitCount` counts them all).
const MAX_COMMITS: usize = 20;
/// 10 s in the app. Tests allow longer: on a busy machine one git call can take seconds.
const GIT_TIMEOUT: Duration = Duration::from_secs(if cfg!(test) { 120 } else { 10 });
/// How long a command waits for the worker before giving up.
const WAIT: Duration = Duration::from_secs(120);
/// The refs that can be a repo's default, in the order they are tried.
const DEFAULT_REFS: [&str; 4] = ["refs/heads/main", "refs/heads/master", "refs/remotes/origin/main", "refs/remotes/origin/master"];
/// The only files the board opens with their default app (Hugh's decision).
const OPENABLE: [&str; 6] = ["md", "txt", "png", "jpg", "jpeg", "pdf"];
pub const ONLY_OPENABLE: &str = "Only md, txt, png, jpg and pdf files open from the board; use Show in folder";
/// Columns where a card's checkout may be gone, so no figures means none were kept.
const LATE: [&str; 4] = ["Human test", "Ship it", "Deploy complete", "Done"];

pub const NO_CHECKOUT: &str = "No checkout on the card (worktreePath)";
pub const IN_WSL: &str = "The checkout is in WSL; the app reads only local Windows folders";
pub const ON_NETWORK: &str = "The checkout is on a network path; the app reads only local Windows folders";
pub const NOT_FULL: &str = "The checkout path isn't a full local folder (like C:\\x\\repo)";
pub const GONE: &str = "The checkout folder isn't there any more";
pub const NOT_A_REPO: &str = "The checkout folder isn't a git repository";
pub const NO_GIT: &str = "Git isn't on PATH, so the app can't read the checkout";

// ---------------------------------------------------------------------------------------------
// Shapes

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Totals {
    pub files: usize,
    pub added: u64,
    pub removed: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct FileChange {
    pub path: String,
    pub old: Option<String>,
    /// A, M, D, R, C or T (git's letter).
    pub status: String,
    /// None for a binary file.
    pub added: Option<u64>,
    pub removed: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct CommitLine {
    pub sha: String,
    pub subject: String,
}

/// Committed figures: a branch against its base, or the card's commits. Inside the worker,
/// `files` holds every file; `capped` cuts it for a list.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Figures {
    /// "branch" or "commits".
    pub source: String,
    pub branch: Option<String>,
    pub base: Option<String>,
    /// The base's name for people: "main", or "origin/main" when that is the exact base.
    pub base_name: Option<String>,
    pub totals: Totals,
    pub files: Vec<FileChange>,
    pub more: usize,
    pub commits: Vec<CommitLine>,
    pub commit_count: usize,
}

impl Figures {
    fn capped(&self, n: usize) -> Figures {
        let mut f = self.clone();
        if f.files.len() > n {
            f.more += f.files.len() - n;
            f.files.truncate(n);
        }
        f.commits.truncate(MAX_COMMITS);
        f
    }
}

/// `changesSaved`: the figures when the card entered Human test, and when.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Saved {
    pub at: String,
    #[serde(flatten)]
    pub figures: Figures,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Uncommitted {
    pub totals: Totals,
    pub files: Vec<FileChange>,
    pub untracked: Vec<String>,
    /// Files and untracked names left out of the lists.
    pub more: usize,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OutputFile {
    pub label: Option<String>,
    pub path: String,
    pub name: String,
    pub exists: bool,
    pub size: Option<u64>,
    pub modified: Option<String>,
    pub openable: bool,
}

/// What `card_changes` sends the panel.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Changes {
    /// "branch", "commits", "saved" or "none".
    pub source: String,
    pub why: Option<String>,
    pub checkout: Option<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub base_name: Option<String>,
    pub totals: Totals,
    pub files: Vec<FileChange>,
    pub more: usize,
    pub commits: Vec<CommitLine>,
    pub commit_count: usize,
    pub uncommitted: Option<Uncommitted>,
    pub saved_at: Option<String>,
    pub outputs: Vec<OutputFile>,
}

/// What `card_changes_brief` sends the board for one card.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Brief {
    pub files: usize,
    pub added: u64,
    pub removed: u64,
    pub outputs: usize,
    pub saved: bool,
    /// Up to `BRIEF_PATHS` changed files (card #120): the most changed lines first, then by
    /// name; repo paths with forward slashes, a secret-looking one left out.
    pub paths: Vec<String>,
}

/// How many paths a card's brief names.
pub const BRIEF_PATHS: usize = 3;

/// The brief's paths from `files`: see `Brief::paths`. A binary file counts as no lines.
pub fn top_paths(files: &[FileChange]) -> Vec<String> {
    let mut v: Vec<(u64, String)> = files
        .iter()
        .map(|f| (f.added.unwrap_or(0) + f.removed.unwrap_or(0), f.path.replace('\\', "/")))
        .filter(|(_, p)| !p.is_empty() && p != "(path hidden)" && !looks_secret(p))
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    v.into_iter().take(BRIEF_PATHS).map(|(_, p)| p).collect()
}

/// Where a card's figures come from: its own checkout, or (a stack's child with none) its
/// lead's, where only the commits that name the child count.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Target {
    pub number: i64,
    pub checkout: Option<String>,
    pub branch: Option<String>,
    pub via_lead: bool,
}

/// What the worker found.
#[derive(Clone, Debug, Default)]
pub struct Live {
    pub checkout: Option<String>,
    /// Why there are no live figures: the checkout can't be read, or git failed.
    pub why: Option<String>,
    pub branch: Option<String>,
    pub figures: Option<Figures>,
    pub uncommitted: Option<Uncommitted>,
}

// ---------------------------------------------------------------------------------------------
// Pure checks

/// A path inside WSL's own file system (or a WSL path), which Windows tools must not open.
pub fn is_wsl(path: &str) -> bool {
    let p = path.replace('/', "\\").to_ascii_lowercase();
    // A `/` path is WSL's on Windows; on a Mac it is an ordinary local folder (card #117).
    p.starts_with("\\\\wsl$\\") || p.starts_with("\\\\wsl.localhost\\") || (crate::platform::DEV_STACKS && path.starts_with('/') && !path.starts_with("//"))
}

/// A UNC or network path (`\\host\share`, `//host/share`).
pub fn is_network(path: &str) -> bool {
    let p = path.replace('/', "\\");
    p.starts_with("\\\\")
}

/// A full local Windows path: a drive letter, a colon and a separator.
pub fn is_local_full(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

/// Why the app won't read `checkout` (without touching the disk), if it won't.
pub fn checkout_problem(checkout: Option<&str>) -> Option<&'static str> {
    let p = checkout.map(str::trim).filter(|p| !p.is_empty())?;
    if is_wsl(p) {
        Some(IN_WSL)
    } else if is_network(p) {
        Some(ON_NETWORK)
    } else if !is_local_full(p) {
        Some(NOT_FULL)
    } else {
        None
    }
}

fn checkout_dir(checkout: Option<&str>) -> Result<PathBuf, String> {
    let Some(p) = checkout.map(str::trim).filter(|p| !p.is_empty()) else { return Err(NO_CHECKOUT.into()) };
    if let Some(why) = checkout_problem(Some(p)) {
        return Err(why.into());
    }
    let dir = PathBuf::from(p);
    if !dir.is_dir() {
        return Err(GONE.into());
    }
    Ok(dir)
}

/// Whether `path` (one of a card's outputs) may be opened (`reveal` false) or shown in its
/// folder (`reveal` true) from the board.
/// The path rule is the store's, so a path the store took is one the board can open.
pub fn open_allowed(path: &str, reveal: bool) -> Result<(), String> {
    if let Some(why) = output_path_problem(path) {
        return Err(format!("The file path {why}; only a file on a local drive opens from the board"));
    }
    if !reveal && !openable_ext(path) {
        return Err(ONLY_OPENABLE.into());
    }
    Ok(())
}

fn openable_ext(path: &str) -> bool {
    Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .map_or(false, |e| OPENABLE.contains(&e.as_str()))
}

pub fn is_default(branch: &str) -> bool {
    branch == "main" || branch == "master"
}

/// A card's `branch` as a local branch name, or None when it can't be one.
fn clean_branch(b: Option<&str>) -> Option<String> {
    let b = b.map(str::trim)?;
    let b = b.strip_prefix("refs/heads/").unwrap_or(b);
    let bad = b.is_empty()
        || b.starts_with('-')
        || b.starts_with('/')
        || b.ends_with('/')
        || b.ends_with(".lock")
        || b.contains("..")
        || b.contains("@{")
        || b.chars().any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c));
    (!bad).then(|| b.to_string())
}

/// The number at `b[i..]` (ASCII digits) and where it ends; None when there is none.
fn num_at(b: &[u8], i: usize) -> Option<(i64, usize)> {
    let mut k = i;
    while k < b.len() && b[k].is_ascii_digit() {
        k += 1;
    }
    if k == i {
        return None;
    }
    std::str::from_utf8(&b[i..k]).ok()?.parse::<i64>().ok().map(|v| (v, k))
}

/// Does a commit message name card `n`: "card #N", a list such as "cards #89, #94" or
/// "cards #89 and #94", or a range such as "cards #96-#112", "cards #96–#112" or "cards #96 to
/// #112"? N must not run on into another digit ("#940" isn't #94), and "issue #N" or a bare "#N"
/// doesn't count. Case doesn't matter.
pub fn mentions_card(msg: &str, n: i64) -> bool {
    let s = msg.to_ascii_lowercase();
    let b = s.as_bytes();
    let ws = |mut i: usize| {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        i
    };
    let mut from = 0;
    while let Some(off) = s[from..].find("card") {
        let p = from + off;
        from = p + 4;
        if p > 0 && (b[p - 1].is_ascii_alphanumeric() || b[p - 1] == b'_') {
            continue;
        }
        let mut j = p + 4;
        if j < b.len() && b[j] == b's' {
            j += 1;
        }
        j = ws(j);
        while j < b.len() && b[j] == b'#' {
            let Some((lo, k)) = num_at(b, j + 1) else { break };
            if lo == n {
                return true;
            }
            let mut m = ws(k);
            // A range: "#96-#112", "#96–#112" (en dash) or "#96 to #112".
            let dash = if b[m..].starts_with(b"-") {
                Some(1)
            } else if b[m..].starts_with("\u{2013}".as_bytes()) {
                Some(3)
            } else if b[m..].starts_with(b"to") && b.get(m + 2).map_or(false, |c| c.is_ascii_whitespace() || *c == b'#') {
                Some(2)
            } else {
                None
            };
            if let Some(d) = dash {
                let mut r = ws(m + d);
                if r < b.len() && b[r] == b'#' {
                    r += 1;
                }
                if let Some((hi, e)) = num_at(b, r) {
                    if lo.min(hi) <= n && n <= lo.max(hi) {
                        return true;
                    }
                    m = ws(e);
                }
            }
            // The list goes on: ", #94", " and #94", ", and #94", " & #94".
            let mut sep = false;
            if m < b.len() && (b[m] == b',' || b[m] == b'&') {
                m = ws(m + 1);
                sep = true;
            }
            if s[m..].starts_with("and") && b.get(m + 3).map_or(false, |c| c.is_ascii_whitespace() || *c == b'#') {
                m = ws(m + 3);
                sep = true;
            }
            if !sep {
                break;
            }
            j = m;
        }
    }
    false
}

fn mask_path(p: String) -> String {
    if looks_secret(&p) { "(path hidden)".into() } else { p }
}

fn mask_subject(s: String) -> String {
    if looks_secret(&s) { "(subject hidden)".into() } else { s }
}

fn mask_branch(b: String) -> String {
    if looks_secret(&b) { "(branch hidden)".into() } else { b }
}

fn mask_file(mut f: FileChange) -> FileChange {
    f.path = mask_path(f.path);
    f.old = f.old.map(mask_path);
    f
}

fn totals(files: &[FileChange]) -> Totals {
    Totals {
        files: files.len(),
        added: files.iter().filter_map(|f| f.added).sum(),
        removed: files.iter().filter_map(|f| f.removed).sum(),
    }
}

// ---------------------------------------------------------------------------------------------
// Parsers for git's -z output

/// One commit of `git log --format=%x01%H%x02%B%x03 --raw --numstat -z`, or the whole of a
/// `git diff --raw --numstat -z` (no sha).
#[derive(Debug, Default, PartialEq)]
pub struct Section {
    pub sha: Option<String>,
    pub message: String,
    pub files: Vec<FileChange>,
}

struct RawEntry {
    letter: String,
    old: Option<String>,
    path: String,
}

struct NumEntry {
    added: Option<u64>,
    removed: Option<u64>,
    path: String,
}

fn text(t: &[u8]) -> String {
    String::from_utf8_lossy(t).into_owned()
}

fn trim_nl(mut t: &[u8]) -> &[u8] {
    while let Some((b'\n', rest)) = t.split_first() {
        t = rest;
    }
    t
}

/// "12\t3\tpath" (or "-\t-\t" for a binary file; an empty path means a rename's two paths follow).
fn numstat(t: &str) -> Option<(Option<u64>, Option<u64>, String)> {
    let mut it = t.splitn(3, '\t');
    let (a, r, rest) = (it.next()?, it.next()?, it.next()?);
    let num = |x: &str| if x == "-" { Some(None) } else { x.parse::<u64>().ok().map(Some) };
    Some((num(a)?, num(r)?, rest.to_string()))
}

fn merge(raw: Vec<RawEntry>, num: Vec<NumEntry>) -> Vec<FileChange> {
    if raw.is_empty() {
        return num
            .into_iter()
            .map(|n| FileChange { path: n.path, old: None, status: "M".into(), added: n.added, removed: n.removed })
            .collect();
    }
    raw.into_iter()
        .enumerate()
        .map(|(i, r)| {
            let n = num.get(i).filter(|n| n.path == r.path).or_else(|| num.iter().find(|n| n.path == r.path));
            FileChange { path: r.path, old: r.old, status: r.letter, added: n.and_then(|n| n.added), removed: n.and_then(|n| n.removed) }
        })
        .collect()
}

/// Parses `--raw --numstat -z` output, with or without log headers (`%x01%H%x02<msg>%x03`).
/// Names with spaces, tabs or non-ASCII come through as they are (`-z`, `core.quotepath=off`).
pub fn parse_stream(out: &[u8]) -> Vec<Section> {
    let toks: Vec<&[u8]> = out.split(|b| *b == 0).collect();
    let tok = |i: usize| toks.get(i).map(|t| text(t)).unwrap_or_default();
    let mut sections = Vec::new();
    let mut cur = Section::default();
    let (mut raw, mut num) = (Vec::new(), Vec::new());
    let mut started = false;
    let mut i = 0;
    while i < toks.len() {
        let t = trim_nl(toks[i]);
        match t.first() {
            Some(1) => {
                if started || !raw.is_empty() || !num.is_empty() {
                    cur.files = merge(std::mem::take(&mut raw), std::mem::take(&mut num));
                    sections.push(std::mem::take(&mut cur));
                }
                let h = text(&t[1..]);
                let (sha, rest) = h.split_once('\u{2}').unwrap_or((h.as_str(), ""));
                let msg = rest.split('\u{3}').next().unwrap_or("");
                cur = Section { sha: Some(sha.trim().to_string()), message: msg.to_string(), files: vec![] };
                started = true;
                i += 1;
            }
            Some(b':') => {
                let meta = text(t);
                let letter: String = meta.split_whitespace().last().unwrap_or("M").chars().take(1).collect();
                if letter == "R" || letter == "C" {
                    raw.push(RawEntry { letter, old: Some(tok(i + 1)), path: tok(i + 2) });
                    i += 3;
                } else {
                    raw.push(RawEntry { letter, old: None, path: tok(i + 1) });
                    i += 2;
                }
            }
            Some(_) => match numstat(&text(t)) {
                Some((added, removed, rest)) if rest.is_empty() => {
                    num.push(NumEntry { added, removed, path: tok(i + 2) });
                    i += 3;
                }
                Some((added, removed, path)) => {
                    num.push(NumEntry { added, removed, path });
                    i += 1;
                }
                None => i += 1,
            },
            None => i += 1,
        }
    }
    if started || !raw.is_empty() || !num.is_empty() {
        cur.files = merge(raw, num);
        sections.push(cur);
    }
    sections
}

/// The untracked names in `git status --porcelain=v1 -z` output.
pub fn parse_untracked(out: &[u8]) -> Vec<String> {
    let toks: Vec<&[u8]> = out.split(|b| *b == 0).collect();
    let mut names = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i];
        i += 1;
        if t.len() < 4 {
            continue;
        }
        if &t[..3] == b"?? " {
            names.push(text(&t[3..]));
        } else if matches!(t[0], b'R' | b'C') || matches!(t[1], b'R' | b'C') {
            // A rename or copy: its old name is the next entry.
            i += 1;
        }
    }
    names
}

/// The figures of several commits, file by file, oldest first in `commits`. A rename carries
/// the file's figures to its new name (keeping the first old name); a file added and then
/// deleted within the commits is left out.
fn sum_commits<'a>(commits: impl Iterator<Item = &'a Section>) -> Vec<FileChange> {
    let mut map: BTreeMap<String, FileChange> = BTreeMap::new();
    let add = |a: Option<u64>, b: Option<u64>| match (a, b) {
        (Some(a), Some(b)) => Some(a + b),
        _ => None,
    };
    for c in commits {
        for f in &c.files {
            let renamed_from = f.old.as_ref().filter(|_| f.status == "R");
            let prior = renamed_from.and_then(|o| map.remove(o)).or_else(|| map.remove(&f.path));
            let Some(mut e) = prior else {
                map.insert(f.path.clone(), f.clone());
                continue;
            };
            let status = match (e.status.as_str(), f.status.as_str()) {
                ("A", "D") => None,
                (_, "D") => Some("D"),
                ("D", "A") => Some("M"),
                ("A", _) => Some("A"),
                (prev, "M") => Some(prev),
                (_, new) => Some(new),
            };
            let Some(status) = status.map(str::to_string) else { continue };
            e.added = add(e.added, f.added);
            e.removed = add(e.removed, f.removed);
            if status == "A" {
                e.old = None;
            } else if e.old.is_none() {
                e.old = f.old.clone();
            }
            e.status = status;
            e.path = f.path.clone();
            map.insert(f.path.clone(), e);
        }
    }
    map.into_values().collect()
}

// ---------------------------------------------------------------------------------------------
// Git

#[derive(Debug)]
enum GitErr {
    Missing,
    Timeout,
    Failed,
    Io(String),
}

fn why(e: GitErr) -> String {
    match e {
        GitErr::Missing => NO_GIT.into(),
        GitErr::Timeout => "Git didn't answer within 10 s".into(),
        GitErr::Failed => "Git couldn't read the checkout".into(),
        GitErr::Io(m) => format!("Git couldn't run: {m}"),
    }
}

/// Runs a read-only git command in `dir` and returns its output.
fn git(dir: &Path, args: &[&str]) -> Result<Vec<u8>, GitErr> {
    let mut cmd = Command::new("git");
    cmd.args(["-c", "core.fsmonitor=false", "-c", "core.quotepath=off", "-C"])
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    {
        use crate::platform::NoWindow;
        cmd.no_window();
    }
    let mut child = cmd.spawn().map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { GitErr::Missing } else { GitErr::Io(e.to_string()) })?;
    let mut out = child.stdout.take().ok_or_else(|| GitErr::Io("no output".into()))?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + GIT_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(GitErr::Timeout);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(GitErr::Io(e.to_string()));
            }
        }
    };
    let buf = reader.join().unwrap_or_default();
    if status.success() { Ok(buf) } else { Err(GitErr::Failed) }
}

const DIFF_OPTS: [&str; 7] = ["--no-ext-diff", "--no-textconv", "--no-color", "-M", "--raw", "--numstat", "-z"];

/// What the checkout is: a linked worktree or a main checkout, its HEAD and current branch,
/// and the shas of the default refs and the card's branch.
#[derive(Debug, Clone, PartialEq)]
struct Probe {
    linked: bool,
    head: Option<String>,
    current: Option<String>,
    defaults: Vec<(String, String)>,
    branch_sha: Option<String>,
}

fn same_dir(dir: &Path, a: &str, b: &str) -> bool {
    let full = |p: &str| {
        let p = Path::new(p);
        let p = if p.is_absolute() { p.to_path_buf() } else { dir.join(p) };
        p.canonicalize().unwrap_or(p).to_string_lossy().replace('/', "\\").to_ascii_lowercase()
    };
    full(a) == full(b)
}

fn probe(dir: &Path, card_branch: Option<&str>) -> Result<Probe, String> {
    let lines = |b: Vec<u8>| text(&b).lines().map(|l| l.trim().to_string()).collect::<Vec<_>>();
    let (git_dir, common, head, current) = match git(dir, &["rev-parse", "--git-dir", "--git-common-dir", "HEAD", "--symbolic-full-name", "HEAD"]) {
        Ok(out) => {
            let l = lines(out);
            let current = l.get(3).and_then(|r| r.strip_prefix("refs/heads/")).map(str::to_string);
            (l.first().cloned().unwrap_or_default(), l.get(1).cloned().unwrap_or_default(), l.get(2).cloned(), current)
        }
        // No commit yet, or not a repo at all.
        Err(GitErr::Failed) => match git(dir, &["rev-parse", "--git-dir", "--git-common-dir"]) {
            Ok(out) => {
                let l = lines(out);
                (l.first().cloned().unwrap_or_default(), l.get(1).cloned().unwrap_or_default(), None, None)
            }
            Err(GitErr::Failed) => return Err(NOT_A_REPO.into()),
            Err(e) => return Err(why(e)),
        },
        Err(e) => return Err(why(e)),
    };
    let linked = !same_dir(dir, &git_dir, &common);
    let branch_ref = clean_branch(card_branch).map(|b| format!("refs/heads/{b}"));
    let mut args = vec!["for-each-ref", "--format=%(refname)%00%(objectname)"];
    args.extend(DEFAULT_REFS);
    if let Some(r) = &branch_ref {
        args.push(r);
    }
    let out = git(dir, &args).map_err(why)?;
    let mut shas: HashMap<String, String> = HashMap::new();
    for line in text(&out).lines() {
        if let Some((name, sha)) = line.split_once('\0') {
            shas.insert(name.to_string(), sha.trim().to_string());
        }
    }
    let defaults = DEFAULT_REFS.iter().filter_map(|r| shas.get(*r).map(|s| (r.to_string(), s.clone()))).collect();
    let branch_sha = branch_ref.and_then(|r| shas.get(&r).cloned());
    Ok(Probe { linked, head, current, defaults, branch_sha })
}

/// The branch the figures are for: the card's, if it exists; else, only in a linked worktree,
/// the one checked out there.
fn pick_branch(t: &Target, p: &Probe) -> Option<String> {
    if p.branch_sha.is_some() {
        clean_branch(t.branch.as_deref())
    } else if p.linked {
        p.current.clone()
    } else {
        None
    }
}

fn log_commits(dir: &Path, range: &str) -> Result<(Vec<CommitLine>, usize), String> {
    let out = git(dir, &["log", "--no-merges", "-z", "--format=%x01%H%x02%s%x03", range, "--"]).map_err(why)?;
    let all = parse_stream(&out);
    let count = all.len();
    let commits = all.into_iter().take(MAX_COMMITS).map(commit_line).collect();
    Ok((commits, count))
}

fn commit_line(s: Section) -> CommitLine {
    let sha: String = s.sha.unwrap_or_default().chars().take(7).collect();
    let subject = s.message.lines().next().unwrap_or("").trim().to_string();
    CommitLine { sha, subject: mask_subject(subject) }
}

/// A name for a branch's base: the default ref whose tip is exactly `base` ("main",
/// "origin/main"), else `fallback` ("main" or "master"), since the base is where the branch left it.
fn base_name(defaults: &[(String, String)], base: &str, fallback: Option<&str>) -> String {
    defaults
        .iter()
        .find(|(_, sha)| sha == base)
        .map(|(r, _)| r.trim_start_matches("refs/heads/").trim_start_matches("refs/remotes/").to_string())
        .unwrap_or_else(|| fallback.unwrap_or("main").to_string())
}

/// The committed figures for the card in `dir`, or None when there are none.
fn committed(dir: &Path, p: &Probe, t: &Target, branch: Option<&str>) -> Result<Option<Figures>, String> {
    let defaults: Vec<&str> = p.defaults.iter().map(|(r, _)| r.as_str()).collect();
    let default_name = defaults.first().map(|r| if r.ends_with("master") { "master" } else { "main" });
    if let (Some(b), false) = (branch, defaults.is_empty()) {
        if !is_default(b) && !t.via_lead {
            let bref = format!("refs/heads/{b}");
            let mut args = vec!["merge-base", bref.as_str()];
            args.extend(defaults.iter().copied());
            match git(dir, &args) {
                Ok(out) => {
                    let base = text(&out).lines().next().unwrap_or("").trim().to_string();
                    if !base.is_empty() {
                        let mut args = vec!["diff"];
                        args.extend(DIFF_OPTS);
                        args.extend([base.as_str(), bref.as_str(), "--"]);
                        let out = git(dir, &args).map_err(why)?;
                        let files: Vec<FileChange> = parse_stream(&out).into_iter().flat_map(|s| s.files).map(mask_file).collect();
                        if !files.is_empty() {
                            let (commits, commit_count) = log_commits(dir, &format!("{base}..{bref}"))?;
                            return Ok(Some(Figures {
                                source: "branch".into(),
                                branch: Some(mask_branch(b.to_string())),
                                base_name: Some(base_name(&p.defaults, &base, default_name)),
                                base: Some(base),
                                totals: totals(&files),
                                files,
                                more: 0,
                                commits,
                                commit_count,
                            }));
                        }
                    }
                }
                // No history shared with main/master: the card's commits instead.
                Err(GitErr::Failed) => {}
                Err(e) => return Err(why(e)),
            }
        }
    }
    let r = match (branch, defaults.first()) {
        (Some(b), _) => format!("refs/heads/{b}"),
        (None, Some(d)) => d.to_string(),
        (None, None) => return Ok(None),
    };
    // git ORs the two: "#N" finds most, "cards #" finds a range ("cards #96-#112") that holds N.
    // mentions_card then keeps only the commits that name the card.
    let grep = format!("--grep=#{}", t.number);
    let mut args = vec!["log", r.as_str(), "--no-merges", "-F", "-i", grep.as_str(), "--grep=cards #", "--max-count=1000"];
    args.extend(DIFF_OPTS);
    args.extend(["--format=%x01%H%x02%B%x03", "--"]);
    let out = git(dir, &args).map_err(why)?;
    let kept: Vec<Section> = parse_stream(&out).into_iter().filter(|s| s.sha.is_some() && mentions_card(&s.message, t.number)).collect();
    if kept.is_empty() {
        return Ok(None);
    }
    let files: Vec<FileChange> = sum_commits(kept.iter().rev()).into_iter().map(mask_file).collect();
    let commit_count = kept.len();
    let commits = kept.into_iter().take(MAX_COMMITS).map(commit_line).collect();
    Ok(Some(Figures {
        source: "commits".into(),
        branch: branch.map(str::to_string).or(default_name.map(str::to_string)).map(mask_branch),
        base: None,
        base_name: None,
        totals: totals(&files),
        files,
        more: 0,
        commits,
        commit_count,
    }))
}

/// Work not committed yet in the checkout: tracked changes against HEAD (staged and not) and
/// the names of untracked files.
fn uncommitted(dir: &Path, p: &Probe) -> Result<Uncommitted, String> {
    let mut files: Vec<FileChange> = Vec::new();
    if p.head.is_some() {
        let mut args = vec!["diff"];
        args.extend(DIFF_OPTS);
        args.extend(["HEAD", "--"]);
        let out = git(dir, &args).map_err(why)?;
        files = parse_stream(&out).into_iter().flat_map(|s| s.files).map(mask_file).collect();
    }
    let out = git(dir, &["status", "--porcelain=v1", "-z", "--untracked-files=normal"]).map_err(why)?;
    let mut untracked: Vec<String> = parse_untracked(&out).into_iter().map(mask_path).collect();
    let totals = totals(&files);
    let mut more = 0;
    if files.len() > MAX_FILES {
        more += files.len() - MAX_FILES;
        files.truncate(MAX_FILES);
    }
    if untracked.len() > MAX_FILES {
        more += untracked.len() - MAX_FILES;
        untracked.truncate(MAX_FILES);
    }
    Ok(Uncommitted { totals, files, untracked, more })
}

// ---------------------------------------------------------------------------------------------
// The worker

/// The cache key: the card's target and every sha the figures depend on.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    target: Target,
    branch: Option<String>,
    probe: Probe,
}

type Cache = HashMap<String, (Key, Option<Figures>)>;

struct Job {
    card_id: String,
    target: Target,
    /// The panel's job: first in the queue, and with uncommitted work.
    panel: bool,
    replies: Vec<Sender<Live>>,
}

struct Queue {
    jobs: Mutex<VecDeque<Job>>,
    ready: Condvar,
}

fn queue() -> &'static Queue {
    static Q: OnceLock<Queue> = OnceLock::new();
    Q.get_or_init(|| {
        std::thread::Builder::new()
            .name("changes-git".into())
            .spawn(worker)
            .expect("start the changes worker");
        Queue { jobs: Mutex::new(VecDeque::new()), ready: Condvar::new() }
    })
}

fn worker() {
    let mut cache: Cache = HashMap::new();
    loop {
        let job = {
            let q = queue();
            let mut jobs = q.jobs.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if let Some(j) = jobs.pop_front() {
                    break j;
                }
                jobs = q.ready.wait(jobs).unwrap_or_else(|e| e.into_inner());
            }
        };
        let live = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_job(&mut cache, &job.card_id, &job.target, job.panel)))
            .unwrap_or_else(|_| Live { checkout: job.target.checkout.clone(), why: Some("The app couldn't read the checkout".into()), ..Live::default() });
        for r in job.replies {
            let _ = r.send(live.clone());
        }
    }
}

/// Queues a job for card `card_id`: a panel job at the front, a board job at the back. A job
/// for the same card already waiting takes this reply too.
fn submit(card_id: &str, target: Target, panel: bool) -> Receiver<Live> {
    let (tx, rx) = channel();
    let q = queue();
    let mut jobs = q.jobs.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(j) = jobs.iter_mut().find(|j| j.card_id == card_id && j.panel == panel && j.target == target) {
        j.replies.push(tx);
    } else {
        let job = Job { card_id: card_id.to_string(), target, panel, replies: vec![tx] };
        if panel {
            jobs.push_front(job);
        } else {
            jobs.push_back(job);
        }
    }
    q.ready.notify_one();
    rx
}

fn wait(rx: Receiver<Live>, target: &Target) -> Live {
    rx.recv_timeout(WAIT)
        .unwrap_or_else(|_| Live { checkout: target.checkout.clone(), why: Some("The app's git reader didn't answer in time".into()), ..Live::default() })
}

/// The figures for one card: the committed ones from the cache when nothing they depend on
/// moved, and (for the panel) the uncommitted work, which is never cached.
fn run_job(cache: &mut Cache, card_id: &str, t: &Target, panel: bool) -> Live {
    let mut live = Live { checkout: t.checkout.clone(), ..Live::default() };
    let dir = match checkout_dir(t.checkout.as_deref()) {
        Ok(d) => d,
        Err(w) => {
            live.why = Some(w);
            return live;
        }
    };
    let p = match probe(&dir, t.branch.as_deref()) {
        Ok(p) => p,
        Err(w) => {
            live.why = Some(w);
            return live;
        }
    };
    let branch = pick_branch(t, &p);
    let key = Key { target: t.clone(), branch: branch.clone(), probe: p.clone() };
    let figures = match cache.get(card_id) {
        Some((k, f)) if *k == key => f.clone(),
        _ => match committed(&dir, &p, t, branch.as_deref()) {
            Ok(f) => {
                cache.insert(card_id.to_string(), (key, f.clone()));
                f
            }
            Err(w) => {
                live.why = Some(w);
                None
            }
        },
    };
    let own_branch = branch.as_deref().filter(|b| !is_default(b) && p.current.as_deref() == Some(*b));
    if panel && p.linked && !t.via_lead && own_branch.is_some() {
        match uncommitted(&dir, &p) {
            Ok(u) => live.uncommitted = Some(u),
            Err(w) => live.why = live.why.or(Some(w)),
        }
    }
    live.branch = branch.map(mask_branch);
    live.figures = figures;
    live
}

// ---------------------------------------------------------------------------------------------
// Cards

fn field(c: &Card, k: &str) -> Option<String> {
    c.fields.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// Where card `c`'s figures come from. A stack's child with no checkout of its own uses its
/// lead's checkout and branch.
pub fn target_of(store: &Store, c: &Card) -> Target {
    let own = field(c, "worktreePath");
    if own.is_none() {
        if let Some(lead) = c.stack_of().and_then(|n| store.get_by_number(n).ok()) {
            return Target { number: c.number, checkout: field(&lead, "worktreePath"), branch: field(&lead, "branch"), via_lead: true };
        }
    }
    Target { number: c.number, checkout: own, branch: field(c, "branch"), via_lead: false }
}

fn saved_of(c: &Card) -> Option<Saved> {
    c.fields.get(SAVED_KEY).and_then(|v| serde_json::from_value::<Saved>(v.clone()).ok())
}

fn outputs_count(c: &Card) -> usize {
    c.fields.get(OUTPUTS_KEY).and_then(Value::as_array).map_or(0, Vec::len)
}

fn modified(m: &std::fs::Metadata) -> Option<String> {
    let t = m.modified().ok()?;
    Some(chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// The card's outputs, each with whether it is there, its size and when it last changed.
pub fn outputs_of(c: &Card) -> Vec<OutputFile> {
    let Some(items) = c.fields.get(OUTPUTS_KEY).and_then(Value::as_array) else { return vec![] };
    items
        .iter()
        .filter_map(|e| {
            let path = e.get("path").and_then(Value::as_str)?.to_string();
            let label = e.get("label").and_then(Value::as_str).map(str::to_string);
            let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone());
            let meta = open_allowed(&path, true).ok().and_then(|_| std::fs::metadata(&path).ok()).filter(|m| m.is_file());
            let exists = meta.is_some();
            Some(OutputFile {
                label,
                name,
                exists,
                size: meta.as_ref().map(|m| m.len()),
                modified: meta.as_ref().and_then(modified),
                openable: exists && open_allowed(&path, false).is_ok(),
                path,
            })
        })
        .collect()
}

/// The panel's answer for `card` from what the worker found.
pub fn assemble(card: &Card, live: Live) -> Changes {
    let outputs = outputs_of(card);
    let uncommitted = live.uncommitted.clone();
    let mut out = Changes {
        source: "none".into(),
        why: None,
        checkout: live.checkout.clone(),
        branch: live.branch.clone(),
        base: None,
        base_name: None,
        totals: Totals::default(),
        files: vec![],
        more: 0,
        commits: vec![],
        commit_count: 0,
        uncommitted,
        saved_at: None,
        outputs,
    };
    let fill = |out: &mut Changes, f: Figures| {
        let f = f.capped(MAX_FILES);
        out.source = f.source;
        out.branch = f.branch;
        out.base = f.base;
        out.base_name = f.base_name;
        out.totals = f.totals;
        out.files = f.files;
        out.more = f.more;
        out.commits = f.commits;
        out.commit_count = f.commit_count;
    };
    if let Some(f) = live.figures {
        fill(&mut out, f);
        return out;
    }
    if let Some(s) = saved_of(card) {
        fill(&mut out, s.figures);
        out.source = "saved".into();
        out.saved_at = Some(s.at);
        out.why = live.why;
        return out;
    }
    let late = card.archived || LATE.contains(&card.column.as_str());
    let pending = out.uncommitted.as_ref().map_or(false, |u| !u.files.is_empty() || !u.untracked.is_empty());
    out.why = Some(if late {
        "No figures kept".into()
    } else if let Some(w) = live.why {
        w
    } else if pending {
        "Nothing committed yet".into()
    } else {
        "No changes yet".into()
    });
    out
}

/// Whether the board's brief runs git for `card`. A Done card goes by its saved figures only, so
/// a board full of Done cards runs no git when `main` moves.
pub fn asks_git(card: &Card) -> bool {
    card.column != "Done"
}

/// The board's figures for `card`: the live committed ones, else the saved ones. None when it
/// has neither and no outputs.
pub fn brief_of(card: &Card, live: &Live) -> Option<Brief> {
    let outputs = outputs_count(card);
    let (t, paths, saved) = match (&live.figures, saved_of(card)) {
        (Some(f), _) => (Some(f.totals.clone()), top_paths(&f.files), false),
        (None, Some(s)) => (Some(s.figures.totals.clone()), top_paths(&s.figures.files), true),
        (None, None) => (None, vec![], false),
    };
    if t.is_none() && outputs == 0 {
        return None;
    }
    let t = t.unwrap_or_default();
    Some(Brief { files: t.files, added: t.added, removed: t.removed, outputs, saved, paths })
}

/// The `changesSaved` value for `f`, saved now.
pub fn saved_value(f: &Figures) -> Value {
    let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    serde_json::to_value(Saved { at, figures: f.capped(MAX_SAVED_FILES) }).unwrap_or(Value::Null)
}

fn read_card(app: &AppHandle, id: &str) -> Result<(Card, Target), String> {
    let board = app.state::<Board>();
    let store = board.0.lock().map_err(|e| e.to_string())?;
    let card = store.get(id).map_err(|e| e.to_string())?;
    let target = target_of(&store, &card);
    Ok((card, target))
}

/// Whether the worker has a checkout to read for `t`: why not when it hasn't (none on the card,
/// in WSL, on a network, not a full path). The panel and the board both go by this.
pub fn usable(t: &Target) -> Result<(), &'static str> {
    if t.checkout.as_deref().map(str::trim).map_or(true, str::is_empty) {
        return Err(NO_CHECKOUT);
    }
    checkout_problem(t.checkout.as_deref()).map_or(Ok(()), Err)
}

fn live_for(card_id: &str, target: &Target, panel: bool) -> Live {
    // Nothing to ask git about: answer at once, without waiting behind other cards.
    if let Err(w) = usable(target) {
        return Live { checkout: target.checkout.clone(), why: Some(w.into()), ..Live::default() };
    }
    wait(submit(card_id, target.clone(), panel), target)
}

/// When a card enters Human test, its committed figures are saved on it (`changesSaved`), since
/// a ship can delete its branch and worktree later. Git runs on the worker; the board is locked
/// only to read the card and to write. Nothing found, nothing written.
pub fn on_entered_human_test(app: &AppHandle, card_id: &str) {
    static SAVING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let saving = SAVING.get_or_init(|| Mutex::new(HashSet::new()));
    // A move can reach here twice (the stack rules and the move itself): one save is enough.
    if !saving.lock().unwrap_or_else(|e| e.into_inner()).insert(card_id.to_string()) {
        return;
    }
    let (app, id) = (app.clone(), card_id.to_string());
    std::thread::spawn(move || {
        save_figures(&app, &id);
        saving.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    });
}

fn save_figures(app: &AppHandle, id: &str) {
    let Ok((card, target)) = read_card(app, id) else { return };
    if card.archived {
        return;
    }
    let live = live_for(id, &target, false);
    let Some(f) = live.figures else { return };
    let mut patch = Patch::new();
    patch.insert(SAVED_KEY.into(), saved_value(&f));
    let mut version = card.version;
    for attempt in 0..2 {
        let board = app.state::<Board>();
        let Ok(mut store) = board.0.lock() else { return };
        match store.update(id, version, &patch, "the app") {
            Ok(_) => break,
            Err(Error::Conflict { current, .. }) if attempt == 0 => version = current,
            Err(e) => {
                eprintln!("saving card {id}'s figures: {e}");
                return;
            }
        }
    }
    let _ = app.emit("cards-changed", ());
}

// ---------------------------------------------------------------------------------------------
// Commands

/// The card's panel: its committed figures, its uncommitted work, and its outputs.
#[tauri::command]
pub async fn card_changes(app: AppHandle, id: String) -> Result<Changes, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (card, target) = read_card(&app, &id)?;
        let live = live_for(&id, &target, true);
        Ok(assemble(&card, live))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The board's figures, by card id: committed only, live else saved (Done cards: saved only, no
/// git). Archived cards and cards in Backlog or Planning are left out, and so is a card with no
/// figures and no outputs.
#[tauri::command]
pub async fn card_changes_brief(app: AppHandle, ids: Vec<String>) -> Result<BTreeMap<String, Brief>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cards: Vec<(Card, Target)> = {
            let board = app.state::<Board>();
            let store = board.0.lock().map_err(|e| e.to_string())?;
            ids.iter()
                .filter_map(|id| store.get(id).ok())
                .filter(|c| !c.archived && c.column != "Backlog" && c.column != "Planning")
                .map(|c| {
                    let t = target_of(&store, &c);
                    (c, t)
                })
                .collect()
        };
        // Every card's job is queued before any is waited on, so a panel job can go between them.
        let pending: Vec<(Card, Target, Option<Receiver<Live>>)> = cards
            .into_iter()
            .map(|(c, t)| {
                let rx = (asks_git(&c) && usable(&t).is_ok()).then(|| submit(&c.id, t.clone(), false));
                (c, t, rx)
            })
            .collect();
        let mut out = BTreeMap::new();
        for (c, t, rx) in pending {
            let live = match rx {
                Some(rx) => wait(rx, &t),
                None => Live::default(),
            };
            if let Some(b) = brief_of(&c, &live) {
                out.insert(c.id.clone(), b);
            }
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub const OUTPUTS_CHANGED: &str = "The card's handed-over files changed; reopen the card and try again";

/// The path of `changesOutputs[index]` on `card`, only when it is still `path`: the panel's list
/// can be older than the card.
pub fn stored_output(card: &Card, index: usize, path: &str) -> Result<String, String> {
    card.fields
        .get(OUTPUTS_KEY)
        .and_then(|v| v.get(index))
        .and_then(|e| e.get("path"))
        .and_then(Value::as_str)
        .filter(|p| *p == path)
        .map(str::to_string)
        .ok_or_else(|| OUTPUTS_CHANGED.to_string())
}

/// Opens one of the card's outputs (`changesOutputs[index]`) with its default app, or shows it
/// in Explorer (`reveal`), refusing it when the card no longer has `path` there. Only md, txt,
/// png, jpg and pdf files open.
#[tauri::command]
pub async fn changes_open(app: AppHandle, id: String, index: usize, path: String, reveal: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = {
            let board = app.state::<Board>();
            let store = board.0.lock().map_err(|e| e.to_string())?;
            let card = store.get(&id).map_err(|e| e.to_string())?;
            stored_output(&card, index, &path)?
        };
        open_allowed(&path, reveal)?;
        if !Path::new(&path).is_file() {
            return Err(format!("{path} isn't there any more"));
        }
        if reveal {
            tauri_plugin_opener::reveal_item_in_dir(&path).map_err(|e| e.to_string())
        } else {
            tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| e.to_string())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_base_is_named_by_its_branch() {
        let d = vec![("refs/heads/main".to_string(), "aaa".to_string()), ("refs/remotes/origin/main".to_string(), "bbb".to_string())];
        assert_eq!(base_name(&d, "aaa", Some("main")), "main");
        assert_eq!(base_name(&d, "bbb", Some("main")), "origin/main");
        assert_eq!(base_name(&d, "ccc", Some("main")), "main");
        assert_eq!(base_name(&[], "ccc", None), "main");
    }

    #[test]
    fn mentions_card_needs_the_word_card() {
        assert!(mentions_card("Slash commands in a card's chat (card #94)", 94));
        assert!(mentions_card("CARD #94: fix", 94));
        assert!(mentions_card("Drop a card on a Ship it card (Card#94)", 94));
        assert!(mentions_card("Ship them as one (cards #89, #94)", 94));
        assert!(mentions_card("Ship them as one (cards #89, #94)", 89));
        assert!(mentions_card("cards #89 and #94", 94));
        assert!(mentions_card("cards #89, #90, and #94", 94));
        assert!(mentions_card("cards #89 & #94", 94));
        assert!(mentions_card("first line\n\nBody: refs card #94.", 94));
        assert!(mentions_card("card #94a", 94));
        assert!(!mentions_card("card #940", 94));
        assert!(!mentions_card("cards #89, #940", 94));
        assert!(!mentions_card("issue #94", 94));
        assert!(!mentions_card("fix #94", 94));
        assert!(!mentions_card("scorecard #94", 94));
        assert!(!mentions_card("card #9", 94));
        assert!(!mentions_card("card #94", 9));
        assert!(!mentions_card("cards #89 and issue #94", 94));
        assert!(!mentions_card("cardinal #94", 94));
        assert!(!mentions_card("", 94));
        assert!(mentions_card("ü card #94 ü", 94));
    }

    #[test]
    fn mentions_card_takes_ranges() {
        for msg in ["Build the stacks (cards #96-#112)", "cards #96–#112", "Cards #96 to #112", "cards #96 - #112", "cards #96-112"] {
            for n in [96, 100, 108, 112] {
                assert!(mentions_card(msg, n), "{msg} names #{n}");
            }
            assert!(!mentions_card(msg, 95), "{msg}");
            assert!(!mentions_card(msg, 113), "{msg}");
            assert!(!mentions_card(msg, 1120), "{msg}");
        }
        assert!(!mentions_card("cards #1000-#1001", 100));
        assert!(mentions_card("cards #1000-#1001", 1001));
        // A list goes on after a range.
        assert!(mentions_card("cards #96-#98, #120", 120));
        assert!(mentions_card("cards #90, #96-#98 and #120", 97));
        // Not a range.
        assert!(!mentions_card("card #96-fix #100", 100));
        assert!(!mentions_card("card #96 to fix #100", 100));
        assert!(!mentions_card("issue #96-#112", 100));
    }

    #[test]
    fn summed_commits_follow_renames_and_drop_added_then_deleted() {
        let f = |path: &str, old: Option<&str>, status: &str, a: u64| FileChange { path: path.into(), old: old.map(String::from), status: status.into(), added: Some(a), removed: Some(0) };
        let commit = |files: Vec<FileChange>| Section { sha: Some("x".into()), message: String::new(), files };
        // Added, then renamed: one file, at its new name, still added.
        let c = [commit(vec![f("a.txt", None, "A", 3)]), commit(vec![f("b.txt", Some("a.txt"), "R", 1)])];
        assert_eq!(sum_commits(c.iter()), vec![f("b.txt", None, "A", 4)]);
        // Changed, renamed, renamed again: the first old name is kept.
        let c = [
            commit(vec![f("a.txt", None, "M", 1)]),
            commit(vec![f("b.txt", Some("a.txt"), "R", 1)]),
            commit(vec![f("c.txt", Some("b.txt"), "R", 1)]),
        ];
        assert_eq!(sum_commits(c.iter()), vec![f("c.txt", Some("a.txt"), "R", 3)]);
        // Added, then deleted: left out.
        let c = [commit(vec![f("a.txt", None, "A", 3), f("k.txt", None, "M", 1)]), commit(vec![f("a.txt", None, "D", 0)])];
        assert_eq!(sum_commits(c.iter()), vec![f("k.txt", None, "M", 1)]);
        // Deleted after a change: a deletion.
        let c = [commit(vec![f("a.txt", None, "M", 3)]), commit(vec![f("a.txt", None, "D", 0)])];
        assert_eq!(sum_commits(c.iter())[0].status, "D");
    }

    #[test]
    fn an_output_opens_only_if_the_card_still_has_it() {
        let c = card("Human test", json!({ "title": "t", "changesOutputs": [{ "path": "C:\\x\\a.md" }, { "path": "C:\\x\\b.png" }] }));
        assert_eq!(stored_output(&c, 1, "C:\\x\\b.png").unwrap(), "C:\\x\\b.png");
        assert_eq!(stored_output(&c, 0, "C:\\x\\b.png").unwrap_err(), OUTPUTS_CHANGED);
        assert_eq!(stored_output(&c, 2, "C:\\x\\b.png").unwrap_err(), OUTPUTS_CHANGED);
        assert_eq!(stored_output(&card("Human test", json!({ "title": "t" })), 0, "C:\\x\\a.md").unwrap_err(), OUTPUTS_CHANGED);
    }

    #[test]
    fn usable_checkouts() {
        let t = |p: Option<&str>| Target { number: 1, checkout: p.map(String::from), branch: None, via_lead: false };
        assert_eq!(usable(&t(None)), Err(NO_CHECKOUT));
        assert_eq!(usable(&t(Some("  "))), Err(NO_CHECKOUT));
        assert_eq!(usable(&t(Some(r"\\wsl$\U\home\h"))), Err(IN_WSL));
        assert_eq!(usable(&t(Some(r"\\host\share"))), Err(ON_NETWORK));
        assert_eq!(usable(&t(Some("repo"))), Err(NOT_FULL));
        assert_eq!(usable(&t(Some(r"C:\Users\x"))), Ok(()));
        // The board and the panel answer the same, without git.
        let live = live_for("c", &t(Some("  ")), true);
        assert_eq!(live.why.as_deref(), Some(NO_CHECKOUT));
    }

    #[test]
    fn wsl_and_network_paths_are_not_read() {
        assert_eq!(checkout_problem(None), None);
        assert_eq!(checkout_problem(Some(r"C:\Users\someone\kanban-app")), None);
        assert_eq!(checkout_problem(Some("C:/Users/x")), None);
        assert_eq!(checkout_problem(Some(r"\\wsl$\Ubuntu\home\h\web")), Some(IN_WSL));
        assert_eq!(checkout_problem(Some(r"\\wsl.localhost\Ubuntu\home\h")), Some(IN_WSL));
        assert_eq!(checkout_problem(Some("//wsl.localhost/Ubuntu/home/h")), Some(IN_WSL));
        assert_eq!(checkout_problem(Some("/home/h/web")), Some(IN_WSL));
        assert_eq!(checkout_problem(Some("/mnt/c/Users/x")), Some(IN_WSL));
        assert_eq!(checkout_problem(Some(r"\\server\share\repo")), Some(ON_NETWORK));
        assert_eq!(checkout_problem(Some("//server/share/repo")), Some(ON_NETWORK));
        assert_eq!(checkout_problem(Some("repo")), Some(NOT_FULL));
        assert_eq!(checkout_problem(Some("C:repo")), Some(NOT_FULL));
        assert_eq!(checkout_dir(None).unwrap_err(), NO_CHECKOUT);
        assert_eq!(checkout_dir(Some("  ")).unwrap_err(), NO_CHECKOUT);
        assert_eq!(checkout_dir(Some(r"C:\definitely\not\here\kanban-108")).unwrap_err(), GONE);
    }

    #[test]
    fn only_some_files_open() {
        for p in [r"C:\x\a.md", r"C:\x\a.TXT", "C:/x/a.png", r"C:\x\a.jpg", r"C:\x\a.JPEG", r"C:\x\a.pdf"] {
            assert!(open_allowed(p, false).is_ok(), "{p}");
        }
        for p in [r"C:\x\a.exe", r"C:\x\a.html", r"C:\x\a.bat", r"C:\x\a", r"C:\x\a.md.lnk", r"C:\x\a.ps1"] {
            assert_eq!(open_allowed(p, false).unwrap_err(), ONLY_OPENABLE, "{p}");
            assert!(open_allowed(p, true).is_ok(), "shown in its folder: {p}");
        }
        for p in [r"\\server\s\a.md", "//server/s/a.md", r"\\wsl$\U\a.md", "/home/a.md", "a.md", "https://x/a.md"] {
            assert!(open_allowed(p, false).is_err() && open_allowed(p, true).is_err(), "{p}");
        }
    }

    #[test]
    fn branches_are_checked() {
        assert_eq!(clean_branch(Some(" dsh-108 ")).as_deref(), Some("dsh-108"));
        assert_eq!(clean_branch(Some("refs/heads/feat/x")).as_deref(), Some("feat/x"));
        for b in ["", "a b", "-x", "a..b", "a:b", "a~1", "a^", "x.lock", "a*", "a\\b", "@{u}x@{1}"] {
            assert_eq!(clean_branch(Some(b)), None, "{b}");
        }
    }

    #[test]
    fn parses_raw_and_numstat_with_renames_binaries_and_odd_names() {
        // As `git diff --raw --numstat -z -M` writes it.
        let mut out: Vec<u8> = Vec::new();
        for part in [
            ":000000 100644 0000000 b680253 A", "added.txt",
            ":100644 100644 bdc955b 8835708 M", "bin.dat",
            ":100644 100644 0fdf397 f9d9a01 R085", "old name.txt", "new name.txt",
            ":100644 100644 587be6b 975fbec M", "ü\tx.txt",
            ":100644 000000 587be6b 0000000 D", "gone.txt",
            "1\t0\tadded.txt",
            "-\t-\tbin.dat",
            "1\t0\t", "old name.txt", "new name.txt",
            "1\t1\tü\tx.txt",
            "0\t3\tgone.txt",
        ] {
            out.extend(part.as_bytes());
            out.push(0);
        }
        let s = parse_stream(&out);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].sha, None);
        let f = &s[0].files;
        let fc = |path: &str, old: Option<&str>, status: &str, a: Option<u64>, r: Option<u64>| FileChange {
            path: path.into(),
            old: old.map(String::from),
            status: status.into(),
            added: a,
            removed: r,
        };
        assert_eq!(
            f,
            &vec![
                fc("added.txt", None, "A", Some(1), Some(0)),
                fc("bin.dat", None, "M", None, None),
                fc("new name.txt", Some("old name.txt"), "R", Some(1), Some(0)),
                fc("ü\tx.txt", None, "M", Some(1), Some(1)),
                fc("gone.txt", None, "D", Some(0), Some(3)),
            ]
        );
        assert_eq!(totals(f), Totals { files: 5, added: 3, removed: 4 });
    }

    #[test]
    fn parses_log_sections() {
        let mut out: Vec<u8> = Vec::new();
        let push = |out: &mut Vec<u8>, s: &[u8]| {
            out.extend(s);
            out.push(0);
        };
        push(&mut out, b"\x01aaaaaaaaaaaaaaaa\x02Second (card #7)\n\nBody\n\x03");
        push(&mut out, b"\n:100644 100644 1 2 M");
        push(&mut out, b"a.txt");
        push(&mut out, b"2\t1\ta.txt");
        push(&mut out, b"\x01bbbbbbbbbbbbbbbb\x02Empty commit\n\x03");
        push(&mut out, b"\x01cccccccccccccccc\x02First (card #7)\n\x03");
        push(&mut out, b"\n:000000 100644 0 1 A");
        push(&mut out, b"a.txt");
        push(&mut out, b"3\t0\ta.txt");
        let s = parse_stream(&out);
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].sha.as_deref(), Some("aaaaaaaaaaaaaaaa"));
        assert!(s[0].message.starts_with("Second (card #7)") && s[0].message.contains("Body"));
        assert_eq!(s[0].files.len(), 1);
        assert!(s[1].files.is_empty());
        assert_eq!(s[2].files[0].status, "A");
        // Summed oldest first: added in the first, changed in the second.
        let sum = sum_commits(s.iter().rev());
        assert_eq!(sum, vec![FileChange { path: "a.txt".into(), old: None, status: "A".into(), added: Some(5), removed: Some(1) }]);
        assert_eq!(commit_line(s.into_iter().next().unwrap()), CommitLine { sha: "aaaaaaa".into(), subject: "Second (card #7)".into() });
    }

    #[test]
    fn parses_untracked_names() {
        let out = b"?? new file.txt\0 M a.txt\0R  b.txt\0?? old-looking.txt\0?? \xc3\xbc.md\0?? dir/\0";
        // The rename's old name ("?? old-looking.txt" here) is skipped as its second entry.
        assert_eq!(parse_untracked(out), vec!["new file.txt", "ü.md", "dir/"]);
    }

    #[test]
    fn secrets_are_masked() {
        assert_eq!(mask_path("src/password-reset.ts".into()), "(path hidden)");
        assert_eq!(mask_path("src/a.ts".into()), "src/a.ts");
        assert_eq!(mask_subject("Use sk_live_123 (card #5)".into()), "(subject hidden)");
        assert_eq!(mask_subject("Fine (card #5)".into()), "Fine (card #5)");
    }

    fn card(column: &str, fields: Value) -> Card {
        Card { id: "card-5".into(), number: 5, column: column.into(), order: None, order_col: None, archived: false, fields: fields.as_object().cloned().unwrap(), version: 1 }
    }

    fn figures(files: usize) -> Figures {
        let fl: Vec<FileChange> = (0..files).map(|i| FileChange { path: format!("f{i}"), old: None, status: "A".into(), added: Some(1), removed: Some(0) }).collect();
        Figures { source: "branch".into(), branch: Some("b".into()), base: Some("abc".into()), base_name: None, totals: totals(&fl), files: fl, more: 0, commits: vec![], commit_count: 0 }
    }

    #[test]
    fn the_panel_falls_back_to_saved_figures_then_says_why() {
        // Live figures win, cut to 300 files.
        let c = card("In progress", json!({ "title": "t" }));
        let out = assemble(&c, Live { figures: Some(figures(305)), ..Live::default() });
        assert_eq!((out.source.as_str(), out.files.len(), out.more, out.totals.files), ("branch", 300, 5, 305));
        // Nothing live: the saved figures.
        let saved = serde_json::from_value::<Value>(json!({ "at": "2026-11-06T10:00:00.000Z", "source": "commits", "branch": "b",
            "base": null, "totals": { "files": 1, "added": 2, "removed": 0 },
            "files": [{ "path": "a", "old": null, "status": "A", "added": 2, "removed": 0 }], "more": 0,
            "commits": [{ "sha": "abcdef1", "subject": "s" }], "commitCount": 1 })).unwrap();
        let c = card("Deploy complete", json!({ "title": "t", "changesSaved": saved }));
        let out = assemble(&c, Live { why: Some(GONE.into()), ..Live::default() });
        assert_eq!(out.source, "saved");
        assert_eq!(out.saved_at.as_deref(), Some("2026-11-06T10:00:00.000Z"));
        assert_eq!((out.totals.added, out.commit_count, out.files[0].path.as_str()), (2, 1, "a"));
        assert_eq!(brief_of(&c, &Live::default()), Some(Brief { files: 1, added: 2, removed: 0, outputs: 0, saved: true, paths: vec!["a".into()] }));
        // Nothing at all.
        let out = assemble(&card("Ship it", json!({ "title": "t" })), Live { why: Some(GONE.into()), ..Live::default() });
        assert_eq!((out.source.as_str(), out.why.as_deref()), ("none", Some("No figures kept")));
        let out = assemble(&card("In progress", json!({ "title": "t" })), Live { why: Some(NO_CHECKOUT.into()), ..Live::default() });
        assert_eq!(out.why.as_deref(), Some(NO_CHECKOUT));
        let out = assemble(&card("In progress", json!({ "title": "t" })), Live::default());
        assert_eq!(out.why.as_deref(), Some("No changes yet"));
        let mut gone = card("In progress", json!({ "title": "t" }));
        gone.archived = true;
        assert_eq!(assemble(&gone, Live::default()).why.as_deref(), Some("No figures kept"));
        // The board runs no git for a Done card: its saved figures only.
        assert!(!asks_git(&card("Done", json!({ "title": "t" }))));
        assert!(["In progress", "Human test", "Ship it", "Deploy complete"].iter().all(|col| asks_git(&card(col, json!({ "title": "t" })))));
        // The board leaves out a card with nothing.
        assert_eq!(brief_of(&card("In progress", json!({ "title": "t" })), &Live::default()), None);
        let with = card("In progress", json!({ "title": "t", "changesOutputs": [{ "path": "C:\\nope\\a.md" }, { "path": "C:\\nope\\b.exe", "label": "B" }] }));
        assert_eq!(brief_of(&with, &Live::default()), Some(Brief { files: 0, added: 0, removed: 0, outputs: 2, saved: false, paths: vec![] }));
        let out = assemble(&with, Live::default());
        assert_eq!(out.outputs[0], OutputFile { label: None, path: "C:\\nope\\a.md".into(), name: "a.md".into(), exists: false, size: None, modified: None, openable: false });
        assert_eq!(out.outputs[1].label.as_deref(), Some("B"));
    }

    #[test]
    fn the_brief_names_the_most_changed_paths() {
        let f = |path: &str, added: Option<u64>, removed: Option<u64>| FileChange { path: path.into(), old: None, status: "M".into(), added, removed };
        let files = vec![
            f("src/small.rs", Some(1), Some(0)),
            f("src/b.rs", Some(10), Some(5)),
            f("src\\win\\a.rs", Some(7), Some(8)),
            f("src/password-reset.ts", Some(500), Some(0)),
            f("(path hidden)", Some(400), Some(0)),
            f("img/logo.png", None, None),
            f("src/big.rs", Some(100), Some(0)),
        ];
        // Most lines first, a tie by name, backslashes made forward, secret paths left out.
        assert_eq!(top_paths(&files), vec!["src/big.rs", "src/b.rs", "src/win/a.rs"]);
        assert_eq!(top_paths(&files[..1]), vec!["src/small.rs"]);
        assert!(top_paths(&[]).is_empty());
        // Live figures give the brief its paths, and so do saved ones.
        let mut fig = figures(0);
        fig.files = files.clone();
        let b = brief_of(&card("In progress", json!({ "title": "t" })), &Live { figures: Some(fig), ..Live::default() }).unwrap();
        assert_eq!(b.paths.len(), BRIEF_PATHS);
        let v = serde_json::to_value(&b).unwrap();
        assert_eq!(v["paths"], json!(["src/big.rs", "src/b.rs", "src/win/a.rs"]));
    }

    #[test]
    fn saved_figures_keep_100_files_and_read_back() {
        let v = saved_value(&figures(130));
        assert_eq!(v["files"].as_array().unwrap().len(), 100);
        assert_eq!((v["more"].clone(), v["totals"]["files"].clone(), v["source"].clone()), (json!(30), json!(130), json!("branch")));
        assert!(v["at"].as_str().unwrap().ends_with('Z'));
        assert!(v.get("commitCount").is_some() && v.get("base").is_some());
        let back: Saved = serde_json::from_value(v).unwrap();
        assert_eq!(back.figures.files.len(), 100);
    }

    #[test]
    fn json_names_are_camel_case() {
        let c = assemble(&card("In progress", json!({ "title": "t" })), Live::default());
        let v = serde_json::to_value(&c).unwrap();
        for k in ["source", "why", "checkout", "branch", "base", "totals", "files", "more", "commits", "commitCount", "uncommitted", "savedAt", "outputs"] {
            assert!(v.get(k).is_some(), "{k}");
        }
        let f = serde_json::to_value(FileChange::default()).unwrap();
        for k in ["path", "old", "status", "added", "removed"] {
            assert!(f.get(k).is_some(), "{k}");
        }
    }

    // --- Scratch repos -----------------------------------------------------------------------

    fn have_git() -> bool {
        Command::new("git").arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().map_or(false, |s| s.success())
    }

    fn sh(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "core.autocrlf=false", "-c", "commit.gpgsign=false", "-C"])
            .arg(dir)
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn write(dir: &Path, name: &str, body: &[u8]) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    fn commit(dir: &Path, msg: &str) {
        sh(dir, &["add", "-A"]);
        sh(dir, &["commit", "-q", "-m", msg]);
    }

    fn target(n: i64, dir: &Path, branch: Option<&str>) -> Target {
        Target { number: n, checkout: Some(dir.to_string_lossy().into_owned()), branch: branch.map(String::from), via_lead: false }
    }

    fn find<'a>(files: &'a [FileChange], path: &str) -> &'a FileChange {
        files.iter().find(|f| f.path == path).unwrap_or_else(|| panic!("no {path} in {files:?}"))
    }

    #[test]
    fn reads_scratch_repos() {
        if !have_git() {
            eprintln!("git isn't on PATH: skipped");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let wt = tmp.path().join("wt");
        std::fs::create_dir(&repo).unwrap();
        sh(&repo, &["init", "-q", "-b", "main"]);
        write(&repo, "a.txt", b"one\n");
        write(&repo, "old name.txt", b"1\n2\n3\n4\n5\n6\n7\n8\n");
        write(&repo, "bin.dat", b"\x00\x01\x02");
        commit(&repo, "Start");
        let main_sha = sh(&repo, &["rev-parse", "HEAD"]);
        let mut cache = Cache::new();

        // A branch with two commits, in its own linked worktree: a new file, a rename and a binary.
        sh(&repo, &["worktree", "add", "-q", "-b", "feat", wt.to_str().unwrap(), "main"]);
        write(&wt, "b c.txt", b"x\ny\n");
        commit(&wt, "Add b (card #7)");
        sh(&wt, &["mv", "old name.txt", "new name.txt"]);
        write(&wt, "new name.txt", b"1\n2\n3\n4\n5\n6\n7\n8\n9\n");
        write(&wt, "bin.dat", b"\x00\x03\x04");
        commit(&wt, "Rename and binary (card #7)");
        let t7 = target(7, &wt, Some("feat"));
        let live = run_job(&mut cache, "c7", &t7, false);
        let f = live.figures.clone().expect("branch figures");
        assert_eq!(f.source, "branch");
        assert_eq!(f.base.as_deref(), Some(main_sha.as_str()));
        assert_eq!(f.base_name.as_deref(), Some("main"));
        assert_eq!(f.branch.as_deref(), Some("feat"));
        assert_eq!((f.commit_count, f.commits[0].subject.as_str(), f.commits[0].sha.len()), (2, "Rename and binary (card #7)", 7));
        assert_eq!(find(&f.files, "b c.txt").status, "A");
        let r = find(&f.files, "new name.txt");
        assert_eq!((r.status.as_str(), r.old.as_deref(), r.added, r.removed), ("R", Some("old name.txt"), Some(1), Some(0)));
        let b = find(&f.files, "bin.dat");
        assert_eq!((b.status.as_str(), b.added, b.removed), ("M", None, None));
        assert_eq!(f.totals, Totals { files: 3, added: 3, removed: 0 });
        assert!(live.uncommitted.is_none(), "the board's job has no uncommitted scan");
        assert!(cache.contains_key("c7"));
        // The same again comes from the cache.
        assert_eq!(run_job(&mut cache, "c7", &t7, false).figures, Some(f.clone()));

        // A card with no branch field in its own worktree uses the one checked out there.
        assert_eq!(run_job(&mut cache, "c7b", &target(7, &wt, None), false).figures.map(|f| f.source), Some("branch".into()));

        // Uncommitted and untracked work in the worktree, for the panel only.
        write(&wt, "a.txt", b"one\ntwo\n");
        write(&wt, "untracked ü.txt", b"new\n");
        let live = run_job(&mut cache, "c7", &t7, true);
        let u = live.uncommitted.expect("uncommitted work");
        assert_eq!(u.files.len(), 1);
        assert_eq!((u.files[0].path.as_str(), u.files[0].status.as_str(), u.files[0].added), ("a.txt", "M", Some(1)));
        assert_eq!(u.untracked, vec!["untracked ü.txt".to_string()]);
        assert_eq!(live.figures, Some(f.clone()), "uncommitted work isn't in the committed figures");
        // A stack's child on its lead's checkout counts only its own commits, and never the
        // lead's uncommitted work.
        let child = Target { number: 7, checkout: t7.checkout.clone(), branch: Some("feat".into()), via_lead: true };
        let live = run_job(&mut cache, "child", &child, true);
        assert!(live.uncommitted.is_none());
        let cf = live.figures.expect("the child's commits");
        assert_eq!((cf.source.as_str(), cf.commit_count), ("commits", 2));
        let other_child = Target { number: 9, ..child.clone() };
        assert!(run_job(&mut cache, "child9", &other_child, true).figures.is_none());

        // Main with commits naming card #8, one naming #80, and a list naming #3 and #8.
        write(&repo, "m1.txt", b"a\n");
        commit(&repo, "Main work (card #8)");
        write(&repo, "m2.txt", b"a\n");
        commit(&repo, "Not it (card #80)");
        write(&repo, "m1.txt", b"a\nb\n");
        commit(&repo, "Fix cards #3 and #8");
        write(&repo, "m3.txt", b"a\n");
        commit(&repo, "Mentions #8 bare");
        write(&repo, "m4.txt", b"a\n");
        commit(&repo, "Stack work (Cards #6-#9)");
        // The shared main checkout has another branch checked out now: it must not be used.
        sh(&repo, &["checkout", "-q", "-b", "other"]);
        write(&repo, "o.txt", b"o\n");
        commit(&repo, "On other (card #8)");
        let t8 = target(8, &repo, None);
        let live = run_job(&mut cache, "c8", &t8, true);
        assert!(live.uncommitted.is_none(), "a shared main checkout has no uncommitted work of the card's");
        let f8 = live.figures.unwrap_or_else(|| panic!("commit figures: {:?}", live.why));
        assert_eq!(f8.source, "commits");
        assert_eq!(f8.branch.as_deref(), Some("main"));
        assert_eq!(f8.commit_count, 3);
        assert_eq!(find(&f8.files, "m4.txt").status, "A");
        assert_eq!(f8.commits.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(), vec!["Stack work (Cards #6-#9)", "Fix cards #3 and #8", "Main work (card #8)"]);
        assert_eq!(f8.files.len(), 2, "{:?}", f8.files);
        let m1 = find(&f8.files, "m1.txt");
        assert_eq!((m1.status.as_str(), m1.added), ("A", Some(2)));
        // A card's branch field that is main reads main's commits too.
        assert_eq!(run_job(&mut cache, "c8m", &target(8, &repo, Some("main")), false).figures.map(|f| f.commit_count), Some(3));
        // A branch field naming a branch that isn't there, in the shared checkout: main's commits.
        assert_eq!(run_job(&mut cache, "c8x", &target(8, &repo, Some("gone")), false).figures.map(|f| f.source), Some("commits".into()));

        // Merged: the branch's diff is empty, so its commits are the figures.
        sh(&repo, &["checkout", "-q", "main"]);
        sh(&repo, &["merge", "-q", "--no-ff", "feat", "-m", "Merge feat"]);
        let live = run_job(&mut cache, "c7", &t7, true);
        let f = live.figures.expect("commits after the merge");
        assert_eq!((f.source.as_str(), f.commit_count, f.base.as_deref()), ("commits", 2, None));
        assert!(f.files.iter().any(|x| x.path == "b c.txt") && f.files.iter().any(|x| x.path == "new name.txt"));

        // No repo, no checkout.
        let plain = tmp.path().join("plain");
        std::fs::create_dir(&plain).unwrap();
        assert_eq!(run_job(&mut cache, "p", &target(1, &plain, None), true).why.as_deref(), Some(NOT_A_REPO));
        assert_eq!(run_job(&mut cache, "g", &target(1, &tmp.path().join("gone"), None), true).why.as_deref(), Some(GONE));
    }
}
