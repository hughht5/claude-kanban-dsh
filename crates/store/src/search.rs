//! The session search index (card #110): a full-text index of the card sessions'
//! transcripts, in its own file (`search.db`) beside the board, so the board's lock,
//! backups and migrations never carry it.
//!
//! - FTS5 with the trigram tokenizer: any run of 3 or more characters matches, ignoring case.
//! - Inserts only. Each month, or when [`INDEX_VERSION`] goes up, a new file is built
//!   ([`SearchIndex::start_new`]) and swapped in ([`swap_in`]); the old one is renamed into
//!   `backups\search`, which keeps one copy.
//! - Secrets and sign-in links are masked ([`mask`]) before anything is stored.
//! - Only the last [`KEEP_DAYS`] days are indexed and found.

use crate::{prune_backups, Error, Result};
use regex::Regex;
use rusqlite::{params, Connection, ErrorCode, OpenFlags, OptionalExtension};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

/// What stops a running search (from [`SearchIndex::interrupt_handle`]).
pub use rusqlite::InterruptHandle;

/// The index's own migrations (never `schemaN.sql`), counted in its own `user_version`.
const SEARCH_MIGRATIONS: [&str; 1] = [include_str!("search1.sql")];

/// Raised when the parser, the mask or the tokenizer changes: a build with a higher number
/// builds a new file; a build with a lower one leaves the file alone.
pub const INDEX_VERSION: i64 = 1;
/// Only lines from the last this many days are indexed and found.
pub const KEEP_DAYS: i64 = 90;
/// A search takes at most this many of the newest matching entries.
pub const MAX_ROWS: usize = 2000;
/// A card's match count stops here ("1,000+").
pub const COUNT_CAP: usize = 1000;
/// At most this much of a transcript is read into one transaction.
pub const MAX_CHUNK: u64 = 4 * 1024 * 1024;
/// An entry's text is cut to this many bytes.
pub const MAX_TEXT: usize = 32 * 1024;
/// `backups\search` keeps this many copies.
pub const KEEP_SEARCH_BACKUPS: usize = 1;
/// Words shorter than this are dropped from a query.
pub const MIN_TERM: usize = 3;

const META_MONTH: &str = "month";
const META_VERSION: &str = "index_version";

/// `<data dir>\search.db`.
pub fn search_path(dir: &Path) -> PathBuf {
    dir.join("search.db")
}

/// `<data dir>\search.new.db`: the next month's file while it is built.
pub fn new_search_path(dir: &Path) -> PathBuf {
    dir.join("search.new.db")
}

/// `<data dir>\backups\search`.
pub fn search_backups_dir(dir: &Path) -> PathBuf {
    dir.join("backups").join("search")
}

/// The month a file is for, as "2026-11" (UTC).
pub fn month_of(now: chrono::DateTime<chrono::Utc>) -> String {
    now.format("%Y-%m").to_string()
}

/// The oldest time a search finds or the indexer keeps: [`KEEP_DAYS`] before `now`, in the
/// transcripts' own format, so a text comparison orders it.
pub fn since(now: chrono::DateTime<chrono::Utc>) -> String {
    (now - chrono::Duration::days(KEEP_DAYS)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// True when `e` is a search stopped by [`SearchIndex::interrupt_handle`].
pub fn is_interrupted(e: &Error) -> bool {
    matches!(e, Error::Db(rusqlite::Error::SqliteFailure(f, _)) if f.code == ErrorCode::OperationInterrupted)
}

/// One transcript file the index knows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IndexedFile {
    pub id: i64,
    pub path: String,
    pub card_id: String,
    /// The Claude Code session id: for a subagent, its parent session's.
    pub session: String,
    /// A subagent's type ("card-tester"); None for a main transcript.
    pub agent: Option<String>,
}

/// An entry to store. Its text is masked and cut by [`SearchIndex::add_chunk`].
#[derive(Debug, Clone, PartialEq)]
pub struct NewEntry {
    pub key: String,
    /// "user", "assistant", "tool", "task" or "cmdout".
    pub kind: String,
    /// A tool's name; empty otherwise.
    pub who: String,
    pub at: String,
    pub text: String,
}

/// A piece of a snippet: `m` is true for a match.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Part {
    pub t: String,
    pub m: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub kind: String,
    pub who: String,
    pub agent: Option<String>,
    /// A subagent's id (from its `agent-<id>.jsonl`), so the panel can show its conversation.
    pub agent_id: Option<String>,
    pub at: String,
    pub session: String,
    pub snippet: Vec<Part>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CardHits {
    pub card_id: String,
    pub count: usize,
    /// The count is a floor: it reached [`COUNT_CAP`], or the search reached [`MAX_ROWS`].
    pub capped: bool,
    pub hits: Vec<Hit>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Found {
    /// The query had no word of [`MIN_TERM`] characters or more.
    pub short: bool,
    pub cards: Vec<CardHits>,
    /// Matching entries taken.
    pub total: usize,
    /// The search reached [`MAX_ROWS`].
    pub limited: bool,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct Stats {
    pub files: usize,
    pub entries: usize,
}

pub struct SearchIndex {
    conn: Connection,
    path: PathBuf,
    writable: bool,
}

impl SearchIndex {
    /// Opens (or creates) the index at `path` and brings its schema up to date, taking a
    /// backup into `backups\search` before migrating an existing file. A new file is for
    /// `month`. A file with a newer schema, or a higher index version, than this build knows is
    /// opened without migrating and left alone ([`SearchIndex::writable`] is false).
    pub fn open(path: impl AsRef<Path>, month: &str) -> Result<SearchIndex> {
        Self::open_with(path.as_ref(), month, &SEARCH_MIGRATIONS)
    }

    fn open_with(path: &Path, month: &str, migrations: &[&str]) -> Result<SearchIndex> {
        let path = path.to_path_buf();
        let existed = path.exists();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // So a REPLACE on this connection would meet the delete triggers too.
        conn.pragma_update(None, "recursive_triggers", "ON")?;
        let mut ix = SearchIndex { conn, path, writable: true };
        let current = ix.schema_version()?;
        if current > migrations.len() {
            ix.writable = false;
            return Ok(ix);
        }
        if current < migrations.len() {
            if existed && current > 0 {
                ix.backup("pre-migration")?;
            }
            for (i, sql) in migrations.iter().enumerate().skip(current) {
                let tx = ix.conn.transaction()?;
                tx.execute_batch(sql)?;
                tx.pragma_update(None, "user_version", (i + 1) as i64)?;
                tx.commit()?;
            }
        }
        if ix.meta(META_MONTH)?.is_none() {
            let tx = ix.conn.transaction()?;
            tx.execute("INSERT OR IGNORE INTO meta (key, value) VALUES (?1, ?2)", params![META_MONTH, month])?;
            tx.execute("INSERT OR IGNORE INTO meta (key, value) VALUES (?1, ?2)", params![META_VERSION, INDEX_VERSION.to_string()])?;
            tx.commit()?;
        }
        if ix.index_version()? > INDEX_VERSION {
            ix.writable = false;
        }
        Ok(ix)
    }

    /// The connection searches use: read-only, so indexing never blocks it (WAL).
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<SearchIndex> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        Ok(SearchIndex { conn, path, writable: false })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn schema_version(&self) -> Result<usize> {
        Ok(self.conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as usize)
    }

    /// False for a read-only connection, and for a file a newer build made: then nothing is
    /// written to it and no new file is built from it.
    pub fn writable(&self) -> bool {
        self.writable
    }

    fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    /// The month the file is for ("2026-11").
    pub fn month(&self) -> Result<Option<String>> {
        self.meta(META_MONTH)
    }

    pub fn index_version(&self) -> Result<i64> {
        Ok(self.meta(META_VERSION)?.and_then(|v| v.parse().ok()).unwrap_or(0))
    }

    /// True when a new file should be built: the month has changed, or this build's index
    /// version is higher than the file's. Never for a file this build must leave alone.
    pub fn needs_new(&self, month: &str) -> Result<bool> {
        if !self.writable {
            return Ok(false);
        }
        Ok(self.month()?.as_deref() != Some(month) || self.index_version()? < INDEX_VERSION)
    }

    /// Stops the search running on this connection; it fails with an error [`is_interrupted`]
    /// knows. A call while nothing runs does nothing.
    pub fn interrupt_handle(&self) -> rusqlite::InterruptHandle {
        self.conn.get_interrupt_handle()
    }

    fn backup(&self, why: &str) -> Result<PathBuf> {
        let dir = self.path.parent().map(search_backups_dir).unwrap_or_else(|| PathBuf::from("backups").join("search"));
        std::fs::create_dir_all(&dir)?;
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
        let out = dir.join(format!("{stamp}-{why}.db"));
        self.conn.execute("VACUUM INTO ?1", [out.to_string_lossy().as_ref()])?;
        prune_backups(&dir, KEEP_SEARCH_BACKUPS)?;
        Ok(out)
    }

    fn check_writable(&self) -> Result<()> {
        if self.writable {
            Ok(())
        } else {
            Err(Error::Invalid("the search index is read-only here (a read connection, or a newer build's file)".into()))
        }
    }

    /// Records a transcript file for a card, and returns its id. A path already known keeps
    /// the card and session it was first recorded with.
    pub fn add_file(&mut self, path: &str, card_id: &str, session: &str, agent: Option<&str>) -> Result<i64> {
        self.check_writable()?;
        self.conn.execute(
            "INSERT OR IGNORE INTO files (path, card_id, session, agent, added_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![path, card_id, session, agent, crate::now()],
        )?;
        Ok(self.conn.query_row("SELECT id FROM files WHERE path = ?1", [path], |r| r.get(0))?)
    }

    pub fn file_id(&self, path: &str) -> Result<Option<i64>> {
        Ok(self.conn.query_row("SELECT id FROM files WHERE path = ?1", [path], |r| r.get(0)).optional()?)
    }

    /// Every file the index knows, oldest first.
    pub fn files(&self) -> Result<Vec<IndexedFile>> {
        let mut st = self.conn.prepare("SELECT id, path, card_id, session, agent FROM files ORDER BY id")?;
        let rows = st
            .query_map([], |r| Ok(IndexedFile { id: r.get(0)?, path: r.get(1)?, card_id: r.get(2)?, session: r.get(3)?, agent: r.get(4)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// How far file `file` has been read: its latest progress row, else 0.
    pub fn offset(&self, file: i64) -> Result<u64> {
        let v: Option<i64> = self
            .conn
            .query_row("SELECT offset FROM progress WHERE file = ?1 ORDER BY seq DESC LIMIT 1", [file], |r| r.get(0))
            .optional()?;
        Ok(v.unwrap_or(0).max(0) as u64)
    }

    /// Every file's offset, for the sweep.
    pub fn offsets(&self) -> Result<HashMap<i64, u64>> {
        let mut st = self.conn.prepare(
            "SELECT p.file, p.offset FROM progress p WHERE p.seq = (SELECT MAX(q.seq) FROM progress q WHERE q.file = p.file)",
        )?;
        let rows = st
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?.max(0) as u64)))?
            .collect::<rusqlite::Result<HashMap<_, _>>>()?;
        Ok(rows)
    }

    /// Stores a chunk of file `file`'s entries (masked, cut to [`MAX_TEXT`], each key once)
    /// and the offset reached, in one transaction. A progress row is written only when the
    /// offset moved. Returns how many entries were new.
    pub fn add_chunk(&mut self, file: i64, entries: &[NewEntry], offset: u64) -> Result<usize> {
        self.check_writable()?;
        let before = self.offset(file)?;
        let tx = self.conn.transaction()?;
        let mut added = 0;
        {
            let mut ins = tx.prepare_cached(
                "INSERT OR IGNORE INTO entries (key, file, kind, who, at, text) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for e in entries {
                let text = cut(&mask(e.text.trim()), MAX_TEXT);
                if text.is_empty() {
                    continue;
                }
                added += ins.execute(params![e.key, file, e.kind, mask(&e.who), e.at, text])?;
            }
        }
        if offset != before {
            tx.execute("INSERT INTO progress (file, offset, at) VALUES (?1, ?2, ?3)", params![file, offset as i64, crate::now()])?;
        }
        tx.commit()?;
        Ok(added)
    }

    pub fn stats(&self) -> Result<Stats> {
        let files: i64 = self.conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
        let entries: i64 = self.conn.query_row("SELECT COALESCE(MAX(id), 0) FROM entries", [], |r| r.get(0))?;
        Ok(Stats { files: files as usize, entries: entries as usize })
    }

    /// Searches entries from `since` on, optionally for one card: every word (or "quoted
    /// phrase") of 3 or more characters must appear. Takes the newest `limit` (at most
    /// [`MAX_ROWS`]) matching entries and groups them by card: most matches first, then the
    /// newest. Operators and stray quotes are taken literally, so no query is a syntax error.
    pub fn search(&self, query: &str, card: Option<&str>, since: &str, limit: usize) -> Result<Found> {
        let terms = query_terms(query);
        let Some(m) = fts_query(&terms) else { return Ok(Found { short: true, ..Default::default() }) };
        let limit = limit.clamp(1, MAX_ROWS);
        let mut st = self.conn.prepare_cached(
            "SELECT e.id, e.kind, e.who, e.at, e.text, f.card_id, f.session, f.agent, f.path
             FROM entries_fts JOIN entries e ON e.id = entries_fts.rowid JOIN files f ON f.id = e.file
             WHERE entries_fts MATCH ?1 AND e.at >= ?2 AND (?3 IS NULL OR f.card_id = ?3)
             ORDER BY entries_fts.rowid DESC LIMIT ?4",
        )?;
        let re = term_regex(&terms);
        let mut groups: Vec<CardHits> = Vec::new();
        let mut at: HashMap<String, usize> = HashMap::new();
        let mut newest: Vec<i64> = Vec::new();
        let mut total = 0;
        let mut rows = st.query(params![m, since, card, limit as i64])?;
        while let Some(r) = rows.next()? {
            total += 1;
            let id: i64 = r.get(0)?;
            let text: String = r.get(4)?;
            let card_id: String = r.get(5)?;
            let agent: Option<String> = r.get(7)?;
            let path: String = r.get(8)?;
            let agent_id = agent.as_ref().and_then(|_| agent_id_of(&path));
            let hit = Hit { kind: r.get(1)?, who: r.get(2)?, at: r.get(3)?, session: r.get(6)?, agent, agent_id, snippet: snippet(&text, re.as_ref()) };
            let i = *at.entry(card_id.clone()).or_insert_with(|| {
                groups.push(CardHits { card_id, count: 0, capped: false, hits: vec![] });
                newest.push(id);
                groups.len() - 1
            });
            let g = &mut groups[i];
            g.count += 1;
            if g.hits.len() < COUNT_CAP {
                g.hits.push(hit);
            }
        }
        let limited = total >= limit;
        for g in &mut groups {
            g.capped = limited || g.count > COUNT_CAP;
            g.count = g.count.min(COUNT_CAP);
        }
        let mut order: Vec<usize> = (0..groups.len()).collect();
        order.sort_by(|&a, &b| groups[b].count.cmp(&groups[a].count).then(newest[b].cmp(&newest[a])));
        let mut slots: Vec<Option<CardHits>> = groups.into_iter().map(Some).collect();
        let cards = order.into_iter().filter_map(|i| slots[i].take()).collect();
        Ok(Found { short: false, cards, total, limited })
    }

    /// Starts the next file at `new_path` for `month`, carrying this file's files rows (so
    /// paths found through hooks after a /clear are kept); its entries are read again from
    /// the transcripts. A half-built file left there for the same month and version is carried
    /// on; any other is moved into `backups\search` first.
    pub fn start_new(&self, new_path: &Path, month: &str) -> Result<SearchIndex> {
        if new_path.exists() {
            let keep = {
                let n = SearchIndex::open(new_path, month)?;
                n.writable && n.month()?.as_deref() == Some(month) && n.index_version()? == INDEX_VERSION
            };
            if !keep {
                let dir = new_path.parent().map(search_backups_dir).unwrap_or_else(|| PathBuf::from("backups").join("search"));
                std::fs::create_dir_all(&dir)?;
                let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
                rename_db(new_path, &dir.join(format!("{stamp}-stale-new.db")))?;
                prune_backups(&dir, KEEP_SEARCH_BACKUPS)?;
            }
        }
        let n = SearchIndex::open(new_path, month)?;
        n.check_writable()?;
        n.conn.execute("ATTACH DATABASE ?1 AS old", [self.path.to_string_lossy().as_ref()])?;
        let carried = n.conn.execute(
            "INSERT OR IGNORE INTO main.files (path, card_id, session, agent, added_at)
             SELECT path, card_id, session, agent, added_at FROM old.files ORDER BY id",
            [],
        );
        n.conn.execute("DETACH DATABASE old", [])?;
        carried?;
        Ok(n)
    }
}

/// A subagent transcript's id: `<id>` from `...\agent-<id>.jsonl`.
fn agent_id_of(path: &str) -> Option<String> {
    let name = path.rsplit(['\\', '/']).next()?;
    let id = name.strip_prefix("agent-")?.strip_suffix(".jsonl")?;
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric())).then(|| id.to_string())
}

/// Ends WAL mode on the file at `p` (SQLite checkpoints it and removes the `-wal` and `-shm`),
/// so a rename moves the whole database. Fails while another connection has the file open,
/// which stops a swap before anything is moved.
fn settle(p: &Path) -> Result<()> {
    let c = Connection::open_with_flags(p, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let mode: String = c.query_row("PRAGMA journal_mode = DELETE", [], |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("delete") {
        return Err(Error::Invalid(format!("{} is still in use ({mode} mode)", p.display())));
    }
    Ok(())
}

/// Removes `-wal` and `-shm` files in `dir` whose database is gone (pruned).
fn prune_sidecars(dir: &Path) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(()) };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if let Some(db) = name.strip_suffix("-wal").or_else(|| name.strip_suffix("-shm")) {
            if db.ends_with(".db") && !dir.join(db).exists() {
                std::fs::remove_file(e.path())?;
            }
        }
    }
    Ok(())
}

/// Finishes a swap that stopped half-way: `search.db` moved aside but `search.new.db` not yet
/// moved in. Call before opening the index.
pub fn recover_swap(dir: &Path) -> Result<bool> {
    let (cur, new) = (search_path(dir), new_search_path(dir));
    if cur.exists() || !new.exists() {
        return Ok(false);
    }
    rename_db(&new, &cur)?;
    Ok(true)
}

/// Moves `from` (and its `-wal` and `-shm`) to `to`: a rename, never a removal.
fn rename_db(from: &Path, to: &Path) -> Result<()> {
    std::fs::rename(from, to)?;
    for ext in ["-wal", "-shm"] {
        let side = PathBuf::from(format!("{}{ext}", from.display()));
        if side.exists() {
            std::fs::rename(&side, PathBuf::from(format!("{}{ext}", to.display())))?;
        }
    }
    Ok(())
}

/// Swaps the finished new file in: `search.db` is renamed to
/// `backups\search\<stamp>-search-<old month>.db`, `search.new.db` to `search.db`, and the
/// backups folder is pruned to [`KEEP_SEARCH_BACKUPS`]. Every connection to both files must
/// be closed first. Returns the backup's path, if there was an old file.
pub fn swap_in(dir: &Path, old_month: &str) -> Result<Option<PathBuf>> {
    let cur = search_path(dir);
    let new = new_search_path(dir);
    if !new.exists() {
        return Err(Error::Invalid("no new search index to swap in".into()));
    }
    let backups = search_backups_dir(dir);
    std::fs::create_dir_all(&backups)?;
    // Both files whole, with no WAL left beside them, before anything moves.
    settle(&new)?;
    if cur.exists() {
        settle(&cur)?;
    }
    let mut out = None;
    if cur.exists() {
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
        let month: String = old_month.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        let to = backups.join(format!("{stamp}-search-{month}.db"));
        rename_db(&cur, &to)?;
        out = Some(to);
    }
    rename_db(&new, &cur)?;
    prune_backups(&backups, KEEP_SEARCH_BACKUPS)?;
    prune_sidecars(&backups)?;
    Ok(out)
}

/// An entry's key: the line's uuid and the entry's place in the line; for a line with no
/// uuid (summary and queue lines), the file id, the line's byte offset and the place.
pub fn entry_key(uuid: Option<&str>, file: i64, line_offset: u64, place: usize) -> String {
    match uuid.filter(|u| !u.is_empty()) {
        Some(u) => format!("{u}#{place}"),
        None => format!("f{file}@{line_offset}#{place}"),
    }
}

/// Whole lines read from a transcript, and where to read from next.
#[derive(Debug, Default, PartialEq)]
pub struct Lines {
    /// Each line's byte offset in the file, and its text (without the newline).
    pub lines: Vec<(u64, String)>,
    pub offset: u64,
    /// More whole lines may follow: read again.
    pub more: bool,
}

/// A single line longer than this is skipped (passed over, not read into memory).
pub const MAX_LINE: u64 = 64 * 1024 * 1024;

/// Reads whole lines of `path` from `offset`, about `max` bytes at most, and only up to the
/// last newline: a last line still being written is left for the next read, and the offset
/// never passes it. A line longer than `max` is read on its own; one longer than [`MAX_LINE`]
/// is passed over. A file shorter than `offset` (rewritten) is read from the start.
pub fn read_lines(path: &Path, offset: u64, max: u64) -> std::io::Result<Lines> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let start = if offset > len { 0 } else { offset };
    if start >= len {
        return Ok(Lines { lines: vec![], offset: start, more: false });
    }
    f.seek(SeekFrom::Start(start))?;
    let want = (len - start).min(max.max(1));
    let mut buf = vec![0u8; want as usize];
    f.read_exact(&mut buf)?;
    // `buf` always ends with a newline here.
    let split = |buf: &[u8], base: u64| -> Vec<(u64, String)> {
        let body = buf.strip_suffix(b"\n").unwrap_or(buf);
        let mut out = Vec::new();
        let mut at = 0u64;
        for piece in body.split(|b| *b == b'\n') {
            let s = String::from_utf8_lossy(piece.strip_suffix(b"\r").unwrap_or(piece)).into_owned();
            if !s.trim().is_empty() {
                out.push((base + at, s));
            }
            at += piece.len() as u64 + 1;
        }
        out
    };
    if let Some(last) = buf.iter().rposition(|b| *b == b'\n') {
        let end = last + 1;
        let lines = split(&buf[..end], start);
        let next = start + end as u64;
        return Ok(Lines { lines, offset: next, more: next < len });
    }
    if start + want >= len {
        // Only an incomplete line: wait for its newline.
        return Ok(Lines { lines: vec![], offset: start, more: false });
    }
    // One line longer than `max`: read on to its newline.
    let mut piece = vec![0u8; 1024 * 1024];
    let mut read = want;
    let mut keep = true;
    loop {
        let n = f.read(&mut piece)?;
        if n == 0 {
            // It is still being written.
            return Ok(Lines { lines: vec![], offset: start, more: false });
        }
        if let Some(i) = piece[..n].iter().position(|b| *b == b'\n') {
            read += i as u64 + 1;
            if keep {
                buf.extend_from_slice(&piece[..=i]);
            }
            break;
        }
        read += n as u64;
        if keep && read > MAX_LINE {
            keep = false;
            buf = Vec::new();
        }
        if keep {
            buf.extend_from_slice(&piece[..n]);
        }
    }
    let next = start + read;
    let lines = if keep { split(&buf, start) } else { vec![] };
    Ok(Lines { lines, offset: next, more: next < len })
}

/// `s` cut to at most `max` bytes, on a character boundary.
fn cut(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

// ---- the mask ----

/// What a masked token becomes.
pub const HIDDEN: &str = "[hidden]";
/// What a sign-in link becomes.
pub const LINK_HIDDEN: &str = "[sign-in link hidden]";

/// A token's characters: everything up to the next separator (whitespace, a quote, a
/// backtick, & < > , ; ) ] }).
const TOK: &str = r#"[^\s'"`&<>,;)\]}]"#;

fn masks() -> &'static [(Regex, String)] {
    static RE: OnceLock<Vec<(Regex, String)>> = OnceLock::new();
    RE.get_or_init(|| {
        let t = TOK;
        let rules: Vec<(String, String)> = vec![
            // A private key block, through its END line (or to the end of the text).
            (r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----[\s\S]*?(?:-----END [A-Z0-9 ]*PRIVATE KEY-----|\z)".into(), HIDDEN.into()),
            // A sign-in link: the whole token that holds signin, sign-in, login or log-in, query and all.
            (r#"(?i)[^\s'"`<>(\[{]*(?:sign-?in|log-?in)[^\s'"`<>)\]}]*"#.into(), LINK_HIDDEN.into()),
            // Bearer <token>.
            (format!(r"(?i)\b(Bearer)\s+{t}+"), format!("$1 {HIDDEN}")),
            // The board's own words (secret.rs), with the value after them: ct=<v>,
            // customToken: <v>, idToken=<v>, password=<v>, --password <v>.
            (format!(r"(?i)\bct={t}+"), format!("ct={HIDDEN}")),
            (
                format!(r#"(?i)\b(custom[_-]?token|id[_-]?token|pass(?:word|wd)|pwd)(["']?\s*[:=]\s*["']?){t}+"#),
                format!("$1$2{HIDDEN}"),
            ),
            (format!(r"(?i)(--pass(?:word|wd)|--pwd)(=|\s+){t}+"), format!("$1$2{HIDDEN}")),
            // A JWT: all three dot-separated parts.
            (r"eyJ[A-Za-z0-9_-]{6,}(?:\.[A-Za-z0-9_-]*){0,2}".into(), HIDDEN.into()),
            // Stripe secret and restricted keys, live and test.
            (format!(r"(?i)\b[rs]k_(?:live|test){t}*"), HIDDEN.into()),
            // GitHub tokens.
            (format!(r"\b(?:gh[pousr]_|github_pat_){t}*"), HIDDEN.into()),
            // Anthropic keys.
            (format!(r"\bsk-ant-{t}*"), HIDDEN.into()),
            // Google API keys and OAuth tokens.
            (format!(r"\bAIza{t}*"), HIDDEN.into()),
            (format!(r"\bya29\.{t}*"), HIDDEN.into()),
            // Slack tokens: xoxb-, xoxp-, xoxa-, xoxe-, xoxs-, xoxr- and the rest.
            (format!(r"\bxox[a-z]-{t}*"), HIDDEN.into()),
            // A run of exactly 32 hex digits, such as the app's own API token. Git hashes (40)
            // and SHA-256 hashes (64) stay, so a commit can be searched for.
            (r"\b[0-9A-Fa-f]{32}\b".into(), HIDDEN.into()),
        ];
        rules.into_iter().map(|(p, r)| (Regex::new(&p).expect("mask pattern"), r)).collect()
    })
}

/// `s` with every secret token and sign-in link it holds replaced by [`HIDDEN`] or
/// [`LINK_HIDDEN`]: the whole token, up to the next separator, not only its prefix.
pub fn mask(s: &str) -> String {
    let mut out = std::borrow::Cow::Borrowed(s);
    for (re, with) in masks() {
        if re.is_match(&out) {
            out = std::borrow::Cow::Owned(re.replace_all(&out, with.as_str()).into_owned());
        }
    }
    out.into_owned()
}

// ---- queries and snippets ----

/// The query's terms: each word, or "quoted phrase", of [`MIN_TERM`] characters or more,
/// once each (ignoring case), at most 16. A quote with no partner is taken as part of the
/// words after it.
pub fn query_terms(q: &str) -> Vec<String> {
    let mut raw: Vec<String> = Vec::new();
    let mut rest = q;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        if let Some(after) = rest.strip_prefix('"') {
            if let Some(end) = after.find('"') {
                raw.push(after[..end].trim().to_string());
                rest = &after[end + 1..];
                continue;
            }
            raw.extend(after.split_whitespace().map(str::to_string));
            break;
        }
        let end = rest.find(|c: char| c.is_whitespace() || c == '"').unwrap_or(rest.len());
        raw.push(rest[..end].to_string());
        rest = &rest[end..];
    }
    let mut seen = std::collections::HashSet::new();
    raw.into_iter()
        .filter(|t| t.chars().count() >= MIN_TERM)
        .filter(|t| seen.insert(t.to_lowercase()))
        .take(16)
        .collect()
}

/// The FTS5 MATCH text for `terms`: each a quoted string (so operators are literal), ANDed.
/// None when there are no terms.
pub fn fts_query(terms: &[String]) -> Option<String> {
    if terms.is_empty() {
        return None;
    }
    Some(terms.iter().map(|t| format!("\"{}\"", t.replace('"', "\"\""))).collect::<Vec<_>>().join(" AND "))
}

fn term_regex(terms: &[String]) -> Option<Regex> {
    let mut ts: Vec<&String> = terms.iter().collect();
    ts.sort_by_key(|t| std::cmp::Reverse(t.len()));
    let alt = ts.iter().map(|t| regex::escape(t)).collect::<Vec<_>>().join("|");
    Regex::new(&format!("(?i){alt}")).ok()
}

/// About 160 characters round the first match, whitespace runs made single spaces, the
/// matches marked.
pub fn snippet(text: &str, re: Option<&Regex>) -> Vec<Part> {
    static WS: OnceLock<Regex> = OnceLock::new();
    let flat = WS.get_or_init(|| Regex::new(r"\s+").unwrap()).replace_all(text.trim(), " ");
    let flat = flat.as_ref();
    let first = re.and_then(|r| r.find(flat)).map(|m| m.start()).unwrap_or(0);
    // 60 characters before the first match, 160 in all.
    let start = flat[..first].char_indices().rev().nth(59).map(|(i, _)| i).unwrap_or(0);
    let end = flat[start..].char_indices().nth(160).map(|(i, _)| start + i).unwrap_or(flat.len());
    let win = &flat[start..end];
    let mut parts = Vec::new();
    let mut push = |t: &str, m: bool| {
        if !t.is_empty() {
            parts.push(Part { t: t.to_string(), m });
        }
    };
    if start > 0 {
        push("…", false);
    }
    let mut last = 0;
    if let Some(re) = re {
        for m in re.find_iter(win) {
            push(&win[last..m.start()], false);
            push(m.as_str(), true);
            last = m.end();
        }
    }
    push(&win[last..], false);
    if end < flat.len() {
        push("…", false);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An existing file is backed up into backups\search before a migration runs on it.
    #[test]
    fn an_existing_file_is_backed_up_before_a_migration() {
        let dir = tempfile::tempdir().unwrap();
        let p = search_path(dir.path());
        drop(SearchIndex::open(&p, "2026-11").unwrap());
        assert!(!search_backups_dir(dir.path()).exists());
        let next = "CREATE TABLE extra (x INTEGER);";
        let ix = SearchIndex::open_with(&p, "2026-11", &[SEARCH_MIGRATIONS[0], next]).unwrap();
        assert_eq!(ix.schema_version().unwrap(), 2);
        let backups: Vec<_> = std::fs::read_dir(search_backups_dir(dir.path())).unwrap().flatten().collect();
        assert_eq!(backups.len(), 1);
        assert!(backups[0].file_name().to_string_lossy().ends_with("-pre-migration.db"));
        // A new file is not backed up: there is nothing to keep.
        let fresh = dir.path().join("other").join("search.db");
        drop(SearchIndex::open_with(&fresh, "2026-11", &[SEARCH_MIGRATIONS[0], next]).unwrap());
        assert!(!search_backups_dir(&dir.path().join("other")).exists());
    }
}
