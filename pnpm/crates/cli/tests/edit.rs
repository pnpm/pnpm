use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{fs, process::Command};

fn pacquet(workspace: &std::path::Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

#[test]
fn edit_fails_without_package_name() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let output = pacquet(&workspace).with_arg("edit").output().expect("run pacquet edit");

    assert!(!output.status.success(), "edit without args should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required arguments were not provided"),
        "should show error about missing package name: {stderr}",
    );
    drop(root);
}

#[test]
fn edit_help_succeeds() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let output = pacquet(&workspace)
        .with_args(["edit", "--help"])
        .output()
        .expect("run pacquet edit --help");
    assert!(output.status.success(), "edit --help should succeed");
    drop(root);
}

#[test]
fn edit_dependency_successfully() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "is-positive": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet(&workspace).with_arg("install").assert().success();

    let pkg_dir = workspace.join("node_modules/is-positive");
    let index_js = pkg_dir.join("index.js");
    assert!(index_js.exists());

    // We will use a node script as a dummy editor to overwrite index.js
    let dummy_editor = r#"node -e "const fs = require('fs'); fs.writeFileSync(require('path').join(process.argv[1], 'index.js'), 'module.exports = () => \"modified\";');""#;

    let mut cmd = pacquet(&workspace);
    cmd.env("EDITOR", dummy_editor);
    cmd.with_args(["edit", "is-positive"]).assert().success();

    let content = fs::read_to_string(&index_js).unwrap();
    assert!(content.contains("modified"));

    drop(npmrc_info);
    drop(root);
}

#[test]
fn edit_dependency_fails_with_invalid_editor() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "is-positive": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet(&workspace).with_arg("install").assert().success();

    let mut cmd = pacquet(&workspace);
    cmd.env("EDITOR", "non_existent_editor_command_xyz");
    let output = cmd.with_args(["edit", "is-positive"]).output().unwrap();

    assert!(!output.status.success(), "edit must fail when editor is invalid");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Failed to execute editor command"),
        "stderr should contain editor execution failure: {stderr}",
    );

    drop(npmrc_info);
    drop(root);
}
