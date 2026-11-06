//! Test-app turns (card #115): the parts that touch the whole store.

use kanban_store::{Ask, Store, TestAppQueue};

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

    let to = dir.path().join("test").join("board.db");
    kanban_store::copy_board(&live, &to, false).unwrap();
    let copy = Store::open(&to).unwrap();
    assert!(copy.test_app_queue().unwrap().is_empty(), "the live holder and line stay on the live board");
    assert_eq!(copy.test_app_limit().unwrap(), 90, "the setting itself is copied");
    // The source is only read.
    assert_eq!(s.test_app_queue().unwrap(), q);
}
