use super::workspace_yaml::{allow_builds, append_workspace_yaml_key};
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::fs;

#[test]
fn install_names_the_missing_shell_when_a_dependency_build_script_fails() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    allow_builds(&workspace, &[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]);
    let missing_shell = workspace.join("no-such-shell");
    append_workspace_yaml_key(&workspace, "scriptShell", missing_shell.display());

    let output = pacquet
        .with_arg("install")
        .output()
        .expect("spawn pacquet install");
    let stderr = String::from_utf8_lossy(&output.stderr);
    // miette wraps the report at the terminal width, splitting the temp path.
    let unwrapped: String = stderr
        .chars()
        .filter(|&c| !c.is_whitespace() && c != '│')
        .collect();
    assert!(!output.status.success(), "the install must fail, got: {output:?}");
    assert!(
        unwrapped.contains("TheconfiguredscriptShellwasnotfound")
            && unwrapped.contains(&missing_shell.display().to_string()),
        "the error must name the configured scriptShell, got: {stderr}",
    );

    drop((root, mock_instance));
}
