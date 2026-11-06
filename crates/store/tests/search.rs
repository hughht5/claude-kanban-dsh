// The session search index (card #110): crates/store/src/search.rs and search1.sql.

use kanban_store::search::{
    self, entry_key, fts_query, is_interrupted, mask, query_terms, read_lines, search_backups_dir, search_path, since, swap_in,
    NewEntry, SearchIndex, HIDDEN, INDEX_VERSION, LINK_HIDDEN, MAX_ROWS,
};
use std::path::Path;

const NOW: &str = "2026-11-06T12:00:00.000Z";
const SINCE: &str = "2026-08-08T12:00:00.000Z";

fn temp() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let p = search_path(dir.path());
    (dir, p)
}

fn entry(key: &str, kind: &str, text: &str) -> NewEntry {
    NewEntry { key: key.into(), kind: kind.into(), who: String::new(), at: NOW.into(), text: text.into() }
}

fn texts(ix: &SearchIndex, q: &str) -> Vec<String> {
    let f = ix.search(q, None, SINCE, MAX_ROWS).unwrap();
    f.cards.iter().flat_map(|c| c.hits.iter().map(|h| h.snippet.iter().map(|p| p.t.as_str()).collect::<String>())).collect()
}

/// `parts` joined, so a test never writes a whole token in the source.
fn j(parts: &[&str]) -> String {
    parts.concat()
}

#[test]
fn the_schema_is_made_and_refuses_updates_and_deletes() {
    let (_d, p) = temp();
    let mut ix = SearchIndex::open(&p, "2026-11").unwrap();
    assert_eq!(ix.schema_version().unwrap(), 1);
    assert_eq!(ix.month().unwrap().as_deref(), Some("2026-11"));
    assert_eq!(ix.index_version().unwrap(), INDEX_VERSION);
    assert!(ix.writable());
    let f = ix.add_file("C:/t/a.jsonl", "card-1", "s1", None).unwrap();
    ix.add_chunk(f, &[entry("u1#0", "user", "hello there")], 10).unwrap();
    let conn = rusqlite::Connection::open(&p).unwrap();
    for t in ["meta", "files", "entries", "progress"] {
        assert!(conn.execute(&format!("{} FROM {t}", "DELETE"), []).is_err(), "{t} delete");
    }
    assert!(conn.execute("UPDATE files SET card_id = 'x'", []).is_err());
    assert!(conn.execute("UPDATE entries SET text = 'x'", []).is_err());
    assert!(conn.execute("UPDATE progress SET offset = 0", []).is_err());
    assert!(conn.execute("UPDATE meta SET value = '1999-01'", []).is_err());
    // INSERT OR REPLACE on a known key is ignored: the first row stays.
    assert_eq!(conn.execute("INSERT OR REPLACE INTO entries (key, file, kind, who, at, text) VALUES ('u1#0', 1, 'user', '', 'x', 'y')", []).unwrap(), 0);
    assert_eq!(conn.execute("INSERT OR REPLACE INTO files (path, card_id, session, added_at) VALUES ('C:/t/a.jsonl', 'z', 'z', 'z')", []).unwrap(), 0);
    assert_eq!(conn.execute("INSERT OR REPLACE INTO meta (key, value) VALUES ('month', '1999-01')", []).unwrap(), 0);
    let text: String = conn.query_row("SELECT text FROM entries WHERE key = 'u1#0'", [], |r| r.get(0)).unwrap();
    assert_eq!(text, "hello there");
    assert_eq!(ix.files().unwrap()[0].card_id, "card-1");
    assert_eq!(ix.month().unwrap().as_deref(), Some("2026-11"));
    // And the REPLACE leaves the full-text index matching the row.
    assert_eq!(texts(&ix, "hello").len(), 1);
}

#[test]
fn the_same_line_is_stored_once_and_a_line_with_no_uuid_gets_the_file_offset_key() {
    let (_d, p) = temp();
    let mut ix = SearchIndex::open(&p, "2026-11").unwrap();
    let f = ix.add_file("C:/t/a.jsonl", "card-1", "s1", None).unwrap();
    assert_eq!(ix.add_file("C:/t/a.jsonl", "card-2", "s9", None).unwrap(), f, "a known path keeps its id");
    let k = entry_key(Some("abc-uuid"), f, 0, 0);
    assert_eq!(ix.add_chunk(f, &[entry(&k, "user", "invoice one")], 50).unwrap(), 1);
    assert_eq!(ix.add_chunk(f, &[entry(&k, "user", "invoice one")], 50).unwrap(), 0);
    assert_eq!(ix.stats().unwrap().entries, 1);
    // No uuid: the file id, the line's offset and the place.
    assert_eq!(entry_key(None, 3, 1200, 1), "f3@1200#1");
    assert_eq!(entry_key(Some(""), 3, 1200, 1), "f3@1200#1");
    assert_ne!(entry_key(None, 3, 1200, 0), entry_key(None, 3, 1300, 0));
    assert_eq!(entry_key(Some("u"), 3, 1200, 2), "u#2");
    assert_eq!(ix.files().unwrap()[0].card_id, "card-1");
}

#[test]
fn the_latest_progress_wins_and_a_row_is_written_only_when_it_moved() {
    let (_d, p) = temp();
    let mut ix = SearchIndex::open(&p, "2026-11").unwrap();
    let f = ix.add_file("C:/t/a.jsonl", "card-1", "s1", None).unwrap();
    assert_eq!(ix.offset(f).unwrap(), 0);
    ix.add_chunk(f, &[], 100).unwrap();
    ix.add_chunk(f, &[], 250).unwrap();
    ix.add_chunk(f, &[], 250).unwrap();
    assert_eq!(ix.offset(f).unwrap(), 250);
    assert_eq!(ix.offsets().unwrap().get(&f), Some(&250));
    let conn = rusqlite::Connection::open(&p).unwrap();
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM progress", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 2);
}

#[test]
fn an_incomplete_last_line_waits_and_the_offset_stops_before_it() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("t.jsonl");
    std::fs::write(&p, "{\"a\":1}\n{\"b\":2}\n{\"c\":").unwrap();
    let r = read_lines(&p, 0, 1 << 20).unwrap();
    assert_eq!(r.lines, vec![(0, "{\"a\":1}".to_string()), (8, "{\"b\":2}".to_string())]);
    assert_eq!(r.offset, 16);
    // Nothing more until the line ends.
    let again = read_lines(&p, r.offset, 1 << 20).unwrap();
    assert!(again.lines.is_empty());
    assert_eq!(again.offset, 16);
    std::fs::write(&p, "{\"a\":1}\n{\"b\":2}\n{\"c\":3}\r\n").unwrap();
    let done = read_lines(&p, 16, 1 << 20).unwrap();
    assert_eq!(done.lines, vec![(16, "{\"c\":3}".to_string())]);
    assert_eq!(done.offset, 25);
    // A rewritten (shorter) file is read from the start.
    std::fs::write(&p, "{\"z\":1}\n").unwrap();
    assert_eq!(read_lines(&p, 25, 1 << 20).unwrap().lines, vec![(0, "{\"z\":1}".to_string())]);
}

#[test]
fn reads_are_bounded_and_a_line_longer_than_a_chunk_is_read_on_its_own() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("t.jsonl");
    let long = "x".repeat(3000);
    std::fs::write(&p, format!("a\nb\n{long}\nc\n")).unwrap();
    // 4 bytes a chunk: "a\nb\n".
    let r = read_lines(&p, 0, 4).unwrap();
    assert_eq!(r.lines.len(), 2);
    assert!(r.more);
    let r2 = read_lines(&p, r.offset, 4).unwrap();
    assert_eq!(r2.lines, vec![(4, long.clone())]);
    assert_eq!(r2.offset, 4 + 3001);
    let r3 = read_lines(&p, r2.offset, 4).unwrap();
    assert_eq!(r3.lines, vec![(3005, "c".to_string())]);
    assert!(!r3.more);
    // A long line still being written waits too.
    std::fs::write(&p, format!("a\n{long}")).unwrap();
    let w = read_lines(&p, 2, 4).unwrap();
    assert!(w.lines.is_empty());
    assert_eq!(w.offset, 2);
}

fn sample() -> (tempfile::TempDir, SearchIndex) {
    let (d, p) = temp();
    let mut ix = SearchIndex::open(&p, "2026-11").unwrap();
    let a = ix.add_file("C:/t/a.jsonl", "card-1", "s1", None).unwrap();
    let b = ix.add_file("C:/t/b.jsonl", "card-2", "s2", Some("card-tester")).unwrap();
    ix.add_chunk(a, &[
        entry("a1#0", "user", "Fix the InvoiceRequest handler in invoices.ts"),
        entry("a2#0", "assistant", "The invoice code is in server/billing. Done."),
        NewEntry { key: "a3#0".into(), kind: "tool".into(), who: "Bash".into(), at: NOW.into(), text: "Bash\tgrep -rn invoice server/".into() },
    ], 100).unwrap();
    ix.add_chunk(b, &[
        entry("b1#0", "assistant", "Tested the billing page: the invoice total is right."),
        NewEntry { key: "b2#0".into(), kind: "user".into(), who: String::new(), at: "2026-07-01T00:00:00.000Z".into(), text: "an old invoice note".into() },
    ], 100).unwrap();
    (d, ix)
}

#[test]
fn trigrams_match_parts_of_words_and_every_word_must_match() {
    let (_d, ix) = sample();
    // Part of a camelCase name, ignoring case.
    assert_eq!(texts(&ix, "voicereq"), vec!["Fix the InvoiceRequest handler in invoices.ts"]);
    let f = ix.search("invoice", None, SINCE, MAX_ROWS).unwrap();
    assert_eq!(f.total, 4, "the old note is outside the 90 days");
    assert_eq!(f.cards[0].card_id, "card-1", "most matches first");
    assert_eq!(f.cards[0].count, 3);
    assert_eq!(f.cards[1].hits[0].agent.as_deref(), Some("card-tester"));
    // Every word must match.
    assert_eq!(texts(&ix, "invoice billing").len(), 2);
    assert_eq!(texts(&ix, "\"invoice total\"").len(), 1);
    // The card filter.
    let one = ix.search("invoice", Some("card-2"), SINCE, MAX_ROWS).unwrap();
    assert_eq!(one.cards.len(), 1);
    assert_eq!(one.cards[0].card_id, "card-2");
    // The match is marked in the snippet.
    let hit = &f.cards[0].hits[0];
    assert!(hit.snippet.iter().any(|p| p.m && p.t.eq_ignore_ascii_case("invoice")), "{:?}", hit.snippet);
    assert_eq!(hit.who, "Bash");
}

#[test]
fn the_90_day_cut_applies_to_results() {
    let (_d, ix) = sample();
    assert!(texts(&ix, "old invoice note").is_empty());
    let all = ix.search("old invoice note", None, "2026-01-01T00:00:00.000Z", MAX_ROWS).unwrap();
    assert_eq!(all.total, 1);
    let now = chrono::DateTime::parse_from_rfc3339(NOW).unwrap().with_timezone(&chrono::Utc);
    assert_eq!(since(now), SINCE);
}

#[test]
fn odd_queries_never_error() {
    let (_d, ix) = sample();
    for q in ["NEAR(invoice", "\"invoice", "invoice\"", "AND OR NOT", "inv* ^bill", "a\"b\"c", "(((", "col:invoice", "\"\"", "''"] {
        assert!(ix.search(q, None, SINCE, MAX_ROWS).is_ok(), "{q}");
    }
    // Only short words: nothing to search.
    assert!(ix.search("ab cd", None, SINCE, MAX_ROWS).unwrap().short);
    assert!(ix.search("   ", None, SINCE, MAX_ROWS).unwrap().short);
    assert_eq!(query_terms("ab invoice \"a phrase\" x \"open"), vec!["invoice", "a phrase", "open"]);
    assert_eq!(query_terms("Invoice invoice"), vec!["Invoice"]);
    assert_eq!(fts_query(&query_terms("NEAR(a\"b")).unwrap(), "\"NEAR(a\"");
    assert_eq!(fts_query(&["x\"yz".to_string()]).unwrap(), "\"x\"\"yz\"");
    assert!(fts_query(&[]).is_none());
}

// ---- the mask: one test for each kind ----

fn hidden(s: &str, token: &str) {
    let m = mask(s);
    assert!(!m.contains(token), "{token} survived: {m}");
    assert!(m.contains(HIDDEN) || m.contains(LINK_HIDDEN), "{m}");
}

#[test]
fn mask_the_boards_own_words_with_their_values() {
    let v = j(&["Zq81", "valueA"]);
    hidden(&format!("open /login?ct={v}&next=/o"), &v);
    hidden(&format!("{{\"customToken\":\"{v}\"}}"), &v);
    hidden(&format!("custom_token = {v}"), &v);
    hidden(&format!("idToken: {v}"), &v);
    hidden(&format!("password={v}"), &v);
    hidden(&format!("PASSWORD: '{v}'"), &v);
    hidden(&format!("mysql --password {v} db"), &v);
    // Prose about a password keeps its words.
    assert_eq!(mask("never store a password in the log"), "never store a password in the log");
}

#[test]
fn mask_a_jwt_whole() {
    let jwt = j(&["eyJ", "hbGciOiJIUzI1NiJ9", ".", "eyJzdWIiOiIxIn0", ".", "sig_part-XYZ"]);
    let m = mask(&format!("token {jwt} end"));
    assert_eq!(m, format!("token {HIDDEN} end"));
}

#[test]
fn mask_stripe_keys() {
    for k in [j(&["sk", "_live_", "51Habc"]), j(&["sk", "_test_", "4eC39Hq"]), j(&["rk", "_live_", "9xY"]), j(&["rk", "_test_", "0aB"])] {
        hidden(&format!("key {k}."), &k);
    }
}

#[test]
fn mask_github_tokens() {
    for k in [
        j(&["gh", "p_", "16C7e42F292c6912E7710c838347Ae178B4a"]),
        j(&["gh", "o_", "abcDEF123"]),
        j(&["gh", "s_", "abcDEF123"]),
        j(&["gh", "u_", "abcDEF123"]),
        j(&["github", "_pat_", "11ABC_def"]),
    ] {
        hidden(&format!("export GH_TOKEN={k}"), &k);
    }
}

#[test]
fn mask_anthropic_keys() {
    let k = j(&["sk-", "ant-", "api03-AbC_dEf"]);
    hidden(&format!("ANTHROPIC_API_KEY=\"{k}\""), &k);
}

#[test]
fn mask_google_keys_and_oauth_tokens() {
    let k = j(&["AI", "zaSyD-abc_123"]);
    hidden(&format!("?key={k}"), &k);
    let t = j(&["ya", "29.", "a0AfH6SM-x"]);
    hidden(&format!("access {t}"), &t);
}

#[test]
fn mask_slack_tokens() {
    for k in [j(&["xo", "xb-", "123-456-abc"]), j(&["xo", "xp-", "1-2"]), j(&["xo", "xa-", "2-x"]), j(&["xo", "xe-", "1-y"])] {
        hidden(&format!("SLACK={k}"), &k);
    }
}

#[test]
fn mask_bearer_headers() {
    let t = j(&["abc", "DEF.123-x"]);
    let m = mask(&format!("curl -H \"Authorization: Bearer {t}\" http://x"));
    assert!(m.contains(&format!("Bearer {HIDDEN}")) && !m.contains(&t), "{m}");
}

#[test]
fn mask_private_key_blocks() {
    let block = j(&["-----BEGIN ", "RSA PRIVATE KEY-----\nMIIEow\nAAAA\n-----END ", "RSA PRIVATE KEY-----"]);
    let m = mask(&format!("key:\n{block}\nafter"));
    assert_eq!(m, format!("key:\n{HIDDEN}\nafter"));
    // A block cut off before its END line is hidden to the end.
    let cut = j(&["-----BEGIN ", "PRIVATE KEY-----\nMIIEow"]);
    assert_eq!(mask(&format!("x {cut}")), format!("x {HIDDEN}"));
}

#[test]
fn mask_32_hex_runs_such_as_the_api_token() {
    let t = j(&["0123456789abcdef", "0123456789ABCDEF"]);
    hidden(&format!("http://127.0.0.1:4100/mcp/{t}/external"), &t);
    // Commit hashes stay searchable: short, full (40) and SHA-256 (64).
    assert_eq!(mask("commit 0321fbe"), "commit 0321fbe");
    let sha = j(&["0321fbe0321fbe0321fb", "e0321fbe0321fbe0321f"]);
    assert_eq!(mask(&format!("HEAD is {sha}")), format!("HEAD is {sha}"));
    let sha256 = sha.repeat(2)[..64].to_string();
    assert_eq!(mask(&sha256), sha256);
}

#[test]
fn mask_sign_in_links() {
    let a = j(&["http://127.0.0.1:5173/dev-", "signin.html?as=admin&next=/o/acme/home"]);
    assert_eq!(mask(&format!("open {a} now")), format!("open {LINK_HIDDEN} now"));
    let b = j(&["https://x.example/auth/signin-", "token?t=abc"]);
    assert_eq!(mask(&format!("({b})")), format!("({LINK_HIDDEN})"));
}

#[test]
fn stored_text_is_masked() {
    let (_d, p) = temp();
    let mut ix = SearchIndex::open(&p, "2026-11").unwrap();
    let f = ix.add_file("C:/t/a.jsonl", "card-1", "s1", None).unwrap();
    let k = j(&["gh", "p_", "secretvalue123"]);
    ix.add_chunk(f, &[entry("u#0", "user", &format!("use {k} for the push"))], 10).unwrap();
    assert!(texts(&ix, "secretvalue").is_empty());
    let t = texts(&ix, "for the push");
    assert_eq!(t, vec![format!("use {HIDDEN} for the push")]);
    let raw: String = rusqlite::Connection::open(&p).unwrap().query_row("SELECT text FROM entries", [], |r| r.get(0)).unwrap();
    assert!(!raw.contains("secretvalue"));
}

// ---- a new file each month, or for a new index version ----

#[test]
fn a_new_month_builds_a_new_file_carries_its_files_and_keeps_one_backup() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path();
    let mut old = SearchIndex::open(search_path(dir), "2026-10").unwrap();
    let f = old.add_file("C:/t/cleared.jsonl", "card-7", "s-after-clear", None).unwrap();
    old.add_chunk(f, &[entry("u#0", "user", "october words")], 40).unwrap();
    assert!(!old.needs_new("2026-10").unwrap());
    assert!(old.needs_new("2026-11").unwrap());
    let mut new = old.start_new(&search::new_search_path(dir), "2026-11").unwrap();
    assert_eq!(new.month().unwrap().as_deref(), Some("2026-11"));
    let files = new.files().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!((files[0].path.as_str(), files[0].card_id.as_str()), ("C:/t/cleared.jsonl", "card-7"));
    assert_eq!(new.offset(files[0].id).unwrap(), 0, "entries are read again");
    new.add_chunk(files[0].id, &[entry("u#0", "user", "october words")], 40).unwrap();
    drop(new);
    drop(old);
    let b1 = swap_in(dir, "2026-10").unwrap().unwrap();
    assert!(b1.starts_with(search_backups_dir(dir)) && b1.to_string_lossy().ends_with("-search-2026-10.db"));
    let cur = SearchIndex::open(search_path(dir), "2026-11").unwrap();
    assert_eq!(cur.month().unwrap().as_deref(), Some("2026-11"));
    assert_eq!(texts(&cur, "october").len(), 1);
    assert!(!search::new_search_path(dir).exists());
    // The next month: the backup folder still holds one copy, the newest.
    let next = cur.start_new(&search::new_search_path(dir), "2026-12").unwrap();
    drop(next);
    drop(cur);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let b2 = swap_in(dir, "2026-11").unwrap().unwrap();
    let left: Vec<_> = std::fs::read_dir(search_backups_dir(dir)).unwrap().flatten().map(|e| e.path()).collect();
    assert_eq!(left, vec![b2], "one backup, and no -wal or -shm left beside it");
    assert!(!dir.join("search.db-wal").exists());
}

// Windows refuses to rename a file another connection has open; macOS allows it (the open
// reader keeps the old file), so this rule is Windows-only (card #117).
#[cfg(windows)]
#[test]
fn a_swap_refuses_while_the_file_is_open_and_moves_nothing() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path();
    let old = SearchIndex::open(search_path(dir), "2026-10").unwrap();
    drop(old.start_new(&search::new_search_path(dir), "2026-11").unwrap());
    // A search connection still open on the old file.
    let reader = SearchIndex::open_read_only(search_path(dir)).unwrap();
    drop(old);
    assert!(swap_in(dir, "2026-10").is_err());
    assert!(search_path(dir).exists() && search::new_search_path(dir).exists());
    drop(reader);
    assert!(swap_in(dir, "2026-10").unwrap().is_some());
    assert!(!search::new_search_path(dir).exists());
}

#[test]
fn a_half_done_swap_is_finished() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path();
    drop(SearchIndex::open(search::new_search_path(dir), "2026-11").unwrap());
    assert!(search::recover_swap(dir).unwrap());
    assert!(search_path(dir).exists() && !search::new_search_path(dir).exists());
    assert!(!search::recover_swap(dir).unwrap(), "nothing to do the second time");
}

#[test]
fn a_subagent_hit_carries_its_id() {
    let (_d, p) = temp();
    let mut ix = SearchIndex::open(&p, "2026-11").unwrap();
    let f = ix.add_file("C:\\t\\main\\subagents\\agent-a1b2c3.jsonl", "card-1", "s1", Some("card-tester")).unwrap();
    ix.add_chunk(f, &[entry("x#0", "assistant", "the tester checked the invoice")], 10).unwrap();
    let hit = &ix.search("invoice", None, SINCE, 10).unwrap().cards[0].hits[0];
    assert_eq!(hit.agent_id.as_deref(), Some("a1b2c3"));
}

#[test]
fn a_half_built_new_file_is_carried_on_or_moved_aside() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path();
    let old = SearchIndex::open(search_path(dir), "2026-10").unwrap();
    let np = search::new_search_path(dir);
    {
        let mut n = old.start_new(&np, "2026-11").unwrap();
        let f = n.add_file("C:/t/x.jsonl", "card-1", "s", None).unwrap();
        n.add_chunk(f, &[], 99).unwrap();
    }
    // Same month: carried on, progress kept.
    let n = old.start_new(&np, "2026-11").unwrap();
    let id = n.file_id("C:/t/x.jsonl").unwrap().unwrap();
    assert_eq!(n.offset(id).unwrap(), 99);
    drop(n);
    // A different month: the stale one goes into backups\search.
    let n = old.start_new(&np, "2026-12").unwrap();
    assert_eq!(n.month().unwrap().as_deref(), Some("2026-12"));
    assert!(n.file_id("C:/t/x.jsonl").unwrap().is_none());
    assert_eq!(std::fs::read_dir(search_backups_dir(dir)).unwrap().count(), 1);
}

/// A file as a newer build made it: the schema, then `version` in meta and `user_version`.
fn newer_file(p: &Path, version: i64, user_version: i64) {
    let c = rusqlite::Connection::open(p).unwrap();
    c.execute_batch(include_str!("../src/search1.sql")).unwrap();
    c.execute("INSERT INTO meta (key, value) VALUES ('month', '2026-10'), ('index_version', ?1)", [version.to_string()]).unwrap();
    c.pragma_update(None, "user_version", user_version).unwrap();
}

#[test]
fn a_higher_index_version_is_left_alone() {
    let (_d, p) = temp();
    newer_file(&p, INDEX_VERSION + 1, 1);
    let mut ix = SearchIndex::open(&p, "2026-11").unwrap();
    assert!(!ix.writable());
    assert!(!ix.needs_new("2026-11").unwrap(), "no rebuild from a newer build's file");
    assert!(ix.add_file("C:/t/a.jsonl", "c", "s", None).is_err());
    assert!(ix.search("anything", None, SINCE, 10).is_ok(), "searches still work");
}

#[test]
fn a_lower_index_version_builds_a_new_file() {
    let (_d, p) = temp();
    newer_file(&p, INDEX_VERSION - 1, 1);
    let ix = SearchIndex::open(&p, "2026-10").unwrap();
    assert!(ix.writable());
    assert!(ix.needs_new("2026-10").unwrap());
}

#[test]
fn a_newer_schema_is_opened_without_migrating() {
    let (d, p) = temp();
    newer_file(&p, INDEX_VERSION, 7);
    let ix = SearchIndex::open(&p, "2026-11").unwrap();
    assert_eq!(ix.schema_version().unwrap(), 7);
    assert!(!ix.writable());
    assert!(!search_backups_dir(d.path()).exists(), "no migration, so no backup");
}

// ---- speed, and a search stopped by the next ----

/// A 200,000-entry index, built once into cargo's test folder and shared by the tests that
/// need it (it takes about 30 s to build). Its name carries a hash of the schema, so a changed
/// schema builds a new one; it is built under a temporary name, so a half-built one is never used.
fn big() -> std::path::PathBuf {
    use std::hash::{Hash, Hasher};
    static BUILT: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = BUILT.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = std::collections::hash_map::DefaultHasher::new();
    include_str!("../src/search1.sql").hash(&mut h);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let p = dir.join(format!("search-fixture-200k-{:016x}.db", h.finish()));
    if p.exists() {
        return p;
    }
    let tmp = dir.join(format!("search-fixture-building-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let mut ix = SearchIndex::open(&tmp, "2026-11").unwrap();
    for c in 0..50 {
        ix.add_file(&format!("C:/t/{c}.jsonl"), &format!("card-{c}"), "s", None).unwrap();
    }
    drop(ix);
    // Straight into the table (the mask is tested above), so the fixture is quicker to build.
    let mut c = rusqlite::Connection::open(&tmp).unwrap();
    let tx = c.transaction().unwrap();
    {
        let words = ["invoice", "billing", "the", "handler", "request", "session", "card", "board", "search", "index"];
        let mut st = tx.prepare("INSERT INTO entries (key, file, kind, who, at, text) VALUES (?1, ?2, 'assistant', '', ?3, ?4)").unwrap();
        for i in 0..200_000usize {
            let text = format!(
                "{} {} {} line {i} of the fixture with the usual words and some padding text",
                words[i % 10], words[(i / 10) % 10], words[(i / 100) % 10]
            );
            st.execute(rusqlite::params![format!("k{i}"), (i % 50) as i64 + 1, NOW, text]).unwrap();
        }
    }
    tx.commit().unwrap();
    c.pragma_update(None, "journal_mode", "DELETE").unwrap();
    drop(c);
    std::fs::rename(&tmp, &p).unwrap();
    p
}

#[test]
fn a_common_query_on_200000_rows_answers_in_under_300_ms() {
    let p = big();
    let ix = SearchIndex::open_read_only(&p).unwrap();
    // Once to warm the page cache, as a second keystroke would find it.
    ix.search("the", None, SINCE, MAX_ROWS).unwrap();
    let t = std::time::Instant::now();
    let f = ix.search("the", None, SINCE, MAX_ROWS).unwrap();
    let ms = t.elapsed().as_millis();
    println!("search took {ms} ms");
    assert!(f.limited);
    assert_eq!(f.total, MAX_ROWS);
    assert!(f.cards.iter().all(|c| c.capped));
    // A debug build runs SQLite and the regexes unoptimised: the bound is for the app's
    // release build; a debug run gets five times the room.
    let bound = if cfg!(debug_assertions) { 1500 } else { 300 };
    assert!(ms < bound, "took {ms} ms");
}

#[test]
fn a_search_stopped_by_the_next_reports_interrupted() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let p = big();
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel();
    let stop2 = stop.clone();
    // The searching thread owns the connection, as the app's search command does; the next
    // search stops it through the connection's interrupt handle.
    let worker = std::thread::spawn(move || {
        let ix = SearchIndex::open_read_only(&p).unwrap();
        tx.send(ix.interrupt_handle()).unwrap();
        let (mut ok, mut interrupted) = (0, 0);
        while !stop2.load(Ordering::Relaxed) {
            match ix.search("line of the", None, "2000-01-01T00:00:00.000Z", MAX_ROWS) {
                Ok(_) => ok += 1,
                Err(e) if is_interrupted(&e) => interrupted += 1,
                Err(e) => panic!("not an interruption: {e}"),
            }
        }
        // The connection still works after an interruption.
        assert!(ix.search("invoice", None, SINCE, 10).is_ok());
        (ok, interrupted)
    });
    let h = rx.recv().unwrap();
    let t = std::time::Instant::now();
    while t.elapsed() < std::time::Duration::from_millis(400) {
        h.interrupt();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    stop.store(true, Ordering::Relaxed);
    let (_ok, interrupted) = worker.join().unwrap();
    assert!(interrupted > 0, "no search was interrupted");
}
