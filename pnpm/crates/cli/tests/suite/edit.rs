use super::_utils::{append_workspace_yaml_key, enable_gvs_in_workspace_yaml};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{fs, process::Command};

fn pacquet(workspace: &std::path::Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

#[test]
fn edit_fails_without_package_name() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let output = pacquet(&workspace)
        .with_arg("edit")
        .output()
        .expect("run pacquet edit");

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

    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let pkg_dir = workspace.join("node_modules/is-positive");
    let index_js = pkg_dir.join("index.js");
    assert!(index_js.exists());

    let dummy_editor = r#"node -e "const fs = require('fs'); fs.writeFileSync(require('path').join(process.argv[1], 'index.js'), 'module.exports = () => \"modified\";');""#;

    let mut cmd = pacquet(&workspace);
    cmd.env("EDITOR", dummy_editor);
    cmd.with_args(["edit", "is-positive"])
        .assert()
        .success();

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

    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let mut cmd = pacquet(&workspace);
    cmd.env("EDITOR", "non_existent_editor_command_xyz");
    let output = cmd
        .with_args(["edit", "is-positive"])
        .output()
        .unwrap();

    assert!(!output.status.success(), "edit must fail when editor is invalid");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Failed to execute editor command"),
        "stderr should contain editor execution failure: {stderr}",
    );

    drop(npmrc_info);
    drop(root);
}

#[test]
fn edit_rejects_project_local_editor() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"test-project"}"#).unwrap();
    let package_dir = workspace.join("node_modules/test-dependency");
    fs::create_dir_all(&package_dir).unwrap();
    fs::write(package_dir.join("package.json"), r#"{"name":"test-dependency"}"#).unwrap();
    let executable = workspace.join(if cfg!(windows) {
        "pnpm-edit-project-editor.cmd"
    } else {
        "pnpm-edit-project-editor"
    });
    fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = pacquet(&workspace)
        .with_args(["edit", "test-dependency", "--editor", "pnpm-edit-project-editor"])
        .with_env("PATH", &workspace)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("outside the project was found"), "{stderr}");
    drop(root);
}

#[test]
fn edit_fails_for_package_that_is_not_installed() {
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

    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let output = pacquet(&workspace)
        .with_args(["edit", "missing-dependency"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "edit must fail for missing package");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Could not find package"), "{stderr}");

    drop(npmrc_info);
    drop(root);
}

#[test]
fn edit_rejects_path_traversal() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"test-project"}"#).unwrap();
    for traversal in ["../outside", "foo/../bar", ".."] {
        let output = pacquet(&workspace)
            .with_args(["edit", traversal])
            .output()
            .unwrap();
        assert!(!output.status.success(), "edit {traversal} must fail");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("Invalid package path segment"), "{traversal}: {stderr}");
    }
    drop(root);
}

#[test]
fn edit_flag_editor_overrides_env_and_accepts_arguments() {
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

    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let dummy_editor = r#"node -e "const fs = require('fs'); fs.writeFileSync(require('path').join(process.argv[1], 'index.js'), 'module.exports = () => \"flag-edited\";');""#;
    let mut cmd = pacquet(&workspace);
    cmd.env("EDITOR", "non_existent_editor_command_xyz");
    cmd.with_args(["edit", "is-positive", "--editor", dummy_editor])
        .assert()
        .success();

    let content = fs::read_to_string(workspace.join("node_modules/is-positive/index.js")).unwrap();
    assert!(content.contains("flag-edited"), "{content}");

    drop(npmrc_info);
    drop(root);
}

#[test]
fn edit_fails_when_editor_exits_non_zero() {
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

    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let mut cmd = pacquet(&workspace);
    cmd.env("EDITOR", r#"node -e "process.exit(1)""#);
    let output = cmd
        .with_args(["edit", "is-positive"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "edit must fail on editor failure");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Editor command exited with failure status"), "{stderr}");

    drop(npmrc_info);
    drop(root);
}

#[test]
fn edit_fails_with_empty_editor() {
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

    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let output = pacquet(&workspace)
        .with_args(["edit", "is-positive", "--editor", ""])
        .output()
        .unwrap();
    assert!(!output.status.success(), "edit must fail with empty editor");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No editor command specified"), "{stderr}");

    drop(npmrc_info);
    drop(root);
}

#[test]
fn edit_uses_configured_editor() {
    for source in ["workspace", "environment", "cli"] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        fs::write(workspace.join("package.json"), r#"{"name":"test-project"}"#).unwrap();
        let package_dir = workspace.join("node_modules/test-dependency");
        fs::create_dir_all(&package_dir).unwrap();
        fs::write(package_dir.join("package.json"), r#"{"name":"test-dependency"}"#).unwrap();
        let editor = "pnpm-edit-configured-editor-missing";
        let mut command = pacquet(&workspace).with_args(["edit", "test-dependency"]);
        command.env("EDITOR", "pnpm-edit-fallback-editor-missing");
        match source {
            "workspace" => {
                fs::write(workspace.join("pnpm-workspace.yaml"), format!("editor: {editor}\n"))
                    .unwrap();
            }
            "environment" => {
                command.env("PNPM_CONFIG_EDITOR", editor);
            }
            "cli" => {
                command.arg(format!("--config.editor={editor}"));
            }
            _ => unreachable!(),
        }
        let output = command.output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success());
        assert!(stderr.contains(editor), "{source}: {stderr}");
        drop(root);
    }
}

#[test]
fn edit_rebuilds_an_aliased_dependency_and_preserves_store_content() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    fs::write(workspace.join("package.json"), serde_json::json!({"dependencies": {"alias": "npm:@pnpm.e2e/install-script-example@1.0.0"}}).to_string()).unwrap();
    append_workspace_yaml_key(
        &workspace,
        "allowBuilds",
        "\n  '@pnpm.e2e/install-script-example': true",
    );
    append_workspace_yaml_key(&workspace, "packageImportMethod", "hardlink");
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let package_dir = workspace.join("node_modules/alias");
    let marker = package_dir.join("generated-by-install.js");
    assert!(marker.exists());
    fs::remove_file(&marker).unwrap();
    let original = fs::read(package_dir.join("package.json")).unwrap();
    let store_copy = root.path().join("store-copy.json");
    fs::hard_link(package_dir.join("package.json"), &store_copy).unwrap();
    let editor = r#"node -e "const fs = require('fs'); const p = require('path').join(process.argv[1], 'package.json'); fs.appendFileSync(p, '\n');""#;
    pacquet(&workspace)
        .with_args(["edit", "alias", "--editor", editor])
        .assert()
        .success();
    assert!(marker.exists(), "edit did not rebuild the aliased package");
    assert_eq!(fs::read(&store_copy).unwrap(), original);
    assert_ne!(fs::read(package_dir.join("package.json")).unwrap(), original);
    drop(npmrc_info);
    drop(root);
}

#[test]
fn edit_ignores_empty_editor_environment_values() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({"dependencies": {"is-positive": "1.0.0"}}).to_string(),
    )
    .unwrap();
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let editor = r#"node -e "require('fs').writeFileSync(require('path').join(process.argv[1], 'edited'), 'yes');""#;
    for empty in ["", "   "] {
        pacquet(&workspace)
            .with_args(["edit", "is-positive"])
            .with_env("EDITOR", empty)
            .with_env("VISUAL", editor)
            .assert()
            .success();
        let marker = workspace.join("node_modules/is-positive/edited");
        assert_eq!(fs::read_to_string(&marker).unwrap(), "yes");
        fs::remove_file(marker).unwrap();
    }
    drop(npmrc_info);
    drop(root);
}

#[test]
fn edit_rebuilds_only_the_selected_snapshot() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let package = "@pnpm.e2e/pre-and-postinstall-scripts-example";
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({"dependencies": {
            "first": format!("npm:{package}@1.0.0"), "second": format!("npm:{package}@2.0.0")
        }})
        .to_string(),
    )
    .unwrap();
    append_workspace_yaml_key(&workspace, "allowBuilds", format!("\n  '{package}': true"));
    append_workspace_yaml_key(&workspace, "sideEffectsCache", false);
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let first_bin = workspace.join("node_modules/first/generated-by-postinstall.js");
    let second_bin = workspace.join("node_modules/second/generated-by-postinstall.js");
    assert!(first_bin.exists());
    assert!(second_bin.exists());
    fs::remove_file(&first_bin).unwrap();
    fs::remove_file(&second_bin).unwrap();
    pacquet(&workspace)
        .with_args(["edit", "first", "--editor", r#"node -e "process.exit(0)""#])
        .assert()
        .success();
    assert!(first_bin.exists(), "the edited snapshot was not rebuilt");
    assert!(!second_bin.exists(), "the unedited snapshot was rebuilt");
    pacquet(&workspace)
        .with_args(["rebuild", package])
        .assert()
        .success();
    assert!(second_bin.exists(), "name-based rebuild no longer selected the other version");
    drop(npmrc_info);
    drop(root);
}

#[test]
fn edit_rejects_shared_virtual_store_without_modifying_the_package() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({"dependencies": {"is-positive": "1.0.0"}}).to_string(),
    )
    .unwrap();
    enable_gvs_in_workspace_yaml(&workspace, "globalVirtualStoreDir: ../shared-store\n");
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let package = workspace.join("node_modules/is-positive");
    let original = fs::read(package.join("index.js")).unwrap();
    let editor = r#"node -e "require('fs').writeFileSync(require('path').join(process.argv[1], 'index.js'), 'edited');""#;
    let output = pacquet(&workspace)
        .with_args(["edit", "is-positive", "--editor", editor])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("shared virtual store"), "{stderr}");
    assert!(stderr.contains("pnpm patch"), "{stderr}");
    assert_eq!(fs::read(package.join("index.js")).unwrap(), original);
    drop(npmrc_info);
    drop(root);
}

#[test]
fn edit_does_not_publish_modified_files_to_the_shared_build_cache() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({"dependencies": {"@pnpm.e2e/install-script-example": "1.0.0"}})
            .to_string(),
    )
    .unwrap();
    append_workspace_yaml_key(
        &workspace,
        "allowBuilds",
        "\n  '@pnpm.e2e/install-script-example': true",
    );
    append_workspace_yaml_key(&workspace, "sideEffectsCache", true);
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let package = workspace.join("node_modules/@pnpm.e2e/install-script-example");
    let original = fs::read(package.join("create.js")).unwrap();
    let editor = r#"node -e "require('fs').writeFileSync(require('path').join(process.argv[1], 'create.js'), 'require(\"fs\").writeFileSync(\"generated-by-install.js\", \"edited\");');""#;
    pacquet(&workspace)
        .with_args(["edit", "@pnpm.e2e/install-script-example", "--editor", editor])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(package.join("generated-by-install.js")).unwrap(), "edited");
    assert_ne!(fs::read(package.join("create.js")).unwrap(), original);
    let other = root.path().join("other-project");
    fs::create_dir(&other).unwrap();
    for name in ["package.json", "pnpm-workspace.yaml", ".npmrc"] {
        fs::copy(workspace.join(name), other.join(name)).unwrap();
    }
    pacquet(&other)
        .with_args(["install", "--offline"])
        .assert()
        .success();
    let other_package = other.join("node_modules/@pnpm.e2e/install-script-example");
    assert_eq!(fs::read(other_package.join("create.js")).unwrap(), original);
    assert_ne!(
        fs::read_to_string(other_package.join("generated-by-install.js")).unwrap(),
        "edited",
    );
    drop(npmrc_info);
    drop(root);
}

#[test]
fn edit_rejects_linked_workspace_dependencies() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n").unwrap();
    fs::create_dir_all(workspace.join("packages/linked")).unwrap();
    fs::write(
        workspace.join("packages/linked/package.json"),
        r#"{"name":"linked","version":"1.0.0"}"#,
    )
    .unwrap();
    fs::write(workspace.join("package.json"), r#"{"dependencies":{"linked":"workspace:*"}}"#)
        .unwrap();
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let output = pacquet(&workspace)
        .with_args(["edit", "linked", "--editor", "node -e 'process.exit(0)'"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("outside the expected"), "{stderr}");
    drop(root);
}

#[cfg(unix)]
#[test]
fn edit_preserves_an_unreadable_hardlink_when_detaching_fails() {
    use std::os::unix::fs::PermissionsExt;
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    fs::write(workspace.join("package.json"), r#"{"dependencies":{"is-positive":"1.0.0"}}"#)
        .unwrap();
    append_workspace_yaml_key(&workspace, "packageImportMethod", "hardlink");
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let package_file = workspace.join("node_modules/is-positive/index.js");
    let original = fs::read(&package_file).unwrap();
    let store_copy = root.path().join("store-copy.js");
    fs::hard_link(&package_file, &store_copy).unwrap();
    let permissions = fs::metadata(&package_file).unwrap().permissions();
    fs::set_permissions(&package_file, fs::Permissions::from_mode(0o000)).unwrap();
    // Privileged processes can read mode-000 files, so they cannot exercise this failure.
    if fs::File::open(&package_file).is_ok() {
        fs::set_permissions(&store_copy, permissions).unwrap();
        return;
    }
    let output = pacquet(&workspace)
        .with_args(["edit", "is-positive", "--editor", "node -e 'process.exit(0)'"])
        .output()
        .unwrap();
    fs::set_permissions(&store_copy, permissions).unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Failed to break store hard links"), "{stderr}");
    assert_eq!(fs::read(&package_file).unwrap(), original);
    assert_eq!(fs::read(&store_copy).unwrap(), original);
    drop(npmrc_info);
    drop(root);
}
