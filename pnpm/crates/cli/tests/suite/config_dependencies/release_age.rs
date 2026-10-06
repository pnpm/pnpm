use super::pacquet_at;
use crate::_utils::set_minimum_release_age;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::fs;

/// `minimumReleaseAge` gates the versions resolution picks, and a config
/// dependency's version is the one the workspace declares. A clean install of
/// a locked config dependency therefore succeeds however recently it was
/// published, as it does when the config dependency is first added.
#[test]
fn frozen_install_does_not_hold_locked_config_dependencies_to_minimum_release_age() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), serde_json::json!({}).to_string())
        .expect("write package.json");
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    yaml.push_str("\nconfigDependencies:\n  '@pnpm.e2e/foo': 100.0.0\n");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");

    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    set_minimum_release_age(&workspace, 100 * 365 * 24 * 60);
    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert!(
        workspace.join("node_modules/.pnpm-config/@pnpm.e2e/foo/package.json").exists(),
        "the clean install links the locked config dependency",
    );

    drop((root, mock_instance));
}
