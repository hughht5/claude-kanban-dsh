//! Test-app turns (card #115) and a test app per card (card #147): the parts that touch the
//! whole store.

use kanban_store::{card_slot, Ask, Patch, SlotClaim, Store, TestAppQueue, TEST_APP_KEY};
use serde_json::json;
use std::collections::BTreeSet;

fn t(min: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339("2026-11-09T01:00:00Z").unwrap().with_timezone(&chrono::Utc) + chrono::Duration::minutes(min)
}

fn board() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("b.db")).unwrap();
    (dir, s)
}

/// A card in `col`, returned by its id and number.
fn card(s: &mut Store, title: &str, col: &str) -> (String, i64) {
    let mut p = Patch::new();
    p.insert("title".into(), json!(title));
    let c = s.create(None, p, "t").unwrap();
    let c = if col == "Backlog" { c } else { s.move_by_hugh(&c.id, c.version, col, Patch::new()).unwrap().card };
    (c.id, c.number)
}

fn mv(s: &mut Store, id: &str, col: &str) {
    let c = s.get(id).unwrap();
    s.move_by_hugh(id, c.version, col, Patch::new()).unwrap();
}

fn slot(s: &Store, id: &str) -> Option<u32> {
    card_slot(&s.get(id).unwrap())
}

const NONE: fn() -> BTreeSet<u32> = BTreeSet::new;

#[test]
fn copy_board_starts_the_copy_with_no_test_app_turns() {
    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("live.db");
    let mut s = Store::open(&live).unwrap();
    let now = chrono::Utc::now();
    let mut q = TestAppQueue::default();
    q.take(Ask::tester("a", 1), now, false);
    q.take(Ask::tester("b", 2), now, false);
    q.take(Ask::install("router", "wave 3"), now, false);
    s.save_test_app_queue(&q).unwrap();
    s.set_test_app_limit(90).unwrap();
    // Card #147: a card with a test app, another waiting, and a turn on Test 2.
    let (a, _) = card(&mut s, "a", "In progress");
    s.give_test_app(&a, 1, now).unwrap();
    let (b, bn) = card(&mut s, "b", "In progress");
    let mut line = s.slot_line().unwrap();
    line.join(&b, bn, now);
    s.save_slot_line(&line).unwrap();
    s.save_test_app_queue_for(2, &q).unwrap();

    let to = dir.path().join("test").join("board.db");
    kanban_store::copy_board(&live, &to, false).unwrap();
    let copy = Store::open(&to).unwrap();
    assert!(copy.test_app_queue().unwrap().is_empty(), "the live holder and line stay on the live board");
    assert_eq!(copy.test_app_limit().unwrap(), 90, "the setting itself is copied");
    assert!(copy.cards_with_test_apps().unwrap().is_empty(), "no card in the copy has a test app");
    assert!(copy.slot_line().unwrap().line.is_empty());
    assert!(copy.test_app_queue_for(2).unwrap().is_empty());
    // The source is only read.
    assert_eq!(s.test_app_queue().unwrap(), q);
    assert_eq!(slot(&s, &a), Some(1));
}

#[test]
fn a_card_gets_the_lowest_free_test_app_and_a_new_one_when_the_lower_ones_are_taken() {
    let (_d, mut s) = board();
    let ids: Vec<String> = (1..=4).map(|i| card(&mut s, &format!("c{i}"), "In progress").0).collect();
    for (i, id) in ids.iter().enumerate().take(3) {
        assert_eq!(s.claim_test_app(id, t(i as i64), 0, &NONE()).unwrap(), SlotClaim::Given(i as u32 + 1));
    }
    // Test to Test 3 hold builds: the fourth card gets Test 4, which its install creates.
    assert_eq!(s.claim_test_app(&ids[3], t(5), 0, &NONE()).unwrap(), SlotClaim::Given(4));
    let c = s.get(&ids[3]).unwrap();
    let v = &c.fields[TEST_APP_KEY];
    assert_eq!(v["channel"], "test4");
    assert_eq!(v["label"], "Test 4");
    assert_eq!(v["name"], "Claude Kanban DSH Test 4");
    assert_eq!(v["port"], 9337);
    assert!(s.log(&ids[3]).unwrap().iter().any(|l| l.text.contains("Its test app is Test 4") && l.text.contains("channel test4")));
    // Asking again keeps it.
    assert_eq!(s.claim_test_app(&ids[3], t(6), 0, &NONE()).unwrap(), SlotClaim::Kept(4));
    // A freed slot is the next one given.
    mv(&mut s, &ids[1], "Done");
    let (e, _) = card(&mut s, "e", "In progress");
    assert_eq!(s.claim_test_app(&e, t(7), 0, &NONE()).unwrap(), SlotClaim::Given(2));
}

#[test]
fn a_test_app_never_gives_its_own_slot_nor_one_another_board_has() {
    let (_d, mut s) = board();
    let (a, _) = card(&mut s, "a", "In progress");
    let (b, _) = card(&mut s, "b", "In progress");
    // This app is Test (slot 1), and another board has Test 2.
    let elsewhere: BTreeSet<u32> = [2].into_iter().collect();
    assert_eq!(s.claim_test_app(&a, t(0), 1, &elsewhere).unwrap(), SlotClaim::Given(3));
    assert_eq!(s.claim_test_app(&b, t(1), 1, &elsewhere).unwrap(), SlotClaim::Given(4));
}

#[test]
fn an_outside_install_turn_on_a_free_slot_keeps_it_from_cards() {
    let (_d, mut s) = board();
    let mut q = TestAppQueue::default();
    q.take(Ask::install("router", "wave 3"), t(0), false);
    s.save_test_app_queue_for(1, &q).unwrap();
    let (a, _) = card(&mut s, "a", "In progress");
    assert_eq!(s.claim_test_app(&a, t(1), 0, &NONE()).unwrap(), SlotClaim::Given(2));
}

#[test]
fn a_card_waits_only_at_the_ceiling_and_the_line_gets_the_next_freed_slot_in_order() {
    let (_d, mut s) = board();
    s.set_page_setting("testapp.slots", "2").unwrap();
    let (a, _) = card(&mut s, "a", "In progress");
    let (b, _) = card(&mut s, "b", "Human test");
    let (c, _) = card(&mut s, "c", "In progress");
    let (d, _) = card(&mut s, "d", "In progress");
    assert_eq!(s.claim_test_app(&a, t(0), 0, &NONE()).unwrap(), SlotClaim::Given(1));
    assert_eq!(s.claim_test_app(&b, t(0), 0, &NONE()).unwrap(), SlotClaim::Given(2));
    assert_eq!(s.claim_test_app(&c, t(1), 0, &NONE()).unwrap(), SlotClaim::Waiting(1));
    assert_eq!(s.claim_test_app(&d, t(2), 0, &NONE()).unwrap(), SlotClaim::Waiting(2));
    assert_eq!(s.claim_test_app(&c, t(3), 0, &NONE()).unwrap(), SlotClaim::Waiting(1), "asking again keeps the place");
    // Nothing free: the line stays.
    assert_eq!(s.step_slot_line(t(4), 0, &NONE()).unwrap(), Default::default());
    // b moves on: its test app goes to c, the first in line, and d waits on.
    mv(&mut s, &b, "Ship it");
    for (card, _, why) in s.stale_test_apps().unwrap() {
        s.free_test_app(&card.id, &why, "the app").unwrap();
    }
    assert_eq!(slot(&s, &b), None);
    assert!(s.log(&b).unwrap().iter().any(|l| l.text.contains("Its test app (Test 2) is free again: the card moved to Ship it")));
    let step = s.step_slot_line(t(5), 0, &NONE()).unwrap();
    assert_eq!(step.given.iter().map(|g| (g.0.clone(), g.2)).collect::<Vec<_>>(), vec![(c.clone(), 2)]);
    assert_eq!(slot(&s, &c), Some(2));
    assert_eq!(s.slot_line().unwrap().place(&d), Some(1));
    // A card in line that moves on leaves the line.
    mv(&mut s, &d, "Blocked");
    let step = s.step_slot_line(t(6), 0, &NONE()).unwrap();
    assert_eq!(step.left, vec![(d.clone(), "the card moved to Blocked".to_string())]);
    assert!(s.slot_line().unwrap().line.is_empty());
}

#[test]
fn a_card_keeps_its_test_app_in_in_progress_and_human_test_and_loses_it_after() {
    let (_d, mut s) = board();
    let cases = [("Ship it", "the card moved to Ship it"), ("Done", "the card moved to Done"), ("Backlog", "the card moved to Backlog")];
    for (col, why) in cases {
        let (id, _) = card(&mut s, col, "In progress");
        s.claim_test_app(&id, t(0), 0, &NONE()).unwrap();
        mv(&mut s, &id, "Human test");
        assert!(s.stale_test_apps().unwrap().is_empty(), "Human test keeps it");
        mv(&mut s, &id, "In progress");
        assert!(s.stale_test_apps().unwrap().is_empty(), "back to In progress keeps it");
        mv(&mut s, &id, col);
        let stale = s.stale_test_apps().unwrap();
        assert_eq!(stale.len(), 1, "{col}");
        assert_eq!(stale[0].2, why);
        s.free_test_app(&id, &stale[0].2, "the app").unwrap();
        assert!(s.stale_test_apps().unwrap().is_empty());
    }
    // Archived, in place.
    let (id, _) = card(&mut s, "archived", "Human test");
    s.claim_test_app(&id, t(0), 0, &NONE()).unwrap();
    let c = s.get(&id).unwrap();
    s.archive(&id, c.version, true, "Hugh").unwrap();
    let stale = s.stale_test_apps().unwrap();
    assert_eq!(stale[0].2, "the card was archived");
    s.free_test_app(&id, &stale[0].2, "the app").unwrap();
    assert!(!s.get(&id).unwrap().fields.contains_key(TEST_APP_KEY));
}

#[test]
fn a_stack_s_card_keeps_its_test_app_only_in_in_progress() {
    let (_d, mut s) = board();
    let (_, lead) = card(&mut s, "lead", "In progress");
    let (child, cn) = card(&mut s, "child", "In progress");
    s.stack_add(lead, cn, "t").unwrap();
    let lead_id = s.get_by_number(lead).unwrap().id;
    assert_eq!(s.claim_test_app(&lead_id, t(0), 0, &NONE()).unwrap(), SlotClaim::Given(1));
    assert_eq!(s.claim_test_app(&child, t(1), 0, &NONE()).unwrap(), SlotClaim::Given(2));
    mv(&mut s, &child, "Human test");
    let stale = s.stale_test_apps().unwrap();
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].0.id, child);
    assert!(stale[0].2.contains("lead's test app"), "{}", stale[0].2);
    // And it is given none there.
    assert!(s.claim_test_app(&child, t(2), 0, &NONE()).is_err());
}

#[test]
fn a_card_outside_in_progress_and_human_test_is_given_no_test_app() {
    let (_d, mut s) = board();
    let (id, _) = card(&mut s, "p", "Planning");
    let e = s.claim_test_app(&id, t(0), 0, &NONE()).unwrap_err().to_string();
    assert!(e.contains("Planning"), "{e}");
}

#[test]
fn freeing_a_test_app_also_ends_its_turn_and_place_on_that_slot() {
    let (_d, mut s) = board();
    let (a, an) = card(&mut s, "a", "In progress");
    let (b, bn) = card(&mut s, "b", "In progress");
    s.claim_test_app(&a, t(0), 0, &NONE()).unwrap();
    let mut q = s.test_app_queue_for(1).unwrap();
    q.take(Ask::tester(&a, an), t(0), false);
    q.take(Ask::tester(&b, bn), t(0), false);
    s.save_test_app_queue_for(1, &q).unwrap();
    assert_eq!(s.free_test_app(&a, "Hugh freed it", "Hugh").unwrap(), Some(1));
    let q = s.test_app_queue_for(1).unwrap();
    assert!(q.holder.is_none(), "a's turn went with its test app");
    assert_eq!(q.line.len(), 1, "b still waits");
    assert_eq!(s.free_test_app(&a, "again", "Hugh").unwrap(), None, "nothing left to free");
}

#[test]
fn two_cards_on_one_slot_the_first_given_keeps_it() {
    let (_d, mut s) = board();
    let (a, _) = card(&mut s, "a", "In progress");
    let (b, _) = card(&mut s, "b", "In progress");
    s.give_test_app(&a, 3, t(0)).unwrap();
    s.give_test_app(&b, 3, t(1)).unwrap();
    let owners = s.slot_owners().unwrap();
    assert_eq!(owners[&3].id, a);
    let stale = s.stale_test_apps().unwrap();
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].0.id, b);
    assert!(stale[0].2.contains("Test 3 was given to #"), "{}", stale[0].2);
}

#[test]
fn per_slot_queues_survive_a_reopen_and_test_s_is_the_old_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("b.db");
    let mut s = Store::open(&path).unwrap();
    let mut one = TestAppQueue::default();
    one.take(Ask::tester("a", 1), t(0), false);
    let mut two = TestAppQueue::default();
    two.take(Ask::install("router", "wave 3"), t(0), false);
    s.save_test_app_queue_for(1, &one).unwrap();
    s.save_test_app_queue_for(2, &two).unwrap();
    drop(s);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.test_app_queue().unwrap(), one, "slot 1 is testapp.queue, which an older build reads");
    assert!(s.setting("testapp.queue").unwrap().unwrap().contains("\"cardId\":\"a\""));
    assert_eq!(s.test_app_queue_for(2).unwrap(), two);
    assert!(s.setting("testapp.queue.2").unwrap().is_some());
    assert!(s.test_app_queue_for(3).unwrap().is_empty());
}

#[test]
fn the_ceiling_is_a_page_setting_from_1_to_10_default_10() {
    let (_d, mut s) = board();
    assert_eq!(s.test_slots_ceiling().unwrap(), 10);
    assert_eq!(s.page_setting("testapp.slots").unwrap(), "10");
    assert_eq!(s.set_page_setting("testapp.slots", " 4 ").unwrap(), "4");
    assert_eq!(s.test_slots_ceiling().unwrap(), 4);
    for bad in ["0", "11", "lots", ""] {
        assert!(s.set_page_setting("testapp.slots", bad).is_err(), "{bad:?}");
    }
    s.set_setting("testapp.slots", "99").unwrap();
    assert_eq!(s.test_slots_ceiling().unwrap(), 10, "a stored value out of range is clamped");
}

#[test]
fn sessions_can_t_write_test_app_through_a_patch_check() {
    // The field is the app's: it isn't in the tool's list (src-tauri/src/tools.rs UPDATABLE),
    // so only the store's own calls set it. Here: the store's update takes it like any field, and
    // card_slot ignores a slot out of range.
    let (_d, mut s) = board();
    let (a, _) = card(&mut s, "a", "In progress");
    let c = s.get(&a).unwrap();
    let mut p = Patch::new();
    p.insert(TEST_APP_KEY.into(), json!({ "slot": 11 }));
    s.update(&a, c.version, &p, "t").unwrap();
    assert_eq!(slot(&s, &a), None);
    assert_eq!(s.stale_test_apps().unwrap()[0].2, "it named no test app");
}

#[test]
fn a_card_with_an_old_turn_on_test_keeps_test_and_no_turn_is_left_behind() {
    let (_d, mut s) = board();
    let (a, an) = card(&mut s, "a", "In progress");
    // Before card #147: a holds the one turn, on Test, and has no test app.
    let mut q = TestAppQueue::default();
    q.take(Ask::tester(&a, an), t(0), false);
    s.save_test_app_queue_for(1, &q).unwrap();
    assert_eq!(s.claim_test_app(&a, t(1), 0, &NONE()).unwrap(), SlotClaim::Given(1), "its own turn doesn't make Test taken");
    // Another card's turn on Test does.
    let (b, _) = card(&mut s, "b", "In progress");
    let mut q2 = TestAppQueue::default();
    q2.take(Ask::tester(&b, 99), t(0), false);
    s.save_test_app_queue_for(3, &q2).unwrap();
    assert_eq!(s.claim_test_app(&b, t(2), 0, &NONE()).unwrap(), SlotClaim::Given(3), "b keeps its own old turn's test app");
}

#[test]
fn a_card_that_can_t_use_a_test_app_ahead_in_line_doesn_t_hold_a_free_one_back() {
    let (_d, mut s) = board();
    let (x, xn) = card(&mut s, "x", "In progress");
    let mut line = s.slot_line().unwrap();
    line.join(&x, xn, t(0));
    s.save_slot_line(&line).unwrap();
    mv(&mut s, &x, "Done");
    let (y, _) = card(&mut s, "y", "In progress");
    assert_eq!(s.claim_test_app(&y, t(1), 0, &NONE()).unwrap(), SlotClaim::Given(1));
    assert!(s.slot_line().unwrap().line.is_empty());
}
