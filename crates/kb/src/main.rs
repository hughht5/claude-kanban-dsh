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
  kb log <n> "<text>"                  add a log line
  kb move <n> "<column>" [--reason r]  move a card (your own card's move waits for your turn to end)
  kb create --title <t> [--repo r] [--column c] [--summary s] [--template id] [--stack <lead>] [--labels BUG,RCS] [--to Planning|"Human test"]
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
  kb ship <n> --asked-by-hugh          ask Hugh to confirm "Ship #n?" in the app (only when he asked;
                                       from a session outside the app; it never ships by itself)
  kb notify <n> "<text>"               tell Hugh
  kb status                            Claude Code statusLine (reads its JSON on stdin)

card sessions (only from outside a card session):
  kb start <n> [--message m] [--mode new|resume]   start or resume a card's session (queued over the cap)
  kb send <n> "<text>"                 message the session (typed when it is next free)
  kb stop <n>                          stop it, or take a queued start out of the queue
  kb sessions [<n>]                    what is running and queued, or one card's session
  kb read <n> [--last k] [--tools]     the last k messages of the card's conversation
  kb switch <n> [--model m] [--effort e]   change model/effort on the running session
  kb keys <n> <key>...                 press keys in its terminal (enter, up, down, esc, tab, text:yes)
  kb mcp                               an MCP server on stdin/stdout for Claude Code (all of the above as tools)
options:
  --db <path>   board database (default %USERPROFILE%\.claude-kanban-dsh\board.db;
                %USERPROFILE%\.claude-kanban-dsh-test\board.db for the test build)"#;

fn default_db() -> PathBuf {
    kanban_store::data_dir().join("board.db")
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
            ("kanban_update_card", json!({ "card": n, "fields": api::fields(&args[1..])? }))
        }
        "log" => ("kanban_add_log", json!({ "card": num(0).ok_or("log needs a card number")?, "text": args.get(1).ok_or("log needs the text")? })),
        "move" => ("kanban_move_card", json!({ "card": num(0).ok_or("move needs a card number")?, "column": args.get(1).ok_or("move needs a column")?, "reason": opt("--reason") })),
        "create" => {
            let stack = match opt("--stack") {
                Some(v) => Some(v.parse::<i64>().map_err(|_| "--stack needs the lead card's number")?),
                None => None,
            };
            ("kanban_create_card", json!({ "title": opt("--title").ok_or("create needs --title")?, "repo": opt("--repo"), "column": opt("--column"), "summary": opt("--summary"), "stack": stack, "template": opt("--template"), "labels": opt("--labels"), "to": opt("--to") }))
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
        "notify" => ("kanban_notify", json!({ "card": num(0).ok_or("notify needs a card number")?, "text": args.get(1).ok_or("notify needs the text")? })),
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

/// `kb ship <n> --asked-by-hugh`. Refused here without the flag; the app checks it again.
fn ship_args(args: &[String]) -> Result<serde_json::Value, String> {
    let n = args.first().and_then(|a| a.parse::<i64>().ok()).ok_or("ship needs a card number")?;
    if !args.iter().any(|a| a == "--asked-by-hugh") {
        return Err("refused: only Hugh ships a card. Add --asked-by-hugh only when Hugh asked for this ship".into());
    }
    Ok(json!({ "card": n, "asked_by_hugh": true }))
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
    fn t4_6_ship_without_the_flag_is_refused() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(ship_args(&a(&["34"])).unwrap_err().starts_with("refused"));
        assert!(ship_args(&a(&["--asked-by-hugh"])).is_err());
        assert_eq!(ship_args(&a(&["34", "--asked-by-hugh"])).unwrap(), json!({ "card": 34, "asked_by_hugh": true }));
    }
}
