//! Card #137: uninstall takes kb.exe and its kb.old-N.exe copies with it, then the emptied install
//! folder, and does it in NSIS_HOOK_POSTUNINSTALL. Tauri runs PREUNINSTALL before its "is the app
//! running?" check, which can abort; a kb.exe deleted there would leave the app with no `kb`.
//! This reads src-tauri/windows/hooks.nsh, so a move of the clean-up fails here.

use std::path::PathBuf;

fn hooks() -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("windows").join("hooks.nsh");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The body of `!macro <name>` up to its `!macroend`, without comment lines; None if absent.
fn macro_body(text: &str, name: &str) -> Option<Vec<String>> {
    let mut lines = text.lines().map(str::trim);
    lines.find(|l| *l == format!("!macro {name}"))?;
    Some(
        lines
            .take_while(|l| *l != "!macroend")
            .filter(|l| !l.is_empty() && !l.starts_with(';'))
            .map(str::to_string)
            .collect(),
    )
}

#[test]
fn uninstall_deletes_kb_and_its_old_copies_then_the_folder() {
    let body = macro_body(&hooks(), "NSIS_HOOK_POSTUNINSTALL").expect("hooks.nsh has NSIS_HOOK_POSTUNINSTALL");
    let at = |line: &str| body.iter().position(|l| l == line).unwrap_or_else(|| panic!("POSTUNINSTALL lacks `{line}`: {body:#?}"));
    let kb = at(r#"Delete "$INSTDIR\kb.exe""#);
    let old = at(r#"Delete "$INSTDIR\kb.old-*.exe""#);
    let rmdir = at(r#"RMDir "$INSTDIR""#);
    assert!(kb < rmdir && old < rmdir, "the folder is removed after the kb files: {body:#?}");
}

#[test]
fn nothing_deletes_kb_before_the_running_app_check() {
    let text = hooks();
    if let Some(body) = macro_body(&text, "NSIS_HOOK_PREUNINSTALL") {
        // Any file change at all, not only a named kb file: a wildcard Delete or RMDir /r takes
        // kb.exe too.
        let bad: Vec<_> = body
            .iter()
            .filter(|l| {
                let w = l.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
                matches!(w.as_str(), "delete" | "rmdir" | "rename" | "file" | "copyfiles") || l.contains("kb.")
            })
            .collect();
        assert!(bad.is_empty(), "PREUNINSTALL changes files, which would leave an aborted uninstall with no kb: {bad:?}");
    }
}
