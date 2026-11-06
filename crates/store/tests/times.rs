// Card #111: time in each column, rebuilt from the card's revisions (times.rs), and the board's
// median per column over visits that ended in the last 30 days.

use kanban_store::{card_columns, column_since, median, medians, timeline, LogIn, Patch, RevIn, Store, Timeline};
use serde_json::{json, Value};
use std::path::PathBuf;

const MIN: i64 = 60_000;
const HOUR: i64 = 60 * MIN;
const DAY: i64 = 24 * HOUR;
/// 6 Nov 2026, 00:00 UTC.
const T0: i64 = 1_793_923_200_000;

fn at(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
fn rev(ms: i64, changed: Value) -> RevIn {
    RevIn { at: at(ms), changed }
}
fn line(ms: i64, text: &str) -> LogIn {
    LogIn { at: at(ms), text: text.into() }
}
fn total(t: &Timeline, col: &str) -> (i64, usize, bool) {
    let c = card_columns(t).into_iter().find(|c| c.column == col).unwrap();
    (c.ms, c.visits, c.running)
}
fn patch(v: Value) -> Patch {
    v.as_object().cloned().unwrap()
}
fn temp_board() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    (dir, path)
}
fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

// ---- The pure rebuild, with fixed times ----

#[test]
fn a_card_that_never_moved_runs_in_its_column_until_now() {
    let revs = [rev(T0, json!({ "created": true, "column": "Backlog" })), rev(T0 + HOUR, json!({ "summary": "x" }))];
    let t = timeline(&revs, &[], "Backlog", false, T0 + 3 * HOUR);
    assert_eq!(total(&t, "Backlog"), (3 * HOUR, 1, true));
    assert_eq!(t.start, Some(T0));
    assert_eq!(t.unknown_ms, 0);
}

#[test]
fn several_visits_add_up_and_count() {
    let revs = [
        rev(T0, json!({ "created": true, "column": "Planning" })),
        rev(T0 + HOUR, json!({ "column": "In progress" })),
        rev(T0 + 3 * HOUR, json!({ "column": "Human test" })),
        rev(T0 + 4 * HOUR, json!({ "column": "In progress" })),
        rev(T0 + 4 * HOUR + 30 * MIN, json!({ "column": "Human test" })),
    ];
    let t = timeline(&revs, &[], "Human test", false, T0 + 6 * HOUR);
    assert_eq!(total(&t, "Planning"), (HOUR, 1, false));
    assert_eq!(total(&t, "In progress"), (2 * HOUR + 30 * MIN, 2, false));
    assert_eq!(total(&t, "Human test"), (HOUR + 90 * MIN, 2, true));
    assert_eq!(total(&t, "Blocked"), (0, 0, false));
    // Done is never a row.
    assert!(card_columns(&t).iter().all(|c| c.column != "Done"));
}

#[test]
fn archive_stops_the_clock_and_restore_carries_on_the_same_visit() {
    let revs = [
        rev(T0, json!({ "created": true, "column": "Human test" })),
        rev(T0 + HOUR, json!({ "archived": true, "archivedAt": at(T0 + HOUR) })),
        rev(T0 + 5 * HOUR, json!({ "archived": false, "archivedAt": null })),
    ];
    let t = timeline(&revs, &[], "Human test", false, T0 + 6 * HOUR);
    // 1h before the archive and 1h after the restore: one visit, not two.
    assert_eq!(total(&t, "Human test"), (2 * HOUR, 1, true));
}

#[test]
fn a_card_archived_now_has_no_running_visit_and_its_visit_ended_at_the_archive() {
    let revs = [
        rev(T0, json!({ "created": true, "column": "Backlog" })),
        rev(T0 + HOUR, json!({ "archived": true })),
    ];
    let t = timeline(&revs, &[], "Backlog", true, T0 + 9 * HOUR);
    assert_eq!(total(&t, "Backlog"), (HOUR, 1, false));
    assert_eq!(t.visits[0].end, T0 + HOUR);
    assert!(!t.visits[0].open);
}

#[test]
fn restoring_from_done_counts_the_column_it_goes_back_to_and_never_done() {
    // Archived by reaching Done, then restored to Deploy complete in one revision (restore_patch).
    let revs = [
        rev(T0, json!({ "created": true, "column": "Deploy complete" })),
        rev(T0 + HOUR, json!({ "column": "Done", "archived": true })),
        rev(T0 + 3 * HOUR, json!({ "column": "Deploy complete", "archived": false })),
    ];
    let t = timeline(&revs, &[], "Deploy complete", false, T0 + 4 * HOUR);
    // It left Deploy complete for Done and came back: two visits of an hour each.
    assert_eq!(total(&t, "Deploy complete"), (2 * HOUR, 2, true));
    assert!(t.visits.iter().all(|v| v.column != "Done"));
}

#[test]
fn done_never_collects_time_even_unarchived() {
    // Cards from before card #83 sat in Done unarchived until migration 10 archived them.
    let revs = [
        rev(T0, json!({ "created": true, "column": "Deploy complete" })),
        rev(T0 + HOUR, json!({ "column": "Done" })),
        rev(T0 + 10 * DAY, json!({ "archived": true, "archivedBy": "the app (Done is archived)" })),
    ];
    let t = timeline(&revs, &[], "Done", true, T0 + 11 * DAY);
    assert_eq!(total(&t, "Deploy complete"), (HOUR, 1, false));
    assert!(t.visits.iter().all(|v| v.column != "Done"));
    assert_eq!(t.unknown_ms, 0);
}

#[test]
fn an_older_card_takes_its_first_column_from_the_move_line() {
    let revs = [rev(T0, json!({ "created": true })), rev(T0 + HOUR, json!({ "column": "Planning" }))];
    let log = [line(T0 + HOUR + 2, "Hugh moved it from Backlog to Planning")];
    let t = timeline(&revs, &log, "Planning", false, T0 + 2 * HOUR);
    assert_eq!(total(&t, "Backlog"), (HOUR, 1, false));
    assert_eq!(total(&t, "Planning"), (HOUR, 1, true));
    assert_eq!(t.unknown_ms, 0);
}

#[test]
fn every_real_move_line_is_understood() {
    // Each line the app writes for a move, wherever the move text sits in it.
    let lines = [
        ("Hugh moved it from Blocked to Planning", "Blocked", "Planning"),
        ("Hugh moved it from Planning to In progress", "Planning", "In progress"),
        ("The app moved it from In progress to Human test for Hugh: it passed", "In progress", "Human test"),
        ("Moved from Human test to In progress by card #12's session: a bug", "Human test", "In progress"),
        ("Moved from Human test to Deploy complete with its stack #96", "Human test", "Deploy complete"),
        ("The app moved the stack from In progress to Human test: ready (#97 moved)", "In progress", "Human test"),
        (
            "Ship request cancelled: it ships with stack #96. Moved from Ship it to Human test, where a stack's cards wait until it ships",
            "Ship it",
            "Human test",
        ),
    ];
    for (text, from, to) in lines {
        let revs = [rev(T0, json!({ "created": true })), rev(T0 + HOUR, json!({ "column": to }))];
        let t = timeline(&revs, &[line(T0 + HOUR, text)], to, false, T0 + 2 * HOUR);
        assert_eq!(total(&t, from).0, HOUR, "{text}");
        assert_eq!(t.unknown_ms, 0, "{text}");
    }
}

#[test]
fn a_move_line_for_another_move_or_too_far_away_is_ignored() {
    let revs = [rev(T0, json!({ "created": true })), rev(T0 + HOUR, json!({ "column": "Planning" }))];
    let wrong_to = [line(T0 + HOUR, "Hugh moved it from Backlog to Blocked")];
    let too_late = [line(T0 + HOUR + 6_000, "Hugh moved it from Backlog to Planning")];
    let free_text = [line(T0 + HOUR, "A note: it went from bad to worse")];
    for log in [&wrong_to[..], &too_late[..], &free_text[..]] {
        let t = timeline(&revs, log, "Planning", false, T0 + 2 * HOUR);
        assert_eq!(t.unknown_ms, HOUR);
        assert_eq!(total(&t, "Backlog").0, 0);
        assert_eq!(total(&t, "Planning"), (HOUR, 1, true));
    }
}

#[test]
fn an_older_card_with_no_move_line_leaves_its_first_stretch_uncounted() {
    let revs = [rev(T0, json!({ "created": true })), rev(T0 + HOUR, json!({ "column": "In progress" }))];
    let t = timeline(&revs, &[], "In progress", false, T0 + 2 * HOUR);
    assert_eq!(t.unknown_ms, HOUR);
    assert_eq!(total(&t, "In progress"), (HOUR, 1, true));
    assert!(card_columns(&t).iter().filter(|c| c.column != "In progress").all(|c| c.ms == 0));
}

#[test]
fn an_older_card_that_never_moved_is_in_its_column_now() {
    let revs = [rev(T0, json!({ "created": true }))];
    let t = timeline(&revs, &[], "Blocked", false, T0 + HOUR);
    assert_eq!(total(&t, "Blocked"), (HOUR, 1, true));
}

#[test]
fn an_imported_card_starts_its_clock_at_the_import() {
    let revs = [rev(T0, json!({ "imported": true })), rev(T0 + HOUR, json!({ "column": "Human test" }))];
    let log = [
        // The artifact's own history, before the import: not counted.
        line(T0 - 5 * DAY, "Hugh moved it from Backlog to Planning"),
        line(T0 + HOUR, "Moved from In progress to Human test by a session"),
    ];
    let t = timeline(&revs, &log, "Human test", false, T0 + 2 * HOUR);
    assert!(t.imported);
    assert_eq!(t.start, Some(T0));
    assert_eq!(total(&t, "In progress"), (HOUR, 1, false));
    assert_eq!(total(&t, "Planning").0, 0);
}

#[test]
fn an_imported_card_archived_at_import_counts_nothing_until_restored() {
    let revs = [rev(T0, json!({ "imported": true })), rev(T0 + 2 * HOUR, json!({ "archived": false }))];
    let t = timeline(&revs, &[], "Backlog", false, T0 + 3 * HOUR);
    // An older import's revision says nothing, so a restore (it clears archivedAt) tells.
    assert_eq!(total(&t, "Backlog"), (3 * HOUR, 1, true), "archived:false alone is not a restore");
    let revs = [rev(T0, json!({ "imported": true })), rev(T0 + 2 * HOUR, json!({ "archived": false, "archivedAt": null }))];
    let t = timeline(&revs, &[], "Backlog", false, T0 + 3 * HOUR);
    assert_eq!(total(&t, "Backlog"), (HOUR, 1, true));
}

#[test]
fn an_import_that_records_its_column_and_archive_state_needs_no_guess() {
    let revs = [
        rev(T0, json!({ "imported": true, "column": "Human test", "archived": false })),
        rev(T0 + HOUR, json!({ "column": "In progress" })),
    ];
    let t = timeline(&revs, &[], "In progress", false, T0 + 2 * HOUR);
    assert_eq!(total(&t, "Human test"), (HOUR, 1, false));
    assert_eq!(t.unknown_ms, 0);
    let revs = [rev(T0, json!({ "replaced": true, "column": "Backlog", "archived": true }))];
    let t = timeline(&revs, &[], "Backlog", true, T0 + HOUR);
    assert_eq!(total(&t, "Backlog"), (0, 0, false));
}

#[test]
fn the_import_revisions_record_the_column_and_archive_state() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let dir = path.with_file_name("export");
    std::fs::create_dir(&dir).unwrap();
    let doc = json!({ "number": 7, "title": "imported", "column": "Planning" });
    std::fs::write(dir.join("card-x.json"), doc.to_string()).unwrap();
    s.import_dir(&dir, false).unwrap();
    let t = s.card_times("card-x", now_ms() + HOUR).unwrap();
    let p = t.columns.iter().find(|c| c.column == "Planning").unwrap();
    assert!(p.running && p.ms >= HOUR && t.imported);
    assert_eq!(t.unknown_ms, 0);
}

#[test]
fn a_replace_makes_the_column_unknown_until_the_next_move() {
    let revs = [
        rev(T0, json!({ "created": true, "column": "Planning" })),
        rev(T0 + HOUR, json!({ "replaced": true })),
        rev(T0 + 2 * HOUR, json!({ "column": "In progress" })),
    ];
    let t = timeline(&revs, &[], "In progress", false, T0 + 3 * HOUR);
    assert_eq!(total(&t, "Planning"), (HOUR, 1, false));
    assert_eq!(t.unknown_ms, HOUR);
    assert_eq!(total(&t, "In progress"), (HOUR, 1, true));
}

#[test]
fn a_clock_that_goes_backwards_never_makes_negative_time() {
    let revs = [
        rev(T0, json!({ "created": true, "column": "Planning" })),
        rev(T0 + HOUR, json!({ "column": "In progress" })),
        // A time sync put the clock back 30 minutes.
        rev(T0 + 30 * MIN, json!({ "column": "Human test" })),
        rev(T0 + 2 * HOUR, json!({ "column": "Ship it" })),
    ];
    let t = timeline(&revs, &[], "Ship it", false, T0 + 2 * HOUR);
    assert_eq!(total(&t, "In progress"), (0, 1, false));
    assert_eq!(total(&t, "Human test"), (HOUR, 1, false));
    assert!(card_columns(&t).iter().all(|c| c.ms >= 0));
    // And "now" before the last revision is no time, not negative.
    let t = timeline(&revs, &[], "Ship it", false, T0);
    assert_eq!(total(&t, "Ship it"), (0, 1, true));
}

#[test]
fn medians_take_odd_and_even_counts() {
    assert_eq!(median(&mut []), None);
    assert_eq!(median(&mut [5]), Some(5));
    assert_eq!(median(&mut [9, 1, 5]), Some(5));
    assert_eq!(median(&mut [4, 1, 3, 2]), Some(2)); // (2 + 3) / 2, in whole milliseconds
    assert_eq!(median(&mut [10, 30]), Some(20));
}

#[test]
fn the_board_median_uses_finished_visits_of_the_last_30_days_only() {
    let now = T0 + 40 * DAY;
    let card = |start: i64, plan: i64, build: i64, still_building: bool| {
        let mut revs = vec![rev(start, json!({ "created": true, "column": "Planning" })), rev(start + plan, json!({ "column": "In progress" }))];
        if !still_building {
            revs.push(rev(start + plan + build, json!({ "column": "Human test" })));
        }
        let col = if still_building { "In progress" } else { "Human test" };
        timeline(&revs, &[], col, false, now)
    };
    let ts = [
        card(now - 2 * DAY, HOUR, 2 * HOUR, false),
        card(now - 3 * DAY, 3 * HOUR, 4 * HOUR, false),
        card(now - 4 * DAY, 2 * HOUR, 6 * HOUR, false),
        // Still in In progress: its Planning visit counts, its open In progress one doesn't.
        card(now - 5 * DAY, 4 * HOUR, 0, true),
        // Finished 38 days ago: outside the window.
        card(now - 39 * DAY, 10 * HOUR, 10 * HOUR, false),
    ];
    let m = medians(&ts, now);
    let get = |c: &str| m.iter().find(|x| x.column == c).cloned().unwrap();
    assert_eq!(get("Planning").visits, 4);
    assert_eq!(get("Planning").ms, Some(150 * MIN)); // 1h, 2h, 3h, 4h
    assert_eq!(get("In progress").visits, 3);
    assert_eq!(get("In progress").ms, Some(4 * HOUR)); // 2h, 4h, 6h
    // Human test: every visit is still running.
    assert_eq!(get("Human test").visits, 0);
    assert_eq!(get("Human test").ms, None);
    assert!(m.iter().all(|x| x.column != "Done"));
}

// ---- The store, with real writes ----

#[test]
fn the_create_revision_records_the_column_it_was_made_in() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let c = s.create(None, patch(json!({ "title": "made in Human test", "column": "Human test" })), "test").unwrap();
    let t = s.card_times(&c.id, now_ms() + HOUR).unwrap();
    let ht = t.columns.iter().find(|x| x.column == "Human test").unwrap();
    assert!(ht.ms >= HOUR && ht.running && ht.visits == 1);
    assert_eq!(t.unknown_ms, 0);
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    let changed: String = db
        .query_row("SELECT changed FROM card_revisions WHERE card_id = ?1 AND version = 1", [&c.id], |r| r.get(0))
        .unwrap();
    let changed: Value = serde_json::from_str(&changed).unwrap();
    assert_eq!(changed, json!({ "created": true, "column": "Human test" }));
}

#[test]
fn every_kind_of_move_is_counted() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let c = s.create(None, patch(json!({ "title": "moves" })), "test").unwrap();
    // Hugh's drag.
    let c = s.move_by_hugh(&c.id, c.version, "Planning", Patch::new()).unwrap().card;
    // A session's move (dispatch writes the column with update).
    let c = s.update(&c.id, c.version, &patch(json!({ "column": "In progress" })), "card #1's session").unwrap();
    // The app's auto-progress.
    let c = s.move_for_hugh(&c.id, c.version, "Human test", "it passed").unwrap().card;
    // A reorder in the same column is not a move.
    let c = s.update(&c.id, c.version, &patch(json!({ "order": 2.0, "orderCol": "Human test" })), "Hugh (board)").unwrap();
    // Archive and restore keep the same visit.
    let c = s.archive(&c.id, c.version, true, "Hugh (board)").unwrap();
    let c = s.archive(&c.id, c.version, false, "Hugh (board)").unwrap();
    let t = s.card_timeline(&c.id, now_ms()).unwrap();
    let cols: Vec<&str> = t.visits.iter().map(|v| v.column.as_str()).collect();
    assert_eq!(cols, ["Backlog", "Planning", "In progress", "Human test"]);
    assert!(t.visits.last().unwrap().open);
    assert!(t.visits[..3].iter().all(|v| !v.open));
    assert_eq!(t.unknown_ms, 0);
}

#[test]
fn a_stack_move_is_counted_for_the_children() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let lead = s.create(None, patch(json!({ "title": "lead", "column": "Human test" })), "test").unwrap();
    let kid = s.create(None, patch(json!({ "title": "child", "column": "Human test" })), "test").unwrap();
    s.stack_add(lead.number, kid.number, "test").unwrap();
    let lead = s.get(&lead.id).unwrap();
    s.update(&lead.id, lead.version, &patch(json!({ "column": "Deploy complete" })), "test").unwrap();
    s.stack_after_move(&lead.id, "test").unwrap();
    let kid = s.get(&kid.id).unwrap();
    assert_eq!(kid.column, "Deploy complete");
    let t = s.card_timeline(&kid.id, now_ms()).unwrap();
    let cols: Vec<&str> = t.visits.iter().map(|v| v.column.as_str()).collect();
    assert_eq!(cols, ["Human test", "Deploy complete"]);
}

#[test]
fn reaching_done_stops_the_clock() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let c = s.create(None, patch(json!({ "title": "done" , "column": "Deploy complete" })), "test").unwrap();
    let c = s.move_by_hugh(&c.id, c.version, "Done", Patch::new()).unwrap().card;
    assert!(c.archived);
    let t = s.card_times(&c.id, now_ms() + DAY).unwrap();
    let dc = t.columns.iter().find(|x| x.column == "Deploy complete").unwrap();
    assert!(dc.ms < HOUR && !dc.running);
    assert!(t.columns.iter().all(|x| !x.running));
}

#[test]
fn the_board_medians_leave_dry_run_cards_out() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    for dry in [false, true, true] {
        let c = s.create(None, patch(json!({ "title": "x", "column": "Planning", "dryRun": dry })), "test").unwrap();
        s.update(&c.id, c.version, &patch(json!({ "column": "In progress" })), "test").unwrap();
    }
    let m = s.column_medians(now_ms()).unwrap();
    let p = m.iter().find(|x| x.column == "Planning").unwrap();
    assert_eq!(p.visits, 1);
    assert!(p.ms.is_some());
    let ip = m.iter().find(|x| x.column == "In progress").unwrap();
    assert_eq!(ip.visits, 0, "a running visit isn't finished");
}

#[test]
fn the_board_medians_are_quick_on_a_big_board() {
    // About 500 cards with about 50 revisions each, written straight into a test board.
    let (_d, path) = temp_board();
    let s = Store::open(&path).unwrap();
    drop(s);
    {
        let mut db = rusqlite::Connection::open(&path).unwrap();
        let tx = db.transaction().unwrap();
        let cols = ["Planning", "In progress", "Human test"];
        let start = now_ms() - 20 * DAY;
        for n in 1..=500i64 {
            let id = format!("card-{n}");
            tx.execute(
                "INSERT INTO cards (id, number, col, archived, fields, created_at, updated_at, version) VALUES (?1, ?2, 'Planning', 0, '{}', ?3, ?3, 50)",
                rusqlite::params![id, n, at(start)],
            )
            .unwrap();
            for v in 1..=50i64 {
                let changed = if v == 1 {
                    json!({ "created": true, "column": "Planning" })
                } else if v % 5 == 0 {
                    json!({ "column": cols[(v / 5) as usize % 3] })
                } else {
                    json!({ "summary": format!("edit {v}") })
                };
                tx.execute(
                    "INSERT INTO card_revisions (card_id, version, at, by, changed) VALUES (?1, ?2, ?3, 'test', ?4)",
                    rusqlite::params![id, v, at(start + n * MIN + v * HOUR), changed.to_string()],
                )
                .unwrap();
            }
            tx.execute(
                "INSERT INTO card_log (card_id, at, text, by) VALUES (?1, ?2, 'Hugh moved it from Planning to In progress', 'test')",
                rusqlite::params![id, at(start)],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }
    let s = Store::open(&path).unwrap();
    let t = std::time::Instant::now();
    let m = s.column_medians(now_ms()).unwrap();
    let took = t.elapsed();
    assert!(m.iter().find(|x| x.column == "In progress").unwrap().visits > 1000);
    // Generous limits on a debug build, so a busy machine doesn't fail it: this guards against a
    // per-card query or a quadratic join (minutes), not against a slow second.
    assert!(took < std::time::Duration::from_secs(20), "column_medians took {took:?}");
    let t = std::time::Instant::now();
    let one = s.card_timeline("card-250", now_ms()).unwrap();
    assert!(t.elapsed() < std::time::Duration::from_secs(10), "card_times took {:?}", t.elapsed());
    // Only that card's revisions: its start plus 10 moves (versions 5, 10, ... 50) are 11 visits.
    assert_eq!(one.visits.len(), 11);
}

// ---- Card #120: when a card entered its column ----

#[test]
fn column_since_is_the_start_of_the_open_visit() {
    let revs = [
        rev(T0, json!({ "created": true, "column": "Planning" })),
        rev(T0 + HOUR, json!({ "column": "In progress" })),
        rev(T0 + 2 * HOUR, json!({ "summary": "x" })),
        rev(T0 + 3 * HOUR, json!({ "column": "Human test" })),
    ];
    let t = timeline(&revs, &[], "Human test", false, T0 + 5 * HOUR);
    assert_eq!(column_since(&t), Some(T0 + 3 * HOUR));
    // Never moved: its created time.
    let t = timeline(&revs[..1], &[], "Planning", false, T0 + 5 * HOUR);
    assert_eq!(column_since(&t), Some(T0));
    // Archived, in Done, or no history: not known.
    let t = timeline(&[rev(T0, json!({ "created": true, "column": "Planning" })), rev(T0 + HOUR, json!({ "archived": true }))], &[], "Planning", true, T0 + 5 * HOUR);
    assert_eq!(column_since(&t), None);
    let t = timeline(&[rev(T0, json!({ "created": true, "column": "Deploy complete" })), rev(T0 + HOUR, json!({ "column": "Done" }))], &[], "Done", false, T0 + 5 * HOUR);
    assert_eq!(column_since(&t), None);
    assert_eq!(column_since(&timeline(&[], &[], "Backlog", false, T0)), None);
    // A restore into the same column carries on the same visit, so the time it came in stands.
    let revs = [
        rev(T0, json!({ "created": true, "column": "Planning" })),
        rev(T0 + HOUR, json!({ "archived": true })),
        rev(T0 + 2 * HOUR, json!({ "archived": false, "archivedAt": null })),
    ];
    assert_eq!(column_since(&timeline(&revs, &[], "Planning", false, T0 + 3 * HOUR)), Some(T0));
}

#[test]
fn column_since_all_gives_every_card_its_time_in_one_pass() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let moved = s.create(None, patch(json!({ "title": "moved twice" })), "test").unwrap();
    let moved = s.update(&moved.id, moved.version, &patch(json!({ "column": "Planning" })), "test").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let moved = s.update(&moved.id, moved.version, &patch(json!({ "column": "In progress" })), "test").unwrap();
    let still = s.create(None, patch(json!({ "title": "never moved", "column": "Planning" })), "test").unwrap();
    let gone = s.create(None, patch(json!({ "title": "archived" })), "test").unwrap();
    s.update(&gone.id, gone.version, &patch(json!({ "archived": true })), "test").unwrap();
    let all = s.column_since_all(now_ms()).unwrap();
    assert_eq!(all.len(), 3);
    let second_move = s.entered_at(&moved.id, "In progress").unwrap();
    assert!(second_move.is_some());
    assert_eq!(all[&moved.id], second_move);
    assert_ne!(all[&moved.id], s.entered_at(&moved.id, "Planning").unwrap());
    // The create revision records the column it was made in, so a card never moved has its created time.
    assert_eq!(all[&still.id], s.entered_at(&still.id, "Planning").unwrap());
    assert_eq!(all[&gone.id], None);
}

#[test]
fn column_since_of_takes_listed_cards_and_the_marker_moves_with_history() {
    let (_d, path) = temp_board();
    let mut s = Store::open(&path).unwrap();
    let a = s.create(None, patch(json!({ "title": "a" })), "test").unwrap();
    let b = s.create(None, patch(json!({ "title": "b", "column": "Planning" })), "test").unwrap();
    let cards = s.list(true).unwrap();
    assert_eq!(s.column_since_of(&cards, now_ms()).unwrap(), s.column_since_all(now_ms()).unwrap());
    // Only the cards passed in.
    let one = s.column_since_of(&cards[..1], now_ms()).unwrap();
    assert_eq!(one.len(), 1);
    let m0 = s.history_marker().unwrap();
    assert_eq!(s.history_marker().unwrap(), m0, "reading changes nothing");
    std::thread::sleep(std::time::Duration::from_millis(5));
    s.update(&a.id, a.version, &patch(json!({ "column": "In progress" })), "test").unwrap();
    let m1 = s.history_marker().unwrap();
    assert!(m1.0 > m0.0, "a move writes a revision");
    s.add_log(&b.id, "a note", "test").unwrap();
    let m2 = s.history_marker().unwrap();
    assert_eq!(m2.0, m1.0);
    assert!(m2.1 > m1.1, "a log line moves the log part");
    assert_eq!(s.column_since_all(now_ms()).unwrap()[&a.id], s.entered_at(&a.id, "In progress").unwrap());
}
