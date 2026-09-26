use super::{pacquet_in, write_manifest, write_workspace_yaml};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, process::Command};

#[test]
fn external_virtual_store_packages_resolve_root_dependencies() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_yaml(
        &workspace,
        "enableGlobalVirtualStore: false\nvirtualStoreDir: ../external-store\n",
    );
    write_manifest(
        &workspace,
        serde_json::json!({ "is-positive": "3.1.0", "is-negative": "2.1.0" }),
    );
    pacquet
        .with_arg("install")
        .assert()
        .success();

    let assert_resolution = || {
        Command::new("node")
        .with_current_dir(&workspace)
        .without_env("NODE_PATH")
        .without_env("NODE_OPTIONS")
        .with_args([
            "-e",
            "const { createRequire } = require('node:module'); \
             const { realpathSync } = require('node:fs'); \
             const fromDependency = createRequire(realpathSync('node_modules/is-negative/package.json')); \
             require('node:assert/strict').equal(fromDependency('is-positive')(1), true);",
        ])
        .assert()
        .success()
    };

    assert_resolution();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove project modules");
    fs::remove_dir_all(workspace.join("../external-store")).expect("remove virtual store");
    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_resolution();

    drop((root, mock_instance));
}
