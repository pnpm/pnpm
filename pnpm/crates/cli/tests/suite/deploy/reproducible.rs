use super::{AddMockedRegistry, CommandExtra, CommandTempCwd, fs, pacquet_cmd, write_workspace};
use assert_cmd::assert::OutputAssertExt;

/// `SOURCE_DATE_EPOCH` replaces the current time in the bookkeeping files a
/// deploy writes, so two deploys of the same project are byte-identical.
#[test]
fn deploy_records_source_date_epoch_in_bookkeeping_files() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, true);
    let app_manifest_path = workspace.join("packages/app/package.json");
    let mut app_manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&app_manifest_path).unwrap()).unwrap();
    app_manifest["scripts"] = serde_json::json!({ "start": "node index.js" });
    fs::write(app_manifest_path, app_manifest.to_string()).unwrap();
    pacquet
        .with_arg("install")
        .assert()
        .success();

    let deploy_dir = workspace.join("out");
    let deploy = || {
        pacquet_cmd(&workspace)
            .with_env("SOURCE_DATE_EPOCH", "1700000000")
            .with_args(["--filter", "app", "deploy", "--prod", "out"])
            .assert()
            .success();
        let modules_dir = deploy_dir.join("node_modules");
        (
            fs::read_to_string(modules_dir.join(".modules.yaml")).unwrap(),
            fs::read_to_string(modules_dir.join(".pnpm-workspace-state-v1.json")).unwrap(),
        )
    };
    let (modules_yaml, workspace_state) = deploy();
    fs::remove_dir_all(&deploy_dir).unwrap();

    assert!(modules_yaml.contains("Tue, 14 Nov 2023 22:13:20 GMT"), "{modules_yaml}");
    assert!(
        workspace_state.contains(r#""lastValidatedTimestamp": 1700000000000"#),
        "{workspace_state}",
    );
    assert_eq!(deploy(), (modules_yaml, workspace_state));
    pacquet_cmd(&deploy_dir)
        .with_args(["--config.verify-deps-before-run=error", "run", "start"])
        .assert()
        .success();

    drop((root, mock_instance));
}
