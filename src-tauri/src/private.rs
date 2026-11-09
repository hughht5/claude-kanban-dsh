//! The private folder (`private.dir` in Settings): a folder outside this repo that holds the
//! owner's own column prompts and repo list, so the public repo carries only generic examples.
//!
//! - `<private>\prompts\*.md` replaces the bundled `prompts\` folder as a whole when it exists.
//!   Column prompts and templates name it as `{prompts}`.
//! - `<private>\repos.json` lists the repos a card can be for:
//!   `[{ "repo": "owner/name", "folder": "~\\name", "label": "name" }]`. `folder` is where the
//!   card's session starts (`~` is the home folder); `label` is the short name on the board.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

static DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repo {
    pub repo: String,
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub label: String,
    /// Top-level folders a user signs in to: a change there needs a sign-in to check.
    #[serde(default, rename = "loginDirs")]
    pub login_dirs: Vec<String>,
}

pub fn home() -> PathBuf {
    kanban_store::home_var().unwrap_or_else(crate::platform::no_home)
}

/// `~` or `~\x` under `home`; anything else as it is.
pub fn expand(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix('~') {
        Some(rest) => home.join(rest.trim_start_matches(['\\', '/'])),
        None => PathBuf::from(path),
    }
}

/// Sets the private folder from the saved setting (empty: none).
pub fn set(value: &str) {
    let v = value.trim();
    let dir = (!v.is_empty()).then(|| expand(v, &home()));
    if let Ok(mut d) = DIR.write() {
        *d = dir;
    }
}

pub fn dir() -> Option<PathBuf> {
    DIR.read().ok().and_then(|d| d.clone())
}

/// The prompts this build carries. A test build: the checkout it was built from (`PROMPTS_DIR`,
/// card #96), so the test app runs the prompts under test. Otherwise `prompts\` beside the
/// installed exe (the installer puts them there), else the checkout it was built from.
fn bundled_prompts() -> PathBuf {
    let built = PathBuf::from(crate::dispatch::PROMPTS_DIR);
    if kanban_store::is_test() && built.is_dir() {
        return built;
    }
    let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("prompts")));
    match beside {
        Some(p) if p.is_dir() => p,
        _ => built,
    }
}

fn prompts_in(private: Option<&Path>, bundled: PathBuf) -> PathBuf {
    match private.map(|d| d.join("prompts")) {
        Some(p) if p.is_dir() => p,
        _ => bundled,
    }
}

/// The folder `{prompts}` stands for.
pub fn prompts_dir() -> PathBuf {
    let p = prompts_in(dir().as_deref(), bundled_prompts());
    // `..` in a path reads badly in a prompt.
    std::fs::canonicalize(&p).map(|c| PathBuf::from(c.to_string_lossy().trim_start_matches(r"\\?\"))).unwrap_or(p)
}

fn repos_in(private: Option<&Path>) -> Vec<Repo> {
    private
        .map(|d| d.join("repos.json"))
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str::<Vec<Repo>>(&t).map_err(|e| eprintln!("repos.json: {e}")).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|r| !r.repo.trim().is_empty())
        .map(|mut r| {
            if r.label.trim().is_empty() {
                r.label = r.repo.rsplit('/').next().unwrap_or(&r.repo).to_string();
            }
            r
        })
        .collect()
}

/// The repos a card can be for, from the private folder's `repos.json` (none without one).
pub fn repos() -> Vec<Repo> {
    repos_in(dir().as_deref())
}

/// The folders of `repo` a user signs in to (its `loginDirs` in `repos.json`), none if unlisted.
pub fn login_dirs(repo: &str) -> Vec<String> {
    repos().into_iter().find(|r| r.repo.eq_ignore_ascii_case(repo.trim())).map(|r| r.login_dirs).unwrap_or_default()
}

/// The folder a repo's card sessions start in: its `folder` in `repos.json` (repo names compare
/// without case), else the home folder.
pub fn repo_folder_in(list: &[Repo], repo: Option<&str>, home: &Path) -> PathBuf {
    repo.and_then(|r| folder_in(list, r, home)).unwrap_or_else(|| home.to_path_buf())
}

/// `repo`'s own `folder` in `list` (names compare without case), if it has one: unlike
/// `repo_folder_in`, no home folder for a repo that isn't listed.
pub fn folder_in(list: &[Repo], repo: &str, home: &Path) -> Option<PathBuf> {
    list.iter().find(|x| x.repo.eq_ignore_ascii_case(repo.trim())).filter(|x| !x.folder.trim().is_empty()).map(|x| expand(x.folder.trim(), home))
}

/// `repo`'s `folder` in `repos.json`, if listed with one: the last place the merge lane looks
/// for a repo's checkout (card #162).
pub fn folder_for(repo: &str) -> Option<PathBuf> {
    folder_in(&repos(), repo, &home())
}

/// One row of Settings' repo list (card #155): what Hugh can change there. Everything else a
/// repo has in `repos.json` (`loginDirs`, any field a later build adds) is kept as it was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoEdit {
    pub repo: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub folder: String,
    /// The repo's name as it was loaded, so a renamed row keeps its other fields. None: a new row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was: Option<String>,
}

/// `owner/name`: one slash, each part of letters, digits, `-`, `_` or `.`, as GitHub allows.
fn valid_repo(r: &str) -> bool {
    let ok = |p: &str| !p.is_empty() && p != "." && p != ".." && p.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    matches!(r.split_once('/'), Some((o, n)) if ok(o) && ok(n))
}

/// A value as one line, the way `repos.json` is written by hand: `{ "a": 1, "b": ["x", "y"] }`.
fn one_line(v: &serde_json::Value) -> String {
    use serde_json::Value;
    match v {
        Value::Object(m) if m.is_empty() => "{}".into(),
        Value::Object(m) => format!(
            "{{ {} }}",
            m.iter().map(|(k, v)| format!("{}: {}", Value::String(k.clone()), one_line(v))).collect::<Vec<_>>().join(", ")
        ),
        Value::Array(a) => format!("[{}]", a.iter().map(one_line).collect::<Vec<_>>().join(", ")),
        other => other.to_string(),
    }
}

/// The file's entries, each checked as `repos_in` reads it, so Settings never shows an empty list
/// for a file it couldn't read (and a Save then never drops what it couldn't show).
fn strict_entries(text: Option<&str>) -> Result<Vec<serde_json::Map<String, serde_json::Value>>, String> {
    use serde_json::Value;
    let Some(t) = text.map(str::trim).filter(|t| !t.is_empty()) else { return Ok(Vec::new()) };
    let bad = |e: String| format!("repos.json can't be read ({e}): fix it by hand first");
    let list = serde_json::from_str::<Vec<Value>>(t).map_err(|e| bad(e.to_string()))?;
    list.into_iter()
        .map(|v| {
            serde_json::from_value::<Repo>(v.clone()).map_err(|e| bad(e.to_string()))?;
            match v {
                Value::Object(m) => Ok(m),
                _ => Err(bad("an entry isn't an object".into())),
            }
        })
        .collect()
}

/// The repo list as Settings edits it (card #155): each named repo's name, label and folder as
/// written (no label filled in), in file order. Refused when the file can't be read.
pub fn edit_rows_in(dir: &Path) -> Result<Vec<RepoEdit>, String> {
    let text = std::fs::read_to_string(dir.join("repos.json")).ok();
    let s = |m: &serde_json::Map<String, serde_json::Value>, k: &str| m.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    Ok(strict_entries(text.as_deref())?
        .iter()
        .filter(|m| !s(m, "repo").is_empty())
        .map(|m| RepoEdit { repo: s(m, "repo"), label: s(m, "label"), folder: s(m, "folder"), was: Some(s(m, "repo")) })
        .collect())
}

/// Writes `repos` as the private folder's `repos.json`, in their order (card #155). Each row is
/// checked (an `owner/name`, no repo twice); a repo already in the file keeps its other fields (a
/// loaded row is matched by the name it was loaded with, so a rename keeps them too); a row's empty
/// label or folder is left out (the repo's name, the home folder). An entry with no repo name
/// (Settings never shows one) is kept at the end. A changed file's old text is kept as
/// `repos.json.bak`, and the new one is written whole before it replaces the old. A file that
/// can't be read is refused, so nothing in it is lost. Returns the list as `edit_rows_in` reads it.
pub fn save_repos_in(dir: &Path, repos: &[RepoEdit]) -> Result<Vec<RepoEdit>, String> {
    use serde_json::{Map, Value};
    if !dir.is_dir() {
        return Err(format!("the private folder {} isn't there", dir.display()));
    }
    let mut seen = std::collections::HashSet::new();
    for r in repos {
        let name = r.repo.trim();
        if !valid_repo(name) {
            return Err(format!("\"{name}\" isn't a repo: give it as owner/name"));
        }
        if !seen.insert(name.to_ascii_lowercase()) {
            return Err(format!("{name} is listed twice"));
        }
        if r.label.contains(['\r', '\n']) || r.folder.contains(['\r', '\n']) {
            return Err(format!("{name}: a label or folder is one line"));
        }
    }
    let file = dir.join("repos.json");
    let old_text = std::fs::read_to_string(&file).ok();
    let old = strict_entries(old_text.as_deref())?;
    let name_of = |o: &Map<String, Value>| o.get("repo").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let mut rows: Vec<Map<String, Value>> = repos
        .iter()
        .map(|r| {
            let name = r.repo.trim();
            let key = r.was.as_deref().map(str::trim).unwrap_or(name);
            let mut m = old.iter().find(|o| name_of(o).eq_ignore_ascii_case(key)).cloned().unwrap_or_default();
            m.insert("repo".into(), Value::String(name.into()));
            for (key, val) in [("folder", r.folder.trim()), ("label", r.label.trim())] {
                if val.is_empty() {
                    m.remove(key);
                } else {
                    m.insert(key.into(), Value::String(val.into()));
                }
            }
            m
        })
        .collect();
    rows.extend(old.iter().filter(|o| name_of(o).is_empty()).cloned());
    // Each row's known fields first, in the order the file has always had, then any others.
    const FIRST: [&str; 4] = ["repo", "folder", "label", "loginDirs"];
    let row_text = |m: &Map<String, Value>| {
        let keys = FIRST.iter().copied().filter(|k| m.contains_key(*k)).chain(m.keys().map(String::as_str).filter(|k| !FIRST.contains(k)));
        format!("  {{ {} }}", keys.map(|k| format!("{}: {}", Value::String(k.into()), one_line(&m[k]))).collect::<Vec<_>>().join(", "))
    };
    let text = if rows.is_empty() {
        "[]\n".to_string()
    } else {
        format!("[\n{}\n]\n", rows.iter().map(row_text).collect::<Vec<_>>().join(",\n"))
    };
    // Unchanged: nothing is written, so the .bak still holds the file before the last real change.
    if old_text.as_deref() != Some(text.as_str()) {
        let tmp = dir.join("repos.json.tmp");
        std::fs::write(&tmp, &text).map_err(|e| format!("couldn't write repos.json: {e}"))?;
        let done = match &old_text {
            Some(t) => std::fs::write(dir.join("repos.json.bak"), t).map_err(|e| format!("couldn't keep the old repos.json: {e}")),
            None => Ok(()),
        }
        .and_then(|_| std::fs::rename(&tmp, &file).map_err(|e| format!("couldn't replace repos.json: {e}")));
        if let Err(e) = done {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    }
    edit_rows_in(dir)
}

fn need_dir() -> Result<PathBuf, String> {
    dir().ok_or_else(|| "set a private folder first: the repo list is kept in its repos.json".to_string())
}

/// The repo list for Settings (card #155); refused with no private folder set.
pub fn edit_rows() -> Result<Vec<RepoEdit>, String> {
    edit_rows_in(&need_dir()?)
}

/// Saves the repo list in the private folder (card #155); refused with none set.
pub fn save_repos(repos: &[RepoEdit]) -> Result<Vec<RepoEdit>, String> {
    save_repos_in(&need_dir()?, repos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(repo: &str, label: &str, folder: &str) -> RepoEdit {
        RepoEdit { repo: repo.into(), label: label.into(), folder: folder.into(), was: None }
    }

    #[test]
    fn saving_repos_adds_removes_and_keeps_other_fields() {
        let t = tempfile::tempdir().unwrap();
        let old = "[\n  { \"repo\": \"acme/web\", \"folder\": \"~/web\", \"label\": \"web\", \"loginDirs\": [\"server\", \"frontends\"], \"extra\": 1 },\n  { \"repo\": \"acme/docs\", \"label\": \"Docs\" }\n]\n";
        std::fs::write(t.path().join("repos.json"), old).unwrap();
        // Remove acme/docs, rename acme/web's label (given in another case), add acme/new.
        let rows = save_repos_in(t.path(), &[edit("ACME/Web", " Web app ", "~/web"), edit("acme/new", "", "")]).unwrap();
        // As Settings shows it: no label filled in.
        assert_eq!(rows[1], RepoEdit { repo: "acme/new".into(), label: "".into(), folder: "".into(), was: Some("acme/new".into()) });
        let list = repos_in(Some(t.path()));
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].repo, "ACME/Web");
        assert_eq!(list[0].label, "Web app");
        assert_eq!(list[0].login_dirs, vec!["server".to_string(), "frontends".to_string()]);
        assert_eq!(list[1].repo, "acme/new");
        assert_eq!(list[1].label, "new");
        let text = std::fs::read_to_string(t.path().join("repos.json")).unwrap();
        assert_eq!(
            text,
            "[\n  { \"repo\": \"ACME/Web\", \"folder\": \"~/web\", \"label\": \"Web app\", \"loginDirs\": [\"server\", \"frontends\"], \"extra\": 1 },\n  { \"repo\": \"acme/new\" }\n]\n"
        );
        // The old file is kept, and no temporary file is left.
        assert_eq!(std::fs::read_to_string(t.path().join("repos.json.bak")).unwrap(), old);
        assert!(!t.path().join("repos.json.tmp").exists());
        // Saving the same list again writes nothing, so the .bak still holds the file before it.
        assert_eq!(save_repos_in(t.path(), &rows).unwrap(), rows);
        assert_eq!(std::fs::read_to_string(t.path().join("repos.json.bak")).unwrap(), old);
        // A renamed row keeps its other fields: it is matched by the name it was loaded with.
        let mut renamed = rows.clone();
        renamed[0].repo = "acme/website".into();
        save_repos_in(t.path(), &renamed).unwrap();
        let list = repos_in(Some(t.path()));
        assert_eq!(list[0].repo, "acme/website");
        assert_eq!(list[0].login_dirs, vec!["server".to_string(), "frontends".to_string()]);
        // Removing every repo leaves an empty list, which lists none.
        assert!(save_repos_in(t.path(), &[]).unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(t.path().join("repos.json")).unwrap(), "[]\n");
    }

    #[test]
    fn an_entry_with_no_repo_name_is_kept() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("repos.json"), r#"[{ "repo": "", "note": "spare" }, { "repo": "a/b" }]"#).unwrap();
        assert_eq!(edit_rows_in(t.path()).unwrap().len(), 1);
        save_repos_in(t.path(), &[edit("a/c", "", "")]).unwrap();
        assert_eq!(std::fs::read_to_string(t.path().join("repos.json")).unwrap(), "[\n  { \"repo\": \"a/c\" },\n  { \"repo\": \"\", \"note\": \"spare\" }\n]\n");
    }

    #[test]
    fn saving_repos_with_no_file_makes_one() {
        let t = tempfile::tempdir().unwrap();
        save_repos_in(t.path(), &[edit("me/app", "App", "~\\app")]).unwrap();
        let list = repos_in(Some(t.path()));
        assert_eq!(list[0].folder, "~\\app");
        assert!(!t.path().join("repos.json.bak").exists());
        let h = Path::new(r"C:\Users\u");
        assert_eq!(repo_folder_in(&list, Some("me/app"), h), h.join("app"));
    }

    #[test]
    fn saving_repos_refuses_bad_rows_and_changes_nothing() {
        let t = tempfile::tempdir().unwrap();
        let old = r#"[{ "repo": "acme/web" }]"#;
        std::fs::write(t.path().join("repos.json"), old).unwrap();
        for bad in [
            vec![edit("acme", "", "")],
            vec![edit("acme/web/x", "", "")],
            vec![edit("ac me/web", "", "")],
            vec![edit("acme/..", "", "")],
            vec![edit("", "", "")],
            vec![edit("acme/web", "", ""), edit("Acme/WEB", "", "")],
            vec![edit("acme/web", "a\nb", "")],
        ] {
            assert!(save_repos_in(t.path(), &bad).is_err(), "{:?}", bad.iter().map(|r| &r.repo).collect::<Vec<_>>());
        }
        assert_eq!(std::fs::read_to_string(t.path().join("repos.json")).unwrap(), old);
        // A file the board can't read is neither shown nor overwritten: not a list, an entry that
        // isn't an object, a null label, a loginDirs that isn't a list, an entry with no repo.
        for text in ["not json", r#"["acme/web"]"#, r#"[{ "repo": "a/b", "label": null }]"#, r#"[{ "repo": "a/b", "loginDirs": "server" }]"#, r#"[{ "label": "x" }]"#] {
            std::fs::write(t.path().join("repos.json"), text).unwrap();
            assert!(edit_rows_in(t.path()).unwrap_err().contains("fix it by hand"), "{text}");
            assert!(save_repos_in(t.path(), &[edit("acme/web", "", "")]).unwrap_err().contains("fix it by hand"), "{text}");
            assert_eq!(std::fs::read_to_string(t.path().join("repos.json")).unwrap(), text);
        }
        // Nor is a folder that isn't there written.
        assert!(save_repos_in(&t.path().join("gone"), &[]).is_err());
    }

    #[test]
    fn tilde_expands_under_home() {
        let h = Path::new(r"C:\Users\u");
        assert_eq!(expand(r"~\work\app", h), h.join(r"work\app"));
        assert_eq!(expand("~", h), h.to_path_buf());
        assert_eq!(expand(r"D:\x", h), PathBuf::from(r"D:\x"));
    }

    #[test]
    fn private_prompts_win_only_when_the_folder_exists() {
        let t = tempfile::tempdir().unwrap();
        let bundled = t.path().join("bundled");
        assert_eq!(prompts_in(None, bundled.clone()), bundled);
        assert_eq!(prompts_in(Some(t.path()), bundled.clone()), bundled);
        std::fs::create_dir(t.path().join("prompts")).unwrap();
        assert_eq!(prompts_in(Some(t.path()), bundled), t.path().join("prompts"));
    }

    #[test]
    fn repos_come_from_repos_json() {
        let t = tempfile::tempdir().unwrap();
        assert!(repos_in(Some(t.path())).is_empty());
        std::fs::write(
            t.path().join("repos.json"),
            r#"[{ "repo": "acme/web", "folder": "~\\web-main", "loginDirs": ["server"] }, { "repo": "acme/docs", "label": "Docs" }, { "repo": " " }]"#,
        )
        .unwrap();
        let list = repos_in(Some(t.path()));
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].label, "web");
        assert_eq!(list[1].label, "Docs");
        assert_eq!(list[0].login_dirs, vec!["server".to_string()]);
        assert!(list[1].login_dirs.is_empty());
        let h = Path::new(r"C:\Users\u");
        assert_eq!(repo_folder_in(&list, Some("ACME/Web"), h), h.join("web-main"));
        assert_eq!(repo_folder_in(&list, Some("acme/docs"), h), h.to_path_buf());
        assert_eq!(repo_folder_in(&list, Some("other/x"), h), h.to_path_buf());
        assert_eq!(repo_folder_in(&list, None, h), h.to_path_buf());
        // The merge lane's last place to look (card #162): only a listed folder, never home.
        assert_eq!(folder_in(&list, " acme/WEB ", h), Some(h.join("web-main")));
        assert_eq!(folder_in(&list, "acme/docs", h), None);
        assert_eq!(folder_in(&list, "other/x", h), None);
    }

    #[test]
    fn a_broken_repos_json_lists_none() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("repos.json"), "not json").unwrap();
        assert!(repos_in(Some(t.path())).is_empty());
    }
}
