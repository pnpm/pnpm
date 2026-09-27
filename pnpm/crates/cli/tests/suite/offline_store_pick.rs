use crate::_utils::{importer_version, read_lockfile};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    command_env::CommandTestExt,
};
use std::{fs, path::Path, process::Command};

/// Published as 100.0.0 and 100.1.0, so `^100.0.0` has a newer match than
/// the one the store is seeded with.
const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";

fn pnpm_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
}

fn write_manifest(workspace: &Path, dependencies: &serde_json::Value) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "project", "dependencies": dependencies }).to_string(),
    )
    .unwrap();
}

/// Installs `DEP@100.0.0` online, which stores only that tarball while the
/// metadata cache also lists 100.1.0, then removes every project artifact so
/// the next command resolves from scratch.
fn seed_store_with_older_version(workspace: &Path) {
    write_manifest(workspace, &serde_json::json!({ DEP: "100.0.0" }));
    pnpm_at(workspace)
        .with_arg("install")
        .assert()
        .success();
    fs::remove_dir_all(workspace.join("node_modules")).unwrap();
    fs::remove_file(workspace.join("pnpm-lock.yaml")).unwrap();
}

#[test]
fn offline_install_resolves_a_range_to_the_version_in_the_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    seed_store_with_older_version(&workspace);
    write_manifest(&workspace, &serde_json::json!({ DEP: "^100.0.0" }));

    let output = pnpm_at(&workspace)
        .with_args(["install", "--offline"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_version(&lockfile, ".", DEP), "100.0.0");
    drop((root, mock_instance));
}

#[test]
fn offline_add_resolves_a_range_to_the_version_in_the_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    seed_store_with_older_version(&workspace);
    write_manifest(&workspace, &serde_json::json!({}));

    let output = pnpm_at(&workspace)
        .with_args(["add", &format!("{DEP}@^100.0.0"), "--offline"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(workspace.join("package.json")).unwrap()).unwrap();
    assert_eq!(manifest["dependencies"][DEP], "^100.0.0");
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_version(&lockfile, ".", DEP), "100.0.0");
    drop((root, mock_instance));
}
