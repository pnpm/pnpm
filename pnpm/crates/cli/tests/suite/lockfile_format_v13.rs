//! Installing with `lockfile.format: v13`: the lockfile pnpm writes, and a
//! frozen install from it.

use crate::_utils::pacquet_in;
use assert_cmd::assert::OutputAssertExt;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::fs;

#[test]
fn install_writes_the_v13_lockfile_and_a_frozen_install_reads_it() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init().add_mocked_registry();
    fs::write(workspace.join("pnpm-workspace.yaml"), "lockfile:\n  format: v13\n")
        .expect("write pnpm-workspace.yaml");
    let manifest = json!({
        "name": "v13-project",
        "version": "0.0.0",
        "dependencies": {
            "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
        },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");

    pacquet_in(&workspace)
        .with_args(["install", "--reporter=silent"])
        .assert()
        .success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read the lockfile");
    assert!(lockfile.starts_with("{\n\t\"lockfileVersion\": \"13.0\",\n"), "{lockfile}");
    assert!(lockfile.contains("\n\t\t\"@pnpm.e2e/pkg-with-1-dep@100.0.0\": {\n"), "{lockfile}");
    assert!(
        lockfile.contains("\n\t\t\t\t\"@pnpm.e2e/pkg-with-1-dep\": \"100.0.0\",\n"),
        "{lockfile}",
    );
    assert!(!lockfile.contains("snapshots"), "{lockfile}");

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile", "--reporter=silent"])
        .assert()
        .success();

    assert!(workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep/package.json").exists());
    assert_eq!(fs::read_to_string(&lockfile_path).expect("read the lockfile"), lockfile);

    drop(root);
}
