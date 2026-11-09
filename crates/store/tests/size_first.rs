// Card #159: the setting that has the app size a card before its session starts.

use kanban_store::{Store, PAGE_SETTINGS, RULE_KEYS, SIZE_FIRST_KEY};

#[test]
fn sizing_before_the_start_is_on_by_default_and_hugh_can_turn_it_off() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("board.db")).unwrap();
    assert_eq!(SIZE_FIRST_KEY, "sessions.size_before_start");
    assert!(RULE_KEYS.contains(&SIZE_FIRST_KEY) && PAGE_SETTINGS.contains(&SIZE_FIRST_KEY));
    assert_eq!(kanban_store::page_default(SIZE_FIRST_KEY).unwrap(), "on");
    assert_eq!(s.page_setting(SIZE_FIRST_KEY).unwrap(), "on");
    assert!(s.size_first());
    assert_eq!(s.set_page_setting(SIZE_FIRST_KEY, "off").unwrap(), "off");
    assert!(!s.size_first());
    assert!(s.set_page_setting(SIZE_FIRST_KEY, "maybe").is_err());
    assert!(!s.size_first(), "a refused value changes nothing");
    assert_eq!(s.set_page_setting(SIZE_FIRST_KEY, "on").unwrap(), "on");
    assert!(s.size_first());
}
