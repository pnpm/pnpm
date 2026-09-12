//! The warning that names Yarn's `workspaces` field in the root manifest:
//! what the user sees on stderr, and the installs that say it. Both install
//! entry points warn — the full install path and the up-to-date
//! short-circuit a repeat install takes — and neither says it inside a pnpm
//! workspace, where `pnpm-workspace.yaml` already selects the projects.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::CommandTempCwd, command_env::CommandTestExt,
    diagnostics::assert_diagnostic_contains as assert_contains,
};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

/// The whole line as it reaches a terminal, label included: the wording is
/// pnpm 11's, and a user grepping their CI log for it must find the same
/// text under pnpm 12.
const WARNING: &str = r#"[WARN] The "workspaces" field in package.json is not supported by pnpm. Create a "pnpm-workspace.yaml" file instead."#;

#[test]
fn an_install_warns_about_a_workspaces_field_with_no_pnpm_workspace_yaml() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(
        &workspace,
        r#"{"name":"converted","version":"1.0.0","private":true,"workspaces":["packages/*"]}"#,
    );

    let output = run(pacquet, root.path(), &["install", "--lockfile-only"]);

    assert_success(&output);
    assert_contains(&stderr(&output), WARNING);
}

/// The second install of an unchanged project never reaches the full
/// install path, so the up-to-date short-circuit has to emit the warning
/// itself. A converted repository whose install is already current is
/// exactly the case that has no other hint about the ignored field.
#[test]
fn a_repeat_install_taking_the_up_to_date_path_warns_too() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_manifest(
        &workspace,
        r#"{"name":"converted","version":"1.0.0","private":true,"workspaces":["packages/*"]}"#,
    );

    let first = run(pacquet_in(&workspace), root.path(), &["install"]);
    assert_success(&first);
    let second = run(pacquet_in(&workspace), root.path(), &["install"]);

    assert_success(&second);
    let printed = format!("{}{}", stdout(&second), stderr(&second));
    assert!(printed.contains("Already up to date"), "the short-circuit ran:\n{printed}");
    assert_contains(&stderr(&second), WARNING);
}

/// Inside a pnpm workspace the field is redundant rather than misleading:
/// `pnpm-workspace.yaml` selects the projects, and the install links them.
#[test]
fn a_workspaces_field_inside_a_pnpm_workspace_stays_quiet() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(
        &workspace,
        r#"{"name":"converted","version":"1.0.0","private":true,"workspaces":["packages/*"]}"#,
    );
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - .\n")
        .expect("write pnpm-workspace.yaml");

    let output = run(pacquet, root.path(), &["install", "--lockfile-only"]);

    assert_success(&output);
    assert_quiet(&output);
}

/// Yarn's object spelling is the documented limit of the check: pnpm 11
/// does not warn about it either, and the two are kept aligned.
#[test]
fn an_object_form_workspaces_field_stays_quiet() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(
        &workspace,
        r#"{"name":"converted","version":"1.0.0","private":true,"workspaces":{"packages":["packages/*"]}}"#,
    );

    let output = run(pacquet, root.path(), &["install", "--lockfile-only"]);

    assert_success(&output);
    assert_quiet(&output);
}

fn write_manifest(workspace: &Path, contents: &str) {
    fs::write(workspace.join("package.json"), contents).expect("write package.json");
}

/// A fresh command per run: [`CommandTempCwd`] hands out one, and the
/// repeat-install test needs a second.
fn pacquet_in(workspace: &Path) -> Command {
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
    command.args(args).output().expect("run pacquet")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command should succeed\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output),
    );
}

fn assert_quiet(output: &Output) {
    let stderr = stderr(output);
    assert!(!stderr.contains("workspaces"), "expected no workspaces warning; got:\n{stderr}");
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
