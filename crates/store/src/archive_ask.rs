//! The Router's asks to archive a card, and Hugh's answer (card #131).
//!
//! The Router never archives. It asks (`ask_archive`): the card gets `archiveAsk`, and the board
//! puts the card in its Done pile. Hugh's Archive all (`approve_archive`: his Archive drop, a
//! move into Done, which archives it) clears the pile; Not now on a card (`dismiss_archive`)
//! puts it back. Every rule is here, over the store, so it is tested without the app; the
//! Router's tool and the board's commands only wrap these and add the app's part (events, the
//! after-move dispatch, stopping sessions).
//!
//! - An ask is refused in Ship it (Hugh's ship), for a stack's child (ask for its lead), and for a
//!   lead with a child in Ship it.
//! - An ask goes when the card changes column, is archived or restored, or joins a stack
//!   (`stale`, applied in `cards::write_tx`), so an old ask never archives a card sent back to work.
//! - After Not now, a new ask is refused until Hugh or the card's own session changes the card.

use crate::cards::{Card, Patch, DONE};
use crate::{Error, Result, Store, ROUTER_BY};
use rusqlite::params;
use serde::Serialize;
use serde_json::{json, Value};

/// The Router's ask: {reason, evidence, at, by, column}.
pub const ARCHIVE_ASK_KEY: &str = "archiveAsk";
/// Hugh's Not now: {at, version}, the card's version after that write.
pub const ARCHIVE_DECLINED_KEY: &str = "archiveAskDeclined";
/// The longest reason or evidence kept, in characters.
pub const ASK_TEXT_MAX: usize = 400;

/// One card's answer to Hugh's click.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ArchiveResult {
    pub id: String,
    pub number: i64,
    /// Archived (moved into Done).
    pub done: bool,
    /// Why not, when it wasn't.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The column it came from, when it was archived.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// For a lead: the ids of the children archived with it (their sessions are the app's to stop).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<String>,
}

/// Why a card may not be archived through an ask, if it may not: Ship it is Hugh's ship (his own
/// drop holds it back too); a stack's child goes with its lead; a lead takes every child with it,
/// so none may be in Ship it. Every other column is allowed, as for Hugh's drop.
pub fn archive_refused(store: &Store, card: &Card) -> Option<String> {
    if card.column == "Ship it" {
        return Some(format!("card #{} is in Ship it: that is Hugh's ship", card.number));
    }
    if let Some(lead) = card.stack_of() {
        return Some(format!("card #{} is in stack #{lead}; ask for the lead", card.number));
    }
    if card.is_lead() {
        // A lookup that fails refuses: the archive would take every child with it.
        let kids = match store.stack_children(card) {
            Ok(k) => k,
            Err(e) => return Some(format!("couldn't read card #{}'s stack: {e}", card.number)),
        };
        if let Some(k) = kids.iter().find(|k| !k.archived && k.column == "Ship it") {
            return Some(format!("card #{}'s stack has #{} in Ship it: that is Hugh's ship", card.number, k.number));
        }
    }
    None
}

/// Whether an ask on `old` no longer holds once it is written as `new`: its column changed, it
/// was archived or restored, or it joined a stack. Used by `cards::write_tx` on every write.
pub(crate) fn stale(old: &Card, new: &Card) -> bool {
    old.column != new.column || old.archived != new.archived || (new.stack_of().is_some() && old.stack_of().is_none())
}

/// Whether a write by `by` lifts Hugh's Not now on card `number`: only Hugh's own writes and the
/// card's own session's ("card #N's session", also "card #N's session (held until its turn
/// ended)"). The Router, the app, another card's session and a session outside the app don't.
pub fn lifts_decline(by: &str, number: i64) -> bool {
    if by.starts_with("Hugh") {
        return true;
    }
    let own = format!("card #{number}'s session");
    by == own || by.strip_prefix(&own).is_some_and(|rest| rest.starts_with(' '))
}

fn cut(s: &str) -> String {
    let t = s.trim();
    if t.chars().count() <= ASK_TEXT_MAX {
        return t.to_string();
    }
    let mut out: String = t.chars().take(ASK_TEXT_MAX - 1).collect();
    out.push('…');
    out
}

impl Store {
    /// The card's writes after `version`, as (version, by), oldest first.
    pub fn revisions_after(&self, card_id: &str, version: i64) -> Result<Vec<(i64, String)>> {
        let mut st = self.conn.prepare("SELECT version, COALESCE(by, '') FROM card_revisions WHERE card_id = ?1 AND version > ?2 ORDER BY seq")?;
        let rows = st.query_map(params![card_id, version], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// The Router asks to archive card `number` (it never archives itself). Refused for a changed
    /// version, an archived card, `archive_refused`, an empty reason, and while Hugh's Not now
    /// stands. A second ask replaces the first.
    pub fn ask_archive(&mut self, number: i64, if_version: i64, reason: &str, evidence: Option<&str>) -> Result<Card> {
        let card = self.get_by_number(number)?;
        if card.version != if_version {
            return Err(Error::Conflict { id: card.id.clone(), yours: if_version, current: card.version });
        }
        if card.archived {
            return Err(Error::Invalid(format!("card #{number} is archived already")));
        }
        if reason.trim().is_empty() {
            return Err(Error::Invalid("pass a reason: it goes on the card and in Hugh's list".into()));
        }
        if let Some(why) = archive_refused(self, &card) {
            return Err(Error::Invalid(why));
        }
        if let Some(d) = card.fields.get(ARCHIVE_DECLINED_KEY) {
            let at = d.get("version").and_then(Value::as_i64).unwrap_or(0);
            let lifted = self.revisions_after(&card.id, at)?.iter().any(|(_, by)| lifts_decline(by, number));
            if !lifted {
                let when = d.get("at").and_then(Value::as_str).unwrap_or("");
                return Err(Error::Invalid(format!(
                    "Hugh said not now at {when}; ask again only after Hugh or the card's own session changes it"
                )));
            }
        }
        let reason = cut(reason);
        let evidence = evidence.map(cut).filter(|e| !e.is_empty());
        let mut p = Patch::new();
        p.insert(
            ARCHIVE_ASK_KEY.into(),
            json!({ "reason": reason, "evidence": evidence, "at": crate::now(), "by": ROUTER_BY, "column": card.column }),
        );
        let out = self.update(&card.id, if_version, &p, ROUTER_BY)?;
        self.add_log(&card.id, &format!("The Router asks to archive it: {reason}"), ROUTER_BY)?;
        let line = match &evidence {
            Some(e) => format!("Asked Hugh to archive #{number}: {reason} ({e})"),
            None => format!("Asked Hugh to archive #{number}: {reason}"),
        };
        let _ = self.add_router_log("decision", Some(number), &line, ROUTER_BY);
        Ok(out)
    }

    /// Hugh's Archive all: for each card that still has the Router's ask and is still in the
    /// column it was asked in, his move into Done at that version (his Archive drop), which
    /// archives it in the same write. One result per card; a refusal never stops the others.
    pub fn approve_archive(&mut self, items: &[(String, i64)]) -> Vec<ArchiveResult> {
        items.iter().map(|(id, v)| self.approve_one(id, *v)).collect()
    }

    fn approve_one(&mut self, id: &str, version: i64) -> ArchiveResult {
        let card = match self.get(id) {
            Ok(c) => c,
            Err(e) => return ArchiveResult { id: id.into(), number: 0, done: false, why: Some(e.to_string()), from: None, children: vec![] },
        };
        let no = |why: String| ArchiveResult { id: card.id.clone(), number: card.number, done: false, why: Some(why), from: None, children: vec![] };
        let Some(ask) = card.fields.get(ARCHIVE_ASK_KEY).cloned() else {
            return no(format!("card #{} has no ask from the Router any more", card.number));
        };
        if card.archived {
            return no(format!("card #{} is archived already", card.number));
        }
        if ask.get("column").and_then(Value::as_str).is_some_and(|c| c != card.column) {
            return no(format!("card #{} has moved since the Router asked", card.number));
        }
        if let Some(why) = archive_refused(self, &card) {
            return no(why);
        }
        let from = card.column.clone();
        match self.move_by_hugh(&card.id, version, DONE, Patch::new()) {
            Ok(m) => {
                // The children this move archived with it (`archivedWith`), read after the write,
                // so the app stops exactly their sessions.
                let children: Vec<String> = if m.card.is_lead() {
                    self.stack_children(&m.card)
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|k| k.archived && k.fields.get("archivedWith").and_then(Value::as_i64) == Some(card.number))
                        .map(|k| k.id)
                        .collect()
                } else {
                    vec![]
                };
                let _ = self.add_router_log("event", Some(card.number), &format!("Hugh archived #{} as you asked (it was in {from})", card.number), crate::cards::HUGH);
                ArchiveResult { id: card.id.clone(), number: card.number, done: true, why: None, from: Some(from), children }
            }
            Err(Error::Conflict { current, .. }) => no(format!("card #{} changed since the list was drawn (now version {current})", card.number)),
            Err(e) => no(e.to_string()),
        }
    }

    /// Hugh's Not now: the ask goes, and a new one is refused until Hugh or the card's own session
    /// changes the card. One result per card.
    pub fn dismiss_archive(&mut self, items: &[(String, i64)]) -> Vec<ArchiveResult> {
        items.iter().map(|(id, v)| self.dismiss_one(id, *v)).collect()
    }

    fn dismiss_one(&mut self, id: &str, version: i64) -> ArchiveResult {
        let res = (|| -> Result<Card> {
            let card = self.get(id)?;
            if !card.fields.contains_key(ARCHIVE_ASK_KEY) {
                return Err(Error::Invalid(format!("card #{} has no ask from the Router any more", card.number)));
            }
            let mut p = Patch::new();
            p.insert(ARCHIVE_ASK_KEY.into(), Value::Null);
            // The decline names the version this write makes, so any later write by Hugh or the
            // card's own session lifts it. The write always happens (the ask is there to remove)
            // and `write_tx` always moves a card from `version` to `version + 1`.
            p.insert(ARCHIVE_DECLINED_KEY.into(), json!({ "at": crate::now(), "version": version + 1 }));
            let out = self.update(id, version, &p, crate::cards::HUGH)?;
            debug_assert_eq!(out.version, version + 1);
            self.add_log(id, "Hugh said not now to the Router's ask to archive it", crate::cards::HUGH)?;
            let _ = self.add_router_log("event", Some(out.number), &format!("Hugh said not now to archiving #{}", out.number), crate::cards::HUGH);
            Ok(out)
        })();
        match res {
            Ok(c) => ArchiveResult { id: c.id, number: c.number, done: true, why: None, from: None, children: vec![] },
            Err(e) => {
                let number = self.get(id).map(|c| c.number).unwrap_or(0);
                ArchiveResult { id: id.into(), number, done: false, why: Some(e.to_string()), from: None, children: vec![] }
            }
        }
    }
}
