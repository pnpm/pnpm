//! Ports `pnpm11/pnpm/test/packageManagerCheck.test.ts`.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, command_env::CommandTestExt};
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
    let stderr = stderr(&output);
    assert_contains(&stderr, "This project is configured to use yarn");
    // pnpm can provide that package manager, so the failure says how
    // rather than leaving the project unusable.
    assert_contains(&stderr, "pnpm dlx yarn");
    assert_contains(&stderr, "pnpm shim add yarn");
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
    let CommandTempCwd { mut pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry_with_pnpm_version(pnpm_config::PNPM_VERSION);
    let pinned = format!("pnpm@{}+sha256.123456789", pnpm_config::PNPM_VERSION);
    write_manifest(&workspace, &serde_json::json!({ "packageManager": pinned }));
    pacquet.env("PNPM_CONFIG_REGISTRY", npmrc_info.mock_instance.url());

    let output = run(pacquet, root.path(), &["install"]);

    assert_success(&output);
    drop((root, npmrc_info));
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
fn control_characters_in_a_package_manager_name_are_stripped_from_the_output() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &serde_json::json!({ "packageManager": "ya\u{1b}[2Jrn@4.0.0" }));

    let output = run(pacquet, root.path(), &["install"]);

    assert_failure(&output);
    let stderr = stderr(&output);
    assert!(!stderr.contains('\u{1b}'), "escape sequence reached the terminal:\n{stderr}");
    assert_contains(&stderr, "This project is configured to use ya[2Jrn");
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

/// The pinned pnpm belongs in the lockfile whichever command records it, so
/// a project's first pacquet command being something other than an install
/// must not leave `packageManagerDependencies` unwritten.
#[test]
fn a_command_outside_the_install_family_records_the_pinned_package_manager() {
    let CommandTempCwd { mut pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry_with_pnpm_version(pnpm_config::PNPM_VERSION);
    write_dev_engines_package_manager(&workspace, "pnpm", pnpm_config::PNPM_VERSION, None);
    pacquet.env("PNPM_CONFIG_REGISTRY", npmrc_info.mock_instance.url());

    let output = run(pacquet, root.path(), &EXEC_NODE_VERSION);

    assert_success(&output);
    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read the written lockfile");
    assert_contains(&lockfile, "packageManagerDependencies:");
    assert_contains(&lockfile, &format!("pnpm@{}", pnpm_config::PNPM_VERSION));
    drop((root, npmrc_info));
}

/// Adding a pnpm pin to a project whose dependencies are already installed
/// must still record it. The up-to-date fast path returns before the install
/// pipeline that writes the entry, so a plain install kept reporting success
/// while every `--frozen-lockfile` run failed on the entry it never wrote.
#[test]
fn adding_a_pin_to_an_up_to_date_project_records_the_package_manager() {
    let CommandTempCwd { mut pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry_with_pnpm_version(pnpm_config::PNPM_VERSION);
    write_manifest(
        &workspace,
        &serde_json::json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } }),
    );
    pacquet.env("PNPM_CONFIG_REGISTRY", npmrc_info.mock_instance.url());
    assert_success(&run(pacquet, root.path(), &["install"]));

    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": { "@pnpm.e2e/foo": "100.0.0" },
            "devEngines": {
                "packageManager": { "name": "pnpm", "version": pnpm_config::PNPM_VERSION },
            },
        }),
    );
    let mut pinned = pacquet_at(&workspace);
    pinned.env("PNPM_CONFIG_REGISTRY", npmrc_info.mock_instance.url());
    let output = run(pinned, root.path(), &["install"]);

    assert_success(&output);
    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read the written lockfile");
    assert_contains(&lockfile, "packageManagerDependencies:");
    assert_contains(&lockfile, &format!("pnpm@{}", pnpm_config::PNPM_VERSION));
    drop((root, npmrc_info));
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
    assert_eq!(stdout(&output).trim(), pnpm_config::PNPM_VERSION);
}

/// `exec` is the cheapest command that still goes through the pre-command
/// checks; the dependency verification it would otherwise run is unrelated.
const EXEC_NODE_VERSION: [&str; 4] =
    ["--config.verify-deps-before-run=false", "exec", "node", "--version"];

/// A second command for a test that runs pacquet twice; the first one
/// [`CommandTempCwd::init`] hands out is consumed by [`run`].
fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
}

fn run(command: Command, root: &Path, args: &[&str]) -> Output {
    let mut command = command;
    command.env("PNPM_HOME", root.join("pnpm-home"));
    command.env("HOME", root);
    command.env("XDG_CONFIG_HOME", root.join("xdg-config"));
    // These checks run `exec node`, which resolves `node` from `PATH`. A
    // context-aware global shim there would honour the deliberately
    // unsatisfiable `devEngines.runtime` pin these fixtures declare and
    // fail to fetch it, instead of running the node the test expects.
    command.env("PNPM_SHIM_BYPASS", "1");
    // The pnpm and npm settings these checks read are already gone:
    // `CommandTempCwd::init` strips the inherited ones, and an explicit
    // `env` on the command overrides that removal. `COREPACK_ROOT` is not
    // one of them, so clear it here — unless a test set it on purpose.
    // Windows matches environment names case-insensitively, so does this.
    let explicitly_set =
        command.get_envs().map(|(name, _)| name.to_string_lossy().into_owned()).collect::<Vec<_>>();
    if !explicitly_set.iter().any(|name| name.eq_ignore_ascii_case("COREPACK_ROOT")) {
        command.env_remove("COREPACK_ROOT");
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
        "expected {expected:?} in:\n{text}",
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

/// Naming a package manager records which one the project uses, so it
/// stays possible in a project pinned to another one — changing that
/// declaration is not the pinned manager's work. Adding anything else
/// still is.
#[test]
fn a_project_pinned_to_another_package_manager_can_still_be_repinned() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &serde_json::json!({ "packageManager": "yarn@4.0.0" }));

    let output = run(pacquet, root.path(), &["add", "npm@11"]);

    assert_success(&output);
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(workspace.join("package.json")).unwrap()).unwrap();
    assert_eq!(
        manifest["devEngines"]["packageManager"],
        serde_json::json!({ "name": "npm", "version": "11" }),
    );
    // The two fields declare the same thing, so the one that was replaced
    // is gone rather than left to contradict the new declaration.
    assert_eq!(manifest.get("packageManager"), None, "{manifest}");
}

/// Declaring a package manager and adding a dependency in one command
/// lands both together: the declaration is written into the manifest the
/// install saves, not into one of its own.
#[test]
fn a_mixed_add_writes_the_declaration_with_the_dependency() {
    let CommandTempCwd { mut pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_manifest(&workspace, &serde_json::json!({}));
    pacquet.env("PNPM_CONFIG_REGISTRY", npmrc_info.mock_instance.url());

    let output = run(
        pacquet,
        root.path(),
        &["add", "yarn@1", "@pnpm.e2e/dep-of-pkg-with-1-dep", "--lockfile-only"],
    );

    assert_success(&output);
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(workspace.join("package.json")).unwrap()).unwrap();
    assert!(
        manifest["packageManager"].as_str().is_some_and(|pin| pin.starts_with("yarn@1.")),
        "{manifest}",
    );
    assert!(manifest["dependencies"]["@pnpm.e2e/dep-of-pkg-with-1-dep"].is_string(), "{manifest}");
    drop((root, npmrc_info));
}

/// And an install that fails takes the declaration with it, rather than
/// leaving the project declaring a package manager it never installed
/// anything for.
#[test]
fn a_failed_add_leaves_the_declaration_unwritten() {
    let CommandTempCwd { mut pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let original = serde_json::json!({ "name": "project", "version": "1.0.0" });
    write_manifest(&workspace, &original);
    pacquet.env("PNPM_CONFIG_REGISTRY", npmrc_info.mock_instance.url());

    let output = run(
        pacquet,
        root.path(),
        &["add", "yarn@1", "@pnpm.e2e/this-package-does-not-exist", "--lockfile-only"],
    );

    assert_failure(&output);
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(workspace.join("package.json")).unwrap()).unwrap();
    assert_eq!(manifest, original);
    drop((root, npmrc_info));
}

/// Which package manager a project uses is that project's declaration,
/// not something a filter writes across a selection — and it must not
/// quietly become an install of the npm package that shares the name.
#[test]
fn declaring_a_package_manager_for_a_filtered_selection_is_refused() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &serde_json::json!({ "name": "root", "version": "1.0.0" }));
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n").unwrap();
    let project = workspace.join("packages").join("app");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("package.json"),
        serde_json::json!({ "name": "app", "version": "1.0.0" }).to_string(),
    )
    .unwrap();

    let output = run(pacquet, root.path(), &["add", "yarn@4", "--filter", "app"]);

    assert_failure(&output);
    assert_contains(&stderr(&output), "ERR_PNPM_PACKAGE_MANAGER_IN_SELECTION");
}

/// A `package.json` that parses but is not an object has nowhere to
/// record a package manager. That is the project's own file rather than
/// an impossible state, so it fails as an error.
#[test]
fn a_manifest_that_is_not_an_object_fails_instead_of_panicking() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "[]").unwrap();

    let output = run(pacquet, root.path(), &["add", "npm@11"]);

    assert_failure(&output);
    let stderr = stderr(&output);
    assert_contains(&stderr, "ERR_PNPM_INVALID_MANIFEST");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

/// Yarn is started from a project pin by corepack, which reads only
/// `packageManager` and only accepts an exact version there — so a Yarn
/// pin is resolved to one, and carries nothing else: the release corepack
/// downloads is corepack's to verify, not pnpm's to pin.
#[test]
fn a_yarn_pin_is_recorded_as_the_exact_version_corepack_requires() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(
        &workspace,
        &serde_json::json!({ "devEngines": { "packageManager": { "name": "npm" } } }),
    );

    let output = run(pacquet, root.path(), &["add", "yarn@1"]);

    assert_success(&output);
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(workspace.join("package.json")).unwrap()).unwrap();
    let pin = manifest["packageManager"].as_str().expect("a recorded package manager");
    let reference =
        pin.strip_prefix("yarn@").unwrap_or_else(|| panic!("expected a Yarn pin, got {pin}"));
    let version = node_semver::Version::parse(reference).expect("an exact version");
    assert_eq!(version.major, 1, "{pin}");
    assert!(!reference.contains('+'), "{pin}");
    assert_eq!(manifest.get("devEngines"), None, "{manifest}");
}

#[test]
fn a_project_pinned_to_another_package_manager_still_refuses_an_ordinary_add() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &serde_json::json!({ "packageManager": "yarn@4.0.0" }));

    let output = run(pacquet, root.path(), &["add", "lodash"]);

    assert_failure(&output);
    assert_contains(&stderr(&output), "This project is configured to use yarn");
}
