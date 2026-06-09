pub mod _utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

/// `pacquet install` resolves the `configDependencies` declared in
/// `pnpm-workspace.yaml`, links them under `node_modules/.pnpm-config`,
/// and records them in the env lockfile (the first document of
/// `pnpm-lock.yaml`).
#[test]
fn installs_configurational_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), serde_json::json!({}).to_string())
        .expect("write package.json");

    // Append a configDependencies block to the workspace manifest the
    // mocked-registry helper already wrote.
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    yaml.push_str("\nconfigDependencies:\n  '@pnpm.e2e/foo': 100.0.0\n");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");

    pacquet.with_arg("install").assert().success();

    let installed = workspace.join("node_modules/.pnpm-config/@pnpm.e2e/foo/package.json");
    assert!(installed.exists(), "config dep must be linked under .pnpm-config");

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(lockfile.starts_with("---\n"), "env document must lead pnpm-lock.yaml");
    assert!(lockfile.contains("configDependencies:"));
    assert!(lockfile.contains("@pnpm.e2e/foo"));

    drop((root, mock_instance));
}

/// A second `pacquet install` with the env lockfile already in place is
/// a no-op for config deps — it must still succeed and keep the link.
#[test]
fn second_install_keeps_config_dependency() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), serde_json::json!({}).to_string())
        .expect("write package.json");
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    yaml.push_str("\nconfigDependencies:\n  '@pnpm.e2e/foo': 100.0.0\n");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");

    pacquet_at(&workspace).with_arg("install").assert().success();
    pacquet_at(&workspace).with_arg("install").assert().success();

    assert!(
        workspace.join("node_modules/.pnpm-config/@pnpm.e2e/foo/package.json").exists(),
        "config dep must remain linked after a repeat install",
    );

    drop((root, mock_instance));
}
