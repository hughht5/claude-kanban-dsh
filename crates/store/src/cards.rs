use crate::done_when::{self, clean_done_when, clean_done_when_checks, DONE_WHEN_CHECKS_KEY, DONE_WHEN_KEY};
use crate::secret::find_secret;
use crate::waits::{self, BLOCKED_KEY, WAITED_KEY, WAIT_BY};
use crate::{now, Error, Result, Store, COLUMNS};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::Serialize;
use serde_json::{json, Map, Value};

/// Fields held in their own columns rather than in `fields`.
const CORE: [&str; 5] = ["number", "column", "order", "orderCol", "log"];
/// Fields no write may set directly.
const READ_ONLY: [&str; 3] = ["id", "number", "version"];

/// A partial update in the artifact's field names. A `null` value removes the field.
pub type Patch = Map<String, Value>;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LogEntry {
    pub at: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Card {
    pub id: String,
    pub number: i64,
    pub column: String,
    pub order: Option<f64>,
    pub order_col: Option<String>,
    pub archived: bool,
    pub fields: Map<String, Value>,
    pub version: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MoveOutcome {
    pub card: Card,
    /// The move asked for a ship (Hugh dropped it into Ship it).
    pub ship: bool,
    /// The move asked for a session to plan it (Hugh dropped it into Planning).
    pub plan: bool,
}

impl Card {
    pub fn title(&self) -> &str {
        self.fields.get("title").and_then(Value::as_str).unwrap_or("")
    }

    /// Where the card goes on to by itself (`autoTo`), if anywhere.
    pub fn auto_to(&self) -> Option<&str> {
        self.fields.get(AUTO_KEY).and_then(Value::as_str).filter(|t| AUTO_TARGETS.contains(t))
    }

    fn state(&self, key: &str) -> Option<&str> {
        self.fields.get(key)?.get("state")?.as_str()
    }

    fn retries(&self) -> i64 {
        self.fields
            .get("ship")
            .and_then(|s| s.get("retries"))
            .and_then(Value::as_i64)
            .unwrap_or(0)
    }

    /// The card in the artifact's document shape, with the given log.
    pub fn to_document(&self, log: &[LogEntry]) -> Value {
        let mut doc = self.fields.clone();
        doc.insert("number".into(), json!(self.number));
        doc.insert("column".into(), json!(self.column));
        if let Some(o) = self.order {
            doc.insert("order".into(), json!(o));
        }
        if let Some(c) = &self.order_col {
            doc.insert("orderCol".into(), json!(c));
        }
        if !log.is_empty() {
            let entries: Vec<Value> = log
                .iter()
                .map(|e| json!({ "at": e.at, "text": e.text }))
                .collect();
            doc.insert("log".into(), Value::Array(entries));
        }
        Value::Object(doc)
    }
}

fn row_to_card(r: &rusqlite::Row) -> rusqlite::Result<Card> {
    let fields: String = r.get("fields")?;
    let fields: Map<String, Value> = serde_json::from_str(&fields).unwrap_or_default();
    Ok(Card {
        id: r.get("id")?,
        number: r.get("number")?,
        column: r.get("col")?,
        order: r.get("ord")?,
        order_col: r.get("ord_col")?,
        archived: r.get::<_, i64>("archived")? != 0,
        fields,
        version: r.get("version")?,
    })
}

pub(crate) fn get_tx(tx: &rusqlite::Connection, id: &str) -> Result<Option<Card>> {
    Ok(tx
        .query_row("SELECT * FROM cards WHERE id = ?1", [id], row_to_card)
        .optional()?)
}

fn check_secrets(patch: &Patch) -> Result<()> {
    for (k, v) in patch {
        if let Some(path) = find_secret(k, v) {
            return Err(Error::Secret(path));
        }
    }
    Ok(())
}

fn check_column(c: &str) -> Result<()> {
    if COLUMNS.contains(&c) {
        Ok(())
    } else {
        Err(Error::Invalid(format!("no column called \"{c}\"")))
    }
}

/// Most labels a card can carry, and the longest a label may be (in characters).
pub const MAX_LABELS: usize = 8;
pub const MAX_LABEL_LEN: usize = 24;

/// Cleans a card's `labels`: a list of strings (or one string of comma-separated labels, as
/// `kb update 5 --labels BUG,RCS` sends), each trimmed with inner spaces collapsed, empty ones
/// dropped, and repeats (ignoring case) kept once in their first spelling. `null` or an empty list
/// means no labels (`None`). Too many, or one too long, is refused rather than cut short.
pub fn clean_labels(v: &Value) -> Result<Option<Value>> {
    let raw: Vec<String> = match v {
        Value::Null => return Ok(None),
        Value::String(s) => s.split(',').map(str::to_string).collect(),
        Value::Array(a) => a
            .iter()
            .map(|x| x.as_str().map(str::to_string))
            .collect::<Option<_>>()
            .ok_or_else(|| Error::Invalid("labels is a list of short words, like [\"BUG\", \"RCS\"]".into()))?,
        _ => return Err(Error::Invalid("labels is a list of short words, like [\"BUG\", \"RCS\"]".into())),
    };
    let mut out: Vec<String> = Vec::new();
    for l in raw {
        let l = l.split_whitespace().collect::<Vec<_>>().join(" ");
        if l.is_empty() || out.iter().any(|o| o.to_lowercase() == l.to_lowercase()) {
            continue;
        }
        if l.chars().count() > MAX_LABEL_LEN {
            return Err(Error::Invalid(format!("the label \"{l}\" is longer than {MAX_LABEL_LEN} characters")));
        }
        out.push(l);
    }
    if out.len() > MAX_LABELS {
        return Err(Error::Invalid(format!("a card takes at most {MAX_LABELS} labels")));
    }
    Ok(if out.is_empty() { None } else { Some(json!(out)) })
}

/// Files a session hands over to Hugh (card #108): `changesOutputs`, a list of `{path, label}`.
pub const OUTPUTS_KEY: &str = "changesOutputs";
/// The figures the app saved when the card entered Human test (card #108); the app writes it.
pub const SAVED_KEY: &str = "changesSaved";
pub const MAX_OUTPUTS: usize = 20;
pub const MAX_OUTPUT_LABEL_LEN: usize = 80;

/// Why `p` is not a full local Windows file path (like C:/x/report.md), if it isn't: the one rule for
/// a card's outputs, used by the store and by the app before it opens one.
pub fn output_path_problem(p: &str) -> Option<&'static str> {
    let lower = p.to_ascii_lowercase();
    if p.is_empty() {
        return Some("is empty");
    }
    if lower.contains("://") || lower.starts_with("file:") || lower.starts_with("http:") || lower.starts_with("https:") {
        return Some("is a URL; give a local file path");
    }
    if p.starts_with("\\\\") || p.starts_with("//") || p.starts_with("\\/") || p.starts_with("/\\") {
        return Some("is a network path; give a local file path");
    }
    let b = p.as_bytes();
    if b.len() < 4 || !b[0].is_ascii_alphabetic() || b[1] != b':' || !(b[2] == b'\\' || b[2] == b'/') {
        return Some("isn't a full local path (like C:\\x\\report.md)");
    }
    None
}

/// Cleans a card's `changesOutputs`: a list of at most 20 `{path, label}`, path a full local
/// Windows file path, label optional (trimmed, at most 80 characters; empty means none).
/// `null` or `[]` clears it (`None`). Anything else is refused.
pub fn clean_changes_outputs(v: &Value) -> Result<Option<Value>> {
    let shape = || Error::Invalid("changesOutputs is a list of {\"path\": \"C:\\\\x\\\\report.md\", \"label\": \"optional\"} (or null to clear it)".into());
    let items = match v {
        Value::Null => return Ok(None),
        Value::Array(a) => a,
        _ => return Err(shape()),
    };
    if items.len() > MAX_OUTPUTS {
        return Err(Error::Invalid(format!("changesOutputs takes at most {MAX_OUTPUTS} files")));
    }
    let mut out = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let o = item.as_object().ok_or_else(shape)?;
        if let Some(k) = o.keys().find(|k| *k != "path" && *k != "label") {
            return Err(Error::Invalid(format!("changesOutputs[{i}] has \"{k}\"; an entry has only path and label")));
        }
        let path = o.get("path").and_then(Value::as_str).ok_or_else(shape)?.trim();
        if let Some(why) = output_path_problem(path) {
            return Err(Error::Invalid(format!("changesOutputs[{i}].path \"{path}\" {why}")));
        }
        let label = match o.get("label") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.trim()).filter(|s| !s.is_empty()),
            Some(_) => return Err(Error::Invalid(format!("changesOutputs[{i}].label is a string"))),
        };
        if label.map_or(false, |l| l.chars().count() > MAX_OUTPUT_LABEL_LEN) {
            return Err(Error::Invalid(format!("changesOutputs[{i}].label is longer than {MAX_OUTPUT_LABEL_LEN} characters")));
        }
        let mut e = Map::new();
        e.insert("path".into(), json!(path));
        if let Some(l) = label {
            e.insert("label".into(), json!(l));
        }
        out.push(Value::Object(e));
    }
    Ok(if out.is_empty() { None } else { Some(Value::Array(out)) })
}

/// Checks a card's `changesSaved`: an object, or `null` for none (`None`).
fn clean_changes_saved(v: &Value) -> Result<Option<Value>> {
    match v {
        Value::Null => Ok(None),
        Value::Object(_) => Ok(Some(v.clone())),
        _ => Err(Error::Invalid("changesSaved is an object (or null)".into())),
    }
}

/// Where a card can go on to by itself (card #76): Hugh's "Take it to" past Planning. The app
/// moves it on until it gets there (`autoTo`); Planning is only a start, so it isn't kept.
pub const AUTO_TARGETS: [&str; 2] = ["Human test", "Deploy complete"];
/// The card field that holds it.
pub const AUTO_KEY: &str = "autoTo";
/// The card field holding its latest subagent test (card #76): `{state, at, detail, ...}`, where
/// state is "needed" (set on every move into In progress), "passed", "failed" or "unclear".
pub const TEST_KEY: &str = "test";
/// The card field holding its latest subagent review of the diff (card #96): the same shape as
/// `test`, plus `round`, the FAILs since the card last came into In progress.
pub const REVIEW_KEY: &str = "review";
/// The card field holding the card-router subagent's route (card #98): `{size, model, effort,
/// check, reason, children, at, agentId, by}`, where size is "small", "normal" or "split". Only
/// the app writes it (sessions can't: it isn't in kanban_update_card's list); Hugh's Route again
/// clears it. A card is routed once until then.
pub const ROUTE_KEY: &str = "route";

/// The card field holding the card-reproducer's record of a reported bug (card #112):
/// `{state, at, reports, before, after, test, earlier, ...}`. Not reset by a move: a bug report
/// moves the card into In progress and sets it at once.
pub const REPRO_KEY: &str = "repro";

/// Is a bug report on the card still to be proved (card #112): its repro needed, reproduced but
/// not proved fixed, or its latest run unclear. Fixed, not reproduced, or no record: no.
pub fn repro_open(card: &Card) -> bool {
    matches!(card.fields.get(REPRO_KEY).and_then(|r| r.get("state")).and_then(Value::as_str), Some("needed" | "reproduced" | "unclear"))
}
/// The fields every move into In progress sets back to `{state: "needed"}` (an older verdict
/// was for older work), unless the same patch sets them. One line each.
pub const RESET_ON_IN_PROGRESS: &[&str] = &[
    TEST_KEY,
    REVIEW_KEY,
];
/// The card field holding its latest plan critique (card #97): `{state, at, agentId, detail,
/// round, started}`. Every move into Planning resets it to needed with round 0; an edit to the
/// summary (the plan) in Planning, or in Blocked before the build started, makes it needed
/// again but keeps the FAIL count; every move into In progress marks `started`, so a card
/// blocked mid-build can go back without a new critique.
pub const CRITIQUE_KEY: &str = "critique";
/// The card fields that hold the plan, so an edit to one makes the critique needed again: the
/// summary, and card #105's acceptance checks (if that card's fields are on the board).
pub const PLAN_FIELDS: [&str; 3] = ["summary", "doneWhen", "doneWhenChecks"];

/// The card field holding how long its Human test should take to check (card #103), worked out
/// by the app from the card's diff: `{class: "quick"|"real", why, reasons, diff, ...}`.
pub const CHECK_SIZE_KEY: &str = "checkSize";
/// The fields every move into Human test removes (an older verdict was for older work), unless
/// the same patch sets them. The app works them out again. One line each.
pub const RESET_ON_HUMAN_TEST: &[&str] = &[
    CHECK_SIZE_KEY,
];

/// The card field holding the card-merger's latest report at ship (card #100): `{state, at,
/// agentId, detail, head, origin: {before, after}}`, state "passed", "failed" or "unclear".
/// Only the app writes it (it isn't a session field); every move into Ship it clears it, so a
/// report from an earlier ship never shows on a new one.
pub const MERGE_KEY: &str = "merge";

/// The card field holding its small-change check (card #104): `{state, at, ...}`, where state is
/// "waiting" (it goes on to Ship it at `goesAt` unless Hugh stops it), "went on" or "stopped".
pub const SMALL_KEY: &str = "smallChange";
/// The state of a card waiting to go on by itself.
pub const SMALL_WAITING: &str = "waiting";
/// The state of a card the app moved into Ship it this way.
pub const SMALL_WENT_ON: &str = "went on";
/// The field Hugh sets to keep a card from ever going on by itself.
pub const SMALL_OPT_OUT_KEY: &str = "smallOptOut";
/// Who the app's small-change moves and records are written as.
pub const SMALL_BY: &str = "the app (small-change auto-ship)";
/// Who Hugh's own moves on the board are recorded as. The waits poll reads it back from the
/// revisions to tell his own drag into Blocked (card #109).
pub(crate) const HUGH: &str = "Hugh (board)";
/// Who the app's moves for Hugh are recorded as.
pub const AUTO_BY: &str = "the app (Hugh's Take it to)";
/// Columns Hugh can move a card into himself without turning its auto-progress off: starting
/// it, or approving its plan. Any other move of his means he has taken it over.
pub const AUTO_KEEPS: [&str; 2] = ["Planning", "In progress"];

/// Checks a card's `autoTo`: one of `AUTO_TARGETS` (any case, trimmed), or `null` / `""` for
/// none (`None`).
pub fn clean_auto_to(v: &Value) -> Result<Option<Value>> {
    let bad = || Error::Invalid("autoTo is \"Human test\" or \"Deploy complete\" (or null for none)".into());
    match v {
        Value::Null => Ok(None),
        Value::String(s) if s.trim().is_empty() => Ok(None),
        Value::String(s) => AUTO_TARGETS
            .iter()
            .find(|t| t.eq_ignore_ascii_case(s.trim()))
            .map(|t| Some(json!(t)))
            .ok_or_else(bad),
        _ => Err(bad()),
    }
}

/// Applies `patch` to `card` in memory, returning the names of the fields it changed.
pub(crate) fn apply(card: &mut Card, patch: &Patch) -> Result<Vec<String>> {
    let mut changed = Vec::new();
    let was_started = critique_started(card);
    for (k, v) in patch {
        if READ_ONLY.contains(&k.as_str()) {
            return Err(Error::Invalid(format!("\"{k}\" can't be changed")));
        }
        match k.as_str() {
            "log" => return Err(Error::Invalid("add log lines with add_log, not update".into())),
            "column" => {
                let c = v
                    .as_str()
                    .ok_or_else(|| Error::Invalid("column must be a string".into()))?;
                check_column(c)?;
                if card.column != c {
                    card.column = c.to_string();
                    changed.push(k.clone());
                    // Each time into In progress it needs a fresh subagent test and review
                    // before Human test (Hugh, cards #76 and #96): an older verdict was for
                    // older work.
                    if c == "In progress" {
                        for key in RESET_ON_IN_PROGRESS.iter().filter(|k| !patch.contains_key(**k)) {
                            card.fields.insert((*key).into(), json!({ "state": "needed", "at": now() }));
                            changed.push((*key).into());
                        }
                    }
                    if c == "Human test" {
                        for key in RESET_ON_HUMAN_TEST.iter().filter(|k| !patch.contains_key(**k)) {
                            if card.fields.remove(*key).is_some() {
                                changed.push((*key).into());
                            }
                        }
                    }
                    // Each ship gets its own merge (card #100).
                    if c == "Ship it" && !patch.contains_key(MERGE_KEY) && card.fields.remove(MERGE_KEY).is_some() {
                        changed.push(MERGE_KEY.into());
                    }
                    // A card waiting to go on by itself (card #104) stops waiting once it
                    // leaves Human test, however it left, unless the same patch says otherwise.
                    // One that went on and leaves Ship it unshipped (a failed ship, Hugh's drag)
                    // is no longer the app's ship: a later move into Ship it is Hugh's own. It
                    // keeps `wentOnAt`, so it never goes on by itself again.
                    if !patch.contains_key(SMALL_KEY) {
                        if let Some(Value::Object(sc)) = card.fields.get_mut(SMALL_KEY) {
                            let st = sc.get("state").and_then(Value::as_str).unwrap_or("");
                            let reason = match st {
                                SMALL_WAITING => Some(format!("it left Human test (moved to {c}) before it went on")),
                                SMALL_WENT_ON if !matches!(c, "Ship it" | "Deploy complete" | "Done") => Some(format!("it left Ship it (moved to {c}) before it shipped")),
                                _ => None,
                            };
                            if let Some(r) = reason {
                                sc.insert("state".into(), json!("stopped"));
                                sc.insert("at".into(), json!(now()));
                                sc.insert("reason".into(), json!(r));
                                changed.push(SMALL_KEY.into());
                            }
                        }
                    }
                }
            }
            "order" => {
                let o = if v.is_null() { None } else { v.as_f64() };
                if v.is_null() || o.is_some() {
                    if card.order != o {
                        card.order = o;
                        changed.push(k.clone());
                    }
                } else {
                    return Err(Error::Invalid("order must be a number".into()));
                }
            }
            "orderCol" => {
                let c = v.as_str().map(str::to_string);
                if card.order_col != c {
                    card.order_col = c;
                    changed.push(k.clone());
                }
            }
            "archived" => {
                // Kept in `fields` as given, so a card reads back exactly as it was written,
                // and mirrored in its own column for filtering.
                let a = v.as_bool().unwrap_or(false);
                let old = card.fields.get(k);
                if old != Some(v) {
                    if v.is_null() {
                        card.fields.remove(k);
                    } else {
                        card.fields.insert(k.clone(), v.clone());
                    }
                    changed.push(k.clone());
                }
                card.archived = a;
            }
            AUTO_KEY => {
                let new = clean_auto_to(v)?;
                if card.fields.get(k) != new.as_ref() {
                    match new {
                        Some(t) => card.fields.insert(k.clone(), t),
                        None => card.fields.remove(k),
                    };
                    changed.push(k.clone());
                }
            }
            BLOCKED_KEY | WAITED_KEY => {
                let new = if k == BLOCKED_KEY { waits::clean_blocked_by(v)? } else { waits::clean_waited_on(v)? };
                if card.fields.get(k) != new.as_ref() {
                    match new {
                        Some(l) => card.fields.insert(k.clone(), l),
                        None => card.fields.remove(k),
                    };
                    changed.push(k.clone());
                }
            }
            "labels" => {
                let new = clean_labels(v)?;
                if card.fields.get(k) != new.as_ref() {
                    match new {
                        Some(l) => card.fields.insert(k.clone(), l),
                        None => card.fields.remove(k),
                    };
                    changed.push(k.clone());
                }
            }
            OUTPUTS_KEY | SAVED_KEY => {
                let new = if k == OUTPUTS_KEY { clean_changes_outputs(v)? } else { clean_changes_saved(v)? };
                if card.fields.get(k) != new.as_ref() {
                    match new {
                        Some(x) => card.fields.insert(k.clone(), x),
                        None => card.fields.remove(k),
                    };
                    changed.push(k.clone());
                }
            }
            // Card #105: the "Done when..." line and its checks, cleaned; each check's who and
            // when is stamped here and its writer filled in by write_tx.
            DONE_WHEN_KEY | DONE_WHEN_CHECKS_KEY => {
                let new = if k == DONE_WHEN_KEY {
                    clean_done_when(v)?
                } else {
                    clean_done_when_checks(v)?.map(|l| done_when::stamp(l, card.fields.get(k), &now()))
                };
                if card.fields.get(k) != new.as_ref() {
                    match new {
                        Some(x) => card.fields.insert(k.clone(), x),
                        None => card.fields.remove(k),
                    };
                    changed.push(k.clone());
                }
            }
            _ => {
                let old = card.fields.get(k);
                if v.is_null() {
                    if old.is_some() {
                        card.fields.remove(k);
                        changed.push(k.clone());
                    }
                } else if old != Some(v) {
                    card.fields.insert(k.clone(), v.clone());
                    changed.push(k.clone());
                }
            }
        }
    }
    critique_resets(card, patch, &mut changed, was_started);
    Ok(changed)
}

/// Whether `card`'s plan critique says its build has started (`CRITIQUE_KEY`'s `started`).
fn critique_started(card: &Card) -> bool {
    card.fields.get(CRITIQUE_KEY).and_then(|r| r.get("started")).and_then(Value::as_bool) == Some(true)
}

/// The plan critique's resets (card #97), after `patch` changed `changed` on `card`: see
/// `CRITIQUE_KEY`. `was_started` is `started` before the patch. A patch that sets the critique
/// itself (the app recording a verdict) is kept as it is, except that it keeps `started`, so a
/// verdict on a card already being built doesn't hold it again.
fn critique_resets(card: &mut Card, patch: &Patch, changed: &mut Vec<String>, was_started: bool) {
    if patch.contains_key(CRITIQUE_KEY) {
        if let Some(r) = card.fields.get_mut(CRITIQUE_KEY).and_then(Value::as_object_mut) {
            if was_started && !r.contains_key("started") {
                r.insert("started".into(), json!(true));
            }
        }
        return;
    }
    let moved = changed.iter().any(|k| k == "column");
    let edited = changed.iter().any(|k| PLAN_FIELDS.contains(&k.as_str()));
    if !moved && !edited {
        return;
    }
    let needed = |round: u64| json!({ "state": "needed", "at": now(), "round": round });
    let rec = card.fields.get(CRITIQUE_KEY).and_then(Value::as_object);
    let new = if moved && card.column == "Planning" {
        Some(needed(0))
    } else if moved && card.column == "In progress" {
        // The build has started: from here a stop in Blocked doesn't need a new critique.
        if was_started {
            None
        } else {
            let mut r = rec.cloned().unwrap_or_else(|| needed(0).as_object().cloned().unwrap_or_default());
            r.insert("started".into(), json!(true));
            Some(Value::Object(r))
        }
    } else if edited && rec.and_then(|r| r.get("state")).and_then(Value::as_str) != Some("needed") {
        // The plan changed, so an older verdict was for an older plan: in Planning or Backlog,
        // and in Blocked before the build started. A Blocked card with no record needs none
        // (the gate goes by whether it was ever in In progress).
        let round = rec.and_then(|r| r.get("round")).and_then(Value::as_u64).unwrap_or(0);
        match card.column.as_str() {
            "Planning" | "Backlog" => Some(needed(round)),
            "Blocked" if rec.is_some() && !was_started => Some(needed(round)),
            _ => None,
        }
    } else {
        None
    };
    if let Some(v) = new {
        card.fields.insert(CRITIQUE_KEY.into(), v);
        changed.push(CRITIQUE_KEY.into());
    }
}

/// The column every finished card goes to. It isn't drawn on the board any more (card #83):
/// a card that reaches it is archived at once, and shows in the board's Archive.
pub const DONE: &str = "Done";

/// Where Restore puts a card archived in Done: the column it came from (`archivedFrom`), or
/// Deploy complete when that is unknown, Done or Ship it (a restore never starts a ship).
pub(crate) fn restore_column(card: &Card) -> String {
    match card.fields.get("archivedFrom").and_then(Value::as_str) {
        Some(c) if COLUMNS.contains(&c) && c != DONE && c != "Ship it" => c.to_string(),
        _ => "Deploy complete".to_string(),
    }
}

/// The patch that restores an archived card: in Done, it goes back to `restore_column`.
pub(crate) fn restore_patch(card: &Card) -> Patch {
    let mut p = Patch::new();
    p.insert("archived".into(), json!(false));
    p.insert("archivedAt".into(), Value::Null);
    if card.column == DONE {
        p.insert("column".into(), json!(restore_column(card)));
        p.insert("archivedFrom".into(), Value::Null);
        p.insert("order".into(), Value::Null);
    }
    p
}

/// Writes `card` over the row at `expected` version and records the revision.
/// A card this write leaves in Done is archived in the same write (card #83), with the column it
/// came from in `archivedFrom`; a stack's lead takes its children into the archive. So a card is
/// never in Done unarchived (the board draws no Done column), whatever the write was.
pub(crate) fn write_tx(tx: &Transaction, card: &mut Card, expected: i64, changed: &[String], by: &str) -> Result<()> {
    let at = now();
    let mut changed = changed.to_vec();
    // A change of what it waits on is checked against the board as it is now (card #109).
    if changed.iter().any(|k| k == BLOCKED_KEY) {
        let old = get_tx(tx, &card.id)?.map(|c| c.blocked_by()).unwrap_or_default();
        waits::check_waits(tx, card, &old)?;
    }
    let mut archived_now = false;
    if card.column == DONE && !card.archived {
        let from = get_tx(tx, &card.id)?.map(|c| c.column).filter(|c| c != DONE);
        let mut p = Patch::new();
        p.insert("archived".into(), json!(true));
        p.insert("archivedAt".into(), json!(at));
        p.insert("archivedBy".into(), json!(by));
        p.insert("archivedFrom".into(), from.map_or(Value::Null, |c| json!(c)));
        for k in apply(card, &p)? {
            if !changed.contains(&k) {
                changed.push(k);
            }
        }
        archived_now = true;
    }
    // An ask from the Router to archive the card goes once it no longer holds (card #131): the
    // card changed column, was archived or restored, or joined a stack.
    if card.fields.contains_key(crate::ARCHIVE_ASK_KEY) {
        if let Some(old) = get_tx(tx, &card.id)? {
            if crate::archive_ask::stale(&old, card) {
                card.fields.remove(crate::ARCHIVE_ASK_KEY);
                if !changed.iter().any(|k| k == crate::ARCHIVE_ASK_KEY) {
                    changed.push(crate::ARCHIVE_ASK_KEY.into());
                }
            }
        }
    }
    let changed = &changed[..];
    card.fields.insert("updatedAt".into(), json!(at));
    card.fields.insert("updatedBy".into(), json!(by));
    done_when::fill_pending(&mut card.fields, by, &at); // card #105: who marked each check
    let n = tx.execute(
        "UPDATE cards SET col = ?1, ord = ?2, ord_col = ?3, archived = ?4, fields = ?5,
                updated_at = ?6, version = version + 1
         WHERE id = ?7 AND version = ?8",
        params![
            card.column,
            card.order,
            card.order_col,
            card.archived as i64,
            serde_json::to_string(&card.fields)?,
            at,
            card.id,
            expected
        ],
    )?;
    if n != 1 {
        let current = get_tx(tx, &card.id)?.map(|c| c.version).unwrap_or(0);
        return Err(Error::Conflict { id: card.id.clone(), yours: expected, current });
    }
    card.version = expected + 1;
    let mut values = Map::new();
    for k in changed {
        values.insert(k.clone(), card.to_document(&[]).get(k).cloned().unwrap_or(Value::Null));
    }
    tx.execute(
        "INSERT INTO card_revisions (card_id, version, at, by, changed) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![card.id, card.version, at, by, serde_json::to_string(&values)?],
    )?;
    if archived_now {
        log_tx(tx, &card.id, &at, "Archived: it reached Done", Some(by))?;
        if card.is_lead() {
            Store::stack_archive_children(tx, card, true, by)?;
        }
    }
    Ok(())
}

pub(crate) fn log_tx(tx: &rusqlite::Connection, id: &str, at: &str, text: &str, by: Option<&str>) -> Result<()> {
    tx.execute(
        "INSERT INTO card_log (card_id, at, text, by) VALUES (?1, ?2, ?3, ?4)",
        params![id, at, text, by],
    )?;
    Ok(())
}

impl Store {
    pub(crate) fn begin(&mut self) -> Result<Transaction<'_>> {
        Ok(self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?)
    }

    pub fn get(&self, id: &str) -> Result<Card> {
        get_tx(&self.conn, id)?.ok_or_else(|| Error::NotFound(id.to_string()))
    }

    pub fn get_by_number(&self, number: i64) -> Result<Card> {
        self.conn
            .query_row("SELECT * FROM cards WHERE number = ?1", [number], row_to_card)
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("#{number}")))
    }

    pub fn list(&self, include_archived: bool) -> Result<Vec<Card>> {
        let sql = if include_archived {
            "SELECT * FROM cards ORDER BY number"
        } else {
            "SELECT * FROM cards WHERE archived = 0 ORDER BY number"
        };
        let mut st = self.conn.prepare(sql)?;
        let cards = st.query_map([], row_to_card)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(cards)
    }

    /// When the card last moved into `column`: the time of its newest revision that set that
    /// column (card #106: a card's step progress counts from its last move into In progress).
    /// None when no revision did (a card made there, or an import, records no move).
    pub fn entered_at(&self, id: &str, column: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT at FROM card_revisions WHERE card_id = ?1
                   AND CASE WHEN json_valid(changed) THEN json_extract(changed, '$.column') END = ?2
                 ORDER BY seq DESC LIMIT 1",
                params![id, column],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Every log line on a card, newest first.
    pub fn log(&self, id: &str) -> Result<Vec<LogEntry>> {
        let mut st = self
            .conn
            .prepare("SELECT at, text, by FROM card_log WHERE card_id = ?1 ORDER BY seq DESC")?;
        let rows = st
            .query_map([id], |r| Ok(LogEntry { at: r.get(0)?, text: r.get(1)?, by: r.get(2)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn revision_count(&self, id: &str) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM card_revisions WHERE card_id = ?1", [id], |r| r.get(0))?)
    }

    /// Creates a card with the next number, taken in the same transaction.
    /// `id` defaults to `card-<number>`; the column defaults to Backlog.
    pub fn create(&mut self, id: Option<&str>, mut fields: Patch, by: &str) -> Result<Card> {
        check_secrets(&fields)?;
        crate::stacks::check_no_stack_fields(&fields)?;
        for k in READ_ONLY.iter().chain(["log"].iter()) {
            if fields.contains_key(*k) {
                return Err(Error::Invalid(format!("\"{k}\" is set by the board")));
            }
        }
        let column = match fields.remove("column") {
            Some(Value::String(c)) => c,
            None => "Backlog".to_string(),
            Some(_) => return Err(Error::Invalid("column must be a string".into())),
        };
        check_column(&column)?;
        let archived = column == DONE;
        if archived {
            fields.insert("archived".into(), json!(true));
            fields.insert("archivedAt".into(), json!(now()));
            fields.insert("archivedBy".into(), json!(by));
        }
        if let Some(v) = fields.remove("labels") {
            if let Some(l) = clean_labels(&v)? {
                fields.insert("labels".into(), l);
            }
        }
        if let Some(v) = fields.remove(AUTO_KEY) {
            if let Some(t) = clean_auto_to(&v)? {
                fields.insert(AUTO_KEY.into(), t);
            }
        }
        if let Some(v) = fields.remove(OUTPUTS_KEY) {
            if let Some(o) = clean_changes_outputs(&v)? {
                fields.insert(OUTPUTS_KEY.into(), o);
            }
        }
        if let Some(v) = fields.remove(SAVED_KEY) {
            if let Some(s) = clean_changes_saved(&v)? {
                fields.insert(SAVED_KEY.into(), s);
            }
        }
        if let Some(v) = fields.remove(DONE_WHEN_KEY) {
            if let Some(l) = clean_done_when(&v)? {
                fields.insert(DONE_WHEN_KEY.into(), l);
            }
        }
        if let Some(v) = fields.remove(DONE_WHEN_CHECKS_KEY) {
            if let Some(l) = clean_done_when_checks(&v)? {
                fields.insert(DONE_WHEN_CHECKS_KEY.into(), done_when::stamp(l, None, &now()));
            }
        }
        for key in [BLOCKED_KEY, WAITED_KEY] {
            if let Some(v) = fields.remove(key) {
                let clean = if key == BLOCKED_KEY { waits::clean_blocked_by(&v)? } else { waits::clean_waited_on(&v)? };
                if let Some(l) = clean {
                    fields.insert(key.into(), l);
                }
            }
        }
        let tx = self.begin()?;
        let number: i64 = tx
            .query_row("SELECT value FROM meta WHERE key = 'next_number'", [], |r| r.get::<_, String>(0))?
            .parse()
            .map_err(|_| Error::Invalid("next_number is not a number".into()))?;
        let id = id.map(str::to_string).unwrap_or_else(|| format!("card-{number}"));
        let waits_on = fields.get(BLOCKED_KEY).map(|_| Card {
            id: id.clone(),
            number,
            column: column.clone(),
            order: None,
            order_col: None,
            archived,
            fields: fields.clone(),
            version: 0,
        });
        if let Some(c) = &waits_on {
            waits::check_waits(&tx, c, &[])?;
        }
        let at = now();
        fields.insert("createdAt".into(), json!(at));
        fields.insert("createdBy".into(), json!(by));
        fields.insert("updatedAt".into(), json!(at));
        fields.insert("updatedBy".into(), json!(by));
        done_when::fill_pending(&mut fields, by, &at);
        tx.execute(
            "INSERT INTO cards (id, number, col, archived, fields, created_at, updated_at, version)
             VALUES (?1, ?2, ?3, ?6, ?4, ?5, ?5, 1)",
            params![id, number, column, serde_json::to_string(&fields)?, at, archived as i64],
        )?;
        tx.execute(
            "UPDATE meta SET value = ?1 WHERE key = 'next_number'",
            [(number + 1).to_string()],
        )?;
        // The column it starts in goes in the create revision too, so its time in that column
        // is known (card #111, times.rs). Nothing else reads this JSON.
        tx.execute(
            "INSERT INTO card_revisions (card_id, version, at, by, changed) VALUES (?1, 1, ?2, ?3, ?4)",
            params![id, at, by, serde_json::to_string(&json!({ "created": true, "column": column }))?],
        )?;
        if archived {
            log_tx(&tx, &id, &at, "Archived: it was made in Done", Some(by))?;
        }
        if let Some(line) = waits_on.and_then(|c| waits::change_line(&[], &c.blocked_by())) {
            log_tx(&tx, &id, &at, &line, Some(by))?;
        }
        let card = get_tx(&tx, &id)?.ok_or_else(|| Error::NotFound(id.clone()))?;
        tx.commit()?;
        Ok(card)
    }

    /// Merges `patch` into the card, if it is still at `if_version`.
    pub fn update(&mut self, id: &str, if_version: i64, patch: &Patch, by: &str) -> Result<Card> {
        check_secrets(patch)?;
        crate::stacks::check_no_stack_fields(patch)?;
        let tx = self.begin()?;
        let mut card = get_tx(&tx, id)?.ok_or_else(|| Error::NotFound(id.to_string()))?;
        if card.version != if_version {
            return Err(Error::Conflict { id: id.into(), yours: if_version, current: card.version });
        }
        let old_waits = card.blocked_by();
        let changed = apply(&mut card, patch)?;
        if changed.is_empty() {
            return Ok(card);
        }
        write_tx(&tx, &mut card, if_version, &changed, by)?;
        if let Some(line) = waits::change_line(&old_waits, &card.blocked_by()) {
            log_tx(&tx, id, &now(), &line, Some(by))?;
        }
        tx.commit()?;
        Ok(card)
    }

    pub fn add_log(&mut self, id: &str, text: &str, by: &str) -> Result<()> {
        if crate::looks_secret(text) {
            return Err(Error::Secret("log".into()));
        }
        let tx = self.begin()?;
        get_tx(&tx, id)?.ok_or_else(|| Error::NotFound(id.to_string()))?;
        log_tx(&tx, id, &now(), text, Some(by))?;
        tx.commit()?;
        Ok(())
    }

    /// Hugh moves a card (drag, column picker or command bar), with the page's
    /// rules for Ship it and Planning. `extra` carries the new order, if any. A move of his
    /// anywhere but Planning or In progress turns the card's auto-progress off (card #76).
    pub fn move_by_hugh(&mut self, id: &str, if_version: i64, to: &str, extra: Patch) -> Result<MoveOutcome> {
        self.move_as(id, if_version, to, extra, How::Hugh)
    }

    /// The app moves a card on for Hugh, because he set it to go on by itself (`autoTo`,
    /// card #76): the same rules as his own move (a plan or ship request), logged as the
    /// app's with `why`, and its auto-progress kept. A ship asked for this way has `"auto": true`.
    pub fn move_for_hugh(&mut self, id: &str, if_version: i64, to: &str, why: &str) -> Result<MoveOutcome> {
        self.move_as(id, if_version, to, Patch::new(), How::Auto(why))
    }

    /// The same, writing `extra` in the same change (card #104: the small-change record goes
    /// with the move into Ship it, so neither is ever written without the other).
    pub fn move_for_hugh_with(&mut self, id: &str, if_version: i64, to: &str, why: &str, extra: Patch) -> Result<MoveOutcome> {
        self.move_as(id, if_version, to, extra, How::Auto(why))
    }

    /// The app moves a card Hugh had put in Blocked to Planning for him, because every card it
    /// waited on (`waited_on`) is ready (card #109, `Store::settle_waits`): the same rules as his
    /// own move, logged as the app's, its auto-progress kept, with `plan.waitedOn`. Its waits
    /// move to `waitedOn`, watched in case one goes back.
    pub fn move_after_waits(&mut self, id: &str, if_version: i64, waited_on: &[i64]) -> Result<MoveOutcome> {
        let card = self.get(id)?;
        if card.column != "Blocked" {
            return Err(Error::Invalid(format!("#{} isn't in Blocked", card.number)));
        }
        let watch = waits::watch_with(&card, waited_on);
        let mut extra = Patch::new();
        extra.insert(BLOCKED_KEY.into(), Value::Null);
        extra.insert(WAITED_KEY.into(), json!(watch));
        self.move_as(id, if_version, "Planning", extra, How::Waits(waited_on))
    }

    fn move_as(&mut self, id: &str, if_version: i64, to: &str, extra: Patch, how: How) -> Result<MoveOutcome> {
        check_column(to)?;
        check_secrets(&extra)?;
        let auto = match how {
            How::Auto(why) => Some(why),
            _ => None,
        };
        let by = match how {
            How::Hugh => HUGH,
            How::Auto(_) => AUTO_BY,
            How::Waits(_) => WAIT_BY,
        };
        let tx = self.begin()?;
        let mut card = get_tx(&tx, id)?.ok_or_else(|| Error::NotFound(id.to_string()))?;
        if card.version != if_version {
            return Err(Error::Conflict { id: id.into(), yours: if_version, current: card.version });
        }
        if card.archived {
            return Err(Error::Invalid("an archived card can't be moved; restore it first".into()));
        }
        let from = card.column.clone();
        if from == to {
            return Err(Error::Invalid(format!("card is already in {to}")));
        }
        if to == DONE && from == "Ship it" && card.state("ship") == Some("shipping") {
            return Err(Error::Invalid(format!("#{} is shipping: archive it once the ship has finished", card.number)));
        }
        if let (true, Some(n)) = (to == "Ship it", card.stack_of()) {
            return Err(Error::Invalid(format!("#{} ships with its stack: move stack #{n} into Ship it", card.number)));
        }
        let at = now();
        let mut patch = extra;
        let mut texts = Vec::new();
        let into_ship = to == "Ship it";
        let into_plan = to == "Planning";
        if into_ship {
            patch.insert("shipRequestedAt".into(), json!(at));
            let mut ship = json!({ "state": "requested", "requestedAt": at, "retries": card.retries() });
            if auto.is_some() {
                ship["auto"] = json!(true);
            }
            patch.insert("ship".into(), ship);
            texts.push("Ship requested");
        }
        if from == "Ship it" && matches!(card.state("ship"), Some("requested" | "queued")) {
            patch.insert("shipRequestedAt".into(), Value::Null);
            patch.insert("ship".into(), json!({ "state": "cancelled", "cancelledAt": at, "retries": card.retries() }));
            texts.push("Ship request cancelled");
        }
        if into_plan {
            patch.insert("planRequestedAt".into(), json!(at));
            let mut plan = json!({ "state": "requested", "requestedAt": at });
            if auto.is_some() {
                plan["auto"] = json!(true);
            }
            if let How::Waits(ns) = how {
                plan[WAITED_KEY] = json!(ns);
            }
            patch.insert("plan".into(), plan);
            texts.push("Plan requested");
        }
        if from == "Planning" && card.state("plan") == Some("requested") {
            patch.insert("planRequestedAt".into(), Value::Null);
            patch.insert("plan".into(), json!({ "state": "cancelled", "cancelledAt": at }));
            texts.push("Plan request cancelled");
        }
        let auto_off = match (how, card.auto_to()) {
            (How::Hugh, Some(t)) if !AUTO_KEEPS.contains(&to) => Some(t == to),
            _ => None,
        };
        if let Some(arrived) = auto_off {
            patch.insert(AUTO_KEY.into(), Value::Null);
            texts.push(if arrived { "Auto-progress done: Hugh moved it there himself" } else { "Auto-progress off: Hugh moved it himself" });
        }
        patch.insert("column".into(), json!(to));
        if matches!(how, How::Hugh) {
            patch.insert("movedByHand".into(), json!(true));
        }
        let changed = apply(&mut card, &patch)?;
        write_tx(&tx, &mut card, if_version, &changed, by)?;
        let line = match how {
            How::Hugh => format!("Hugh moved it from {from} to {to}"),
            How::Auto(why) => format!("The app moved it from {from} to {to} for Hugh: {why}"),
            How::Waits(ns) => format!("The app moved it from {from} to {to} for Hugh: the cards it waited on are ready ({})", waits::nums(ns)),
        };
        log_tx(&tx, id, &at, &line, Some(by))?;
        if !texts.is_empty() {
            log_tx(&tx, id, &at, &texts.join("; "), Some(by))?;
        }
        tx.commit()?;
        Ok(MoveOutcome { card, ship: into_ship, plan: into_plan })
    }

    /// Archives (`on`) or restores a card. The row and its log stay.
    pub fn archive(&mut self, id: &str, if_version: i64, on: bool, by: &str) -> Result<Card> {
        let mut patch = Patch::new();
        let at = now();
        if on {
            patch.insert("archived".into(), json!(true));
            patch.insert("archivedAt".into(), json!(at));
            patch.insert("archivedBy".into(), json!(by));
        }
        let tx = self.begin()?;
        let mut card = get_tx(&tx, id)?.ok_or_else(|| Error::NotFound(id.to_string()))?;
        if card.version != if_version {
            return Err(Error::Conflict { id: id.into(), yours: if_version, current: card.version });
        }
        if card.archived == on {
            return Ok(card);
        }
        let from = card.column.clone();
        if !on {
            patch = restore_patch(&card);
        }
        let changed = apply(&mut card, &patch)?;
        write_tx(&tx, &mut card, if_version, &changed, by)?;
        if card.is_lead() {
            Store::stack_archive_children(&tx, &card, on, by)?;
        }
        let text = match (on, by) {
            (true, "Hugh (board)") => "Hugh archived it".to_string(),
            (false, "Hugh (board)") => "Hugh restored it".to_string(),
            (true, _) => format!("Archived by {by}"),
            (false, _) => format!("Restored by {by}"),
        };
        let text = if card.column != from { format!("{text}, back to {}", card.column) } else { text };
        log_tx(&tx, id, &at, &text, Some(by))?;
        tx.commit()?;
        Ok(card)
    }
}

/// Who a move is made by, inside `move_as`: Hugh himself, the app for his "Take it to" (with
/// why), or the app because the cards a card waited on are ready (card #109).
#[derive(Clone, Copy)]
enum How<'a> {
    Hugh,
    Auto(&'a str),
    Waits(&'a [i64]),
}

pub(crate) fn core_field(k: &str) -> bool {
    CORE.contains(&k)
}
