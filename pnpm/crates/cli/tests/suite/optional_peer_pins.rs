use crate::_utils::read_lockfile;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

/// Declares `@pnpm.e2e/peer-c` as an optional peer.
const OPTIONAL_PEER_HOST: &str = "@pnpm.e2e/optional-peer-c-host";

/// Declares `@pnpm.e2e/peer-c` as a required peer.
const REQUIRED_PEER_HOST: &str = "@pnpm.e2e/wants-peer-c-1";

// <https://github.com/pnpm/pnpm/issues/16443>
#[test]
fn optional_peer_follows_the_version_its_provider_moves_to() {
    assert_peer_follows_provider(OPTIONAL_PEER_HOST, "1.0.0", "1.0.1");
}

#[test]
fn optional_peer_follows_its_provider_down_to_an_older_version() {
    assert_peer_follows_provider(OPTIONAL_PEER_HOST, "1.0.1", "1.0.0");
}

#[test]
fn required_peer_follows_the_version_its_provider_moves_to() {
    assert_peer_follows_provider(REQUIRED_PEER_HOST, "1.0.0", "1.0.1");
}

// <https://github.com/pnpm/tasks/issues/61>
#[test]
fn required_peer_follows_its_provider_down_to_an_older_version() {
    assert_peer_follows_provider(REQUIRED_PEER_HOST, "1.0.1", "1.0.0");
}

fn assert_peer_follows_provider(host: &str, from: &str, to: &str) {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_json(
        &workspace.join("package.json"),
        &serde_json::json!({
            "dependencies": {
                host: "1.0.0",
                "provider": "file:provider",
            },
        }),
    );
    write_provider(&workspace, from);
    pnpm(&workspace, &["install", "--lockfile-only"]);
    assert_eq!(host_keys(&workspace, host), [format!("{host}@1.0.0(@pnpm.e2e/peer-c@{from})")]);

    write_provider(&workspace, to);
    pnpm(&workspace, &["install", "--lockfile-only"]);
    assert_eq!(host_keys(&workspace, host), [format!("{host}@1.0.0(@pnpm.e2e/peer-c@{to})")]);
    assert_eq!(peer_c_versions(&workspace), [to]);

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let settled = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    pnpm(&workspace, &["install", "--lockfile-only"]);
    let repeated = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(settled == repeated, "a repeat install changed the lockfile:\n{repeated}");

    drop((root, mock_instance));
}

/// A local package that depends on an exact `peer-c`, the way `vue` pins
/// `@vue/server-renderer` in the report.
fn write_provider(workspace: &Path, peer_c_version: &str) {
    let dir = workspace.join("provider");
    fs::create_dir_all(&dir).expect("create provider dir");
    write_json(
        &dir.join("package.json"),
        &serde_json::json!({
            "name": "provider",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/peer-c": peer_c_version },
        }),
    );
}

fn write_json(path: &Path, value: &serde_json::Value) {
    fs::write(path, value.to_string()).expect("write package.json");
}

fn pnpm(workspace: &Path, args: &[&str]) {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_args(args)
        .assert()
        .success();
}

fn host_keys(workspace: &Path, host: &str) -> Vec<String> {
    let prefix = format!("{host}@");
    snapshot_keys(workspace)
        .into_iter()
        .filter(|key| key.starts_with(&prefix))
        .collect()
}

fn peer_c_versions(workspace: &Path) -> Vec<String> {
    snapshot_keys(workspace)
        .into_iter()
        .filter_map(|key| key.strip_prefix("@pnpm.e2e/peer-c@").map(str::to_string))
        .collect()
}

fn snapshot_keys(workspace: &Path) -> Vec<String> {
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    lockfile.snapshots
        .expect("lockfile snapshots")
        .keys()
        .map(ToString::to_string)
        .collect()
}
