use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, flatten_miette_report, fs, pacquet_cmd,
    write_project, write_workspace,
};
use assert_cmd::assert::OutputAssertExt;

#[test]
fn deploy_refuses_non_empty_target_without_force() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    fs::create_dir_all(workspace.join("deploy")).unwrap();
    fs::write(workspace.join("deploy/keep.txt"), "keep").unwrap();

    let output = pacquet
        .with_args(["--filter", "app", "deploy", "--legacy", "deploy"])
        .output()
        .expect("run pacquet deploy");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_DEPLOY_DIR_NOT_EMPTY") && stderr.contains("empty"),
        "unexpected stderr:\n{stderr}",
    );
    assert_eq!(fs::read_to_string(workspace.join("deploy/keep.txt")).unwrap(), "keep");

    drop((root, mock_instance));
}

#[test]
fn force_deploy_rejects_out_of_scope_target_without_deleting_it() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    let outside = root.path().join("outside-deploy");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("keep.txt"), "keep").unwrap();

    let output = pacquet
        .with_args(["--filter", "app", "deploy", "--legacy", "--force", outside.to_str().unwrap()])
        .output()
        .expect("run pacquet deploy");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let flattened = flatten_miette_report(&stderr);
    assert!(
        flattened.contains("unsafe target") && flattened.contains("outside the workspace"),
        "unexpected stderr:\n{stderr}",
    );
    assert_eq!(fs::read_to_string(outside.join("keep.txt")).unwrap(), "keep");

    drop((root, mock_instance));
}

#[cfg(unix)]
#[test]
fn deploy_all_files_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    let mut workspace_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).unwrap();
    workspace_yaml.push_str("deployAllFiles: true\n");
    fs::write(workspace.join("pnpm-workspace.yaml"), workspace_yaml).unwrap();
    let outside = root.path().join("outside-source");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    symlink(&outside, workspace.join("packages/app/outside")).unwrap();

    let output = pacquet
        .with_args(["--filter", "app", "deploy", "--legacy", "deploy"])
        .output()
        .expect("run pacquet deploy");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let flattened = flatten_miette_report(&stderr);
    assert!(
        flattened.contains("ERR_PNPM_DIRECTORY_FETCHER_PATH_ESCAPE")
            && flattened.contains("resolves outside source directory"),
        "unexpected stderr:\n{stderr}",
    );
    assert!(
        !workspace.join("deploy/outside/secret.txt").exists(),
        "deploy must not copy files reached through an outside symlink",
    );

    drop((root, mock_instance));
}

#[cfg(unix)]
#[test]
fn deploy_rejects_symlinked_target_parent() {
    use std::os::unix::fs::symlink;

    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    let outside = root.path().join("outside-target");
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, workspace.join("out")).unwrap();

    let output = pacquet
        .with_args(["--filter", "app", "deploy", "--legacy", "--force", "out/deploy"])
        .output()
        .expect("run pacquet deploy");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let flattened = flatten_miette_report(&stderr);
    assert!(
        flattened.contains("ERR_PNPM_INVALID_DEPLOY_TARGET")
            && flattened.contains("contains a symlink"),
        "unexpected stderr:\n{stderr}",
    );
    assert!(
        !outside.join("deploy").exists(),
        "deploy must not create output through a symlinked target parent",
    );

    drop((root, mock_instance));
}

#[cfg(windows)]
#[test]
fn deploy_rejects_linked_target_parent() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    let outside = root.path().join("outside-target");
    fs::create_dir_all(&outside).unwrap();
    pnpm_fs::symlink_dir(&outside, &workspace.join("out")).unwrap();

    let output = pacquet
        .with_args(["--filter", "app", "deploy", "--legacy", "--force", "out/deploy"])
        .output()
        .expect("run pacquet deploy");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let flattened = flatten_miette_report(&stderr);
    assert!(
        flattened.contains("ERR_PNPM_INVALID_DEPLOY_TARGET")
            && flattened.contains("contains a symlink or junction"),
        "unexpected stderr:\n{stderr}",
    );
    assert!(
        !outside.join("deploy").exists(),
        "deploy must not create output through a linked target parent",
    );

    drop((root, mock_instance));
}

/// `deploy` copies a project through the directory fetcher's packlist
/// mode, so the project's `files` field is what decides the deployed
/// file set.
#[test]
fn deployed_files_field_does_not_match_at_depth() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, true);
    write_project(
        &workspace,
        "packs-its-own-src",
        &serde_json::json!({
            "name": "packs-its-own-src",
            "version": "1.0.0",
            "main": "src/index.js",
            "files": ["src"],
        }),
    );
    let project = workspace.join("packages/packs-its-own-src");
    for path in ["src/index.js", "example/src/App.js"] {
        let file = project.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, "").unwrap();
    }

    pacquet.with_arg("install").assert().success();
    pacquet_cmd(&workspace)
        .with_args(["--filter", "packs-its-own-src", "deploy", "deploy"])
        .assert()
        .success();

    let deploy_dir = workspace.join("deploy");
    assert!(deploy_dir.join("src/index.js").exists(), "the published src is deployed");
    assert!(!deploy_dir.join("example").exists(), "the example app is not deployed");

    drop((root, mock_instance));
}
