//! Removing an `overrides` entry re-resolves the dependencies it pinned,
//! even when the pinned version still satisfies the declared range
//! (<https://github.com/pnpm/pnpm/issues/4587>).

use crate::_utils;

use _utils::{read_lockfile, snapshot_entries};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::{Lockfile, PkgName};
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::bump_mtime,
};
use pretty_assertions::assert_eq;
use std::{fs, path::Path};

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const PARENT: &str = "@pnpm.e2e/pkg-with-1-dep";

fn install_with_overrides(workspace: &Path, base_yaml: &str, overrides: Option<&str>) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = match overrides {
        Some(version) => format!("{base_yaml}\noverrides:\n  \"{DEP}\": {version}\n"),
        None => base_yaml.to_string(),
    };
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
    if workspace.join("node_modules").exists() {
        bump_mtime(&yaml_path);
    }
    std::process::Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_arg("install")
        .assert()
        .success();
}

fn lockfile_text(workspace: &Path) -> String {
    fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read pnpm-lock.yaml")
}

/// The version the parent's snapshot pins for the dependency.
fn transitive_edge(workspace: &Path) -> String {
    let lockfile = read_lockfile(&workspace.join(Lockfile::FILE_NAME));
    let parents = snapshot_entries(&lockfile, PARENT);
    assert_eq!(parents.len(), 1, "one parent snapshot: {parents:?}");
    let dep: PkgName = DEP.parse().expect("parse the dependency name");
    parents[0].1.dependencies
        .as_ref()
        .and_then(|dependencies| dependencies.get(&dep))
        .expect("the parent snapshot depends on the dependency")
        .to_string()
}

#[test]
fn removing_an_override_restores_the_version_a_direct_dependency_pins() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let base_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).unwrap_or_default();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { DEP: "100.0.0", "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    install_with_overrides(&workspace, &base_yaml, Some("100.1.0"));
    assert_eq!(transitive_edge(&workspace), "100.1.0", "the override applies");

    install_with_overrides(&workspace, &base_yaml, None);
    let lockfile = lockfile_text(&workspace);
    assert!(!lockfile.contains("overrides:"), "the override is dropped:\n{lockfile}");
    assert_eq!(
        transitive_edge(&workspace),
        "100.0.0",
        "the transitive edge returns to the direct dependency's version:\n{lockfile}",
    );
    assert!(
        !lockfile.contains(&format!("{DEP}@100.1.0")),
        "the version the override pinned is dropped:\n{lockfile}",
    );

    drop((root, mock_instance));
}

#[test]
fn removing_an_override_resolves_the_version_it_pinned_as_if_never_locked() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let base_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).unwrap_or_default();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" } }).to_string(
        ),
    )
    .expect("write package.json");

    install_with_overrides(&workspace, &base_yaml, Some("100.0.0"));
    assert_eq!(transitive_edge(&workspace), "100.0.0", "the override applies");

    install_with_overrides(&workspace, &base_yaml, None);
    let lockfile = lockfile_text(&workspace);
    assert_eq!(
        transitive_edge(&workspace),
        "100.1.0",
        "the edge resolves the highest version in range:\n{lockfile}",
    );
    assert!(
        !lockfile.contains(&format!("{DEP}@100.0.0")),
        "the version the override pinned is dropped:\n{lockfile}",
    );

    drop((root, mock_instance));
}
