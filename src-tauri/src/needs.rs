//! Cards that need Hugh, and each account's 7-day usage for the board's header.
//!
//! A card needs Hugh when its live session waits at a permission prompt, when its session
//! failed (a StopFailure, or it exited without the app stopping it) since he last opened the
//! card, or when its session ended a turn he hasn't opened the card since. Read and reply
//! marks are kept in the store (migration 5); opening a card's panel records a read.
//!
//! Each need can say what the card asks him (`Ask`, card #107): the question on screen, the
//! command a permission prompt asks to run, why it failed, or the question an unread reply
//! ends with. That text comes from memory only (the sessions' `Waits`, and a cache of each
//! card's last reply that workers fill from the transcript), so `needs_you` never reads a
//! file, and every word of it passes `redact` first.

use crate::sessions::{Sessions, Waiting};
use crate::transcript::{self, ReplyParts, ASK_MAX};
use crate::Board;
use kanban_store::{looks_secret, Store};
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

/// Why a card needs Hugh, most pressing first: `permission`, `failed`, `unread`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Need {
    pub reason: &'static str,
    /// For `failed`: "crashed" (it exited on its own) or "turn failed" (StopFailure).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<&'static str>,
    /// When it started to need him, for the order among pinned cards (newest first).
    pub at: String,
    /// Also has a reply he hasn't read (shown in the tooltip when the reason is another).
    pub unread: bool,
    /// What the card asks him, when that is known. An unread reply that only tells him
    /// something has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ask: Option<Ask>,
}

/// What a card asks Hugh, for its line on the board. Every string passed `redact` and is at
/// most `ASK_MAX` characters.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Ask {
    /// "question" (a question dialog), "permission" (a permission prompt), "failed",
    /// "reply" (an unread reply that asks a question) or "plan" (a plan ready, in Planning).
    pub kind: &'static str,
    pub text: String,
    /// A question dialog's option labels, for its first question.
    pub options: Vec<String>,
    /// How many more questions the dialog asks after the first.
    pub more: usize,
}

/// `s` as the board may show it: secrets hidden (`redact`), then cut to `ASK_MAX`.
pub(crate) fn shown(s: &str) -> String {
    let r = redact(s);
    if r == HIDDEN_ALL {
        return r;
    }
    transcript::cut_words(&r, ASK_MAX)
}

fn ask(kind: &'static str, text: &str, options: Vec<String>, more: usize) -> Ask {
    Ask { kind, text: shown(text), options: options.iter().map(|o| shown(o)).collect(), more }
}

/// A question dialog's first question, its option labels and how many questions follow.
pub(crate) fn question_ask(questions: &Value) -> Option<Ask> {
    let qs = questions.as_array()?;
    let first = qs.first()?;
    let s = |k: &str| first.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
    let text = s("question").or(s("header"))?;
    let options = first
        .get("options")
        .and_then(Value::as_array)
        .map(|o| o.iter().filter_map(|o| o.get("label").and_then(Value::as_str)).filter(|l| !l.trim().is_empty()).map(str::to_string).collect())
        .unwrap_or_default();
    Some(ask("question", text, options, qs.len() - 1))
}

/// A card's last reply as the board keeps it: the reply mark (`Store::mark_card`) it is for,
/// and its parts. `parts` None: no reply was found for that mark, so it isn't looked for again.
#[derive(Debug, Clone, PartialEq)]
pub struct Cached {
    pub mark: String,
    pub parts: Option<ReplyParts>,
}

/// Each card's last reply (`Cached`), in memory only.
fn replies() -> &'static Mutex<HashMap<String, Cached>> {
    static R: OnceLock<Mutex<HashMap<String, Cached>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

/// Every card that needs Hugh, from the live sessions' states (`Sessions::states`) and waits
/// (`Sessions::waits`), the cached replies and the store. Reads no file. Drops the cached
/// replies of cards that are archived or gone.
pub fn compute(
    store: &Store,
    states: &HashMap<String, String>,
    waits: &HashMap<String, Waiting>,
    cached: &mut HashMap<String, Cached>,
) -> kanban_store::Result<BTreeMap<String, Need>> {
    let marks = store.card_marks()?;
    let latest: HashMap<String, kanban_store::LatestSession> =
        store.latest_sessions()?.into_iter().map(|l| (l.card_id.clone(), l)).collect();
    let none = Waiting::default();
    let mut out = BTreeMap::new();
    let cards = store.list(false)?;
    let ids: std::collections::HashSet<&str> = cards.iter().map(|c| c.id.as_str()).collect();
    cached.retain(|id, _| ids.contains(id.as_str()));
    for card in &cards {
        let m = marks.get(&card.id).cloned().unwrap_or_default();
        let unread = m.unread();
        let s = latest.get(&card.id);
        let live = states.get(&card.id).map(String::as_str);
        let w = waits.get(&card.id).unwrap_or(&none);
        // When the recorded state became `want`, if the store's latest state is that one.
        let state_at = |want: &str| {
            s.filter(|l| l.state.as_deref() == Some(want)).and_then(|l| l.state_at.clone())
        };
        let failed = |what: &str, error: Option<&str>| {
            let text = match error {
                Some(e) => format!("{what} ({e})"),
                None => what.to_string(),
            };
            Some(ask("failed", &text, vec![], 0))
        };
        let need = if live == Some("permission") {
            let a = match (&w.questions, &w.prompt) {
                (Some(q), _) => question_ask(q),
                (None, Some(p)) => Some(ask("permission", p, vec![], 0)),
                _ => None,
            };
            Some(Need { reason: "permission", detail: None, at: state_at("permission").unwrap_or_else(now), unread, ask: a })
        } else if live == Some("failed") && state_at("failed").map_or(true, |at| m.after_read(&at)) {
            let a = failed("its turn failed", w.failure.as_deref());
            Some(Need { reason: "failed", detail: Some("turn failed"), at: state_at("failed").unwrap_or_else(now), unread, ask: a })
        } else if let Some(at) = s
            .filter(|l| live.is_none() && l.status.as_deref() == Some("exited"))
            .and_then(|l| l.status_at.clone())
            .filter(|at| m.after_read(at))
        {
            Some(Need { reason: "failed", detail: Some("crashed"), at, unread, ask: failed("its session crashed", None) })
        } else if unread {
            // Only the reply kept for this very mark: an older one is another turn's.
            let a = cached
                .get(&card.id)
                .filter(|c| m.reply.as_deref() == Some(c.mark.as_str()))
                .and_then(|c| c.parts.as_ref())
                .and_then(|p| transcript::ask_of(p, card.column == "Planning"))
                .map(|(kind, text)| ask(kind, &text, vec![], 0));
            Some(Need { reason: "unread", detail: None, at: m.reply.clone().unwrap_or_default(), unread, ask: a })
        } else {
            None
        };
        if let Some(n) = need {
            out.insert(card.id.clone(), n);
        }
    }
    Ok(out)
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// What `redact` gives for a text that still looks secret once its parts are hidden.
pub const HIDDEN_ALL: &str = "details hidden";

/// `text` with anything that could be a secret hidden, for the board:
/// - a URL loses its userinfo ("user:secret@"), its query and fragment, and its path after the
///   first segment ("https://host/seg/…", which hides a token in a magic link's path);
/// - an `Authorization` header's value, a Bearer value, and the value of a `key: value`,
///   `key=value` or JSON pair whose key holds token, secret, key, password, auth or
///   credential (also `oobCode=`, `ct=`, `sig=`) become "[hidden]";
/// - so do known token shapes (GitHub, GitLab, Slack, AWS, Google, Anthropic, JWTs), long
///   opaque strings (32 or more letters, digits, "_" or "-", with letters and digits mixed)
///   and every word the store's `looks_secret` matches.
/// If the result still looks secret, it is "details hidden".
pub fn redact(text: &str) -> String {
    struct Res {
        url: Regex,
        authorization: Regex,
        bearer: Regex,
        named: Regex,
        pair: Regex,
        token: Regex,
        opaque: Regex,
        word: Regex,
    }
    static RES: OnceLock<Res> = OnceLock::new();
    let r = RES.get_or_init(|| Res {
        url: Regex::new(
            r#"(?i)\b(https?://)(?:[^\s/?#<>"'()\[\]@]+@)?([^\s/?#<>"'()\[\]@]+)(/[^\s/?#<>"'()\[\]]*)?(/[^\s?#<>"'()\[\]]*)?([?#][^\s<>"'()\[\]]*)?"#,
        )
        .expect("url"),
        authorization: Regex::new(r#"(?i)\b(authorization)(["']?\s*[:=]\s*["']?)(?:[A-Za-z]+\s+)?[^\s"',;&<>}]+"#).expect("authorization"),
        bearer: Regex::new(r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]+").expect("bearer"),
        named: Regex::new(r#"(?i)(["']?)\b([A-Za-z0-9_.-]*(?:token|secret|key|password|passwd|auth|credential)[A-Za-z0-9_.-]*)(["']?)(\s*[:=]\s*)(["']?)[^\s"',;&<>}]+"#)
            .expect("named"),
        pair: Regex::new(r#"(?i)\b([A-Za-z0-9_-]*(?:oobcode|ct|sig|signature))=[^\s&"'<>]+"#).expect("pair"),
        token: Regex::new(
            r"\b(?:gh[pousr]_[A-Za-z0-9]{10,}|github_pat_[A-Za-z0-9_]{10,}|glpat-[A-Za-z0-9_-]{10,}|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{10,}|sk-ant-[A-Za-z0-9_-]+|xox[abprs]-[A-Za-z0-9-]+|eyJ[A-Za-z0-9_-]{6,}(?:\.[A-Za-z0-9_-]*)*)",
        )
        .expect("token"),
        opaque: Regex::new(r"[A-Za-z0-9_-]{32,}").expect("opaque"),
        word: Regex::new(r"\S+").expect("word"),
    });
    let t = r.url.replace_all(text, |c: &regex::Captures| {
        let get = |i: usize| c.get(i).map_or("", |m| m.as_str());
        let rest = if get(4).len() > 1 { "/…" } else { get(4) };
        let query = if c.get(5).is_some() { "?…" } else { "" };
        format!("{}{}{}{rest}{query}", get(1), get(2), get(3))
    });
    let t = r.authorization.replace_all(&t, "$1$2[hidden]");
    let t = r.bearer.replace_all(&t, "Bearer [hidden]");
    let t = r.named.replace_all(&t, "$1$2$3$4$5[hidden]");
    let t = r.pair.replace_all(&t, "$1=[hidden]");
    let t = r.token.replace_all(&t, "[hidden]");
    let t = r.opaque.replace_all(&t, |c: &regex::Captures| {
        let s = &c[0];
        if s.bytes().any(|b| b.is_ascii_alphabetic()) && s.bytes().any(|b| b.is_ascii_digit()) {
            "[hidden]".to_string()
        } else {
            s.to_string()
        }
    });
    let t = r.word.replace_all(&t, |c: &regex::Captures| if looks_secret(&c[0]) { "[hidden]".to_string() } else { c[0].to_string() });
    if looks_secret(&t) {
        return HIDDEN_ALL.to_string();
    }
    t.into_owned()
}

/// The cards that need Hugh now. Takes the sessions' lock and the board's in turn, never both,
/// then the reply cache's under the board's (board first, never the other way), and reads no
/// file: an unread reply with no text kept for its mark (after a restart) is looked for by a
/// worker (`fill_replies`) once both are released, and the board is told when it is found.
pub fn current(app: &AppHandle) -> BTreeMap<String, Need> {
    let sessions = app.state::<Arc<Sessions>>();
    let states = sessions.states();
    let waits = sessions.waits();
    let (needs, wanted) = {
        let board = app.state::<Board>();
        let Ok(store) = board.0.lock() else { return BTreeMap::new() };
        let Ok(mut cached) = replies().lock() else { return BTreeMap::new() };
        let needs = compute(&store, &states, &waits, &mut cached).unwrap_or_default();
        let wanted: Vec<(String, String)> = needs
            .iter()
            .filter(|(id, n)| n.reason == "unread" && !n.at.is_empty() && cached.get(*id).map_or(true, |c| c.mark != n.at))
            .map(|(id, n)| (id.clone(), n.at.clone()))
            .collect();
        (needs, wanted)
    };
    if !wanted.is_empty() {
        fill_replies(app, wanted);
    }
    needs
}

/// Looks for each card's reply in its latest transcript (the last 256 KB), on one worker
/// thread holding no lock while it reads. Each (card, mark) is looked for once: a reply not
/// found is kept as not found.
fn fill_replies(app: &AppHandle, wanted: Vec<(String, String)>) {
    let todo: Vec<(String, String)> = {
        let Ok(mut c) = replies().lock() else { return };
        wanted
            .into_iter()
            .filter(|(card, mark)| {
                if c.get(card).is_some_and(|x| &x.mark == mark) {
                    return false;
                }
                c.insert(card.clone(), Cached { mark: mark.clone(), parts: None });
                true
            })
            .collect()
    };
    if todo.is_empty() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        for (card, mark) in todo {
            let row = {
                let board = app.state::<Board>();
                let Ok(store) = board.0.lock() else { return };
                store.latest_session(&card).ok().flatten()
            };
            let parts = row
                .and_then(|r| transcript::find(&r.account, &r.claude_session_id))
                .and_then(|p| transcript::last_reply_in(&p))
                .map(|t| transcript::reply_parts(&t));
            if parts.is_none() {
                continue;
            }
            let kept = replies().lock().is_ok_and(|mut c| {
                // Only if no newer turn's reply replaced it meanwhile.
                let mine = c.get(&card).is_some_and(|x| x.mark == mark);
                if mine {
                    c.insert(card.clone(), Cached { mark, parts });
                }
                mine
            });
            if kept {
                let _ = app.emit("needs-changed", card);
            }
        }
    });
}

/// Session `row_id` is at permission prompt `seq` (`Sessions::prompt_wait`): reads what it
/// asks to run from the transcript at `path` on a worker thread, at 0.5 s and again at 2 s if
/// the call isn't written yet, and keeps it on the session while it is still at that prompt.
pub fn read_prompt_call(app: &AppHandle, row_id: &str, seq: u64, message: &str, path: &str) {
    let Some(tool) = transcript::permission_tool(message) else { return };
    let path = std::path::PathBuf::from(path);
    if path.extension().map_or(true, |e| e != "jsonl") {
        return;
    }
    let (app, row) = (app.clone(), row_id.to_string());
    std::thread::spawn(move || {
        for wait in [500, 1500] {
            std::thread::sleep(Duration::from_millis(wait));
            let sessions = app.state::<Arc<Sessions>>();
            if sessions.prompt_wait(&row).map(|(s, _)| s) != Some(seq) {
                return;
            }
            if let Some(call) = transcript::prompt_call(&path, &tool) {
                if let Some(card) = sessions.set_prompt_call(&row, seq, call) {
                    let _ = app.emit("needs-changed", card);
                }
                return;
            }
        }
    });
}

/// Records that Hugh has seen the card's conversation up to now.
pub fn mark_read(app: &AppHandle, card_id: &str) {
    {
        let board = app.state::<Board>();
        let Ok(mut store) = board.0.lock() else { return };
        if store.get(card_id).is_err() {
            return;
        }
        let _ = store.mark_card(card_id, "read");
    }
    let _ = app.emit("needs-changed", card_id.to_string());
}

/// A card's session ended a turn (its Stop hook). Unread, unless Hugh is looking at the card's
/// panel in a focused window. `reply` is the hook's `last_assistant_message`, kept for the
/// board's line; without it a worker reads it from the transcript. Call it holding no lock:
/// the focus check waits on the main thread.
pub fn turn_ended(app: &AppHandle, card_id: &str, reply: Option<&str>) {
    let looking = crate::notify::looking_at(app, card_id);
    let stamp = {
        let board = app.state::<Board>();
        let Ok(mut store) = board.0.lock() else { return };
        let stamp = store.mark_card(card_id, "reply").ok();
        if looking {
            let _ = store.mark_card(card_id, "read");
        }
        stamp
    };
    if let Some(mark) = stamp {
        match reply {
            Some(text) => {
                if let Ok(mut c) = replies().lock() {
                    c.insert(card_id.to_string(), Cached { mark, parts: Some(transcript::reply_parts(text)) });
                }
            }
            None if !looking => fill_replies(app, vec![(card_id.to_string(), mark)]),
            None => {}
        }
    }
    let _ = app.emit("needs-changed", card_id.to_string());
}

/// Each card that needs Hugh: `{ cardId: { reason, detail?, at, unread } }`.
#[tauri::command]
pub fn needs_you(app: AppHandle) -> Value {
    json!(current(&app))
}

/// Marks the card read (the page calls it when the window regains focus with a panel open;
/// opening a panel records a read through `panel_opened`).
#[tauri::command]
pub fn mark_card_read(app: AppHandle, card_id: String) {
    mark_read(&app, &card_id);
}

/// Each account's newest 7-day usage, as its sessions' status lines last reported it:
/// `{ main: { sevenDay, at }, account2: { sevenDay, at } }`, with nulls where none came yet.
pub fn usage(store: &Store) -> Value {
    let one = |a: &str| {
        let v: Value = store
            .setting(&format!("usage.{a}"))
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        json!({ "sevenDay": v.get("seven_day").and_then(Value::as_f64), "at": v.get("at").and_then(Value::as_str) })
    };
    json!({ "main": one("main"), "account2": one("account2") })
}

#[tauri::command]
pub fn usage_summary(board: State<Board>) -> Result<Value, String> {
    let store = board.0.lock().map_err(|e| e.to_string())?;
    Ok(usage(&store))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanban_store::SessionRow;

    fn board() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("b.db")).unwrap();
        (dir, s)
    }

    fn card(s: &mut Store, id: &str) {
        s.create(Some(id), json!({ "title": id }).as_object().cloned().unwrap(), "t").unwrap();
    }

    fn session(s: &mut Store, id: &str, card: &str) {
        s.add_session(&SessionRow {
            id: id.into(),
            card_id: card.into(),
            claude_session_id: format!("c-{id}"),
            account: "main".into(),
            cwd: ".".into(),
            model: None,
            effort: None,
            started_at: now(),
            status: None,
        })
        .unwrap();
    }

    fn tick() {
        std::thread::sleep(std::time::Duration::from_millis(3));
    }

    fn live(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    /// `compute` with no waits and no cached replies.
    fn needs(s: &Store, states: &HashMap<String, String>) -> kanban_store::Result<BTreeMap<String, Need>> {
        compute(s, states, &HashMap::new(), &mut HashMap::new())
    }

    fn card_in(s: &mut Store, id: &str, column: &str) {
        s.create(Some(id), json!({ "title": id, "column": column }).as_object().cloned().unwrap(), "t").unwrap();
    }

    fn cached(mark: &str, reply: &str) -> Cached {
        Cached { mark: mark.into(), parts: Some(transcript::reply_parts(reply)) }
    }

    #[test]
    fn each_need_says_what_it_asks() {
        let (_d, mut s) = board();
        for id in ["q", "p", "m", "f", "c"] {
            card(&mut s, id);
        }
        card_in(&mut s, "plan", "Planning");
        card(&mut s, "r");
        session(&mut s, "sf", "f");
        s.session_state("sf", "failed").unwrap();
        session(&mut s, "sc", "c");
        s.session_event("sc", "exited", None).unwrap();
        let mark_r = s.mark_card("r", "reply").unwrap();
        let mark_plan = s.mark_card("plan", "reply").unwrap();
        let questions = json!([
            { "header": "Base", "question": "Which branch?", "multiSelect": false, "options": [{ "label": "main" }, { "label": "dsh-agents" }] },
            { "header": "B", "question": "Second?", "options": [{ "label": "x" }] },
            { "header": "C", "question": "Third?", "options": [{ "label": "y" }] },
        ]);
        let waits: HashMap<String, Waiting> = [
            ("q", Waiting { questions: Some(questions), ..Default::default() }),
            ("p", Waiting { prompt: Some("Bash: cargo test --workspace".into()), ..Default::default() }),
            ("m", Waiting::default()),
            ("f", Waiting { failure: Some("rate limit".into()), ..Default::default() }),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let mut replies: HashMap<String, Cached> = [
            ("r", cached(&mark_r, "I changed the store.\n\nShall I install it in the test app?")),
            ("plan", cached(&mark_plan, "Plan ready.\n\nIt adds one field and a filter.")),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let states = live(&[("q", "permission"), ("p", "permission"), ("m", "permission"), ("f", "failed")]);
        let n = compute(&s, &states, &waits, &mut replies).unwrap();
        let q = n["q"].ask.as_ref().unwrap();
        assert_eq!((q.kind, q.text.as_str(), q.options.clone(), q.more), ("question", "Which branch?", vec!["main".to_string(), "dsh-agents".into()], 2));
        let p = n["p"].ask.as_ref().unwrap();
        assert_eq!((p.kind, p.text.as_str(), p.options.len(), p.more), ("permission", "Bash: cargo test --workspace", 0, 0));
        // A prompt with no text known yet has no ask, but still needs Hugh.
        assert_eq!((n["m"].reason, n["m"].ask.as_ref()), ("permission", None));
        let f = n["f"].ask.as_ref().unwrap();
        assert_eq!((f.kind, f.text.as_str()), ("failed", "its turn failed (rate limit)"));
        let c = n["c"].ask.as_ref().unwrap();
        assert_eq!((c.kind, c.text.as_str()), ("failed", "its session crashed"));
        let r = n["r"].ask.as_ref().unwrap();
        assert_eq!((n["r"].reason, r.kind, r.text.as_str()), ("unread", "reply", "Shall I install it in the test app?"));
        let pl = n["plan"].ask.as_ref().unwrap();
        assert_eq!((pl.kind, pl.text.as_str()), ("plan", "It adds one field and a filter."));
        // A failure with no error text says only that.
        let none: HashMap<String, Waiting> = HashMap::new();
        let n = compute(&s, &states, &none, &mut replies).unwrap();
        assert_eq!(n["f"].ask.as_ref().unwrap().text, "its turn failed");
        // The JSON the board reads.
        let v = serde_json::to_value(&n["r"]).unwrap();
        assert_eq!(v["ask"], json!({ "kind": "reply", "text": "Shall I install it in the test app?", "options": [], "more": 0 }));
        assert!(serde_json::to_value(&n["m"]).unwrap().get("ask").is_none());
    }

    #[test]
    fn an_unread_statement_or_an_older_reply_has_no_ask() {
        let (_d, mut s) = board();
        card(&mut s, "st");
        card(&mut s, "old");
        card(&mut s, "away");
        let mark_st = s.mark_card("st", "reply").unwrap();
        let old = s.mark_card("old", "reply").unwrap();
        s.mark_card("away", "reply").unwrap();
        tick();
        s.mark_card("old", "reply").unwrap();
        let mut replies: HashMap<String, Cached> = [
            ("st", cached(&mark_st, "Done. The tests pass.")),
            // Kept for the turn before: another turn's question.
            ("old", cached(&old, "Shall I go on?")),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let n = compute(&s, &HashMap::new(), &HashMap::new(), &mut replies).unwrap();
        for id in ["st", "old", "away"] {
            assert_eq!((n[id].reason, n[id].ask.as_ref()), ("unread", None), "{id}");
        }
        // Plan ready outside Planning is only a dot too.
        let mark = s.mark_card("st", "reply").unwrap();
        let mut replies: HashMap<String, Cached> = [("st".to_string(), cached(&mark, "Plan ready. It adds a field."))].into_iter().collect();
        assert_eq!(compute(&s, &HashMap::new(), &HashMap::new(), &mut replies).unwrap()["st"].ask, None);
    }

    #[test]
    fn asks_are_redacted_and_cut() {
        let (_d, mut s) = board();
        card(&mut s, "p");
        card(&mut s, "r");
        let mark = s.mark_card("r", "reply").unwrap();
        let pw = "pass".to_owned() + "word";
        let waits: HashMap<String, Waiting> =
            [("p".to_string(), Waiting { prompt: Some(format!("Bash: curl -H 'Authorization: Bearer abc123' https://x.test/a?{pw}=1")), ..Default::default() })].into_iter().collect();
        let long = format!("{} ok?", "word ".repeat(100));
        let mut replies: HashMap<String, Cached> = [("r".to_string(), cached(&mark, &long))].into_iter().collect();
        let n = compute(&s, &live(&[("p", "permission")]), &waits, &mut replies).unwrap();
        let p = &n["p"].ask.as_ref().unwrap().text;
        assert!(!p.contains("abc123") && !looks_secret(p), "{p}");
        let r = &n["r"].ask.as_ref().unwrap().text;
        assert!(r.chars().count() <= ASK_MAX && r.ends_with('…'), "{r}");
    }

    #[test]
    fn redact_hides_secrets() {
        let pw = "pass".to_owned() + "word";
        let ct = "c".to_owned() + "t=";
        let jwt = "ey".to_owned() + "JhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2ln";
        let ghp = "gh".to_owned() + "p_" + &"a1B2".repeat(9);
        let akia = "AK".to_owned() + "IA" + "ABCDEFGHIJKLMNOP";
        let oob = "oob".to_owned() + "Code=QWERTY123";
        let api = "api".to_owned() + "Key=AIzaZZZ999";
        let tok = "tok".to_owned() + "en=s3cr3tvalue";
        let cases: Vec<(String, &str)> = vec![
            (format!("Open https://app.example.com/signin?{ct}abcDEF123 to sign in"), "abcDEF123"),
            ("Use http://localhost:3000/dev-sign-in?email=h@x.test&next=/home now".to_string(), "email=h@x.test"),
            ("Bash: curl -H \"Authorization: Bearer xyzTOKEN789\" https://api.x.test/v1".to_string(), "xyzTOKEN789"),
            (format!("git push https://x:{ghp}@github.com/a/b"), ghp.as_str()),
            (format!("token is {jwt}"), "c2ln"),
            (format!("Type your {pw} in the box"), pw.as_str()),
            (format!("export AWS_ACCESS_KEY_ID={akia}"), akia.as_str()),
            (format!("the link has {oob} in it"), "QWERTY123"),
            (format!("call it with {api}"), "AIzaZZZ999"),
            (format!("x?{tok}&y=1 and also {tok}"), "s3cr3tvalue"),
        ];
        for (text, secret) in &cases {
            let r = redact(text);
            assert!(!looks_secret(&r), "{text} -> {r}");
            assert!(!r.contains(secret), "{text} -> {r}");
        }
        // Plain text is untouched; a link keeps its host and first path segment only.
        assert_eq!(redact("Run cargo test on #108, then see ~/build"), "Run cargo test on #108, then see ~/build");
        assert_eq!(redact("see https://github.com/hughht5/kanban-app/pull/3"), "see https://github.com/hughht5/…");
        assert_eq!(redact("see https://x.test/docs and https://x.test/"), "see https://x.test/docs and https://x.test/");
        assert_eq!(redact("Open https://x.test/docs?page=2"), "Open https://x.test/docs?…");
        assert_eq!(redact(&pw), "[hidden]");
    }

    #[test]
    fn redact_hides_more_secret_shapes() {
        let secret = "hunter".to_owned() + "2xyz";
        let glpat = "gl".to_owned() + "pat-" + "abcdefghij1234567890";
        let ghs = "gh".to_owned() + "s_" + &"Zx9".repeat(12);
        let ghu = "gh".to_owned() + "u_" + &"Qw7".repeat(12);
        let ghr = "gh".to_owned() + "r_" + &"Er4".repeat(12);
        let xoxb = "xo".to_owned() + "xb-1234-5678-abcdefgh";
        let aiza = "AI".to_owned() + "za" + "SyA1234567890abcdefghijk";
        let opaque = "a1".repeat(20);
        let magic = "AbC123def456".to_owned() + "GhI789";
        let basic = "dXNlcjpw".to_owned() + "YXNzd29yZA";
        let cases: Vec<(String, &str)> = vec![
            // Credentials in a URL's userinfo.
            (format!("clone https://hugh:{secret}@github.com/a/b"), secret.as_str()),
            // key: value and JSON forms.
            (format!(r#"{{"api_{}": "{secret}"}}"#, "token"), secret.as_str()),
            (format!("GITHUB_{}: {secret}", "TOKEN"), secret.as_str()),
            (format!("client_{}: {secret}", "secret"), secret.as_str()),
            (format!("my_{} = {secret}", "credential"), secret.as_str()),
            // Any Authorization scheme.
            (format!("Authorization: Basic {basic}"), basic.as_str()),
            (format!("-H 'Authorization: token {secret}'"), secret.as_str()),
            // Token shapes.
            (format!("use {glpat} here"), glpat.as_str()),
            (format!("use {ghs}"), ghs.as_str()),
            (format!("use {ghu}"), ghu.as_str()),
            (format!("use {ghr}"), ghr.as_str()),
            (format!("slack {xoxb}"), xoxb.as_str()),
            (format!("key {aiza}"), aiza.as_str()),
            (format!("opaque {opaque} string"), opaque.as_str()),
            // A magic link's token in its path.
            (format!("https://app.x.test/auth/magic/{magic}"), magic.as_str()),
        ];
        for (text, hidden) in &cases {
            let r = redact(text);
            assert!(!looks_secret(&r), "{text} -> {r}");
            assert!(!r.contains(hidden), "{text} -> {r}");
        }
        assert_eq!(redact(&format!("clone https://hugh:{secret}@github.com/a/b")), "clone https://github.com/a/…");
        assert_eq!(redact(&format!("GITHUB_{}: {secret}", "TOKEN")), "GITHUB_TOKEN: [hidden]");
        assert_eq!(redact(&format!("Authorization: Basic {basic}")), "Authorization: [hidden]");
        assert_eq!(redact(&format!("https://app.x.test/auth/magic/{magic}")), "https://app.x.test/auth/…");
        // A long word of letters only, or of digits only, stays.
        let letters = "abcdefgh".repeat(5);
        assert_eq!(redact(&letters), letters);
        let digits = "12345678".repeat(5);
        assert_eq!(redact(&digits), digits);
    }

    #[test]
    fn the_reply_cache_drops_archived_and_gone_cards() {
        let (_d, mut s) = board();
        card(&mut s, "keep");
        card(&mut s, "arch");
        let mark = s.mark_card("keep", "reply").unwrap();
        let v = s.get("arch").unwrap().version;
        s.archive("arch", v, true, "t").unwrap();
        let mut replies: HashMap<String, Cached> = [("keep", cached(&mark, "Go?")), ("arch", cached(&mark, "Go?")), ("gone", cached(&mark, "Go?"))]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        compute(&s, &HashMap::new(), &HashMap::new(), &mut replies).unwrap();
        assert_eq!(replies.keys().collect::<Vec<_>>(), ["keep"]);
    }

    #[test]
    fn permission_failed_and_unread_need_hugh() {
        let (_d, mut s) = board();
        for id in ["p", "f", "c", "u", "quiet"] {
            card(&mut s, id);
        }
        session(&mut s, "sp", "p");
        s.session_state("sp", "permission").unwrap();
        session(&mut s, "sf", "f");
        s.session_state("sf", "failed").unwrap();
        session(&mut s, "sc", "c");
        s.session_event("sc", "exited", None).unwrap();
        s.mark_card("u", "reply").unwrap();
        session(&mut s, "sq", "quiet");
        s.session_state("sq", "idle").unwrap();
        let states = live(&[("p", "permission"), ("f", "failed"), ("quiet", "idle")]);
        let n = needs(&s, &states).unwrap();
        assert_eq!(n.keys().map(String::as_str).collect::<Vec<_>>(), ["c", "f", "p", "u"]);
        assert_eq!(n["p"].reason, "permission");
        assert_eq!((n["f"].reason, n["f"].detail), ("failed", Some("turn failed")));
        assert_eq!((n["c"].reason, n["c"].detail), ("failed", Some("crashed")));
        assert_eq!(n["u"].reason, "unread");
        assert!(!n["p"].at.is_empty() && !n["u"].at.is_empty());
    }

    #[test]
    fn opening_the_card_clears_unread_and_failures_but_not_a_prompt() {
        let (_d, mut s) = board();
        for id in ["p", "f", "c", "u"] {
            card(&mut s, id);
        }
        session(&mut s, "sp", "p");
        s.session_state("sp", "permission").unwrap();
        s.mark_card("p", "reply").unwrap();
        session(&mut s, "sf", "f");
        s.session_state("sf", "failed").unwrap();
        session(&mut s, "sc", "c");
        s.session_event("sc", "exited", None).unwrap();
        s.mark_card("u", "reply").unwrap();
        tick();
        for id in ["p", "f", "c", "u"] {
            s.mark_card(id, "read").unwrap();
        }
        let states = live(&[("p", "permission"), ("f", "failed")]);
        let n = needs(&s, &states).unwrap();
        assert_eq!(n.keys().collect::<Vec<_>>(), ["p"]);
        assert!(!n["p"].unread);
        // A new turn's end after the read is unread again.
        tick();
        s.mark_card("u", "reply").unwrap();
        assert_eq!(needs(&s, &states).unwrap()["u"].reason, "unread");
    }

    #[test]
    fn a_prompt_outranks_an_unread_reply_and_archived_cards_never_need_hugh() {
        let (_d, mut s) = board();
        card(&mut s, "p");
        card(&mut s, "gone");
        s.mark_card("p", "reply").unwrap();
        s.mark_card("gone", "reply").unwrap();
        let v = s.get("gone").unwrap().version;
        s.archive("gone", v, true, "t").unwrap();
        let n = needs(&s, &live(&[("p", "permission")])).unwrap();
        assert_eq!(n.len(), 1);
        assert_eq!(n["p"].reason, "permission");
        assert!(n["p"].unread);
    }

    #[test]
    fn usage_reads_the_seven_day_figures() {
        let (_d, mut s) = board();
        assert_eq!(usage(&s), json!({ "main": { "sevenDay": null, "at": null }, "account2": { "sevenDay": null, "at": null } }));
        s.set_setting("usage.main", r#"{"seven_day": 42.5, "five_hour": 90, "at": "2026-11-04T10:00:00Z"}"#).unwrap();
        s.set_setting("usage.account2", r#"{"seven_day": 17}"#).unwrap();
        let u = usage(&s);
        assert_eq!(u["main"]["sevenDay"], json!(42.5));
        assert_eq!(u["main"]["at"], json!("2026-11-04T10:00:00Z"));
        assert_eq!(u["account2"]["sevenDay"], json!(17.0));
        assert!(u["main"].get("fiveHour").is_none());
    }
}
