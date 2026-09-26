use assert_cmd::prelude::*;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn write_workspace(workspace: &Path, app: Value, settings: &str) {
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("packages:\n  - packages/*\n{settings}"),
    )
    .expect("write workspace manifest");
    for (name, manifest) in [("app", app), ("secret", secret_manifest())] {
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

fn install(workspace: &Path) -> Command {
    let mut command = Command::cargo_bin("pnpm").unwrap();
    command.current_dir(workspace);
    command.arg("install");
    command
}

const ERROR_CODE: &str = "ERR_PNPM_PRIVATE_WORKSPACE_PROD_DEP";

#[test]
fn disallow_private_prod_deps_fails_the_install() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_workspace(
        &workspace,
        json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "secret": "workspace:*" },
        }),
        "disallowPrivateProdDeps: true\n",
    );

    let output = install(&workspace).assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains(ERROR_CODE), "{stderr}");
    assert!(stderr.contains("app depends on private workspace package secret"), "{stderr}",);
}

#[test]
fn a_dev_dependency_on_a_private_package_is_allowed() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_workspace(
        &workspace,
        json!({
            "name": "app",
            "version": "1.0.0",
            "devDependencies": { "secret": "workspace:*" },
        }),
        "disallowPrivateProdDeps: true\n",
    );

    let output = install(&workspace).assert().success();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(!stderr.contains(ERROR_CODE), "{stderr}");
}

#[test]
fn a_private_package_may_depend_on_a_private_package() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_workspace(
        &workspace,
        json!({
            "name": "app",
            "version": "1.0.0",
            "private": true,
            "dependencies": { "secret": "workspace:*" },
        }),
        "disallowPrivateProdDeps: true\n",
    );

    let output = install(&workspace).assert().success();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(!stderr.contains(ERROR_CODE), "{stderr}");
}

/// Turning the setting on has to fail the next install, including one the
/// repeat-install shortcut would otherwise finish as already up to date.
#[test]
fn the_install_fails_after_the_setting_is_turned_on() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_workspace(
        &workspace,
        json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "secret": "workspace:*" },
        }),
        "",
    );

    install(&workspace).assert().success();

    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    yaml.push_str("disallowPrivateProdDeps: true\n");
    fs::write(&workspace_yaml, yaml).expect("write pnpm-workspace.yaml");

    let output = install(&workspace).assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains(ERROR_CODE), "{stderr}");
    assert!(stderr.contains("app depends on private workspace package secret"), "{stderr}",);
}
