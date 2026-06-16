//! `--dry-run` coverage for `pacquet install`.
//!
//! `--dry-run` is equivalent to `--frozen-lockfile --lockfile-only`: it
//! validates that the lockfile is up-to-date without installing packages
//! and exits with a non-zero exit code if the lockfile is outdated.
//! Mirrors pnpm's `--dry-run` (pnpm/pnpm#7340).

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

/// Build a `pacquet` command rooted at the given workspace directory.
fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

/// `--dry-run` succeeds when the lockfile is up-to-date: no
/// `node_modules` is created.
#[test]
fn dry_run_succeeds_when_lockfile_is_fresh() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "is-positive": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");

    // Seed a lockfile.
    pacquet.with_args(["install", "--lockfile-only"]).assert().success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile_before = fs::read_to_string(&lockfile_path).expect("read seeded lockfile");

    // --dry-run against the fresh lockfile must succeed.
    pacquet_at(&workspace).with_args(["install", "--dry-run"]).assert().success();

    assert!(
        !workspace.join("node_modules").exists(),
        "node_modules must not be created by --dry-run",
    );

    let lockfile_after = fs::read_to_string(&lockfile_path).expect("read lockfile after --dry-run");
    assert_eq!(
        lockfile_before, lockfile_after,
        "--dry-run must not rewrite pnpm-lock.yaml when it is already fresh",
    );

    drop((root, mock_instance));
}

/// `--dry-run` fails with a non-zero exit code when the lockfile is
/// outdated relative to `package.json`.
#[test]
fn dry_run_rejects_stale_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({ "dependencies": { "is-positive": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();

    // Drift the manifest.
    fs::write(
        &manifest_path,
        serde_json::json!({ "dependencies": { "is-positive": "2.0.0" } }).to_string(),
    )
    .expect("rewrite package.json");

    let output = pacquet_at(&workspace).with_args(["install", "--dry-run"]).assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(
        stderr.contains("outdated_lockfile"),
        "stderr must name the outdated-lockfile diagnostic; got:\n{stderr}",
    );

    drop((root, mock_instance));
}

/// `--dry-run` cannot honor its no-write contract when a pnpr server is
/// configured (the pnpr path resolves through the server and persists the
/// lockfile + store), so the combination is refused up front.
#[test]
fn dry_run_rejects_pnpr_server() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "is-positive": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");

    let output = pacquet
        .with_args(["install", "--dry-run", "--pnpr-server", "http://localhost:1"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(
        stderr.contains("Cannot use --dry-run with a configured pnpr server"),
        "stderr must name the dry-run/pnpr conflict; got:\n{stderr}",
    );

    drop((root, mock_instance));
}
