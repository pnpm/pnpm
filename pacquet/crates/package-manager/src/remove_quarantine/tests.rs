// Quarantine xattrs only exist on macOS, so these tests (and the helpers they
// call) are scoped to it; the whole module compiles to nothing elsewhere.
#![cfg(target_os = "macos")]

use super::{is_native_binary, remove_quarantine, remove_quarantine_from_native_binaries};
use std::{collections::HashMap, fs, path::Path, process::Command};

const QUARANTINE_ATTR: &str = "com.apple.quarantine";

fn set_quarantine(path: &Path) {
    let status = Command::new("/usr/bin/xattr")
        .arg("-w")
        .arg(QUARANTINE_ATTR)
        .arg("0083;00000000;TestApp;")
        .arg(path)
        .status()
        .expect("spawn xattr -w");
    assert!(status.success(), "failed to set quarantine on {path:?}");
}

fn list_xattrs(path: &Path) -> String {
    let output =
        Command::new("/usr/bin/xattr").arg("-l").arg(path).output().expect("spawn xattr -l");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn has_quarantine(path: &Path) -> bool {
    list_xattrs(path).contains(QUARANTINE_ATTR)
}

#[test]
fn matches_only_native_binary_extensions() {
    assert!(is_native_binary("rollup.darwin-arm64.node"));
    assert!(is_native_binary("libfoo.DYLIB"));
    assert!(is_native_binary("addon.so"));
    assert!(!is_native_binary("index.js"));
    assert!(!is_native_binary("package.json"));
    assert!(!is_native_binary("README"));
    // `.dll` is Windows-only and never relevant on macOS.
    assert!(!is_native_binary("addon.dll"));
}

#[test]
fn removes_quarantine_from_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("addon.node");
    fs::write(&file, b"test content").unwrap();
    set_quarantine(&file);
    assert!(has_quarantine(&file));

    remove_quarantine(std::slice::from_ref(&file));

    assert!(!has_quarantine(&file));
}

#[test]
fn does_nothing_when_quarantine_is_absent() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("addon.node");
    fs::write(&file, b"test content").unwrap();
    assert!(!has_quarantine(&file));

    remove_quarantine(std::slice::from_ref(&file));

    assert!(!has_quarantine(&file));
}

#[test]
fn removes_from_a_batch_while_preserving_other_xattrs() {
    let dir = tempfile::tempdir().unwrap();
    let quarantined = dir.path().join("a.node");
    let clean = dir.path().join("b.node");
    fs::write(&quarantined, b"a").unwrap();
    fs::write(&clean, b"b").unwrap();
    set_quarantine(&quarantined);
    Command::new("/usr/bin/xattr")
        .arg("-w")
        .arg("com.example.custom")
        .arg("keep")
        .arg(&quarantined)
        .status()
        .expect("spawn xattr -w custom");

    remove_quarantine(&[quarantined.clone(), clean]);

    assert!(!has_quarantine(&quarantined));
    assert!(list_xattrs(&quarantined).contains("com.example.custom"));
}

#[test]
fn tolerates_missing_files_in_the_batch() {
    let dir = tempfile::tempdir().unwrap();
    let quarantined = dir.path().join("real.node");
    let missing = dir.path().join("dropped.node");
    fs::write(&quarantined, b"a").unwrap();
    set_quarantine(&quarantined);

    remove_quarantine(&[missing, quarantined.clone()]);

    assert!(!has_quarantine(&quarantined));
}

#[test]
fn sweep_targets_only_native_binaries_under_dir() {
    let dir = tempfile::tempdir().unwrap();
    let native = dir.path().join("lib/addon.node");
    let script = dir.path().join("index.js");
    fs::create_dir_all(native.parent().unwrap()).unwrap();
    fs::write(&native, b"a").unwrap();
    fs::write(&script, b"b").unwrap();
    set_quarantine(&native);
    set_quarantine(&script);

    let cas_paths = HashMap::from([
        ("lib/addon.node".to_string(), native.clone()),
        ("index.js".to_string(), script.clone()),
    ]);
    remove_quarantine_from_native_binaries(dir.path(), &cas_paths);

    assert!(!has_quarantine(&native), "native binary should be unquarantined");
    assert!(has_quarantine(&script), "non-binary files must be left untouched");
}

#[test]
fn empty_list_is_a_noop() {
    remove_quarantine(&[]);
}
