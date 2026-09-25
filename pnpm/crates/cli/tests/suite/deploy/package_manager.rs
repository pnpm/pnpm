use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, append_workspace_yaml_key, fs, pacquet_cmd,
    write_project, write_workspace,
};
use assert_cmd::assert::OutputAssertExt;
use std::path::Path;

fn write_pinned_workspace_root(workspace: &Path) {
    write_workspace(workspace, false);
    append_workspace_yaml_key(workspace, "pmOnFail", "ignore");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "private": true,
            "packageManager": "pnpm@10.18.0",
            "devEngines": {
                "packageManager": { "name": "pnpm", "version": "^10.18.0", "onFail": "download" },
            },
        })
        .to_string(),
    )
    .unwrap();
}

fn read_deployed_manifest(deploy_dir: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(deploy_dir.join("package.json")).unwrap()).unwrap()
}

#[test]
fn deploy_copies_the_package_manager_pin_of_the_workspace_root() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_pinned_workspace_root(&workspace);
    write_project(
        &workspace,
        "app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "lib": "workspace:*" },
            "devEngines": { "runtime": { "name": "node", "version": "*" } },
        }),
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();

    for (deploy_args, target) in [(&[][..], "deploy"), (&["--legacy"][..], "legacy-deploy")] {
        pacquet_cmd(&workspace)
            .with_args(["--filter", "app", "deploy", "--prod"])
            .with_args(deploy_args)
            .with_arg(target)
            .assert()
            .success();

        let manifest = read_deployed_manifest(&workspace.join(target));
        assert_eq!(manifest["packageManager"], "pnpm@10.18.0", "{target}: {manifest:#}");
        assert_eq!(
            manifest["devEngines"],
            serde_json::json!({
                "packageManager": { "name": "pnpm", "version": "^10.18.0", "onFail": "download" },
                "runtime": { "name": "node", "version": "*" },
            }),
            "{target}: {manifest:#}",
        );
    }

    drop((root, mock_instance));
}

#[test]
fn deploy_keeps_the_package_manager_pin_of_the_deployed_project() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_pinned_workspace_root(&workspace);
    write_project(
        &workspace,
        "app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "files": ["index.js"],
            "devEngines": { "packageManager": { "name": "pnpm", "version": "^11.0.0" } },
        }),
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();

    for (deploy_args, target) in [(&[][..], "deploy"), (&["--legacy"][..], "legacy-deploy")] {
        pacquet_cmd(&workspace)
            .with_args(["--filter", "app", "deploy", "--prod"])
            .with_args(deploy_args)
            .with_arg(target)
            .assert()
            .success();

        let manifest = read_deployed_manifest(&workspace.join(target));
        assert!(manifest.get("packageManager").is_none(), "{target}: {manifest:#}");
        assert_eq!(
            manifest["devEngines"],
            serde_json::json!({ "packageManager": { "name": "pnpm", "version": "^11.0.0" } }),
            "{target}: {manifest:#}",
        );
    }

    drop((root, mock_instance));
}
