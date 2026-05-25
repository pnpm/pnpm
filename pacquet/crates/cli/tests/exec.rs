use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::fs;

#[cfg(unix)]
use assert_cmd::prelude::*;

/// `pacquet exec <bin>` resolves the command against the project's
/// `node_modules/.bin` (prepended to PATH by `makeEnv`) and runs it in
/// the project directory. Mirrors `pnpm exec`.
#[cfg(unix)]
#[test]
fn exec_resolves_command_from_node_modules_bin() {
    use std::os::unix::fs::PermissionsExt;

    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "test", "version": "0.0.0" }).to_string(),
    )
    .expect("write package.json");

    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create .bin");
    let marker = workspace.join("greeted.txt");
    let bin = bin_dir.join("greet");
    fs::write(&bin, format!("#!/bin/sh\ntouch \"{}\"\n", marker.display())).expect("write bin");
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).expect("chmod bin");

    pacquet.with_arg("exec").with_arg("greet").assert().success();
    assert!(marker.exists(), "the .bin command should have run");

    drop(root);
}

/// A non-existent command surfaces as a failure (pnpm's
/// "Command not found").
#[test]
fn exec_fails_for_unknown_command() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "test", "version": "0.0.0" }).to_string(),
    )
    .expect("write package.json");

    let output = pacquet
        .with_arg("exec")
        .with_arg("definitely-not-a-real-binary-xyz")
        .output()
        .expect("spawn pacquet exec");
    assert!(!output.status.success(), "unknown command must fail");

    drop(root);
}

/// `pacquet exec` with no command is an error.
#[test]
fn exec_requires_a_command() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "test", "version": "0.0.0" }).to_string(),
    )
    .expect("write package.json");

    let output = pacquet.with_arg("exec").output().expect("spawn pacquet exec");
    assert!(!output.status.success(), "missing command must fail");

    drop(root);
}
