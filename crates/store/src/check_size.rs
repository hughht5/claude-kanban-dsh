//! How long a Human test card takes to check (card #103). The app works it out from the card's
//! diff and records it as the card field `checkSize`; every move into Human test removes it
//! (`RESET_ON_HUMAN_TEST` in cards.rs), so the app's loop works it out afresh. This is the loop's
//! cheap question: which cards need one?

use crate::{Card, Result, Store};

impl Store {
    /// Cards in Human test, not archived and not a stack's child, that have no `checkSize`
    /// yet, lowest number first, at most `limit`. One query; the loop asks every few seconds.
    pub fn check_size_due(&self, limit: usize) -> Result<Vec<Card>> {
        let ids: Vec<String> = {
            let mut st = self.conn.prepare(
                "SELECT id FROM cards WHERE col = 'Human test' AND archived = 0
                   AND json_extract(fields, '$.checkSize') IS NULL
                   AND json_extract(fields, '$.stackOf') IS NULL
                 ORDER BY number LIMIT ?1",
            )?;
            let rows = st.query_map([limit as i64], |r| r.get::<_, String>(0))?;
            rows.collect::<std::result::Result<_, _>>()?
        };
        ids.iter().map(|id| self.get(id)).collect()
    }

    /// Stack leads in Human test that have a `checkSize`: the app checks whether the stack's
    /// cards changed since it was worked out (a card joined or left while the lead waited).
    pub fn check_size_leads(&self) -> Result<Vec<Card>> {
        let ids: Vec<String> = {
            let mut st = self.conn.prepare(
                "SELECT id FROM cards WHERE col = 'Human test' AND archived = 0
                   AND json_extract(fields, '$.checkSize') IS NOT NULL
                   AND json_type(fields, '$.stack') = 'array'
                 ORDER BY number",
            )?;
            let rows = st.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<std::result::Result<_, _>>()?
        };
        ids.iter().map(|id| self.get(id)).collect()
    }
}
