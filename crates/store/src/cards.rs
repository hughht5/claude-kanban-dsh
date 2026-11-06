use crate::secret::find_secret;
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

/// Where a card can go on to by itself (card #76): Hugh's "Take it to" past Planning. The app
/// moves it on until it gets there (`autoTo`); Planning is only a start, so it isn't kept.
pub const AUTO_TARGETS: [&str; 2] = ["Human test", "Deploy complete"];
/// The card field that holds it.
pub const AUTO_KEY: &str = "autoTo";
/// The card field holding its latest subagent test (card #76): `{state, at, detail, ...}`, where
/// state is "needed" (set on every move into In progress), "passed", "failed" or "unclear".
pub const TEST_KEY: &str = "test";
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
                    // Each time into In progress it needs a fresh subagent test before Human
                    // test (Hugh, card #76): an older verdict was for older work.
                    if c == "In progress" && !patch.contains_key(TEST_KEY) {
                        card.fields.insert(TEST_KEY.into(), json!({ "state": "needed", "at": now() }));
                        changed.push(TEST_KEY.into());
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
    Ok(changed)
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
    let changed = &changed[..];
    card.fields.insert("updatedAt".into(), json!(at));
    card.fields.insert("updatedBy".into(), json!(by));
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
        let tx = self.begin()?;
        let number: i64 = tx
            .query_row("SELECT value FROM meta WHERE key = 'next_number'", [], |r| r.get::<_, String>(0))?
            .parse()
            .map_err(|_| Error::Invalid("next_number is not a number".into()))?;
        let id = id.map(str::to_string).unwrap_or_else(|| format!("card-{number}"));
        let at = now();
        fields.insert("createdAt".into(), json!(at));
        fields.insert("createdBy".into(), json!(by));
        fields.insert("updatedAt".into(), json!(at));
        fields.insert("updatedBy".into(), json!(by));
        tx.execute(
            "INSERT INTO cards (id, number, col, archived, fields, created_at, updated_at, version)
             VALUES (?1, ?2, ?3, ?6, ?4, ?5, ?5, 1)",
            params![id, number, column, serde_json::to_string(&fields)?, at, archived as i64],
        )?;
        tx.execute(
            "UPDATE meta SET value = ?1 WHERE key = 'next_number'",
            [(number + 1).to_string()],
        )?;
        tx.execute(
            "INSERT INTO card_revisions (card_id, version, at, by, changed) VALUES (?1, 1, ?2, ?3, '{\"created\":true}')",
            params![id, at, by],
        )?;
        if archived {
            log_tx(&tx, &id, &at, "Archived: it was made in Done", Some(by))?;
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
        let changed = apply(&mut card, patch)?;
        if changed.is_empty() {
            return Ok(card);
        }
        write_tx(&tx, &mut card, if_version, &changed, by)?;
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
        self.move_as(id, if_version, to, extra, None)
    }

    /// The app moves a card on for Hugh, because he set it to go on by itself (`autoTo`,
    /// card #76): the same rules as his own move (a plan or ship request), logged as the
    /// app's with `why`, and its auto-progress kept. A ship asked for this way has `"auto": true`.
    pub fn move_for_hugh(&mut self, id: &str, if_version: i64, to: &str, why: &str) -> Result<MoveOutcome> {
        self.move_as(id, if_version, to, Patch::new(), Some(why))
    }

    fn move_as(&mut self, id: &str, if_version: i64, to: &str, extra: Patch, auto: Option<&str>) -> Result<MoveOutcome> {
        check_column(to)?;
        check_secrets(&extra)?;
        let by = if auto.is_some() { AUTO_BY } else { "Hugh (board)" };
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
            patch.insert("plan".into(), plan);
            texts.push("Plan requested");
        }
        if from == "Planning" && card.state("plan") == Some("requested") {
            patch.insert("planRequestedAt".into(), Value::Null);
            patch.insert("plan".into(), json!({ "state": "cancelled", "cancelledAt": at }));
            texts.push("Plan request cancelled");
        }
        let auto_off = match (auto, card.auto_to()) {
            (None, Some(t)) if !AUTO_KEEPS.contains(&to) => Some(t == to),
            _ => None,
        };
        if let Some(arrived) = auto_off {
            patch.insert(AUTO_KEY.into(), Value::Null);
            texts.push(if arrived { "Auto-progress done: Hugh moved it there himself" } else { "Auto-progress off: Hugh moved it himself" });
        }
        patch.insert("column".into(), json!(to));
        if auto.is_none() {
            patch.insert("movedByHand".into(), json!(true));
        }
        let changed = apply(&mut card, &patch)?;
        write_tx(&tx, &mut card, if_version, &changed, by)?;
        let line = match auto {
            None => format!("Hugh moved it from {from} to {to}"),
            Some(why) => format!("The app moved it from {from} to {to} for Hugh: {why}"),
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

pub(crate) fn core_field(k: &str) -> bool {
    CORE.contains(&k)
}
