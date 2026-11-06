//! Test slots (card #118): `scripts\install.ps1` and `tauri.test.conf.json` name each channel's
//! install folder, exe, identifier, quit event, data folder and DevTools port exactly as
//! `channel.rs` derives them, so the app a slot installs is the app that slot's build expects.

#![cfg_attr(not(windows), allow(unused_imports))]

use kanban_store::{app_name_for, data_dir_for, devtools_port_for, exe_name_for, identifier_for, product_name_for, quit_event_for, slot_of};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

// install.ps1 installs into %USERPROFILE%Programs: the names are Windows ones (card #117: the
// Mac runner has PowerShell but no USERPROFILE). scripts/install.sh names its own.
#[cfg(windows)]
fn names(channel: &str) -> Value {
    let out = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(root().join("scripts").join("install.ps1"))
        .args(["-Channel", channel, "-NamesOnly"])
        .output()
        .expect("run powershell");
    assert!(out.status.success(), "{channel}: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{channel}: {e}: {}", String::from_utf8_lossy(&out.stdout)))
}

#[cfg(windows)]
#[test]
fn install_script_names_each_slot_as_the_app_does() {
    let home = PathBuf::from(std::env::var("USERPROFILE").expect("USERPROFILE"));
    for channel in ["live", "test", "test2", "Test3", "test10"] {
        let n = names(channel);
        let channel = channel.to_lowercase();
        let slot = slot_of(&channel).unwrap();
        let install = home.join("Programs").join(product_name_for(slot));
        assert_eq!(n["channel"], channel.as_str());
        assert_eq!(n["slot"], slot);
        assert_eq!(n["install_dir"], install.display().to_string(), "{channel}");
        assert_eq!(n["app_exe"], install.join(format!("{}.exe", exe_name_for(slot))).display().to_string(), "{channel}");
        assert_eq!(n["quit_event"], quit_event_for(slot), "{channel}");
        assert_eq!(n["data_dir"], data_dir_for(&channel, &home).display().to_string(), "{channel}");
        assert_eq!(n["name"], app_name_for(slot), "{channel}");
        assert_eq!(n["identifier"], identifier_for(slot), "{channel}");
        assert_eq!(n["devtools_port"], devtools_port_for(slot), "{channel}");
    }
}

#[test]
fn install_script_refuses_names_that_are_not_slots() {
    for bad in ["test1", "test11"] {
        let out = Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(root().join("scripts").join("install.ps1"))
            .args(["-Channel", bad, "-NamesOnly"])
            .output()
            .expect("run powershell");
        assert!(!out.status.success(), "{bad} was accepted");
    }
}

#[test]
fn test_config_is_slot_one() {
    let conf: Value = serde_json::from_str(&std::fs::read_to_string(root().join("src-tauri").join("tauri.test.conf.json")).unwrap()).unwrap();
    assert_eq!(conf["productName"], product_name_for(1));
    assert_eq!(conf["mainBinaryName"], exe_name_for(1));
    assert_eq!(conf["identifier"], identifier_for(1));
    assert_eq!(conf["app"]["windows"][0]["title"], app_name_for(1));
    let live: Value = serde_json::from_str(&std::fs::read_to_string(root().join("src-tauri").join("tauri.conf.json")).unwrap()).unwrap();
    assert_eq!(live["identifier"], identifier_for(0));
}
