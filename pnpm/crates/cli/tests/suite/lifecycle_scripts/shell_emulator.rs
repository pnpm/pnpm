use super::workspace_yaml::{allow_builds, append_workspace_yaml_key};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

fn emulate_instead_of(workspace: &Path) {
    append_workspace_yaml_key(workspace, "scriptShell", workspace.join("no-such-shell").display());
    append_workspace_yaml_key(workspace, "shellEmulator", true);
}

#[test]
fn runs_the_projects_own_scripts_and_dev_preinstall() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "name": "project-under-the-shell-emulator",
        "version": "1.0.0",
        "scripts": {
            "pnpm:devPreinstall": "echo ran > dev-preinstall.txt",
            "postinstall": "echo ran > postinstall.txt",
        },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    emulate_instead_of(&workspace);

    pacquet.with_arg("install").assert().success();

    assert!(workspace.join("dev-preinstall.txt").exists(), "pnpm:devPreinstall must run");
    assert!(workspace.join("postinstall.txt").exists(), "the postinstall must run");

    drop((root, mock_instance));
}

#[test]
fn runs_dependency_build_scripts() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    allow_builds(&workspace, &[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]);
    emulate_instead_of(&workspace);

    pacquet.with_arg("install").assert().success();

    let pkg_dir = workspace.join(
        "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example",
    );
    assert!(pkg_dir.join("generated-by-preinstall.js").exists());
    assert!(pkg_dir.join("generated-by-postinstall.js").exists());

    drop((root, mock_instance));
}

#[test]
fn expands_braced_parameter_expansions_in_scripts() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "name": "emu-var-test",
        "version": "1.0.0",
        "scripts": {
            "test-braced": "node -e \"require('fs').writeFileSync('out.txt', process.argv[1])\" ${MY_VAR:-fallback}",
        },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    emulate_instead_of(&workspace);

    pacquet.with_arg("run").with_arg("test-braced").with_env("MY_VAR", "hello").assert().success();

    let out = fs::read_to_string(workspace.join("out.txt")).expect("read out.txt");
    assert_eq!(out, "hello");

    super::_utils::pacquet_in(&workspace)
        .with_arg("run")
        .with_arg("test-braced")
        .assert()
        .success();

    let out = fs::read_to_string(workspace.join("out.txt")).expect("read out.txt");
    assert_eq!(out, "fallback");

    drop((root, mock_instance));
}
