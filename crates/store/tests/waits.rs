// Cards that wait on others (card #109): `blockedBy`, its checks, and the app's poll.

use kanban_store::{clean_blocked_by, is_met, wait_path, Card, Error, Patch, Store, WaitKind, SCHEMA_VERSION, WAIT_BY};
use serde_json::{json, Map, Value};

const REPO: &str = "hughht5/kanban-app";

fn patch(v: Value) -> Patch {
    v.as_object().cloned().unwrap()
}

fn board() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("board.db")).unwrap();
    (dir, s)
}

fn mk(s: &mut Store, column: &str) -> Card {
    s.create(None, patch(json!({ "title": "T", "column": column, "repo": REPO })), "test").unwrap()
}

fn get(s: &Store, c: &Card) -> Card {
    s.get(&c.id).unwrap()
}

fn set(s: &mut Store, c: &Card, v: Value) -> kanban_store::Result<Card> {
    let now = get(s, c);
    s.update(&c.id, now.version, &patch(v), "test")
}

fn waits(s: &mut Store, c: &Card, on: &[i64]) -> kanban_store::Result<Card> {
    set(s, c, json!({ "blockedBy": on }))
}

/// Moves it the way a session or the API does (not Hugh's drag).
fn put(s: &mut Store, c: &Card, column: &str) -> Card {
    set(s, c, json!({ "column": column })).unwrap()
}

fn drag(s: &mut Store, c: &Card, column: &str) -> Card {
    let now = get(s, c);
    s.move_by_hugh(&c.id, now.version, column, Patch::new()).unwrap().card
}

fn refused(r: kanban_store::Result<Card>, words: &str) {
    match r {
        Err(Error::Invalid(m)) => assert!(m.contains(words), "{m:?} should say {words:?}"),
        other => panic!("expected a refusal saying {words:?}, got {other:?}"),
    }
}

fn logs(s: &Store, c: &Card) -> Vec<String> {
    s.log(&c.id).unwrap().into_iter().map(|e| e.text).collect()
}

fn card(number: i64, column: &str, archived: bool, repo: &str) -> Card {
    let mut fields = Map::new();
    fields.insert("repo".into(), json!(repo));
    Card { id: format!("c{number}"), number, column: column.into(), order: None, order_col: None, archived, fields, version: 1 }
}

#[test]
fn blocked_by_is_cleaned_in_any_spelling() {
    for v in [json!([12, 40]), json!("12, #40"), json!("#12 #40"), json!(["#12", 40]), json!([12, 40, 12]), json!(["12,40"])] {
        assert_eq!(clean_blocked_by(&v).unwrap(), Some(json!([12, 40])), "{v}");
    }
    assert_eq!(clean_blocked_by(&json!(12)).unwrap(), Some(json!([12])));
    for none in [Value::Null, json!(""), json!([]), json!("  ")] {
        assert_eq!(clean_blocked_by(&none).unwrap(), None, "{none}");
    }
    for bad in [json!("x"), json!(-1), json!(0), json!(true), json!({}), json!([12, "abc"]), json!([1.5]), json!([[1]])] {
        assert!(clean_blocked_by(&bad).is_err(), "{bad}");
    }
    let many: Vec<i64> = (1..=21).collect();
    assert!(clean_blocked_by(&json!(many)).is_err());
    let ok: Vec<i64> = (1..=20).collect();
    assert!(clean_blocked_by(&json!(ok)).is_ok());
}

#[test]
fn a_wait_on_itself_or_a_missing_card_is_refused_and_nothing_is_written() {
    let (_d, mut s) = board();
    let a = mk(&mut s, "Backlog");
    let revs = s.revision_count(&a.id).unwrap();
    refused(waits(&mut s, &a, &[a.number]), "itself");
    refused(waits(&mut s, &a, &[9999]), "no card #9999");
    assert_eq!(s.revision_count(&a.id).unwrap(), revs);
    assert!(get(&s, &a).blocked_by().is_empty());
    // At create too.
    let r = s.create(None, patch(json!({ "title": "T", "blockedBy": [9999] })), "test");
    refused(r, "no card #9999");
}

#[test]
fn archived_and_ready_cards_are_refused_outside_a_stack() {
    let (_d, mut s) = board();
    let a = mk(&mut s, "Backlog");
    let gone = mk(&mut s, "Backlog");
    let gone = s.archive(&gone.id, gone.version, true, "Hugh (board)").unwrap();
    refused(waits(&mut s, &a, &[gone.number]), "archived");
    let ready = mk(&mut s, "Human test");
    refused(waits(&mut s, &a, &[ready.number]), "ready already");
    // In another repo, Human test isn't ready yet, so it can be waited on.
    let other = s.create(None, patch(json!({ "title": "T", "column": "Human test", "repo": "acme/web" })), "test").unwrap();
    waits(&mut s, &a, &[other.number]).unwrap();
}

#[test]
fn siblings_may_wait_on_each_other_whatever_the_column() {
    let (_d, mut s) = board();
    let lead = mk(&mut s, "In progress");
    let x = mk(&mut s, "In progress");
    let y = mk(&mut s, "In progress");
    s.stack_add(lead.number, x.number, "test").unwrap();
    s.stack_add(lead.number, y.number, "test").unwrap();
    put(&mut s, &y, "Human test");
    let x = waits(&mut s, &x, &[y.number]).unwrap();
    assert_eq!(x.blocked_by(), vec![y.number]);
}

#[test]
fn circles_of_any_length_are_refused_with_their_path() {
    let (_d, mut s) = board();
    let a = mk(&mut s, "Backlog");
    let b = mk(&mut s, "Backlog");
    let c = mk(&mut s, "Backlog");
    waits(&mut s, &a, &[b.number]).unwrap();
    let revs = s.revision_count(&b.id).unwrap();
    refused(waits(&mut s, &b, &[a.number]), &format!("#{} → #{} → #{}", b.number, a.number, b.number));
    assert_eq!(s.revision_count(&b.id).unwrap(), revs);
    waits(&mut s, &b, &[c.number]).unwrap();
    refused(waits(&mut s, &c, &[a.number]), &format!("#{} → #{} → #{} → #{}", c.number, a.number, b.number, c.number));
    // Through an archived card too, so restoring it never closes a circle.
    let b = s.archive(&b.id, get(&s, &b).version, true, "Hugh (board)").unwrap();
    refused(waits(&mut s, &c, &[a.number]), "circle");
    let _ = b;
}

#[test]
fn a_lead_and_its_own_child_never_wait_on_each_other() {
    let (_d, mut s) = board();
    let lead = mk(&mut s, "In progress");
    let kid = mk(&mut s, "In progress");
    s.stack_add(lead.number, kid.number, "test").unwrap();
    refused(waits(&mut s, &kid, &[lead.number]), "lead and its own child");
    let lead_now = get(&s, &lead);
    refused(waits(&mut s, &lead_now, &[kid.number]), "lead and its own child");
    // Nor can a join make them so.
    let a = mk(&mut s, "Backlog");
    let b = mk(&mut s, "Backlog");
    waits(&mut s, &a, &[b.number]).unwrap();
    assert!(matches!(s.stack_add(a.number, b.number, "test"), Err(Error::Invalid(m)) if m.contains("waits on")));
    assert!(matches!(s.stack_add(b.number, a.number, "test"), Err(Error::Invalid(m)) if m.contains("waits on")));
    // A card made in a stack can't wait on its lead.
    let r = s.create_in_stack(patch(json!({ "title": "T", "repo": REPO, "blockedBy": [lead.number] })), lead.number, "test");
    assert!(matches!(r, Err(Error::Invalid(m)) if m.contains("lead and its own child")));
    // But it can wait on a sibling, whatever its column, and nothing is left behind when refused.
    put(&mut s, &kid, "Human test");
    let made = s.create_in_stack(patch(json!({ "title": "T", "repo": REPO, "blockedBy": [kid.number] })), lead.number, "test").unwrap();
    assert_eq!((made.stack_of(), made.blocked_by()), (Some(lead.number), vec![kid.number]));
    assert!(logs(&s, &made).contains(&format!("Now waits on #{}", kid.number)));
}

#[test]
fn a_change_is_logged_and_removal_is_a_new_revision() {
    let (_d, mut s) = board();
    let a = mk(&mut s, "Backlog");
    let b = mk(&mut s, "Backlog");
    let c = s.create(None, patch(json!({ "title": "T", "repo": REPO, "blockedBy": format!("#{}", a.number) })), "test").unwrap();
    assert_eq!(c.blocked_by(), vec![a.number]);
    assert!(logs(&s, &c).contains(&format!("Now waits on #{}", a.number)));
    let c = waits(&mut s, &c, &[a.number, b.number]).unwrap();
    assert_eq!(logs(&s, &c)[0], format!("Now waits on #{}", b.number));
    let revs = s.revision_count(&c.id).unwrap();
    let c = set(&mut s, &c, json!({ "blockedBy": null })).unwrap();
    assert!(!c.fields.contains_key("blockedBy"));
    assert_eq!(s.revision_count(&c.id).unwrap(), revs + 1);
    assert_eq!(logs(&s, &c)[0], format!("No longer waits on #{}, #{}", a.number, b.number));
}

#[test]
fn other_writes_keep_the_wait_and_hughs_drag_is_never_refused() {
    let (_d, mut s) = board();
    let a = mk(&mut s, "In progress");
    let c = mk(&mut s, "Backlog");
    waits(&mut s, &c, &[a.number]).unwrap();
    // The blocker became ready since: an unrelated write is still fine and keeps the wait.
    put(&mut s, &a, "Human test");
    let c = set(&mut s, &c, json!({ "summary": "s" })).unwrap();
    assert_eq!(c.blocked_by(), vec![a.number]);
    put(&mut s, &a, "In progress");
    let c = drag(&mut s, &c, "Planning");
    assert_eq!((c.column.as_str(), c.blocked_by()), ("Planning", vec![a.number]));
}

#[test]
fn met_means_human_test_in_the_same_repo_and_shipped_in_another() {
    let w = card(1, "Blocked", false, "hughht5/Kanban-App ");
    for (col, same, other) in [
        ("Backlog", false, false),
        ("Blocked", false, false),
        ("Planning", false, false),
        ("In progress", false, false),
        ("Human test", true, false),
        ("Ship it", true, false),
        ("Deploy complete", true, true),
    ] {
        assert_eq!(is_met(&w, &card(2, col, false, REPO)), same, "same repo, {col}");
        assert_eq!(is_met(&w, &card(2, col, false, "acme/web")), other, "other repo, {col}");
    }
    // No repo on either counts as the same repo.
    assert!(is_met(&card(1, "Blocked", false, ""), &card(2, "Human test", false, "")));
    // Archived: only in Deploy complete or Done.
    assert!(is_met(&w, &card(2, "Deploy complete", true, REPO)));
    assert!(is_met(&w, &card(2, "Done", true, "acme/web")));
    assert!(!is_met(&w, &card(2, "Human test", true, REPO)));
    assert!(!is_met(&w, &card(2, "Backlog", true, REPO)));
}

#[test]
fn a_card_hugh_put_in_blocked_starts_once_every_wait_is_met() {
    let (_d, mut s) = board();
    let a = mk(&mut s, "In progress");
    let a = set(&mut s, &a, json!({ "branch": "dsh-a", "title": "Alpha" })).unwrap();
    let b = mk(&mut s, "In progress");
    let c = mk(&mut s, "Backlog");
    waits(&mut s, &c, &[a.number, b.number]).unwrap();
    let c = drag(&mut s, &c, "Blocked");
    // Set after the drag: Hugh's own move to Blocked turns a Take it to off.
    let c = set(&mut s, &c, json!({ "autoTo": "Human test" })).unwrap();
    assert!(s.settle_waits(false).unwrap().is_empty());
    put(&mut s, &a, "Human test");
    assert!(s.settle_waits(false).unwrap().is_empty(), "one wait is not enough");
    assert_eq!(get(&s, &c).column, "Blocked");
    put(&mut s, &b, "Human test");
    let out = s.settle_waits(false).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!((out[0].kind, out[0].number), (WaitKind::Released, c.number));
    let c = get(&s, &c);
    assert_eq!(c.column, "Planning");
    assert_eq!(c.fields["plan"]["state"], json!("requested"));
    assert_eq!(c.fields["plan"]["waitedOn"], json!([a.number, b.number]));
    assert!(c.fields["plan"].get("auto").is_none());
    assert_eq!(c.auto_to(), Some("Human test"), "its Take it to is kept");
    assert!(c.blocked_by().is_empty());
    assert_eq!(c.waited_on(), vec![a.number, b.number]);
    assert_eq!(c.fields["updatedBy"], json!(WAIT_BY));
    let log = s.log(&c.id).unwrap();
    assert!(log.iter().any(|e| e.by.as_deref() == Some(WAIT_BY)
        && e.text == format!("The app moved it from Blocked to Planning for Hugh: the cards it waited on are ready (#{}, #{})", a.number, b.number)));
    // Its session is told where each one is, and that a Human test one isn't on main yet.
    let note = s.wait_note(&c).unwrap();
    assert!(note.contains(&format!("#{} \"Alpha\": in Human test on branch dsh-a. It is not on main yet", a.number)), "{note}");
    assert!(note.contains("no branch named on the card"), "{note}");
    // Nothing more to do on the next poll.
    assert!(s.settle_waits(false).unwrap().is_empty());
}

#[test]
fn a_lead_started_by_its_waits_starts_alone() {
    // Router decision (whole-build review, N2): a wait release starts only the lead; its
    // children follow the stack's rules (they start on Hugh's own move of the lead).
    let (_d, mut s) = board();
    let a = mk(&mut s, "In progress");
    let lead = mk(&mut s, "Backlog");
    let kid = mk(&mut s, "Backlog");
    s.stack_add(lead.number, kid.number, "test").unwrap();
    let lead_now = get(&s, &lead);
    waits(&mut s, &lead_now, &[a.number]).unwrap();
    drag(&mut s, &lead, "Blocked");
    put(&mut s, &a, "Human test");
    let out = s.settle_waits(false).unwrap();
    assert_eq!((out.len(), out[0].kind, out[0].number), (1, WaitKind::Released, lead.number));
    let lead = get(&s, &lead);
    assert_eq!(lead.column, "Planning");
    assert!(logs(&s, &lead).iter().any(|l| l == kanban_store::LEAD_NOTE));
    assert!(s.wait_note(&lead).unwrap().ends_with(kanban_store::LEAD_NOTE));
    assert_eq!(get(&s, &kid).column, "Backlog", "the store moves no child");
}

#[test]
fn a_card_in_backlog_is_never_moved_hugh_is_told() {
    let (_d, mut s) = board();
    let a = mk(&mut s, "In progress");
    let d = mk(&mut s, "Backlog");
    waits(&mut s, &d, &[a.number]).unwrap();
    put(&mut s, &a, "Human test");
    let out = s.settle_waits(false).unwrap();
    assert_eq!((out.len(), out[0].kind), (1, WaitKind::TellHugh));
    assert!(out[0].text.contains("Drag it to Planning to start it"), "{}", out[0].text);
    let d = get(&s, &d);
    assert_eq!(d.column, "Backlog");
    assert_eq!((d.blocked_by(), d.waited_on()), (vec![], vec![a.number]));
    assert!(logs(&s, &d)[0].contains("Not moved: it is in Backlog"));
    assert!(s.settle_waits(false).unwrap().is_empty(), "told once");
    // It goes back before Hugh starts the card: the card waits on it again, and he is told.
    put(&mut s, &a, "In progress");
    let out = s.settle_waits(false).unwrap();
    assert_eq!((out.len(), out[0].kind), (1, WaitKind::TellHugh));
    assert!(out[0].text.contains("Don't start it yet"), "{}", out[0].text);
    let d = get(&s, &d);
    assert_eq!((d.blocked_by(), d.waited_on()), (vec![a.number], vec![]));
    assert_eq!(logs(&s, &d)[0], format!("#{} went back to In progress, so it waits on #{} again", a.number, a.number));
    assert!(s.settle_waits(false).unwrap().is_empty());
}

#[test]
fn a_wait_that_can_never_be_met_comes_off_and_hugh_is_told() {
    let (_d, mut s) = board();
    let a = mk(&mut s, "In progress");
    let b = mk(&mut s, "Human test");
    let c = mk(&mut s, "Backlog");
    let b2 = mk(&mut s, "In progress");
    waits(&mut s, &c, &[a.number, b2.number]).unwrap();
    let _ = b;
    // Hugh drops #a: archived without shipping, it can never be ready.
    let a = s.archive(&a.id, get(&s, &a).version, true, "Hugh (board)").unwrap();
    let c = drag(&mut s, &c, "Blocked");
    let out = s.settle_waits(false).unwrap();
    assert_eq!((out.len(), out[0].kind), (1, WaitKind::TellHugh));
    assert!(out[0].text.contains(&format!("#{} was archived in In progress without shipping", a.number)), "{}", out[0].text);
    let c = get(&s, &c);
    assert_eq!((c.column.as_str(), c.blocked_by()), ("Blocked", vec![b2.number]));
    assert!(logs(&s, &c)[0].starts_with(&format!("No longer waits on #{}", a.number)));
    // The wait that is left still starts it.
    put(&mut s, &b2, "Human test");
    assert_eq!(s.settle_waits(false).unwrap()[0].kind, WaitKind::Released);
}

#[test]
fn blocked_with_a_reason_or_by_a_session_is_not_moved() {
    let (_d, mut s) = board();
    let a = mk(&mut s, "In progress");
    let asks = mk(&mut s, "Backlog");
    let asks = drag(&mut s, &asks, "Blocked");
    set(&mut s, &asks, json!({ "blockedReason": "Which account?" })).unwrap();
    waits(&mut s, &asks, &[a.number]).unwrap();
    let parked = mk(&mut s, "In progress");
    put(&mut s, &parked, "Blocked");
    waits(&mut s, &parked, &[a.number]).unwrap();
    let made = mk(&mut s, "Blocked");
    waits(&mut s, &made, &[a.number]).unwrap();
    put(&mut s, &a, "Human test");
    let out = s.settle_waits(false).unwrap();
    assert_eq!(out.len(), 3);
    assert!(out.iter().all(|o| o.kind == WaitKind::TellHugh));
    for (c, why) in [(&asks, "it has a blocked reason"), (&parked, "Hugh didn't move it to Blocked himself"), (&made, "Hugh didn't move it to Blocked himself")] {
        let c = get(&s, c);
        assert_eq!(c.column, "Blocked");
        assert!(c.blocked_by().is_empty());
        assert!(logs(&s, &c)[0].contains(why), "{:?}", logs(&s, &c));
    }
}

#[test]
fn a_running_card_is_told_and_keeps_what_it_waited_on() {
    let (_d, mut s) = board();
    let a = mk(&mut s, "In progress");
    let r = mk(&mut s, "In progress");
    waits(&mut s, &r, &[a.number]).unwrap();
    put(&mut s, &a, "Ship it");
    let out = s.settle_waits(false).unwrap();
    assert_eq!((out.len(), out[0].kind), (1, WaitKind::TellSession));
    assert!(out[0].text.contains("not on main yet"), "{}", out[0].text);
    let r = get(&s, &r);
    assert_eq!((r.column.as_str(), r.blocked_by(), r.waited_on()), ("In progress", vec![], vec![a.number]));
}

#[test]
fn a_stack_child_is_left_to_the_stack() {
    let (_d, mut s) = board();
    let lead = mk(&mut s, "In progress");
    let x = mk(&mut s, "In progress");
    let y = mk(&mut s, "In progress");
    s.stack_add(lead.number, x.number, "test").unwrap();
    s.stack_add(lead.number, y.number, "test").unwrap();
    waits(&mut s, &x, &[y.number]).unwrap();
    put(&mut s, &y, "Human test");
    assert!(s.settle_waits(false).unwrap().is_empty());
    assert_eq!(get(&s, &x).blocked_by(), vec![y.number]);
    assert!(!s.any_waits().unwrap(), "a stack's card doesn't keep the poll busy");
}

#[test]
fn the_test_app_starts_only_dry_run_cards() {
    let (_d, mut s) = board();
    let a = mk(&mut s, "In progress");
    let real = mk(&mut s, "Backlog");
    let dry = mk(&mut s, "Backlog");
    let dry = set(&mut s, &dry, json!({ "dryRun": true })).unwrap();
    for c in [&real, &dry] {
        waits(&mut s, c, &[a.number]).unwrap();
        drag(&mut s, c, "Blocked");
    }
    put(&mut s, &a, "Human test");
    let out = s.settle_waits(true).unwrap();
    let kind = |c: &Card| out.iter().find(|o| o.id == c.id).map(|o| o.kind);
    assert_eq!(kind(&real), Some(WaitKind::TellHugh));
    assert_eq!(kind(&dry), Some(WaitKind::Released));
    assert_eq!(get(&s, &real).column, "Blocked");
    assert!(logs(&s, &real)[0].contains("only dry-run cards"));
    assert_eq!(get(&s, &dry).column, "Planning");
}

#[test]
fn a_card_it_waited_on_going_back_is_said_once() {
    let (_d, mut s) = board();
    let a = mk(&mut s, "In progress");
    let b = mk(&mut s, "In progress");
    let c = mk(&mut s, "Backlog");
    waits(&mut s, &c, &[a.number, b.number]).unwrap();
    drag(&mut s, &c, "Blocked");
    put(&mut s, &a, "Human test");
    put(&mut s, &b, "Deploy complete");
    assert_eq!(s.settle_waits(false).unwrap()[0].kind, WaitKind::Released);
    put(&mut s, &a, "In progress");
    let out = s.settle_waits(false).unwrap();
    assert_eq!((out.len(), out[0].kind), (1, WaitKind::WentBack));
    assert!(out[0].text.contains("its work may change"));
    let c = get(&s, &c);
    assert!(c.waited_on().is_empty(), "#b shipped, so nothing is left to watch");
    assert_eq!(logs(&s, &c)[0], format!("#{} went back to In progress, after this card started on it: its work may change", a.number));
    assert!(s.settle_waits(false).unwrap().is_empty(), "said once");
    assert!(!s.any_waits().unwrap(), "and the poll is back to its empty case");
}

#[test]
fn circles_written_without_the_checks_are_found_on_read() {
    let (dir, mut s) = board();
    let a = mk(&mut s, "Backlog");
    let b = mk(&mut s, "Backlog");
    let c = mk(&mut s, "Backlog");
    waits(&mut s, &c, &[a.number]).unwrap();
    drop(s);
    {
        // As an older build or an import could: no checks.
        let conn = rusqlite::Connection::open(dir.path().join("board.db")).unwrap();
        conn.execute("UPDATE cards SET fields = json_set(fields, '$.blockedBy', json(?1)) WHERE id = ?2", rusqlite::params![format!("[{}]", b.number), a.id]).unwrap();
        conn.execute("UPDATE cards SET fields = json_set(fields, '$.blockedBy', json(?1)) WHERE id = ?2", rusqlite::params![format!("[{}]", a.number), b.id]).unwrap();
    }
    let mut s = Store::open(dir.path().join("board.db")).unwrap();
    let edges = s.wait_edges().unwrap();
    assert_eq!(edges.len(), 3);
    let e = |from: i64| edges.iter().find(|w| w.card == from).unwrap();
    assert!(e(a.number).in_cycle && e(b.number).in_cycle);
    assert!(!e(c.number).in_cycle);
    assert_eq!((e(c.number).on, e(c.number).on_column.as_deref(), e(c.number).met), (a.number, Some("Backlog"), false));
    assert_eq!(s.wait_cycles().unwrap(), vec![vec![a.number, b.number]]);
    // The poll leaves them be (neither is ready), and nothing breaks.
    assert!(s.settle_waits(false).unwrap().is_empty());
    assert_eq!(wait_path(&[(1, 2), (2, 3)], 1, 3), Some(vec![1, 2, 3]));
}

#[test]
fn a_bad_stored_value_is_ignored() {
    let (dir, mut s) = board();
    let a = mk(&mut s, "Human test");
    let c = mk(&mut s, "Blocked");
    drop(s);
    {
        let conn = rusqlite::Connection::open(dir.path().join("board.db")).unwrap();
        conn.execute("UPDATE cards SET fields = json_set(fields, '$.blockedBy', json(?1)) WHERE id = ?2", rusqlite::params![format!("[{}, \"x\"]", a.number), c.id]).unwrap();
    }
    let mut s = Store::open(dir.path().join("board.db")).unwrap();
    let c2 = get(&s, &c);
    assert!(c2.blocked_by().is_empty());
    assert!(s.any_waits().unwrap());
    assert!(s.settle_waits(false).unwrap().is_empty());
    assert_eq!(get(&s, &c).version, c2.version);
}

#[test]
fn the_empty_poll_is_one_query_on_the_index() {
    let (_d, mut s) = board();
    let a = mk(&mut s, "Backlog");
    assert!(!s.any_waits().unwrap());
    let c = mk(&mut s, "Backlog");
    let c = waits(&mut s, &c, &[a.number]).unwrap();
    assert!(s.any_waits().unwrap());
    s.archive(&c.id, c.version, true, "Hugh (board)").unwrap();
    assert!(!s.any_waits().unwrap(), "an archived card doesn't count");
    let plan = s.any_waits_plan().unwrap().join(" | ");
    assert!(plan.contains("cards_waiting"), "{plan}");
}

#[test]
fn migration_12_adds_the_index_with_a_backup_and_changes_no_card() {
    let (dir, mut s) = board();
    assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
    // 12 is #109's index; 13 (card #114, {prompts}), 14 (card #119, the Router), 15 (card
    // #153, the app's own messages), 16 (card #160, the merge lanes) and 17 (card #162, the
    // subagents unstamped, which changes no card that has none stamped) come after it.
    assert_eq!(SCHEMA_VERSION, 17);
    let a = mk(&mut s, "Backlog");
    let c = mk(&mut s, "Backlog");
    waits(&mut s, &c, &[a.number]).unwrap();
    drop(s);
    let path = dir.path().join("board.db");
    let read = || {
        let conn = rusqlite::Connection::open(&path).unwrap();
        let rows: Vec<(String, i64, String)> = conn
            .prepare("SELECT id, version, fields FROM cards ORDER BY id").unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap()
            .map(|r| r.unwrap()).collect();
        let index: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'cards_waiting'", [], |r| r.get(0)).unwrap();
        (rows, index)
    };
    let before = read();
    assert_eq!(before.1, 1);
    // Back to a schema-11 board without the index.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("DROP INDEX cards_waiting").unwrap();
        conn.pragma_update(None, "user_version", 11).unwrap();
    }
    assert_eq!(read().1, 0);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
    assert!(s.any_waits().unwrap());
    drop(s);
    assert_eq!(read(), before);
    assert!(std::fs::read_dir(dir.path().join("backups")).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with("pre-migration.db")));
    // Run again with the index there: nothing changes.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 11).unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
    drop(s);
    assert_eq!(read(), before);
    // A board from a newer build is opened as it is.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), 99);
}
