// Card #146: the repos whose cards each build in their own git worktree, made by the app when a
// card's first session starts. The list is a page setting, empty (off) by default, checked
// like the small-change list (card #104), and every change is kept in the settings log.

use kanban_store::{page_default, OwnWorktreeRepo, Store, OWN_WORKTREE_REPOS_KEY, PAGE_SETTINGS};
use serde_json::json;
use std::path::PathBuf;

fn temp_board() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    (dir, path)
}

/// A folder that looks like a git checkout (it has a .git), for the list's check.
fn checkout(dir: &tempfile::TempDir, name: &str) -> String {
    let p = dir.path().join(name);
    std::fs::create_dir_all(p.join(".git")).unwrap();
    p.display().to_string()
}

#[test]
fn the_list_is_empty_by_default_and_on_the_page() {
    let (_dir, path) = temp_board();
    let s = Store::open(&path).unwrap();
    assert!(PAGE_SETTINGS.contains(&OWN_WORKTREE_REPOS_KEY));
    assert_eq!(page_default(OWN_WORKTREE_REPOS_KEY).unwrap(), "[]");
    assert_eq!(s.page_setting(OWN_WORKTREE_REPOS_KEY).unwrap(), "[]");
    assert!(s.own_worktree_repos().is_empty());
    assert!(s.own_worktree_repo_for("hughht5/claude-kanban-dsh").is_none());
}

#[test]
fn the_list_is_checked_trimmed_and_found_by_repo_in_any_case() {
    let (dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let co = checkout(&dir, "repo");
    // Two repo names may share one checkout (the old and new names of the same repo).
    let good = json!([
        { "repo": " hughht5/claude-kanban-dsh ", "checkout": format!(" {co}\\ ") },
        { "repo": "hughht5/kanban-app", "checkout": co },
    ])
    .to_string();
    let stored = s.set_page_setting(OWN_WORKTREE_REPOS_KEY, &good).unwrap();
    let want = vec![
        OwnWorktreeRepo { repo: "hughht5/claude-kanban-dsh".into(), checkout: co.clone() },
        OwnWorktreeRepo { repo: "hughht5/kanban-app".into(), checkout: co.clone() },
    ];
    assert_eq!(serde_json::from_str::<Vec<OwnWorktreeRepo>>(&stored).unwrap(), want);
    assert_eq!(s.own_worktree_repos(), want);
    assert_eq!(s.own_worktree_repo_for("HughHT5/Claude-Kanban-DSH"), Some(want[0].clone()));
    assert!(s.own_worktree_repo_for("acme/web").is_none());
    assert!(s.own_worktree_repo_for("  ").is_none());
}

#[test]
fn a_bad_list_is_refused_and_the_saved_one_kept() {
    let (dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let co = checkout(&dir, "repo");
    let ok = json!([{ "repo": "a/b", "checkout": co }]).to_string();
    s.set_page_setting(OWN_WORKTREE_REPOS_KEY, &ok).unwrap();
    let not_git = dir.path().join("plain");
    std::fs::create_dir_all(&not_git).unwrap();
    let bads = [
        "not json".to_string(),
        json!({ "repo": "a/b", "checkout": co }).to_string(),
        json!([{ "repo": "a/b" }]).to_string(),
        json!([{ "repo": "kanban-app", "checkout": co }]).to_string(),
        json!([{ "repo": "a/b/c", "checkout": co }]).to_string(),
        json!([{ "repo": "a/..", "checkout": co }]).to_string(),
        json!([{ "repo": "a/b", "checkout": "relative\\path" }]).to_string(),
        json!([{ "repo": "a/b", "checkout": not_git.display().to_string() }]).to_string(),
        json!([{ "repo": "a/b", "checkout": dir.path().join("missing").display().to_string() }]).to_string(),
        json!([{ "repo": "a/b", "checkout": co }, { "repo": "A/B", "checkout": co }]).to_string(),
    ];
    for bad in &bads {
        assert!(s.set_page_setting(OWN_WORKTREE_REPOS_KEY, bad).is_err(), "{bad}");
    }
    assert_eq!(s.own_worktree_repos(), vec![OwnWorktreeRepo { repo: "a/b".into(), checkout: co }]);
}

#[test]
fn every_change_is_logged_and_empty_turns_it_off() {
    let (dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let co = checkout(&dir, "repo");
    let one = json!([{ "repo": "a/b", "checkout": co }]).to_string();
    s.set_page_setting_by(OWN_WORKTREE_REPOS_KEY, &one, "Hugh (settings)").unwrap();
    // The same value again is no change, so no second line.
    s.set_page_setting_by(OWN_WORKTREE_REPOS_KEY, &one, "Hugh (settings)").unwrap();
    assert_eq!(s.set_page_setting(OWN_WORKTREE_REPOS_KEY, "[]").unwrap(), "[]");
    assert!(s.own_worktree_repos().is_empty());
    let log = s.setting_log(OWN_WORKTREE_REPOS_KEY, 10).unwrap();
    assert_eq!(log.len(), 2, "{log:?}");
    assert_eq!(log[0].value, "[]");
    assert_eq!(log[1].by, "Hugh (settings)");
}

#[test]
fn a_drive_root_keeps_its_separator() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    // C:\ has no .git here, so it is refused, but as "isn't a git checkout", not as a relative path.
    let e = s.set_page_setting(OWN_WORKTREE_REPOS_KEY, &json!([{ "repo": "a/b", "checkout": "C:\\" }]).to_string()).unwrap_err().to_string();
    assert!(!e.contains("full path"), "{e}");
}

#[test]
fn a_saved_value_that_no_longer_parses_counts_as_none() {
    let (_dir, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    s.set_setting(OWN_WORKTREE_REPOS_KEY, "{broken").unwrap();
    assert!(s.own_worktree_repos().is_empty());
    assert!(s.own_worktree_repo_for("a/b").is_none());
}
