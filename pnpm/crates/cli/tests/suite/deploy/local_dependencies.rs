use std::process::Command;

use assert_cmd::assert::OutputAssertExt;

use super::{
    AddMockedRegistry,
    CommandExtra,
    CommandTempCwd,
    append_workspace_yaml_key,
    fs,
    pacquet_cmd,
};

/// A `file:` dependency may live outside the lockfile directory. Install
/// links it, so deploy copies it too.
#[test]
fn deploy_copies_file_dependency_outside_the_lockfile_dir() {
    deploy_copies_external_dependency("file:", false);
}

#[test]
fn deploy_copies_link_dependency_outside_the_lockfile_dir() {
    deploy_copies_external_dependency("link:", false);
}

#[test]
fn deploy_copies_link_override_outside_the_lockfile_dir() {
    deploy_copies_external_dependency("link:", true);
}

fn deploy_copies_external_dependency(protocol: &str, link_override: bool) {
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
        r#"{"name":"@example/client","version":"1.0.0","main":"index.js","dependencies":{"is-positive":"1.0.0"}}"#,
    )
    .unwrap();
    fs::write(
        client.join("index.js"),
        "module.exports = require('is-positive/package.json').version",
    )
    .unwrap();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({"name":"app","version":"1.0.0","dependencies":{"@example/client":format!("{protocol}../client")}}).to_string(),
    )
    .unwrap();
    append_workspace_yaml_key(&workspace, "packages", "[]");
    append_workspace_yaml_key(&workspace, "injectWorkspacePackages", "true");

    if link_override {
        append_workspace_yaml_key(&workspace, "overrides", "\n  '@example/client': link:../client");
    }
    pacquet
        .with_arg("install")
        .assert()
        .success();
    pacquet_cmd(&workspace)
        .with_args(["--filter", ".", "deploy", "deploy"])
        .assert()
        .success();

    fs::rename(&client, root.path().join("moved")).unwrap();
    Command::new("node")
        .with_current_dir(workspace.join("deploy"))
        .with_args(["-p", "require('@example/client')"])
        .assert()
        .success()
        .stdout("1.0.0\n");

    drop((root, mock_instance));
}

#[test]
fn deploy_resolves_relative_file_overrides_from_the_source_workspace() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    let source = &fixture.workspace;
    let root = source.parent().unwrap();
    let sdk = root.join("sdk");
    let common = root.join("common");
    for package in [&sdk, &common] {
        fs::create_dir(package).unwrap();
    }
    fs::write(
        source.join("package.json"),
        r#"{"name":"repro-app","version":"1.0.0","dependencies":{"repro-sdk":"file:../sdk"}}"#,
    )
    .unwrap();
    fs::write(
        sdk.join("package.json"),
        r#"{"name":"repro-sdk","version":"1.0.0","main":"index.js","dependencies":{"repro-common":"1.0.0"}}"#,
    )
    .unwrap();
    fs::write(sdk.join("index.js"), "module.exports = require('repro-common')\n").unwrap();
    fs::write(
        common.join("package.json"),
        r#"{"name":"repro-common","version":"1.0.0","main":"index.js"}"#,
    )
    .unwrap();
    fs::write(common.join("index.js"), "module.exports = 42\n").unwrap();
    append_workspace_yaml_key(source, "packages", "[]");
    append_workspace_yaml_key(source, "sharedWorkspaceLockfile", "false");
    append_workspace_yaml_key(
        source,
        "overrides",
        "\n  \"repro-sdk>repro-common\": file:../common",
    );

    pacquet_cmd(source)
        .with_arg("install")
        .assert()
        .success();

    let output = root.join("output");
    for (mode, extra) in [
        ("default", None),
        ("shared", Some("--config.sharedWorkspaceLockfile=true")),
        ("legacy", Some("--legacy")),
    ] {
        let target = output.join(mode);
        let mut deploy = pacquet_cmd(source).with_args(["--filter=.", "deploy"]);
        if let Some(extra) = extra {
            deploy = deploy.with_arg(extra);
        }
        deploy = deploy.with_arg(&target);
        deploy.assert().success();
    }

    fs::rename(&common, root.join("common-away")).unwrap();
    fs::rename(&sdk, root.join("sdk-away")).unwrap();
    for mode in ["default", "shared", "legacy"] {
        Command::new("node")
            .with_current_dir(output.join(mode))
            .with_args(["-e", "if (require('repro-sdk') !== 42) process.exit(1)"])
            .assert()
            .success();
    }
}
