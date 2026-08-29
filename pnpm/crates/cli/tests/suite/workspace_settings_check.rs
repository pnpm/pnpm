//! Reporting of `pnpm-workspace.yaml` keys that set nothing: unrecognized
//! settings warn, harden into an error when the running pnpm is the version
//! the project pins, and stay off `pnpm config get <key>` entirely.

use pnpm_testing_utils::bin::CommandTempCwd;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

#[test]
fn an_unrecognized_workspace_setting_warns_without_a_pin() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_plain_manifest(&workspace);
    write_workspace_yaml(&workspace, "minimumReleaseAg: 100\npackages:\n  - .\n");

    let output = run(pacquet, root.path(), &["install", "--lockfile-only"]);

    assert_success(&output);
    assert_contains(
        &stderr(&output),
        r#"[WARN] The following settings in pnpm-workspace.yaml are not recognized by this version of pnpm and were ignored: "minimumReleaseAg" (did you mean "minimumReleaseAge"?)."#,
    );
}

#[test]
fn an_unrecognized_workspace_setting_fails_when_the_running_pnpm_is_the_pinned_version() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace_yaml(&workspace, "minimumReleaseAg: 100\npackages:\n  - .\n");
    write_package_manager_pin(&workspace);

    let output =
        run(pacquet, root.path(), &["install", "--lockfile-only", "--config.pm-on-fail=error"]);

    assert_failure(&output);
    let stderr = stderr(&output);
    assert_contains(&stderr, "ERR_PNPM_UNRECOGNIZED_WORKSPACE_SETTINGS");
    assert_contains(
        &stderr,
        r#"The following settings in pnpm-workspace.yaml are not recognized by this version of pnpm: "minimumReleaseAg" (did you mean "minimumReleaseAge"?)."#,
    );
}

#[test]
fn a_kebab_case_spelling_of_a_known_setting_warns() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_plain_manifest(&workspace);
    write_workspace_yaml(&workspace, "store-dir: some-store\npackages:\n  - .\n");

    let output = run(pacquet, root.path(), &["install", "--lockfile-only"]);

    assert_success(&output);
    assert_contains(
        &stderr(&output),
        r#"[WARN] The following settings in pnpm-workspace.yaml were ignored because they are not written in camelCase: "store-dir" (use "storeDir")."#,
    );
}

/// Single-key reads are consumed by scripts, so nothing may join the value.
#[test]
fn config_get_of_one_key_stays_quiet_and_succeeds() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace_yaml(&workspace, "minimumReleaseAg: 100\nnodeLinker: hoisted\n");
    write_package_manager_pin(&workspace);

    let output = run_with_switch_disabled(pacquet, root.path(), &["config", "get", "node-linker"]);

    assert_success(&output);
    assert_contains(&stdout(&output), "hoisted");
    let stderr = stderr(&output);
    assert!(!stderr.contains("[WARN]"), "expected no config warning; got:\n{stderr}");
    assert!(!stderr.contains("not recognized"), "expected no unrecognized report; got:\n{stderr}");
}

/// A broken config file must stay inspectable and repairable.
#[test]
fn config_list_warns_but_succeeds_under_a_satisfied_pin() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace_yaml(&workspace, "minimumReleaseAg: 100\n");
    write_package_manager_pin(&workspace);

    let output = run_with_switch_disabled(pacquet, root.path(), &["config", "list"]);

    assert_success(&output);
    assert_contains(
        &stderr(&output),
        r#"[WARN] The following settings in pnpm-workspace.yaml are not recognized by this version of pnpm and were ignored: "minimumReleaseAg" (did you mean "minimumReleaseAge"?)."#,
    );
}

/// A `--global` command does not act on the project, so the project's pin
/// does not harden the report into an error.
#[test]
fn a_global_command_warns_under_a_satisfied_pin() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace_yaml(&workspace, "minimumReleaseAg: 100\n");
    write_package_manager_pin(&workspace);

    let global_bin = root.path().join("pnpm-home");
    fs::create_dir_all(&global_bin).expect("create the global bin dir");
    let mut pacquet = pacquet;
    pacquet.env("PATH", prepend_to_path(&global_bin));
    let output = run_with_switch_disabled(pacquet, root.path(), &["list", "--global"]);

    assert_success(&output);
    assert_contains(
        &stderr(&output),
        r#"[WARN] The following settings in pnpm-workspace.yaml are not recognized by this version of pnpm and were ignored: "minimumReleaseAg" (did you mean "minimumReleaseAge"?)."#,
    );
}

/// A key carrying terminal escapes reaches stderr and any CI log, so the
/// report must not let it move the cursor or repaint the screen.
#[test]
fn a_key_with_control_characters_is_sanitized() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_plain_manifest(&workspace);
    write_workspace_yaml(&workspace, "\"nope\\e[31mRED\\r\": 1\npackages:\n  - .\n");

    let output = run(pacquet, root.path(), &["install", "--lockfile-only"]);

    assert_success(&output);
    let stderr = stderr(&output);
    assert_contains(&stderr, "not recognized by this version of pnpm");
    assert!(!stderr.contains('\u{1b}'), "an escape reached the output: {stderr:?}");
    assert!(!stderr.contains('\r'), "a carriage return reached the output: {stderr:?}");
}

/// `pnpm list --global` refuses to run when the global bin directory is not
/// on `PATH`, which is about the environment rather than this file.
fn prepend_to_path(dir: &Path) -> std::ffi::OsString {
    let mut entries = vec![dir.to_path_buf()];
    if let Some(path) = std::env::var_os("PATH") {
        entries.extend(std::env::split_paths(&path));
    }
    std::env::join_paths(entries).expect("join PATH entries")
}

fn write_plain_manifest(workspace: &Path) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "plain", "version": "1.0.0", "private": true }).to_string(),
    )
    .expect("write package.json");
}

fn write_workspace_yaml(workspace: &Path, contents: &str) {
    fs::write(workspace.join("pnpm-workspace.yaml"), contents).expect("write pnpm-workspace.yaml");
}

/// Pin the running pnpm version itself, with `pm-on-fail` left to each test:
/// the check must pass while the strictness decision sees a satisfied pin.
fn write_package_manager_pin(workspace: &Path) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "pinned",
            "version": "1.0.0",
            "private": true,
            "packageManager": format!("pnpm@{}", pnpm_config::PNPM_VERSION),
        })
        .to_string(),
    )
    .expect("write package.json");
}

fn run(command: Command, root: &Path, args: &[&str]) -> Output {
    let mut command = command;
    command.env("PNPM_HOME", root.join("pnpm-home"));
    command.env("HOME", root);
    command.env("XDG_CONFIG_HOME", root.join("xdg-config"));
    command.args(args).output().expect("run pacquet")
}

/// A pinned project would otherwise resolve the pin's env-lockfile entry,
/// which needs a registry; turning version management off keeps the test
/// hermetic while the pin stays visible to the strictness decision.
fn run_with_switch_disabled(mut command: Command, root: &Path, args: &[&str]) -> Output {
    command.env("PNPM_CONFIG_MANAGE_PACKAGE_MANAGER_VERSIONS", "false");
    run(command, root, args)
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command should succeed\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output),
    );
}

fn assert_failure(output: &Output) {
    assert!(
        !output.status.success(),
        "command should fail\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output),
    );
}

fn assert_contains(text: &str, expected: &str) {
    assert!(
        unwrap_diagnostic(text).contains(&unwrap_diagnostic(expected)),
        "expected {expected:?} in:\n{text}",
    );
}

/// miette hard-wraps a diagnostic to the terminal width and prefixes the
/// continuation lines with `│`, so an expected message only matches after
/// both sides are flattened to single-spaced text.
fn unwrap_diagnostic(text: &str) -> String {
    text.replace('│', " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
