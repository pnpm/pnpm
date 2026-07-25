//! `-w` / `--workspace-root`: run the command on the root workspace
//! project from anywhere inside the workspace.

pub mod _utils;
pub use _utils::*;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// A workspace whose root and one member both carry a manifest, plus the
/// nested directory the command is invoked from.
fn workspace_with_member(workspace: &Path) -> PathBuf {
    fs::write(workspace.join("package.json"), r#"{ "name": "root", "version": "1.0.0" }"#)
        .expect("write the root manifest");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - 'packages/*'\n")
        .expect("write pnpm-workspace.yaml");
    let member = workspace.join("packages/foo");
    fs::create_dir_all(&member).expect("create the member dir");
    fs::write(member.join("package.json"), r#"{ "name": "foo", "version": "1.0.0" }"#)
        .expect("write the member manifest");
    member
}

/// The reported shape of pnpm/pnpm#13031: several packages added to the
/// workspace root from a member directory.
#[test]
fn add_with_workspace_root_saves_to_the_root_manifest() {
    let fixture = WorkspaceFixture::new();
    fixture.write_root_manifest("root", ManifestDeps::default());
    let member = fixture.project("foo", "foo", ManifestDeps::default());

    let output =
        fixture.command_at(&member, ["add", "-D", "is-positive@1.0.0", "is-negative@1.0.0", "-w"]);
    assert!(output.status.success(), "add -w should succeed: {output:?}");

    let root_dev = read_manifest(&fixture.workspace)["devDependencies"].clone();
    assert_eq!(root_dev["is-positive"], "1.0.0");
    assert_eq!(root_dev["is-negative"], "1.0.0");
    assert!(
        read_manifest(&member).get("devDependencies").is_none(),
        "the member the command ran from must keep its manifest untouched",
    );
}

/// `-w` re-anchors the command's directory, so a command that reports
/// where it is running reports the workspace root.
#[test]
fn workspace_root_runs_the_command_on_the_root_project() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let member = workspace_with_member(&workspace);
    let nested = member.join("src/utils");
    fs::create_dir_all(&nested).expect("create the nested dir");

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&nested)
        .with_args(["prefix", "-w"])
        .output()
        .expect("run pnpm prefix -w");
    assert!(output.status.success(), "prefix -w should succeed: {output:?}");

    let expected = dunce::canonicalize(&workspace).expect("canonicalize the workspace");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim_end(), expected.display().to_string());

    drop(root);
}

/// `--global` acts on the globally installed packages, which have no
/// workspace root — pnpm rejects the pair rather than picking one.
#[test]
fn workspace_root_conflicts_with_global() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let member = workspace_with_member(&workspace);

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&member)
        .with_args(["prefix", "-g", "-w"])
        .output()
        .expect("run pnpm prefix -g -w");
    assert!(!output.status.success(), "-g with -w must fail: {output:?}");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_OPTIONS_CONFLICT"),
        "stderr should carry pnpm's conflict code: {stderr}",
    );

    drop(root);
}

#[test]
fn workspace_root_outside_a_workspace_is_an_error() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{ "name": "lonely", "version": "1.0.0" }"#)
        .expect("write the manifest");

    let output = pacquet.with_args(["prefix", "-w"]).output().expect("run pnpm prefix -w");
    assert!(!output.status.success(), "-w outside a workspace must fail: {output:?}");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_NOT_IN_WORKSPACE"),
        "stderr should carry pnpm's not-in-workspace code: {stderr}",
    );

    drop(root);
}
