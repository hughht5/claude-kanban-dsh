//! Moving a board in from another data folder, once: for a rename of the app, whose data folder
//! is named after it. Ask for it by writing the old folder's path into `import-from.txt` in the
//! new data folder (`~` is the home folder) before the app first starts there.
//!
//! On start, a data folder with no `board.db` but an `import-from.txt` takes a consistent copy of
//! the old board (`VACUUM INTO`) and copies everything else in the old folder (sessions,
//! attachments, backups, window place and anything newer) except the old app's own port, token,
//! log and staged update. Sessions keep their state, so the ones mid-turn at the old app's last
//! stop resume as usual. The old folder is only read, never changed or removed, so it stays as
//! the backup. The request is then renamed `imported-from.txt`, with the time, so it runs once.
//!
//! Lines after the first may set page settings on the imported board, as `key=value` (for
//! example `private.dir=~\my-private`); the app applies them through its usual checks. Lines
//! starting with `#` are notes.

use crate::{now, Error, Result};
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

pub const IMPORT_REQUEST: &str = "import-from.txt";
pub const IMPORT_DONE: &str = "imported-from.txt";
/// What stays behind: the old board's live files (the copy is taken with VACUUM INTO), the old
/// app's port and token, its log, its staged update, and the requests themselves.
const LEAVE: [&str; 9] = [
    "board.db",
    "board.db-wal",
    "board.db-shm",
    "api.json",
    "api.log",
    "update",
    IMPORT_REQUEST,
    IMPORT_DONE,
    "board.db.importing",
];

/// What an import did: the folder it came from, and the settings the request asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    pub from: PathBuf,
    pub settings: Vec<(String, String)>,
}

fn expand(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix('~') {
        Some(rest) => home.join(rest.trim_start_matches(['\\', '/'])),
        None => PathBuf::from(path),
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<u64> {
    let mut n = 0;
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let dest = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            n += copy_tree(&e.path(), &dest)?;
        } else if !dest.exists() {
            std::fs::copy(e.path(), &dest)?;
            n += 1;
        }
    }
    Ok(n)
}

/// Does what `<dir>\import-from.txt` asks, if `dir` has no board yet. `None` when there was
/// nothing to do.
pub fn import_if_asked(dir: &Path, home: &Path) -> Result<Option<Imported>> {
    let request = dir.join(IMPORT_REQUEST);
    let board = dir.join("board.db");
    if board.exists() || !request.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&request)?;
    let mut lines = text
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'));
    let first = lines.next().unwrap_or("");
    let from = expand(first, home);
    let settings: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    let old = from.join("board.db");
    if first.is_empty() || !old.is_file() {
        return Err(Error::Invalid(format!("{} asks to import {}, which has no board.db", request.display(), from.display())));
    }
    let tmp = dir.join("board.db.importing");
    let _ = std::fs::remove_file(&tmp);
    {
        let src = Connection::open_with_flags(&old, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        src.execute("VACUUM INTO ?1", [tmp.to_string_lossy().as_ref()])?;
    }
    let mut files = 0;
    for e in std::fs::read_dir(&from)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().to_string();
        if LEAVE.iter().any(|l| l.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let dest = dir.join(&name);
        if e.file_type()?.is_dir() {
            files += copy_tree(&e.path(), &dest)?;
        } else if !dest.exists() {
            std::fs::copy(e.path(), &dest)?;
            files += 1;
        }
    }
    std::fs::rename(&tmp, &board)?;
    std::fs::write(dir.join(IMPORT_DONE), format!("{}\nimported at {} ({files} files besides the board)\n", from.display(), now()))?;
    std::fs::remove_file(&request)?;
    Ok(Some(Imported { from, settings }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board_with_a_row(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        let c = Connection::open(dir.join("board.db")).unwrap();
        c.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE t(x); INSERT INTO t VALUES (42);").unwrap();
        // Left open: the copy must still see the row, as with a board in WAL mode.
        std::mem::forget(c);
    }

    #[test]
    fn imports_once_and_leaves_the_old_folder_alone() {
        let home = tempfile::tempdir().unwrap();
        let old = home.path().join(".old-app");
        board_with_a_row(&old);
        std::fs::create_dir_all(old.join("sessions").join("abc")).unwrap();
        std::fs::write(old.join("sessions").join("abc").join("settings.json"), "{}").unwrap();
        std::fs::write(old.join("window.json"), "{\"x\":1}").unwrap();
        std::fs::write(old.join("api.json"), "{\"port\":1}").unwrap();
        std::fs::create_dir_all(old.join("update")).unwrap();
        std::fs::create_dir_all(old.join("evidence").join("7")).unwrap();
        std::fs::write(old.join("evidence").join("7").join("shot.png"), "png").unwrap();

        let new = home.path().join(".new-app");
        std::fs::create_dir_all(&new).unwrap();
        assert_eq!(import_if_asked(&new, home.path()).unwrap(), None, "no request, nothing to do");
        let request = "\u{feff}~/.old-app\r\n# the owner's own prompts\r\nprivate.dir = ~/mine\r\n";
        std::fs::write(new.join(IMPORT_REQUEST), request).unwrap();
        let got = import_if_asked(&new, home.path()).unwrap().unwrap();
        assert_eq!(got.from, old);
        assert_eq!(got.settings, vec![("private.dir".to_string(), "~/mine".to_string())]);

        let c = Connection::open(new.join("board.db")).unwrap();
        assert_eq!(c.query_row("SELECT x FROM t", [], |r| r.get::<_, i64>(0)).unwrap(), 42);
        assert!(new.join("sessions").join("abc").join("settings.json").is_file());
        assert!(new.join("window.json").is_file() && new.join("evidence").join("7").join("shot.png").is_file());
        assert!(!new.join("api.json").exists() && !new.join("update").exists());
        assert!(!new.join(IMPORT_REQUEST).exists() && new.join(IMPORT_DONE).is_file());
        assert!(old.join("board.db").is_file() && old.join("api.json").is_file());

        // A second start with a new request finds a board and does nothing.
        std::fs::write(new.join(IMPORT_REQUEST), "~/.old-app").unwrap();
        assert_eq!(import_if_asked(&new, home.path()).unwrap(), None);
    }

    #[test]
    fn a_request_for_a_folder_with_no_board_is_refused() {
        let home = tempfile::tempdir().unwrap();
        let new = home.path().join(".new-app");
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join(IMPORT_REQUEST), "~/.nothing-here").unwrap();
        assert!(import_if_asked(&new, home.path()).is_err());
        assert!(!new.join("board.db").exists());
    }
}
