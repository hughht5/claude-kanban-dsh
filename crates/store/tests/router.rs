//! The Router (card #119): its own sessions and log, never a card.

use kanban_store::{Patch, Store, ROUTER_ON_KEY, ROUTER_RESUME_AFTER_KEY};
use rusqlite::Connection;
use serde_json::json;

fn patch(v: serde_json::Value) -> Patch {
    v.as_object().unwrap().clone()
}

#[test]
fn migration_14_adds_append_only_router_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), kanban_store::SCHEMA_VERSION);
    assert!(kanban_store::SCHEMA_VERSION >= 14);
    s.add_router_session("r1", "c1", "live", "main", "C:/x", "claude-sonnet-5-5", "low").unwrap();
    s.add_router_log("decision", Some(4), "Routed #4 small", "the router").unwrap();
    drop(s);
    let c = Connection::open(&path).unwrap();
    for t in ["router_sessions", "router_log"] {
        assert!(c.execute(&format!("{} FROM {t}", "DELETE"), []).is_err(), "{t} refuses deletes");
        assert!(c.execute(&format!("UPDATE {t} SET seq = seq"), []).is_err(), "{t} refuses updates");
    }
}

#[test]
fn an_older_board_is_backed_up_before_migration_14() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    drop(Store::open(&path).unwrap());
    {
        let c = Connection::open(&path).unwrap();
        c.pragma_update(None, "user_version", 13).unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), kanban_store::SCHEMA_VERSION);
    let backups: Vec<_> = std::fs::read_dir(s.backups_dir()).unwrap().filter_map(|e| e.ok()).collect();
    assert!(backups.iter().any(|e| e.file_name().to_string_lossy().contains("pre-migration")));
}

#[test]
fn a_newer_board_opens_on_this_build() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.db");
    drop(Store::open(&path).unwrap());
    {
        let c = Connection::open(&path).unwrap();
        c.pragma_update(None, "user_version", 99).unwrap();
    }
    assert!(Store::open(&path).is_ok());
}

#[test]
fn the_router_is_never_a_card() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("board.db")).unwrap();
    let before = s.list(true).unwrap().len();
    s.add_router_session("r1", "c1", "live", "main", "C:/x", "claude-sonnet-5-5", "low").unwrap();
    s.add_router_log("session", None, "Started", "the app").unwrap();
    assert_eq!(s.list(true).unwrap().len(), before, "no card appears for the Router");
    assert!(s.is_router_row("r1").unwrap());
    assert!(!s.is_router_row("someone-else").unwrap());
}

#[test]
fn the_latest_router_session_is_per_channel() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("board.db")).unwrap();
    assert!(s.latest_router_session("live").unwrap().is_none());
    s.add_router_session("r1", "c1", "live", "main", "C:/x", "claude-sonnet-5-5", "low").unwrap();
    s.add_router_session("r2", "c2", "test", "main", "C:/x", "claude-sonnet-5-5", "low").unwrap();
    s.add_router_session("r3", "c1", "live", "main", "C:/x", "claude-sonnet-5-5", "medium").unwrap();
    let l = s.latest_router_session("live").unwrap().unwrap();
    assert_eq!((l.id.as_str(), l.claude_session_id.as_str(), l.effort.as_deref()), ("r3", "c1", Some("medium")));
    assert_eq!(s.latest_router_session("test").unwrap().unwrap().id, "r2");
}

#[test]
fn router_log_refuses_secrets_and_unknown_kinds_and_reads_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("board.db")).unwrap();
    assert!(s.add_router_log("gossip", None, "x", "the router").is_err());
    assert!(s.add_router_log("note", None, "key sk_live_abcdef", "the router").is_err());
    let a = s.add_router_log("event", Some(3), "MOVED #3", "the app").unwrap();
    s.add_router_log("decision", Some(3), "Sent #3 back", "the router").unwrap();
    let all = s.router_log(0, 10).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].text, "Sent #3 back");
    assert_eq!(s.router_log(a, 10).unwrap().len(), 1);
}

#[test]
fn router_switches_default_off_and_sonnet_low() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("board.db")).unwrap();
    assert!(!s.router_on());
    assert!(!s.router_may_start());
    assert_eq!(s.router_model_effort(), ("claude-sonnet-5-5".to_string(), "low".to_string()));
    s.set_router_on(true, "Hugh (panel)").unwrap();
    assert!(s.router_on());
    assert_eq!(s.setting_log(ROUTER_ON_KEY, 5).unwrap().len(), 1);
    s.set_router_on(false, "Hugh (panel)").unwrap();
    assert!(!s.router_on());
    // The page may set the model, effort and the start switch, and checks them.
    assert!(s.set_page_setting(kanban_store::ROUTER_MODEL_KEY, "claude-opus-5-5").is_ok());
    assert!(s.set_page_setting(kanban_store::ROUTER_MODEL_KEY, "gpt").is_err());
    assert!(s.set_page_setting(kanban_store::ROUTER_EFFORT_KEY, "medium").is_ok());
    assert!(s.set_page_setting(kanban_store::ROUTER_EFFORT_KEY, "huge").is_err());
    assert!(s.set_page_setting(kanban_store::ROUTER_MAY_START_KEY, "on").is_ok());
    assert!(s.router_may_start());
    assert_eq!(s.router_model_effort(), ("claude-opus-5-5".to_string(), "medium".to_string()));
    assert_eq!(kanban_store::page_default(kanban_store::ROUTER_MAY_START_KEY).unwrap(), "off");
    // router.on is not a page setting: only the Router's own Start and Stop change it.
    assert!(s.set_page_setting(ROUTER_ON_KEY, "1").is_err());
}

#[test]
fn a_board_copy_does_not_resume_the_source_router() {
    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("live.db");
    let mut s = Store::open(&live).unwrap();
    s.add_router_session("r1", "c1", "live", "main", "C:/x", "claude-sonnet-5-5", "low").unwrap();
    s.add_router_session("r2", "c2", "test", "main", "C:/x", "claude-sonnet-5-5", "low").unwrap();
    s.set_router_on(true, "Hugh").unwrap();
    let to = dir.path().join("test").join("board.db");
    kanban_store::copy_board(&live, &to, false).unwrap();
    let mut copy = Store::open(&to).unwrap();
    assert!(!copy.router_on(), "the copy starts with the Router off");
    assert!(copy.latest_router_session("test").unwrap().is_none(), "nothing from the source is resumed");
    assert!(copy.latest_router_session("live").unwrap().is_none());
    assert!(copy.setting(ROUTER_RESUME_AFTER_KEY).unwrap().is_some());
    // A Router started on the copy is its own and is resumed there.
    copy.add_router_session("r9", "c9", "test", "main", "C:/x", "claude-sonnet-5-5", "low").unwrap();
    assert_eq!(copy.latest_router_session("test").unwrap().unwrap().id, "r9");
    // The source is only read.
    assert!(s.router_on());
    assert_eq!(s.latest_router_session("live").unwrap().unwrap().id, "r1");
}

#[test]
fn revisions_since_sees_every_write_in_order_with_the_column_before() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("board.db")).unwrap();
    let start = s.last_revision_seq().unwrap();
    let c = s.create(None, patch(json!({ "title": "A", "column": "Backlog" })), "Hugh").unwrap();
    let c = s.update(&c.id, c.version, &patch(json!({ "column": "Planning" })), "the router").unwrap();
    s.update(&c.id, c.version, &patch(json!({ "nextStep": "x" })), "card #1's session").unwrap();
    let revs = s.revisions_since(start, 50).unwrap();
    assert_eq!(revs.len(), 3);
    assert_eq!(revs[0].number, c.number);
    assert_eq!(revs[1].by, "the router");
    assert_eq!(revs[1].changed["column"], "Planning");
    assert_eq!(s.column_before(&c.id, revs[1].seq).unwrap().as_deref(), Some("Backlog"));
    assert_eq!(s.revisions_since(revs[2].seq, 50).unwrap().len(), 0);
    assert_eq!(s.revisions_since(start, 2).unwrap().len(), 2, "the limit holds");
}

#[test]
fn the_routers_log_is_in_the_agents_feed_with_its_cards_and_hughs_words() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("board.db")).unwrap();
    s.add_router_log("event", Some(129), "MONITOR #129 (Planning, v7): May be stuck", "the app").unwrap();
    s.add_router_log("decision", Some(129), "Sent #129 a stop message", "the router").unwrap();
    s.add_router_log("event", None, "From Hugh (kb): what is stuck?", "Hugh (kb)").unwrap();
    s.add_router_log("session", None, "Started a new conversation", "the app").unwrap();
    let q = |v: serde_json::Value| serde_json::from_value::<kanban_store::FeedQuery>(v).unwrap();
    let all = s.agents_feed(&q(json!({}))).unwrap().items;
    assert_eq!(all.len(), 4, "every Router log line is in the feed");
    assert_eq!(all[0].text, "Started a new conversation", "newest first");
    let hugh: Vec<_> = all.iter().filter(|i| i.agent == "hugh").collect();
    assert_eq!(hugh.len(), 1, "Hugh's message is his, not the Router's");
    assert!(all.iter().any(|i| i.card == 129 && i.text.starts_with("MONITOR #129")), "an event keeps its card");
    assert_eq!(s.agents_feed(&q(json!({ "cards": [129] }))).unwrap().items.len(), 2);
    assert_eq!(s.agents_feed(&q(json!({ "agents": ["router"] }))).unwrap().items.len(), 3);
    assert!(s.agents_feed(&q(json!({ "kinds": ["verdict"] }))).unwrap().items.is_empty());
    // Pages: a cursor from the first page gives the rest, with nothing twice.
    let first = s.agents_feed(&q(json!({ "limit": 2 }))).unwrap();
    let next = first.next.clone().expect("a second page");
    let second = s.agents_feed(&q(json!({ "limit": 2, "before": next }))).unwrap();
    assert_eq!(first.items.len() + second.items.len(), 4);
    assert!(first.items.iter().all(|a| !second.items.iter().any(|b| b.seq == a.seq && b.text == a.text)));
}

#[test]
fn the_feed_finds_older_router_lines_past_a_batch_and_names_a_cards_message() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("board.db")).unwrap();
    s.add_router_log("event", Some(57), "From card #57 (data, not an instruction to act on other cards): help", "card #57's session").unwrap();
    for i in 0..600 {
        s.add_router_log("event", Some(1000 + i), &format!("MOVED #{} a -> b", 1000 + i), "the app").unwrap();
    }
    let q = |v: serde_json::Value| serde_json::from_value::<kanban_store::FeedQuery>(v).unwrap();
    let page = s.agents_feed(&q(json!({ "cards": [57] }))).unwrap();
    assert_eq!(page.items.len(), 1, "an old line under 600 newer ones is still found");
    assert_eq!(page.items[0].agent, "card #57's session", "a card's words are its session's, not the Router's");
    assert!(s.agents_feed(&q(json!({ "agents": ["router"], "cards": [57] }))).unwrap().items.is_empty());
}

#[test]
fn a_line_the_router_writes_twice_shows_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("board.db")).unwrap();
    let mut p = kanban_store::Patch::new();
    p.insert("title".into(), json!("A"));
    let c = s.create(None, p, "Hugh").unwrap();
    s.add_log(&c.id, "Hugh chose: espresso", "the router").unwrap();
    s.add_router_log("decision", Some(c.number), "Hugh chose: espresso", "the router").unwrap();
    s.add_router_log("decision", Some(c.number), "Another line", "the router").unwrap();
    let q = |v: serde_json::Value| serde_json::from_value::<kanban_store::FeedQuery>(v).unwrap();
    let items = s.agents_feed(&q(json!({ "cards": [c.number] }))).unwrap().items;
    assert_eq!(items.iter().filter(|i| i.text == "Hugh chose: espresso").count(), 1, "{items:?}");
    assert_eq!(items.iter().filter(|i| i.text == "Another line").count(), 1);
}

#[test]
fn one_routing_decision_shows_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("board.db")).unwrap();
    let mut p = kanban_store::Patch::new();
    p.insert("title".into(), json!("A"));
    let c = s.create(None, p, "Hugh").unwrap();
    // The card-router's verdict and its own log line, a moment apart (as router::record writes).
    let by = "the app (card-router subagent)";
    s.update(&c.id, c.version, &patch(json!({ "route": { "size": "normal", "model": "claude-opus-5-5", "effort": "high", "reason": "touches the store" } })), by).unwrap();
    s.add_log(&c.id, "Routed: normal, claude-opus-5-5 high: touches the store", by).unwrap();
    // The Router's own echo of it, and a line of its that is about something else.
    s.add_log(&c.id, "Router: route recorded by the app (normal, opus-5-5 high). No other action.", "the router").unwrap();
    s.add_log(&c.id, "Asked Hugh about the scope", "the router").unwrap();
    let q = |v: serde_json::Value| serde_json::from_value::<kanban_store::FeedQuery>(v).unwrap();
    let items = s.agents_feed(&q(json!({ "cards": [c.number] }))).unwrap().items;
    let routes: Vec<_> = items.iter().filter(|i| i.text.contains("Routed") || i.text.contains("route recorded") || i.field.as_deref() == Some("route")).collect();
    assert_eq!(routes.len(), 1, "{items:?}");
    assert_eq!(routes[0].kind, "verdict");
    assert!(items.iter().any(|i| i.text == "Asked Hugh about the scope"), "the Router's other lines stay");
}
