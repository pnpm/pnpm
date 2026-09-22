use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, command_env::CommandTestExt};
use std::{fs, path::Path, process::Command};

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

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "engineStrict: true\nnodeVersion: 20.0.0\nconfigDependencies:\n  '@pnpm.e2e/foo': 100.0.0\n",
    )
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
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let node_version = node_version_at(&workspace);
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
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"node":false}"#)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    drop(root);
}

#[test]
fn update_config_can_enable_the_root_engine_check() {
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
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) {\n  config.engineStrict = true;\n  config.nodeVersion = '20.0.0';\n  return config;\n} } }",
    )
    .expect("write .pnpmfile.cjs");

    let assert = pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_UNSUPPORTED_ENGINE"), "stderr: {stderr}");
    assert!(stderr.contains(r#"{"node":"20.0.0"}"#), "stderr: {stderr}");

    drop(root);
}

#[test]
fn update_config_can_disable_the_root_engine_check() {
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
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) {\n  config.engineStrict = false;\n  return config;\n} } }",
    )
    .expect("write .pnpmfile.cjs");

    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    drop(root);
}

#[test]
fn no_runtime_checks_the_active_node_instead_of_the_manifest_runtime() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "compatible-root",
            "version": "1.0.0",
            "devEngines": {
                "runtime": { "name": "node", "version": ">=1.0.0" },
            },
        })
        .to_string(),
    )
    .expect("write initial package.json");
    let node_version = node_version_at(&workspace);
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "compatible-root",
            "version": "1.0.0",
            "engines": { "node": node_version },
            "devEngines": {
                "runtime": { "name": "node", "version": ">=1.0.0" },
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "engineStrict: true\n")
        .expect("write workspace settings");

    pacquet
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"node":false}"#)
        .with_args(["install", "--lockfile-only", "--no-runtime"])
        .assert()
        .success();

    drop(root);
}

fn node_version_at(dir: &Path) -> String {
    let node_output = Command::new("node")
        .without_ambient_pnpm_config()
        .with_env("PNPM_CONFIG_GLOBAL_SHIMS", r#"{"node":false}"#)
        .arg("--version")
        .current_dir(dir)
        .output()
        .expect("run node --version");
    assert!(node_output.status.success(), "node --version must succeed");
    String::from_utf8(node_output.stdout)
        .expect("decode node --version")
        .trim()
        .trim_start_matches('v')
        .to_string()
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
        .with_args(["why", "not-installed"])
        .assert()
        .success();
    let why_stderr = String::from_utf8_lossy(&why.get_output().stderr);
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

#[test]
fn maintenance_install_commands_check_the_root_engine() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
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

    for args in [vec!["dedupe"], vec!["prune"], vec!["deploy", "deployment"]] {
        let assert = Command::cargo_bin("pnpm")
            .expect("find pnpm binary")
            .with_current_dir(&workspace)
            .with_args(args)
            .assert()
            .failure();
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
        assert!(stderr.contains("ERR_PNPM_UNSUPPORTED_ENGINE"), "stderr: {stderr}");
    }

    drop(root);
}
