//! The page's own JavaScript tests (card #120): `cargo test` runs `node tools/ui-tests/run.mjs`,
//! which runs every tools/ui-tests/*.test.mjs with node:test on the real ui/*.js files in a vm.
//! Passes with a skip message when node isn't on PATH.
use std::path::PathBuf;
use std::process::Command;

#[test]
fn ui_js_tests_pass() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tools")
        .join("ui-tests")
        .join("run.mjs");
    assert!(script.is_file(), "missing {}", script.display());
    let out = match Command::new("node").arg(&script).output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("node isn't on PATH: the UI tests (tools/ui-tests) were skipped");
            return;
        }
        Err(e) => panic!("couldn't run node: {e}"),
    };
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "the UI tests failed:\n{text}");
}
