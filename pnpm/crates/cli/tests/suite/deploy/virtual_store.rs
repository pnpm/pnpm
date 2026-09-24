use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, append_workspace_yaml_key, fs, pacquet_cmd,
    write_project, write_workspace,
};
use assert_cmd::assert::OutputAssertExt;
use std::path::Path;

const FOO_ENTRY: &str = "@pnpm.e2e+foo@100.0.0";

fn write_app_with_foo(workspace: &Path) {
    write_workspace(workspace, false);
    write_project(
        workspace,
        "app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "@pnpm.e2e/foo": "100.0.0" },
        }),
    );
}

fn assert_foo_resolves_through(deploy_dir: &Path, virtual_store_dir: &str) {
    assert_eq!(
        dunce::canonicalize(deploy_dir.join("node_modules/@pnpm.e2e/foo")).unwrap(),
        dunce::canonicalize(
            deploy_dir
                .join(virtual_store_dir)
                .join(FOO_ENTRY)
                .join("node_modules/@pnpm.e2e/foo"),
        )
        .unwrap(),
    );
}

#[test]
fn shared_lockfile_deploy_resolves_virtual_store_dir_against_the_deploy_dir() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_app_with_foo(&workspace);
    append_workspace_yaml_key(&workspace, "virtualStoreDir", ".pnpm");

    pacquet
        .with_arg("install")
        .assert()
        .success();
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", "deploy"])
        .assert()
        .success();

    let deploy_dir = workspace.join("deploy");
    assert_foo_resolves_through(&deploy_dir, ".pnpm");
    assert!(
        !deploy_dir
            .join("node_modules/.pnpm")
            .join(FOO_ENTRY)
            .exists(),
        "the default virtual store should stay empty",
    );
    let deploy_workspace_yaml = fs::read_to_string(deploy_dir.join("pnpm-workspace.yaml")).unwrap();
    assert!(
        deploy_workspace_yaml.contains("virtualStoreDir: .pnpm\n"),
        "the deployed pnpm-workspace.yaml should keep virtualStoreDir:\n{deploy_workspace_yaml}",
    );

    drop((root, mock_instance));
}

#[test]
fn legacy_deploy_resolves_virtual_store_dir_against_the_deploy_dir() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_app_with_foo(&workspace);
    append_workspace_yaml_key(&workspace, "virtualStoreDir", "node_modules/.custom");

    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--legacy", "--prod", "deploy"])
        .assert()
        .success();

    let deploy_dir = workspace.join("deploy");
    assert_foo_resolves_through(&deploy_dir, "node_modules/.custom");
    assert!(
        !deploy_dir
            .join("node_modules/.pnpm")
            .join(FOO_ENTRY)
            .exists(),
        "the default virtual store should stay empty",
    );

    drop((root, mock_instance));
}

#[test]
fn deploy_keeps_the_default_virtual_store_when_virtual_store_dir_names_the_global_virtual_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_app_with_foo(&workspace);
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).unwrap();
    assert!(yaml.contains("enableGlobalVirtualStore: false\n"));
    fs::write(
        &yaml_path,
        yaml.replace("enableGlobalVirtualStore: false\n", "enableGlobalVirtualStore: true\n"),
    )
    .unwrap();
    append_workspace_yaml_key(&workspace, "virtualStoreDir", "global-virtual-store");

    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--legacy", "--prod", "deploy"])
        .assert()
        .success();

    let deploy_dir = workspace.join("deploy");
    assert_foo_resolves_through(&deploy_dir, "node_modules/.pnpm");
    assert!(!deploy_dir.join("global-virtual-store").exists());
    assert!(!workspace.join("global-virtual-store").exists());

    drop((root, mock_instance));
}

#[test]
fn deploy_keeps_the_default_virtual_store_when_virtual_store_dir_is_absolute() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_app_with_foo(&workspace);
    let shared_virtual_store =
        dunce::canonicalize(&workspace).unwrap().join("shared-virtual-store");
    append_workspace_yaml_key(
        &workspace,
        "virtualStoreDir",
        shared_virtual_store.to_str().unwrap(),
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", "deploy"])
        .assert()
        .success();

    let deploy_dir = workspace.join("deploy");
    assert_foo_resolves_through(&deploy_dir, "node_modules/.pnpm");
    assert!(
        !fs::read_to_string(deploy_dir.join("pnpm-workspace.yaml"))
            .unwrap()
            .contains("virtualStoreDir"),
    );

    drop((root, mock_instance));
}
