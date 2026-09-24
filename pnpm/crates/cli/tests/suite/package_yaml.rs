use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_package_manifest::PackageManifest;
use pnpm_testing_utils::{bin::CommandTempCwd, command_env::CommandTestExt};
use serde_json::json;
use std::{fs, process::Command};

#[test]
fn pkg_set_and_delete_preserve_yaml_comments() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let path = workspace.join("package.yaml");
    fs::write(&path, "# project\nname: fixture\nversion: 1.0.0 # version\ncustom:\n  keep: true # keep\n  remove: false\n").unwrap();
    command(&pacquet)
        .args(["pkg", "set", "version=2.0.0"])
        .assert()
        .success();
    command(&pacquet)
        .args(["pkg", "delete", "custom.remove"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "# project\nname: fixture\nversion: 2.0.0 # version\ncustom:\n  keep: true # keep\n",
    );
    assert!(!workspace.join("package.json").exists());
    drop(root);
}

#[test]
fn version_updates_package_yaml() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let path = workspace.join("package.yaml");
    fs::write(&path, "# project\nname: fixture\nversion: 1.0.0 # version\n").unwrap();
    pacquet
        .with_args(["version", "patch", "--no-git-tag-version"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "# project\nname: fixture\nversion: 1.0.1 # version\n",
    );
    assert!(!workspace.join("package.json").exists());
    drop(root);
}

#[test]
fn add_update_and_remove_save_package_yaml() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let path = workspace.join("package.yaml");
    fs::write(&path, "# project\nname: fixture\nversion: 1.0.0\n").unwrap();
    command(&pacquet)
        .args(["add", "@pnpm.e2e/foo@1.0.0", "--lockfile-only"])
        .assert()
        .success();
    assert_eq!(
        PackageManifest::from_path(path.clone()).unwrap().value()["dependencies"],
        json!({"@pnpm.e2e/foo":"1.0.0"}),
    );
    command(&pacquet)
        .args(["update", "@pnpm.e2e/foo@2.0.0", "--lockfile-only"])
        .assert()
        .success();
    assert_eq!(
        PackageManifest::from_path(path.clone()).unwrap().value()["dependencies"],
        json!({"@pnpm.e2e/foo":"2.0.0"}),
    );
    command(&pacquet)
        .args(["remove", "@pnpm.e2e/foo", "--lockfile-only"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(path).unwrap(), "# project\nname: fixture\nversion: 1.0.0\n");
    assert!(!workspace.join("package.json").exists());
    drop((root, npmrc_info));
}

#[test]
fn recursive_pkg_edits_yaml_workspace_projects() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n").unwrap();
    let project = workspace.join("packages/foo");
    fs::create_dir_all(&project).unwrap();
    let path = project.join("package.yaml");
    fs::write(&path, "# project\nname: foo\nversion: 1.0.0\n").unwrap();
    pacquet
        .with_args(["--filter", "foo", "pkg", "set", "version=2.0.0"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(path).unwrap(), "# project\nname: foo\nversion: 2.0.0\n");
    assert!(!project.join("package.json").exists());
    drop(root);
}

#[test]
fn add_saves_package_yml_without_creating_package_json() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let path = workspace.join("package.yml");
    fs::write(&path, "name: fixture\nversion: 1.0.0\n").unwrap();
    command(&pacquet)
        .args(["add", "@pnpm.e2e/foo@1.0.0", "--lockfile-only"])
        .assert()
        .success();
    assert_eq!(
        PackageManifest::from_path(path).unwrap().value()["dependencies"],
        json!({"@pnpm.e2e/foo":"1.0.0"}),
    );
    assert!(!workspace.join("package.json").exists());
    drop((root, npmrc_info));
}

fn command(template: &Command) -> Command {
    Command::new(template.get_program())
        .without_ambient_pnpm_config()
        .with_current_dir(template.get_current_dir().unwrap())
}

#[test]
fn json_manifest_takes_precedence_over_yaml() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let yaml = "# leave alone\nname: yaml\n";
    fs::write(workspace.join("package.yaml"), yaml).unwrap();
    fs::write(workspace.join("package.json"), "{\"name\":\"json\"}\n").unwrap();
    pacquet
        .with_args(["pkg", "set", "version=1.0.0"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(workspace.join("package.yaml")).unwrap(), yaml);
    assert_eq!(
        PackageManifest::from_path(workspace.join("package.json")).unwrap().value(),
        &json!({"name":"json","version":"1.0.0"}),
    );
    drop(root);
}

#[test]
fn link_saves_yaml_and_accepts_a_yaml_target() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let path = workspace.join("package.yaml");
    fs::write(&path, "# project\nname: fixture\nversion: 1.0.0\n").unwrap();
    let target = root.path().join("target-project");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("package.yaml"), "name: target-project\nversion: 1.0.0\n").unwrap();
    pacquet
        .with_args(["link", "../target-project"])
        .assert()
        .success();
    assert_eq!(
        PackageManifest::from_path(path.clone()).unwrap().value()["dependencies"],
        json!({"target-project":"link:../target-project"}),
    );
    assert!(fs::read_to_string(path).unwrap().starts_with("# project\n"));
    assert!(!workspace.join("package.json").exists());
    drop((root, npmrc_info));
}

#[test]
fn set_script_updates_package_yaml() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let path = workspace.join("package.yaml");
    fs::write(&path, "# project\nname: fixture\nscripts:\n  test: old # test\n  build: build\n")
        .unwrap();
    command(&pacquet)
        .args(["set-script", "test", "new"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "# project\nname: fixture\nscripts:\n  test: new # test\n  build: build\n",
    );
    assert!(!workspace.join("package.json").exists());
    drop(root);
}

#[test]
fn link_reports_a_malformed_yaml_target() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.yaml"), "name: fixture\n").unwrap();
    let target = workspace.join("target");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("package.yaml"), "name: [invalid\n").unwrap();
    let assertion = command(&pacquet)
        .args(["link", "./target"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assertion.get_output().stderr);
    assert!(stderr.contains("package.yaml"), "{stderr}");
    assert!(!stderr.contains("No package.json found"), "{stderr}");
    drop(root);
}
