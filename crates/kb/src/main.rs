//! `kb`: the board's command line. `import`, `check` and `export-list` work on the
//! database directly, with the app closed. Everything else goes through the running
//! app, which stays the only writer.

mod api;

use kanban_store::Store;
use serde_json::json;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = r#"usage:
  kb import <export dir> [--replace]   import the artifact's cards (one JSON file per card)
  kb check <export dir>                compare the board with an export, field by field
  kb local-list [--archived]           list cards from the database (app closed)
  kb copy-board --to <path> [--from <path>] [--replace]
                                       a consistent snapshot of a board (safe while its app runs);
                                       --from defaults to the live board, whichever build kb is;
                                       an existing --to is refused without --replace

through the running app (a card session's own card when <n> is left out):
  kb card [<n>]                        show a card as JSON
  kb list [--column <c>] [--archived]  list cards
  kb update <n> --<field> <value> ...  change fields (summary, nextStep, pr, branch, logins, ...)
                                       --labels BUG,RCS sets the card's labels; --labels null clears them
                                       --doneWhen "it shows" --doneWhenChecks '["one check","another"]'
                                       --blockedBy 12,40 sets the cards it waits on; --blockedBy null clears them
                                       the card's flow, only from outside a card session (Hugh's own):
                                       --skip-planning on|off|default  --skip-human-test on|off|default
                                       --skip-plan-review on|off|default (off: a finished plan waits in
                                       Plan review for Hugh to approve)
                                       --subagent critic=on|off|default (router, critic, reproducer,
                                       reviewer, tester; give it once for each)
  kb flow-defaults [--skip-planning on|off] [--skip-plan-review on|off] [--skip-human-test on|off]
                                       the board's flow defaults (Settings > Sessions); with flags, set
                                       them as Hugh's Settings toggle would (outside a card session only)
  kb log <n> "<text>"                  add a log line
  kb move <n> "<column>" [--reason r]  move a card (your own card's move waits for your turn to end)
  kb move <n> "Ship it" --for-hugh "<his words>"
                                       only when Hugh asked you to ship it for him: asks him to confirm
                                       "Ship #n?" in the app; only his click on Ship moves it (10 minutes)
  kb create --title <t> [--repo r] [--column c] [--summary s] [--template id] [--stack <lead>] [--labels BUG,RCS] [--to Planning|"Human test"] [--skip-planning]
                                       --skip-planning: its flow skips Planning; it starts now in In progress
                                       --stack: create it straight into that card's stack
  kb stack [<lead>]                    list a stack: its lead and its cards in order
  kb stack <lead> add <n>              put card n into the stack led by <lead>
  kb stack <lead> remove <n>           take card n out: an ordinary card again
  kb stack <lead> join <n>             as Hugh's drag: add n and brief the stack's session
                                       (plan and build it, or merge its branch if it's built)
  kb stack <lead> detach <n>           as Hugh's Detach: the stack stops building n; n goes to
                                       Planning as its own card and its own session starts
  kb passed <n> [--off]                mark a stack's card passed (or take it back)
  kb ship-result <n> <state> [--detail d]   shipping | shipped | failed | needs-hugh
  kb ship <n> --asked-by-hugh [--reason r]   the same request as move --for-hugh (only when he
                                       asked; it never ships by itself)
  kb notify <n> "<text>"               tell Hugh
  kb testapp                           the test apps: each one's card, turn, line and install
  kb testapp take                      a card session: its own card's test app (Test, Test 2, ...) and a
                                       tester turn on it (In progress or Human test)
  kb testapp take --install [--owner o] [--label "wave 3"] [--slot N] [--wait]
                                       outside a card session: an install turn on test app N (default 1,
                                       Test), ahead of every tester; --owner is the KANBAN_LOCK_OWNER
                                       install.ps1 runs with (default $KANBAN_LOCK_OWNER, else hugh);
                                       --wait waits until it is yours
  kb testapp release [--owner o] [--slot N] [--force]   give the turn back; --force (outside only)
                                       releases whoever has it; a card keeps its test app
  kb testapp free --slot N             outside only: take test app N from the card that has it
  kb testapp release-mkdir             outside only: rename the old DSH lock folder aside
  kb status                            Claude Code statusLine (reads its JSON on stdin)
  kb router "<text>"                   send the Router (the board's long-running session) a message
  kb router --replies [n]              print the Router's last n replies (default 1)

card sessions (only from outside a card session):
  kb start <n> [--message m] [--mode new|resume]   start or resume a card's session (queued over the cap)
  kb send <n> "<text>"                 message the session (typed when it is next free)
  kb stop <n>                          stop it, or take a queued start out of the queue
  kb sessions [<n>]                    what is running and queued, or one card's session
  kb read <n> [--last k] [--tools]     the last k messages of the card's conversation
  kb switch <n> [--model m] [--effort e]   change model/effort on the running session
  kb keys <n> <key>...                 press keys in its terminal (enter, up, down, esc, tab, text:yes)
  kb search <words> [--card n] [--limit n]   search every card's session history (the last 90 days)
  kb mcp                               an MCP server on stdin/stdout for Claude Code (all of the above as tools)
options:
  --db <path>   board database (default %USERPROFILE%\.claude-kanban-dsh\board.db;
                %USERPROFILE%\.claude-kanban-dsh-test\board.db for the test build,
                .claude-kanban-dsh-test2\board.db for test 2, and so on; a test app's
                card sessions set KANBAN_DATA_DIR to their own board's folder, which wins)"#;

fn default_db() -> PathBuf {
    data_dir().join("board.db")
}

/// The board's folder: the session's board (`KANBAN_DATA_DIR`, set by a test app for its card
/// sessions) when there is one, else this build's channel's folder.
pub(crate) fn data_dir() -> PathBuf {
    kanban_store::kb_data_dir(std::env::var_os(kanban_store::DATA_DIR_ENV), kanban_store::data_dir)
}

/// The app's name for messages: this build's, or the folder's when a session points kb elsewhere.
pub(crate) fn app_label() -> String {
    match std::env::var_os(kanban_store::DATA_DIR_ENV) {
        Some(v) if !v.is_empty() => format!("board app for {}", PathBuf::from(v).display()),
        _ => kanban_store::app_name().to_string(),
    }
}

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("kb: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(mut args: Vec<String>) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let mut db = default_db();
    if let Some(i) = args.iter().position(|a| a == "--db") {
        if i + 1 >= args.len() {
            return Err("--db needs a path".into());
        }
        db = PathBuf::from(args.remove(i + 1));
        args.remove(i);
    }
    let flag = |args: &mut Vec<String>, name: &str| {
        let found = args.iter().position(|a| a == name);
        if let Some(i) = found {
            args.remove(i);
        }
        found.is_some()
    };
    let Some(cmd) = args.first().cloned() else {
        println!("{USAGE}");
        return Ok(ExitCode::from(2));
    };
    args.remove(0);
    // Hidden macOS helpers (card #117), before the data folder is touched.
    if cmd == "reap" || cmd == "install-lock" {
        return Ok(ExitCode::from(unix_command(&cmd, &args)));
    }
    if let Some(parent) = db.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if cmd == "mcp" {
        api::mcp_bridge();
        return Ok(ExitCode::SUCCESS);
    }
    if let Some(code) = through_app(&cmd, &args)? {
        return Ok(code);
    }
    match cmd.as_str() {
        "import" => {
            let replace = flag(&mut args, "--replace");
            let dir = args.first().ok_or("import needs an export folder")?;
            let mut store = Store::open(&db)?;
            let r = store.import_dir(dir.as_ref(), replace)?;
            println!("imported {}, replaced {}, next card number {}", r.imported, r.replaced, r.next_number);
            let report = store.check_dir(dir.as_ref())?;
            print_check(&report);
            Ok(if report.clean() { ExitCode::SUCCESS } else { ExitCode::from(3) })
        }
        "check" => {
            let dir = args.first().ok_or("check needs an export folder")?;
            let report = Store::open(&db)?.check_dir(dir.as_ref())?;
            print_check(&report);
            Ok(if report.clean() { ExitCode::SUCCESS } else { ExitCode::from(3) })
        }
        "copy-board" => {
            let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
            let to = opt("--to").ok_or("copy-board needs --to <path>")?;
            let from = opt("--from").map(PathBuf::from).unwrap_or_else(kanban_store::live_board);
            let replace = args.iter().any(|a| a == "--replace");
            let r = kanban_store::copy_board(&from, to.as_ref(), replace)?;
            println!("copied {} cards from {} to {}", r.cards, from.display(), to);
            if r.sessions_detached > 0 {
                println!("{} session(s) running on the source are marked suspended in the copy, so the copy never resumes them", r.sessions_detached);
            }
            if let Some(old) = r.moved_aside {
                println!("the board that was there is kept at {}", old.display());
            }
            Ok(ExitCode::SUCCESS)
        }
        "local-list" => {
            let archived = flag(&mut args, "--archived");
            for c in Store::open(&db)?.list(archived)? {
                let tag = if c.archived { " (archived)" } else { "" };
                println!("{:>4}  {:<12} {}{}", c.number, c.column, c.title(), tag);
            }
            Ok(ExitCode::SUCCESS)
        }
        _ => {
            println!("{USAGE}");
            Ok(ExitCode::from(2))
        }
    }
}

fn print_check(r: &kanban_store::CheckReport) {
    println!("cards {}, archived {}, by column {:?}", r.cards, r.archived, r.by_column);
    for id in &r.missing_on_board {
        println!("missing on the board: {id}");
    }
    for id in &r.only_on_board {
        println!("on the board but not in the export: {id}");
    }
    for d in &r.diffs {
        println!("differs: {} {}: export {} / board {}", d.card, d.field, d.export, d.board);
    }
    if !r.kept_unused.is_empty() {
        println!("kept as they were (not used by the app): {:?}", r.kept_unused);
    }
    println!("{}", if r.clean() { "check: no differences" } else { "check: DIFFERENCES FOUND" });
}

/// `kb reap <app-pid> <record-file>` and `kb install-lock --fd <n> --lock-dir <D> ...`, used
/// by the app and install.sh on a Mac. Not in the usage text. The work is in kanban-unix.
fn unix_command(cmd: &str, args: &[String]) -> u8 {
    // Only macOS runs them; elsewhere they are parsed for the error message only.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    enum Job {
        Reap(i32, String),
        Lock(kanban_unix::lock::LockArgs),
    }
    let job = match cmd {
        "reap" => reap_args(args).map(|(pid, file)| Job::Reap(pid, file)),
        _ => kanban_unix::lock::parse_args(args).map(Job::Lock),
    };
    let job = match job {
        Ok(j) => j,
        Err(e) => {
            eprintln!("kb {cmd}: {e}");
            return 2;
        }
    };
    #[cfg(target_os = "macos")]
    {
        let code = match job {
            Job::Reap(pid, file) => kanban_unix::reap::run(pid, std::path::Path::new(&file)),
            Job::Lock(a) => kanban_unix::lock::install_lock(&a),
        };
        u8::try_from(code).unwrap_or(1)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = job;
        eprintln!("kb {cmd}: only on macOS");
        2
    }
}

/// `kb reap`'s arguments: the app's pid and the tracker's record file.
fn reap_args(args: &[String]) -> Result<(i32, String), String> {
    let pid = args.first().and_then(|a| a.parse::<i32>().ok()).filter(|p| *p > 0).ok_or("usage: kb reap <app-pid> <record-file>")?;
    let file = args.get(1).filter(|f| !f.is_empty()).ok_or("usage: kb reap <app-pid> <record-file>")?;
    Ok((pid, file.clone()))
}

/// The commands that go through the running app. `None` means it isn't one of them.
fn through_app(cmd: &str, args: &[String]) -> Result<Option<ExitCode>, Box<dyn std::error::Error>> {
    let num = |i: usize| args.get(i).and_then(|a| a.parse::<i64>().ok());
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let (tool, body) = match cmd {
        "status" => {
            println!("{}", api::status());
            return Ok(Some(ExitCode::SUCCESS));
        }
        "card" => ("kanban_get_card", num(0).map(|n| json!({ "card": n })).unwrap_or(json!({}))),
        "list" => ("kanban_list_cards", json!({ "column": opt("--column"), "include_archived": args.iter().any(|a| a == "--archived") })),
        "update" => {
            let n = num(0).ok_or("update needs a card number")?;
            ("kanban_update_card", json!({ "card": n, "fields": update_fields(&args[1..])? }))
        }
        "log" => ("kanban_add_log", json!({ "card": num(0).ok_or("log needs a card number")?, "text": args.get(1).ok_or("log needs the text")? })),
        "move" => ("kanban_move_card", move_args(args)?),
        "create" => {
            let stack = match opt("--stack") {
                Some(v) => Some(v.parse::<i64>().map_err(|_| "--stack needs the lead card's number")?),
                None => None,
            };
            ("kanban_create_card", json!({ "title": opt("--title").ok_or("create needs --title")?, "repo": opt("--repo"), "column": opt("--column"), "summary": opt("--summary"), "stack": stack, "template": opt("--template"), "labels": opt("--labels"), "to": opt("--to"), "skipPlanning": args.iter().any(|a| a == "--skip-planning") }))
        }
        "stack" => stack_args(args)?,
        "passed" => ("kanban_set_passed", json!({ "card": num(0).ok_or("passed needs a card number")?, "passed": !args.iter().any(|a| a == "--off") })),
        "ship-result" => ("kanban_ship_result", json!({ "card": num(0).ok_or("ship-result needs a card number")?, "state": args.get(1).ok_or("ship-result needs a state")?, "detail": opt("--detail") })),
        "ship" => ("kanban_request_ship", ship_args(args)?),
        "start" => ("kanban_session_start", json!({ "card": num(0).ok_or("start needs a card number")?, "mode": opt("--mode"), "message": opt("--message") })),
        "send" => ("kanban_session_send", json!({ "card": num(0).ok_or("send needs a card number")?, "text": args.get(1).ok_or("send needs the text")? })),
        "stop" => ("kanban_session_stop", json!({ "card": num(0).ok_or("stop needs a card number")? })),
        "sessions" => ("kanban_session_status", json!({ "card": num(0) })),
        "read" => ("kanban_session_read", json!({ "card": num(0).ok_or("read needs a card number")?, "last": opt("--last").and_then(|v| v.parse::<i64>().ok()), "include_tools": args.iter().any(|a| a == "--tools").then_some(true) })),
        "switch" => ("kanban_session_switch", json!({ "card": num(0).ok_or("switch needs a card number")?, "model": opt("--model"), "effort": opt("--effort") })),
        "keys" => ("kanban_session_keys", json!({ "card": num(0).ok_or("keys needs a card number")?, "keys": args.iter().skip(1).map(|k| match k.strip_prefix("text:") { Some(t) => json!({ "text": t }), None => json!(k) }).collect::<Vec<_>>() })),
        "testapp" => return testapp(args).map(Some),
        "search" => ("kanban_search", search_args(args)?),
        "flow-defaults" => ("kanban_flow_defaults", flow_default_args(args)?),
        "notify" => ("kanban_notify", json!({ "card": num(0).ok_or("notify needs a card number")?, "text": args.get(1).ok_or("notify needs the text")? })),
        "router" => ("kanban_router", router_args(args)?),
        _ => return Ok(None),
    };
    // Leave out options that weren't given, so the tool sees them as missing.
    let body = match body {
        serde_json::Value::Object(m) => serde_json::Value::Object(m.into_iter().filter(|(_, v)| !v.is_null()).collect()),
        other => other,
    };
    match api::tool(tool, body) {
        Ok(serde_json::Value::String(s)) => println!("{s}"),
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
        Err(e) => {
            eprintln!("kb: {e}");
            return Ok(Some(ExitCode::from(1)));
        }
    }
    Ok(Some(ExitCode::SUCCESS))
}

/// The args of `kb router ...` (card #119): `kb router "<text>"` sends the Router a message,
/// `kb router --replies [n]` prints its last n replies (1 by default).
fn router_args(args: &[String]) -> Result<serde_json::Value, String> {
    if let Some(i) = args.iter().position(|a| a == "--replies") {
        let n = match args.get(i + 1) {
            Some(v) => v.parse::<u64>().map_err(|_| "--replies takes a number")?,
            None => 1,
        };
        return Ok(json!({ "replies": n }));
    }
    let text = args.iter().filter(|a| !a.starts_with("--")).cloned().collect::<Vec<_>>().join(" ");
    if text.trim().is_empty() {
        return Err("usage: kb router \"<text>\" or kb router --replies [n]".into());
    }
    Ok(json!({ "text": text }))
}

/// The args of `kb testapp ...` as the kanban_test_app tool's input.
fn testapp_args(args: &[String]) -> Result<serde_json::Value, String> {
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let has = |name: &str| args.iter().any(|a| a == name);
    let owner = || opt("--owner").or_else(|| std::env::var("KANBAN_LOCK_OWNER").ok()).filter(|o| !o.trim().is_empty());
    // --slot N: the test app (card #147), 1 to 10.
    let slot = match opt("--slot") {
        None => None,
        Some(s) => match s.trim().parse::<u32>() {
            Ok(n) if (1..=10).contains(&n) => Some(n),
            _ => return Err(format!("--slot is a test app's number, 1 to 10, not {s}")),
        },
    };
    let with_slot = |mut v: serde_json::Value| {
        if let Some(n) = slot {
            v["slot"] = json!(n);
        }
        v
    };
    match args.first().map(String::as_str) {
        None | Some("status") => Ok(json!({ "action": "status" })),
        Some("take") if has("--install") => Ok(with_slot(
            json!({ "action": "take", "install": true, "owner": owner().unwrap_or_else(|| "hugh".into()), "label": opt("--label").unwrap_or_default() }),
        )),
        Some("take") => Ok(json!({ "action": "take" })),
        Some("release") => {
            let mut v = json!({ "action": "release", "force": has("--force") });
            if let Some(o) = owner() {
                v["owner"] = json!(o);
            }
            Ok(with_slot(v))
        }
        Some("free") if slot.is_some() => Ok(with_slot(json!({ "action": "free" }))),
        Some("free") => Err("kb testapp free --slot N: name the test app to free".into()),
        Some("release-mkdir") => Ok(json!({ "action": "release-mkdir" })),
        Some(other) => Err(format!("kb testapp [status] | take [--install ...] [--slot N] [--wait] | release [--slot N] [--force] | free --slot N | release-mkdir, not {other}")),
    }
}

/// The holder of test app `slot`'s turn in a status: from its entry in `slots` (card #147), or
/// the old top-level `holder` from an app without that change.
fn status_holder(st: &serde_json::Value, slot: u64) -> serde_json::Value {
    match st.get("slots").and_then(serde_json::Value::as_array) {
        Some(slots) => slots.iter().find(|s| s["slot"].as_u64() == Some(slot)).map(|s| s["holder"].clone()).unwrap_or(serde_json::Value::Null),
        None => st.get("holder").cloned().unwrap_or(serde_json::Value::Null),
    }
}

/// `kb testapp ...`. With --wait, asks again every 10 s until the turn is the asker's.
fn testapp(args: &[String]) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let body = testapp_args(args)?;
    let wait = args.iter().any(|a| a == "--wait") && body["action"] == "take";
    let started = std::time::Instant::now();
    let mut last = String::new();
    loop {
        let r = match api::tool("kanban_test_app", body.clone()) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("kb: {e}");
                return Ok(ExitCode::from(1));
            }
        };
        let text = match &r {
            serde_json::Value::String(s) => s.clone(),
            v => serde_json::to_string_pretty(v)?,
        };
        // Whose turn it is comes from the status, not from the reply's wording.
        let yours = !wait || {
            let st = api::tool("kanban_test_app", json!({ "action": "status" })).unwrap_or_default();
            let h = &status_holder(&st, body.get("slot").and_then(serde_json::Value::as_u64).unwrap_or(1));
            !h.is_null() && h["kind"] == body.get("install").map_or("tester", |_| "install") && body.get("owner").map_or(true, |o| h["owner"] == *o)
        };
        if !wait || yours {
            println!("{text}");
            return Ok(ExitCode::SUCCESS);
        }
        if text != last {
            println!("{text}");
            last = text;
        }
        if started.elapsed() > std::time::Duration::from_secs(4 * 60 * 60) {
            eprintln!("kb: still not your turn after 4 hours");
            return Ok(ExitCode::from(3));
        }
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
}

/// `kb stack [<lead>]`, `kb stack <lead> add <n>`, `kb stack <lead> remove <n>`.
fn stack_args(args: &[String]) -> Result<(&'static str, serde_json::Value), String> {
    let num = |i: usize, what: &str| args.get(i).and_then(|a| a.parse::<i64>().ok()).ok_or(format!("stack needs {what}"));
    match args.get(1).map(String::as_str) {
        None => Ok(("kanban_stack_list", json!({ "stack": args.first().and_then(|a| a.parse::<i64>().ok()) }))),
        Some("add") => Ok(("kanban_stack_add", json!({ "stack": num(0, "the lead card's number")?, "card": num(2, "the card to add")? }))),
        Some("remove") => Ok(("kanban_stack_remove", json!({ "stack": num(0, "the lead card's number")?, "card": num(2, "the card to take out")? }))),
        Some("join") => Ok(("kanban_stack_join", json!({ "stack": num(0, "the lead card's number")?, "card": num(2, "the card to join")? }))),
        Some("detach") => Ok(("kanban_stack_detach", json!({ "stack": num(0, "the lead card's number")?, "card": num(2, "the card to detach")? }))),
        Some(other) => Err(format!("kb stack <lead> add|remove|join|detach <n>, not {other}")),
    }
}

/// `kb move <n> "<column>" [--reason r]`, or `kb move <n> "Ship it" --for-hugh "<his words>"`
/// (card #184): a request Hugh confirms in the app, never a move by itself.
fn move_args(args: &[String]) -> Result<serde_json::Value, String> {
    let n = args.first().and_then(|a| a.parse::<i64>().ok()).ok_or("move needs a card number")?;
    let col = args.get(1).filter(|c| !c.starts_with("--")).ok_or("move needs a column")?;
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).filter(|v| !v.starts_with("--")).cloned();
    if args.iter().any(|a| a == "--for-hugh") {
        if col != "Ship it" {
            return Err("--for-hugh is only for \"Ship it\"".into());
        }
        let words = opt("--for-hugh").or_else(|| opt("--reason")).ok_or("--for-hugh needs Hugh's words: kb move <n> \"Ship it\" --for-hugh \"<what he asked>\"")?;
        return Ok(json!({ "card": n, "column": col, "for_hugh": true, "reason": words }));
    }
    Ok(json!({ "card": n, "column": col, "reason": opt("--reason") }))
}

/// `kb ship <n> --asked-by-hugh`. Refused here without the flag; the app checks it again.
fn ship_args(args: &[String]) -> Result<serde_json::Value, String> {
    let n = args.first().and_then(|a| a.parse::<i64>().ok()).ok_or("ship needs a card number")?;
    if !args.iter().any(|a| a == "--asked-by-hugh") {
        return Err("refused: only Hugh ships a card. Add --asked-by-hugh only when Hugh asked for this ship".into());
    }
    let reason = args.iter().position(|a| a == "--reason").and_then(|i| args.get(i + 1)).cloned();
    Ok(match reason {
        Some(r) => json!({ "card": n, "asked_by_hugh": true, "reason": r }),
        None => json!({ "card": n, "asked_by_hugh": true }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_commands() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(stack_args(&a(&["44"])).unwrap(), ("kanban_stack_list", json!({ "stack": 44 })));
        assert_eq!(stack_args(&a(&[])).unwrap(), ("kanban_stack_list", json!({ "stack": null })));
        assert_eq!(stack_args(&a(&["44", "add", "45"])).unwrap(), ("kanban_stack_add", json!({ "stack": 44, "card": 45 })));
        assert_eq!(stack_args(&a(&["44", "remove", "45"])).unwrap(), ("kanban_stack_remove", json!({ "stack": 44, "card": 45 })));
        assert!(stack_args(&a(&["44", "add"])).is_err());
        assert!(stack_args(&a(&["44", "drop", "45"])).is_err());
    }

    #[test]
    fn done_when_fields_pass_through_update() {
        // Card #105: kb update passes the line as text and the checks as a JSON list.
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let f = api::fields(&a(&["--doneWhen", "it shows", "--doneWhenChecks", r#"["one", {"text": "two", "state": "met"}]"#])).unwrap();
        assert_eq!(f, json!({ "doneWhen": "it shows", "doneWhenChecks": ["one", { "text": "two", "state": "met" }] }));
        assert_eq!(api::fields(&a(&["--doneWhenChecks", "null"])).unwrap(), json!({ "doneWhenChecks": null }));
    }

    #[test]
    fn testapp_commands() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(testapp_args(&a(&[])).unwrap(), json!({ "action": "status" }));
        assert_eq!(testapp_args(&a(&["take"])).unwrap(), json!({ "action": "take" }));
        let t = testapp_args(&a(&["take", "--install", "--owner", "router", "--label", "wave 3", "--wait"])).unwrap();
        assert_eq!(t, json!({ "action": "take", "install": true, "owner": "router", "label": "wave 3" }));
        assert_eq!(testapp_args(&a(&["release", "--force", "--owner", "router"])).unwrap(), json!({ "action": "release", "force": true, "owner": "router" }));
        assert_eq!(testapp_args(&a(&["release-mkdir"])).unwrap(), json!({ "action": "release-mkdir" }));
        assert!(testapp_args(&a(&["grab"])).is_err());
        // Card #147: a test app by number.
        let t = testapp_args(&a(&["take", "--install", "--owner", "router", "--slot", "3"])).unwrap();
        assert_eq!(t["slot"], 3);
        assert_eq!(testapp_args(&a(&["free", "--slot", "2"])).unwrap(), json!({ "action": "free", "slot": 2 }));
        assert!(testapp_args(&a(&["free"])).is_err());
        assert!(testapp_args(&a(&["take", "--install", "--slot", "11"])).is_err());
        assert_eq!(testapp_args(&a(&["release", "--slot", "4"])).unwrap()["slot"], 4);
        let st = json!({ "slots": [{ "slot": 2, "holder": { "owner": "router" } }] });
        assert_eq!(status_holder(&st, 2)["owner"], "router");
        assert!(status_holder(&st, 1).is_null());
        assert_eq!(status_holder(&json!({ "holder": { "owner": "x" } }), 1)["owner"], "x", "an older app's status");
    }

    /// Card #162: kb update sets the card's flow with on, off or default.
    #[test]
    fn update_takes_the_cards_flow() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let f = update_fields(&a(&["--skip-planning", "on", "--skip-human-test", "default", "--subagent", "critic=off", "--subagent", "card-tester=On", "--summary", "s"])).unwrap();
        assert_eq!(f, json!({ "skipPlanning": true, "skipHumanTest": null, "subagents": { "critic": false, "tester": true }, "summary": "s" }));
        assert_eq!(update_fields(&a(&["--subagent", "router=default"])).unwrap(), json!({ "subagents": { "router": null } }));
        assert_eq!(update_fields(&a(&["--skip-planning", "off"])).unwrap(), json!({ "skipPlanning": false }));
        // Card #195.
        assert_eq!(update_fields(&a(&["--skip-plan-review", "on"])).unwrap(), json!({ "skipPlanReview": true }));
        assert_eq!(flow_default_args(&a(&[])).unwrap(), json!({}));
        assert_eq!(flow_default_args(&a(&["--skip-plan-review", "on", "--skip-planning", "off"])).unwrap(), json!({ "set": { "skipPlanReview": true, "skipPlanning": false } }));
        assert!(flow_default_args(&a(&["--skip-plan-review", "default"])).is_err());
        assert!(flow_default_args(&a(&["--critic", "on"])).is_err());
        assert!(USAGE.contains("kb flow-defaults") && USAGE.contains("--skip-plan-review on|off|default"));
        // Card #162: a retired or unknown subagent is refused with a plain message, not dropped.
        let e = update_fields(&a(&["--subagent", "merger=off"])).unwrap_err();
        assert!(e.contains("card-merger is no longer a card subagent") && e.contains("router, critic, reproducer, reviewer, tester"), "{e}");
        assert!(update_fields(&a(&["--subagent", "card-merger=on"])).unwrap_err().contains("no longer"));
        assert!(update_fields(&a(&["--subagent", "planner=off"])).unwrap_err().contains("--subagent planner: no such subagent"));
        assert_eq!(update_fields(&a(&["--subagent", "Card-Reviewer=off"])).unwrap(), json!({ "subagents": { "reviewer": false } }));
        for bad in [&["--skip-planning", "yes"][..], &["--skip-human-test"][..], &["--subagent", "critic"][..], &["--subagent", "critic=maybe"][..]] {
            assert!(update_fields(&a(bad)).is_err(), "{bad:?}");
        }
        assert!(USAGE.contains("--skip-planning on|off|default") && USAGE.contains("--subagent critic=on|off|default"));
    }

    #[test]
    fn usage_names_blocked_by() {
        assert!(USAGE.contains("--blockedBy 12,40"));
        assert!(USAGE.contains("--blockedBy null"));
    }

    #[test]
    fn hidden_unix_commands() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(reap_args(&a(&["4242", "/x/run/pids-4242.json"])).unwrap(), (4242, "/x/run/pids-4242.json".to_string()));
        assert!(reap_args(&a(&["4242"])).is_err());
        assert!(reap_args(&a(&["0", "/f"])).is_err());
        assert!(reap_args(&a(&["x", "/f"])).is_err());
        assert_eq!(unix_command("reap", &a(&[])), 2, "bad arguments");
        assert_eq!(unix_command("install-lock", &a(&["--fd", "9"])), 2, "no --lock-dir");
        assert!(!USAGE.contains("reap") && !USAGE.contains("install-lock"), "hidden from the usage text");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(unix_command("reap", &a(&["4242", "/f"])), 2, "only on macOS");
    }

    #[test]
    fn t4_6_ship_without_the_flag_is_refused() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(ship_args(&a(&["34"])).unwrap_err().starts_with("refused"));
        assert!(ship_args(&a(&["--asked-by-hugh"])).is_err());
        assert_eq!(ship_args(&a(&["34", "--asked-by-hugh"])).unwrap(), json!({ "card": 34, "asked_by_hugh": true }));
        assert_eq!(ship_args(&a(&["34", "--asked-by-hugh", "--reason", "ship it"])).unwrap()["reason"], "ship it");
    }

    /// Card #184: `kb move <n> "Ship it" --for-hugh "<his words>"` asks; plain moves are as before.
    #[test]
    fn move_for_hugh_asks_with_his_words() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(move_args(&a(&["34", "Ship it", "--for-hugh", "ship 34"])).unwrap(), json!({ "card": 34, "column": "Ship it", "for_hugh": true, "reason": "ship 34" }));
        assert!(move_args(&a(&["34", "Ship it", "--for-hugh"])).unwrap_err().contains("Hugh's words"));
        assert!(move_args(&a(&["34", "Done", "--for-hugh", "x"])).unwrap_err().contains("only for"));
        assert_eq!(move_args(&a(&["34", "Done", "--reason", "r"])).unwrap(), json!({ "card": 34, "column": "Done", "reason": "r" }));
        assert!(move_args(&a(&["34"])).is_err());
    }
}

/// `on`, `off` or `default` as a flow option's value (card #162): true, false or null.
fn on_off_default(name: &str, v: Option<&String>) -> Result<serde_json::Value, String> {
    match v.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("on") => Ok(json!(true)),
        Some("off") => Ok(json!(false)),
        Some("default") => Ok(serde_json::Value::Null),
        _ => Err(format!("{name} takes on, off or default")),
    }
}

/// A `--subagent` name (`critic` or `card-critic`, any case) as its card key, or why not: the
/// card-merger is retired (the merge lane ships every card, card #162), and any other name isn't
/// one of the card's subagents. Refused here, so a typo isn't dropped without a word.
fn subagent_key(name: &str) -> Result<String, String> {
    let key = name.trim().to_ascii_lowercase();
    let key = key.trim_start_matches("card-");
    let known: Vec<&str> = kanban_store::SUBAGENTS.iter().map(|s| s.key).collect();
    if known.contains(&key) {
        return Ok(key.to_string());
    }
    if kanban_store::RETIRED_SUBAGENTS.contains(&key) {
        return Err(format!("--subagent {}: the card-{key} is no longer a card subagent (the merge lane merges every ship). Use one of: {}", name.trim(), known.join(", ")));
    }
    Err(format!("--subagent {}: no such subagent. Use one of: {}", name.trim(), known.join(", ")))
}

/// `kb update`'s fields: `--<field> <value>` pairs (see `api::fields`), and the card's flow
/// (card #162), Hugh's alone: `--skip-planning on|off|default`, `--skip-human-test
/// on|off|default`, and `--subagent <name>=on|off|default`, which may be given more than once.
fn update_fields(args: &[String]) -> Result<serde_json::Value, String> {
    let mut rest = Vec::new();
    let mut flow = serde_json::Map::new();
    let mut subs = serde_json::Map::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--skip-planning" => {
                flow.insert("skipPlanning".into(), on_off_default("--skip-planning", it.next())?);
            }
            "--skip-human-test" => {
                flow.insert("skipHumanTest".into(), on_off_default("--skip-human-test", it.next())?);
            }
            // Card #195.
            "--skip-plan-review" => {
                flow.insert("skipPlanReview".into(), on_off_default("--skip-plan-review", it.next())?);
            }
            "--subagent" => {
                let v = it.next().ok_or("--subagent needs <name>=on|off|default, like critic=off")?;
                let (name, value) = v.split_once('=').ok_or("--subagent needs <name>=on|off|default, like critic=off")?;
                let key = subagent_key(name)?;
                subs.insert(key, on_off_default("--subagent", Some(&value.to_string()))?);
            }
            _ => rest.push(a.clone()),
        }
    }
    let mut out = match api::fields(&rest)? {
        serde_json::Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    out.extend(flow);
    if !subs.is_empty() {
        out.insert("subagents".into(), serde_json::Value::Object(subs));
    }
    Ok(serde_json::Value::Object(out))
}

/// `kb flow-defaults`'s arguments (card #195): `--skip-planning`, `--skip-plan-review` and
/// `--skip-human-test`, each on or off, as kanban_flow_defaults's `set`. None reads them.
fn flow_default_args(args: &[String]) -> Result<serde_json::Value, String> {
    let mut set = serde_json::Map::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let key = match a.as_str() {
            "--skip-planning" => "skipPlanning",
            "--skip-plan-review" => "skipPlanReview",
            "--skip-human-test" => "skipHumanTest",
            other => return Err(format!("flow-defaults takes --skip-planning, --skip-plan-review and --skip-human-test, not {other}")),
        };
        match on_off_default(a, it.next())? {
            serde_json::Value::Null => return Err(format!("{a} takes on or off")),
            v => set.insert(key.into(), v),
        };
    }
    Ok(if set.is_empty() { json!({}) } else { json!({ "set": set }) })
}

/// `kb search <words> [--card n] [--limit n]`: the words are everything else, in order.
fn search_args(args: &[String]) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut words = Vec::new();
    let (mut card, mut limit) = (None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--card" => card = Some(it.next().and_then(|v| v.parse::<i64>().ok()).ok_or("--card needs a card number")?),
            "--limit" => limit = Some(it.next().and_then(|v| v.parse::<i64>().ok()).ok_or("--limit needs a number")?),
            w => words.push(w.to_string()),
        }
    }
    if words.is_empty() {
        return Err("search needs the words to find".into());
    }
    Ok(json!({ "query": words.join(" "), "card": card, "limit": limit }))
}

#[cfg(test)]
mod search_tests {
    #[test]
    fn search_takes_the_words_and_its_options() {
        let a = |s: &str| s.split(' ').map(str::to_string).collect::<Vec<_>>();
        let v = super::search_args(&a("invoice code --card 34 --limit 50")).unwrap();
        assert_eq!(v, serde_json::json!({ "query": "invoice code", "card": 34, "limit": 50 }));
        assert!(super::search_args(&a("--card 3")).is_err());
        assert!(super::search_args(&a("x --card")).is_err());
    }

    #[test]
    fn kb_router_sends_text_or_reads_replies() {
        let a = |s: &str| s.split(' ').map(str::to_string).collect::<Vec<_>>();
        assert_eq!(super::router_args(&a("what is stuck?")).unwrap(), serde_json::json!({ "text": "what is stuck?" }));
        assert_eq!(super::router_args(&a("--replies")).unwrap(), serde_json::json!({ "replies": 1 }));
        assert_eq!(super::router_args(&a("--replies 3")).unwrap(), serde_json::json!({ "replies": 3 }));
        assert!(super::router_args(&a("--replies x")).is_err());
        assert!(super::router_args(&[]).is_err());
        assert!(super::USAGE.contains("kb router"));
    }
}
