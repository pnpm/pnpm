use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{ffi::OsStr, fs, path::Path, process::Command};
use tempfile::TempDir;

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const PKG: &str = "@pnpm.e2e/pkg-with-1-dep";

fn setup() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    (root, workspace, npmrc_info)
}

fn pacquet(workspace: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(workspace)
        .with_args(args)
}

fn write_manifest(workspace: &Path, dependencies: &str) {
    let manifest =
        format!(r#"{{ "name": "test-why", "version": "1.0.0", "dependencies": {dependencies} }}"#);
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
}

#[test]
fn why_fails_without_package_name() {
    let (_root, workspace, _anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why"]).output().expect("run pacquet why");
    assert!(!output.status.success(), "why without args should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("requires a package name"),
        "should show error about missing package name: {stderr}",
    );
}

#[test]
fn why_shows_reverse_tree_for_direct_dep() {
    let (_root, workspace, _anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why", PKG]).output().expect("run pacquet why");
    assert!(output.status.success(), "why should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(PKG), "should mention the package: {stdout}");
    assert!(stdout.contains("100.0.0"), "should show the version: {stdout}");
    assert!(stdout.contains("test-why"), "should show the project as a dependent: {stdout}");
}

#[test]
fn why_shows_reverse_tree_for_transitive_dep() {
    let (_root, workspace, _anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why", DEP]).output().expect("run pacquet why");
    assert!(output.status.success(), "why should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(DEP), "should mention the package: {stdout}");
    assert!(stdout.contains(PKG), "should show PKG as a dependent: {stdout}");
    assert!(stdout.contains("test-why"), "should show the project as a dependent: {stdout}");
}

#[test]
fn why_with_glob_pattern() {
    let (_root, workspace, _anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0", "{DEP}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why", "@pnpm.e2e/*"]).output().expect("run pacquet why");
    assert!(output.status.success(), "why with glob should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(PKG), "should mention pkg-with-1-dep: {stdout}");
    assert!(stdout.contains(DEP), "should mention dep-of-pkg-with-1-dep: {stdout}");
}

#[test]
fn why_without_lockfile_returns_empty() {
    let (_root, workspace, _anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));

    let output = pacquet(&workspace, ["why", PKG]).output().expect("run pacquet why");
    assert!(output.status.success(), "why without lockfile should succeed like pnpm: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.is_empty(), "should produce no output without lockfile: {stdout}");
}

#[test]
fn why_depth_limits_output() {
    let (_root, workspace, _anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output_full =
        pacquet(&workspace, ["why", DEP]).output().expect("run pacquet why --depth unset");
    let output_depth1 = pacquet(&workspace, ["why", DEP, "--depth", "1"])
        .output()
        .expect("run pacquet why --depth 1");

    let full_stdout = String::from_utf8_lossy(&output_full.stdout);
    let depth1_stdout = String::from_utf8_lossy(&output_depth1.stdout);
    assert!(full_stdout.contains("test-why"), "full output shows project: {full_stdout}");
    assert!(depth1_stdout.contains(DEP), "depth=1 output still shows the target: {depth1_stdout}");
    assert!(depth1_stdout.contains(PKG), "depth=1 output shows direct parent: {depth1_stdout}");
}
