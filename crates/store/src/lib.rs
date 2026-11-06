//! The board's store: SQLite, owned by one process, with version-pinned card
//! writes and append-only logs. Nothing here ever removes a row.

mod cards;
mod channel;
mod move_in;
mod columns;
mod config;
mod import;
mod secret;
mod marks;
mod sessions;
mod ships;
mod stacks;
mod start_queue;
mod templates;

use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use channel::{app_name, channel, data_dir_for, is_test, live_board, quit_event};
pub use move_in::{import_if_asked, IMPORT_DONE, IMPORT_REQUEST};
pub use cards::{clean_auto_to, clean_labels, Card, LogEntry, MoveOutcome, Patch, AUTO_BY, AUTO_KEEPS, AUTO_KEY, AUTO_TARGETS, MAX_LABELS, MAX_LABEL_LEN, TEST_KEY};
pub use columns::{ColumnCfg, Pending};
pub use config::{default_profiles, page_default, profile_for, Profile, EFFORTS, MAX_RUNNING_DEFAULT, MAX_RUNNING_KEY, IDLE_PAUSE_DEFAULT, IDLE_PAUSE_KEY, PAGE_SETTINGS, PRIVATE_DIR_KEY, PROFILES_KEY, TEST_RULE_KEY};
pub use import::{CheckReport, FieldDiff, ImportReport};
pub use secret::{looks_like_token, looks_secret};
pub use marks::{CardMarks, LatestSession};
pub use sessions::{SessionRow, MID_TURN, RESUME_OFFER_KEY};
pub use ships::{ship_goes_first, ship_lane, ship_started, QUEUED as SHIP_QUEUED};
pub use stacks::{stack_ready, Joined, StackMove, READY as STACK_READY, STACK_FIELDS, STACK_RULE_KEY};
pub use start_queue::{StartQueue, WaitingStart, DEFAULT_CAP as DEFAULT_SESSION_CAP, MAX_CAP as MAX_SESSION_CAP};
pub use templates::{Template, TemplateInput};

pub const COLUMNS: [&str; 8] = [
    "Backlog",
    "Blocked",
    "Planning",
    "In progress",
    "Human test",
    "Ship it",
    "Deploy complete",
    "Done",
];

const MIGRATIONS: [&str; 10] = [
    include_str!("schema.sql"),
    include_str!("schema2.sql"),
    include_str!("schema3.sql"),
    include_str!("schema4.sql"),
    include_str!("schema5.sql"),
    include_str!("schema6.sql"),
    include_str!("schema7.sql"),
    include_str!("schema8.sql"),
    include_str!("schema9.sql"),
    include_str!("schema10.sql"),
];

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("no card {0}")]
    NotFound(String),
    #[error("card {id} changed: you had version {yours}, it is now at {current}")]
    Conflict { id: String, yours: i64, current: i64 },
    #[error("\"{0}\" looks like it holds a token, key or password; secrets never go on the board")]
    Secret(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Store {
    conn: Connection,
    path: PathBuf,
}

impl Store {
    /// Opens (or creates) the board at `path` and brings its schema up to date,
    /// taking a backup beside it before any migration of an existing board.
    pub fn open(path: impl AsRef<Path>) -> Result<Store> {
        let path = path.as_ref().to_path_buf();
        let existed = path.exists();
        let conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut store = Store { conn, path };
        store.migrate(existed)?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn journal_mode(&self) -> Result<String> {
        Ok(self.conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?)
    }

    pub fn schema_version(&self) -> Result<usize> {
        Ok(self.conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as usize)
    }

    fn migrate(&mut self, existed: bool) -> Result<()> {
        let current = self.schema_version()?;
        if current >= MIGRATIONS.len() {
            return Ok(());
        }
        if existed && current > 0 {
            self.backup("pre-migration")?;
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()?;
        }
        Ok(())
    }

    /// The `backups` folder beside the board.
    pub fn backups_dir(&self) -> PathBuf {
        self.path
            .parent()
            .map(|p| p.join("backups"))
            .unwrap_or_else(|| PathBuf::from("backups"))
    }

    /// Writes a consistent copy of the board to `backups/<stamp>-<why>.db` beside it.
    pub fn backup(&self, why: &str) -> Result<PathBuf> {
        let dir = self.backups_dir();
        std::fs::create_dir_all(&dir)?;
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
        let out = dir.join(format!("{stamp}-{why}.db"));
        self.conn
            .execute("VACUUM INTO ?1", [out.to_string_lossy().as_ref()])?;
        Ok(out)
    }

    /// The backup taken at the app's start: one only if none in `backups` is from today
    /// (the stamp's UTC date). Then the folder is pruned to the newest `keep`.
    pub fn backup_if_none_today(&self, keep: usize) -> Result<Option<PathBuf>> {
        let today = chrono::Utc::now().format("%Y%m%d").to_string();
        let dir = self.backups_dir();
        let have = std::fs::read_dir(&dir)
            .map(|d| {
                d.flatten().any(|e| {
                    let n = e.file_name().to_string_lossy().to_string();
                    n.ends_with(".db") && n.starts_with(&today)
                })
            })
            .unwrap_or(false);
        let out = if have { None } else { Some(self.backup("daily")?) };
        prune_backups(&dir, keep)?;
        Ok(out)
    }

    /// The backup taken every 24 hours while the app runs, then pruned to the newest `keep`.
    pub fn backup_and_prune(&self, why: &str, keep: usize) -> Result<PathBuf> {
        let out = self.backup(why)?;
        prune_backups(&self.backups_dir(), keep)?;
        Ok(out)
    }

    pub fn next_number(&self) -> Result<i64> {
        let v: String =
            self.conn
                .query_row("SELECT value FROM meta WHERE key = 'next_number'", [], |r| r.get(0))?;
        v.parse()
            .map_err(|_| Error::Invalid(format!("next_number is not a number: {v}")))
    }
}

/// How many backups the folder keeps.
pub const KEEP_BACKUPS: usize = 30;

/// Keeps the newest `keep` `*.db` files in `dir` and removes the older ones. A backup's
/// name starts with its UTC stamp, so name order is age order. Returns the files removed.
pub fn prune_backups(dir: &Path, keep: usize) -> Result<Vec<PathBuf>> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(vec![]) };
    let mut dbs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().map_or(false, |x| x == "db"))
        .collect();
    dbs.sort();
    let extra = dbs.len().saturating_sub(keep);
    let old: Vec<PathBuf> = dbs.into_iter().take(extra).collect();
    for p in &old {
        std::fs::remove_file(p)?;
    }
    Ok(old)
}

pub(crate) fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// The folder that holds the board, its backups, session files and the API's port and token:
/// `%USERPROFILE%\.claude-kanban-dsh`, or `%USERPROFILE%\.claude-kanban-dsh-test` for a test build. It is
/// outside AppData on purpose. Windows redirects AppData writes made by processes started from
/// a packaged app (the Claude desktop app is one), so an AppData folder showed different boards
/// to different launches of the same app.
pub fn data_dir() -> PathBuf {
    let dir = data_dir_for(channel(), &channel::home());
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Writes a consistent snapshot of the board at `from` to `to` with `VACUUM INTO`, which is
/// safe while the app that owns `from` is running. `from` is opened read-only and never
/// migrated. An existing `to` is refused unless `replace`; then it (and any `-wal`/`-shm`
/// beside it) is moved into `backups` beside it first, so nothing is lost.
///
/// Sessions still running in the snapshot belong to the board it came from: in the copy each
/// is recorded as suspended ("copied", not mid-turn), so an app opening the copy never resumes
/// another app's conversation.
pub fn copy_board(from: &Path, to: &Path, replace: bool) -> Result<CopyReport> {
    if !from.is_file() {
        return Err(Error::Invalid(format!("no board at {}", from.display())));
    }
    if let (Ok(a), Ok(b)) = (from.canonicalize(), to.canonicalize()) {
        if a == b {
            return Err(Error::Invalid("--from and --to are the same file".into()));
        }
    }
    if let Some(parent) = to.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    // Into a temporary name first, so a failed copy never leaves a half board at `to`.
    let tmp = to.with_extension("db.copying");
    if tmp.exists() {
        std::fs::remove_file(&tmp)?;
    }
    {
        let src = Connection::open_with_flags(from, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        src.busy_timeout(Duration::from_secs(10))?;
        src.execute("VACUUM INTO ?1", [tmp.to_string_lossy().as_ref()])?;
    }
    let mut moved_aside = None;
    if to.exists() {
        if !replace {
            std::fs::remove_file(&tmp)?;
            return Err(Error::Invalid(format!("{} already exists; pass --replace to overwrite it (the old one is kept in backups)", to.display())));
        }
        let dir = to.parent().map(|p| p.join("backups")).unwrap_or_else(|| PathBuf::from("backups"));
        std::fs::create_dir_all(&dir)?;
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
        let aside = dir.join(format!("{stamp}-replaced-by-copy.db"));
        std::fs::rename(to, &aside)?;
        for ext in ["-wal", "-shm"] {
            let side = PathBuf::from(format!("{}{ext}", to.display()));
            if side.exists() {
                std::fs::rename(&side, PathBuf::from(format!("{}{ext}", aside.display())))?;
            }
        }
        moved_aside = Some(aside);
    }
    std::fs::rename(&tmp, to)?;
    let mut copy = Store::open(to)?;
    let running = copy.running_sessions()?;
    for id in &running {
        copy.session_event(id, "suspended", Some("copied: running on the board this was copied from; not resumed here"))?;
    }
    let last: i64 = copy.conn.query_row("SELECT COALESCE(MAX(seq), 0) FROM session_events", [], |r| r.get(0))?;
    copy.set_setting("app.run_started_seq", &last.to_string())?;
    // Nor is anything stopped on the source offered for resuming here.
    copy.set_setting(sessions::RESUME_OFFER_KEY, &last.to_string())?;
    // Starts waiting under the cap belong to the board this came from too.
    copy.save_start_queue(&StartQueue::default())?;
    let cards = copy.list(true)?.len();
    Ok(CopyReport { cards, sessions_detached: running.len(), moved_aside })
}

/// What `copy_board` did.
#[derive(Debug)]
pub struct CopyReport {
    pub cards: usize,
    pub sessions_detached: usize,
    pub moved_aside: Option<PathBuf>,
}
