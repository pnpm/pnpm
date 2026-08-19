//! The cache directory keeps a memo of the last wanted lockfile written
//! for a workspace, and an install that has neither `pnpm-lock.yaml`
//! nor a virtual store to synthesize from restores its resolution from
//! that memo — the same rule, and the same freshness gate, as the
//! existing synthesis from `<virtual_store_dir>/lock.yaml`. A manifest
//! change rejects the memo and falls through to a fresh resolve.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, net::TcpListener, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

/// See `lockfile_resolution_reuse.rs` — an ephemeral port with nothing
/// listening, so any resolution attempt is refused instead of answered.
fn dead_registry_url() -> String {
    let listener =
        TcpListener::bind(("127.0.0.1", 0)).expect("bind an ephemeral port to learn a free one");
    let addr = listener.local_addr().expect("read the ephemeral port");
    drop(listener);
    format!("http://127.0.0.1:{}/", addr.port())
}

/// The dead-registry assertions need verification off, like the
/// neighboring `lockfile_resolution_reuse` tests: without
/// `trustLockfile` the memo-synthesized lockfile is eagerly verified
/// against the registry (the release-age policy), which is its own
/// network traffic and not what these tests measure.
fn trust_lockfile(workspace: &Path) {
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}trustLockfile: true\n"))
        .expect("enable trusted lockfile");
}

fn point_npmrc_at(npmrc_path: &Path, registry: &str) {
    let npmrc = fs::read_to_string(npmrc_path).expect("read .npmrc");
    let npmrc = npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(npmrc_path, format!("registry={registry}\n{npmrc}\n"))
        .expect("rewrite .npmrc registry");
}

#[test]
fn a_deleted_lockfile_and_modules_restore_from_the_cache_memo() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/has-optional-peer-with-peer": "^1.0.0" }
        })
        .to_string(),
    )
    .expect("write package.json");
    trust_lockfile(&workspace);
    pacquet_at(&workspace).with_arg("install").assert().success();
    let fresh_lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read the fresh lockfile");

    // The state a CI cache restore leaves: warm store and cache, but no
    // lockfile and no `node_modules` — and, to prove the memo carries the
    // whole resolution, no registry either.
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("delete the lockfile");
    fs::remove_dir_all(workspace.join("node_modules")).expect("delete node_modules");
    point_npmrc_at(&npmrc_path, &dead_registry_url());

    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "the memo-synthesized install must skip resolution",
    );
    let restored =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read the restored lockfile");
    assert_eq!(restored, fresh_lockfile, "the memo must restore the resolution it recorded");
    assert!(
        workspace.join("node_modules/@pnpm.e2e/has-optional-peer-with-peer").exists(),
        "the tree must be materialized from the restored resolution",
    );

    drop((root, mock_instance));
}

/// Custom resolvers and fetchers shape resolution but are not covered
/// by the lockfile's `pnpmfileChecksum`, so a memo written under one
/// pnpmfile regime can't be attested against another. The memo
/// therefore refuses to answer whenever a pnpmfile is loaded, and the
/// no-lockfile install re-resolves — which the dead registry turns
/// into a failure here.
#[test]
fn a_loaded_pnpmfile_disables_the_memo() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/has-optional-peer-with-peer": "^1.0.0" }
        })
        .to_string(),
    )
    .expect("write package.json");
    trust_lockfile(&workspace);
    pacquet_at(&workspace).with_arg("install").assert().success();

    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("delete the lockfile");
    fs::remove_dir_all(workspace.join("node_modules")).expect("delete node_modules");
    fs::write(workspace.join(".pnpmfile.cjs"), "module.exports = {}\n").expect("write a pnpmfile");
    point_npmrc_at(&npmrc_path, &dead_registry_url());

    pacquet_at(&workspace).with_arg("install").assert().failure();

    drop((root, mock_instance));
}

#[test]
fn a_changed_manifest_rejects_the_memo() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/has-optional-peer-with-peer": "^1.0.0" }
        })
        .to_string(),
    )
    .expect("write package.json");
    trust_lockfile(&workspace);
    pacquet_at(&workspace).with_arg("install").assert().success();

    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("delete the lockfile");
    fs::remove_dir_all(workspace.join("node_modules")).expect("delete node_modules");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/has-optional-peer-with-peer": "^1.0.0",
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0"
            }
        })
        .to_string(),
    )
    .expect("add a dependency");
    point_npmrc_at(&npmrc_path, &dead_registry_url());

    // The memo no longer satisfies the manifest, so the install must try a
    // fresh resolve — which the dead registry turns into a failure. A memo
    // that answered here would have silently installed a stale tree.
    pacquet_at(&workspace).with_arg("install").assert().failure();

    drop((root, mock_instance));
}
