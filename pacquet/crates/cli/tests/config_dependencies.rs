pub mod _utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
#[cfg(unix)]
use pacquet_testing_utils::fs::is_symlink_or_junction;
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

/// An `updateConfig` pnpmfile hook mutates the resolved config before
/// the install runs: a hook that flips `nodeLinker` to `hoisted` changes
/// the on-disk layout (the dependency becomes a real directory rather
/// than a symlink into the virtual store).
#[test]
fn update_config_hook_mutates_config_before_install() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@pnpm.e2e/foo": "100.0.0" } }).to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) {\n  config.nodeLinker = 'hoisted';\n  return config;\n} } }",
    )
    .expect("write .pnpmfile.cjs");

    pacquet_at(&workspace).with_arg("install").assert().success();

    let dep = workspace.join("node_modules/@pnpm.e2e/foo");
    assert!(dep.join("package.json").exists(), "dependency is installed");
    // On Unix, hoisted linking materializes the dep as a real directory
    // (isolated would symlink it), which proves the hook flipped
    // `nodeLinker`. Windows top-level deps are junctions under both
    // linkers — see `hoisted_node_linker.rs`'s `#![cfg(unix)]` gate — so
    // the cross-platform proof that `updateConfig` ran lives in
    // `update_config_hook_injects_catalog`.
    #[cfg(unix)]
    assert!(
        !is_symlink_or_junction(&dep).unwrap(),
        "updateConfig forced nodeLinker: hoisted, so the dep is a real directory, not a symlink",
    );

    drop((root, mock_instance));
}

/// `pacquet add --config <pkg>@<version>` resolves and installs the
/// package as a configurational dependency, writing the clean specifier
/// to `pnpm-workspace.yaml` and linking it under `.pnpm-config`.
#[test]
fn add_config_writes_workspace_yaml_and_installs() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), serde_json::json!({}).to_string())
        .expect("write package.json");

    pacquet_at(&workspace)
        .with_arg("add")
        .with_arg("--config")
        .with_arg("@pnpm.e2e/foo@100.0.0")
        .assert()
        .success();

    let yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read yaml");
    eprintln!("pnpm-workspace.yaml:\n{yaml}");
    assert!(yaml.contains("configDependencies:"), "configDependencies block written");
    assert!(yaml.contains("@pnpm.e2e/foo"));
    assert!(yaml.contains("100.0.0"));
    // The pre-existing storeDir setting must survive the format-preserving edit.
    assert!(yaml.contains("storeDir:"), "untouched settings are preserved");

    assert!(
        workspace.join("node_modules/.pnpm-config/@pnpm.e2e/foo/package.json").exists(),
        "config dep linked into .pnpm-config",
    );

    drop((root, mock_instance));
}

/// An `updateConfig` hook can inject a `catalogs` entry that the install
/// then resolves a `catalog:` specifier against — even though
/// `pnpm-workspace.yaml` declares no catalog. Without the hook the
/// `catalog:` dependency would have no entry to resolve to.
#[test]
fn update_config_hook_injects_catalog() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@pnpm.e2e/foo": "catalog:" } }).to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) {\n  config.catalogs = { default: { '@pnpm.e2e/foo': '100.0.0' } };\n  return config;\n} } }",
    )
    .expect("write .pnpmfile.cjs");

    pacquet_at(&workspace).with_arg("install").assert().success();

    assert!(
        workspace.join("node_modules/.pnpm/@pnpm.e2e+foo@100.0.0").exists(),
        "the catalog: dep resolved to the version the updateConfig hook injected",
    );

    drop((root, mock_instance));
}
