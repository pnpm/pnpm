//! `pnpm-workspace.yaml` delegating settings to included files and config
//! dependencies.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_modules_yaml::{Host, NodeLinker, read_modules_manifest};
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    diagnostics::assert_diagnostic_contains,
};
use std::{fs, path::Path, process::Command};

fn pnpm_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn append_to_workspace_manifest(workspace: &Path, text: &str) {
    let path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&path).expect("read pnpm-workspace.yaml");
    yaml.push('\n');
    yaml.push_str(text);
    fs::write(&path, yaml).expect("write pnpm-workspace.yaml");
}

fn write(workspace: &Path, name: &str, text: &str) {
    let path = workspace.join(name);
    fs::create_dir_all(path.parent().expect("file has a parent")).expect("create parent");
    fs::write(path, text).expect("write file");
}

fn recorded_node_linker(workspace: &Path) -> Option<NodeLinker> {
    read_modules_manifest::<Host>(&workspace.join("node_modules"))
        .expect("read the modules manifest")
        .expect("the install wrote a modules manifest")
        .node_linker
}

#[test]
fn install_reads_settings_and_catalogs_from_included_files() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write(
        &workspace,
        "package.json",
        &serde_json::json!({ "dependencies": { "@pnpm.e2e/foo": "catalog:" } }).to_string(),
    );
    append_to_workspace_manifest(
        &workspace,
        "include:\n  ./config/catalog.yaml: [catalog]\n  ./config/deploy.yml: [nodeLinker]\n",
    );
    write(&workspace, "config/catalog.yaml", "catalog:\n  '@pnpm.e2e/foo': 100.0.0\n");
    write(&workspace, "config/deploy.yml", "nodeLinker: hoisted\n");

    pacquet
        .with_arg("install")
        .assert()
        .success();

    assert_eq!(recorded_node_linker(&workspace), Some(NodeLinker::Hoisted));
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(lockfile.contains("'@pnpm.e2e/foo':\n      specifier: 100.0.0"), "{lockfile}");

    drop((root, mock_instance));
}

#[test]
fn a_setting_outside_the_delegation_fails_the_install() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write(&workspace, "package.json", "{}");
    append_to_workspace_manifest(&workspace, "include:\n  ./catalog.yaml: [catalog]\n");
    write(&workspace, "catalog.yaml", "minimumReleaseAgeExclude: ['*']\n");

    let output = pacquet
        .with_arg("install")
        .output()
        .expect("run pnpm install");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_diagnostic_contains(&stderr, "ERR_PNPM_SETTING_NOT_DELEGATED");

    drop((root, mock_instance));
}

#[test]
fn config_set_writes_to_the_file_that_owns_the_setting() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_to_workspace_manifest(&workspace, "include:\n  ./deploy.yaml: [nodeLinker]\n");
    write(&workspace, "deploy.yaml", "");
    let root_manifest = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read");

    pacquet
        .with_args(["config", "set", "nodeLinker", "hoisted", "--location", "project"])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(workspace.join("deploy.yaml")).expect("read"),
        "nodeLinker: hoisted\n",
    );
    assert_eq!(
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read"),
        root_manifest,
    );

    drop((root, mock_instance));
}

#[test]
fn a_setting_owned_by_a_config_dependency_is_read_only_before_install() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_to_workspace_manifest(
        &workspace,
        "configDependencies:\n  '@pnpm.e2e/pnpm-policy': 1.0.0\ninclude:\n  '@pnpm.e2e/pnpm-policy': [minimumReleaseAge]\n",
    );

    let output = pnpm_at(&workspace)
        .with_args(["config", "set", "minimumReleaseAge", "0", "--location", "project"])
        .output()
        .expect("run pnpm config set");
    assert!(!output.status.success());
    assert_diagnostic_contains(
        &String::from_utf8_lossy(&output.stderr),
        "ERR_PNPM_SETTING_OWNED_BY_CONFIG_DEPENDENCY",
    );

    pnpm_at(&workspace)
        .with_args(["config", "set", "nodeLinker", "hoisted", "--location", "project"])
        .assert()
        .success();

    let output = pnpm_at(&workspace)
        .with_args(["config", "get", "minimumReleaseAge"])
        .output()
        .expect("run pnpm config get");
    assert!(output.status.success());
    assert_diagnostic_contains(&String::from_utf8_lossy(&output.stderr), "which is not installed");

    drop((root, mock_instance));
}

#[test]
fn settings_of_a_config_dependency_apply_on_its_first_install() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write(
        &workspace,
        "package.json",
        &serde_json::json!({ "dependencies": { "@pnpm.e2e/foo": "catalog:shared" } }).to_string(),
    );
    append_to_workspace_manifest(
        &workspace,
        "configDependencies:\n  '@pnpm.e2e/pnpm-policy': 1.0.0\ninclude:\n  '@pnpm.e2e/pnpm-policy': [nodeLinker, catalogs.shared]\n",
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();

    assert_eq!(recorded_node_linker(&workspace), Some(NodeLinker::Hoisted));
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(lockfile.contains("specifier: 100.0.0"), "{lockfile}");

    drop((root, mock_instance));
}

#[test]
fn a_config_dependency_defining_an_undelegated_setting_fails_the_install() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write(&workspace, "package.json", "{}");
    append_to_workspace_manifest(
        &workspace,
        "configDependencies:\n  '@pnpm.e2e/pnpm-policy': 1.0.0\ninclude:\n  '@pnpm.e2e/pnpm-policy': [nodeLinker]\n",
    );

    let output = pacquet
        .with_arg("install")
        .output()
        .expect("run pnpm install");
    assert!(!output.status.success());
    assert_diagnostic_contains(
        &String::from_utf8_lossy(&output.stderr),
        "ERR_PNPM_SETTING_NOT_DELEGATED",
    );

    drop((root, mock_instance));
}

#[test]
fn a_config_dependency_packed_with_pnpm_pack_keeps_its_config_entry() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let policy_dir = root.path().join("policy");
    write(
        &policy_dir,
        "package.json",
        &serde_json::json!({
            "name": "@acme/pnpm-policy",
            "version": "1.0.0",
            "pnpm": {
                "config": "./config.yaml",
                "overrides": { "ignored": "1.0.0" },
            },
            "files": ["index.js"],
        })
        .to_string(),
    );
    write(&policy_dir, "index.js", "");
    write(&policy_dir, "config.yaml", "nodeLinker: hoisted\n");

    pnpm_at(&policy_dir)
        .with_arg("pack")
        .assert()
        .success();

    let tarball_bytes = fs::read(policy_dir.join("acme-pnpm-policy-1.0.0.tgz"))
        .expect("read packed config dependency tarball");
    let integrity = ssri::Integrity::from(tarball_bytes.as_slice());
    let mut server = mockito::Server::new();
    let tarball_path = "/@acme/pnpm-policy/-/pnpm-policy-1.0.0.tgz";
    let mock = server
        .mock("GET", tarball_path)
        .with_status(200)
        .with_body(tarball_bytes)
        .expect(1)
        .create();
    let registry = format!("{}/", server.url());
    write(&workspace, ".npmrc", &format!("registry={registry}\n"));
    write(&workspace, "package.json", "{}");
    write(
        &workspace,
        "pnpm-workspace.yaml",
        &format!(
            "storeDir: ../store\ncacheDir: ../cache\nconfigDependencies:\n  '@acme/pnpm-policy':\n    integrity: 1.0.0+{integrity}\n    tarball: {}{tarball_path}\ninclude:\n  '@acme/pnpm-policy': [nodeLinker]\n",
            server.url(),
        ),
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();

    mock.assert();
    assert_eq!(recorded_node_linker(&workspace), Some(NodeLinker::Hoisted));
    let installed_manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join(
            "node_modules/.pnpm-config/@acme/pnpm-policy/package.json",
        ))
        .expect("read installed config dependency package.json"),
    )
    .expect("parse installed config dependency package.json");
    assert_eq!(installed_manifest["pnpm"], serde_json::json!({ "config": "./config.yaml" }));

    drop(root);
}
