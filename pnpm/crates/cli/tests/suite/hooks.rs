use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::fs;

#[test]
fn filter_log_is_ignored_with_a_warning() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { filterLog: () => false } }",
    )
    .expect("write filterLog hook");
    fs::write(workspace.join("pnpm-lock.yaml"), "not: [valid").expect("write broken lockfile");

    let output = pacquet_in(&workspace).with_arg("install").assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("filterLog hook is deprecated"), "STDOUT:\n{stdout}");
    assert!(stdout.contains("Ignoring broken lockfile"), "STDOUT:\n{stdout}");

    drop(root);
}

const EXTRA_ENV_PNPMFILE: &str = "module.exports = { hooks: { updateConfig (config) { config.extraEnv = { ...config.extraEnv, PNPM_HOOK_MARKER: 'from-hook' }; return config } } }";

/// `updateConfig` applies to `pnpm run`, not just to the install family:
/// the settings a hook returns — `extraEnv` here — reach the environment of
/// the script it spawns.
#[cfg(unix)]
#[test]
fn update_config_applies_to_run() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    let manifest = serde_json::json!({
        "name": "run-reads-extra-env",
        "version": "0.0.0",
        "scripts": {
            "write-marker": format!(r#"printf %s "$PNPM_HOOK_MARKER" > "{}""#, marker.display()),
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    fs::write(workspace.join(".pnpmfile.cjs"), EXTRA_ENV_PNPMFILE).expect("write pnpmfile");

    pacquet_in(&workspace).with_arg("run").with_arg("write-marker").assert().success();

    assert_eq!(fs::read_to_string(&marker).expect("read marker"), "from-hook");

    drop(root);
}

/// The same for `pnpm exec`, which spawns its command through the same
/// environment.
#[cfg(unix)]
#[test]
fn update_config_applies_to_exec() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("marker.txt");
    fs::write(workspace.join("package.json"), r#"{"name":"exec-reads-extra-env"}"#)
        .expect("write package.json");
    fs::write(workspace.join(".pnpmfile.cjs"), EXTRA_ENV_PNPMFILE).expect("write pnpmfile");

    pacquet_in(&workspace)
        .with_arg("exec")
        .with_arg("sh")
        .with_arg("-c")
        .with_arg(format!(r#"printf %s "$PNPM_HOOK_MARKER" > "{}""#, marker.display()))
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&marker).expect("read marker"), "from-hook");

    drop(root);
}

/// A recursive `pnpm run` applies the workspace-root hook to every
/// project's script environment.
#[cfg(unix)]
#[test]
fn update_config_applies_to_recursive_run() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"root","private":true}"#)
        .expect("write root package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join(".pnpmfile.cjs"), EXTRA_ENV_PNPMFILE).expect("write pnpmfile");
    let project = workspace.join("packages").join("a");
    fs::create_dir_all(&project).expect("create project dir");
    let marker = project.join("marker.txt");
    let manifest = serde_json::json!({
        "name": "a",
        "version": "0.0.0",
        "scripts": {
            "write-marker": format!(r#"printf %s "$PNPM_HOOK_MARKER" > "{}""#, marker.display()),
        },
    })
    .to_string();
    fs::write(project.join("package.json"), manifest).expect("write project package.json");

    pacquet_in(&workspace)
        .with_arg("--recursive")
        .with_arg("run")
        .with_arg("write-marker")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&marker).expect("read marker"), "from-hook");

    drop(root);
}

/// A hook that changes an install-affecting setting must be applied before
/// the verify-deps-before-run check compares the live settings with the ones
/// the last install recorded. Without the hook, `pnpm run` sees the
/// pre-hook value, reports the setting as changed, and — under
/// `verifyDepsBeforeRun: error` — refuses to run any script at all.
#[cfg(unix)]
#[test]
fn update_config_applies_before_the_verify_deps_check() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let manifest = serde_json::json!({
        "name": "run-under-verify-deps",
        "private": true,
        "scripts": { "foo": "printf ran" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages: []\nverifyDepsBeforeRun: error\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) { config.dedupePeers = true; return config } } }",
    )
    .expect("write pnpmfile");

    pacquet_in(&workspace).with_arg("install").assert().success();
    let output = pacquet_in(&workspace).with_arg("run").with_arg("foo").assert().success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("ran"), "the script should have run\nSTDOUT:\n{stdout}");

    drop(root);
}
