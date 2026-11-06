use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, append_workspace_yaml_key, fs, pacquet_cmd,
    write_project,
};
use assert_cmd::assert::OutputAssertExt;
use std::path::Path;

const BUILD_SCRIPT: &str = r#"node -e "require('fs').writeFileSync('built.txt', '1')""#;

fn write_workspace_with_built_lib(workspace: &Path) {
    append_workspace_yaml_key(workspace, "packages", "\n  - packages/*");
    write_project(
        workspace,
        "app",
        &serde_json::json!({
            "name": "app", "version": "1.0.0", "dependencies": { "lib": "workspace:*" },
        }),
    );
    write_project(
        workspace,
        "lib",
        &serde_json::json!({
            "name": "lib", "version": "1.0.0", "scripts": { "postinstall": BUILD_SCRIPT },
        }),
    );
}

/// The source install runs the project's build, and the deploy copies the
/// project's files, so the output is removed to observe the deploy's build.
fn remove_source_build_output(workspace: &Path) {
    fs::remove_file(workspace.join("packages/lib/built.txt")).unwrap();
}

#[test]
fn deploy_builds_a_workspace_project_it_converts_to_a_file_dependency() {
    for deploy_args in
        [["--config.node-linker=isolated"], ["--config.node-linker=hoisted"], ["--legacy"]]
    {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;
        write_workspace_with_built_lib(&workspace);

        pacquet_cmd(&workspace)
            .with_arg("install")
            .assert()
            .success();
        remove_source_build_output(&workspace);
        pacquet_cmd(&workspace)
            .with_args(["--filter", "app", "deploy", "--prod"])
            .with_args(deploy_args)
            .with_arg("out")
            .assert()
            .success();

        assert!(
            workspace.join("out/node_modules/lib/built.txt").exists(),
            "the deployed workspace project should run its build with {deploy_args:?}",
        );
        drop((root, mock_instance));
    }
}

#[test]
fn deploy_keeps_an_explicit_denial_of_a_workspace_project_build() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_with_built_lib(&workspace);
    append_workspace_yaml_key(&workspace, "allowBuilds", "{ lib: false }");

    pacquet_cmd(&workspace)
        .with_arg("install")
        .assert()
        .success();
    remove_source_build_output(&workspace);
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", "out"])
        .assert()
        .success();

    assert!(!workspace.join("out/node_modules/lib/built.txt").exists());
    let deployed_workspace_manifest =
        fs::read_to_string(workspace.join("out/pnpm-workspace.yaml")).unwrap();
    assert!(
        !deployed_workspace_manifest.contains("lib@file:"),
        "a denied project must not be approved by its dependency path:\n{deployed_workspace_manifest}",
    );
    drop((root, mock_instance));
}

fn write_workspace_with_built_publish_directory(workspace: &Path) {
    append_workspace_yaml_key(workspace, "packages", "\n  - packages/*");
    write_project(
        workspace,
        "app",
        &serde_json::json!({
            "name": "app", "version": "1.0.0", "dependencies": { "lib": "workspace:*" },
        }),
    );
    write_project(
        workspace,
        "lib",
        &serde_json::json!({
            "name": "lib", "version": "1.0.0", "publishConfig": { "directory": "dist" },
        }),
    );
    let publish_directory = workspace.join("packages/lib/dist");
    fs::create_dir_all(&publish_directory).unwrap();
    fs::write(
        publish_directory.join("package.json"),
        serde_json::json!({
            "name": "lib", "version": "1.0.0", "scripts": { "postinstall": BUILD_SCRIPT },
        })
        .to_string(),
    )
    .unwrap();
}

#[test]
fn deploy_builds_the_publish_directory_of_a_workspace_project() {
    for deploy_args in
        [["--config.node-linker=isolated"], ["--config.node-linker=hoisted"], ["--legacy"]]
    {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;
        write_workspace_with_built_publish_directory(&workspace);

        pacquet_cmd(&workspace)
            .with_arg("install")
            .assert()
            .success();
        pacquet_cmd(&workspace)
            .with_args(["--filter", "app", "deploy", "--prod"])
            .with_args(deploy_args)
            .with_arg("out")
            .assert()
            .success();

        assert!(
            workspace.join("out/node_modules/lib/built.txt").exists(),
            "the deployed publish directory should run its build with {deploy_args:?}",
        );
        drop((root, mock_instance));
    }
}
