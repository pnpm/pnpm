use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use pretty_assertions::assert_eq;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn canonicalize(path: &Path) -> PathBuf {
    dunce::canonicalize(path).expect("canonicalize path")
}

#[test]
fn prefix_prints_the_local_prefix_dir() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{ "name": "root-pkg" }"#)
        .expect("write package.json");

    let output = pacquet.with_args(["prefix"]).output().expect("run pacquet prefix");
    dbg!(&output);
    assert!(output.status.success(), "pacquet prefix should succeed");

    let expected = format!("{}\n", canonicalize(&workspace).display());
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);

    drop(root);
}

#[test]
fn prefix_walks_up_to_find_package_json() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{ "name": "root-pkg" }"#)
        .expect("write package.json");

    let member = workspace.join("sub/dir/deep");
    fs::create_dir_all(&member).expect("create member dir");

    let pacquet_out = Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(&member)
        .with_args(["prefix"])
        .output()
        .expect("run pacquet prefix in the subdir");
    assert!(pacquet_out.status.success(), "pacquet prefix should succeed in the subdir");

    let expected = format!("{}\n", canonicalize(&workspace).display());
    assert_eq!(String::from_utf8_lossy(&pacquet_out.stdout), expected);

    drop(root);
}

#[test]
fn prefix_global_is_not_supported_yet() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet.with_args(["prefix", "-g"]).output().expect("run pacquet prefix -g");
    dbg!(&output);
    assert!(!output.status.success(), "pacquet prefix -g should fail until global support lands");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not supported yet"), "stderr should explain the gap: {stderr}");

    drop(root);
}

#[test]
#[cfg_attr(
    target_os = "windows",
    ignore = "spawns the external `pnpm` shim (`pnpm.cmd`); std::process::Command can't resolve it via PATHEXT"
)]
fn prefix_matches_pnpm_from_a_workspace_subdir() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();

    fs::write(workspace.join("package.json"), r#"{ "name": "wsroot", "version": "1.0.0" }"#)
        .expect("write workspace-root package.json");
    let member = workspace.join("packages/foo");
    fs::create_dir_all(&member).expect("create workspace member dir");
    fs::write(member.join("package.json"), r#"{ "name": "foo", "version": "1.0.0" }"#)
        .expect("write member package.json");

    let sub_member = member.join("src/utils");
    fs::create_dir_all(&sub_member).expect("create sub_member dir");

    let pacquet_out = Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(&sub_member)
        .with_args(["prefix"])
        .output()
        .expect("run pacquet prefix in the subdir");
    assert!(pacquet_out.status.success(), "pacquet prefix should succeed in the subdir");

    let pnpm_out = Command::new("pnpm")
        .with_current_dir(&sub_member)
        .with_args(["prefix"])
        .output()
        .expect("run pnpm prefix in the subdir");
    assert!(
        pnpm_out.status.success(),
        "pnpm prefix failed: {}",
        String::from_utf8_lossy(&pnpm_out.stderr),
    );

    let pacquet_stdout = String::from_utf8_lossy(&pacquet_out.stdout);
    let pnpm_stdout = String::from_utf8_lossy(&pnpm_out.stdout);
    eprintln!("pacquet: {pacquet_stdout:?}\npnpm:    {pnpm_stdout:?}");
    assert_eq!(
        pacquet_stdout, pnpm_stdout,
        "pacquet prefix must match pnpm prefix from a workspace subdir",
    );

    drop(root);
}
