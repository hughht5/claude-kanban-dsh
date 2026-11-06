//! Card #114: operating-system calls live only in `src-tauri/src/platform/`. This test reads
//! every Rust source file of the workspace (each crate's `src/` and `build.rs`) and fails on a
//! Windows-only call, variable or path anywhere else, so new code can't quietly break the
//! macOS build (card #117). Test code (`#[cfg(test)]` items) and comment lines are skipped.
//!
//! A deliberate case goes in `platform-allow.txt` beside this file, with a reason. An entry
//! that no longer matches anything fails too, so the list stays honest.

use std::path::{Path, PathBuf};

/// Matched as written.
const TOKENS: &[&str] = &[
    "std::os::windows",
    "windows::",
    "Win32",
    "creation_flags",
    "CREATE_NO_WINDOW",
    "USERPROFILE",
    "APPDATA",
    "ProgramFiles",
    ".hwnd(",
    ".exe\"",
    "C:\\",
    // A Windows kernel-object name (a named event or mutex).
    "Local\\",
    // A Windows-only branch: it belongs in platform/windows.rs.
    "cfg(windows)",
    "cfg!(windows)",
    "\"cmd\"",
];

/// Matched in any case.
const TOKENS_ANY_CASE: &[&str] = &["powershell", "wsl"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn rel(p: &Path) -> String {
    p.strip_prefix(root()).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The workspace's source files, outside the platform folder.
fn sources() -> Vec<PathBuf> {
    let root = root();
    let mut crates = vec![root.join("src-tauri")];
    if let Ok(entries) = std::fs::read_dir(root.join("crates")) {
        crates.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
    }
    let mut files = Vec::new();
    for c in crates {
        rs_files(&c.join("src"), &mut files);
        let build = c.join("build.rs");
        if build.is_file() {
            files.push(build);
        }
    }
    let platform = root.join("src-tauri").join("src").join("platform");
    files.retain(|f| !f.starts_with(&platform));
    files.sort();
    files
}

fn is_ident(c: Option<&char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || *c == '_')
}

/// For each line of `text` (from 0), whether it is part of an item marked `#[cfg(test)]`. A
/// small Rust scanner: braces count only outside strings, raw strings, char literals and
/// comments, so a `{` in a test's JSON can't end a test item early or keep it open.
fn test_lines(text: &str) -> Vec<bool> {
    let b: Vec<char> = text.chars().collect();
    let mut test = vec![false; text.matches('\n').count() + 1];
    let (mut i, mut line, mut depth) = (0usize, 0usize, 0i64);
    // The test item being read: its first line, the depth outside it, whether its `{` came.
    let mut item: Option<(usize, i64, bool)> = None;
    let mut ranges = Vec::new();
    let at = |i: usize, s: &str| s.chars().enumerate().all(|(k, c)| b.get(i + k) == Some(&c));
    while i < b.len() {
        let c = b[i];
        if c == '\n' {
            line += 1;
        } else if at(i, "//") {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            continue;
        } else if at(i, "/*") {
            let mut d = 0;
            while i < b.len() {
                if at(i, "/*") {
                    d += 1;
                    i += 2;
                } else if at(i, "*/") {
                    d -= 1;
                    i += 2;
                    if d == 0 {
                        break;
                    }
                } else {
                    line += usize::from(b[i] == '\n');
                    i += 1;
                }
            }
            continue;
        } else if c == '"' {
            i += 1;
            while i < b.len() && b[i] != '"' {
                if b[i] == '\\' {
                    i += 1;
                }
                line += usize::from(b.get(i) == Some(&'\n'));
                i += 1;
            }
        } else if c == 'r' && (!is_ident(i.checked_sub(1).and_then(|p| b.get(p))) || (i >= 1 && b[i - 1] == 'b' && !is_ident(i.checked_sub(2).and_then(|p| b.get(p))))) && matches!(b.get(i + 1), Some('"') | Some('#')) {
            let mut j = i + 1;
            let mut hashes = 0;
            while b.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if b.get(j) == Some(&'"') {
                let close: String = std::iter::once('"').chain(std::iter::repeat('#').take(hashes)).collect();
                j += 1;
                while j < b.len() && !at(j, &close) {
                    line += usize::from(b[j] == '\n');
                    j += 1;
                }
                i = j + close.len();
                continue;
            }
        } else if c == '\'' {
            if b.get(i + 1) == Some(&'\\') {
                // '\n', '\'', '\u{7b}': up to the closing quote.
                let mut j = i + 3;
                while j < b.len() && b[j] != '\'' {
                    j += 1;
                }
                i = j + 1;
                continue;
            }
            if b.get(i + 2) == Some(&'\'') {
                i += 3;
                continue;
            }
            // A lifetime ('a): nothing to skip.
        } else if item.is_none() && at(i, "#[cfg(test)]") {
            item = Some((line, depth, false));
            i += "#[cfg(test)]".len();
            continue;
        } else if c == '{' {
            depth += 1;
            if let Some((s, d0, false)) = item {
                item = Some((s, d0, true));
            }
        } else if c == '}' {
            depth -= 1;
            if let Some((s, d0, true)) = item {
                if depth == d0 {
                    ranges.push((s, line));
                    item = None;
                }
            }
        } else if c == ';' {
            // An item with no braces (`use ...;`) ends at its `;`.
            if let Some((s, _, false)) = item {
                ranges.push((s, line));
                item = None;
            }
        }
        i += 1;
    }
    if let Some((s, ..)) = item {
        ranges.push((s, line));
    }
    for (from, to) in ranges {
        for l in from..=to.min(test.len() - 1) {
            test[l] = true;
        }
    }
    test
}

/// The lines that are code: not comments, and not inside an item marked `#[cfg(test)]`.
fn code_lines(text: &str) -> Vec<(usize, &str)> {
    let test = test_lines(text);
    text.lines()
        .enumerate()
        .filter(|(i, l)| {
            let t = l.trim();
            !test.get(*i).copied().unwrap_or(false) && !t.is_empty() && !t.starts_with("//")
        })
        .map(|(i, l)| (i + 1, l))
        .collect()
}

/// Each `file | token | reason` line of the allow-list.
fn allowed() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("platform-allow.txt")).unwrap();
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let parts: Vec<&str> = l.split('|').map(str::trim).collect();
            assert!(parts.len() == 3 && !parts[2].is_empty(), "platform-allow.txt: \"{l}\" is not \"file | token | reason\"");
            (parts[0].to_string(), parts[1].to_string())
        })
        .collect()
}

/// Every (file, line, token) hit outside the platform folder.
fn hits() -> Vec<(String, usize, String, String)> {
    let mut out = Vec::new();
    for f in sources() {
        let text = std::fs::read_to_string(&f).unwrap();
        for (n, line) in code_lines(&text) {
            let lower = line.to_lowercase();
            let found = TOKENS.iter().filter(|t| line.contains(*t)).chain(TOKENS_ANY_CASE.iter().filter(|t| lower.contains(*t)));
            for t in found {
                out.push((rel(&f), n, t.to_string(), line.trim().to_string()));
            }
        }
    }
    out
}

#[test]
fn windows_only_code_stays_in_the_platform_folder() {
    let allow = allowed();
    let hits = hits();
    let bad: Vec<String> = hits
        .iter()
        .filter(|(f, _, t, _)| !allow.iter().any(|(af, at)| af == f && at == t))
        .map(|(f, n, t, l)| format!("{f}:{n}: \"{t}\" in: {l}"))
        .collect();
    assert!(
        bad.is_empty(),
        "Windows-only code outside src-tauri/src/platform/ (move it into platform/windows.rs behind a function both systems provide, or add it to tests/platform-allow.txt with a reason):\n{}",
        bad.join("\n")
    );
    let stale: Vec<String> = allow
        .iter()
        .filter(|(af, at)| !hits.iter().any(|(f, _, t, _)| f == af && t == at))
        .map(|(f, t)| format!("{f} | {t}"))
        .collect();
    assert!(stale.is_empty(), "platform-allow.txt entries that match nothing any more (remove them):\n{}", stale.join("\n"));
}

/// The scan itself: it finds a token in code, and skips comments and test items.
#[test]
fn the_scan_skips_comments_and_tests_only() {
    let text = r##"fn a() {
    // USERPROFILE in a comment
    let x = "USERPROFILE {";
}
#[cfg(test)]
mod tests {
    fn b() { let y = "}}} C:\\x"; let c = '}'; let e = '\u{7d}'; }
    const J: &str = r#"{"a":{"#;
    /* } a comment brace */
}
fn c<'a>(s: &'a str) -> char { '{' }
#[cfg(test)] mod t { fn d() {} }
#[cfg(test)]
use std::x;
fn e() {}
"##;
    let lines: Vec<usize> = code_lines(text).into_iter().map(|(n, _)| n).collect();
    assert_eq!(lines, vec![1, 3, 4, 11, 15], "braces in strings, raw strings, chars and comments don't count");
    // Every test module of the app's own sources closes where it should: the last code line of
    // transcript.rs (whose tests hold JSON with braces in strings) is before its tests.
    let t = std::fs::read_to_string(root().join("src-tauri/src/transcript.rs")).unwrap();
    let tests_at = t.lines().position(|l| l.trim() == "#[cfg(test)]").unwrap() + 1;
    let after: Vec<usize> = code_lines(&t).into_iter().map(|(n, _)| n).filter(|n| *n > tests_at).collect();
    assert!(after.is_empty(), "transcript.rs's tests run to the end of the file, so nothing after them is code: {after:?}");
    assert!(sources().iter().any(|f| rel(f) == "src-tauri/src/sessions.rs"), "the scan reads the app's sources");
    assert!(!sources().iter().any(|f| rel(f).starts_with("src-tauri/src/platform/")), "the platform folder is left out");
    assert!(sources().iter().any(|f| rel(f) == "crates/store/src/channel.rs"), "the scan reads every crate");
}
