//! The install-family handling of Yarn's `workspaces` field in the root
//! manifest: the field is converted into a `pnpm-workspace.yaml` on the
//! first install, so a repository migrated from Yarn or npm links its
//! projects instead of silently installing as a single one. An existing
//! `pnpm-workspace.yaml` always wins, and `--ignore-workspace` keeps the
//! project standalone.

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

/// The notice the converting install prints, as it reaches a terminal,
/// label included.
const CREATED: &str =
    r#"[WARN] Created "pnpm-workspace.yaml" from the "workspaces" field in package.json."#;

#[test]
fn an_install_creates_a_workspace_yaml_from_the_workspaces_field() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(
        &workspace,
        r#"{"name":"converted","version":"1.0.0","private":true,"workspaces":["packages/*","apps/web"]}"#,
    );

    let output = run(pacquet, root.path(), &["install", "--lockfile-only"]);

    assert_success(&output);
    assert_contains(&stderr(&output), CREATED);
    let created =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("workspace yaml created");
    assert_eq!(created, "packages:\n  - packages/*\n  - apps/web\n");
}

/// The install keeps the generated file and the converted repository no
/// longer warns about the field: the generated `pnpm-workspace.yaml` now
/// selects the projects, so the field is redundant rather than ignored.
#[test]
fn a_converted_repository_warns_once_then_stays_quiet() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_manifest(
        &workspace,
        r#"{"name":"converted","version":"1.0.0","private":true,"workspaces":["packages/*"]}"#,
    );

    let first = run(pacquet_in(&workspace), root.path(), &["install", "--lockfile-only"]);
    assert_success(&first);
    assert_contains(&stderr(&first), CREATED);

    let second = run(pacquet_in(&workspace), root.path(), &["install", "--lockfile-only"]);
    assert_success(&second);
    assert_quiet(&second);
}

/// The second install of an unchanged project never reaches the full
/// install path; the generated workspace manifest has to keep the
/// up-to-date short-circuit quiet about the field as well.
#[test]
fn a_repeat_install_taking_the_up_to_date_path_stays_quiet() {
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
    assert_quiet(&second);
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

/// A hand-authored `pnpm-workspace.yaml` always wins: the install neither
/// overwrites it nor adds the field's patterns to it.
#[test]
fn an_existing_workspace_yaml_is_left_untouched() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(
        &workspace,
        r#"{"name":"converted","version":"1.0.0","private":true,"workspaces":["packages/*"]}"#,
    );
    let authored = "packages:\n  - .\n";
    fs::write(workspace.join("pnpm-workspace.yaml"), authored).expect("write pnpm-workspace.yaml");

    let output = run(pacquet, root.path(), &["install", "--lockfile-only"]);

    assert_success(&output);
    assert_quiet(&output);
    let kept =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("workspace yaml kept");
    assert_eq!(kept, authored);
}

/// `--ignore-workspace` means "standalone project": the install does not
/// create a workspace manifest the user just asked it to ignore.
#[test]
fn an_ignored_workspace_creates_no_workspace_yaml() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(
        &workspace,
        r#"{"name":"converted","version":"1.0.0","private":true,"workspaces":["packages/*"]}"#,
    );

    let output = run(pacquet, root.path(), &["install", "--lockfile-only", "--ignore-workspace"]);

    assert_success(&output);
    assert!(
        !workspace.join("pnpm-workspace.yaml").exists(),
        "--ignore-workspace must not create pnpm-workspace.yaml",
    );
}

/// A `workspaces` array whose entries are not usable patterns still gets
/// pnpm 11's warning: the field names projects pnpm cannot find, and the
/// notice explains why none of them linked.
#[test]
fn an_array_without_usable_patterns_keeps_the_unsupported_field_warning() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(
        &workspace,
        r#"{"name":"converted","version":"1.0.0","private":true,"workspaces":["",1]}"#,
    );

    let output = run(pacquet, root.path(), &["install", "--lockfile-only"]);

    assert_success(&output);
    assert_contains(
        &stderr(&output),
        r#"[WARN] The "workspaces" field in package.json is not supported by pnpm. Create a "pnpm-workspace.yaml" file instead."#,
    );
    assert!(
        !workspace.join("pnpm-workspace.yaml").exists(),
        "no usable pattern must not create pnpm-workspace.yaml",
    );
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
    command
        .args(args)
        .output()
        .expect("run pacquet")
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
