//! Small-change auto-ship (Hugh, card #104). For the repos Hugh lists in Settings
//! (`sessions.small_goes_on_repos`, empty by default), a card whose own session moves it into
//! Human test is checked by `small.rs`. If its change is small and fully checked, it waits 10
//! minutes in Human test (a chip with Stop, and the panel's opt-out), is checked again, and the
//! app moves it into Ship it for Hugh as Take it to does. The check runs again when the ship is
//! released, and the push is checked when the session reports it. A card goes on by itself at
//! most once. Every git call runs off the board lock.

use crate::{doc, refused, small, Board, CmdError, CmdResult};
use kanban_store::{Card, Patch, SmallRepo, Store, SMALL_BY, SMALL_KEY, SMALL_OPT_OUT_KEY, SMALL_WAITING};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager, State};

/// How long a card that passed waits in Human test for Hugh to stop it.
pub const GRACE_MINUTES: i64 = 10;
/// The state of a card that went on.
pub const WENT_ON: &str = "went on";
/// The state of a card that stopped for Hugh.
pub const STOPPED: &str = "stopped";

/// Whether a card's CSS change has #101's before-and-after screenshots (router's decision 3):
/// the card-tester's latest pass saved an "after" shot in that same run.
fn css_evidence(_store: &Store, card: &Card) -> bool {
    kanban_store::has_tested_evidence(card)
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn repo_of(card: &Card) -> &str {
    card.fields.get("repo").and_then(Value::as_str).unwrap_or("")
}

/// The card's small-change record, if any.
pub fn record(card: &Card) -> Option<&serde_json::Map<String, Value>> {
    card.fields.get(SMALL_KEY).and_then(Value::as_object)
}

/// The record's state: "waiting", "went on" or "stopped".
pub fn state(card: &Card) -> Option<&str> {
    record(card).and_then(|r| r.get("state")).and_then(Value::as_str)
}

/// Did this card go on to Ship it by itself (it is now in Ship it or later because of it)?
pub fn went_on(card: &Card) -> bool {
    state(card) == Some(WENT_ON)
}

/// The commit the app checked, for a card that went on.
pub fn head(card: &Card) -> Option<&str> {
    record(card).and_then(|r| r.get("head")).and_then(Value::as_str).filter(|h| !h.is_empty())
}

/// What the Ship it session is told about a card that went on (card #104, critic 2 and 15).
pub fn ship_note(card: &Card) -> Option<String> {
    if !went_on(card) || card.column != "Ship it" {
        return None;
    }
    let r = record(card)?;
    let branch = r.get("branch").and_then(Value::as_str).unwrap_or("main");
    let h = head(card)?;
    Some(format!(
        "The app moved this card into Ship it for Hugh: the change was small and fully checked, and his setting lets such cards in {repo} go on by themselves (small-change auto-ship). This is his approval to ship it with no human test. Push exactly the commit the app checked, {h}, and nothing else: `git push origin {h}:{branch}`. The app checks that origin/{branch} is {h} when you report the ship.",
        repo = repo_of(card)
    ))
}

/// Sets the card's record and logs `line`, as the app.
fn write(store: &mut Store, card: &Card, rec: Value, line: &str) -> Option<Card> {
    let mut patch = Patch::new();
    patch.insert(SMALL_KEY.into(), rec);
    let c = store.update(&card.id, card.version, &patch, SMALL_BY).ok()?;
    let _ = store.add_log(&card.id, line, SMALL_BY);
    Some(c)
}

fn stop(store: &mut Store, card: &Card, reason: &str) -> Option<Card> {
    let mut rec = record(card).cloned().unwrap_or_default();
    rec.insert("state".into(), json!(STOPPED));
    rec.insert("at".into(), json!(now()));
    rec.insert("reason".into(), json!(reason));
    write(store, card, Value::Object(rec), &format!("Stops in Human test for Hugh: {reason}"))
}

fn tell(app: &AppHandle, card: &Card, text: &str) {
    // The same non-activating toast as kanban_notify: it never takes Hugh's focus.
    let _ = app.emit("notify", json!({ "card": card.id, "number": card.number, "text": text }));
}

/// What a check needs, read under the lock: the card, Hugh's entry for its repo, and whether a
/// CSS change has its screenshots. None when the card isn't one to check.
fn read(store: &Store, card_id: &str) -> Option<(Card, SmallRepo, bool)> {
    let card = store.get(card_id).ok()?;
    if small::skip(&card) {
        return None;
    }
    let entry = store.small_repo_for(repo_of(&card))?;
    let ev = css_evidence(store, &card);
    Some((card, entry, ev))
}

/// The whole check at once (git, then the rules), for the tests.
#[cfg(test)]
fn check(card: &Card, entry: &SmallRepo, ev: bool) -> Result<Value, String> {
    let facts = small::git_facts(&entry.checkout, &entry.branch)?;
    let rec = small::decide(card, entry, &facts, ev)?;
    clean_record(rec)
}

/// The rules, on the card as it is now (read under the lock), with git facts read just before
/// without it. Deciding on the fresh card, not comparing versions, is what lets the app's own
/// writes straight after a move into Human test (changes saved, the diff check) pass: they
/// change the version but nothing the rules read (the test app showed the race, card #104).
/// None when the card is no longer one to check: moved on, skipped, or its repo's entry changed.
fn decide_now(store: &Store, card_id: &str, entry: &SmallRepo, facts: &Result<small::Facts, String>) -> Option<(Card, Result<Value, String>)> {
    let card = store.get(card_id).ok()?;
    if card.column != "Human test" || small::skip(&card) || store.small_repo_for(repo_of(&card)).as_ref() != Some(entry) {
        return None;
    }
    let ev = css_evidence(store, &card);
    let verdict = match facts {
        Ok(f) => small::decide(&card, entry, f, ev).and_then(clean_record),
        Err(e) => Err(e.clone()),
    };
    Some((card, verdict))
}

/// The record goes on the card, so nothing in it (a commit subject, a path) may look like a
/// secret: the store would refuse it, and the board never shows one. Such a change stops for
/// Hugh, with a reason that quotes nothing.
fn clean_record(rec: Value) -> Result<Value, String> {
    if kanban_store::looks_secret(&rec.to_string()) {
        return Err("a commit message or path in the change looks like it holds a secret".into());
    }
    Ok(rec)
}

/// After a card's own session moved it into Human test (its held move, applied when its turn
/// ended): checks it in a thread and, if it passed, starts its wait. `had_auto`: the card had
/// Take it to set before the move (which the move may already have cleared); such a card never
/// goes on this way.
pub fn consider(app: &AppHandle, card_id: &str, had_auto: bool) {
    if had_auto {
        return;
    }
    let (app, id) = (app.clone(), card_id.to_string());
    std::thread::spawn(move || consider_now(&app, &id));
}

fn consider_now(app: &AppHandle, card_id: &str) {
    let board = app.state::<Board>();
    {
        let Some((card, entry, _)) = board.0.lock().ok().and_then(|s| read(&s, card_id)) else { return };
        if card.column != "Human test" {
            return;
        }
        let facts = small::git_facts(&entry.checkout, &entry.branch);
        let Ok(mut store) = board.0.lock() else { return };
        let Some((card, verdict)) = decide_now(&store, card_id, &entry, &facts) else { return };
        match verdict {
            Ok(mut ev) => {
                let at = chrono::Utc::now();
                let goes = at + chrono::Duration::minutes(GRACE_MINUTES);
                ev["state"] = json!(SMALL_WAITING);
                ev["at"] = json!(at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
                ev["goesAt"] = json!(goes.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
                let line = format!(
                    "Goes on by itself in {GRACE_MINUTES} minutes unless Hugh stops it: {}; tester PASS; reviewer PASS (small-change auto-ship for {})",
                    summary(&ev),
                    entry.repo
                );
                if write(&mut store, &card, ev, &line).is_some() {
                    tell(app, &card, &format!("goes on to Ship it by itself in {GRACE_MINUTES} min unless you stop it ({})", summary_short(&card)));
                }
            }
            Err(reason) => {
                let _ = stop(&mut store, &card, &reason);
            }
        }
    }
    let _ = app.emit("cards-changed", ());
}

/// "README.md (docs, +2 −1)", for the log.
fn summary(ev: &Value) -> String {
    let files: Vec<String> = ev
        .get("files")
        .and_then(Value::as_array)
        .map(|fs| {
            fs.iter()
                .map(|f| format!("{} ({}, +{} -{})", f["path"].as_str().unwrap_or("?"), f["kind"].as_str().unwrap_or("?"), f["added"], f["removed"]))
                .collect()
        })
        .unwrap_or_default();
    format!("{} lines in {}", ev["lines"], files.join(", "))
}

fn summary_short(card: &Card) -> String {
    let t: String = card.title().chars().take(60).collect();
    t
}

static TICKING: AtomicBool = AtomicBool::new(false);

/// Every 20 s from the dispatch loop: moves on each card whose wait is over, in a thread.
pub fn tick(app: &AppHandle) {
    if TICKING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let due: Vec<String> = app
            .state::<Board>()
            .0
            .lock()
            .ok()
            .and_then(|s| s.list(false).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|c| due(c, chrono::Utc::now()))
            .map(|c| c.id)
            .collect();
        for id in due {
            go_on(&app, &id);
        }
        TICKING.store(false, Ordering::SeqCst);
    });
}

/// Is `card`'s wait over at `now`? (Whether it may still go on is `go_on`'s to check: a wait
/// that can't end in a move is stopped there, never left waiting.)
pub fn due(card: &Card, now: chrono::DateTime<chrono::Utc>) -> bool {
    if card.column != "Human test" || state(card) != Some(SMALL_WAITING) {
        return false;
    }
    record(card)
        .and_then(|r| r.get("goesAt"))
        .and_then(Value::as_str)
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .map_or(false, |t| t <= now)
}

/// Why a waiting card may no longer go on at all (not a failed check, a changed rule), if so.
fn cant_go_on(store: &Store, card: &Card) -> Option<String> {
    let why = if card.archived {
        "it was archived"
    } else if card.stack_of().is_some() || card.is_lead() {
        "it is in a stack now, and a stack ships whole"
    } else if card.auto_to().is_some() {
        "Hugh set Take it to on it"
    } else if card.fields.get(SMALL_OPT_OUT_KEY).and_then(Value::as_bool) == Some(true) {
        "Hugh turned small-change auto-ship off for it"
    } else if store.small_repo_for(repo_of(card)).is_none() {
        "its repo is no longer on the small-change list"
    } else {
        return None;
    };
    Some(why.to_string())
}

/// A card whose wait is over: checked again, then moved into Ship it for Hugh.
fn go_on(app: &AppHandle, card_id: &str) {
    let board = app.state::<Board>();
    // A wait that can no longer end in a move is stopped, so it never waits on, and never goes
    // on later with no fresh wait (if its repo is listed again, say).
    let read_due = |s: &mut Store| -> Option<(Card, SmallRepo, bool)> {
        let card = s.get(card_id).ok()?;
        if !due(&card, chrono::Utc::now()) {
            return None;
        }
        if let Some(why) = cant_go_on(s, &card) {
            if let Some(c) = stop(s, &card, &why) {
                tell(app, &c, &format!("stays in Human test for you: {why}"));
            }
            return None;
        }
        read(s, card_id)
    };
    let Some((_, entry, _)) = board.0.lock().ok().and_then(|mut s| read_due(&mut s)) else { return };
    let facts = small::git_facts(&entry.checkout, &entry.branch);
    let Ok(mut store) = board.0.lock() else { return };
    // Decided on the card as it is now; one that stopped waiting meanwhile isn't moved.
    let Some((card, verdict)) = decide_now(&store, card_id, &entry, &facts) else { return };
    if !due(&card, chrono::Utc::now()) {
        return;
    }
    let waited_head = head(&card).unwrap_or("").to_string();
    let mut rec = match verdict {
        Ok(rec) if rec["head"].as_str() == Some(waited_head.as_str()) => rec,
        Ok(_) => {
            let why = "its commit changed during the wait";
            if let Some(c) = stop(&mut store, &card, why) {
                tell(app, &c, &format!("stays in Human test for you: {why}"));
            }
            drop(store);
            let _ = app.emit("cards-changed", ());
            return;
        }
        Err(reason) => {
            if let Some(c) = stop(&mut store, &card, &reason) {
                tell(app, &c, &format!("stays in Human test for you: {reason}"));
            }
            drop(store);
            let _ = app.emit("cards-changed", ());
            return;
        }
    };
    let at = now();
    rec["state"] = json!(WENT_ON);
    rec["at"] = json!(at);
    rec["wentOnAt"] = json!(at);
    rec["goesAt"] = record(&card).and_then(|r| r.get("goesAt")).cloned().unwrap_or(Value::Null);
    let line = format!("Went on by itself: a small, fully checked change ({}); tester PASS; reviewer PASS. Hugh's small-change auto-ship for {}", summary(&rec), entry.repo);
    let why = format!("the change was small and fully checked, and his small-change auto-ship is on for {}", entry.repo);
    // The record and the move are one change: neither is ever written without the other.
    let mut extra = Patch::new();
    extra.insert(SMALL_KEY.into(), rec);
    match store.move_for_hugh_with(&card.id, card.version, "Ship it", &why, extra) {
        Ok(m) => {
            let _ = store.add_log(&m.card.id, &line, SMALL_BY);
            tell(app, &m.card, &format!("went on to Ship it by itself ({})", summary_short(&m.card)));
            drop(store);
            // Its checkout was checked just now, so the ship's start isn't checked again here;
            // a ship that waits its turn is checked when it is released.
            let (a, id) = (app.clone(), m.card.id.clone());
            std::thread::spawn(move || crate::dispatch::after_move_checked(&a, &id, "Human test"));
        }
        Err(e) => {
            if let Ok(c) = store.get(card_id) {
                let _ = stop(&mut store, &c, &format!("the app couldn't move it into Ship it ({e})"));
            }
            drop(store);
        }
    }
    let _ = app.emit("cards-changed", ());
}

/// Before a released ship's prompt goes to a card that went on (critic 2 and 3): its checkout
/// must still be at the commit the app checked, on its branch, with nothing uncommitted. If it
/// isn't, the card goes back to Human test for Hugh. True when it went back.
pub fn ship_start_refused(app: &AppHandle, card_id: &str) -> bool {
    let board = app.state::<Board>();
    let read_now = |s: &Store| -> Option<(Card, Option<SmallRepo>)> {
        let c = s.get(card_id).ok()?;
        // Only before its ship begins: a ship already running is never pulled back.
        let ship = c.fields.get("ship").and_then(|v| v.get("state")).and_then(Value::as_str);
        let waiting = matches!(ship, Some("requested") | Some(kanban_store::SHIP_QUEUED));
        (c.column == "Ship it" && went_on(&c) && waiting).then(|| {
            let e = s.small_repo_for(repo_of(&c));
            (c, e)
        })
    };
    let Some((card, entry)) = board.0.lock().ok().and_then(|s| read_now(&s)) else { return false };
    let want = head(&card).unwrap_or("").to_string();
    let facts = entry.as_ref().map(|e| small::git_facts(&e.checkout, &e.branch));
    let Some(why) = ship_start_problem(entry.as_ref(), facts.as_ref(), &want) else { return false };
    let Ok(mut store) = board.0.lock() else { return true };
    let Ok(c) = store.get(card_id) else { return true };
    let reason = format!("Back to Human test before its ship started: {why}");
    let Some(c) = stop(&mut store, &c, &format!("{why} (checked when its ship was released)")) else { return true };
    if let Err(e) = store.move_for_hugh(&c.id, c.version, "Human test", &reason) {
        let _ = store.add_log(&c.id, &format!("Couldn't move it back to Human test: {e}. Hugh, don't let this ship run."), SMALL_BY);
    }
    // Its session hears why, so it doesn't think a ship is coming.
    let _ = store.queue(
        &c.id,
        "message",
        &format!("The app moved this card back from Ship it to Human test before its ship started: {why}. It no longer goes on by itself; Hugh moves it on from Human test. Don't ship it."),
    );
    tell(app, &c, &format!("stopped before its ship: {why}"));
    drop(store);
    let _ = app.emit("cards-changed", ());
    true
}

/// Why a released ship of a card that went on must not start, if it mustn't: its repo is off the
/// list, or its checkout isn't at the checked commit `want`, clean, on its branch.
pub fn ship_start_problem(entry: Option<&SmallRepo>, facts: Option<&Result<small::Facts, String>>, want: &str) -> Option<String> {
    let (Some(e), Some(facts)) = (entry, facts) else {
        return Some("Hugh took its repo off the small-change list before the ship started".to_string());
    };
    match facts {
        Err(err) => Some(format!("the ship's checkout couldn't be checked ({err})")),
        Ok(f) if f.tamper.is_some() => Some(format!("the ship's checkout can't be trusted ({})", f.tamper.clone().unwrap_or_default())),
        Ok(f) if want.is_empty() || f.head != want => Some(format!("the ship's checkout is at {} now, not the checked commit {}", short(&f.head), short(want))),
        Ok(f) if f.branch_now != e.branch => Some(format!("the ship's checkout is on {} now, not {}", if f.branch_now.is_empty() { "no branch" } else { &f.branch_now }, e.branch)),
        Ok(f) if !f.status_clean => Some("the ship's checkout has changes that aren't committed".to_string()),
        Ok(_) => None,
    }
}

/// The log line after a reported ship of a card that went on: Ok when origin/<branch> is the
/// checked commit `want`, Err (a warning for Hugh) when it isn't or can't be read.
pub fn push_check(base: Option<Result<String, String>>, branch: &str, want: &str) -> Result<String, String> {
    match base {
        Some(Ok(b)) if !want.is_empty() && b == want => Ok(format!("Checked the push: origin/{branch} is {}, the commit the app checked", short(want))),
        Some(Ok(b)) => Err(format!("WARNING: after the ship, origin/{branch} is {}, not the checked commit {}. Something else was pushed; Hugh, look at it.", short(&b), short(want))),
        Some(Err(e)) => Err(format!("WARNING: couldn't check the push ({e}). Hugh, check that only {} was pushed.", short(want))),
        None => Err(format!("WARNING: its repo is no longer on the small-change list, so the push wasn't checked. Hugh, check that only {} was pushed.", short(want))),
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

/// After the session reported a ship of a card that went on (critic 2): origin/<branch> in the
/// checkout must be the commit the app checked. The push can't be undone, so a mismatch is a
/// loud log line and a notification.
pub fn after_shipped(app: &AppHandle, card_id: &str) {
    let (app, id) = (app.clone(), card_id.to_string());
    std::thread::spawn(move || {
        let board = app.state::<Board>();
        let Some((card, entry)) = board.0.lock().ok().and_then(|s| {
            let c = s.get(&id).ok()?;
            if !went_on(&c) {
                return None;
            }
            let e = s.small_repo_for(repo_of(&c));
            Some((c, e))
        }) else {
            return;
        };
        let want = head(&card).unwrap_or("").to_string();
        let branch = entry.as_ref().map(|e| e.branch.clone()).unwrap_or_default();
        let base = entry.map(|e| small::git_facts(&e.checkout, &e.branch).map(|f| f.base));
        let line = push_check(base, &branch, &want);
        let Ok(mut store) = board.0.lock() else { return };
        match line {
            Ok(l) => {
                let _ = store.add_log(&card.id, &l, SMALL_BY);
            }
            Err(l) => {
                let _ = store.add_log(&card.id, &l, SMALL_BY);
                tell(&app, &card, &l);
            }
        }
        drop(store);
        let _ = app.emit("cards-changed", ());
    });
}

/// Hugh's Stop on a waiting card, and the panel's opt-out (`on`), or his undo of it.
#[tauri::command]
pub fn small_opt_out(app: AppHandle, board: State<Board>, id: String, version: i64, on: bool) -> CmdResult<Value> {
    let mut store = board.0.lock().map_err(CmdError::other)?;
    let card = store.get(&id).map_err(|e| refused(&store, e))?;
    let mut patch = Patch::new();
    patch.insert(SMALL_OPT_OUT_KEY.into(), json!(on));
    let waiting = state(&card) == Some(SMALL_WAITING);
    if on && waiting {
        let mut rec = record(&card).cloned().unwrap_or_default();
        rec.insert("state".into(), json!(STOPPED));
        rec.insert("at".into(), json!(now()));
        rec.insert("reason".into(), json!("Hugh stopped it"));
        patch.insert(SMALL_KEY.into(), Value::Object(rec));
    }
    let c = store.update(&id, version, &patch, "Hugh (board)").map_err(|e| refused(&store, e))?;
    let line = match (on, waiting) {
        (true, true) => "Hugh stopped it going on by itself: it stays in Human test for him",
        (true, false) => "Hugh turned small-change auto-ship off for this card",
        (false, _) => "Hugh let this card go on by itself again, if its change is small and fully checked",
    };
    let _ = store.add_log(&id, line, "Hugh (board)");
    let out = doc(&store, &c);
    drop(store);
    let _ = app.emit("cards-changed", ());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(fields: Value, column: &str) -> Card {
        Card { id: "c9".into(), number: 9, column: column.into(), order: None, order_col: None, archived: false, fields: fields.as_object().cloned().unwrap(), version: 1 }
    }

    #[test]
    fn a_wait_is_due_only_in_human_test_after_its_time() {
        let at = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&chrono::Utc);
        let w = json!({ "title": "T", "smallChange": { "state": "waiting", "goesAt": "2026-11-06T10:10:00.000Z" } });
        let c = card(w.clone(), "Human test");
        assert!(!due(&c, at("2026-11-06T10:09:59Z")));
        assert!(due(&c, at("2026-11-06T10:10:00Z")));
        assert!(!due(&card(w.clone(), "In progress"), at("2026-11-06T11:00:00Z")));
        // Opted out (or off the list): still due, so go_on stops it rather than leave it waiting.
        let mut opt = w.clone();
        opt["smallOptOut"] = json!(true);
        assert!(due(&card(opt, "Human test"), at("2026-11-06T11:00:00Z")));
        let stopped = json!({ "title": "T", "smallChange": { "state": "stopped", "goesAt": "2026-11-06T10:10:00.000Z" } });
        assert!(!due(&card(stopped, "Human test"), at("2026-11-06T11:00:00Z")));
        let bad = json!({ "title": "T", "smallChange": { "state": "waiting", "goesAt": "soon" } });
        assert!(!due(&card(bad, "Human test"), at("2026-11-06T11:00:00Z")));
    }

    #[test]
    fn the_ship_session_is_told_to_push_exactly_the_checked_commit() {
        let went = json!({ "title": "T", "repo": "a/b", "smallChange": { "state": "went on", "head": "abc123", "branch": "main" } });
        let n = ship_note(&card(went.clone(), "Ship it")).unwrap();
        assert!(n.contains("git push origin abc123:main") && n.contains("his approval to ship it with no human test") && n.contains("a/b"), "{n}");
        assert!(ship_note(&card(went, "Human test")).is_none());
        assert!(ship_note(&card(json!({ "title": "T", "smallChange": { "state": "waiting", "head": "abc" } }), "Ship it")).is_none());
        assert!(ship_note(&card(json!({ "title": "T" }), "Ship it")).is_none());
    }

    fn facts(head: &str, branch: &str, clean: bool, tamper: Option<&str>) -> small::Facts {
        small::Facts {
            checkout: "C:/r".into(),
            branch_now: branch.into(),
            head: head.into(),
            status_hash: "h".into(),
            status_clean: clean,
            base: "b".into(),
            commits: vec![],
            files: vec![],
            tamper: tamper.map(str::to_string),
        }
    }

    #[test]
    fn a_released_ship_starts_only_on_the_checked_commit_clean_and_on_its_branch() {
        let e = SmallRepo { repo: "a/b".into(), checkout: "C:/r".into(), branch: "main".into() };
        assert_eq!(ship_start_problem(Some(&e), Some(&Ok(facts("abc", "main", true, None))), "abc"), None);
        let why = |f: Result<small::Facts, String>| ship_start_problem(Some(&e), Some(&f), "abc").unwrap();
        assert!(why(Ok(facts("def", "main", true, None))).contains("not the checked commit abc"));
        assert!(why(Ok(facts("abc", "other", true, None))).contains("on other now, not main"));
        assert!(why(Ok(facts("abc", "", true, None))).contains("on no branch now"));
        assert!(why(Ok(facts("abc", "main", false, None))).contains("aren't committed"));
        assert!(why(Ok(facts("abc", "main", true, Some("refs/replace/x exists")))).contains("can't be trusted"));
        assert!(why(Err("git rev-parse took more than 5 seconds".into())).contains("couldn't be checked"));
        assert!(ship_start_problem(None, None, "abc").unwrap().contains("off the small-change list"));
        // No checked commit on record: never a pass.
        assert!(ship_start_problem(Some(&e), Some(&Ok(facts("", "main", true, None))), "").is_some());
    }

    #[test]
    fn the_push_is_checked_against_the_checked_commit() {
        assert!(push_check(Some(Ok("abc".into())), "main", "abc").unwrap().contains("origin/main is abc"));
        assert!(push_check(Some(Ok("def".into())), "main", "abc").unwrap_err().starts_with("WARNING: after the ship, origin/main is def"));
        assert!(push_check(Some(Err("no origin/main".into())), "main", "abc").unwrap_err().contains("couldn't check the push"));
        assert!(push_check(None, "", "abc").unwrap_err().contains("no longer on the small-change list"));
        assert!(push_check(Some(Ok("".into())), "main", "").is_err());
    }

    fn git(dir: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git").current_dir(dir).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"]).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// The whole path on a real repo: the verdicts are placed with `snapshot`, and `decide` on
    /// `git_facts` passes; one more commit after them, and it stops.
    #[test]
    fn verdicts_placed_by_snapshot_pass_decide_until_the_work_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let (origin, work) = (tmp.path().join("origin.git"), tmp.path().join("work"));
        git(tmp.path(), &["init", "--bare", "-b", "main", origin.to_str().unwrap()]);
        git(tmp.path(), &["clone", origin.to_str().unwrap(), work.to_str().unwrap()]);
        std::fs::write(work.join("README.md"), "# Title\n\nSome text.\n").unwrap();
        git(&work, &["add", "."]);
        git(&work, &["commit", "-m", "start"]);
        git(&work, &["push", "origin", "HEAD:main"]);
        std::fs::write(work.join("README.md"), "# Title\n\nSome better text.\n").unwrap();
        git(&work, &["commit", "-am", "Fix the wording (card #9)"]);
        let co = work.display().to_string();
        let snap = small::snapshot(&co).unwrap();
        let verdict = |scope: Option<&str>| {
            let mut v = json!({ "state": "passed", "agentId": "a1", "tools": 4, "checkout": co, "head": snap.head, "statusHash": snap.status_hash });
            if let Some(s) = scope {
                v["scope"] = json!(s);
            }
            v
        };
        let c = card(json!({ "title": "T", "repo": "a/b", "test": verdict(None), "review": verdict(Some("full")) }), "Human test");
        let e = SmallRepo { repo: "a/b".into(), checkout: co.clone(), branch: "main".into() };
        let ev = check(&c, &e, false).unwrap();
        assert_eq!(ev["head"], json!(snap.head));
        assert_eq!(ev["files"][0]["path"], "README.md");
        // The race seen on the test app: the app's own writes straight after the move (changes
        // saved, the diff check) bump the card's version. Decided on the card as it is now, it
        // still passes; a change that matters (Hugh's opt-out) is seen.
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path().join("b.db")).unwrap();
        s.set_page_setting(kanban_store::SMALL_REPOS_KEY, &json!([{ "repo": "a/b", "checkout": co, "branch": "main" }]).to_string()).unwrap();
        // The commits name card #9, so make it the ninth card.
        for _ in 0..8 {
            s.create(None, json!({ "title": "filler" }).as_object().cloned().unwrap(), "t").unwrap();
        }
        let made = s.create(None, c.fields.clone(), "t").unwrap();
        assert_eq!(made.number, 9);
        let mut p = Patch::new();
        p.insert("column".into(), json!("Human test"));
        let made = s.update(&made.id, made.version, &p, "t").unwrap();
        let facts = small::git_facts(&co, "main");
        // #108's changesSaved at once on entry, then #103's checkSize, both while git was read.
        let mut p = Patch::new();
        p.insert("changesSaved".into(), json!({ "at": "2026-11-06T10:00:00.000Z" }));
        let one = s.update(&made.id, made.version, &p, "the app").unwrap();
        let mut p = Patch::new();
        p.insert("checkSize".into(), json!({ "class": "quick" }));
        let bumped = s.update(&made.id, one.version, &p, "the app (diff check)").unwrap();
        assert!(bumped.version >= made.version + 2);
        let entry = s.small_repo_for("a/b").unwrap();
        let (now, verdict) = decide_now(&s, &made.id, &entry, &facts).unwrap();
        assert_eq!(now.version, bumped.version);
        assert!(verdict.is_ok(), "{verdict:?}");
        let mut p = Patch::new();
        p.insert(SMALL_OPT_OUT_KEY.into(), json!(true));
        s.update(&made.id, bumped.version, &p, "Hugh (board)").unwrap();
        assert!(decide_now(&s, &made.id, &entry, &facts).is_none(), "opted out meanwhile");
        // A commit after the verdicts: the pass was for older work.
        std::fs::write(work.join("README.md"), "# Title\n\nSome better text, again.\n").unwrap();
        git(&work, &["commit", "-am", "More (card #9)"]);
        assert!(check(&c, &e, false).is_err());
    }

    #[test]
    fn css_needs_an_after_shot_from_the_tester_s_passing_run() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path().join("b.db")).unwrap();
        let test = json!({ "state": "passed", "agentId": "a1", "at": "2026-11-06T10:00:00.000Z" });
        let shot = |run: &str, kind: &str| json!([{ "run": run, "at": "2026-11-06T10:00:00.000Z", "shots": [{ "kind": kind }] }]);
        assert!(!css_evidence(&s, &card(json!({ "title": "T", "test": test }), "Human test")));
        assert!(css_evidence(&s, &card(json!({ "title": "T", "test": test, "evidence": shot("a1", "after") }), "Human test")));
        assert!(!css_evidence(&s, &card(json!({ "title": "T", "test": test, "evidence": shot("a0", "after") }), "Human test")), "an older run's shot");
        assert!(!css_evidence(&s, &card(json!({ "title": "T", "test": test, "evidence": shot("a1", "before") }), "Human test")), "no after shot");
    }

    #[test]
    fn a_record_that_looks_like_it_holds_a_secret_stops_without_quoting_it() {
        let ok = json!({ "commits": [{ "sha": "abc", "subject": "Fix the wording (card #9)" }] });
        assert!(clean_record(ok).is_ok());
        let link = format!("Fix ({}{}=abc)", "c", "t");
        let bad = json!({ "commits": [{ "sha": "abc", "subject": link }] });
        let why = clean_record(bad).unwrap_err();
        assert!(why.contains("looks like it holds a secret") && !why.contains("abc"), "{why}");
    }

    #[test]
    fn the_log_names_the_files_and_lines() {
        let ev = json!({ "lines": 3, "files": [{ "path": "README.md", "kind": "docs", "added": 2, "removed": 1 }] });
        assert_eq!(summary(&ev), "3 lines in README.md (docs, +2 -1)");
    }
}
