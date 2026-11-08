// Card #131: the Router asks to archive a card, which puts it in the Done pile; Hugh's Archive all
// moves it into Done (his Archive drop), Not now declines it. Every rule is a store function, tested here.

use kanban_store::{archive_refused, is_met, lifts_decline, Patch, Store, ARCHIVE_ASK_KEY, ARCHIVE_DECLINED_KEY, ROUTER_BY};
use serde_json::{json, Value};

fn patch(v: Value) -> Patch {
    v.as_object().cloned().unwrap()
}

fn board() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("board.db")).unwrap();
    (dir, s)
}

fn card(s: &mut Store, title: &str, col: &str) -> i64 {
    s.create(None, patch(json!({ "title": title, "column": col })), "test").unwrap().number
}

fn get(s: &Store, n: i64) -> kanban_store::Card {
    s.get_by_number(n).unwrap()
}

fn ask(s: &mut Store, n: i64) -> kanban_store::Card {
    let v = get(s, n).version;
    s.ask_archive(n, v, "its work is live", Some("dsh-1 is in origin/main")).unwrap()
}

fn err<T: std::fmt::Debug>(r: kanban_store::Result<T>) -> String {
    r.unwrap_err().to_string()
}

/// A write to card `n` by `by` (a label, which needs no other rule).
fn touch(s: &mut Store, n: i64, by: &str, label: &str) {
    let c = get(s, n);
    s.update(&c.id, c.version, &patch(json!({ "labels": [label] })), by).unwrap();
}

#[test]
fn ship_it_a_child_and_a_lead_with_a_child_in_ship_it_are_refused() {
    let (_d, mut s) = board();
    let ship = card(&mut s, "shipping", "Ship it");
    assert!(archive_refused(&s, &get(&s, ship)).unwrap().contains("Ship it"));
    for col in ["Backlog", "Blocked", "Planning", "In progress", "Human test", "Deploy complete"] {
        let n = card(&mut s, col, col);
        assert_eq!(archive_refused(&s, &get(&s, n)), None, "{col}");
    }
    let lead = card(&mut s, "lead", "Human test");
    let kid = card(&mut s, "kid", "Human test");
    s.stack_add(lead, kid, "t").unwrap();
    assert!(archive_refused(&s, &get(&s, kid)).unwrap().contains(&format!("stack #{lead}; ask for the lead")));
    assert_eq!(archive_refused(&s, &get(&s, lead)), None);
    let k = get(&s, kid);
    s.update(&k.id, k.version, &patch(json!({ "column": "Ship it" })), "test").unwrap();
    assert!(archive_refused(&s, &get(&s, lead)).unwrap().contains(&format!("#{kid} in Ship it")));
    // The tool says the same.
    let v = get(&s, ship).version;
    assert!(err(s.ask_archive(ship, v, "x", None)).contains("Ship it"));
}

#[test]
fn an_ask_is_written_with_its_column_and_needs_the_version_and_a_reason() {
    let (_d, mut s) = board();
    let n = card(&mut s, "done work", "Human test");
    let v = get(&s, n).version;
    assert!(err(s.ask_archive(n, v + 1, "x", None)).contains("changed"));
    assert!(err(s.ask_archive(n, v, "  ", None)).contains("reason"));
    let c = ask(&mut s, n);
    let a = &c.fields[ARCHIVE_ASK_KEY];
    assert_eq!(a["reason"], json!("its work is live"));
    assert_eq!(a["evidence"], json!("dsh-1 is in origin/main"));
    assert_eq!(a["column"], json!("Human test"));
    assert_eq!(a["by"], json!(ROUTER_BY));
    assert!(s.log(&c.id).unwrap().iter().any(|l| l.text == "The Router asks to archive it: its work is live"));
    assert!(s.router_log(0, 5).unwrap().iter().any(|l| l.kind == "decision" && l.card == Some(n) && l.text.contains("archive")));
    // A second ask replaces the first.
    let v = get(&s, n).version;
    let c = s.ask_archive(n, v, "shipped on 7 Nov", None).unwrap();
    assert_eq!(c.fields[ARCHIVE_ASK_KEY]["reason"], json!("shipped on 7 Nov"));
    assert_eq!(c.fields[ARCHIVE_ASK_KEY]["evidence"], Value::Null);
}

#[test]
fn approve_moves_the_card_into_done_archived_and_a_wait_on_it_is_met() {
    let (_d, mut s) = board();
    let n = card(&mut s, "live", "Human test");
    let waiter = card(&mut s, "waits", "Blocked");
    let c = ask(&mut s, n);
    let out = s.approve_archive(&[(c.id.clone(), c.version)]);
    assert_eq!(out.len(), 1);
    assert!(out[0].done, "{:?}", out[0]);
    assert_eq!(out[0].from.as_deref(), Some("Human test"));
    let c = get(&s, n);
    assert!(c.archived);
    assert_eq!(c.column, "Done");
    assert_eq!(c.fields["archivedFrom"], json!("Human test"));
    assert_eq!(c.fields["archivedBy"], json!("Hugh (board)"));
    assert!(c.fields.get(ARCHIVE_ASK_KEY).is_none(), "the ask goes in the same write");
    assert!(is_met(&get(&s, waiter), &c));
    assert!(s.router_log(0, 5).unwrap().iter().any(|l| l.text.contains(&format!("Hugh archived #{n}"))));
}

#[test]
fn approve_takes_a_deploy_complete_card_and_a_lead_with_its_children() {
    let (_d, mut s) = board();
    let dc = card(&mut s, "deployed", "Deploy complete");
    let c = ask(&mut s, dc);
    assert!(s.approve_archive(&[(c.id.clone(), c.version)])[0].done);
    assert!(get(&s, dc).archived);

    let lead = card(&mut s, "lead", "Human test");
    let a = card(&mut s, "a", "Human test");
    let b = card(&mut s, "b", "Human test");
    s.stack_add(lead, a, "t").unwrap();
    s.stack_add(lead, b, "t").unwrap();
    let l = ask(&mut s, lead);
    let out = s.approve_archive(&[(l.id.clone(), l.version)]);
    assert!(out[0].done, "{:?}", out[0]);
    let mut kids = out[0].children.clone();
    kids.sort();
    let mut want = vec![get(&s, a).id, get(&s, b).id];
    want.sort();
    assert_eq!(kids, want, "the children archived with it, for the app to stop their sessions");
    assert!(get(&s, a).archived && get(&s, b).archived);
}

#[test]
fn approve_refuses_no_ask_a_changed_version_ship_it_and_a_card_that_moved() {
    let (_d, mut s) = board();
    let none = card(&mut s, "no ask", "Human test");
    let c = get(&s, none);
    let out = s.approve_archive(&[(c.id.clone(), c.version)]);
    assert!(!out[0].done && out[0].why.as_deref().unwrap().contains("no ask"));
    assert!(!get(&s, none).archived);

    let n = card(&mut s, "changed", "Human test");
    let c = ask(&mut s, n);
    touch(&mut s, n, "Hugh (board)", "x");
    let out = s.approve_archive(&[(c.id.clone(), c.version)]);
    assert!(!out[0].done && out[0].why.as_deref().unwrap().contains("changed"), "{:?}", out[0]);
    assert!(!get(&s, n).archived);

    // Sent back to work after the ask: the ask is gone, so approve has nothing to act on.
    let back = card(&mut s, "sent back", "Human test");
    ask(&mut s, back);
    let c = get(&s, back);
    let c = s.update(&c.id, c.version, &patch(json!({ "column": "In progress" })), "the router").unwrap();
    assert!(c.fields.get(ARCHIVE_ASK_KEY).is_none(), "a column change removes the ask");
    let out = s.approve_archive(&[(c.id.clone(), c.version)]);
    assert!(!out[0].done);
    assert_eq!(get(&s, back).column, "In progress");

    // Into Ship it after the ask: the ask is gone too, and a refusal never stops the others.
    let sh = card(&mut s, "shipping now", "Human test");
    ask(&mut s, sh);
    let c = get(&s, sh);
    let c = s.move_by_hugh(&c.id, c.version, "Ship it", Patch::new()).unwrap().card;
    assert!(c.fields.get(ARCHIVE_ASK_KEY).is_none());
    let ok = card(&mut s, "fine", "Human test");
    let o = ask(&mut s, ok);
    let out = s.approve_archive(&[(c.id.clone(), c.version), (o.id.clone(), o.version)]);
    assert!(!out[0].done && out[1].done);
    assert_eq!(get(&s, sh).column, "Ship it");
}

#[test]
fn not_now_stands_until_hugh_or_the_cards_own_session_changes_it() {
    let (_d, mut s) = board();
    let n = card(&mut s, "keep", "Human test");
    let c = ask(&mut s, n);
    let out = s.dismiss_archive(&[(c.id.clone(), c.version)]);
    assert!(out[0].done, "{:?}", out[0]);
    let c = get(&s, n);
    assert!(c.fields.get(ARCHIVE_ASK_KEY).is_none());
    assert_eq!(c.fields[ARCHIVE_DECLINED_KEY]["version"], json!(c.version));
    assert!(s.log(&c.id).unwrap().iter().any(|l| l.text.contains("not now")));
    let again = |s: &mut Store| {
        let v = get(s, n).version;
        s.ask_archive(n, v, "live", None)
    };
    assert!(err(again(&mut s)).contains("Hugh said not now"));
    // Not lifted by the Router, the app, another card's session or a session outside the app.
    for by in [ROUTER_BY, "the app (monitor)", "card #99's session", "card #1311's session", "a session outside the app"] {
        touch(&mut s, n, by, "t");
        touch(&mut s, n, by, "u");
        assert!(err(again(&mut s)).contains("Hugh said not now"), "{by}");
    }
    // Lifted by the card's own session.
    touch(&mut s, n, &format!("card #{n}'s session"), "own");
    assert!(again(&mut s).is_ok());

    // And by Hugh.
    let m = card(&mut s, "keep 2", "Human test");
    let c = ask(&mut s, m);
    s.dismiss_archive(&[(c.id.clone(), c.version)]);
    touch(&mut s, m, "Hugh (board)", "mine");
    let v = get(&s, m).version;
    assert!(s.ask_archive(m, v, "live", None).is_ok());
    // Not now on a card with no ask says so.
    let k = card(&mut s, "plain", "Human test");
    let c = get(&s, k);
    assert!(!s.dismiss_archive(&[(c.id.clone(), c.version)])[0].done);
}

#[test]
fn who_lifts_a_decline() {
    assert!(lifts_decline("Hugh (board)", 7));
    assert!(lifts_decline("Hugh (panel)", 7));
    assert!(lifts_decline("card #7's session", 7));
    assert!(lifts_decline("card #7's session (held until its turn ended)", 7));
    assert!(!lifts_decline("card #70's session", 7));
    assert!(!lifts_decline("card #8's session", 7));
    assert!(!lifts_decline(ROUTER_BY, 7));
    assert!(!lifts_decline("the app", 7));
    assert!(!lifts_decline("a session outside the app", 7));
}

#[test]
fn revisions_after_lists_later_writes_with_their_authors() {
    let (_d, mut s) = board();
    let n = card(&mut s, "r", "Human test");
    let v = get(&s, n).version;
    touch(&mut s, n, "Hugh (board)", "a");
    touch(&mut s, n, ROUTER_BY, "b");
    let id = get(&s, n).id;
    let after = s.revisions_after(&id, v).unwrap();
    assert_eq!(after, vec![(v + 1, "Hugh (board)".to_string()), (v + 2, ROUTER_BY.to_string())]);
    assert!(s.revisions_after(&id, v + 2).unwrap().is_empty());
}

#[test]
fn an_ask_never_outlives_an_archive_a_restore_or_joining_a_stack() {
    let (_d, mut s) = board();
    // Hugh's own Archive drop (a move into Done), then Restore.
    let n = card(&mut s, "dragged", "Human test");
    ask(&mut s, n);
    let c = get(&s, n);
    let c = s.move_by_hugh(&c.id, c.version, "Done", Patch::new()).unwrap().card;
    assert!(c.archived && c.fields.get(ARCHIVE_ASK_KEY).is_none());
    let c = s.archive(&c.id, c.version, false, "Hugh (board)").unwrap();
    assert!(!c.archived && c.fields.get(ARCHIVE_ASK_KEY).is_none());

    // The Archive button (archive in place).
    let p = card(&mut s, "in place", "Human test");
    ask(&mut s, p);
    let c = get(&s, p);
    let c = s.archive(&c.id, c.version, true, "Hugh (board)").unwrap();
    assert!(c.fields.get(ARCHIVE_ASK_KEY).is_none());

    // A lead archived and restored while a child has an ask.
    let lead = card(&mut s, "lead", "Human test");
    let kid = card(&mut s, "kid", "Human test");
    s.stack_add(lead, kid, "t").unwrap();
    let k = get(&s, kid);
    s.update(&k.id, k.version, &patch(json!({ ARCHIVE_ASK_KEY: { "reason": "x", "column": "Human test" } })), "Hugh (board)").unwrap();
    let l = ask(&mut s, lead);
    let l = s.archive(&l.id, l.version, true, "Hugh (board)").unwrap();
    assert!(l.fields.get(ARCHIVE_ASK_KEY).is_none());
    assert!(get(&s, kid).archived && get(&s, kid).fields.get(ARCHIVE_ASK_KEY).is_none());
    let l = s.archive(&l.id, l.version, false, "Hugh (board)").unwrap();
    assert!(l.fields.get(ARCHIVE_ASK_KEY).is_none());
    assert!(get(&s, kid).fields.get(ARCHIVE_ASK_KEY).is_none());

    // Joining a stack.
    let j = card(&mut s, "joins", "Human test");
    ask(&mut s, j);
    let lead2 = card(&mut s, "lead 2", "Human test");
    s.stack_add(lead2, j, "t").unwrap();
    assert!(get(&s, j).fields.get(ARCHIVE_ASK_KEY).is_none());

    // A write that changes nothing about where it is keeps the ask.
    let keep = card(&mut s, "keeps", "Human test");
    ask(&mut s, keep);
    touch(&mut s, keep, "card #5's session", "x");
    assert!(get(&s, keep).fields.get(ARCHIVE_ASK_KEY).is_some());
}
