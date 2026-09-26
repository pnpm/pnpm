use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::fs;

#[test]
fn errors_when_workspace_yaml_has_unresolved_env_var_in_string_setting() {
    let CommandTempCwd { pacquet, workspace, .. } = CommandTempCwd::init();
    fs::create_dir_all(&workspace).expect("create workspace dir");
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - .\nstoreDir: ${PNPM_TEST_NONEXISTENT_VAR_12345}\n",
    )
    .expect("write pnpm-workspace.yaml");

    let output = pacquet
        .with_arg("install")
        .output()
        .expect("run pacquet");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_CONFIG_UNRESOLVED_ENV_VAR"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("Failed to replace env in config: ${PNPM_TEST_NONEXISTENT_VAR_12345}"),
        "stderr:\n{stderr}",
    );
}

#[test]
fn errors_when_workspace_yaml_has_unresolved_env_var_in_typed_setting() {
    let CommandTempCwd { pacquet, workspace, .. } = CommandTempCwd::init();
    fs::create_dir_all(&workspace).expect("create workspace dir");
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - .\nnodeLinker: ${PNPM_TEST_NONEXISTENT_LINKER_12345}\n",
    )
    .expect("write pnpm-workspace.yaml");

    let output = pacquet
        .with_arg("install")
        .output()
        .expect("run pacquet");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_CONFIG_UNRESOLVED_ENV_VAR"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("Failed to replace env in config: ${PNPM_TEST_NONEXISTENT_LINKER_12345}"),
        "stderr:\n{stderr}",
    );
}
