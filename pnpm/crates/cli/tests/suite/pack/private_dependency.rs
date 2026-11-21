use super::read_manifest_from_tarball;
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

fn app_with(field: &str) -> Value {
    json!({
        "name": "app",
        "version": "1.0.0",
        field: { "secret": "workspace:*" },
    })
}

fn write_workspace(workspace: &Path, app: Value) {
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    let secret = json!({ "name": "secret", "version": "1.0.0", "private": true });
    for (name, manifest) in [("app", app), ("secret", secret)] {
        let dir = workspace.join("packages").join(name);
        fs::create_dir_all(&dir).expect("create project dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }
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
    assert_diagnostic_contains(&stderr, "ERR_PNPM_PRIVATE_WORKSPACE_DEPENDENCY");
    assert_diagnostic_contains(
        &stderr,
        r#""app" depends on the private workspace package "secret""#,
    );
}

#[test]
fn pack_rejects_a_private_workspace_package_in_dependencies() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_workspace(&workspace, app_with("dependencies"));
    pnpm(&workspace, &["install"]).assert().success();

    assert_fails_with_the_private_dependency(&mut pnpm(&workspace.join("packages/app"), &["pack"]));
}

/// Without an install the dependency is found among the workspace projects.
#[test]
fn recursive_pack_rejects_it_before_install() {
    let CommandTempCwd { root: _root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, app_with("dependencies"));

    assert_fails_with_the_private_dependency(&mut pnpm(&workspace, &["-r", "pack"]));
}

#[test]
fn pack_allows_a_private_workspace_package_in_dev_dependencies() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_workspace(&workspace, app_with("devDependencies"));
    pnpm(&workspace, &["install"]).assert().success();

    let app_dir = workspace.join("packages/app");
    pnpm(&app_dir, &["pack"]).assert().success();
    let manifest: Value = read_manifest_from_tarball(&app_dir.join("app-1.0.0.tgz"));
    assert_eq!(manifest["devDependencies"]["secret"], "1.0.0");
}

/// Pack decides what is bundled the way the tarball's file list does: from
/// `bundleDependencies`, or from `bundledDependencies` only when the first is
/// absent.
#[test]
fn pack_allows_a_bundled_private_workspace_package() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let mut bundled = app_with("dependencies");
    bundled["bundleDependencies"] = json!(["secret"]);
    write_workspace(&workspace, bundled);
    pnpm(&workspace, &["install"]).assert().success();
    pnpm(&workspace.join("packages/app"), &["pack"]).assert().success();

    let mut shadowed = app_with("dependencies");
    shadowed["bundleDependencies"] = json!([]);
    shadowed["bundledDependencies"] = json!(["secret"]);
    let app_dir = workspace.join("packages/app");
    fs::write(app_dir.join("package.json"), shadowed.to_string()).expect("write package.json");
    assert_fails_with_the_private_dependency(&mut pnpm(&app_dir, &["pack"]));
}
