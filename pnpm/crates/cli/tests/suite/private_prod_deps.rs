use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::CommandTempCwd,
    diagnostics::assert_diagnostic_contains,
};
use serde_json::{
    Value,
    json,
};
use std::{
    fs,
    path::Path,
    process::Command,
};

const ERROR_CODE: &str = "ERR_PNPM_PRIVATE_WORKSPACE_PROD_DEP";
const SETTING: &str = "disallowPrivateProdDeps: true\n";

fn write_workspace(workspace: &Path, app: &Value, settings: &str) {
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("packages:\n  - packages/*\n{settings}"),
    )
    .expect("write workspace manifest");
    for (name, manifest) in [("app", app), ("secret", &secret_manifest())] {
        let dir = workspace.join("packages").join(name);
        fs::create_dir_all(&dir).expect("create project dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }
}

fn secret_manifest() -> Value {
    json!({
        "name": "secret",
        "version": "1.0.0",
        "private": true,
    })
}

fn app_with(field: &str) -> Value {
    json!({
        "name": "app",
        "version": "1.0.0",
        field: { "secret": "workspace:*" },
    })
}

fn pnpm(dir: &Path, args: &[&str]) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(dir)
        .with_args(args)
}

fn assert_fails_with_the_private_dependency(command: &mut Command) {
    let output = command.assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert_diagnostic_contains(&stderr, ERROR_CODE);
    assert_diagnostic_contains(&stderr, "app depends on private workspace package secret");
}

fn assert_succeeds_without_the_error(command: &mut Command) -> String {
    let output = command.assert().success();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(!stderr.contains(ERROR_CODE), "{stderr}");
    String::from_utf8_lossy(&output.get_output().stdout).into_owned()
}

#[test]
fn disallow_private_prod_deps_fails_the_install() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_workspace(&workspace, &app_with("dependencies"), SETTING);

    assert_fails_with_the_private_dependency(&mut pnpm(&workspace, &["install"]));
}

#[test]
fn a_dev_dependency_on_a_private_package_is_allowed() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_workspace(&workspace, &app_with("devDependencies"), SETTING);

    assert_succeeds_without_the_error(&mut pnpm(&workspace, &["install"]));
}

#[test]
fn a_private_package_may_depend_on_a_private_package() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let mut app = app_with("dependencies");
    app["private"] = json!(true);
    write_workspace(&workspace, &app, SETTING);

    assert_succeeds_without_the_error(&mut pnpm(&workspace, &["install"]));
}

/// Turning the setting on fails the next install, including one the
/// repeat-install shortcut would otherwise finish as already up to date. The
/// setting comes from the environment so no file the shortcut reads changes.
#[test]
fn the_install_fails_after_the_setting_is_turned_on() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_workspace(&workspace, &app_with("dependencies"), "");

    pnpm(&workspace, &["install"]).assert().success();

    assert_fails_with_the_private_dependency(
        &mut pnpm(&workspace, &["install"])
            .with_env("PNPM_CONFIG_DISALLOW_PRIVATE_PROD_DEPS", "true"),
    );
}

/// A workspace that passed the check keeps the repeat-install shortcut.
#[test]
fn a_passing_workspace_stays_up_to_date() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_workspace(&workspace, &app_with("devDependencies"), SETTING);

    assert_succeeds_without_the_error(&mut pnpm(&workspace, &["install"]));

    let stdout = assert_succeeds_without_the_error(&mut pnpm(&workspace, &["install"]));
    assert!(stdout.contains("Already up to date"), "{stdout}");
}

/// `pnpm add` does not run the check, so the next install has to.
#[test]
fn the_install_after_an_add_runs_the_check() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let app = json!({ "name": "app", "version": "1.0.0" });
    write_workspace(&workspace, &app, SETTING);
    pnpm(&workspace, &["install"]).assert().success();

    let app_dir = workspace.join("packages/app");
    pnpm(&app_dir, &["add", "secret@workspace:*"]).assert().success();

    assert_fails_with_the_private_dependency(&mut pnpm(&workspace, &["install"]));
}
