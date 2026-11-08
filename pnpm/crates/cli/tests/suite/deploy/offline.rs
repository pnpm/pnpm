use super::{
    CommandExtra, CommandTempCwd, Lockfile, append_workspace_yaml_key, deployed_package_version,
    fs, pacquet_cmd,
};
use assert_cmd::assert::OutputAssertExt;

#[test]
fn offline_deploy_reuses_source_lockfile_without_metadata() {
    for mode in ["shared", "separate", "legacy"] {
        for alias in ["@pnpm.e2e/foo", "aliased"] {
            let fixture = CommandTempCwd::init().add_mocked_registry();
            let workspace = &fixture.workspace;
            append_workspace_yaml_key(workspace, "packages", "\n  - .");
            append_workspace_yaml_key(workspace, "sharedWorkspaceLockfile", mode != "separate");
            append_workspace_yaml_key(workspace, "trustLockfile", true);
            append_workspace_yaml_key(workspace, "ignoreScripts", true);
            let specifier =
                if alias == "aliased" { "npm:@pnpm.e2e/foo@^100.0.0" } else { "^100.0.0" };
            fs::write(
                workspace.join("package.json"),
                serde_json::to_vec(&serde_json::json!({
                    "name": "app", "version": "1.0.0", "dependencies": { alias: specifier },
                }))
                .unwrap(),
            )
            .unwrap();
            pacquet_cmd(workspace)
                .with_arg("install")
                .assert()
                .success();
            let lockfile = fs::read(workspace.join(Lockfile::FILE_NAME)).unwrap();
            fs::remove_dir_all(&fixture.npmrc_info.cache_dir).unwrap();
            pacquet_cmd(workspace)
                .with_args(["install", "--offline", "--frozen-lockfile"])
                .assert()
                .success();
            let mut deploy = pacquet_cmd(workspace);
            deploy.args(["--filter=.", "deploy", "out", "--offline", "--frozen-lockfile"]);
            if mode == "legacy" {
                deploy.arg("--legacy");
            }
            deploy.assert().success();
            assert_eq!(deployed_package_version(&workspace.join("out"), alias), "100.1.0");
            assert_eq!(fs::read(workspace.join(Lockfile::FILE_NAME)).unwrap(), lockfile);
        }
    }
}

#[test]
fn offline_legacy_deploy_reuses_registry_dependencies_of_workspace_projects() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    let workspace = &fixture.workspace.join("workspace (old)");
    fs::create_dir_all(workspace).unwrap();
    for name in [".npmrc", "pnpm-workspace.yaml"] {
        let config = fs::read_to_string(fixture.workspace.join(name)).unwrap();
        fs::write(workspace.join(name), config.replace("../pacquet-", "../../pacquet-")).unwrap();
    }
    super::write_workspace(workspace, false);
    append_workspace_yaml_key(workspace, "trustLockfile", true);
    append_workspace_yaml_key(workspace, "ignoreScripts", true);
    let lib_path = workspace.join("packages/lib/package.json");
    let mut lib: serde_json::Value = serde_json::from_slice(&fs::read(&lib_path).unwrap()).unwrap();
    lib["dependencies"] = serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" });
    fs::write(&lib_path, serde_json::to_vec(&lib).unwrap()).unwrap();
    pacquet_cmd(workspace)
        .with_arg("install")
        .assert()
        .success();
    fs::remove_dir_all(&fixture.npmrc_info.cache_dir).unwrap();
    pacquet_cmd(workspace)
        .with_args(["--filter=app", "deploy", "out", "--legacy", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    fs::rename(workspace.join("packages"), workspace.join("source-packages")).unwrap();
    std::process::Command::new("node")
        .current_dir(workspace.join("out"))
        .args(["-p", "require(require.resolve('@pnpm.e2e/pkg-with-1-dep/package.json', { paths: [require.resolve('lib/package.json')] })).version"])
        .assert().success().stdout("100.0.0\n");
}
