use super::{global_command, prepare_global_home};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};

#[test]
fn global_install_and_update_recommend_global_peer_checks() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let home = root.path().join("pnpm-home");
    prepare_global_home(&home, &npmrc_info);
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let hint = r#"Issues with peer dependencies found. Run "pnpm peers check -g" to list them."#;
    let output = global_command(&workspace, &home)
        .with_args([
            "add",
            "-g",
            "@pnpm.e2e/has-foo100-peer@1.0.0,@pnpm.e2e/foo@2.0.0",
            "--ignore-scripts",
        ])
        .assert()
        .success();
    assert!(String::from_utf8_lossy(&output.get_output().stdout).contains(hint));
    let output = global_command(&workspace, &home)
        .with_args(["update", "--global", "--ignore-scripts"])
        .assert()
        .success();
    assert!(String::from_utf8_lossy(&output.get_output().stdout).contains(hint));
    let output = global_command(&workspace, &home)
        .with_args(["peers", "check", "-g"])
        .assert()
        .code(1);
    assert!(
        String::from_utf8_lossy(&output.get_output().stdout).contains("✕ unmet peer @pnpm.e2e/foo")
    );
    drop((root, mock_instance));
}
