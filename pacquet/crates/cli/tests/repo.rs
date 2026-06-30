//! `pacquet repo` — open the repository URL of a package in the browser.
//!
//! Covers the missing-repository-field error, the missing-package.json error,
//! and the command structure checks. The URL-normalization logic is covered by
//! the unit tests on `repository_to_web_url` / `pick_repo_url`; the full
//! integration path (mock registry + URL open) requires platform-specific
//! browser launcher mocking and follows the `whoami` pattern.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{fs, process::Command};

fn pacquet(workspace: &std::path::Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

#[test]
fn repo_fails_without_package_json() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let output = pacquet(&workspace).with_arg("repo").output().expect("run pacquet repo");

    assert!(!output.status.success(), "repo without package.json should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_NO_REPO_URL")
            && stderr.contains("does not have a repository URL")
            && stderr.contains("to its manifest"),
        "should show no-repo-url error: {stderr}",
    );
    drop(root);
}

#[test]
fn repo_fails_without_repository_field() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name": "test-pkg"}"#).unwrap();
    let output = pacquet(&workspace).with_arg("repo").output().expect("run pacquet repo");

    assert!(!output.status.success(), "repo without repository field should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_NO_REPO_URL")
            && stderr.contains("does not have a repository URL")
            && stderr.contains("to its manifest"),
        "should show no-repo-url error: {stderr}",
    );
    drop(root);
}

#[test]
fn repo_help_succeeds() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let output = pacquet(&workspace)
        .with_args(["repo", "--help"])
        .output()
        .expect("run pacquet repo --help");
    assert!(output.status.success(), "repo --help should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("repository"), "help should mention 'repository': {stdout}");
    drop(root);
}
