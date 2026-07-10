use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pretty_assertions::assert_eq;
use std::{fs, path::Path, process::Command};

#[test]
fn version_flag_prints_the_bare_version() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet.with_arg("--version").output().expect("run pacquet --version");
    dbg!(&output);
    assert!(output.status.success(), "pacquet --version should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", pacquet_config::PNPM_VERSION),
    );

    drop(root);
}

#[test]
fn short_version_flag_prints_the_bare_version() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet.with_arg("-v").output().expect("run pacquet -v");
    dbg!(&output);
    assert!(output.status.success(), "pacquet -v should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", pacquet_config::PNPM_VERSION),
    );

    drop(root);
}

#[test]
fn version_flag_switches_to_project_package_manager_version() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(workspace.join("package.json"), r#"{"packageManager":"pnpm@9.3.0"}"#)
        .expect("write package.json");

    let output = test_command(pacquet, root.path())
        .env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .args(["--version"])
        .output()
        .expect("run pacquet --version");
    dbg!(&output);
    assert!(output.status.success(), "pacquet --version should succeed");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "9.3.0\n");

    drop((root, mock_instance));
}

fn test_command(mut command: Command, root: &Path) -> Command {
    command.env("PNPM_HOME", root.join("pnpm-home"));
    command.env("HOME", root);
    command.env("XDG_CONFIG_HOME", root.join("xdg-config"));
    command.env_remove("COREPACK_ROOT");
    command.env_remove("pnpm_config_pm_on_fail");
    command.env_remove("PNPM_CONFIG_PM_ON_FAIL");
    command
}
