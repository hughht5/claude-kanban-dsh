//! The messages the app sends to a session on its own (card #153): a column prompt, a notice,
//! the Router's events, a test-app turn, a nudge. Their text is kept, append-only, so the chat
//! can draw each one apart from Hugh's own words, which are never recorded here.

use crate::{now, Result, Store};
use rusqlite::{params, Connection};
use serde::Serialize;

/// How many of a card's (or the Router's) newest app messages the chat matches against.
pub const MATCH_LIMIT: usize = 2000;

/// One part of a user message in a transcript: the app's own text, or Hugh's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MsgPart {
    /// True for text the app sent on its own.
    pub auto: bool,
    pub text: String,
}

pub(crate) fn record(conn: &Connection, card_id: &str, text: &str) -> Result<()> {
    let text = norm(text);
    if text.is_empty() {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO app_messages (card_id, at, text) VALUES (?1, ?2, ?3)",
        params![card_id, now(), text],
    )?;
    Ok(())
}

impl Store {
    /// Records `text` as a message the app sent to `card_id`'s session on its own, for a send
    /// that doesn't go through `queue` (the Router's, a nudge typed at once).
    pub fn note_app_message(&mut self, card_id: &str, text: &str) -> Result<()> {
        record(&self.conn, card_id, text)
    }

    /// For a message that carries Hugh's `words` inside the app's own text (a Fail note, a
    /// message that moved a card back, a bug report): records each blank-line piece of `full`
    /// that holds none of his words, so the chat draws the app's sentences apart from his.
    pub fn note_app_pieces(&mut self, card_id: &str, full: &str, words: &str) -> Result<()> {
        let full = norm(full);
        let words = norm(words);
        let his: Vec<&str> = words.split("\n\n").map(str::trim).filter(|p| !p.is_empty()).collect();
        for piece in full.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
            if !his.iter().any(|h| piece.contains(h)) {
                record(&self.conn, card_id, piece)?;
            }
        }
        Ok(())
    }

    /// The texts of the newest app messages to `card_id` (at most `MATCH_LIMIT`), each once,
    /// longest first (the order `split_app_parts` matches in).
    pub fn app_messages(&self, card_id: &str) -> Result<Vec<String>> {
        let mut st = self.conn.prepare(
            "SELECT text FROM (SELECT text, MAX(seq) AS s FROM app_messages WHERE card_id = ?1 GROUP BY text ORDER BY s DESC LIMIT ?2) ORDER BY length(text) DESC, s DESC",
        )?;
        let rows = st.query_map(params![card_id, MATCH_LIMIT as i64], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}

fn norm(s: &str) -> String {
    s.replace("\r\n", "\n").trim().to_string()
}

/// Splits a user message from a transcript into the app's parts and Hugh's. The app joins
/// messages that wait together with a blank line (`queue`), so a message is pieces separated
/// by blank lines: a piece that is exactly one of `apps` (the texts from `app_messages`) is the
/// app's, and the text between them is Hugh's. Returns `None` when no part is the app's.
pub fn split_app_parts(text: &str, apps: &[String]) -> Option<Vec<MsgPart>> {
    if apps.is_empty() {
        return None;
    }
    let m = norm(text);
    // Longest first, so a text that starts with a shorter app text still matches whole.
    // `Store::app_messages` already gives them so; sorted here only when they aren't.
    let mut cands: Vec<&str> = apps.iter().map(|a| a.as_str()).filter(|a| !a.is_empty()).collect();
    if !cands.windows(2).all(|w| w[0].len() >= w[1].len()) {
        cands.sort_by_key(|a| std::cmp::Reverse(a.len()));
    }
    let app_at = |pos: usize| -> Option<usize> {
        let rest = &m[pos..];
        cands
            .iter()
            .find(|a| rest.starts_with(**a) && (rest.len() == a.len() || rest[a.len()..].starts_with("\n\n")))
            .map(|a| a.len())
    };
    let mut parts: Vec<MsgPart> = Vec::new();
    let mut pos = 0;
    let mut hugh_from: Option<usize> = None;
    let push_hugh = |parts: &mut Vec<MsgPart>, from: usize, to: usize| {
        let t = m[from..to].trim();
        if !t.is_empty() {
            parts.push(MsgPart { auto: false, text: t.to_string() });
        }
    };
    while pos < m.len() {
        if let Some(len) = app_at(pos) {
            if let Some(f) = hugh_from.take() {
                push_hugh(&mut parts, f, pos);
            }
            parts.push(MsgPart { auto: true, text: m[pos..pos + len].to_string() });
            pos += len;
            while m[pos..].starts_with('\n') {
                pos += 1;
            }
            continue;
        }
        hugh_from.get_or_insert(pos);
        // On to the next piece: just after the next blank line.
        match m[pos..].find("\n\n") {
            Some(i) => {
                pos += i;
                while m[pos..].starts_with('\n') {
                    pos += 1;
                }
            }
            None => pos = m.len(),
        }
    }
    if let Some(f) = hugh_from {
        push_hugh(&mut parts, f, m.len());
    }
    if parts.iter().any(|p| p.auto) {
        Some(parts)
    } else {
        None
    }
}
