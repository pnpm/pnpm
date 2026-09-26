use crate::_utils::{append_workspace_yaml_key, read_lockfile};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

// <https://github.com/pnpm/pnpm/issues/11800>
#[test]
fn auto_installed_peer_follows_the_version_another_project_moves_to() {
    assert_lib_peer_after_app_bump(">=1.0.0", "2.0.0", &[], "2.0.0");
}

#[test]
fn auto_installed_peer_follows_a_range_another_project_moves_to() {
    assert_lib_peer_after_app_bump(">=1.0.0", "^3.0.0", &[], "3.1.0");
}

#[test]
fn auto_installed_peer_keeps_its_pin_when_no_project_version_fits_its_range() {
    assert_lib_peer_after_app_bump("^1.0.0", "2.0.0", &[], "1.0.0");
}

#[test]
fn filtered_install_of_both_projects_moves_the_auto_installed_peer() {
    assert_lib_peer_after_app_bump(
        ">=1.0.0",
        "2.0.0",
        &["--filter", "app", "--filter", "lib"],
        "2.0.0",
    );
}

#[test]
fn filtered_install_of_the_providing_project_keeps_an_unselected_projects_pin() {
    assert_lib_peer_after_app_bump(">=1.0.0", "2.0.0", &["--filter", "app"], "1.0.0");
}

fn assert_lib_peer_after_app_bump(
    peer_range: &str,
    bumped_app_spec: &str,
    bump_install_filters: &[&str],
    expected: &str,
) {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "packages", "['app', 'lib']");
    write_manifest(
        &workspace.join("lib"),
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "peerDependencies": { "is-positive": peer_range },
        }),
    );

    write_app_manifest(&workspace, "1.0.0");
    install_lockfile_only(&workspace, &[]);
    assert_eq!(locked_version(&workspace, "lib"), "1.0.0");

    write_app_manifest(&workspace, bumped_app_spec);
    install_lockfile_only(&workspace, bump_install_filters);
    assert_eq!(locked_version(&workspace, "lib"), expected);

    drop((root, mock_instance));
}

fn write_app_manifest(workspace: &Path, spec: &str) {
    write_manifest(
        &workspace.join("app"),
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "is-positive": spec },
        }),
    );
}

fn write_manifest(dir: &Path, manifest: &serde_json::Value) {
    fs::create_dir_all(dir).expect("create project dir");
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
}

fn install_lockfile_only(workspace: &Path, filters: &[&str]) {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_args(["install", "--lockfile-only"])
        .with_args(filters)
        .assert()
        .success();
}

fn locked_version(workspace: &Path, importer_id: &str) -> String {
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    lockfile.importers[importer_id].dependencies
        .as_ref()
        .expect("importer dependencies")
        .get(&"is-positive".parse().expect("package name"))
        .expect("is-positive importer entry")
        .version
        .to_string()
}
