use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{fs, process::Command};

#[test]
fn engine_strict_rejects_an_incompatible_root_project() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "incompatible-root",
            "version": "1.0.0",
            "engines": { "node": ">=99.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    fs::write(workspace.join("pnpm-workspace.yaml"), "engineStrict: true\nnodeVersion: 20.0.0\n")
        .expect("write workspace settings");

    let assert = pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(
        stderr.contains("ERR_PNPM_UNSUPPORTED_ENGINE"),
        "stderr must carry pnpm's unsupported-engine code: {stderr}",
    );
    assert!(
        stderr.contains(r#"{"node":">=99.0.0"}"#),
        "stderr must report the root project's Node.js range: {stderr}",
    );
    assert!(
        stderr.contains(r#"{"node":"20.0.0"}"#),
        "stderr must report the configured Node.js version: {stderr}",
    );
    assert!(
        !workspace.join("pnpm-lock.yaml").exists(),
        "the engine check must fail before writing the lockfile",
    );

    drop(root);
}

#[test]
fn engine_strict_accepts_the_active_node_version() {
    let node_output = Command::new("node")
        .arg("--version")
        .output()
        .expect("run node --version");
    assert!(node_output.status.success(), "node --version must succeed");
    let node_version = String::from_utf8(node_output.stdout)
        .expect("decode node --version")
        .trim()
        .trim_start_matches('v')
        .to_string();

    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "compatible-root",
            "version": "1.0.0",
            "engines": { "node": node_version },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "engineStrict: true\n")
        .expect("write workspace settings");

    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    drop(root);
}

#[test]
fn filtered_install_checks_the_workspace_root_with_dedicated_lockfiles() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "incompatible-root",
            "version": "1.0.0",
            "engines": { "node": ">=99.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nengineStrict: true\nnodeVersion: 20.0.0\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write workspace settings");
    let child = workspace.join("packages/child");
    fs::create_dir_all(&child).expect("create child project");
    fs::write(child.join("package.json"), r#"{"name":"child","version":"1.0.0"}"#)
        .expect("write child package.json");

    let assert = pacquet
        .with_args(["--filter", "child", "install", "--lockfile-only"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_UNSUPPORTED_ENGINE"), "stderr: {stderr}");
    assert!(!child.join("pnpm-lock.yaml").exists());

    drop(root);
}

#[test]
fn non_install_commands_do_not_check_the_root_engine() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "incompatible-root",
            "version": "1.0.0",
            "engines": { "node": ">=99.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "engineStrict: true\nnodeVersion: 20.0.0\n")
        .expect("write workspace settings");

    let why = pacquet
        .with_args(["why"])
        .assert()
        .failure();
    let why_stderr = String::from_utf8_lossy(&why.get_output().stderr);
    assert!(why_stderr.contains("ERR_PNPM_MISSING_PACKAGE_NAME"), "stderr: {why_stderr}");
    assert!(!why_stderr.contains("ERR_PNPM_UNSUPPORTED_ENGINE"), "stderr: {why_stderr}");

    let runtime = Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["runtime", "set"])
        .assert()
        .failure();
    let runtime_stderr = String::from_utf8_lossy(&runtime.get_output().stderr);
    assert!(runtime_stderr.contains("ERR_PNPM_MISSING_RUNTIME_NAME"), "stderr: {runtime_stderr}");
    assert!(!runtime_stderr.contains("ERR_PNPM_UNSUPPORTED_ENGINE"), "stderr: {runtime_stderr}");

    drop(root);
}
