use super::{run_peers, write_linked_chain_project};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, process::Command};

/// In a workspace with autoInstallPeers: false and
/// strictPeerDependencies: true, a linked workspace package's peer dependency
/// is satisfied by dependencies installed in the monorepo root.
#[test]
fn linked_workspace_package_peer_satisfied_by_workspace_root_is_not_reported_missing() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nautoInstallPeers: false\nstrictPeerDependencies: true\n",
    )
    .expect("write workspace manifest");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "devDependencies": { "@pnpm.e2e/foo": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write root manifest");
    write_linked_chain_project(
        &workspace,
        "lib",
        serde_json::json!({ "peerDependencies": { "@pnpm.e2e/foo": "100.0.0" } }),
    );
    write_linked_chain_project(
        &workspace,
        "app",
        serde_json::json!({ "dependencies": { "lib": "workspace:*" } }),
    );

    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["peers", "check", "--lockfile-only"])
        .output()
        .expect("run pnpm peers check");
    assert!(
        output.status.success(),
        "peers check must succeed when root provides peer: {output:?}",
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("No peer dependency issues found"), "stdout:\n{stdout}");

    let issues = run_peers(&workspace, &["peers", "check", "--lockfile-only", "--json"]);
    for (_project, report) in issues.as_object().expect("issues map") {
        assert_eq!(report["missing"].as_object().map(serde_json::Map::len), Some(0));
        assert_eq!(report["bad"].as_object().map(serde_json::Map::len), Some(0));
    }

    drop((root, mock_instance));
}

#[test]
fn linked_workspace_package_peer_incompatible_in_workspace_root_is_reported_unmet() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nautoInstallPeers: false\n",
    )
    .expect("write workspace manifest");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "devDependencies": { "@pnpm.e2e/foo": "2.0.0" },
        })
        .to_string(),
    )
    .expect("write root manifest");
    write_linked_chain_project(
        &workspace,
        "lib",
        serde_json::json!({ "peerDependencies": { "@pnpm.e2e/foo": "100.0.0" } }),
    );
    write_linked_chain_project(
        &workspace,
        "app",
        serde_json::json!({ "dependencies": { "lib": "workspace:*" } }),
    );

    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["peers", "check", "--lockfile-only"])
        .output()
        .expect("run pnpm peers check");
    assert_eq!(
        output.status.code(),
        Some(1),
        "incompatible root peer must be reported: {output:?}",
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("✕ unmet peer @pnpm.e2e/foo"), "stdout:\n{stdout}");
    assert!(stdout.contains("Installed: 2.0.0"), "stdout:\n{stdout}");

    drop((root, mock_instance));
}

#[test]
fn linked_package_peer_not_resolved_from_root_when_setting_is_disabled() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nautoInstallPeers: false\nresolvePeersFromWorkspaceRoot: false\n",
    )
    .expect("write workspace manifest");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "devDependencies": { "@pnpm.e2e/foo": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write root manifest");
    write_linked_chain_project(
        &workspace,
        "lib",
        serde_json::json!({ "peerDependencies": { "@pnpm.e2e/foo": "100.0.0" } }),
    );
    write_linked_chain_project(
        &workspace,
        "app",
        serde_json::json!({ "dependencies": { "lib": "workspace:*" } }),
    );

    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["peers", "check", "--lockfile-only"])
        .output()
        .expect("run pnpm peers check");
    assert_eq!(
        output.status.code(),
        Some(1),
        "root peer must not resolve when disabled: {output:?}",
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("✕ missing peer @pnpm.e2e/foo"), "stdout:\n{stdout}");

    drop((root, mock_instance));
}
