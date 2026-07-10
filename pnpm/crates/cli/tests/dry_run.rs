//! `--dry-run` coverage for `pacquet install`.
//!
//! `pacquet install --dry-run` runs a full resolution and reports what a
//! real install would change, but writes nothing to disk (no
//! `pnpm-lock.yaml`, no `node_modules`) and exits 0 regardless of whether
//! changes were found. Mirrors pnpm's `install --dry-run` (pnpm/pnpm#7340).

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

/// A fresh `pacquet` command rooted at `workspace`.
fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

/// On a fresh project (no lockfile), `--dry-run` reports the dependencies a
/// real install would add and writes nothing: no `pnpm-lock.yaml`, no
/// `node_modules`.
#[test]
fn dry_run_reports_changes_without_writing() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "is-positive": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");

    let output = pacquet.with_args(["install", "--dry-run"]).output().expect("spawn pacquet");
    assert!(
        output.status.success(),
        "--dry-run must exit 0 (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("is-positive"),
        "the report must name the dependency a real install would add; got:\n{stdout}",
    );

    assert!(!workspace.join("pnpm-lock.yaml").exists(), "--dry-run must not write pnpm-lock.yaml");
    assert!(!workspace.join("node_modules").exists(), "--dry-run must not create node_modules");

    drop((root, mock_instance));
}

/// Against an existing lockfile, `--dry-run` reports the new dependency a
/// real install would add and leaves the lockfile byte-for-byte unchanged.
#[test]
fn dry_run_reports_added_dependency_without_touching_the_lockfile() {
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
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile_before = fs::read_to_string(&lockfile_path).expect("read seeded lockfile");

    fs::write(
        &manifest_path,
        serde_json::json!({ "dependencies": { "is-positive": "1.0.0", "is-negative": "1.0.0" } })
            .to_string(),
    )
    .expect("rewrite package.json");

    let output =
        pacquet_at(&workspace).with_args(["install", "--dry-run"]).output().expect("spawn pacquet");
    assert!(
        output.status.success(),
        "--dry-run must exit 0 even when the lockfile is stale (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("is-negative"),
        "the report must name the would-be-added dependency; got:\n{stdout}",
    );

    let lockfile_after = fs::read_to_string(&lockfile_path).expect("read lockfile after --dry-run");
    assert_eq!(lockfile_before, lockfile_after, "--dry-run must not rewrite pnpm-lock.yaml");
    assert!(!workspace.join("node_modules").exists(), "--dry-run must not create node_modules");

    drop((root, mock_instance));
}

/// When the lockfile is already up to date, `--dry-run` reports no changes
/// and still exits 0.
#[test]
fn dry_run_reports_no_changes_when_up_to_date() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "is-positive": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();

    let output =
        pacquet_at(&workspace).with_args(["install", "--dry-run"]).output().expect("spawn pacquet");
    assert!(
        output.status.success(),
        "--dry-run must exit 0 (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("up to date"),
        "the report must say the lockfile is up to date; got:\n{stdout}",
    );

    drop((root, mock_instance));
}

/// `--dry-run` is rejected when a pnpr server is configured: that path
/// resolves and links through the server, so it can't honor the no-write
/// contract. Mirrors pnpm's `CONFIG_CONFLICT_DRY_RUN_WITH_PNPR_SERVER`.
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
        .output()
        .expect("spawn pacquet");
    assert!(
        !output.status.success(),
        "--dry-run with a pnpr server must fail (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Cannot use --dry-run with a configured pnpr server"),
        "stderr must name the dry-run/pnpr conflict; got:\n{stderr}",
    );

    drop((root, mock_instance));
}
