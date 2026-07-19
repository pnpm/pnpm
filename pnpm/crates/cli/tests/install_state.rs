use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::{
    allow_known_failure,
    bin::{AddMockedRegistry, CommandTempCwd},
    known_failure::{KnownFailure, KnownResult},
};
use std::{fs, process::Command};

#[test]
fn frozen_reinstall_writes_modules_manifest_current_lockfile_and_bins() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/hello-world-js-bin":"1.0.0"}}"#,
    )
    .expect("write manifest");
    pacquet.with_arg("install").assert().success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    assert!(workspace.join("node_modules/.modules.yaml").exists());
    assert!(workspace.join("node_modules/.pnpm/lock.yaml").exists());
    assert!(workspace.join("node_modules/.bin/hello-world-js-bin").exists());

    drop((root, mock_instance));
}

fn pnp_without_symlinks() -> KnownResult<()> {
    Err(KnownFailure::new(
        "pacquet does not yet materialize the PnP loader and modules manifest for an install with symlinks disabled",
    ))
}

fn external_lockfile_public_hoist() -> KnownResult<()> {
    Err(KnownFailure::new(
        "pacquet does not yet expose the external lockfile and project-root split required by the headless public-hoist scenario",
    ))
}

#[test]
fn pnp_install_without_symlinks_still_writes_modules_manifest_and_bin_directory() {
    allow_known_failure!(pnp_without_symlinks());
}

#[test]
fn public_hoist_uses_the_project_root_when_the_lockfile_is_external() {
    allow_known_failure!(external_lockfile_public_hoist());
}
