//! Card templates (migration 6): a name, a title prefix and description for the new-card form,
//! and a starting prompt for the card's first session. Archived, never removed.

use crate::{now, secret::looks_like_token, Error, Result, Store};
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub id: String,
    pub name: String,
    pub title_prefix: String,
    pub body: String,
    pub start_prompt: String,
    pub position: i64,
    pub archived: bool,
}

/// What the Settings screen sends to add (no `id`) or change a template.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInput {
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub title_prefix: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub start_prompt: String,
}

const COLS: &str = "id, name, title_prefix, body, start_prompt, position, archived";

fn row(r: &Row) -> rusqlite::Result<Template> {
    Ok(Template {
        id: r.get(0)?,
        name: r.get(1)?,
        title_prefix: r.get(2)?,
        body: r.get(3)?,
        start_prompt: r.get(4)?,
        position: r.get(5)?,
        archived: r.get::<_, i64>(6)? != 0,
    })
}

fn slug(name: &str) -> String {
    let mut s = String::new();
    for ch in name.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            s.push(ch);
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let s = s.trim_end_matches('-').chars().take(40).collect::<String>();
    if s.is_empty() { "template".into() } else { s }
}

impl Store {
    /// The templates in their order; archived ones only when asked for.
    pub fn templates(&self, include_archived: bool) -> Result<Vec<Template>> {
        let sql = format!(
            "SELECT {COLS} FROM templates {} ORDER BY position, name",
            if include_archived { "" } else { "WHERE archived = 0" }
        );
        let mut st = self.conn.prepare(&sql)?;
        let out = st.query_map([], row)?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(out)
    }

    /// One template, archived or not (a card made from it still uses it).
    pub fn template(&self, id: &str) -> Result<Option<Template>> {
        Ok(self
            .conn
            .query_row(&format!("SELECT {COLS} FROM templates WHERE id = ?1"), [id], row)
            .optional()?)
    }

    /// Adds a template (no id: one is made from its name) or changes an existing one.
    pub fn save_template(&mut self, t: &TemplateInput) -> Result<Template> {
        let name = t.name.trim();
        if name.is_empty() {
            return Err(Error::Invalid("give the template a name".into()));
        }
        for (field, v) in [("name", name), ("titlePrefix", &t.title_prefix), ("body", &t.body), ("startPrompt", &t.start_prompt)] {
            if looks_like_token(v) {
                return Err(Error::Secret(field.into()));
            }
        }
        let at = now();
        let id = match t.id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(id) => {
                let n = self.conn.execute(
                    "UPDATE templates SET name = ?1, title_prefix = ?2, body = ?3, start_prompt = ?4, updated_at = ?5 WHERE id = ?6",
                    params![name, t.title_prefix, t.body, t.start_prompt, at, id],
                )?;
                if n == 0 {
                    return Err(Error::Invalid(format!("no template {id}")));
                }
                id.to_string()
            }
            None => {
                let base = slug(name);
                let mut id = base.clone();
                let mut k = 2;
                while self.template(&id)?.is_some() {
                    id = format!("{base}-{k}");
                    k += 1;
                }
                let pos: i64 = self.conn.query_row("SELECT COALESCE(MAX(position), -1) + 1 FROM templates", [], |r| r.get(0))?;
                self.conn.execute(
                    "INSERT INTO templates (id, name, title_prefix, body, start_prompt, position, archived, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?7)",
                    params![id, name, t.title_prefix, t.body, t.start_prompt, pos, at],
                )?;
                id
            }
        };
        self.template(&id)?.ok_or(Error::NotFound(id))
    }

    /// Archives a template (`on`) or restores it. It leaves the new-card picker, but cards
    /// made from it keep their link to it.
    pub fn archive_template(&mut self, id: &str, on: bool) -> Result<Template> {
        let n = self.conn.execute(
            "UPDATE templates SET archived = ?1, updated_at = ?2 WHERE id = ?3",
            params![on as i64, now(), id],
        )?;
        if n == 0 {
            return Err(Error::Invalid(format!("no template {id}")));
        }
        self.template(id)?.ok_or_else(|| Error::NotFound(id.into()))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn slugs() {
        assert_eq!(super::slug("Customer request!"), "customer-request");
        assert_eq!(super::slug("  ***  "), "template");
    }
}
