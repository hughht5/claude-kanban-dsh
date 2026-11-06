//! Card #105 in the app: the card-tester reports one `CHECK n: PASS|FAIL` line per acceptance
//! check (`doneWhenChecks`; its prompt in tester.rs asks for them), and `agents::record_check`
//! marks the checks on the card its report names. A PASS that fails one of them counts as a FAIL.

use kanban_store::{mask_secrets as mask, Card, DONE_WHEN_CHECKS_KEY, DONE_WHEN_NOTE_LEN};
use serde_json::{json, Map, Value};

/// One `CHECK n: PASS|FAIL` line from a tester's report.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckResult {
    /// The check's number, from 1, in the card's list order.
    pub n: usize,
    pub met: bool,
    /// What the tester wrote after the verdict, if anything.
    pub note: Option<String>,
}

fn is_mark(c: char) -> bool {
    c == '*' || c == '`' || c == '_'
}

/// One line, if it is a CHECK line: "CHECK 2: FAIL - it wraps", with markdown bold, backticks,
/// a list dash, "#2" or "2." allowed, and PASSED / FAILED for PASS / FAIL.
fn check_line(raw: &str) -> Option<CheckResult> {
    let l = raw.trim_start_matches(|c: char| c.is_whitespace() || is_mark(c) || c == '-' || c == '>');
    if !l.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("CHECK")) {
        return None;
    }
    let rest = l[5..].trim_start_matches(|c: char| c.is_whitespace() || is_mark(c) || c == '#');
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let n: usize = digits.parse().ok().filter(|n| *n > 0)?;
    let rest = rest[digits.len()..].trim_start_matches(|c: char| c.is_whitespace() || is_mark(c) || matches!(c, ':' | '.' | ')' | '-'));
    let word: String = rest.chars().take_while(char::is_ascii_alphabetic).collect();
    let met = match word.to_ascii_uppercase().as_str() {
        "PASS" | "PASSED" => true,
        "FAIL" | "FAILED" => false,
        _ => return None,
    };
    let note = rest[word.len()..]
        .trim_start_matches(|c: char| c.is_whitespace() || is_mark(c) || matches!(c, ':' | '-' | '\u{2013}' | '\u{2014}' | '.' | ','))
        .trim_end_matches(|c: char| c.is_whitespace() || is_mark(c))
        .trim();
    Some(CheckResult { n, met, note: (!note.is_empty()).then(|| note.to_string()) })
}

/// Every CHECK line in a tester's report, the last one for each number winning, in number order.
pub fn check_lines(text: &str) -> Vec<CheckResult> {
    let mut out: Vec<CheckResult> = Vec::new();
    for r in text.lines().filter_map(check_line) {
        out.retain(|x| x.n != r.n);
        out.push(r);
    }
    out.sort_by_key(|r| r.n);
    out
}

/// The FAIL lines for the card's own checks (numbers past the end of its list don't count), for a
/// tester that said PASS while failing one: "check 2 failed: it wraps", masked.
pub fn failed_checks(card: &Card, results: &[CheckResult]) -> Vec<String> {
    let total = card.fields.get(DONE_WHEN_CHECKS_KEY).and_then(Value::as_array).map_or(0, Vec::len);
    results
        .iter()
        .filter(|r| !r.met && r.n <= total)
        .map(|r| match &r.note {
            Some(n) => format!("check {} failed: {}", r.n, mask(n)),
            None => format!("check {} failed", r.n),
        })
        .collect()
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(n).collect::<String>().trim_end())
    }
}

/// The card's checks with the tester's results applied, and the log line naming each one, for a
/// verdict recorded as `state`. Only a real verdict ("passed" or "failed") marks checks: an unclear
/// one, a card with no checks, or a report with no usable CHECK line gives `None`. A number past
/// the end of the list is ignored. A note is masked and cut to fit, so it can't make the write
/// fail; a pass clears an older note.
pub fn checks_patch(card: &Card, state: &str, results: &[CheckResult]) -> Option<(Value, String)> {
    if state != "passed" && state != "failed" {
        return None;
    }
    let list = card.fields.get(DONE_WHEN_CHECKS_KEY)?.as_array()?;
    let mut out: Vec<Value> = Vec::with_capacity(list.len());
    let mut said: Vec<String> = Vec::new();
    for (i, c) in list.iter().enumerate() {
        let Some(obj) = c.as_object() else { return None };
        let mut next = Map::new();
        for k in ["text", "state", "note"] {
            if let Some(v) = obj.get(k) {
                next.insert(k.into(), v.clone());
            }
        }
        if let Some(r) = results.iter().find(|r| r.n == i + 1) {
            next.insert("state".into(), json!(if r.met { "met" } else { "failed" }));
            match r.note.as_deref().map(mask) {
                Some(nt) => next.insert("note".into(), json!(short(&nt, DONE_WHEN_NOTE_LEN - 3))),
                None => next.remove("note"),
            };
            let text = obj.get("text").and_then(Value::as_str).unwrap_or("");
            said.push(format!("{} {} ({})", i + 1, if r.met { "met" } else { "failed" }, short(text, 50)));
        }
        out.push(Value::Object(next));
    }
    if said.is_empty() {
        return None;
    }
    Some((Value::Array(out), format!("Subagent test marked the checks: {}", said.join("; "))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(checks: Value) -> Card {
        let mut f = Map::new();
        f.insert("title".into(), json!("T"));
        if !checks.is_null() {
            f.insert(DONE_WHEN_CHECKS_KEY.into(), checks);
        }
        Card { id: "c7".into(), number: 7, column: "In progress".into(), order: None, order_col: None, archived: false, fields: f, version: 1 }
    }

    #[test]
    fn check_lines_are_read_from_the_tester_report() {
        let report = "Deliverable 1: PASS\n\
                      CHECK 1: PASS\n\
                      **CHECK 2:** FAIL - the line wraps on a narrow card\n\
                      - `CHECK #3` passed\n\
                      CHECK 4 - FAILED: no tick box\n\
                      checking 5: PASS\n\
                      CHECK 0: PASS\n\
                      CHECK 2: FAIL \u{2014} it still wraps **\n\
                      CARD: #7\nVERDICT: FAIL\n- check 2";
        let r = check_lines(report);
        assert_eq!(
            r,
            vec![
                CheckResult { n: 1, met: true, note: None },
                CheckResult { n: 2, met: false, note: Some("it still wraps".into()) },
                CheckResult { n: 3, met: true, note: None },
                CheckResult { n: 4, met: false, note: Some("no tick box".into()) },
            ]
        );
        assert!(check_lines("CARD: #7\nVERDICT: PASS").is_empty());
    }

    #[test]
    fn tester_results_mark_the_checks() {
        let c = card(json!([
            { "text": "the line shows", "state": "open" },
            { "text": "a tick is kept", "state": "failed", "note": "old note", "by": "Hugh (board)", "at": "t" },
            { "text": "untouched", "state": "met", "by": "Hugh (board)", "at": "t" }
        ]));
        let results = check_lines("CHECK 1: FAIL - it shows the password field and http://x/dev-sign-in.html?as=admin\nCHECK 2: PASS\nCHECK 9: PASS");
        let (list, line) = checks_patch(&c, "failed", &results).unwrap();
        assert_eq!(list[0]["state"], json!("failed"));
        let note = list[0]["note"].as_str().unwrap();
        assert!(!kanban_store::looks_secret(note) && !kanban_store::looks_like_sign_in(note), "{note}");
        assert_eq!(list[1], json!({ "text": "a tick is kept", "state": "met" }), "a pass clears the old note; the store stamps who");
        assert_eq!(list[2], json!({ "text": "untouched", "state": "met" }), "by and at are left to the store, which keeps them");
        assert_eq!(list.as_array().unwrap().len(), 3, "CHECK 9 is past the end and ignored");
        assert_eq!(line, "Subagent test marked the checks: 1 failed (the line shows); 2 met (a tick is kept)");
        // A long note is cut to fit the store's limit.
        let long = vec![CheckResult { n: 1, met: false, note: Some("x".repeat(400)) }];
        let (list, _) = checks_patch(&c, "passed", &long).unwrap();
        assert!(list[0]["note"].as_str().unwrap().chars().count() <= DONE_WHEN_NOTE_LEN);
    }

    #[test]
    fn an_unclear_verdict_marks_no_checks() {
        let c = card(json!([{ "text": "a", "state": "open" }]));
        let r = check_lines("CHECK 1: PASS");
        assert!(checks_patch(&c, "unclear", &r).is_none());
        assert!(checks_patch(&c, "needed", &r).is_none());
        assert!(checks_patch(&c, "passed", &[]).is_none(), "no CHECK lines");
        assert!(checks_patch(&c, "passed", &check_lines("CHECK 2: PASS")).is_none(), "only out-of-range lines");
        assert!(checks_patch(&card(Value::Null), "passed", &r).is_none(), "a card with no checks");
        assert!(checks_patch(&c, "passed", &r).is_some());
    }

    #[test]
    fn a_failed_check_is_named_for_the_verdict() {
        let c = card(json!([{ "text": "a", "state": "open" }, { "text": "b", "state": "open" }]));
        let r = check_lines("CHECK 1: PASS
CHECK 2: FAIL - the password box shows
CHECK 5: FAIL - not a check");
        let f = failed_checks(&c, &r);
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(f[0].starts_with("check 2 failed: ") && !kanban_store::looks_secret(&f[0]), "{f:?}");
        assert!(failed_checks(&c, &check_lines("CHECK 1: PASS
CHECK 2: PASS")).is_empty());
        assert!(failed_checks(&card(Value::Null), &r).is_empty(), "a card with no checks");
    }

    #[test]
    fn the_agents_ask_about_the_checks() {
        // Card #105: the tester's, reviewer's and critic's prompts (fixed when the session starts)
        // say what to do with a card's checks, for every card.
        let v: Value = serde_json::from_str(&crate::agents::agents_json()).unwrap();
        for a in ["card-tester", "card-reviewer", "card-critic"] {
            let p = v[a]["prompt"].as_str().unwrap();
            assert!(p.contains("doneWhenChecks"), "{a}");
            assert!(!kanban_store::looks_like_token(p), "{a}");
        }
        let t = v["card-tester"]["prompt"].as_str().unwrap();
        assert!(t.contains("CHECK <n>: PASS") && t.contains("Before the CARD line"), "{t}");
    }
}
