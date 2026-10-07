use super::pacquet_at;
use crate::_utils::{
    append_workspace_yaml_key, bravo_dep_mature_up_to_1_0_1_minimum_release_age,
    set_minimum_release_age,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

fn declare_config_dependency(workspace: &Path, specifier: &str) {
    declare_named_config_dependency(workspace, "@pnpm.e2e/bravo-dep", specifier);
}

fn declare_named_config_dependency(workspace: &Path, name: &str, specifier: &str) {
    fs::write(workspace.join("package.json"), serde_json::json!({}).to_string())
        .expect("write package.json");
    append_workspace_yaml_key(
        workspace,
        "configDependencies",
        format!("{{'{name}': '{specifier}'}}"),
    );
}

fn installed_bravo_dep_version(workspace: &Path) -> String {
    let manifest = fs::read_to_string(workspace.join(
        "node_modules/.pnpm-config/@pnpm.e2e/bravo-dep/package.json",
    ))
    .expect("read the installed config dependency");
    let manifest: serde_json::Value = serde_json::from_str(&manifest).expect("parse package.json");
    manifest["version"]
        .as_str()
        .expect("version")
        .to_string()
}

/// Resolving a config dependency picks a version that the frozen install's
/// registry verification accepts.
#[test]
fn config_dependency_resolution_skips_versions_newer_than_minimum_release_age() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    declare_config_dependency(&workspace, "^1.0.0");
    set_minimum_release_age(&workspace, bravo_dep_mature_up_to_1_0_1_minimum_release_age());

    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(installed_bravo_dep_version(&workspace), "1.0.1");

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_eq!(installed_bravo_dep_version(&workspace), "1.0.1");

    drop((root, mock_instance));
}

#[test]
fn config_dependency_newer_than_minimum_release_age_is_rejected() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    declare_config_dependency(&workspace, "1.1.0");
    set_minimum_release_age(&workspace, bravo_dep_mature_up_to_1_0_1_minimum_release_age());

    let output = pacquet_at(&workspace)
        .with_arg("install")
        .output()
        .expect("run pnpm install");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("minimumReleaseAge"), "stderr: {stderr}");

    drop((root, mock_instance));
}

/// A config dependency is resolved and verified before any config
/// dependency's `updateConfig` hook runs, so only `pnpm-workspace.yaml` can
/// exempt it.
#[test]
fn minimum_release_age_exclude_in_workspace_yaml_admits_a_config_dependency() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    declare_config_dependency(&workspace, "1.1.0");
    set_minimum_release_age(&workspace, bravo_dep_mature_up_to_1_0_1_minimum_release_age());
    append_workspace_yaml_key(&workspace, "minimumReleaseAgeExclude", "['@pnpm.e2e/bravo-dep']");

    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_eq!(installed_bravo_dep_version(&workspace), "1.1.0");

    drop((root, mock_instance));
}

#[test]
fn config_dependency_with_an_optional_dependency_newer_than_minimum_release_age_is_rejected() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    declare_named_config_dependency(&workspace, "@pnpm.e2e/optional-platform-selector", "2.0.0");
    set_minimum_release_age(&workspace, 100 * 365 * 24 * 60);
    append_workspace_yaml_key(
        &workspace,
        "minimumReleaseAgeExclude",
        "['@pnpm.e2e/optional-platform-selector']",
    );

    let output = pacquet_at(&workspace)
        .with_arg("install")
        .output()
        .expect("run pnpm install");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("minimumReleaseAge"), "stderr: {stderr}");

    drop((root, mock_instance));
}
