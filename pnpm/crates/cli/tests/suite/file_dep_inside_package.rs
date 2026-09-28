//! A `file:` dependency that a registry package declares, pointing inside
//! that package, is linked to the directory inside the package.
//! `@pnpm.e2e/pkg-with-internal-file-dep` declares
//! `"@pnpm.e2e/internal-child": "file:./child"` and ships `child/` in its
//! tarball, the way `@eslint/css@0.3.0` shipped `typings/css-tree`.
//!
//! Covers <https://github.com/pnpm/pnpm/issues/9141>.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    command_env::CommandTestExt,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const PARENT: &str = "@pnpm.e2e/pkg-with-internal-file-dep";

fn pnpm_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
}

/// Install the fixture, then install again from the lockfile alone, and
/// return the parent package's directory after each install.
fn install_twice(settings: &str) -> (tempfile::TempDir, PathBuf) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { PARENT: "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&yaml_path, yaml.replace("enableGlobalVirtualStore: false\n", settings))
        .expect("write pnpm-workspace.yaml");

    pnpm_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile.contains("'@pnpm.e2e/internal-child': link:<root>/child"),
        "the lockfile records the link into the package:\n{lockfile}",
    );

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pnpm_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    pnpm_at(&workspace)
        .with_args(["install", "--no-prefer-frozen-lockfile"])
        .assert()
        .success();
    let relocked = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert_eq!(relocked, lockfile, "a repeat resolution keeps the lockfile");

    drop(mock_instance);
    (root, workspace)
}

/// The package directory the project's `node_modules` entry resolves to.
fn parent_dir(workspace: &Path) -> PathBuf {
    let entry = pnpm_fs::join_slash_separated_path(&workspace.join("node_modules"), PARENT);
    fs::canonicalize(&entry).unwrap_or_else(|err| panic!("resolve {entry:?}: {err}"))
}

fn assert_links_into(link: &Path, parent: &Path) {
    let target = fs::canonicalize(link).unwrap_or_else(|err| panic!("resolve {link:?}: {err}"));
    assert_eq!(target, parent.join("child"));
    assert!(target.join("index.js").is_file(), "the child's files are in {target:?}");
}

#[test]
fn a_file_dep_inside_a_registry_package_is_linked_in_the_virtual_store() {
    let (root, workspace) = install_twice("enableGlobalVirtualStore: false\n");
    let parent = parent_dir(&workspace);
    assert_links_into(
        &parent
            .parent()
            .unwrap()
            .join("internal-child"),
        &parent,
    );
    drop(root);
}

#[test]
fn a_file_dep_inside_a_registry_package_is_linked_in_the_global_virtual_store() {
    let (root, workspace) = install_twice("enableGlobalVirtualStore: true\n");
    let parent = parent_dir(&workspace);
    assert_links_into(
        &parent
            .parent()
            .unwrap()
            .join("internal-child"),
        &parent,
    );
    drop(root);
}

#[test]
fn a_file_dep_inside_a_registry_package_is_linked_with_the_hoisted_node_linker() {
    let (root, workspace) = install_twice("enableGlobalVirtualStore: false\nnodeLinker: hoisted\n");
    let parent = parent_dir(&workspace);
    assert_links_into(
        &pnpm_fs::join_slash_separated_path(
            &parent.join("node_modules"),
            "@pnpm.e2e/internal-child",
        ),
        &parent,
    );
    drop(root);
}
