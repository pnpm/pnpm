//! A registry whose metadata disagrees with the `package.json` in the
//! tarball it serves. The lockfile records the registry metadata, whatever
//! the command and whether the store already holds the tarball.
//! <https://github.com/pnpm/pnpm/issues/16615>

use crate::_utils::set_minimum_release_age;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    command_env::CommandTestExt,
};
use serde_json::json;
use std::{fs, path::Path, process::Command};

/// Its tarball marks `peer-b` and `peer-c` as optional peers.
const WITH_PEERS: &str = "@pnpm.e2e/abc-optional-peers";
const UNRELATED: &str = "@pnpm.e2e/foo@100.0.0";
const NO_RELEASE_AGE: &str = "--config.minimum-release-age=0";

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
}

fn run(workspace: &Path, args: &[&str]) {
    pacquet_at(workspace)
        .with_args(args)
        .assert()
        .success();
}

/// A workspace whose registry drops `peerDependenciesMeta` from the
/// metadata of [`WITH_PEERS`], with `minimumReleaseAge` on.
fn setup(specifier: &str) -> (tempfile::TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry_with_own_storage();
    npmrc_info.remove_version_field(WITH_PEERS, "1.0.0", "peerDependenciesMeta");
    set_minimum_release_age(&workspace, 5760);
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "app",
            "private": true,
            "dependencies": {
                WITH_PEERS: specifier,
                "@pnpm.e2e/peer-a": "1.0.0",
                "@pnpm.e2e/peer-b": "1.0.0",
                "@pnpm.e2e/peer-c": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    (root, workspace, npmrc_info)
}

fn registry_peers(range: &str) -> serde_json::Value {
    json!({
        "@pnpm.e2e/peer-a": range,
        "@pnpm.e2e/peer-b": range,
        "@pnpm.e2e/peer-c": range,
    })
}

/// The lockfile records the registry metadata, so every peer is required,
/// and `pnpm dedupe` has nothing to change.
fn assert_records_registry_metadata(workspace: &Path, step: &str) {
    let text = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    eprintln!("lockfile after {step}:\n{text}");
    let lockfile: serde_json::Value = serde_saphyr::from_str(&text).expect("parse lockfile");
    let package = &lockfile["packages"][format!("{WITH_PEERS}@1.0.0")];
    assert_eq!(package["peerDependencies"], registry_peers("^1.0.0"), "{step}");
    assert_eq!(package.get("peerDependenciesMeta"), None, "{step}");
    let (_, snapshot) = lockfile["snapshots"]
        .as_object()
        .expect("snapshots")
        .iter()
        .find(|(key, _)| key.starts_with(&format!("{WITH_PEERS}@")))
        .expect("snapshot of the package with peers");
    assert_eq!(snapshot, &json!({ "dependencies": registry_peers("1.0.0") }), "{step}");
    for args in [&["dedupe", "--check"][..], &["dedupe", "--check", NO_RELEASE_AGE]] {
        pacquet_at(workspace)
            .with_args(args)
            .assert()
            .success();
    }
}

/// Each step runs with the store holding the tarball, with and without
/// the `minimumReleaseAge` cutoff that decides whether pnpm may read the
/// locked package from the store.
fn assert_every_command_records_registry_metadata(specifier: &str) {
    let (_root, workspace, _npmrc_info) = setup(specifier);
    let steps: [(&str, &[&str]); 7] = [
        ("install", &["install"]),
        ("add without release age", &["add", UNRELATED, NO_RELEASE_AGE]),
        ("dedupe without release age", &["dedupe", NO_RELEASE_AGE]),
        ("install without release age", &["install", NO_RELEASE_AGE]),
        ("dedupe", &["dedupe"]),
        ("update without release age", &["update", "@pnpm.e2e/foo", NO_RELEASE_AGE]),
        ("install --force", &["install", "--force", NO_RELEASE_AGE]),
    ];
    for (step, args) in steps {
        run(&workspace, args);
        assert_records_registry_metadata(&workspace, step);
    }

    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove lockfile");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    run(&workspace, &["install", NO_RELEASE_AGE]);
    assert_records_registry_metadata(&workspace, "install without a lockfile over a warm store");
}

#[test]
fn every_command_records_registry_metadata_for_an_exact_version() {
    assert_every_command_records_registry_metadata("1.0.0");
}

#[test]
fn every_command_records_registry_metadata_for_a_range() {
    assert_every_command_records_registry_metadata("^1.0.0");
}
