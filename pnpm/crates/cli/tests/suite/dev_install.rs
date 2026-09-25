pub use _utils::*;

use crate::_utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{
    fs,
    path::{Path, PathBuf},
};

// <https://github.com/pnpm/pnpm/issues/9678>

const DEV_DEP: &str = "@pnpm.e2e/pkg-with-good-optional";
const DEV_DEP_OPTIONAL: &str = "is-positive";
const PROD_DEP: &str = "is-negative";
const ROOT_OPTIONAL: &str = "@pnpm.e2e/bravo";

#[test]
fn dev_install_installs_the_optional_dependencies_of_dev_dependencies() {
    let (root, npmrc_info, workspace) = prepare_workspace();

    pacquet_in(&workspace)
        .with_args(["install", "--dev"])
        .assert()
        .success();

    assert_dev_install(&workspace, true);
    drop((root, npmrc_info));
}

#[test]
fn frozen_dev_install_installs_the_optional_dependencies_of_dev_dependencies() {
    let (root, npmrc_info, workspace) = prepare_workspace();
    write_lockfile(&workspace);

    pacquet_in(&workspace)
        .with_args(["install", "--dev", "--frozen-lockfile"])
        .assert()
        .success();

    assert_dev_install(&workspace, true);
    drop((root, npmrc_info));
}

#[test]
fn hoisted_dev_install_installs_the_optional_dependencies_of_dev_dependencies() {
    let (root, npmrc_info, workspace) = prepare_workspace();
    write_lockfile(&workspace);

    pacquet_in(&workspace)
        .with_args(["install", "--dev", "--frozen-lockfile", "--node-linker=hoisted"])
        .assert()
        .success();

    assert_dev_install(&workspace, true);
    drop((root, npmrc_info));
}

#[test]
fn dev_install_with_no_optional_skips_the_optional_dependencies_of_dev_dependencies() {
    let (root, npmrc_info, workspace) = prepare_workspace();
    write_lockfile(&workspace);

    pacquet_in(&workspace)
        .with_args(["install", "--dev", "--frozen-lockfile", "--no-optional"])
        .assert()
        .success();

    assert_dev_install(&workspace, false);
    drop((root, npmrc_info));
}

#[test]
fn fetch_dev_fetches_the_optional_dependencies_of_dev_dependencies() {
    let (root, npmrc_info, workspace) = prepare_workspace();
    write_lockfile(&workspace);

    pacquet_in(&workspace)
        .with_args(["fetch", "--dev"])
        .assert()
        .success();

    let virtual_store = workspace.join("node_modules/.pnpm");
    assert!(virtual_store.join("@pnpm.e2e+pkg-with-good-optional@1.0.0").is_dir());
    assert!(virtual_store.join("is-positive@1.0.0").is_dir());
    assert!(!virtual_store.join("@pnpm.e2e+bravo@1.0.0").exists());
    assert!(!virtual_store.join("is-negative@1.0.0").exists());
    drop((root, npmrc_info));
}

fn prepare_workspace() -> (tempfile::TempDir, AddMockedRegistry, PathBuf) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { PROD_DEP: "1.0.0" },
            "devDependencies": { DEV_DEP: "1.0.0" },
            "optionalDependencies": { ROOT_OPTIONAL: "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    (root, npmrc_info, workspace)
}

fn write_lockfile(workspace: &Path) {
    pacquet_in(workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
}

fn assert_dev_install(workspace: &Path, expect_dev_dep_optional: bool) {
    let dev_dep_dir = fs::canonicalize(workspace.join("node_modules").join(DEV_DEP))
        .expect("the dev dependency is installed");
    assert_eq!(resolves_from(&dev_dep_dir, DEV_DEP_OPTIONAL), expect_dev_dep_optional);
    let modules_dir = workspace.join("node_modules");
    assert!(!modules_dir.join(PROD_DEP).exists());
    assert!(!modules_dir.join(ROOT_OPTIONAL).exists());
    assert!(!modules_dir.join(".pnpm/@pnpm.e2e+bravo@1.0.0").exists());
}

/// Whether Node's module resolution from `dir` finds `name`.
fn resolves_from(dir: &Path, name: &str) -> bool {
    dir.ancestors()
        .any(|ancestor| {
            ancestor
                .join("node_modules")
                .join(name)
                .join("package.json")
                .is_file()
        })
}
