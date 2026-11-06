//! tools/cdp.mjs refuses a shot when CDP_PORT isn't set, so a shot never comes from the live
//! board (port 9333) by accident, and it writes no file (card #101). Passes without running
//! anything when node isn't on PATH.
use std::path::PathBuf;
use std::process::Command;

#[test]
fn shot_without_cdp_port_is_refused_and_writes_no_file() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tools")
        .join("cdp.mjs");
    assert!(script.is_file(), "missing {}", script.display());
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("x.png");

    // CDP_PORT removed, and CDP_PORT empty; with and without a scheme and a selector.
    let cases: [(Option<&str>, &[&str]); 4] = [
        (None, &[]),
        (None, &["dark", ".ev"]),
        (Some(""), &[]),
        (Some(""), &["light"]),
    ];
    for (port, extra) in cases {
        let mut cmd = Command::new("node");
        cmd.arg(&script).arg("shot").arg(&out).args(extra);
        match port {
            None => cmd.env_remove("CDP_PORT"),
            Some(p) => cmd.env("CDP_PORT", p),
        };
        let run = match cmd.output() {
            Ok(o) => o,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("node isn't on PATH: cdp_shot skipped");
                return;
            }
            Err(e) => panic!("couldn't run node: {e}"),
        };
        let err = String::from_utf8_lossy(&run.stderr);
        assert!(
            !run.status.success(),
            "shot ran without CDP_PORT ({port:?}, {extra:?}): {err}"
        );
        assert!(
            err.contains("shot needs CDP_PORT: 9334 for the test app"),
            "unexpected error ({port:?}, {extra:?}): {err}"
        );
        assert!(
            !out.exists(),
            "shot wrote {} without CDP_PORT",
            out.display()
        );
    }
}
