//! `/btw` from the chat (card #94). Claude Code answers a side question in an overlay on its
//! screen and never writes it to the transcript, so the app types `/btw <question>` into the
//! session, reads the answer off the terminal screen (`Sessions::screen_text`), scrolls the
//! overlay if the answer is longer than it, closes it with Esc, and keeps the answer for the chat
//! (`Sessions::btws`). Only in memory, like the terminal's: an app restart forgets them.
//!
//! The overlay, as Claude Code 2.1.318 draws it (inside a box under the conversation):
//!
//! ```text
//!     /btw what is 17 times 3?
//!
//!       ✽ Answering…            (while it works)
//!       17 × 3 = 51.            (once answered)
//!
//!     ↑/↓ to scroll · c to copy · f to fork · Esc to close   ("Esc to close" alone while answering)
//! ```

use serde::Serialize;

/// One side question and its answer, for the chat.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Btw {
    /// When it was asked (RFC 3339), so the chat places it among the messages.
    pub at: String,
    pub question: String,
    /// "asking", "answered" or "failed".
    pub state: &'static str,
    /// The answer, once "answered"; what went wrong, once "failed".
    pub text: String,
}

/// The overlay as it shows now.
#[derive(Debug, PartialEq)]
pub struct View {
    /// The answer's lines in view, indent removed.
    pub lines: Vec<String>,
    /// Still answering ("Answering…").
    pub answering: bool,
    /// Finished: its footer offers "c to copy" (with "↑/↓ to scroll"), which it shows only once
    /// the answer is complete; while answering it says just "Esc to close".
    pub done: bool,
}

fn indent(s: &str) -> usize {
    s.len() - s.trim_start().len()
}

/// Finds the `/btw` overlay on `screen` (one line per row): its footer is the last line with
/// "Esc to close", its question the last `/btw` line above that. None when it isn't showing.
pub fn view(screen: &str) -> Option<View> {
    let lines: Vec<&str> = screen.lines().collect();
    let foot = lines.iter().rposition(|l| l.contains("Esc to close"))?;
    let q = lines[..foot].iter().rposition(|l| l.trim_start().starts_with("/btw"))?;
    let qi = indent(lines[q]);
    let mut body = &lines[q + 1..foot];
    // A long question wraps onto lines at its own indent, before a blank line or the answer.
    while let Some(first) = body.first() {
        if !first.trim().is_empty() && indent(first) <= qi {
            body = &body[1..];
        } else {
            break;
        }
    }
    let answering = body.iter().any(|l| l.trim_end().ends_with("Answering…"));
    let cut = body.iter().filter(|l| !l.trim().is_empty()).map(|l| indent(l)).min().unwrap_or(0);
    let mut out: Vec<String> = body.iter().map(|l| l.get(cut..).unwrap_or("").trim_end().to_string()).collect();
    while out.first().is_some_and(|l| l.is_empty()) {
        out.remove(0);
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    Some(View { lines: out, answering, done: lines[foot].contains("to copy") })
}

/// What `merge` did.
#[derive(Debug, PartialEq)]
pub enum Merged {
    /// Nothing new: the overlay is at its end.
    Nothing,
    /// New lines, joined onto the end of what came before.
    Added,
    /// New lines, but none of the window matched the end of what came before: the scroll went
    /// further than the overlay is tall, so lines may be missing between them.
    Gap,
}

/// Adds the part of `window` (the overlay after scrolling down) that `got` doesn't end with yet.
pub fn merge(got: &mut Vec<String>, window: &[String]) -> Merged {
    // The largest overlap: `got` ends with the first `k` lines of `window`.
    let most = got.len().min(window.len());
    let k = (0..=most).rev().find(|&k| got[got.len() - k..] == window[..k]).unwrap_or(0);
    if k == window.len() {
        return Merged::Nothing;
    }
    let gap = k == 0 && !got.is_empty();
    got.extend_from_slice(&window[k..]);
    if gap {
        Merged::Gap
    } else {
        Merged::Added
    }
}

/// Joins the lines the terminal wrapped back into paragraphs. A line was wrapped when the next
/// line's first word wouldn't have fitted on it; the width is taken as the longest line's. A
/// list item, an indented line or a blank line always starts afresh. Short answers (no line of
/// 60 characters or more) never wrapped.
pub fn unwrap(lines: &[String]) -> Vec<String> {
    let len = |l: &str| l.chars().count();
    let width = lines.iter().map(|l| len(l)).max().unwrap_or(0);
    if width < 60 {
        return lines.to_vec();
    }
    let fresh = |l: &str| {
        let t = l.trim_start();
        t.is_empty()
            || l.starts_with(char::is_whitespace)
            || ["- ", "* ", "• ", "> ", "#", "|", "```"].iter().any(|m| t.starts_with(m))
            || t.split_once(". ").is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    };
    let mut out: Vec<String> = Vec::new();
    let mut last_len = 0;
    for l in lines {
        let first = l.split_whitespace().next().map_or(0, len);
        // Wrapped onto this line: the line before reached near the full width, and this line's
        // first word wouldn't have fitted after it.
        let wrapped = last_len + 12 >= width && last_len + 1 + first > width - 2;
        match out.last_mut() {
            Some(prev) if wrapped && !prev.is_empty() && !fresh(l) => {
                prev.push(' ');
                prev.push_str(l.trim());
            }
            _ => out.push(l.clone()),
        }
        last_len = len(l);
    }
    out
}

/// The question in `text` if it is a `/btw` command ("/btw what next?"), trimmed. Some("") for
/// a bare `/btw`.
pub fn question(text: &str) -> Option<&str> {
    let t = text.trim_start();
    let rest = t.strip_prefix("/btw")?;
    if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
        return None;
    }
    Some(rest.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_wrapped_lines_back_up() {
        let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let wrapped = l(&[
            "Last time I gave you the dry-run plan for card #152 and asked what slash commands should cover, since the card has no brief and",
            "your restart message arrived with /exit joined onto it, and it went on long enough, with more besides, to wrap onto",
            "considerably-long-hyphenated-words on a third line too.",
            "",
            "- a list item that is short",
            "- another",
            "That's all.",
        ]);
        let got = unwrap(&wrapped);
        assert!(got[0].starts_with("Last time") && got[0].ends_with("onto considerably-long-hyphenated-words on a third line too."), "{got:?}");
        assert_eq!(&got[1..], &l(&["", "- a list item that is short", "- another", "That's all."])[..]);
        // A short answer is left as it is.
        assert_eq!(unwrap(&l(&["17 × 3 = 51.", "Done."])), l(&["17 × 3 = 51.", "Done."]));
        // A long line followed by a line whose first word would have fitted wasn't wrapped.
        let kept = l(&[&"x".repeat(70), "y", &"z".repeat(80)]);
        assert_eq!(unwrap(&kept), kept);
    }

    fn screen(rows: &[&str]) -> String {
        rows.join("\n")
    }

    #[test]
    fn reads_the_answer_once_its_there() {
        let asking = screen(&["● Earlier reply", "▔▔▔▔", "    /btw what is 17 times 3?", "", "      ✽ Answering…", "", "    Esc to close"]);
        let v = view(&asking).unwrap();
        assert!(v.answering && !v.done);
        let done = screen(&["● Earlier reply", "▔▔▔▔", "    /btw what is 17 times 3?", "", "      17 × 3 = 51.", "", "      That's all.", "", "    ↑/↓ to scroll · c to copy · f to fork · Esc to close"]);
        let v = view(&done).unwrap();
        assert_eq!(v, View { lines: vec!["17 × 3 = 51.".into(), "".into(), "That's all.".into()], answering: false, done: true });
    }

    #[test]
    fn keeps_the_answers_own_indent_and_skips_a_wrapped_question() {
        let s = screen(&["    /btw list the files and say what", "    each one does", "", "      Files:", "        - a.rs", "", "    Esc to close"]);
        assert_eq!(view(&s).unwrap().lines, vec!["Files:", "  - a.rs"]);
    }

    #[test]
    fn no_overlay_no_view() {
        assert_eq!(view(&screen(&["❯ /btw typed but not sent", "──", "  #152 · Opus"])), None);
        // An old /btw line in the conversation with no footer below it isn't the overlay.
        assert_eq!(view(&screen(&["Esc to close", "    /btw old"])), None);
    }

    #[test]
    fn merges_scrolled_windows() {
        let mut got: Vec<String> = ["1", "2", "3", "4"].map(String::from).to_vec();
        assert_eq!(merge(&mut got, &["3", "4", "5", "6"].map(String::from)), Merged::Added);
        assert_eq!(got, ["1", "2", "3", "4", "5", "6"]);
        // The same window again: at the end.
        assert_eq!(merge(&mut got, &["3", "4", "5", "6"].map(String::from)), Merged::Nothing);
        // A window with no overlap (scrolled further than the overlay is tall) is added whole.
        assert_eq!(merge(&mut got, &["9", "10"].map(String::from)), Merged::Gap);
        assert_eq!(got.len(), 8);
    }

    #[test]
    fn spots_a_btw_command() {
        assert_eq!(question("/btw what next?"), Some("what next?"));
        assert_eq!(question("  /btw\n multi\nline "), Some("multi\nline"));
        assert_eq!(question("/btw"), Some(""));
        assert_eq!(question("/btwx hi"), None);
        assert_eq!(question("hello /btw"), None);
    }
}
