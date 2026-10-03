use super::{super::workspace_yaml::append_workspace_yaml_key, project_with_lifecycle_scripts};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, process::Command};

/// With `optimisticRepeatInstall: false`, a repeat install whose tree is
/// already up to date still runs the project's own lifecycle scripts.
/// <https://github.com/pnpm/pnpm/issues/16545>
#[test]
fn up_to_date_repeat_install_runs_project_lifecycle_scripts() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let mut manifest = project_with_lifecycle_scripts();
    manifest["dependencies"] = serde_json::json!({ "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0" });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
    append_workspace_yaml_key(&workspace, "optimisticRepeatInstall", false);

    pacquet
        .with_arg("install")
        .assert()
        .success();
    fs::remove_file(workspace.join("order.txt")).expect("clear the first install's order.txt");

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_arg("install")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&output);
    eprintln!("STDOUT:\n{stdout}");
    assert!(
        stdout.contains("Lockfile is up to date"),
        "the repeat install must take the up-to-date path this test covers",
    );

    let order = fs::read_to_string(workspace.join("order.txt")).expect("read order.txt");
    let stages: Vec<&str> = order.lines().collect();
    assert_eq!(
        stages,
        ["preinstall", "install", "postinstall", "preprepare", "prepare", "postprepare"],
    );

    drop((root, mock_instance));
}
