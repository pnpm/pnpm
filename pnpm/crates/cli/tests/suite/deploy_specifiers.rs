use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::Lockfile;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{fs, process::Command};

#[test]
fn deployed_peer_dependencies_can_install_with_a_fresh_lockfile() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    let dependencies = json!({
        "@pnpm.e2e/abc": "1.0.0",
        "alias": "npm:@pnpm.e2e/abc@1.0.0",
        "@pnpm.e2e/peer-a": "1.0.0",
        "@pnpm.e2e/peer-b": "1.0.0",
        "@pnpm.e2e/peer-c": "1.0.0",
    });
    fs::write(fixture.workspace.join("package.json"), r#"{"name":"root","private":true}"#).unwrap();
    fs::create_dir(fixture.workspace.join("app")).unwrap();
    fs::write(
        fixture.workspace.join("app/package.json"),
        json!({ "name": "app", "version": "1.0.0", "dependencies": dependencies }).to_string(),
    )
    .unwrap();
    crate::_utils::append_workspace_yaml_key(&fixture.workspace, "packages", "[app]");
    crate::_utils::append_workspace_yaml_key(&fixture.workspace, "injectWorkspacePackages", true);
    fixture.pacquet
        .with_arg("install")
        .assert()
        .success();
    let deploy_dir = fixture.root.path().join("deploy");
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&fixture.workspace)
        .with_args(["--filter=app", "deploy"])
        .with_arg(&deploy_dir)
        .assert()
        .success();
    let manifest_path = deploy_dir.join("package.json");
    let manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["dependencies"], dependencies);
    let deployed_lockfile = Lockfile::load_wanted_from_dir(&deploy_dir).unwrap().unwrap();
    let reference = &deployed_lockfile.importers["."].dependencies.as_ref().unwrap()
        [&"@pnpm.e2e/abc".parse().unwrap()]
        .version;
    assert!(reference.to_string().contains('('), "expected peer-qualified reference: {reference}");

    fs::remove_file(deploy_dir.join("pnpm-lock.yaml")).unwrap();
    fs::remove_dir_all(deploy_dir.join("node_modules")).unwrap();
    fs::copy(&fixture.npmrc_info.npmrc_path, deploy_dir.join(".npmrc")).unwrap();
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&deploy_dir)
        .with_args(["install", "--no-frozen-lockfile"])
        .assert()
        .success();
    let fresh_manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    assert_eq!(fresh_manifest, manifest);
    let fresh_lockfile = Lockfile::load_wanted_from_dir(&deploy_dir).unwrap().unwrap();
    let reference = &fresh_lockfile.importers["."].dependencies.as_ref().unwrap()
        [&"@pnpm.e2e/abc".parse().unwrap()]
        .version;
    assert!(reference.to_string().contains('('), "expected peer-qualified reference: {reference}");
    for name in ["@pnpm.e2e/abc", "alias"] {
        let installed: Value = serde_json::from_slice(
            &fs::read(
                deploy_dir
                    .join("node_modules")
                    .join(name)
                    .join("package.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(installed["name"], "@pnpm.e2e/abc");
        assert_eq!(installed["version"], "1.0.0");
    }
}
