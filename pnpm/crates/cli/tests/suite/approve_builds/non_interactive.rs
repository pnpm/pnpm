#[cfg(unix)]
use super::pacquet_in_terminal;
use super::{INSTALL_MARKER, install_with_ignored_build, pacquet};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::fs;

#[test]
fn approve_builds_without_args_fails_when_stdin_is_not_a_terminal() {
    let (harness, workspace) = install_with_ignored_build();

    let assert = pacquet(&workspace)
        .with_env("CI", "false")
        .with_arg("approve-builds")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_APPROVE_BUILDS_NON_INTERACTIVE"), "stderr:\n{stderr}");
    assert!(!workspace.join(INSTALL_MARKER).exists());

    pacquet(&workspace)
        .with_env("CI", "true")
        .with_args(["approve-builds", "--all"])
        .assert()
        .success();
    assert!(workspace.join(INSTALL_MARKER).exists());

    pacquet(&workspace)
        .with_env("CI", "true")
        .with_arg("approve-builds")
        .assert()
        .success();

    drop(harness);
}

#[cfg(unix)]
#[test]
fn approve_builds_and_approve_reject_ci_even_with_a_controlling_terminal() {
    let (harness, workspace) = install_with_ignored_build();

    for cmd in ["approve-builds", "approve"] {
        let assert = pacquet_in_terminal(&workspace)
            .with_env("CI", "true")
            .with_arg(cmd)
            .assert()
            .failure();
        let output = String::from_utf8_lossy(&assert.get_output().stdout);
        let expected_code = if cmd == "approve" {
            "ERR_PNPM_APPROVE_NON_INTERACTIVE"
        } else {
            "ERR_PNPM_APPROVE_BUILDS_NON_INTERACTIVE"
        };
        assert!(output.contains(expected_code), "{cmd} output:\n{output}");
    }
    assert!(!workspace.join(INSTALL_MARKER).exists());

    pacquet_in_terminal(&workspace)
        .with_env("CI", "true")
        .with_args(["approve", "--all"])
        .assert()
        .success();
    assert!(workspace.join(INSTALL_MARKER).exists());

    pacquet_in_terminal(&workspace)
        .with_env("CI", "true")
        .with_arg("approve")
        .assert()
        .success();

    drop(harness);
}

#[test]
fn approve_named_packages_and_empty_pending_succeed_without_a_terminal() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"app","version":"1.0.0"}"#)
        .expect("write package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "").expect("write pnpm-workspace.yaml");

    pacquet(&workspace)
        .with_env("CI", "true")
        .with_arg("approve")
        .assert()
        .success();

    pacquet(&workspace)
        .with_env("CI", "true")
        .with_args(["approve", "esbuild", "!sharp"])
        .assert()
        .success();
    let yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .expect("read pnpm-workspace.yaml");
    assert!(yaml.contains("esbuild"), "yaml:\n{yaml}");
    assert!(yaml.contains("sharp"), "yaml:\n{yaml}");

    drop(root);
}
