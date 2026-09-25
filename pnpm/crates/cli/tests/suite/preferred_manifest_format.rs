use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_package_manifest::PackageManifest;
use pnpm_testing_utils::{bin::CommandTempCwd, command_env::CommandTestExt};
use serde_json::json;
use std::{fs, io::Write, path::Path, process::Command};

const STUB_JSON: &str = r#"{"name": "stub", "version": "0.0.0"}"#;

fn command(template: &Command) -> Command {
    Command::new(template.get_program())
        .without_ambient_pnpm_config()
        .with_current_dir(template.get_current_dir().unwrap())
}

fn append_to_workspace_yaml(workspace: &Path, text: &str) {
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(workspace.join("pnpm-workspace.yaml"))
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
}

#[test]
fn reads_and_writes_the_preferred_manifest_of_the_root_project() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    append_to_workspace_yaml(&workspace, "preferredManifestFormat: json5\n");
    fs::write(workspace.join("package.json"), STUB_JSON).unwrap();
    let path = workspace.join("package.json5");
    fs::write(
        &path,
        "// real\n{ name: 'fixture', version: '1.0.0', scripts: { probe: 'echo JSON5-SELECTED' }, }\n",
    )
    .unwrap();
    command(&pacquet)
        .args(["version", "patch", "--no-git-tag-version"])
        .assert()
        .success();
    command(&pacquet)
        .args(["pkg", "set", "custom.keep=true", "--json"])
        .assert()
        .success();
    command(&pacquet)
        .args(["set-script", "other", "echo other"])
        .assert()
        .success();
    let output = command(&pacquet)
        .args(["run", "probe"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("JSON5-SELECTED"), "{output:?}");
    let manifest = PackageManifest::from_path(path.clone()).unwrap();
    assert_eq!(manifest.value()["version"], "1.0.1");
    assert_eq!(manifest.value()["custom"]["keep"], true);
    assert_eq!(manifest.value()["scripts"]["other"], "echo other");
    assert!(fs::read_to_string(path).unwrap().contains("// real"));
    assert_eq!(fs::read_to_string(workspace.join("package.json")).unwrap(), STUB_JSON);
    drop(root);
}

#[test]
fn add_and_remove_write_the_preferred_manifest() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    append_to_workspace_yaml(&workspace, "preferredManifestFormat: json5\n");
    fs::write(workspace.join("package.json"), STUB_JSON).unwrap();
    let path = workspace.join("package.json5");
    fs::write(&path, "// real\n{ name: 'fixture', version: '1.0.0', }\n").unwrap();
    command(&pacquet)
        .args(["add", "@pnpm.e2e/foo@1.0.0", "--lockfile-only"])
        .assert()
        .success();
    assert_eq!(
        PackageManifest::from_path(path.clone()).unwrap().value()["dependencies"],
        json!({"@pnpm.e2e/foo": "1.0.0"}),
    );
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
    assert!(lockfile.contains("'@pnpm.e2e/foo'"), "{lockfile}");
    command(&pacquet)
        .args(["remove", "@pnpm.e2e/foo", "--lockfile-only"])
        .assert()
        .success();
    assert_eq!(
        PackageManifest::from_path(path.clone())
            .unwrap()
            .value()
            .get("dependencies"),
        None,
    );
    assert!(fs::read_to_string(path).unwrap().contains("// real"));
    assert_eq!(fs::read_to_string(workspace.join("package.json")).unwrap(), STUB_JSON);
    drop((root, npmrc_info));
}

#[test]
fn workspace_discovery_reads_the_preferred_manifest_of_each_project() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    append_to_workspace_yaml(
        &workspace,
        "packages:\n  - packages/*\npreferredManifestFormat: yaml\n",
    );
    let project = workspace.join("packages/foo");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("package.json"),
        r#"{"name": "foo", "scripts": {"probe": "echo JSON-WRONG"}}"#,
    )
    .unwrap();
    let path = project.join("package.yaml");
    fs::write(&path, "name: foo\nscripts:\n  probe: echo YAML-SELECTED\n").unwrap();
    let output = command(&pacquet)
        .args(["-r", "run", "probe"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("YAML-SELECTED"), "{stdout}");
    assert!(!stdout.contains("JSON-WRONG"), "{stdout}");
    command(&pacquet)
        .args(["--filter", "foo", "pkg", "set", "version=2.0.0"])
        .assert()
        .success();
    assert_eq!(PackageManifest::from_path(path).unwrap().value()["version"], "2.0.0");
    assert!(!fs::read_to_string(project.join("package.json")).unwrap().contains("version"));
    drop(root);
}

#[test]
fn rejects_an_unknown_manifest_format() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    append_to_workspace_yaml(&workspace, "preferredManifestFormat: jsonc\n");
    fs::write(workspace.join("package.json"), STUB_JSON).unwrap();
    let output = command(&pacquet)
        .args(["pkg", "get", "name"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("preferredManifestFormat"),
        "{output:?}",
    );
    drop(root);
}
