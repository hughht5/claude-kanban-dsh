//! Importing the Work Board artifact's `cards` export, and checking the copy
//! against it field by field.

use crate::cards::{core_field, get_tx, log_tx};
use crate::secret::find_secret;
use crate::{now, Error, Result, Store, COLUMNS};
use rusqlite::{params, TransactionBehavior};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Card fields the app reads. Anything else in an export is kept as it is and listed.
const USED: &[&str] = &[
    "title", "column", "number", "order", "orderCol", "repo", "issue", "pr", "branch", "worktreePath",
    "devStack", "persona", "account", "summary", "nextStep", "blockedReason", "links", "log",
    "logins", "movedByHand", "ship", "shipRequestedAt", "plan", "planRequestedAt", "dryRun",
    "archived", "archivedAt", "archivedBy", "createdAt", "createdBy", "updatedAt", "updatedBy",
    "modelOverride", "effortOverride", "permissionOverride", "accountPin",
];

#[derive(Debug, Serialize)]
pub struct ImportReport {
    pub imported: usize,
    pub replaced: usize,
    pub next_number: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct FieldDiff {
    pub card: String,
    pub field: String,
    pub export: Value,
    pub board: Value,
}

#[derive(Debug, Serialize)]
pub struct CheckReport {
    pub cards: usize,
    pub archived: usize,
    pub by_column: BTreeMap<String, usize>,
    pub missing_on_board: Vec<String>,
    pub only_on_board: Vec<String>,
    pub diffs: Vec<FieldDiff>,
    /// Fields the app doesn't use, kept as they were, with how many cards hold each.
    pub kept_unused: BTreeMap<String, usize>,
}

impl CheckReport {
    pub fn clean(&self) -> bool {
        self.missing_on_board.is_empty() && self.only_on_board.is_empty() && self.diffs.is_empty()
    }
}

/// Reads every `<id>.json` document in `dir`, sorted by id.
fn read_export(dir: &Path) -> Result<Vec<(String, Map<String, Value>)>> {
    let mut docs = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| Error::Invalid(format!("bad file name {}", path.display())))?
            .to_string();
        let text = std::fs::read_to_string(&path)?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|e| Error::Invalid(format!("{id}: not JSON ({e})")))?;
        // Accept both a bare document and the tool's {id, data, version} wrapper.
        let doc = match value {
            Value::Object(mut m) if m.contains_key("data") && m.get("data").map_or(false, Value::is_object) => {
                match m.remove("data") {
                    Some(Value::Object(d)) => d,
                    _ => unreachable!(),
                }
            }
            Value::Object(m) => m,
            _ => return Err(Error::Invalid(format!("{id}: not a JSON object"))),
        };
        docs.push((id, doc));
    }
    docs.sort_by(|a, b| a.0.cmp(&b.0));
    if docs.is_empty() {
        return Err(Error::Invalid(format!("no card documents in {}", dir.display())));
    }
    Ok(docs)
}

/// Every reason a document can't be imported, or nothing.
fn problems(id: &str, doc: &Map<String, Value>) -> Vec<String> {
    let mut out = Vec::new();
    match doc.get("number").and_then(Value::as_i64) {
        Some(n) if n > 0 => {}
        _ => out.push(format!("{id}: number is missing or not a positive whole number")),
    }
    match doc.get("column").and_then(Value::as_str) {
        Some(c) if COLUMNS.contains(&c) => {}
        other => out.push(format!("{id}: column {other:?} is not one of the seven")),
    }
    if !doc.get("title").map_or(false, Value::is_string) {
        out.push(format!("{id}: title is missing"));
    }
    if let Some(log) = doc.get("log") {
        let ok = log.as_array().map_or(false, |a| {
            a.iter().all(|e| e.get("at").map_or(false, Value::is_string) && e.get("text").map_or(false, Value::is_string))
        });
        if !ok {
            out.push(format!("{id}: log is not a list of {{at, text}} entries"));
        }
    }
    for (k, v) in doc {
        if let Some(path) = find_secret(k, v) {
            out.push(format!("{id}: {path} looks like a secret"));
        }
    }
    out
}

fn split(doc: &Map<String, Value>) -> (Map<String, Value>, Vec<(String, String)>) {
    let fields: Map<String, Value> = doc
        .iter()
        .filter(|(k, _)| !core_field(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let log = doc
        .get("log")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|e| {
                    (
                        e["at"].as_str().unwrap_or_default().to_string(),
                        e["text"].as_str().unwrap_or_default().to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    (fields, log)
}

impl Store {
    /// Imports an export folder in one transaction: every card goes in, or none does.
    /// Cards already on the board are refused unless `replace` is set.
    pub fn import_dir(&mut self, dir: &Path, replace: bool) -> Result<ImportReport> {
        let docs = read_export(dir)?;
        let mut errors: Vec<String> = docs.iter().flat_map(|(id, d)| problems(id, d)).collect();
        let mut numbers = BTreeMap::new();
        for (id, d) in &docs {
            if let Some(n) = d.get("number").and_then(Value::as_i64) {
                if let Some(other) = numbers.insert(n, id.clone()) {
                    errors.push(format!("{id}: number {n} is also on {other}"));
                }
            }
        }
        if !errors.is_empty() {
            return Err(Error::Invalid(format!("import refused, nothing written:\n{}", errors.join("\n"))));
        }

        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let at = now();
        let (mut imported, mut replaced) = (0, 0);
        for (id, doc) in &docs {
            let (fields, log) = split(doc);
            let number = doc["number"].as_i64().unwrap_or_default();
            let column = doc["column"].as_str().unwrap_or("Backlog");
            let order = doc.get("order").and_then(Value::as_f64);
            let order_col = doc.get("orderCol").and_then(Value::as_str);
            let archived = doc.get("archived").and_then(Value::as_bool).unwrap_or(false);
            let created = fields.get("createdAt").and_then(Value::as_str);
            let updated = fields.get("updatedAt").and_then(Value::as_str);
            let fields_json = serde_json::to_string(&fields)?;
            let existing_log: BTreeSet<(String, String)> = match get_tx(&tx, id)? {
                Some(card) => {
                    if !replace {
                        return Err(Error::Invalid(format!(
                            "card {id} is already on the board; pass --replace to overwrite it"
                        )));
                    }
                    if card.number != number {
                        return Err(Error::Invalid(format!(
                            "card {id} is #{} on the board but #{number} in the export", card.number
                        )));
                    }
                    tx.execute(
                        "UPDATE cards SET col = ?1, ord = ?2, ord_col = ?3, archived = ?4, fields = ?5,
                                created_at = ?6, updated_at = ?7, version = version + 1 WHERE id = ?8",
                        params![column, order, order_col, archived as i64, fields_json, created, updated, id],
                    )?;
                    let version: i64 = tx.query_row("SELECT version FROM cards WHERE id = ?1", [id], |r| r.get(0))?;
                    tx.execute(
                        "INSERT INTO card_revisions (card_id, version, at, by, changed) VALUES (?1, ?2, ?3, 'import --replace', ?4)",
                        params![id, version, at, serde_json::to_string(&json!({ "replaced": true }))?],
                    )?;
                    replaced += 1;
                    let mut st = tx.prepare("SELECT at, text FROM card_log WHERE card_id = ?1")?;
                    let rows = st
                        .query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                        .collect::<rusqlite::Result<BTreeSet<_>>>()?;
                    rows
                }
                None => {
                    tx.execute(
                        "INSERT INTO cards (id, number, col, ord, ord_col, archived, fields, created_at, updated_at, version)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1)",
                        params![id, number, column, order, order_col, archived as i64, fields_json, created, updated],
                    )?;
                    tx.execute(
                        "INSERT INTO card_revisions (card_id, version, at, by, changed) VALUES (?1, 1, ?2, 'import', ?3)",
                        params![id, at, serde_json::to_string(&json!({ "imported": true }))?],
                    )?;
                    imported += 1;
                    BTreeSet::new()
                }
            };
            // The export lists newest first; insert oldest first so sequence order is time order.
            for (when, text) in log.iter().rev() {
                if !existing_log.contains(&(when.clone(), text.clone())) {
                    log_tx(&tx, id, when, text, None)?;
                }
            }
        }
        let max: i64 = tx.query_row("SELECT COALESCE(MAX(number), 0) FROM cards", [], |r| r.get(0))?;
        let current: i64 = tx
            .query_row("SELECT value FROM meta WHERE key = 'next_number'", [], |r| r.get::<_, String>(0))?
            .parse()
            .unwrap_or(1);
        let next = current.max(max + 1);
        tx.execute("UPDATE meta SET value = ?1 WHERE key = 'next_number'", [next.to_string()])?;
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('imported_from', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [format!("{} at {at}", dir.display())],
        )?;
        tx.commit()?;
        Ok(ImportReport { imported, replaced, next_number: next })
    }

    /// Compares the board with an export folder: counts, then every field of every card.
    pub fn check_dir(&self, dir: &Path) -> Result<CheckReport> {
        let docs = read_export(dir)?;
        let mut report = CheckReport {
            cards: 0,
            archived: 0,
            by_column: BTreeMap::new(),
            missing_on_board: Vec::new(),
            only_on_board: Vec::new(),
            diffs: Vec::new(),
            kept_unused: BTreeMap::new(),
        };
        let export_ids: BTreeSet<&str> = docs.iter().map(|(id, _)| id.as_str()).collect();
        for card in self.list(true)? {
            if !export_ids.contains(card.id.as_str()) {
                report.only_on_board.push(card.id.clone());
            }
        }
        for (id, doc) in &docs {
            let card = match self.get(id) {
                Ok(c) => c,
                Err(Error::NotFound(_)) => {
                    report.missing_on_board.push(id.clone());
                    continue;
                }
                Err(e) => return Err(e),
            };
            report.cards += 1;
            if card.archived {
                report.archived += 1;
            } else {
                *report.by_column.entry(card.column.clone()).or_default() += 1;
            }
            let log = self.log(id)?;
            let board = card.to_document(&log);
            let board = board.as_object().cloned().unwrap_or_default();
            let keys: BTreeSet<&String> = doc.keys().chain(board.keys()).collect();
            for k in keys {
                if !USED.contains(&k.as_str()) && doc.contains_key(k) {
                    *report.kept_unused.entry(k.clone()).or_default() += 1;
                }
                let a = doc.get(k).cloned().unwrap_or(Value::Null);
                let b = board.get(k).cloned().unwrap_or(Value::Null);
                if !same(&a, &b) {
                    report.diffs.push(FieldDiff { card: id.clone(), field: k.clone(), export: a, board: b });
                }
            }
        }
        Ok(report)
    }
}

/// JSON equality that treats 3 and 3.0 as the same number.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).map_or(false, |w| same(v, w)))
        }
        _ => a == b,
    }
}
