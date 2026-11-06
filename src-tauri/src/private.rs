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
}

pub fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\"))
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

/// The bundled example prompts: `prompts\` beside the installed exe, else this repo's own.
fn bundled_prompts() -> PathBuf {
    let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("prompts")));
    match beside {
        Some(p) if p.is_dir() => p,
        _ => Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("prompts"),
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

/// The folder a repo's card sessions start in: its `folder` in `repos.json` (repo names compare
/// without case), else the home folder.
pub fn repo_folder_in(list: &[Repo], repo: Option<&str>, home: &Path) -> PathBuf {
    repo.and_then(|r| list.iter().find(|x| x.repo.eq_ignore_ascii_case(r.trim())))
        .filter(|x| !x.folder.trim().is_empty())
        .map(|x| expand(x.folder.trim(), home))
        .unwrap_or_else(|| home.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            r#"[{ "repo": "acme/web", "folder": "~\\web-main" }, { "repo": "acme/docs", "label": "Docs" }, { "repo": " " }]"#,
        )
        .unwrap();
        let list = repos_in(Some(t.path()));
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].label, "web");
        assert_eq!(list[1].label, "Docs");
        let h = Path::new(r"C:\Users\u");
        assert_eq!(repo_folder_in(&list, Some("ACME/Web"), h), h.join("web-main"));
        assert_eq!(repo_folder_in(&list, Some("acme/docs"), h), h.to_path_buf());
        assert_eq!(repo_folder_in(&list, Some("other/x"), h), h.to_path_buf());
        assert_eq!(repo_folder_in(&list, None, h), h.to_path_buf());
    }

    #[test]
    fn a_broken_repos_json_lists_none() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("repos.json"), "not json").unwrap();
        assert!(repos_in(Some(t.path())).is_empty());
    }
}
