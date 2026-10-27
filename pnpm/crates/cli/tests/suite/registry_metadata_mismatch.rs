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

#[test]
fn partial_locked_peer_declarations_are_recovered_from_registry() {
    let (_root, workspace, _npmrc_info) = setup("1.0.0");
    run(&workspace, &["install"]);
    let path = workspace.join("pnpm-lock.yaml");
    let text = fs::read_to_string(&path).expect("read lockfile");
    let mut lockfile: serde_json::Value = serde_saphyr::from_str(&text).expect("parse lockfile");
    lockfile["packages"][format!("{WITH_PEERS}@1.0.0")]["peerDependencies"]
        .as_object_mut()
        .expect("peer declarations")
        .remove("@pnpm.e2e/peer-b");
    lockfile["packages"][format!("{WITH_PEERS}@1.0.0")]["peerDependenciesMeta"] =
        json!({ "@pnpm.e2e/peer-b": { "optional": true } });
    fs::write(path, serde_saphyr::to_string(&lockfile).expect("serialize lockfile"))
        .expect("write partial lockfile");

    run(&workspace, &["add", UNRELATED, NO_RELEASE_AGE]);
    assert_records_registry_metadata(&workspace, "install over partial peer declarations");
}

#[test]
fn manifest_hooks_receive_original_peer_ranges_on_repeated_resolution() {
    let (_root, workspace, _npmrc_info) = setup("1.0.0");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"module.exports = { hooks: { readPackage(pkg) {
          if (pkg.name === '@pnpm.e2e/abc-optional-peers') {
            const peers = pkg.peerDependencies;
            peers['@pnpm.e2e/peer-a'] = peers['@pnpm.e2e/peer-a'] === '*' ? '^2.0.0' : '*';
          }
          return pkg;
        } } };",
    )
    .expect("write pnpmfile");

    for args in [&["install", NO_RELEASE_AGE][..], &["add", UNRELATED, NO_RELEASE_AGE]] {
        run(&workspace, args);
        let text = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
        let lockfile: serde_json::Value = serde_saphyr::from_str(&text).expect("parse lockfile");
        assert_eq!(
            lockfile["packages"][format!("{WITH_PEERS}@1.0.0")]["peerDependencies"]["@pnpm.e2e/peer-a"],
            "*",
        );
    }
}
