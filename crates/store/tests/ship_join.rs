// Dropping a card on a Ship it card ships them as one (card #89): Store::stack_join with Hugh's
// drop, ship_started, and how the ship queue and the stack rules carry on afterwards.

use kanban_store::{ship_goes_first, ship_started, Error, Joined, Patch, Store};
use serde_json::{json, Value};

const REPO: &str = "acme/web";

fn patch(v: Value) -> Patch {
    v.as_object().cloned().unwrap()
}

fn board() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("board.db")).unwrap();
    (dir, s)
}

/// A card in `col` for REPO; returns its number.
fn card(s: &mut Store, title: &str, col: &str) -> i64 {
    s.create(None, patch(json!({ "title": title, "column": col, "repo": REPO })), "test").unwrap().number
}

/// Hugh drops card `n` into Ship it, and the app hands it on or queues it, as move_card does.
/// Returns the card it waits for, if any. A short sleep keeps the order of the asks.
fn ship(s: &mut Store, n: i64) -> Option<i64> {
    let c = s.get_by_number(n).unwrap();
    s.move_by_hugh(&c.id, c.version, "Ship it", Patch::new()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let c = s.get_by_number(n).unwrap();
    match s.ship_blocker(&c).unwrap() {
        Some(b) => {
            s.ship_wait(&c.id, &b, "Human test").unwrap();
            Some(b.number)
        }
        None => None,
    }
}

/// Merges `v` into card `n`'s ship, as the app or its session would.
fn set_ship(s: &mut Store, n: i64, v: Value) {
    let c = s.get_by_number(n).unwrap();
    let mut ship = c.fields["ship"].as_object().cloned().unwrap();
    for (k, x) in v.as_object().unwrap() {
        ship.insert(k.clone(), x.clone());
    }
    s.update(&c.id, c.version, &patch(json!({ "ship": ship })), "test").unwrap();
}

fn handed(s: &mut Store, n: i64) {
    set_ship(s, n, json!({ "handedAt": "2026-11-05T13:28:00.000Z" }));
}

fn state(s: &Store, n: i64) -> String {
    s.get_by_number(n).unwrap().fields["ship"]["state"].as_str().unwrap_or("").to_string()
}

fn col(s: &Store, n: i64) -> String {
    s.get_by_number(n).unwrap().column
}

fn join(s: &mut Store, lead: i64, child: i64) -> Joined {
    s.stack_join(lead, child, "Hugh (board)", true).unwrap().1
}

fn invalid<T: std::fmt::Debug>(r: Result<T, Error>) -> String {
    match r {
        Err(Error::Invalid(m)) => m,
        other => panic!("expected Invalid, got {other:?}"),
    }
}

fn log(s: &Store, n: i64) -> Vec<String> {
    let c = s.get_by_number(n).unwrap();
    s.log(&c.id).unwrap().into_iter().map(|e| e.text).collect()
}

/// Hugh's 5 Nov case: #73 shipping, stack #53 and #81 both waiting behind it. Dropping #81 on
/// #53 stacks them, and the stack ships as one when its turn comes.
#[test]
fn a_waiting_card_dropped_on_a_waiting_stack_ships_with_it() {
    let (_d, mut s) = board();
    let first = card(&mut s, "73", "Human test");
    let lead = card(&mut s, "53", "Human test");
    let kid = card(&mut s, "53's child", "Human test");
    let x = card(&mut s, "81", "Human test");
    s.stack_add(lead, kid, "t").unwrap();
    assert_eq!(ship(&mut s, first), None);
    handed(&mut s, first);
    assert_eq!(ship(&mut s, lead), Some(first));
    assert_eq!(ship(&mut s, x), Some(first));

    let j = join(&mut s, lead, x);
    assert_eq!(j, Joined { unshipped: vec![x], came_with: vec![] });
    let l = s.get_by_number(lead).unwrap();
    assert_eq!(l.stack(), vec![kid, x]);
    assert_eq!((l.column.as_str(), state(&s, lead).as_str()), ("Ship it", "queued"), "the stack keeps its place");
    let xc = s.get_by_number(x).unwrap();
    assert_eq!((xc.column.as_str(), xc.stack_of(), xc.passed()), ("Human test", Some(lead), true));
    assert_eq!(xc.fields["ship"]["state"], "cancelled");
    assert_eq!(xc.fields["ship"]["detail"], format!("Ships with stack #{lead}"));
    assert!(xc.fields.get("shipRequestedAt").map_or(true, Value::is_null));
    let xl = log(&s, x);
    assert!(xl.iter().any(|t| t.starts_with(&format!("Ship request cancelled: it ships with stack #{lead}"))), "{xl:?}");
    assert!(xl.iter().any(|t| t.starts_with(&format!("Passed: Hugh dropped it on #{lead} in Ship it"))), "{xl:?}");
    assert!(log(&s, lead).iter().any(|t| t == &format!("Added #{x} to this stack")));

    // The stack rules leave a stack in Ship it where it is.
    assert!(s.stack_recheck(lead, "test", "t").unwrap().is_empty());
    assert_eq!(s.ship_cancel_strays().unwrap(), 0);

    // When the first ship is over, the stack goes, and the card that joined it doesn't go alone.
    set_ship(&mut s, first, json!({ "state": "shipped" }));
    let due: Vec<i64> = s.ship_due().unwrap().iter().map(|c| c.number).collect();
    assert_eq!(due, vec![lead]);

    // The stack's one ship takes every card in it to Deploy complete.
    s.ship_release(&l.id).unwrap();
    set_ship(&mut s, lead, json!({ "state": "shipped" }));
    let l = s.get_by_number(lead).unwrap();
    s.update(&l.id, l.version, &patch(json!({ "column": "Deploy complete" })), "card session").unwrap();
    s.stack_after_move(&l.id, "card session").unwrap();
    assert_eq!((col(&s, kid), col(&s, x)), ("Deploy complete".to_string(), "Deploy complete".to_string()));
}

#[test]
fn the_stack_keeps_the_earlier_place_in_the_queue() {
    let (_d, mut s) = board();
    let first = card(&mut s, "first", "Human test");
    let x = card(&mut s, "asked first", "Human test");
    let lead = card(&mut s, "asked later", "Human test");
    let other = card(&mut s, "asked between", "Human test");
    ship(&mut s, first);
    handed(&mut s, first);
    ship(&mut s, x);
    ship(&mut s, other);
    ship(&mut s, lead);
    let asked = s.get_by_number(x).unwrap().fields["ship"]["requestedAt"].clone();
    join(&mut s, lead, x);
    assert_eq!(s.get_by_number(lead).unwrap().fields["ship"]["requestedAt"], asked);
    assert!(log(&s, lead).iter().any(|t| t == &format!("The stack takes #{x}'s earlier place in the ship queue")));
    set_ship(&mut s, first, json!({ "state": "shipped" }));
    let due: Vec<i64> = s.ship_due().unwrap().iter().map(|c| c.number).collect();
    assert_eq!(due, vec![lead], "it goes before the card asked for between the two");
}

#[test]
fn ship_started_says_how() {
    let (_d, mut s) = board();
    let n = card(&mut s, "x", "Human test");
    assert_eq!(ship_started(&s.get_by_number(n).unwrap()), None, "not in Ship it");
    ship(&mut s, n);
    assert_eq!(ship_started(&s.get_by_number(n).unwrap()), None, "asked for, not handed yet");
    handed(&mut s, n);
    assert_eq!(ship_started(&s.get_by_number(n).unwrap()), Some("its session has the ship"));
    for (st, why) in [("shipping", "it is shipping now"), ("needs-hugh", "its ship is waiting for Hugh"), ("shipped", "it has shipped")] {
        set_ship(&mut s, n, json!({ "state": st }));
        assert_eq!(ship_started(&s.get_by_number(n).unwrap()), Some(why), "{st}");
    }
    set_ship(&mut s, n, json!({ "state": "queued" }));
    assert_eq!(ship_started(&s.get_by_number(n).unwrap()), None);
}

#[test]
fn a_ship_that_has_started_neither_takes_nor_gives_a_card() {
    let (_d, mut s) = board();
    let lead = card(&mut s, "lead", "Human test");
    let x = card(&mut s, "x", "Human test");
    ship(&mut s, lead);
    handed(&mut s, lead);
    let e = invalid(s.stack_join(lead, x, "Hugh (board)", true));
    assert!(e.contains("ship has started (its session has the ship)") && e.contains("ship it straight after"), "{e}");
    set_ship(&mut s, lead, json!({ "state": "shipping" }));
    assert!(invalid(s.stack_join(lead, x, "Hugh (board)", true)).contains("it is shipping now"));

    // A card whose own ship has started can't leave it for a stack.
    let t = card(&mut s, "target", "Human test");
    for st in ["shipping", "needs-hugh"] {
        set_ship(&mut s, lead, json!({ "state": st }));
        let e = invalid(s.stack_join(t, lead, "Hugh (board)", true));
        assert!(e.contains("its merge may be done already"), "{st}: {e}");
    }
    assert!(s.get_by_number(t).unwrap().stack().is_empty(), "nothing changed");
    assert_eq!(s.get_by_number(x).unwrap().stack_of(), None);
}

#[test]
fn only_a_built_card_joins_a_stack_in_ship_it() {
    let (_d, mut s) = board();
    let lead = card(&mut s, "lead", "Human test");
    ship(&mut s, lead);
    for c in ["Backlog", "Planning", "In progress", "Blocked"] {
        let x = card(&mut s, c, c);
        let e = invalid(s.stack_join(lead, x, "Hugh (board)", true));
        assert!(e.contains(&format!("isn't built yet (it's in {c})")) && e.contains("build it first"), "{e}");
    }
    // A stack with a card still being built can't come in either.
    let l2 = card(&mut s, "stack", "Human test");
    let k2 = card(&mut s, "building", "In progress");
    s.stack_add(l2, k2, "t").unwrap();
    assert!(invalid(s.stack_join(lead, l2, "Hugh (board)", true)).contains(&format!("#{k2} isn't built yet")));
    // A built one does.
    let x = card(&mut s, "built", "Human test");
    assert_eq!(join(&mut s, lead, x), Joined::default());
    let xc = s.get_by_number(x).unwrap();
    assert_eq!((xc.column.as_str(), xc.passed()), ("Human test", true));
}

#[test]
fn a_card_taken_out_of_ship_it_waits_for_its_new_stack() {
    let (_d, mut s) = board();
    let t = card(&mut s, "target", "Human test");
    let x = card(&mut s, "shipping alone", "Human test");
    ship(&mut s, x);
    let j = join(&mut s, t, x);
    assert_eq!(j.unshipped, vec![x]);
    let xc = s.get_by_number(x).unwrap();
    assert_eq!((xc.column.as_str(), xc.passed(), xc.stack_of()), ("Human test", true, Some(t)));
    assert_eq!(xc.fields["ship"]["state"], "cancelled");
    assert!(log(&s, x).iter().any(|e| e == "Passed: Hugh had moved it to Ship it, which is his approval to ship it"));
    // The target didn't go to Ship it: Hugh hasn't approved it.
    assert_eq!(col(&s, t), "Human test");
    assert!(s.get_by_number(t).unwrap().fields.get("ship").is_none());
    // A queued card can be taken out too.
    let first = card(&mut s, "first", "Human test");
    let y = card(&mut s, "queued", "Human test");
    ship(&mut s, first);
    handed(&mut s, first);
    assert_eq!(ship(&mut s, y), Some(first));
    join(&mut s, t, y);
    assert_eq!((col(&s, y), state(&s, y)), ("Human test".to_string(), "cancelled".to_string()));
}

#[test]
fn a_whole_stack_joins_another() {
    let (_d, mut s) = board();
    let t = card(&mut s, "target", "Human test");
    let tk = card(&mut s, "target's card", "Human test");
    s.stack_add(t, tk, "t").unwrap();
    let l2 = card(&mut s, "other stack", "Human test");
    let a = card(&mut s, "a", "Human test");
    let b = card(&mut s, "b", "Human test");
    s.stack_add(l2, a, "t").unwrap();
    s.stack_add(l2, b, "t").unwrap();

    let j = join(&mut s, t, l2);
    assert_eq!(j, Joined { unshipped: vec![], came_with: vec![a, b] });
    let tl = s.get_by_number(t).unwrap();
    assert_eq!(tl.stack(), vec![tk, l2, a, b]);
    let kids: Vec<i64> = s.stack_children(&tl).unwrap().iter().map(|k| k.number).collect();
    assert_eq!(kids, vec![tk, l2, a, b]);
    let old = s.get_by_number(l2).unwrap();
    assert_eq!((old.is_lead(), old.stack_of()), (false, Some(t)));
    for n in [a, b] {
        let k = s.get_by_number(n).unwrap();
        assert_eq!((k.stack_of(), k.passed()), (Some(t), false), "not approved: nothing is in Ship it");
        assert!(log(&s, n).iter().any(|e| e == &format!("Moved with stack #{l2} into stack #{t}")));
    }
    assert!(log(&s, t).iter().any(|e| e == &format!("Added stack #{l2} to this stack, with its cards #{a}, #{b}")));

    // A stack still being built can't be dragged in.
    let l3 = card(&mut s, "building stack", "In progress");
    let c = card(&mut s, "c", "In progress");
    s.stack_add(l3, c, "t").unwrap();
    assert!(invalid(s.stack_join(t, l3, "Hugh (board)", true)).contains("still being built"));
}

#[test]
fn a_waiting_stack_joins_a_waiting_stack_as_one_ship() {
    let (_d, mut s) = board();
    let first = card(&mut s, "first", "Human test");
    let t = card(&mut s, "target", "Human test");
    let l2 = card(&mut s, "other stack", "Human test");
    let a = card(&mut s, "a", "Human test");
    s.stack_add(l2, a, "t").unwrap();
    ship(&mut s, first);
    handed(&mut s, first);
    ship(&mut s, t);
    ship(&mut s, l2);
    let j = join(&mut s, t, l2);
    assert_eq!(j, Joined { unshipped: vec![l2], came_with: vec![a] });
    assert_eq!(s.get_by_number(t).unwrap().stack(), vec![l2, a]);
    // A card that had left Ship it for l2 ships with t now, and says so.
    let x = card(&mut s, "x", "Human test");
    let t2 = card(&mut s, "t2", "Human test");
    ship(&mut s, x);
    s.stack_join(l2, x, "Hugh (board)", true).unwrap_err(); // l2 is in t's stack now: it can't lead
    s.stack_join(t2, x, "Hugh (board)", true).unwrap();
    let l3 = card(&mut s, "l3", "Human test");
    s.stack_join(l3, t2, "Hugh (board)", true).unwrap();
    let xs = s.get_by_number(x).unwrap().fields["ship"].clone();
    assert_eq!((xs["with"].clone(), xs["detail"].clone()), (json!(l3), json!(format!("Ships with stack #{l3}"))));
    for n in [l2, a] {
        let k = s.get_by_number(n).unwrap();
        assert_eq!((k.column.as_str(), k.passed()), ("Human test", true), "#{n}");
    }
    assert_eq!(state(&s, l2), "cancelled");
}

#[test]
fn sessions_keep_the_old_rules() {
    let (_d, mut s) = board();
    let lead = card(&mut s, "lead", "Human test");
    let x = card(&mut s, "x", "Human test");
    let l2 = card(&mut s, "l2", "Human test");
    let k = card(&mut s, "k", "Human test");
    s.stack_add(l2, k, "t").unwrap();
    assert!(invalid(s.stack_add(lead, l2, "card session")).contains("leads a stack of its own"));
    let y = card(&mut s, "y", "Human test");
    ship(&mut s, y);
    assert!(invalid(s.stack_add(lead, y, "card session")).contains("add it once its ship is over"));
    ship(&mut s, lead);
    let e = invalid(s.stack_add(lead, x, "card session"));
    assert!(e.contains("only Hugh adds one"), "{e}");
    assert!(invalid(s.stack_join(lead, x, "card session", false)).contains("only Hugh adds one"));
    assert_eq!(s.get_by_number(x).unwrap().stack_of(), None);
}

#[test]
fn a_stack_is_one_repo() {
    let (_d, mut s) = board();
    let lead = card(&mut s, "lead", "Human test");
    let other = s.create(None, patch(json!({ "title": "app", "column": "Human test", "repo": "acme/kanban" })), "t").unwrap().number;
    let e = invalid(s.stack_join(lead, other, "Hugh (board)", true));
    assert_eq!(e, format!("#{other} is for acme/kanban and #{lead} is for {REPO}: a stack is one branch in one repo"));
    assert!(invalid(s.stack_add(lead, other, "t")).contains("one repo"));
    // The same repo in another case, or a card with no repo, joins.
    let same = s.create(None, patch(json!({ "title": "same", "column": "Human test", "repo": "ACME/WEB" })), "t").unwrap().number;
    let none = s.create(None, patch(json!({ "title": "none", "column": "Human test" })), "t").unwrap().number;
    s.stack_add(lead, same, "t").unwrap();
    s.stack_add(lead, none, "t").unwrap();
    assert_eq!(s.get_by_number(lead).unwrap().stack(), vec![same, none]);
}

/// Review of card #89: anything in Ship it but a ship still waiting to start counts as started.
#[test]
fn only_a_waiting_ship_is_open_to_another_card() {
    let (_d, mut s) = board();
    let n = card(&mut s, "x", "Human test");
    ship(&mut s, n);
    for st in ["failed", "cancelled", "something new"] {
        set_ship(&mut s, n, json!({ "state": st }));
        assert_eq!(ship_started(&s.get_by_number(n).unwrap()), Some("it has no ship waiting to start"), "{st}");
    }
    // Which started ships a card dropped into Ship it really waits for (the queue holds the repo).
    set_ship(&mut s, n, json!({ "state": "requested", "handedAt": "2026-11-05T13:28:00.000Z" }));
    assert!(ship_goes_first(&s.get_by_number(n).unwrap()));
    for (st, first) in [("shipping", true), ("shipped", true), ("needs-hugh", false), ("failed", false)] {
        set_ship(&mut s, n, json!({ "state": st }));
        assert_eq!(ship_goes_first(&s.get_by_number(n).unwrap()), first, "{st}");
    }
}

#[test]
fn a_shipped_card_cant_join_a_stack_in_ship_it() {
    let (_d, mut s) = board();
    let lead = card(&mut s, "lead", "Human test");
    ship(&mut s, lead);
    for c in ["Deploy complete", "Done"] {
        let x = card(&mut s, c, c);
        if c == "Done" {
            // Done archives a card; an archived card is refused first anyway.
            assert!(invalid(s.stack_join(lead, x, "Hugh (board)", true)).contains("archived"));
            continue;
        }
        let e = invalid(s.stack_join(lead, x, "Hugh (board)", true));
        assert_eq!(e, format!("#{x} has shipped already (it's in Deploy complete), so it can't join stack #{lead}'s ship"));
    }
}

#[test]
fn the_check_says_what_the_join_would_and_writes_nothing() {
    let (_d, mut s) = board();
    let lead = card(&mut s, "lead", "Human test");
    let x = card(&mut s, "x", "Human test");
    let y = card(&mut s, "y", "Planning");
    ship(&mut s, lead);
    let v = s.get_by_number(lead).unwrap().version;
    s.stack_join_check(lead, x, true).unwrap();
    assert!(invalid(s.stack_join_check(lead, y, true)).contains("isn't built yet"));
    assert!(invalid(s.stack_join_check(lead, x, false)).contains("only Hugh adds one"));
    assert_eq!(s.get_by_number(lead).unwrap().version, v);
    assert!(s.get_by_number(lead).unwrap().stack().is_empty());
    // The join itself marks the cancelled ship with the stack it went to.
    let z = card(&mut s, "z", "Human test");
    let t = card(&mut s, "t", "Human test");
    ship(&mut s, z);
    join(&mut s, t, z);
    assert_eq!(s.get_by_number(z).unwrap().fields["ship"]["with"], json!(t));
}

#[test]
fn a_refused_create_into_a_stack_leaves_no_card_behind() {
    let (_d, mut s) = board();
    let lead = card(&mut s, "lead", "Human test");
    let before = s.list(true).unwrap().len();
    let other = patch(json!({ "title": "other repo", "repo": "acme/kanban" }));
    assert!(invalid(s.create_in_stack(other, lead, "card session")).contains("one repo"));
    ship(&mut s, lead);
    let same = patch(json!({ "title": "into Ship it", "repo": REPO }));
    assert!(invalid(s.create_in_stack(same, lead, "card session")).contains("only Hugh adds one"));
    assert_eq!(s.list(true).unwrap().len(), before);
}

/// An archived card of a joining stack comes along, still archived, and no lead claims it as
/// archived with it.
#[test]
fn an_archived_card_of_a_joining_stack_comes_along_still_archived() {
    let (_d, mut s) = board();
    let t = card(&mut s, "target", "Human test");
    let l2 = card(&mut s, "stack", "Human test");
    let a = card(&mut s, "a", "Human test");
    let b = card(&mut s, "b", "Human test");
    s.stack_add(l2, a, "t").unwrap();
    s.stack_add(l2, b, "t").unwrap();
    let bc = s.get_by_number(b).unwrap();
    s.archive(&bc.id, bc.version, true, "t").unwrap();
    join(&mut s, t, l2);
    assert_eq!(s.get_by_number(t).unwrap().stack(), vec![l2, a, b]);
    let bc = s.get_by_number(b).unwrap();
    assert_eq!((bc.stack_of(), bc.archived), (Some(t), true));
    assert!(bc.fields.get("archivedWith").map_or(true, Value::is_null));
}
