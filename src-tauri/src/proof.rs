//! A card's proof in Human test (card #125): the screenshots and reports that show its change
//! works, shown as pictures and readable docs instead of file paths.
//!
//! - `proof_items(card)`: the card's `proof` field, then the pictures and docs its hand-over
//!   (`nextStep`) names, each with a state: ok, missing, refused (with why) or dup (already a
//!   shot in the review strip).
//! - `check_read`: which files the app may read or open for a card. Only files the card names,
//!   inside the home or temp folder, never in a dot folder (bar a repo's card worktrees) or the
//!   system's settings folders. Paths are resolved first.
//! - `read_doc`: an md or txt report as text, at most 1 MB, decoded and with tokens masked.
//! - The commands `proof_list`, `proof_read` and `proof_open`. Each reads the card under the
//!   board lock and lets go of it before it touches the disk.

use crate::Board;
use kanban_store::{Card, PROOF_KEY, REPRO_KEY};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// The most of a report the app reads.
pub const DOC_CAP: usize = 1024 * 1024;
/// The biggest image compared byte for byte with the strip's shots.
const DUP_BYTES: u64 = 25 * 1024 * 1024;

const IMAGE: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];
const DOC: &[&str] = &["md", "txt"];
const OPENABLE: &[&str] = &["md", "txt", "png", "jpg", "jpeg", "gif", "webp", "pdf"];

fn ext(p: &str) -> String {
    let name = p.rsplit(['\\', '/']).next().unwrap_or(p);
    match name.rsplit_once('.') {
        Some((stem, e)) if !stem.is_empty() => e.to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// image, doc, pdf or file, by the file's extension.
pub fn kind_of(p: &str) -> &'static str {
    let e = ext(p);
    if IMAGE.contains(&e.as_str()) {
        "image"
    } else if DOC.contains(&e.as_str()) {
        "doc"
    } else if e == "pdf" {
        "pdf"
    } else {
        "file"
    }
}

/// A kind the hand-over text may name: pictures, docs and PDFs, never source files or scripts.
fn text_kind(p: &str) -> bool {
    kind_of(p) != "file"
}

fn file_name(p: &str) -> &str {
    p.rsplit(['\\', '/']).find(|s| !s.is_empty()).unwrap_or(p)
}

// --- Finding paths in the hand-over text ----------------------------------------------------

/// A path the hand-over names, and the exact text it was written as.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub path: String,
    pub spelling: String,
}

/// The lines of `text` that are prose, not code: fenced blocks (``` or ~~~) and indented code
/// blocks (four spaces or a tab after a blank line, not inside a list) are left out. The same rule
/// as ui/proof.js's `pfLines`.
pub fn prose_lines(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut fence: Option<&str> = None;
    let mut prev_blank = true;
    let mut in_list = false;
    let mut in_indented = false;
    for (i, line) in text.lines().enumerate() {
        let t = line.trim_start();
        if let Some(f) = fence {
            if t.starts_with(f) {
                fence = None;
            }
            prev_blank = false;
            continue;
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = Some(if t.starts_with("```") { "```" } else { "~~~" });
            prev_blank = false;
            in_indented = false;
            continue;
        }
        let blank = t.is_empty();
        let indented = line.starts_with("    ") || line.starts_with('\t');
        if !blank && indented && (in_indented || (prev_blank && !in_list)) {
            in_indented = true;
            prev_blank = false;
            continue;
        }
        if !blank {
            in_indented = false;
            if !indented {
                in_list = is_list_item(t);
            }
        }
        prev_blank = blank;
        out.push((i, line));
    }
    out
}

fn is_list_item(t: &str) -> bool {
    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        return true;
    }
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && matches!(t.as_bytes().get(digits), Some(b'.') | Some(b')')) && t.as_bytes().get(digits + 1) == Some(&b' ')
}

fn is_drive_path(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

/// A path's end: whitespace, a quote, a backtick, < > | or a glob character.
fn path_char(c: char) -> bool {
    !(c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | '|' | '*' | '?'))
}

const TRAILING: &[char] = &[',', '.', ';', ':', ')', ']', '"', '\''];

/// Spans of `line` that are code spans or Markdown link and image targets, as byte ranges.
fn skipped_spans(line: &str) -> (Vec<(usize, usize)>, Vec<(usize, usize)>) {
    let b = line.as_bytes();
    let mut code = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'`' {
            let run = b[i..].iter().take_while(|&&c| c == b'`').count();
            let ticks = &line[i..i + run];
            if let Some(end) = line[i + run..].find(ticks) {
                code.push((i, i + run + end + run));
                i = i + run + end + run;
                continue;
            }
            i += run;
            continue;
        }
        i += 1;
    }
    let mut targets = Vec::new();
    let mut from = 0;
    while let Some(off) = line[from..].find("](") {
        let start = from + off + 2;
        if code.iter().any(|&(a, z)| start > a && start < z) {
            from = start;
            continue;
        }
        let end = line[start..].find(')').map_or(line.len(), |e| start + e + 1);
        targets.push((start, end));
        from = end;
    }
    (code, targets)
}

fn inside(spans: &[(usize, usize)], at: usize) -> bool {
    spans.iter().any(|&(a, z)| at >= a && at < z)
}

/// The full paths in one prose line, with their byte offsets: bare ones ending at a path's end,
/// quoted ones (which may hold spaces), and code spans whose whole text is one path. Markdown
/// link and image targets are left out.
fn line_paths(line: &str) -> Vec<(usize, Found)> {
    let (code, targets) = skipped_spans(line);
    let mut out = Vec::new();
    for &(a, z) in &code {
        let run = line[a..].bytes().take_while(|&c| c == b'`').count();
        let inner = &line[a + run..z - run];
        if is_drive_path(inner) && !inner.contains(['*', '?']) {
            out.push((a, Found { path: inner.to_string(), spelling: inner.to_string() }));
        }
    }
    let b = line.as_bytes();
    let mut i = 0;
    while i + 2 < b.len() {
        if inside(&code, i) || inside(&targets, i) {
            i += 1;
            continue;
        }
        let starts = b[i].is_ascii_alphabetic() && is_drive_path(&line[i..]) && (i == 0 || !(b[i - 1] as char).is_ascii_alphanumeric());
        if !starts {
            i += 1;
            continue;
        }
        let quote = if i > 0 { Some(b[i - 1]).filter(|q| *q == b'"' || *q == b'\'') } else { None };
        let end = match quote.and_then(|q| line[i..].find(q as char).map(|e| i + e)) {
            Some(e) if !line[i..e].contains(['*', '?']) => e,
            _ => i + line[i..].find(|c: char| !path_char(c)).unwrap_or(line.len() - i),
        };
        let mut raw = &line[i..end];
        // A glob has more of the path after its * or ? (M-*.png); a closing ** (bold) or a
        // question mark at a sentence's end doesn't.
        let glob = {
            let after = line[end..].trim_start_matches(['*', '?']);
            after.len() < line.len() - end && {
                let rest = &after[..after.find(|c: char| !path_char(c)).unwrap_or(after.len())];
                !rest.trim_end_matches(TRAILING).is_empty()
            }
        };
        raw = raw.trim_end_matches(TRAILING);
        if !glob && !raw.is_empty() {
            out.push((i, Found { path: raw.to_string(), spelling: raw.to_string() }));
        }
        i = end.max(i + 1);
    }
    out.sort_by_key(|(at, _)| *at);
    out
}

fn folder_of(p: &str) -> &str {
    match p.rfind(['\\', '/']) {
        Some(i) => &p[..=i],
        None => "",
    }
}

/// The pictures, docs and PDFs the hand-over text names, in order, with each spelling. A bare
/// file name (B-107-2.png) counts only after a full path on the same line, in the folder of the
/// nearest full path before it, and only when `exists` finds it there.
pub fn find_in_text(text: &str, exists: &dyn Fn(&str) -> bool) -> Vec<Found> {
    let mut out = Vec::new();
    for (_, line) in prose_lines(text) {
        let full = line_paths(line);
        for (_, f) in &full {
            if text_kind(&f.path) {
                out.push(f.clone());
            }
        }
        if full.is_empty() {
            continue;
        }
        let (code, targets) = skipped_spans(line);
        let covered: Vec<(usize, usize)> = full.iter().map(|(a, f)| (*a, a + f.spelling.len())).collect();
        let b = line.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let word_start = (b[i].is_ascii_alphanumeric() || b[i] == b'_') && (i == 0 || !matches!(b[i - 1], b'\\' | b'/' | b':' | b'.' | b'-' | b'_') && !(b[i - 1] as char).is_ascii_alphanumeric());
            if !word_start || inside(&covered, i) || inside(&code, i) || inside(&targets, i) {
                i += 1;
                continue;
            }
            let end = i + line[i..].find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))).unwrap_or(line.len() - i);
            let word = line[i..end].trim_end_matches('.');
            if word.contains('.') && text_kind(word) {
                if let Some((_, near)) = full.iter().rev().find(|(a, _)| *a < i) {
                    // In the folder of the nearest full path, or in that path itself when it is
                    // a folder ("Shots in C:\x\final: B-1.png, B-2.png").
                    let sep = if near.path.contains('\\') { '\\' } else { '/' };
                    let inside_it = format!("{}{sep}{word}", near.path.trim_end_matches(['\\', '/']));
                    let found = [format!("{}{}", folder_of(&near.path), word), inside_it].into_iter().find(|p| exists(p));
                    if let Some(path) = found {
                        out.push(Found { path, spelling: word.to_string() });
                    }
                }
            }
            i = end.max(i + 1);
        }
    }
    out
}

// --- Which files the app may read -------------------------------------------------------------

/// Why the app won't read `real` (already resolved) for a card, if it won't. `home` and `temp`
/// are resolved too.
pub fn check_read(real: &Path, home: &Path, temp: &Path) -> Result<(), String> {
    let (rest, under_home) = if let Ok(r) = real.strip_prefix(temp) {
        (r, false)
    } else if let Ok(r) = real.strip_prefix(home) {
        (r, true)
    } else {
        return Err("outside your home and temp folders".into());
    };
    let parts: Vec<String> = rest.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    if under_home {
        if let Some(first) = parts.first() {
            if first.to_ascii_lowercase().starts_with(".claude-kanban-dsh") {
                return Err("in the app's data folder".into());
            }
            if first.starts_with('.') {
                return Err("in a settings folder".into());
            }
        }
        if crate::platform::in_settings_folder(home, real) {
            return Err("in a settings folder".into());
        }
    }
    let mut worktree_used = false;
    for (i, part) in parts.iter().enumerate() {
        if !part.starts_with('.') {
            continue;
        }
        let worktrees = parts.get(i + 1).is_some_and(|n| n.eq_ignore_ascii_case("worktrees"));
        if part.eq_ignore_ascii_case(".claude") && worktrees && !worktree_used && i + 2 < parts.len() {
            worktree_used = true;
            continue;
        }
        return Err("in a settings folder".into());
    }
    Ok(())
}

// --- The card's items ------------------------------------------------------------------------

/// The shots the review strip draws for the card (review.js rvFigures): the latest evidence run's
/// shots and the reproducer's before and after.
pub fn strip_shots(card: &Card) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(run) = card.fields.get(kanban_store::EVIDENCE_KEY).and_then(Value::as_array).and_then(|a| a.first()) {
        for s in run.get("shots").and_then(Value::as_array).into_iter().flatten() {
            if let Some(p) = s.get("path").and_then(Value::as_str).filter(|p| !p.is_empty()) {
                out.push(p.to_string());
            }
        }
    }
    if let Some(rp) = card.fields.get(REPRO_KEY) {
        for k in ["before", "after"] {
            if let Some(p) = rp.get(k).and_then(|x| x.get("shot")).and_then(Value::as_str).filter(|p| !p.is_empty()) {
                out.push(p.to_string());
            }
        }
    }
    out
}

/// The folders the read rule compares against.
#[derive(Clone)]
pub struct Roots {
    pub home: PathBuf,
    pub temp: PathBuf,
}

impl Roots {
    pub fn system() -> Roots {
        let home = kanban_store::home_var().unwrap_or_else(crate::platform::no_home);
        Roots::new(&home, &std::env::temp_dir())
    }
    pub fn new(home: &Path, temp: &Path) -> Roots {
        Roots {
            home: std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf()),
            temp: std::fs::canonicalize(temp).unwrap_or_else(|_| temp.to_path_buf()),
        }
    }
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) else { return false };
    if ma.len() != mb.len() || ma.len() > DUP_BYTES {
        return false;
    }
    matches!((std::fs::read(a), std::fs::read(b)), (Ok(x), Ok(y)) if x == y)
}

fn key_of(p: &str) -> String {
    match std::fs::canonicalize(p) {
        Ok(r) => r.to_string_lossy().to_ascii_lowercase(),
        Err(_) => p.replace('/', "\\").to_ascii_lowercase(),
    }
}

/// One file the card names: its path as written, its label, the spellings the hand-over wrote it
/// as, and its key (resolved, any case).
struct Named {
    path: String,
    label: Option<String>,
    spellings: Vec<String>,
    key: String,
}

/// What the card names as proof, in order: its `proof` field, then what its hand-over names, one
/// entry per file however it was spelt.
fn named_files(card: &Card) -> Vec<Named> {
    let mut named: Vec<Named> = Vec::new();
    let mut push = |path: String, label: Option<String>, spelling: Option<String>| {
        let key = key_of(&path);
        if let Some(e) = named.iter_mut().find(|e| e.key == key) {
            if let Some(s) = spelling {
                if !e.spellings.contains(&s) {
                    e.spellings.push(s);
                }
            }
            return;
        }
        named.push(Named { path, label, spellings: spelling.into_iter().collect(), key });
    };
    for e in card.fields.get(PROOF_KEY).and_then(Value::as_array).into_iter().flatten() {
        if let Some(p) = e.get("path").and_then(Value::as_str) {
            push(p.to_string(), e.get("label").and_then(Value::as_str).map(str::to_string), None);
        }
    }
    let text = card.fields.get("nextStep").and_then(Value::as_str).unwrap_or("");
    for f in find_in_text(text, &|p| Path::new(p).is_file()) {
        push(f.path, None, Some(f.spelling));
    }
    named
}

/// One named file as an item with its state. `shots` are the review strip's shots with their keys.
fn item_of(n: Named, shots: &[(String, String)], roots: &Roots) -> Value {
    let Named { path, label, spellings, .. } = n;
    let kind = kind_of(&path);
    let mut item = json!({ "path": path, "name": file_name(&path), "kind": kind, "spellings": spellings });
    if let Some(l) = label {
        item["label"] = json!(l);
    }
    let real = match std::fs::canonicalize(&path) {
        Ok(r) if r.is_file() => r,
        _ => {
            item["state"] = json!("missing");
            return item;
        }
    };
    // A link must lead to the same kind of file as its name says: report.md can't be a script.
    if ext(&real.to_string_lossy()) != ext(&path) {
        item["state"] = json!("refused");
        item["why"] = json!("it points at a different kind of file");
        return item;
    }
    // The same file as a strip shot is a dup without reading it.
    let k = real.to_string_lossy().to_ascii_lowercase();
    if kind == "image" {
        if let Some((s, _)) = shots.iter().find(|(_, sk)| *sk == k) {
            item["state"] = json!("dup");
            item["dupOf"] = json!(s);
            return item;
        }
    }
    if let Err(why) = check_read(&real, &roots.home, &roots.temp) {
        item["state"] = json!("refused");
        item["why"] = json!(why);
        return item;
    }
    // A copy of a strip shot: its bytes are compared only once the read rule allows it.
    if kind == "image" {
        if let Some((s, _)) = shots.iter().find(|(s, _)| same_bytes(&real, Path::new(s.as_str()))) {
            item["state"] = json!("dup");
            item["dupOf"] = json!(s);
            return item;
        }
    }
    item["state"] = json!("ok");
    // The file that passed the check: reads and opens use it, not the path as written, so a link
    // swapped in at the written path afterwards isn't followed. Not shown to the page.
    item["real"] = json!(crate::platform::plain_path(&real));
    item
}

fn strip_with_keys(card: &Card) -> Vec<(String, String)> {
    strip_shots(card).into_iter().map(|s| {
        let k = key_of(&s);
        (s, k)
    }).collect()
}

/// The card's proof items, in order: its `proof` field, then what its hand-over names.
pub fn proof_items(card: &Card, roots: &Roots) -> Vec<Value> {
    let shots = strip_with_keys(card);
    named_files(card)
        .into_iter()
        .map(|n| {
            let mut i = item_of(n, &shots, roots);
            if let Some(o) = i.as_object_mut() {
                o.remove("real");
            }
            i
        })
        .collect()
}

/// The item for `path` on the card, when its state is ok; else why not. Only that item is checked.
fn ok_item(card: &Card, roots: &Roots, path: &str) -> Result<Value, String> {
    let n = named_files(card)
        .into_iter()
        .find(|n| n.path == path)
        .ok_or_else(|| "the card doesn't name this file as proof".to_string())?;
    let item = item_of(n, &strip_with_keys(card), roots);
    match item.get("state").and_then(Value::as_str) {
        Some("ok") => Ok(item),
        Some("missing") => Err(format!("{} isn't there any more", file_name(path))),
        Some("dup") => Err("this picture is already in the review strip".into()),
        _ => Err(item.get("why").and_then(Value::as_str).unwrap_or("not allowed").to_string()),
    }
}

// --- Reading a report -----------------------------------------------------------------------

/// A report's bytes (at most `DOC_CAP` of them) as text: decoded by its BOM (UTF-8 when it has
/// none, or UTF-16), cut back to a whole character and then to whitespace when the file was
/// `longer`, and refused when it isn't readable text. Returns the text and whether it was cut.
/// Tokens are masked after the cut, so a cut never splits one.
pub fn decode_doc(bytes: &[u8], longer: bool) -> Result<(String, bool), String> {
    let unreadable = || "not readable text".to_string();
    let text = if let Some(b) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        utf16(b, longer, u16::from_le_bytes).ok_or_else(unreadable)?
    } else if let Some(b) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        utf16(b, longer, u16::from_be_bytes).ok_or_else(unreadable)?
    } else {
        let b = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        match std::str::from_utf8(b) {
            Ok(s) => s.to_string(),
            // A character cut off by the cap: keep what came before it.
            Err(e) if longer && e.error_len().is_none() => std::str::from_utf8(&b[..e.valid_up_to()]).map_err(|_| unreadable())?.to_string(),
            Err(_) => return Err(unreadable()),
        }
    };
    if text.contains('\0') {
        return Err(unreadable());
    }
    let text = if longer {
        match text.rfind(char::is_whitespace) {
            Some(i) => text[..i].to_string(),
            None => String::new(),
        }
    } else {
        text
    };
    Ok((kanban_store::search::mask(&text), longer))
}

fn utf16(b: &[u8], longer: bool, word: fn([u8; 2]) -> u16) -> Option<String> {
    let b = if longer { &b[..b.len() & !1] } else { b };
    if b.len() % 2 != 0 {
        return None;
    }
    let mut units: Vec<u16> = b.chunks_exact(2).map(|c| word([c[0], c[1]])).collect();
    if longer && units.last().is_some_and(|u| (0xD800..0xDC00).contains(u)) {
        units.pop(); // half of a pair, cut off by the cap
    }
    String::from_utf16(&units).ok()
}

fn read_doc(path: &str) -> Result<(String, bool), String> {
    use std::io::Read;
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    // Still the file that was checked, now it is open.
    let now = std::fs::canonicalize(path).map(|r| crate::platform::plain_path(&r)).map_err(|e| e.to_string())?;
    if !now.eq_ignore_ascii_case(path) {
        return Err("the file changed while it was read".into());
    }
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    let mut bytes = Vec::new();
    f.take(DOC_CAP as u64).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    decode_doc(&bytes, len > DOC_CAP as u64)
}

// --- Commands --------------------------------------------------------------------------------

fn card_of(app: &AppHandle, id: &str) -> Result<Card, String> {
    let board = app.state::<Board>();
    let store = board.0.lock().map_err(|e| e.to_string())?;
    store.get(id).map_err(|e| e.to_string())
}

/// The card's proof items (`proof_items`).
#[tauri::command]
pub async fn proof_list(app: AppHandle, id: String) -> Result<Vec<Value>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let card = card_of(&app, &id)?;
        Ok(proof_items(&card, &Roots::system()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// What `proof_read` hands the page for one ok item.
pub fn read_item(item: &Value) -> Result<Value, String> {
    let path = item.get("real").or_else(|| item.get("path")).and_then(Value::as_str).unwrap_or("");
    match item.get("kind").and_then(Value::as_str) {
        Some("image") => Ok(json!({ "kind": "image", "url": crate::image_data_url(path)? })),
        Some("doc") => {
            let (text, cut) = read_doc(path)?;
            Ok(json!({ "kind": "doc", "text": text, "cut": cut, "plain": ext(path) == "txt" }))
        }
        _ => Err("only pictures and md or txt reports show here; open it in its app".into()),
    }
}

/// One of the card's proof files: a picture as a data URL, or a report's masked text.
#[tauri::command]
pub async fn proof_read(app: AppHandle, id: String, path: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let card = card_of(&app, &id)?;
        let item = ok_item(&card, &Roots::system(), &path)?;
        read_item(&item)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Opens an ok item with `open` (its app, or its folder when `reveal`), after the same checks
/// as `proof_read`. Only some kinds open in their app; any shows in its folder.
pub fn open_item(card: &Card, roots: &Roots, path: &str, reveal: bool, open: &dyn Fn(&str, bool) -> Result<(), String>) -> Result<(), String> {
    let item = ok_item(card, roots, path)?;
    let p = item.get("real").or_else(|| item.get("path")).and_then(Value::as_str).unwrap_or(path).to_string();
    // The kind of the file that is opened, not of the name the card wrote.
    if !reveal && !OPENABLE.contains(&ext(&p).as_str()) {
        return Err("this kind of file only shows in its folder".into());
    }
    open(&p, reveal)
}

/// Opens one of the card's proof files in its app, or shows it in its folder: on Hugh's click.
#[tauri::command]
pub async fn proof_open(app: AppHandle, id: String, path: String, reveal: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let card = card_of(&app, &id)?;
        open_item(&card, &Roots::system(), &path, reveal, &|p, reveal| {
            if reveal {
                tauri_plugin_opener::reveal_item_in_dir(p).map_err(|e| e.to_string())
            } else {
                tauri_plugin_opener::open_path(p, None::<&str>).map_err(|e| e.to_string())
            }
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn card(fields: Value) -> Card {
        Card { id: "c1".into(), number: 1, column: "Human test".into(), order: None, order_col: None, archived: false, fields: fields.as_object().cloned().unwrap(), version: 1 }
    }

    fn paths(f: &[Found]) -> Vec<&str> {
        f.iter().map(|x| x.path.as_str()).collect()
    }

    const NOWHERE: &dyn Fn(&str) -> bool = &|_| false;

    #[test]
    fn the_finder_takes_pictures_and_docs_and_their_bare_siblings() {
        // #107's shape: a report in one folder, a shot in another, then a bare name.
        let t = "Report: C:\\Users\\me\\crit\\final-test-B.md, section #107. Screenshots: C:\\Users\\me\\crit\\final\\B-107-1.png (the \"Asks you\" line) and B-107-2.png (Blocked).";
        let only_final = |p: &str| p == "C:\\Users\\me\\crit\\final\\B-107-2.png";
        let f = find_in_text(t, &only_final);
        assert_eq!(paths(&f), ["C:\\Users\\me\\crit\\final-test-B.md", "C:\\Users\\me\\crit\\final\\B-107-1.png", "C:\\Users\\me\\crit\\final\\B-107-2.png"]);
        assert_eq!(f[2].spelling, "B-107-2.png");
        // A sibling that isn't there is no item.
        assert_eq!(find_in_text(t, NOWHERE).len(), 2);
        // #116's shape: three siblings.
        let t = "Screenshots: C:/Users/me/final/B-116-1.png (a child), B-116-2.png, B-116-3.png.";
        let f = find_in_text(t, &|_| true);
        assert_eq!(paths(&f), ["C:/Users/me/final/B-116-1.png", "C:/Users/me/final/B-116-2.png", "C:/Users/me/final/B-116-3.png"]);
    }

    #[test]
    fn the_finder_leaves_out_code_images_globs_folders_and_source_files() {
        let t = [
            "See `C:\\Users\\me\\a b\\shot.png` and \"C:\\Users\\me\\x y\\r.md\".",
            "![shot](C:\\Users\\me\\img.png) and [link](C:\\Users\\me\\l.md)",
            "Globs C:\\Users\\me\\final\\M-*.png and folder C:\\Users\\me\\mockup, a dot folder C:\\Users\\me\\.claude-kanban-dsh\\api.md.",
            "Code C:\\Users\\me\\repo\\ui\\review.js and C:\\Users\\me\\repo\\scripts\\install.ps1.",
            "A code span `run C:\\Users\\me\\c.png now`.",
            "",
            "```",
            "C:\\Users\\me\\fenced.png",
            "```",
            "",
            "    C:\\Users\\me\\indented.png",
            "",
            "Later README.md alone.",
        ]
        .join("\n");
        let f = find_in_text(&t, &|_| true);
        assert_eq!(paths(&f), ["C:\\Users\\me\\a b\\shot.png", "C:\\Users\\me\\x y\\r.md", "C:\\Users\\me\\.claude-kanban-dsh\\api.md"]);
    }

    #[test]
    fn a_list_items_indented_line_is_prose_and_two_spellings_are_one_item() {
        let t = "1. Look at C:\\Users\\me\\s.png\n    then C:/Users/me/s.png again.";
        let f = find_in_text(t, NOWHERE);
        assert_eq!(f.len(), 2);
        let c = card(json!({ "nextStep": t }));
        let dir = tempfile::tempdir().unwrap();
        let items = proof_items(&c, &Roots::new(dir.path(), dir.path()));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["spellings"], json!(["C:\\Users\\me\\s.png", "C:/Users/me/s.png"]));
        assert_eq!(items[0]["state"], "missing");
    }

    #[test]
    fn a_bold_path_or_a_question_is_no_glob_and_a_named_folder_holds_its_siblings() {
        let f = find_in_text("**C:\\Users\\me\\a.png** and did C:\\Users\\me\\b.md? Not C:\\Users\\me\\M-*.png.", NOWHERE);
        assert_eq!(paths(&f), ["C:\\Users\\me\\a.png", "C:\\Users\\me\\b.md"]);
        let only = |p: &str| p == "C:\\Users\\me\\final\\B-1.png";
        let f = find_in_text("Shots in C:\\Users\\me\\final: B-1.png.", &only);
        assert_eq!(paths(&f), ["C:\\Users\\me\\final\\B-1.png"]);
    }

    #[test]
    fn trailing_punctuation_is_not_part_of_a_path() {
        let f = find_in_text("(C:\\Users\\me\\a.png), C:\\Users\\me\\b.md; C:\\Users\\me\\c.pdf.", NOWHERE);
        assert_eq!(paths(&f), ["C:\\Users\\me\\a.png", "C:\\Users\\me\\b.md", "C:\\Users\\me\\c.pdf"]);
    }

    struct Tree {
        _d: tempfile::TempDir,
        home: PathBuf,
        temp: PathBuf,
    }

    fn tree() -> Tree {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("me");
        let temp = home.join("AppData").join("Local").join("Temp");
        std::fs::create_dir_all(&temp).unwrap();
        Tree { home: std::fs::canonicalize(&home).unwrap(), temp: std::fs::canonicalize(&temp).unwrap(), _d: d }
    }

    /// Writes the file and gives its path as a session would write it (no \\?\ prefix).
    fn put(p: &Path, bytes: &[u8]) -> PathBuf {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
        let real = std::fs::canonicalize(p).unwrap().to_string_lossy().into_owned();
        PathBuf::from(real.strip_prefix(r"\\?\").unwrap_or(&real))
    }

    #[test]
    fn the_read_rule() {
        let t = tree();
        let ok = |p: &Path| check_read(&std::fs::canonicalize(put(p, b"x")).unwrap(), &t.home, &t.temp);
        let data = t.home.join(".claude-kanban-dsh-test");
        assert_eq!(ok(&data.join("api.json")).unwrap_err(), "in the app's data folder");
        assert_eq!(ok(&t.home.join(".claude-kanban-dsh").join("evidence").join("7").join("old.png")).unwrap_err(), "in the app's data folder");
        // Reached another way: a .. (and on Windows another case) resolves to the same file.
        std::fs::create_dir_all(t.home.join("repo")).unwrap();
        let via = t.home.join("repo").join("..").join(".claude-kanban-dsh-test").join("api.json");
        assert_eq!(check_read(&std::fs::canonicalize(&via).unwrap(), &t.home, &t.temp).unwrap_err(), "in the app's data folder");
        #[cfg(windows)]
        {
            let upper = t.home.join(".CLAUDE-KANBAN-DSH-TEST").join("api.json");
            assert_eq!(check_read(&std::fs::canonicalize(&upper).unwrap(), &t.home, &t.temp).unwrap_err(), "in the app's data folder");
            assert_eq!(ok(&t.home.join("AppData").join("Roaming").join("gcloud").join("c.txt")).unwrap_err(), "in a settings folder");
        }
        #[cfg(target_os = "macos")]
        assert_eq!(ok(&t.home.join("Library").join("Keychains").join("k.txt")).unwrap_err(), "in a settings folder");
        assert_eq!(ok(&t.home.join(".claude.json")).unwrap_err(), "in a settings folder");
        assert_eq!(ok(&t.home.join(".ssh").join("k.txt")).unwrap_err(), "in a settings folder");
        let repo = t.home.join("repo");
        assert!(ok(&repo.join("report.md")).is_ok());
        assert!(ok(&repo.join(".claude").join("worktrees").join("card-1").join("shot.png")).is_ok());
        assert_eq!(ok(&repo.join(".claude").join("settings.json")).unwrap_err(), "in a settings folder");
        assert_eq!(ok(&repo.join(".claude").join("worktrees").join("card-1").join(".claude").join("settings.json")).unwrap_err(), "in a settings folder");
        assert_eq!(ok(&repo.join(".claude").join("worktrees").join("card-1").join(".git").join("x.txt")).unwrap_err(), "in a settings folder");
        assert!(ok(&t.temp.join("claude").join("scratch").join("shot.png")).is_ok());
        let outside = tempfile::tempdir().unwrap();
        assert_eq!(ok(&outside.path().join("x.md")).unwrap_err(), "outside your home and temp folders");
    }

    #[test]
    fn items_get_their_states() {
        let t = tree();
        let roots = Roots::new(&t.home, &t.temp);
        let shot = put(&t.temp.join("s").join("after.png"), b"PNG-A");
        let copy = put(&t.home.join(".claude-kanban-dsh").join("evidence").join("c1").join("after.png"), b"PNG-A");
        let old = put(&t.home.join(".claude-kanban-dsh").join("evidence").join("c1").join("old.png"), b"PNG-OLD");
        let report = put(&t.home.join("crit").join("r.md"), b"# Report");
        let api = put(&t.home.join(".claude-kanban-dsh-test").join("api.json"), b"{}");
        let gone = t.home.join("gone.png");
        let mut proof = vec![json!({ "path": shot.to_string_lossy(), "label": "After" }), json!({ "path": api.to_string_lossy() })];
        // The hand-over finder knows drive-letter paths only, so off Windows the last three go in
        // `proof` instead of the text; their states are the same either way.
        let next = if cfg!(windows) {
            format!("Report {} and old {} and gone {}", report.display(), old.display(), gone.display())
        } else {
            proof.extend([&report, &old, &gone].map(|p| json!({ "path": p.to_string_lossy() })));
            String::new()
        };
        let c = card(json!({
            "proof": proof,
            "nextStep": next,
            "evidence": [{ "run": "r2", "shots": [{ "path": copy.to_string_lossy() }] }, { "run": "r1", "shots": [{ "path": old.to_string_lossy() }] }],
        }));
        let items = proof_items(&c, &roots);
        let state: Vec<(&str, &str)> = items.iter().map(|i| (i["name"].as_str().unwrap(), i["state"].as_str().unwrap())).collect();
        assert_eq!(state, [("after.png", "dup"), ("api.json", "refused"), ("r.md", "ok"), ("old.png", "refused"), ("gone.png", "missing")]);
        assert_eq!(items[0]["dupOf"], json!(copy.to_string_lossy()));
        assert_eq!(items[0]["label"], "After");
        assert_eq!(items[1]["why"], "in the app's data folder");
        assert_eq!(items[3]["why"], "in the app's data folder");
        // Only ok items read or open.
        assert!(ok_item(&c, &roots, report.to_str().unwrap()).is_ok());
        assert!(ok_item(&c, &roots, api.to_str().unwrap()).unwrap_err().contains("data folder"));
        assert_eq!(ok_item(&c, &roots, t.home.join("crit").join("other.md").to_str().unwrap()).unwrap_err(), "the card doesn't name this file as proof");
        assert!(ok_item(&c, &roots, "\\\\host\\share\\r.md").is_err());
    }

    #[test]
    fn open_checks_first_and_reaches_the_seam_only_when_allowed() {
        let t = tree();
        let roots = Roots::new(&t.home, &t.temp);
        let report = put(&t.home.join("crit").join("r.md"), b"# R");
        let zip = put(&t.home.join("crit").join("all.zip"), b"PK");
        let api = put(&t.home.join(".claude-kanban-dsh").join("api.json"), b"{}");
        let c = card(json!({ "proof": [{ "path": report.to_string_lossy() }, { "path": zip.to_string_lossy() }, { "path": api.to_string_lossy() }] }));
        let opened = RefCell::new(Vec::new());
        let seam = |p: &str, reveal: bool| {
            opened.borrow_mut().push((p.to_string(), reveal));
            Ok(())
        };
        open_item(&c, &roots, report.to_str().unwrap(), false, &seam).unwrap();
        assert!(open_item(&c, &roots, zip.to_str().unwrap(), false, &seam).is_err());
        open_item(&c, &roots, zip.to_str().unwrap(), true, &seam).unwrap();
        assert!(open_item(&c, &roots, api.to_str().unwrap(), true, &seam).is_err());
        assert!(open_item(&c, &roots, "C:\\Users\\me\\unnamed.md", false, &seam).is_err());
        assert_eq!(opened.borrow().len(), 2);
    }

    fn j(parts: &[&str]) -> String {
        parts.concat()
    }

    #[test]
    fn a_doc_is_masked_and_keeps_its_prose() {
        let ant = j(&["sk-", "ant-", "api03-AbC_dEf"]);
        let goog = j(&["AI", "zaSyD-abc_123"]);
        let hex = "0123456789abcdef".repeat(2);
        let text = format!("# Report\nThe login secret word stays out of logs.\nKEY={ant}\nkey {goog}\ntoken {hex}\nThe end.");
        let (out, cut) = decode_doc(text.as_bytes(), false).unwrap();
        assert!(!cut);
        for k in [&ant, &goog, &hex] {
            assert!(!out.contains(k.as_str()), "{k} showed: {out}");
        }
        assert!(out.contains("# Report") && out.contains("The end."));
        // The board's secret words in prose don't refuse a doc.
        let word = j(&["pass", "word"]);
        let (out, _) = decode_doc(format!("Never put a {word} on the board.").as_bytes(), false).unwrap();
        assert!(out.contains(&word));
    }

    #[test]
    fn a_long_doc_is_cut_at_whitespace_and_never_splits_a_token() {
        let mut bytes = vec![b'a'; DOC_CAP - 20];
        bytes.extend_from_slice(b" ");
        let hex = "0123456789abcdef".repeat(2);
        bytes.extend_from_slice(hex.as_bytes()); // runs across the cap
        let cap = &bytes[..DOC_CAP];
        let (out, cut) = decode_doc(cap, true).unwrap();
        assert!(cut);
        assert_eq!(out.len(), DOC_CAP - 20);
        assert!(!out.contains("0123"));
        // An em dash across the edge.
        let mut b = vec![b'a'; DOC_CAP - 1];
        b[100] = b' ';
        b.extend_from_slice("—".as_bytes());
        let (out, cut) = decode_doc(&b[..DOC_CAP], true).unwrap();
        assert!(cut && out.len() == 100);
        // UTF-16 LE over the cap, cut on an odd byte.
        let mut u = vec![0xFF, 0xFE];
        for c in "word ".repeat(DOC_CAP / 8).encode_utf16() {
            u.extend_from_slice(&c.to_le_bytes());
        }
        let (out, cut) = decode_doc(&u[..DOC_CAP - 1], true).unwrap();
        assert!(cut && out.starts_with("word word") && !out.ends_with(' '));
    }

    #[test]
    fn encodings() {
        let mut le = vec![0xFF, 0xFE];
        for c in "Hello — UTF-16".encode_utf16() {
            le.extend_from_slice(&c.to_le_bytes());
        }
        assert_eq!(decode_doc(&le, false).unwrap().0, "Hello — UTF-16");
        let mut be = vec![0xFE, 0xFF];
        for c in "Big end".encode_utf16() {
            be.extend_from_slice(&c.to_be_bytes());
        }
        assert_eq!(decode_doc(&be, false).unwrap().0, "Big end");
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice("With a BOM".as_bytes());
        assert_eq!(decode_doc(&bom, false).unwrap().0, "With a BOM");
        assert_eq!(decode_doc(&[0x89, b'P', b'N', b'G', 0, 1, 2], false).unwrap_err(), "not readable text");
        assert_eq!(decode_doc(b"text\0more", false).unwrap_err(), "not readable text");
        assert_eq!(decode_doc(&[b'a', 0xC3], false).unwrap_err(), "not readable text");
    }

    #[test]
    fn an_ok_item_reads_the_checked_file_and_the_list_keeps_it_to_itself() {
        let t = tree();
        let roots = Roots::new(&t.home, &t.temp);
        let report = put(&t.home.join("crit").join("r.md"), b"# Checked");
        // On Windows, written with forward slashes and another case: still the same, checked file.
        let written = if cfg!(windows) { report.to_string_lossy().replace('\\', "/").to_uppercase() } else { report.to_string_lossy().into_owned() };
        let c = card(json!({ "proof": [{ "path": written }] }));
        let item = ok_item(&c, &roots, &written).unwrap();
        let real = item["real"].as_str().unwrap();
        assert!(!real.starts_with(r"\\?\") && real.eq_ignore_ascii_case(report.to_str().unwrap()), "{real}");
        assert_eq!(read_item(&item).unwrap()["text"], "# Checked");
        assert!(proof_items(&c, &roots)[0].get("real").is_none());
        assert_eq!(crate::platform::plain_path(Path::new(r"\\?\UNC\host\share\x.md")), r"\\?\UNC\host\share\x.md");
    }

    /// A symlink, when the system lets this user make one (Windows needs Developer Mode).
    fn link(target: &Path, at: &Path) -> bool {
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(target, at);
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(target, at);
        made.is_ok()
    }

    #[test]
    fn a_link_to_another_kind_of_file_is_refused_and_never_opens() {
        let t = tree();
        let roots = Roots::new(&t.home, &t.temp);
        let script = put(&t.home.join("repo").join("run.bat"), b"echo hi");
        let named = t.home.join("repo").join("report.md");
        if !link(&script, &named) {
            eprintln!("no symlinks here: skipped");
            return;
        }
        let c = card(json!({ "proof": [{ "path": named.to_string_lossy() }] }));
        let items = proof_items(&c, &roots);
        assert_eq!(items[0]["state"], "refused");
        assert_eq!(items[0]["why"], "it points at a different kind of file");
        let opened = RefCell::new(0);
        let seam = |_: &str, _: bool| {
            *opened.borrow_mut() += 1;
            Ok(())
        };
        assert!(open_item(&c, &roots, named.to_str().unwrap(), false, &seam).is_err());
        assert!(open_item(&c, &roots, named.to_str().unwrap(), true, &seam).is_err());
        assert_eq!(*opened.borrow(), 0);
    }

    #[test]
    fn read_item_gives_an_image_as_a_data_url_and_refuses_a_pdf() {
        let t = tree();
        let png = put(&t.temp.join("a.png"), b"PNG");
        let r = read_item(&json!({ "path": png.to_string_lossy(), "kind": "image" })).unwrap();
        assert!(r["url"].as_str().unwrap().starts_with("data:image/png;base64,"));
        let md = put(&t.temp.join("r.md"), b"# R");
        assert_eq!(read_item(&json!({ "path": md.to_string_lossy(), "kind": "doc" })).unwrap()["text"], "# R");
        assert!(read_item(&json!({ "path": "x.pdf", "kind": "pdf" })).is_err());
    }
}
