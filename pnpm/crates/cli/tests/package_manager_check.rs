//! Ports `pnpm11/pnpm/test/packageManagerCheck.test.ts`.

use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

#[test]
fn install_fails_when_the_project_pins_another_package_manager() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &serde_json::json!({ "packageManager": "yarn@4.0.0" }));

    let output = run(pacquet, root.path(), &["install"]);

    assert_failure(&output);
    assert_contains(&stderr(&output), "This project is configured to use yarn");
}

#[test]
fn pm_on_fail_warn_downgrades_the_other_package_manager_failure_to_a_warning() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &serde_json::json!({ "packageManager": "yarn@4.0.0" }));

    let output = run(pacquet, root.path(), &["install", "--config.pm-on-fail=warn"]);

    assert_success(&output);
    assert_contains(&output_text(&output), "This project is configured to use yarn");
}

#[test]
fn pm_on_fail_error_reports_a_package_manager_version_mismatch() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &serde_json::json!({ "packageManager": "pnpm@0.0.0" }));

    let output = run(pacquet, root.path(), &["install", "--config.pm-on-fail=error"]);

    assert_failure(&output);
    assert_contains(
        &stderr(&output),
        "This project is configured to use 0.0.0 of pnpm. Your current pnpm is",
    );
}

#[test]
fn pm_on_fail_ignore_bypasses_the_package_manager_version_mismatch() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &serde_json::json!({ "packageManager": "pnpm@0.0.0" }));

    let output = run(pacquet, root.path(), &["install", "--config.pm-on-fail=ignore"]);

    assert_success(&output);
    assert!(!output_text(&output).contains("0.0.0"), "unexpected mention of the pinned version");
}

#[test]
fn a_package_manager_field_with_an_integrity_hash_matches_the_running_version() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let pinned = format!("pnpm@{}+sha256.123456789", pacquet_config::PNPM_VERSION);
    write_manifest(&workspace, &serde_json::json!({ "packageManager": pinned }));

    let output = run(pacquet, root.path(), &["install"]);

    assert_success(&output);
}

#[test]
fn a_package_manager_field_holding_a_url_is_not_checked() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(
        &workspace,
        &serde_json::json!({ "packageManager": "pnpm@https://github.com/pnpm/pnpm" }),
    );

    let output = run(pacquet, root.path(), &["install"]);

    assert_success(&output);
}

#[test]
fn commands_that_do_not_belong_to_the_project_skip_the_check() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &serde_json::json!({ "packageManager": "yarn@3.0.0" }));

    let output = run(pacquet, root.path(), &["store", "path"]);

    assert_success(&output);
}

#[test]
fn dev_engines_package_manager_with_on_fail_error_reports_a_version_mismatch() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_dev_engines_package_manager(&workspace, "pnpm", "0.0.1", Some("error"));

    let output = run(pacquet, root.path(), &["install"]);

    assert_failure(&output);
    assert_contains(&stderr(&output), "This project is configured to use 0.0.1 of pnpm");
}

#[test]
fn dev_engines_package_manager_with_on_fail_warn_warns_about_a_version_mismatch() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_dev_engines_package_manager(&workspace, "pnpm", "0.0.1", Some("warn"));

    let output = run(pacquet, root.path(), &["install"]);

    assert_success(&output);
    assert_contains(&output_text(&output), "This project is configured to use 0.0.1 of pnpm");
}

#[test]
fn dev_engines_package_manager_with_on_fail_ignore_is_not_checked() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_dev_engines_package_manager(&workspace, "pnpm", "0.0.1", Some("ignore"));

    let output = run(pacquet, root.path(), &["install"]);

    assert_success(&output);
    assert!(!output_text(&output).contains("0.0.1"), "unexpected mention of the pinned version");
}

#[test]
fn dev_engines_package_manager_naming_another_package_manager_fails() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_dev_engines_package_manager(&workspace, "yarn", ">=4.0.0", Some("error"));

    let output = run(pacquet, root.path(), &["install"]);

    assert_failure(&output);
    assert_contains(&stderr(&output), "This project is configured to use yarn");
}

#[test]
fn dev_engines_package_manager_array_selects_the_pnpm_entry() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "devEngines": {
                "packageManager": [
                    { "name": "yarn", "version": ">=4.0.0", "onFail": "ignore" },
                    { "name": "pnpm", "version": "0.0.1", "onFail": "error" },
                ],
            },
        }),
    );

    let output = run(pacquet, root.path(), &["install"]);

    assert_failure(&output);
    assert_contains(&stderr(&output), "This project is configured to use 0.0.1 of pnpm");
}

#[test]
fn dev_engines_package_manager_array_defaults_on_fail_to_ignore_before_the_last_entry() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "devEngines": {
                "packageManager": [
                    { "name": "pnpm", "version": "0.0.1" },
                    { "name": "yarn", "version": ">=4.0.0" },
                ],
            },
        }),
    );

    let output = run(pacquet, root.path(), &["install"]);

    assert_success(&output);
}

#[test]
fn the_pm_on_fail_hint_can_be_followed_as_a_bare_flag() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_dev_engines_package_manager(&workspace, "pnpm", "0.0.1", Some("error"));

    let output = run(pacquet, root.path(), &["install", "--pm-on-fail=ignore"]);

    assert_success(&output);
    assert!(!output_text(&output).contains("0.0.1"), "unexpected mention of the pinned version");
}

#[test]
fn the_runtime_on_fail_hint_can_be_followed_as_a_bare_flag() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "node", "version": "99999.0.0", "onFail": "error",
        }),
    );

    let output = run(
        pacquet,
        root.path(),
        &[
            "--config.verify-deps-before-run=false",
            "--runtime-on-fail=ignore",
            "exec",
            "node",
            "--version",
        ],
    );

    assert_success(&output);
    assert!(!output_text(&output).contains("99999.0.0"), "unexpected mention of the pinned range");
}

#[test]
fn pm_on_fail_ignore_from_the_env_bypasses_the_check() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_dev_engines_package_manager(&workspace, "pnpm", "0.0.1", Some("error"));

    let output =
        run(pacquet.with_env("pnpm_config_pm_on_fail", "ignore"), root.path(), &["install"]);

    assert_success(&output);
    assert!(!output_text(&output).contains("0.0.1"), "unexpected mention of the pinned version");
}

#[test]
fn pm_on_fail_ignore_from_the_workspace_manifest_bypasses_the_check() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_dev_engines_package_manager(&workspace, "pnpm", "0.0.1", Some("error"));
    fs::write(workspace.join("pnpm-workspace.yaml"), "pmOnFail: ignore\n")
        .expect("write pnpm-workspace.yaml");

    let output = run(pacquet, root.path(), &["install"]);

    assert_success(&output);
    assert!(!output_text(&output).contains("0.0.1"), "unexpected mention of the pinned version");
}

#[test]
fn the_check_still_runs_under_corepack_and_explains_why_no_switch_happened() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_dev_engines_package_manager(&workspace, "pnpm", "0.0.1", Some("warn"));

    let output =
        run(pacquet.with_env("COREPACK_ROOT", "/fake/corepack"), root.path(), &["install"]);

    assert_success(&output);
    let text = output_text(&output);
    assert_contains(&text, "This project is configured to use 0.0.1 of pnpm");
    assert_contains(&text, "Corepack invoked pnpm");
}

#[test]
fn on_fail_download_under_corepack_fails_instead_of_switching_versions() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_dev_engines_package_manager(&workspace, "pnpm", "0.0.1", Some("download"));

    let output =
        run(pacquet.with_env("COREPACK_ROOT", "/fake/corepack"), root.path(), &["install"]);

    assert_failure(&output);
    let stderr = stderr(&output);
    assert_contains(&stderr, "This project is configured to use 0.0.1 of pnpm");
    assert_contains(&stderr, "does not switch versions when running under corepack");
    assert_contains(&stderr, "invoke pnpm directly");
}

#[test]
fn turning_off_version_management_accepts_a_mismatched_pnpm_pin() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &serde_json::json!({ "packageManager": "pnpm@0.0.0" }));

    let output = run(
        pacquet.with_env("PNPM_CONFIG_MANAGE_PACKAGE_MANAGER_VERSIONS", "false"),
        root.path(),
        &["install"],
    );

    assert_success(&output);
    assert!(!output_text(&output).contains("0.0.0"), "unexpected mention of the pinned version");
}

#[test]
fn a_global_command_warns_instead_of_failing_the_package_manager_check() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &serde_json::json!({ "packageManager": "yarn@4.0.0" }));

    let output = run(pacquet, root.path(), &["list", "--global"]);

    assert_success(&output);
    assert_contains(
        &output_text(&output),
        "Using --global skips the package manager check for this project",
    );
}

#[test]
fn dev_engines_runtime_with_on_fail_error_reports_a_node_version_mismatch() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "node", "version": "99999.0.0", "onFail": "error",
        }),
    );

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_failure(&output);
    assert_contains(&stderr(&output), "This project requires Node.js 99999.0.0");
}

#[test]
fn dev_engines_runtime_with_on_fail_warn_warns_about_a_node_version_mismatch() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "node", "version": "99999.0.0", "onFail": "warn",
        }),
    );

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_success(&output);
    assert_contains(&output_text(&output), "This project requires Node.js 99999.0.0");
}

#[test]
fn dev_engines_runtime_with_on_fail_ignore_is_not_checked() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "node", "version": "99999.0.0", "onFail": "ignore",
        }),
    );

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_success(&output);
    assert!(!output_text(&output).contains("99999.0.0"), "unexpected mention of the pinned range");
}

#[test]
fn engines_runtime_is_checked_too() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "engines",
        &serde_json::json!({
            "name": "node", "version": "99999.0.0", "onFail": "error",
        }),
    );

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_failure(&output);
    assert_contains(&stderr(&output), "This project requires Node.js 99999.0.0");
}

#[test]
fn an_invalid_node_version_range_fails_with_the_runtime_on_fail_hint() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "node", "version": "invalid range", "onFail": "error",
        }),
    );

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_failure(&output);
    let stderr = stderr(&output);
    assert_contains(
        &stderr,
        "This project requires an invalid Node.js version range: invalid range",
    );
    assert_contains(&stderr, "--runtime-on-fail=ignore");
}

#[test]
fn an_invalid_deno_version_range_names_deno() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "deno", "version": "invalid range", "onFail": "error",
        }),
    );

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_failure(&output);
    assert_contains(
        &stderr(&output),
        "This project requires an invalid Deno version range: invalid range",
    );
}

#[test]
fn an_invalid_bun_version_range_names_bun() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "bun", "version": "invalid range", "onFail": "error",
        }),
    );

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_failure(&output);
    assert_contains(
        &stderr(&output),
        "This project requires an invalid Bun version range: invalid range",
    );
}

#[test]
fn a_runtime_without_a_version_range_fails() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "node", "onFail": "error",
        }),
    );

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_failure(&output);
    assert_contains(
        &stderr(&output),
        "This project requires a Node.js runtime but does not specify a version range",
    );
}

#[test]
fn runtime_array_entries_are_checked_beyond_the_first_one() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!([
            { "name": "node", "version": "*", "onFail": "error" },
            { "name": "deno", "version": "invalid range", "onFail": "error" },
        ]),
    );

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_failure(&output);
    assert_contains(
        &stderr(&output),
        "This project requires an invalid Deno version range: invalid range",
    );
}

#[test]
fn runtime_on_fail_ignore_bypasses_the_manifest_on_fail() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "node", "version": "99999.0.0", "onFail": "error",
        }),
    );

    let output = run(
        pacquet,
        root.path(),
        &[
            "--config.verify-deps-before-run=false",
            "--config.runtime-on-fail=ignore",
            "exec",
            "node",
            "--version",
        ],
    );

    assert_success(&output);
    assert!(!output_text(&output).contains("99999.0.0"), "unexpected mention of the pinned range");
}

#[test]
fn a_failing_runtime_check_does_not_block_the_version_output() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_runtime(
        &workspace,
        "devEngines",
        &serde_json::json!({
            "name": "node", "version": "99999.0.0", "onFail": "error",
        }),
    );

    let output = run(pacquet, root.path(), &["--version"]);

    assert_success(&output);
    assert_eq!(stdout(&output).trim(), pacquet_config::PNPM_VERSION);
}

/// `exec` is the cheapest command that still goes through the pre-command
/// checks; the dependency verification it would otherwise run is unrelated.
const EXEC_NODE_VERSION: [&str; 4] =
    ["--config.verify-deps-before-run=false", "exec", "node", "--version"];

fn run(command: Command, root: &Path, args: &[&str]) -> Output {
    let mut command = command;
    command.env("PNPM_HOME", root.join("pnpm-home"));
    command.env("HOME", root);
    command.env("XDG_CONFIG_HOME", root.join("xdg-config"));
    for name in ["pnpm_config_pm_on_fail", "PNPM_CONFIG_PM_ON_FAIL", "COREPACK_ROOT"] {
        if command.get_envs().all(|(set_name, _)| set_name != name) {
            command.env_remove(name);
        }
    }
    let output = command.args(args).output().expect("run pacquet");
    dbg!(&output);
    output
}

fn write_manifest(workspace: &Path, manifest: &serde_json::Value) {
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
}

fn write_dev_engines_package_manager(
    workspace: &Path,
    name: &str,
    version: &str,
    on_fail: Option<&str>,
) {
    let mut package_manager = serde_json::json!({ "name": name, "version": version });
    if let Some(on_fail) = on_fail {
        package_manager["onFail"] = serde_json::Value::String(on_fail.to_string());
    }
    write_manifest(
        workspace,
        &serde_json::json!({ "devEngines": { "packageManager": package_manager } }),
    );
}

fn write_runtime(workspace: &Path, engines_field: &str, runtime: &serde_json::Value) {
    write_manifest(workspace, &serde_json::json!({ engines_field: { "runtime": runtime } }));
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
        "expected {expected:?} in:\n{text}"
    );
}

/// miette hard-wraps a diagnostic to the terminal width and prefixes the
/// continuation lines with `│`, so an expected message only matches after
/// both sides are flattened to single-spaced text.
fn unwrap_diagnostic(text: &str) -> String {
    text.replace('│', " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn output_text(output: &Output) -> String {
    format!("{}{}", stdout(output), stderr(output))
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
