//! The test app always has its WebView DevTools port, 9334, so tools/cdp.mjs and the
//! card-tester's shots can reach it however it was started (card #101). It comes from the test
//! channel's own window config; the live app's config never has a debug port.
use serde_json::Value;
use std::path::PathBuf;

fn conf(name: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(name);
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn the_test_app_has_its_debug_port_and_keeps_wrys_defaults() {
    let test = conf("tauri.test.conf.json");
    let windows = test["app"]["windows"].as_array().expect("the test config's windows");
    assert_eq!(windows.len(), 1, "one window, the main one");
    let args = windows[0]["additionalBrowserArgs"].as_str().expect("additionalBrowserArgs on the test window");
    assert!(args.contains("--remote-debugging-port=9334"), "{args}");
    assert!(args.contains("--remote-allow-origins=http://127.0.0.1:9334"), "{args}");
    // Setting the args replaces wry's defaults, so they are passed again.
    assert!(args.contains("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection"), "{args}");
    assert!(!args.contains("9333"), "the test app never takes the live port: {args}");
}

#[test]
fn the_live_app_has_no_debug_port() {
    let live = conf("tauri.conf.json");
    let text = live.to_string();
    assert!(!text.contains("remote-debugging"), "the live config opens a debug port");
    for w in live["app"]["windows"].as_array().into_iter().flatten() {
        assert!(w.get("additionalBrowserArgs").is_none(), "the live window sets browser args: {w}");
    }
}
