//! The merge lanes (card #160): their own append-only sessions, the opt-in repo list, the
//! lanes' model and effort, and the jobs the cards' `merge` records hold.

use kanban_store::{lane_key, lane_repo, merge_state, page_default, MergeLaneRepo, Patch, Store, LANE_EFFORT_KEY, LANE_MODEL_KEY, MERGE_KEY, MERGE_LANE_REPOS_KEY, MERGE_STATES, MERGE_WAITING, PAGE_SETTINGS};
use rusqlite::Connection;
use serde_json::{json, Value};

fn patch(v: Value) -> Patch {
    v.as_object().cloned().unwrap()
}

fn board() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("board.db")).unwrap();
    (dir, s)
}

/// A folder that looks like a git checkout (it has a .git), for the list's check.
fn checkout(dir: &tempfile::TempDir, name: &str) -> String {
    let p = dir.path().join(name);
    std::fs::create_dir_all(p.join(".git")).unwrap();
    p.display().to_string()
}

#[test]
fn migration_16_adds_an_append_only_lane_sessions_table() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), kanban_store::SCHEMA_VERSION);
    assert!(kanban_store::SCHEMA_VERSION >= 16);
    s.add_lane_session("l1", "Acme/Web", "c1", "live", "main", "C:/co/.claude/worktrees/merge-lane", "claude-opus-5-5", "high").unwrap();
    drop(s);
    let c = Connection::open(&path).unwrap();
    assert!(c.execute(&format!("{} FROM lane_sessions", "DELETE"), []).is_err(), "lane_sessions refuses deletes");
    assert!(c.execute("UPDATE lane_sessions SET seq = seq", []).is_err(), "lane_sessions refuses updates");
}

#[test]
fn an_older_board_is_backed_up_before_migration_16() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    drop(Store::open(&path).unwrap());
    {
        let c = Connection::open(&path).unwrap();
        c.pragma_update(None, "user_version", 15).unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), kanban_store::SCHEMA_VERSION);
    let backups: Vec<_> = std::fs::read_dir(s.backups_dir()).unwrap().filter_map(|e| e.ok()).collect();
    assert!(backups.iter().any(|e| e.file_name().to_string_lossy().contains("pre-migration")));
}

/// An older build (one migration short) opens a board at this version as it is.
#[test]
fn a_board_at_this_version_opens_on_an_older_build() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    {
        let mut s = Store::open(&path).unwrap();
        s.add_lane_session("l1", "acme/web", "c1", "live", "main", "C:/x", "claude-opus-5-5", "high").unwrap();
        s.create(None, patch(json!({ "title": "T" })), "test").unwrap();
    }
    {
        let c = Connection::open(&path).unwrap();
        c.pragma_update(None, "user_version", 99).unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), 99, "never migrated down");
    assert_eq!(s.list(false).unwrap().len(), 1);
}

#[test]
fn a_lane_is_never_a_card_and_its_latest_session_is_per_repo_and_channel() {
    let (_d, mut s) = board();
    let before = s.list(true).unwrap().len();
    assert!(s.latest_lane_session("acme/web", "live").unwrap().is_none());
    s.add_lane_session("l1", "Acme/Web", "c1", "live", "main", "C:/x", "claude-opus-5-5", "high").unwrap();
    s.add_lane_session("l2", "acme/web", "c2", "test", "main", "C:/x", "claude-opus-5-5", "high").unwrap();
    s.add_lane_session("l3", "acme/api", "c3", "live", "main", "C:/y", "claude-opus-5-5", "high").unwrap();
    s.add_lane_session("l4", "acme/web", "c1", "live", "main", "C:/x", "claude-opus-5-5", "low").unwrap();
    assert_eq!(s.list(true).unwrap().len(), before, "no card appears for a lane");
    let l = s.latest_lane_session("ACME/web", "live").unwrap().unwrap();
    assert_eq!((l.id.as_str(), l.claude_session_id.as_str(), l.repo.as_str(), l.effort.as_deref()), ("l4", "c1", "acme/web", Some("low")));
    assert_eq!(s.latest_lane_session("acme/web", "test").unwrap().unwrap().id, "l2");
    assert_eq!(s.latest_lane_session("acme/api", "live").unwrap().unwrap().id, "l3");
    assert!(s.latest_lane_session("acme/api", "test").unwrap().is_none());
    assert!(s.is_lane_row("l3").unwrap());
    assert_eq!(s.lane_of_row("l1").unwrap().as_deref(), Some("acme/web"));
    assert!(!s.is_lane_row("someone-else").unwrap());
    // Lane rows are not the Router's, and the other way round.
    s.add_router_session("r1", "c9", "live", "main", "C:/r", "claude-sonnet-5-5", "low").unwrap();
    assert!(!s.is_lane_row("r1").unwrap());
    assert!(!s.is_router_row("l1").unwrap());
}

#[test]
fn lane_keys_are_the_repo_lower_cased() {
    assert_eq!(lane_key(" Hughht5/Claude-Kanban-DSH "), "lane:hughht5/claude-kanban-dsh");
    assert_eq!(lane_repo("lane:acme/web"), Some("acme/web"));
    assert_eq!(lane_repo("lane:"), None);
    assert_eq!(lane_repo("router"), None);
    assert_eq!(lane_repo("card-12"), None);
}

#[test]
fn the_list_is_empty_by_default_and_checked_like_the_own_worktree_list() {
    let (dir, mut s) = board();
    assert!(PAGE_SETTINGS.contains(&MERGE_LANE_REPOS_KEY));
    assert_eq!(page_default(MERGE_LANE_REPOS_KEY).unwrap(), "[]");
    assert!(s.merge_lane_repos().is_empty());
    assert!(s.merge_lane_repo_for("acme/web").is_none());
    let co = checkout(&dir, "repo");
    let good = json!([{ "repo": " acme/web ", "checkout": format!(" {co}/ ") }]).to_string();
    let stored = s.set_page_setting_by(MERGE_LANE_REPOS_KEY, &good, "Hugh (settings)").unwrap();
    let want = vec![MergeLaneRepo { repo: "acme/web".into(), checkout: co.clone() }];
    assert_eq!(serde_json::from_str::<Vec<MergeLaneRepo>>(&stored).unwrap(), want);
    assert_eq!(s.merge_lane_repo_for("ACME/Web"), Some(want[0].clone()));
    assert!(s.merge_lane_repo_for("acme/api").is_none());
    assert!(s.merge_lane_repo_for(" ").is_none());
    assert_eq!(s.setting_log(MERGE_LANE_REPOS_KEY, 5).unwrap().len(), 1);
    let not_git = dir.path().join("plain");
    std::fs::create_dir_all(&not_git).unwrap();
    for bad in [
        "{broken".to_string(),
        json!([{ "repo": "web", "checkout": co }]).to_string(),
        json!([{ "repo": "acme/web", "checkout": "relative/path" }]).to_string(),
        json!([{ "repo": "acme/web", "checkout": not_git.display().to_string() }]).to_string(),
        json!([{ "repo": "acme/web", "checkout": co }, { "repo": "ACME/web", "checkout": co }]).to_string(),
        json!([{ "repo": "acme/web" }]).to_string(),
    ] {
        assert!(s.set_page_setting(MERGE_LANE_REPOS_KEY, &bad).is_err(), "{bad}");
    }
    assert_eq!(s.merge_lane_repos(), want, "a refused list keeps the saved one");
    // A saved value that no longer parses counts as none: ships use the card-merger.
    s.set_setting(MERGE_LANE_REPOS_KEY, "{broken").unwrap();
    assert!(s.merge_lane_repos().is_empty());
}

#[test]
fn the_lanes_model_and_effort_default_to_opus_high_and_are_checked() {
    let (_d, mut s) = board();
    assert_eq!(s.lane_model_effort(), ("claude-opus-5-5".to_string(), "high".to_string()));
    assert_eq!(page_default(LANE_MODEL_KEY).unwrap(), "claude-opus-5-5");
    assert_eq!(page_default(LANE_EFFORT_KEY).unwrap(), "high");
    assert!(s.set_page_setting(LANE_MODEL_KEY, "gpt").is_err());
    assert!(s.set_page_setting(LANE_EFFORT_KEY, "huge").is_err());
    s.set_page_setting(LANE_MODEL_KEY, "claude-sonnet-5-5").unwrap();
    s.set_page_setting(LANE_EFFORT_KEY, "low").unwrap();
    assert_eq!(s.lane_model_effort(), ("claude-sonnet-5-5".to_string(), "low".to_string()));
}

fn job(state: &str, lane: &str, at: &str) -> Value {
    json!({ "state": state, "lane": lane, "base": "abc1234", "at": at })
}

#[test]
fn the_jobs_query_counts_and_lists_what_each_lane_owes() {
    let (_d, mut s) = board();
    assert_eq!(s.lane_jobs_waiting().unwrap(), 0);
    let web = lane_key("acme/web");
    let a = s.create(None, patch(json!({ "title": "A", "column": "Ship it", "repo": "acme/web", "merge": job("queued", &web, "2026-11-09T10:00:02.000Z") })), "the app").unwrap();
    let b = s.create(None, patch(json!({ "title": "B", "column": "Ship it", "repo": "acme/web", "merge": job("running", &web, "2026-11-09T10:00:05.000Z") })), "the app").unwrap();
    let c = s.create(None, patch(json!({ "title": "C", "column": "Ship it", "repo": "acme/web", "merge": job("queued", &web, "2026-11-09T10:00:01.000Z") })), "the app").unwrap();
    s.create(None, patch(json!({ "title": "D", "column": "Ship it", "repo": "acme/api", "merge": job("queued", &lane_key("acme/api"), "2026-11-09T10:00:00.000Z") })), "the app").unwrap();
    // Verdicts and cancelled jobs are owed nothing.
    for st in ["passed", "failed", "unclear", "cancelled"] {
        s.create(None, patch(json!({ "title": st, "column": "Ship it", "repo": "acme/web", "merge": job(st, &web, "2026-11-09T09:00:00.000Z") })), "the app").unwrap();
    }
    // A card-merger report has no lane.
    s.create(None, patch(json!({ "title": "M", "repo": "acme/web", "merge": { "state": "passed", "at": "2026-11-09T08:00:00.000Z" } })), "the app").unwrap();
    assert_eq!(s.lane_jobs_waiting().unwrap(), 4);
    let jobs: Vec<i64> = s.lane_jobs(&web).unwrap().iter().map(|c| c.number).collect();
    assert_eq!(jobs, vec![b.number, c.number, a.number], "the running job first, then the queue, oldest first");
    assert_eq!(merge_state(&s.get(&b.id).unwrap()), Some("running"));
    assert!(s.lane_jobs("lane:nobody/none").unwrap().is_empty());
    let all: Vec<(String, i64)> = s.lane_jobs_all().unwrap().iter().map(|c| (c.fields[MERGE_KEY]["lane"].as_str().unwrap().to_string(), c.number)).collect();
    assert_eq!(all.len(), 4);
    assert_eq!(all[0].0, "lane:acme/api", "grouped by lane");
    assert_eq!(all[1..].iter().map(|(_, n)| *n).collect::<Vec<_>>(), vec![b.number, c.number, a.number]);
    // An archived card's job still counts, so the lane cancels it.
    let v = s.get(&a.id).unwrap().version;
    s.archive(&a.id, v, true, "Hugh").unwrap();
    assert_eq!(s.lane_jobs_waiting().unwrap(), 4);
    assert!(MERGE_WAITING.iter().all(|w| MERGE_STATES.contains(w)));
}

#[test]
fn the_history_is_the_repos_last_finished_merges_newest_first() {
    let (_d, mut s) = board();
    let mk = |s: &mut Store, t: &str, repo: &str, m: Value| s.create(None, patch(json!({ "title": t, "repo": repo, "merge": m })), "the app").unwrap();
    let one = mk(&mut s, "one", "acme/web", json!({ "state": "passed", "at": "2026-11-09T01:00:00.000Z", "head": "aaaaaaa" }));
    let two = mk(&mut s, "two", "Acme/Web", json!({ "state": "failed", "at": "2026-11-09T03:00:00.000Z" }));
    let three = mk(&mut s, "three", "acme/web", json!({ "state": "passed", "at": "2026-11-09T02:00:00.000Z" }));
    mk(&mut s, "queued", "acme/web", json!({ "state": "queued", "at": "2026-11-09T04:00:00.000Z" }));
    mk(&mut s, "other", "acme/api", json!({ "state": "passed", "at": "2026-11-09T05:00:00.000Z" }));
    let got: Vec<i64> = s.recent_merges("acme/web", 5, None).unwrap().iter().map(|c| c.number).collect();
    assert_eq!(got, vec![two.number, three.number, one.number]);
    let got: Vec<i64> = s.recent_merges("ACME/WEB", 2, Some(&two.id)).unwrap().iter().map(|c| c.number).collect();
    assert_eq!(got, vec![three.number, one.number]);
}

#[test]
fn a_board_copy_cancels_the_sources_lane_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("live.db");
    let mut s = Store::open(&live).unwrap();
    let web = lane_key("acme/web");
    let a = s.create(None, patch(json!({ "title": "A", "column": "Ship it", "repo": "acme/web", "merge": job("running", &web, "t") })), "the app").unwrap();
    let b = s.create(None, patch(json!({ "title": "B", "column": "Ship it", "repo": "acme/web", "merge": job("passed", &web, "t") })), "the app").unwrap();
    let to = dir.path().join("test").join("board.db");
    kanban_store::copy_board(&live, &to, false).unwrap();
    let copy = Store::open(&to).unwrap();
    assert_eq!(copy.lane_jobs_waiting().unwrap(), 0);
    let ca = copy.get(&a.id).unwrap();
    assert_eq!(merge_state(&ca), Some("cancelled"));
    assert!(ca.fields[MERGE_KEY]["detail"].as_str().unwrap().contains("board copied"));
    assert_eq!(merge_state(&copy.get(&b.id).unwrap()), Some("passed"), "a verdict stays as it was");
    // The source is only read.
    assert_eq!(s.lane_jobs_waiting().unwrap(), 1);
    let _ = s.get(&a.id).unwrap();
    s.add_lane_session("l1", "acme/web", "c1", "live", "main", "C:/x", "m", "e").unwrap();
}

#[test]
fn a_session_may_not_write_the_merge_record() {
    // The merge record is the app's (card #100); kanban_update_card's list is in src-tauri, so
    // this checks the store keeps clearing it on every move into Ship it, lane job or not.
    let (_d, mut s) = board();
    let c = s.create(None, patch(json!({ "title": "A", "column": "Human test", "repo": "acme/web", "merge": job("cancelled", &lane_key("acme/web"), "t") })), "t").unwrap();
    s.move_by_hugh(&c.id, c.version, "Ship it", Patch::new()).unwrap();
    assert!(!s.get(&c.id).unwrap().fields.contains_key(MERGE_KEY));
}
