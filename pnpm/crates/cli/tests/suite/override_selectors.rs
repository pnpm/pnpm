//! Dependency-type pseudo-selectors (`:prod`, `:optional`, `:peer`)
//! and `*` wildcard patterns in `overrides`.

use crate::_utils::read_lockfile;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::{Lockfile, PackageKey, PkgName, PkgNameVerPeer};
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pretty_assertions::assert_eq;
use std::{fs, path::Path};

fn extend_workspace_yaml(workspace: &Path, block: &str) {
    let path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&path).unwrap_or_default();
    if !yaml.is_empty() && !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str(block);
    fs::write(&path, yaml).expect("update pnpm-workspace.yaml");
}

#[test]
fn peer_pseudo_selector_and_package_extensions_replace_peer_with_direct_dependency() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": {
                "@pnpm.e2e/wants-peer-c-1": "1.0.0",
                "@pnpm.e2e/peer-c": "2.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    extend_workspace_yaml(
        &workspace,
        "\
strictPeerDependencies: true
packageExtensions:
  \"@pnpm.e2e/wants-peer-c-1\":
    dependencies:
      \"@pnpm.e2e/peer-c\": \"1.0.0\"
overrides:
  \"@pnpm.e2e/wants-peer-c-1>@pnpm.e2e/peer-c:peer\": \"-\"
",
    );

    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    let lockfile = read_lockfile(&workspace.join(Lockfile::FILE_NAME));
    let root_deps = lockfile.importers["."].dependencies.as_ref().expect("root dependencies");
    let wants_peer_c: PkgName = "@pnpm.e2e/wants-peer-c-1".parse().unwrap();
    let peer_c: PkgName = "@pnpm.e2e/peer-c".parse().unwrap();
    assert_eq!(
        root_deps[&wants_peer_c].version.to_string(),
        "1.0.0",
        "the peer dependency was removed, so the consumer has no peer suffix",
    );
    assert_eq!(
        root_deps[&peer_c].version.to_string(),
        "2.0.0",
        "the root project's direct dependency on @pnpm.e2e/peer-c@2.0.0 is unaffected",
    );

    let pkg_key: PackageKey = "@pnpm.e2e/wants-peer-c-1@1.0.0".parse().unwrap();
    assert!(
        lockfile.packages.as_ref().unwrap()[&pkg_key].peer_dependencies.is_none(),
        "peerDependencies were removed from @pnpm.e2e/wants-peer-c-1@1.0.0",
    );

    let snapshot_key: PkgNameVerPeer = "@pnpm.e2e/wants-peer-c-1@1.0.0".parse().unwrap();
    let snapshot_deps = lockfile.snapshots.as_ref().unwrap()[&snapshot_key]
        .dependencies
        .as_ref()
        .expect("consumer dependencies");
    assert_eq!(
        snapshot_deps[&peer_c].to_string(),
        "1.0.0",
        "@pnpm.e2e/wants-peer-c-1 resolves its own private @pnpm.e2e/peer-c@1.0.0",
    );

    drop((root, mock_instance));
}

#[test]
fn wildcard_peer_override_widens_peer_dependency_range() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": {
                "@pnpm.e2e/wants-peer-c-1": "1.0.0",
                "@pnpm.e2e/peer-c": "2.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    extend_workspace_yaml(
        &workspace,
        "\
strictPeerDependencies: true
overrides:
  \"@pnpm.e2e/*>@pnpm.e2e/peer-*:peer\": \"*\"
",
    );

    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    let lockfile = read_lockfile(&workspace.join(Lockfile::FILE_NAME));
    let root_deps = lockfile.importers["."].dependencies.as_ref().expect("root dependencies");
    let wants_peer_c: PkgName = "@pnpm.e2e/wants-peer-c-1".parse().unwrap();
    assert_eq!(
        root_deps[&wants_peer_c].version.to_string(),
        "1.0.0(@pnpm.e2e/peer-c@2.0.0)",
        "the widened peer dependency resolves to the root's @pnpm.e2e/peer-c@2.0.0",
    );

    drop((root, mock_instance));
}

#[test]
fn optional_pseudo_selector_with_wildcard_removes_optional_and_duplicate_regular_dependencies() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": {
                "@pnpm.e2e/pkg-with-good-optional": "1.0.0",
                "is-positive": "1.0.0",
                "is-negative": "1.0.0",
            },
            "optionalDependencies": {
                "is-positive": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    extend_workspace_yaml(
        &workspace,
        "\
overrides:
  \"is-*:optional\": \"-\"
",
    );

    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    let text = fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read lockfile");
    assert!(
        !text.contains("is-positive@"),
        "optional is-positive (and its duplicate in dependencies) must be removed:\n{text}",
    );
    let lockfile = read_lockfile(&workspace.join(Lockfile::FILE_NAME));
    let root_deps = lockfile.importers["."].dependencies.as_ref().expect("root dependencies");
    assert!(
        root_deps.contains_key(&"is-negative".parse().unwrap()),
        "non-optional is-negative must remain in dependencies",
    );

    drop((root, mock_instance));
}

#[test]
fn prod_pseudo_selector_overrides_only_production_dependencies() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": {
                "@pnpm.e2e/dep-of-pkg-with-1-dep": "^100.0.0",
            },
            "devDependencies": {
                "is-positive": "^3.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    extend_workspace_yaml(
        &workspace,
        "\
overrides:
  \"@pnpm.e2e/dep-of-pkg-with-1-dep:prod\": \"100.0.0\"
  \"is-positive:prod\": \"3.0.0\"
",
    );

    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    let lockfile = read_lockfile(&workspace.join(Lockfile::FILE_NAME));
    let importer = &lockfile.importers["."];
    let prod_dep: PkgName = "@pnpm.e2e/dep-of-pkg-with-1-dep".parse().unwrap();
    let dev_dep: PkgName = "is-positive".parse().unwrap();
    assert_eq!(
        importer.dependencies.as_ref().unwrap()[&prod_dep].version.to_string(),
        "100.0.0",
        "the :prod override pins the production dependency to 100.0.0 instead of 100.1.0",
    );
    assert_eq!(
        importer.dev_dependencies.as_ref().unwrap()[&dev_dep].version.to_string(),
        "3.1.0",
        "the :prod override must not pin a devDependency",
    );

    drop((root, mock_instance));
}
