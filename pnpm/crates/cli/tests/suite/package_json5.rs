use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_package_manifest::PackageManifest;
use pnpm_testing_utils::{bin::CommandTempCwd, command_env::CommandTestExt};
use serde_json::json;
use std::{fs, io::Read, process::Command};

fn command(template: &Command) -> Command {
    Command::new(template.get_program())
        .without_ambient_pnpm_config()
        .with_current_dir(template.get_current_dir().unwrap())
}

#[test]
fn edits_and_runs_json5_without_creating_json() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let path = workspace.join("package.json5");
    fs::write(&path, "// project\n{ name: 'fixture', version: '1.0.0', scripts: { probe: 'echo json5-probe' }, }\n").unwrap();
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
    assert!(String::from_utf8_lossy(&output.stdout).contains("json5-probe"), "{output:?}");
    let manifest = PackageManifest::from_path(path.clone()).unwrap();
    assert_eq!(manifest.value()["version"], "1.0.1");
    assert_eq!(manifest.value()["custom"]["keep"], true);
    let text = fs::read_to_string(path).unwrap();
    assert!(text.contains("// project"), "{text}");
    assert!(text.contains("name:'fixture'"), "{text}");
    assert!(text.contains("version:'1.0.1'"), "{text}");
    assert!(text.contains("keep:true"), "{text}");
    assert!(!text.contains(r#""name""#), "{text}");
    assert!(!text.contains(r#""version""#), "{text}");
    assert!(!workspace.join("package.json").exists(), "JSON must not be created");
    drop(root);
}

#[test]
fn add_update_remove_preserve_json5_comments() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let path = workspace.join("package.json5");
    fs::write(&path, "// project\n{ name: 'fixture', version: '1.0.0', }\n").unwrap();
    command(&pacquet)
        .args(["add", "@pnpm.e2e/foo@1.0.0", "--lockfile-only"])
        .assert()
        .success();
    assert_eq!(
        PackageManifest::from_path(path.clone()).unwrap().value()["dependencies"],
        json!({"@pnpm.e2e/foo": "1.0.0"}),
    );
    command(&pacquet)
        .args(["update", "@pnpm.e2e/foo@2.0.0", "--lockfile-only"])
        .assert()
        .success();
    assert_eq!(
        PackageManifest::from_path(path.clone()).unwrap().value()["dependencies"],
        json!({"@pnpm.e2e/foo": "2.0.0"}),
    );
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
    let text = fs::read_to_string(path).unwrap();
    assert!(text.contains("// project"), "{text}");
    assert!(text.contains("name:'fixture'"), "{text}");
    assert!(!text.contains(r#""name""#), "{text}");
    assert!(!text.contains(r#""version""#), "{text}");
    assert!(!workspace.join("package.json").exists(), "JSON must not be created");
    drop((root, npmrc_info));
}

#[test]
fn workspace_discovery_runs_one_preferred_manifest_per_directory() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n").unwrap();
    let project = workspace.join("packages/foo");
    fs::create_dir_all(&project).unwrap();
    let path = project.join("package.json5");
    fs::write(&path, "{name: 'foo', scripts: {probe: 'echo JSON5-SELECTED'}}").unwrap();
    fs::write(project.join("package.yaml"), "name: yaml\nscripts:\n  probe: echo YAML-WRONG\n")
        .unwrap();
    let output = command(&pacquet)
        .args(["-r", "run", "probe"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("JSON5-SELECTED"), "{stdout}");
    assert!(!stdout.contains("YAML-WRONG"), "{stdout}");
    command(&pacquet)
        .args(["--filter", "foo", "pkg", "set", "version=2.0.0"])
        .assert()
        .success();
    assert_eq!(PackageManifest::from_path(path).unwrap().value()["version"], "2.0.0");
    drop(root);
}

#[test]
fn install_runs_json5_project_hooks() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json5"),
        r#"{
      name: 'fixture', version: '1.0.0',
      scripts: {postinstall: 'node -e "require(\'fs\').writeFileSync(\'hook-ran\', \'yes\')"'},
    }"#,
    )
    .unwrap();
    command(&pacquet)
        .args(["install", "--offline"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(workspace.join("hook-ran")).unwrap(), "yes");
    drop(root);
}

#[test]
fn pack_normalizes_alternative_project_manifests() {
    for basename in ["package.json5", "package.yaml"] {
        for (extra, ignored) in [("", false), ("", true), (r#", "files": ["dist"]"#, false)] {
            assert_packed_manifest(basename, extra, ignored);
        }
    }
}

fn assert_packed_manifest(basename: &str, extra: &str, ignored: bool) {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join(basename),
        format!(r#"{{"name": "json5-fixture", "version": "1.0.0"{extra}}}"#),
    )
    .unwrap();
    if basename == "package.json5" {
        fs::write(workspace.join("package.yaml"), "name: wrong\nversion: 9.0.0\n").unwrap();
    }
    fs::create_dir(workspace.join("dist")).unwrap();
    fs::write(workspace.join("dist/index.js"), "module.exports = 1").unwrap();
    if ignored {
        fs::write(workspace.join(".npmignore"), "package.json5\npackage.yaml\n").unwrap();
    }
    command(&pacquet)
        .args(["pack", "--ignore-scripts"])
        .assert()
        .success();
    let file = fs::File::open(workspace.join("json5-fixture-1.0.0.tgz")).unwrap();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut manifests = Vec::new();
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = entry.path().unwrap().into_owned();
        assert!(!path.ends_with("package.json5") && !path.ends_with("package.yaml"));
        if path == std::path::Path::new("package/package.json") {
            let mut text = String::new();
            entry.read_to_string(&mut text).unwrap();
            manifests.push(serde_json::from_str::<serde_json::Value>(&text).unwrap());
        }
    }
    assert_eq!(manifests.len(), 1, "expected exactly one normalized manifest");
    assert_eq!(manifests[0]["name"], "json5-fixture");
    assert_eq!(manifests[0]["version"], "1.0.0");
    drop(root);
}
