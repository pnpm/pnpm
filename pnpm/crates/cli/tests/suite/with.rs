//! Ports `pnpm11/pnpm/test/withCommand.test.ts`.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    command_env::CommandTestExt,
};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

const PINNED_PNPM_VERSION: &str = "9.3.0";

#[test]
fn with_current_runs_the_currently_active_pnpm_even_when_project_pins_a_different_version() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &serde_json::json!({ "packageManager": "pnpm@9.3.0" }));

    let output = test_command(pacquet, root.path())
        .with_args(["with", "current", "--version"])
        .output()
        .expect("run pacquet with current --version");
    dbg!(&output);
    assert_success(&output);
    assert_current_version(&output);
    assert!(!stdout(&output).contains("9.3.0"));

    drop(root);
}

#[test]
fn with_current_bypasses_the_package_manager_check_when_an_unrelated_manager_is_pinned() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &serde_json::json!({ "packageManager": "yarn@4.0.0" }));

    let output = test_command(pacquet, root.path())
        .with_args(["with", "current", "--version"])
        .output()
        .expect("run pacquet with current --version");
    dbg!(&output);
    assert_success(&output);
    assert!(!stderr(&output).contains("This project is configured to use yarn"));

    drop(root);
}

#[test]
fn with_current_bypasses_dev_engines_package_manager_with_on_fail_download() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "devEngines": {
                "packageManager": {
                    "name": "pnpm",
                    "version": "9.3.0",
                    "onFail": "download",
                },
            },
        }),
    );

    let output = test_command(pacquet, root.path())
        .with_args(["with", "current", "--version"])
        .output()
        .expect("run pacquet with current --version");
    dbg!(&output);
    assert_success(&output);
    assert_current_version(&output);
    assert!(!stdout(&output).contains("9.3.0"));

    drop(root);
}

#[test]
fn with_forwards_subsequent_args_to_the_child_pnpm() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &serde_json::json!({ "name": "project", "version": "1.0.0" }));

    let output = test_command(pacquet, root.path())
        .with_args(["with", "current", "--version"])
        .output()
        .expect("run pacquet with current --version");
    dbg!(&output);
    assert_success(&output);
    assert_semver_like(stdout(&output).trim());

    drop(root);
}

#[test]
fn with_current_dispatches_the_inner_command_after_global_options() {
    for global in [vec!["--color"], vec!["--yes"], vec!["--workspace-root"], vec!["-yC", "."]] {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        write_manifest(&workspace, &serde_json::json!({ "name": "project", "version": "1.0.0" }));

        let args: Vec<&str> =
            global.iter().copied().chain(["with", "current", "--version"]).collect();
        let output = test_command(pacquet, root.path())
            .with_args(args)
            .output()
            .expect("run pacquet with current --version after a global option");
        dbg!(&global, &output);
        assert_success(&output);
        assert_semver_like(stdout(&output).trim());

        drop(root);
    }
}

#[test]
fn with_fails_when_no_spec_is_provided() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output =
        test_command(pacquet, root.path()).with_args(["with"]).output().expect("run pacquet with");
    dbg!(&output);
    assert!(!output.status.success(), "pacquet with (no spec) should fail");

    let stderr = stderr(&output);
    assert!(stderr.contains("Missing version argument"), "stderr should explain the gap: {stderr}");

    drop(root);
}

#[test]
fn with_version_downloads_and_runs_the_specified_pnpm_version() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_manifest(&workspace, &serde_json::json!({ "name": "project", "version": "1.0.0" }));

    let registry_arg = format!("--config.registry={}", mock_instance.url());
    let output = test_command(pacquet, root.path())
        .args([registry_arg.as_str(), "with", PINNED_PNPM_VERSION, "help"])
        .output()
        .expect("run pacquet with a specified pnpm version");
    dbg!(&output);
    assert_success(&output);
    assert!(
        stdout(&output).contains("Version 9.3.0"),
        "downloaded pnpm help should show the requested version; stdout:\n{}",
        stdout(&output),
    );

    drop((root, mock_instance));
}

#[test]
fn with_version_ignores_the_package_manager_pin_and_uses_the_requested_version() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_manifest(&workspace, &serde_json::json!({ "packageManager": "pnpm@9.1.0" }));

    let registry_arg = format!("--config.registry={}", mock_instance.url());
    let output = test_command(pacquet, root.path())
        .args([registry_arg.as_str(), "with", PINNED_PNPM_VERSION, "help"])
        .output()
        .expect("run pacquet with a specified pnpm version");
    dbg!(&output);
    assert_success(&output);
    let stdout = stdout(&output);
    assert!(
        stdout.contains("Version 9.3.0"),
        "downloaded pnpm help should show the requested version; stdout:\n{stdout}",
    );
    assert!(!stdout.contains("Version 9.1.0"), "the packageManager pin must be ignored");

    drop((root, mock_instance));
}

#[test]
fn with_current_requires_a_command() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = test_command(pacquet, root.path())
        .with_args(["with", "current"])
        .output()
        .expect("run pacquet with current");
    dbg!(&output);
    assert!(!output.status.success(), "pacquet with current (no command) should fail");

    let stderr = stderr(&output);
    assert!(
        stderr.contains(r#"Missing command after "current""#),
        "stderr should explain the gap: {stderr}",
    );

    drop(root);
}

fn test_command(command: Command, root: &Path) -> Command {
    let mut command = command.without_ambient_pnpm_config();
    command.env("PNPM_HOME", root.join("pnpm-home"));
    command.env("HOME", root);
    command.env("XDG_CONFIG_HOME", root.join("xdg-config"));
    command.env_remove("COREPACK_ROOT");
    command
}

fn write_manifest(workspace: &Path, manifest: &serde_json::Value) {
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command should succeed\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output),
    );
}

fn assert_current_version(output: &Output) {
    assert_eq!(stdout(output).trim(), pnpm_config::PNPM_VERSION);
}

fn assert_semver_like(value: &str) {
    let mut parts = value.split('.');
    assert!(
        parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
            && parts
                .next()
                .is_some_and(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
            && parts
                .next()
                .is_some_and(|part| part.chars().next().is_some_and(|c| c.is_ascii_digit())),
        "expected a semver-looking version, got {value:?}",
    );
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A task runner that pins `packageManager` spawns many `pnpm run`
/// children at once, and on a cold cache every one of them installs the
/// same engine into the same host-wide store slot. They must not clobber
/// each other's slot mid-install.
#[test]
fn concurrent_engine_installs_all_succeed_on_a_cold_cache() {
    const CONCURRENCY: usize = 7;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_manifest(&workspace, &serde_json::json!({ "name": "project", "version": "1.0.0" }));

    // Every child is spawned before any of them is waited on — that
    // overlap is the whole point, since a serial run never races.
    let registry_arg = format!("--config.registry={}", mock_instance.url());
    let mut children = Vec::with_capacity(CONCURRENCY);
    for _ in 0..CONCURRENCY {
        let pacquet = Command::cargo_bin("pnpm").expect("find the pnpm binary");
        children.push(
            test_command(pacquet, root.path())
                .current_dir(&workspace)
                .args([registry_arg.as_str(), "with", PINNED_PNPM_VERSION, "help"])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("spawn a concurrent engine install"),
        );
    }

    let mut outputs = Vec::with_capacity(CONCURRENCY);
    for child in children {
        outputs.push(child.wait_with_output().expect("wait for a concurrent engine install"));
    }
    for output in &outputs {
        assert_success(output);
        assert!(stdout(output).contains("Version 9.3.0"), "stdout:\n{}", stdout(output));
    }

    drop((root, mock_instance));
}
