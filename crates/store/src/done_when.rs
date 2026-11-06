//! A card's "Done when..." line and its acceptance checks (card #105). Planning writes them; the
//! session, the card-tester (through the app) and Hugh mark each check met or failed. Both are
//! plain card fields: no migration, and an older build keeps them untouched (its `apply` only
//! touches the keys a patch names). The status on the board is worked out from them in the UI
//! (ui/done-when.js), never stored.

use crate::{Error, Result};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

/// The card field holding the one-line goal.
pub const DONE_WHEN_KEY: &str = "doneWhen";
/// The card field holding the checks: a list of `{text, state, note?, by?, at?}`.
pub const DONE_WHEN_CHECKS_KEY: &str = "doneWhenChecks";
/// The longest the line may be, in characters.
pub const DONE_WHEN_MAX_LEN: usize = 200;
/// The most checks a card takes.
pub const DONE_WHEN_MAX_CHECKS: usize = 10;
/// The longest a check's text may be, in characters.
pub const DONE_WHEN_CHECK_LEN: usize = 160;
/// The longest a check's note may be, in characters.
pub const DONE_WHEN_NOTE_LEN: usize = 300;
/// A check's states.
pub const DONE_WHEN_STATES: [&str; 3] = ["open", "met", "failed"];

/// Marks a check whose state this write changed, until `fill_pending` puts the writer's name in.
/// No writer can send it: `by` is dropped when a check is cleaned.
const PENDING_BY: &str = "\u{1}pending";

/// Sign-in links and Bearer headers, which the board's older filter (secret.rs) doesn't catch.
fn sign_in_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)https?://\S*(sign-?in|log-?in)\S*|\bbearer\s+[A-Za-z0-9._~+/=-]{12,}")
            .expect("sign-in pattern")
    })
}

/// Whether `s` holds a sign-in link or a Bearer header.
pub fn looks_like_sign_in(s: &str) -> bool {
    sign_in_pattern().is_match(s)
}

/// Whitespace runs (new lines too) as single spaces, trimmed.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Cleans `doneWhen`: one line, at most `DONE_WHEN_MAX_LEN` characters, with a leading "Done when"
/// (and the ":", "..." or "-" after it) or a leading "..." dropped. Other leading punctuation is
/// kept (".env is ignored"). `null` or `""` removes it (`None`).
pub fn clean_done_when(v: &Value) -> Result<Option<Value>> {
    static PREFIX: OnceLock<Regex> = OnceLock::new();
    let prefix = PREFIX.get_or_init(|| {
        Regex::new(r"(?i)^(?:done\s+when\b[\s:.,\-\u{2026}]*|(?:\.\.\.|\u{2026})\s*)").expect("prefix")
    });
    let s = match v {
        Value::Null => return Ok(None),
        Value::String(s) => s,
        _ => return Err(Error::Invalid("doneWhen is one line of text, like \"the card shows its checks\"".into())),
    };
    let line = one_line(s);
    let line = prefix.replace(&line, "").trim().to_string();
    if line.is_empty() {
        return Ok(None);
    }
    let n = line.chars().count();
    if n > DONE_WHEN_MAX_LEN {
        return Err(Error::Invalid(format!("doneWhen is one line of at most {DONE_WHEN_MAX_LEN} characters (this one has {n})")));
    }
    if looks_like_sign_in(&line) {
        return Err(Error::Secret(DONE_WHEN_KEY.into()));
    }
    Ok(Some(json!(line)))
}

/// A bullet or number at the start of a pasted line ("- ", "* ", "1. ", "2) ").
fn strip_bullet(s: &str) -> &str {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^\s*(?:[-*\u{2022}]|\d{1,2}[.)])\s+").expect("bullet"));
    match re.find(s) {
        Some(m) => &s[m.end()..],
        None => s,
    }
}

fn bad_shape() -> Error {
    Error::Invalid(
        "doneWhenChecks is a list of checks: strings, or {text, state, note} with state open, met or failed".into(),
    )
}

/// Cleans `doneWhenChecks`. Takes one string (a check per line), or a list of strings and
/// `{text, state, note}` objects (`by` and `at` are accepted and dropped: the store sets them).
/// Text is one line of 1 to `DONE_WHEN_CHECK_LEN` characters, state is open (the default), met or
/// failed, a note is at most `DONE_WHEN_NOTE_LEN`. A repeat (ignoring case) is kept once. Too many,
/// too long, a bad state or an unknown key is refused rather than cut. `null` or an empty list
/// removes the field (`None`).
pub fn clean_done_when_checks(v: &Value) -> Result<Option<Vec<Map<String, Value>>>> {
    let raw: Vec<Value> = match v {
        Value::Null => return Ok(None),
        Value::String(s) => s.lines().map(|l| json!(l)).collect(),
        Value::Array(a) => a.clone(),
        _ => return Err(bad_shape()),
    };
    let mut out: Vec<Map<String, Value>> = Vec::new();
    for (i, item) in raw.iter().enumerate() {
        let n = i + 1;
        let (text, state, note) = match item {
            Value::String(s) => (s.as_str(), None, None),
            Value::Object(o) => {
                if let Some(k) = o.keys().find(|k| !["text", "state", "note", "by", "at"].contains(&k.as_str())) {
                    return Err(Error::Invalid(format!("check {n} takes text, state and note, not \"{k}\"")));
                }
                let text = o.get("text").and_then(Value::as_str).ok_or_else(|| Error::Invalid(format!("check {n} has no text")))?;
                let state = match o.get("state") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(s)) => Some(s.as_str()),
                    Some(_) => return Err(bad_shape()),
                };
                let note = match o.get("note") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(s)) => Some(s.as_str()),
                    Some(_) => return Err(bad_shape()),
                };
                if one_line(text).is_empty() {
                    return Err(Error::Invalid(format!("check {n} has no text")));
                }
                (text, state, note)
            }
            _ => return Err(bad_shape()),
        };
        let text = one_line(strip_bullet(text));
        if text.is_empty() {
            continue; // a blank line in a pasted list
        }
        let len = text.chars().count();
        if len > DONE_WHEN_CHECK_LEN {
            return Err(Error::Invalid(format!("check {n} is longer than {DONE_WHEN_CHECK_LEN} characters ({len})")));
        }
        let state = match state.map(|s| s.trim().to_lowercase()) {
            None => "open".to_string(),
            Some(s) if s.is_empty() => "open".to_string(),
            Some(s) if DONE_WHEN_STATES.contains(&s.as_str()) => s,
            Some(s) => return Err(Error::Invalid(format!("check {n}'s state is open, met or failed, not \"{s}\""))),
        };
        let note = note.map(one_line).filter(|s| !s.is_empty());
        if let Some(nt) = &note {
            let len = nt.chars().count();
            if len > DONE_WHEN_NOTE_LEN {
                return Err(Error::Invalid(format!("check {n}'s note is longer than {DONE_WHEN_NOTE_LEN} characters ({len})")));
            }
            if looks_like_sign_in(nt) {
                return Err(Error::Secret(format!("{DONE_WHEN_CHECKS_KEY}[{i}].note")));
            }
        }
        if looks_like_sign_in(&text) {
            return Err(Error::Secret(format!("{DONE_WHEN_CHECKS_KEY}[{i}].text")));
        }
        if out.iter().any(|c| c["text"].as_str().is_some_and(|t| t.to_lowercase() == text.to_lowercase())) {
            continue;
        }
        let mut c = Map::new();
        c.insert("text".into(), json!(text));
        c.insert("state".into(), json!(state));
        if let Some(nt) = note {
            c.insert("note".into(), json!(nt));
        }
        out.push(c);
    }
    if out.len() > DONE_WHEN_MAX_CHECKS {
        return Err(Error::Invalid(format!("a card takes at most {DONE_WHEN_MAX_CHECKS} checks ({} given)", out.len())));
    }
    Ok(if out.is_empty() { None } else { Some(out) })
}

/// Who set each check's state, and when. A check whose state is unchanged from the old check with
/// the same text (ignoring case) keeps that check's `by` and `at`, wherever it now sits in the list.
/// A changed one gets `at` and a pending `by` that `fill_pending` fills with the writer. An open
/// check has neither.
pub(crate) fn stamp(new: Vec<Map<String, Value>>, old: Option<&Value>, at: &str) -> Value {
    let old: &[Value] = old.and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let list = new
        .into_iter()
        .map(|mut c| {
            let text = c["text"].as_str().unwrap_or("").to_lowercase();
            let state = c["state"].as_str().unwrap_or("open").to_string();
            let before = old
                .iter()
                .find(|o| o.get("text").and_then(Value::as_str).is_some_and(|t| t.to_lowercase() == text));
            match before {
                Some(o) if o.get("state").and_then(Value::as_str) == Some(state.as_str()) => {
                    for k in ["by", "at"] {
                        if let Some(v) = o.get(k) {
                            c.insert(k.into(), v.clone());
                        }
                    }
                }
                _ if state == "open" => {}
                _ => {
                    c.insert("by".into(), json!(PENDING_BY));
                    c.insert("at".into(), json!(at));
                }
            }
            Value::Object(c)
        })
        .collect();
    Value::Array(list)
}

/// Puts the writer's name (and the write's time) on every check `stamp` marked as changed.
pub(crate) fn fill_pending(fields: &mut Map<String, Value>, by: &str, at: &str) {
    if let Some(Value::Array(list)) = fields.get_mut(DONE_WHEN_CHECKS_KEY) {
        for c in list.iter_mut().filter_map(Value::as_object_mut) {
            if c.get("by").and_then(Value::as_str) == Some(PENDING_BY) {
                c.insert("by".into(), json!(by));
                c.insert("at".into(), json!(at));
            }
        }
    }
}
