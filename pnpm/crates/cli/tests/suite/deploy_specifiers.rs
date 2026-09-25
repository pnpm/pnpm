use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::{ImporterDepVersion, Lockfile, ResolvedDependencySpec};
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

#[test]
fn prod_deploy_skips_a_devengines_runtime_without_failing_the_frozen_lockfile_check() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    fs::write(workspace.join("package.json"), r#"{"name":"root","private":true}"#).unwrap();
    fs::create_dir(workspace.join("app")).unwrap();
    let app_manifest_path = workspace.join("app/package.json");
    fs::write(
        &app_manifest_path,
        json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/abc": "1.0.0" },
        })
        .to_string(),
    )
    .unwrap();
    crate::_utils::append_workspace_yaml_key(&workspace, "packages", "[app]");
    crate::_utils::append_workspace_yaml_key(&workspace, "injectWorkspacePackages", true);

    pacquet
        .with_arg("install")
        .assert()
        .success();

    // Resolving a `devEngines.runtime` with `onFail: download` reaches the
    // Node.js download mirrors, which the mocked registry does not serve, so
    // the runtime edge is declared only after the install, in both the
    // manifest and the lockfile, in the shape a real resolution writes.
    let mut app_manifest: Value =
        serde_json::from_str(&fs::read_to_string(&app_manifest_path).unwrap()).unwrap();
    app_manifest["devEngines"] = json!({
        "runtime": { "name": "node", "version": "24.0.0", "onFail": "download" }
    });
    fs::write(&app_manifest_path, app_manifest.to_string()).unwrap();
    let mut lockfile = Lockfile::load_wanted_from_dir(&workspace).unwrap().unwrap();
    lockfile.importers
        .get_mut("app")
        .unwrap()
        .dev_dependencies
        .get_or_insert_with(Default::default)
        .insert(
            "node".parse().unwrap(),
            ResolvedDependencySpec {
                specifier: "runtime:24.0.0".to_string(),
                version: "runtime:24.0.0".parse::<ImporterDepVersion>().unwrap(),
            },
        );
    lockfile
        .save_to_path(&workspace.join("pnpm-lock.yaml"))
        .unwrap();

    let deploy_dir = root.path().join("deploy");
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(&workspace)
        .with_args(["--filter=app", "--prod", "deploy"])
        .with_arg(&deploy_dir)
        .assert()
        .success();

    assert!(deploy_dir.join("node_modules/@pnpm.e2e/abc").exists());
    assert!(
        !deploy_dir.join("node_modules/node").exists(),
        "the devEngines runtime is a dev dependency, so --prod must skip it",
    );
    let deployed_lockfile = Lockfile::load_wanted_from_dir(&deploy_dir).unwrap().unwrap();
    let runtime = &deployed_lockfile.importers["."].dev_dependencies.as_ref().unwrap()
        [&"node".parse().unwrap()];
    assert_eq!(runtime.specifier, "runtime:24.0.0");
}
