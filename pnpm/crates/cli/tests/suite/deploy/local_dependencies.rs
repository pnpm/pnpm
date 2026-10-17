use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, append_workspace_yaml_key, fs, pacquet_cmd,
};
use assert_cmd::assert::OutputAssertExt;

/// A `file:` dependency may live outside the lockfile directory. Install
/// links it, so deploy copies it too.
#[test]
fn deploy_copies_file_dependency_outside_the_lockfile_dir() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let client = root.path().join("client");
    fs::create_dir(&client).unwrap();
    fs::write(
        client.join("package.json"),
        r#"{"name":"@example/client","version":"1.0.0","main":"index.js"}"#,
    )
    .unwrap();
    fs::write(client.join("index.js"), "module.exports = 42").unwrap();
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"@example/client":"file:../client"}}"#,
    )
    .unwrap();
    append_workspace_yaml_key(&workspace, "packages", "[]");
    append_workspace_yaml_key(&workspace, "injectWorkspacePackages", "true");

    pacquet
        .with_arg("install")
        .assert()
        .success();
    pacquet_cmd(&workspace)
        .with_args(["--filter", ".", "deploy", "deploy"])
        .assert()
        .success();

    let deployed_file = workspace.join("deploy/node_modules/@example/client/index.js");
    assert_eq!(fs::read_to_string(deployed_file).unwrap(), "module.exports = 42");

    drop((root, mock_instance));
}
