//! The board's store: SQLite, owned by one process, with version-pinned card
//! writes and append-only logs. Nothing here ever removes a row.

// The Router's asks to archive a card, and Hugh's answer (card #131).
mod archive_ask;
mod cards;
mod channel;
mod check_size;
mod move_in;
mod columns;
mod config;
mod done_when;
mod evidence;
mod feed;
mod import;
mod secret;
// The Router's own sessions and log (card #119).
mod router;
mod marks;
pub mod search;
mod sessions;
mod ships;
mod stacks;
mod start_queue;
mod templates;
pub mod testapp;
// Time in each column (card #111).
mod times;
mod waits;

use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use archive_ask::{archive_refused, lifts_decline, ArchiveResult, ARCHIVE_ASK_KEY, ARCHIVE_DECLINED_KEY, ASK_TEXT_MAX};
pub use move_in::{import_if_asked, Imported, IMPORT_DONE, IMPORT_REQUEST};
pub use config::PRIVATE_DIR_KEY;
pub use channel::{app_name, app_name_for, channel, data_dir_for, devtools_port_for, exe_name_for, home_var, identifier_for, is_test, kb_data_dir, DATA_DIR_ENV, label_for, live_board, product_name_for, quit_event, quit_event_for, slot, slot_of, LIVE_DEVTOOLS_PORT};
pub use cards::{clean_auto_to, clean_labels, Card, LogEntry, MoveOutcome, Patch, AUTO_BY, AUTO_KEEPS, AUTO_KEY, AUTO_TARGETS, CRITIQUE_KEY, MAX_LABELS, MAX_LABEL_LEN, MERGE_KEY, repro_open, REPRO_KEY, RESET_ON_IN_PROGRESS, REVIEW_KEY, ROUTE_KEY, SMALL_BY, SMALL_KEY, SMALL_OPT_OUT_KEY, SMALL_WAITING, SMALL_WENT_ON, TEST_KEY};
pub use cards::{CHECK_SIZE_KEY, RESET_ON_HUMAN_TEST};
pub use cards::{clean_changes_outputs, output_path_problem, OUTPUTS_KEY, SAVED_KEY};
pub use columns::{ColumnCfg, Pending};
pub use config::{default_profiles, page_default, profile_for, Profile, CRITIC_RULE_KEY, EFFORTS, MAX_RUNNING_DEFAULT, MAX_RUNNING_KEY, IDLE_PAUSE_DEFAULT, IDLE_PAUSE_KEY, PAGE_SETTINGS, PROFILES_KEY, REPRO_RULE_KEY, REVIEW_RULE_KEY, RULE_KEYS, SMALL_GO_ON_KEY, STUCK_CHECK_DEFAULT, STUCK_CHECK_KEY, TEST_RULE_KEY, check_small_repos, SettingChange, SmallRepo, SMALL_REPOS_KEY};
pub use evidence::{add_run, has_tested_evidence, EVIDENCE_KEY};
pub use feed::{agent_of_by, agent_of_field, FeedItem, FeedPage, FeedQuery, FEED_DEFAULT_LIMIT, FEED_MAX_LIMIT, VERDICT_FIELDS, VERDICT_TEXT_MAX};
pub use config::TODO_TOOLS_KEY;
pub use done_when::{
    clean_done_when, clean_done_when_checks, looks_like_sign_in, DONE_WHEN_CHECKS_KEY, DONE_WHEN_CHECK_LEN,
    DONE_WHEN_KEY, DONE_WHEN_MAX_CHECKS, DONE_WHEN_MAX_LEN, DONE_WHEN_NOTE_LEN, DONE_WHEN_STATES,
};
pub use import::{CheckReport, FieldDiff, ImportReport};
pub use config::ROUTER_MODELS;
pub use router::{
    Revision, RouterLogLine, RouterSession, ROUTER_BY, ROUTER_EFFORT_DEFAULT, ROUTER_EFFORT_KEY, ROUTER_LAST_OPENED_KEY, ROUTER_LOG_KINDS,
    ROUTER_MAY_START_KEY, ROUTER_MODEL_DEFAULT, ROUTER_MODEL_KEY, ROUTER_ON_KEY, ROUTER_RESUME_AFTER_KEY,
};
pub use secret::{looks_like_token, looks_secret, mask_secrets, mask_tokens};
pub use search::SearchIndex;
pub use marks::{CardMarks, LatestSession};
pub use sessions::{SessionRow, MID_TURN, RESUME_OFFER_KEY};
pub use ships::{ship_goes_first, ship_lane, ship_started, QUEUED as SHIP_QUEUED};
pub use stacks::{children_reviewed as stack_children_reviewed, lead_missing, lead_missing_in_stack, merged as stack_merged, own_build as stack_own_build, OWN_BUILD_KEY as STACK_OWN_BUILD_KEY, stack_ready, stack_stage, Joined, StackMove, READY as STACK_READY, STACK_FIELDS, STACK_HELD_KEY, STACK_RULE_KEY};
pub use start_queue::{StartQueue, WaitingStart, DEFAULT_CAP as DEFAULT_SESSION_CAP, MAX_CAP as MAX_SESSION_CAP};
pub use templates::{Template, TemplateInput};
// Time in each column (card #111).
pub use times::{card_columns, column_since, median, medians, timeline, CardTimes, ColumnMedian, ColumnTime, LogIn, RevIn, Timeline, Visit, LOG_WINDOW_MS, MEDIAN_DAYS};
pub use testapp::{take_refused, Ask, Ended, Take, TestAppQueue, Turn, Waiting, INSTALL as TESTAPP_INSTALL, TESTER as TESTAPP_TESTER, TESTER_COLUMNS};
pub use waits::{clean_blocked_by, is_met, wait_path, Wait, WaitKind, WaitOutcome, BLOCKED_KEY, LEAD_NOTE, MAX_WAITS, MET_OTHER_REPO, MET_SAME_REPO, WAITED_KEY, WAIT_BY};

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

const MIGRATIONS: [&str; 14] = [
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
    include_str!("schema11.sql"),
    include_str!("schema12.sql"),
    // Card #114: the column prompts name their folder as {prompts}.
    include_str!("schema13.sql"),
    // Card #119: the Router's own sessions and log.
    include_str!("schema14.sql"),
];

/// The schema version this build brings a board up to: the number of migrations. A board at a
/// higher version (from a newer build) is opened as it is.
pub const SCHEMA_VERSION: usize = MIGRATIONS.len();

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
/// `%USERPROFILE%\.claude-kanban-dsh`, or `%USERPROFILE%\.claude-kanban-dsh-test` (`-test2`, ...) for a test build. It is
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
    // So are the test app's turns (card #115): a test board never starts with the live
    // holder and line, and so never tells test-board sessions about turns they never took.
    copy.save_test_app_queue(&TestAppQueue::default())?;
    // And the Router (card #119): a copy never resumes the source's Router conversation, and
    // starts with it off.
    copy.detach_router()?;
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
