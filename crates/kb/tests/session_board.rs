//! Card #118, final-test finding F1: `kb` and `kb mcp` follow `KANBAN_DATA_DIR`, which a test
//! app sets for its card sessions, so a test session's kb (even the live one on PATH) reaches
//! the test board and never the live one.

use std::io::Write;
use std::process::{Command, Stdio};

fn kb() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kb"))
}

#[test]
fn kb_reads_the_board_folder_it_is_given() {
    let dir = tempfile::tempdir().unwrap();
    // No api.json there: kb says it looked in that folder, not in this build's own.
    let out = kb().args(["list"]).env("KANBAN_DATA_DIR", dir.path()).env_remove("KANBAN_SESSION").output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains(&dir.path().display().to_string()), "{err}");

    // A board there, read straight from the file (`--db` defaults to the folder's board.db).
    let api = dir.path().join("api.json");
    std::fs::write(&api, r#"{"port":1,"token":"x"}"#).unwrap();
    let out = kb().args(["list"]).env("KANBAN_DATA_DIR", dir.path()).env_remove("KANBAN_SESSION").output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    // Port 1 never answers: the error is about that app, so kb took the port from this folder.
    assert!(!out.status.success() && err.contains("isn't running"), "{err}");
}

#[test]
fn kb_mcp_reads_the_board_folder_it_is_given() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = kb()
        .arg("mcp")
        .env("KANBAN_DATA_DIR", dir.path())
        .env_remove("KANBAN_SESSION")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let call = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"kanban_list_cards","arguments":{}}}"#;
    writeln!(child.stdin.take().unwrap(), "{call}").unwrap();
    let out = child.wait_with_output().unwrap();
    let all = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(all.contains("api.json") || all.contains("isn't running"), "{all}");
    let shown = dir.path().display().to_string().replace('\\', r"\\");
    assert!(all.contains(&dir.path().display().to_string()) || all.contains(&shown), "{all}");
}
