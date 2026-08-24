use assert_cmd::prelude::*;
use pnpm_store_dir::{StoreDir, StoreIndex};
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{fs, path::Path};

/// Rewrite the store row for `is-odd@3.0.1` so its bundled manifest
/// names another package — the state a lockfile pairing an integrity
/// with the wrong package, or a registry serving content that doesn't
/// match its metadata, leaves behind.
fn make_the_store_row_hold_another_package(store_dir: &Path) {
    let store_dir = StoreDir::from(store_dir.to_path_buf());
    let index = StoreIndex::open_in(&store_dir).expect("open the store index");
    let key = index
        .keys()
        .expect("read the store index keys")
        .into_iter()
        .find(|key| key.ends_with("\tis-odd@3.0.1"))
        .expect("the install wrote a row for is-odd@3.0.1");
    let mut entry = index.get(&key).expect("read the row").expect("the row exists");
    entry.manifest = Some(serde_json::json!({ "name": "not-is-odd", "version": "3.0.1" }));
    index.set(&key, &entry).expect("rewrite the row");
}

#[test]
fn install_fails_when_the_store_holds_another_package() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet.arg("add").arg("is-odd@3.0.1").assert().success();
    make_the_store_row_hold_another_package(&npmrc_info.store_dir);
    fs::remove_dir_all(workspace.join("node_modules")).expect("drop the materialized modules");

    let mut reinstall = std::process::Command::cargo_bin("pnpm").unwrap();
    reinstall.current_dir(&workspace);
    let output = reinstall.arg("install").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_UNEXPECTED_PKG_CONTENT_IN_STORE"), "{stderr}");
    assert!(stderr.contains("Actual package in the store: not-is-odd@3.0.1."), "{stderr}");
}

#[test]
fn strict_store_pkg_content_check_false_downgrades_the_failure_to_a_warning() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet.arg("add").arg("is-odd@3.0.1").assert().success();
    make_the_store_row_hold_another_package(&npmrc_info.store_dir);
    fs::remove_dir_all(workspace.join("node_modules")).expect("drop the materialized modules");

    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    yaml.push_str("strictStorePkgContentCheck: false\n");
    fs::write(&workspace_yaml, yaml).expect("write pnpm-workspace.yaml");

    let mut reinstall = std::process::Command::cargo_bin("pnpm").unwrap();
    reinstall.current_dir(&workspace);
    let output = reinstall.arg("install").assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(
        stdout.contains(
            "[WARN] Package name or version mismatch found while reading from the store.",
        ),
        "{stdout}",
    );
    assert!(stdout.contains("Actual package in the store: not-is-odd@3.0.1."), "{stdout}");
    assert!(workspace.join("node_modules/is-odd/package.json").exists());
}
