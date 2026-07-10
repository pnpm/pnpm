//! `pacquet docs` / `pacquet home` — open the documentation of a package
//! in the browser.
//!
//! Covers the missing-package-name error and the command structure
//! checks. The browser-opening success path is covered by the unit tests
//! on `is_http_url`; the full integration path (mock registry + URL open)
//! follows the `whoami` pattern but is deferred because it requires a
//! platform-specific browser launcher.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::process::Command;

fn pacquet(workspace: &std::path::Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pacquet binary").with_current_dir(workspace)
}

#[test]
fn docs_fails_without_package_name() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let output = pacquet(&workspace).with_arg("docs").output().expect("run pacquet docs");

    assert!(!output.status.success(), "docs without args should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required arguments were not provided"),
        "should show error about missing package name: {stderr}",
    );
    drop(root);
}

#[test]
fn home_alias_fails_without_package_name() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let output = pacquet(&workspace).with_arg("home").output().expect("run pacquet home");

    assert!(!output.status.success(), "home without args should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required arguments were not provided"),
        "should show error about missing package name: {stderr}",
    );
    drop(root);
}

#[test]
fn aliases_are_recognised() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let docs_output = pacquet(&workspace)
        .with_args(["docs", "--help"])
        .output()
        .expect("run pacquet docs --help");
    assert!(docs_output.status.success(), "docs --help should succeed");

    let home_output = pacquet(&workspace)
        .with_args(["home", "--help"])
        .output()
        .expect("run pacquet home --help");
    assert!(home_output.status.success(), "home --help should succeed");

    drop(root);
}
