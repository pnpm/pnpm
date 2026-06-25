use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use std::{fmt::Write as _, fs, path::Path, process::Command};

#[test]
fn deploy_from_shared_lockfile_installs_selected_project() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, true);

    pacquet.with_arg("install").assert().success();
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", "deploy"])
        .assert()
        .success();

    let deploy_dir = workspace.join("deploy");
    let deploy_manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(deploy_dir.join("package.json")).unwrap())
            .unwrap();
    assert_eq!(deploy_manifest["name"], "app");
    assert!(
        deploy_manifest["dependencies"]["lib"]
            .as_str()
            .is_some_and(|version| version.starts_with("lib@file://")),
        "deployed manifest should point workspace dependencies at file URLs: {deploy_manifest:#}",
    );
    assert!(deploy_dir.join("index.js").exists());
    assert!(
        !deploy_dir.join("test.js").exists(),
        "deploy should copy the package packlist by default",
    );
    assert!(deploy_dir.join("pnpm-lock.yaml").exists());
    let lockfile = fs::read_to_string(deploy_dir.join("pnpm-lock.yaml")).unwrap();
    assert!(
        !lockfile.contains("injectWorkspacePackages: true"),
        "deploy lockfile should not preserve injectWorkspacePackages: true:\n{lockfile}",
    );

    let lib_link = deploy_dir.join("node_modules/lib");
    assert!(
        is_symlink_or_junction(&lib_link).unwrap(),
        "prod workspace dependency should be linked into the deploy dir",
    );
    assert!(
        !deploy_dir.join("node_modules/dev-only").exists(),
        "dev-only workspace dependency should not be linked with --prod",
    );

    drop((root, mock_instance));
}

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
fn shared_lockfile_deploy_refuses_non_injected_workspace_before_target_mutation() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);

    let output = pacquet
        .with_args(["--filter", "app", "deploy", "deploy"])
        .output()
        .expect("run pacquet deploy");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("inject-workspace-packages=true"), "unexpected stderr:\n{stderr}");
    assert!(
        !workspace.join("deploy").exists(),
        "non-injected shared-lockfile deploy must fail before creating the target",
    );

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
    assert!(
        stderr.contains("unsafe target") && stderr.contains("outside the workspace"),
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
    assert!(
        stderr.contains("path_escape") && stderr.contains("resolves outside source"),
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
    assert!(
        stderr.contains("ERR_PNPM_INVALID_DEPLOY_TARGET") && stderr.contains("contains a symlink"),
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
    pacquet_fs::symlink_dir(&outside, &workspace.join("out")).unwrap();

    let output = pacquet
        .with_args(["--filter", "app", "deploy", "--legacy", "--force", "out/deploy"])
        .output()
        .expect("run pacquet deploy");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_INVALID_DEPLOY_TARGET")
            && stderr.contains("contains a symlink or junction"),
        "unexpected stderr:\n{stderr}",
    );
    assert!(
        !outside.join("deploy").exists(),
        "deploy must not create output through a linked target parent",
    );

    drop((root, mock_instance));
}

#[test]
fn legacy_deploy_installs_selected_project() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);

    pacquet
        .with_args(["--filter", "app", "deploy", "--legacy", "--prod", "legacy-deploy"])
        .assert()
        .success();

    let deploy_dir = workspace.join("legacy-deploy");
    assert!(deploy_dir.join("index.js").exists());
    assert!(!deploy_dir.join("test.js").exists());
    assert!(deploy_dir.join("node_modules/lib").exists());
    assert!(!deploy_dir.join("node_modules/dev-only").exists());

    drop((root, mock_instance));
}

fn pacquet_cmd(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

fn write_workspace(workspace: &Path, inject_workspace_packages: bool) {
    let mut workspace_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).unwrap();
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    writeln!(
        workspace_yaml,
        "injectWorkspacePackages: {}",
        if inject_workspace_packages { "true" } else { "false" },
    )
    .unwrap();
    fs::write(workspace.join("pnpm-workspace.yaml"), workspace_yaml).unwrap();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "version": "1.0.0", "private": true }).to_string(),
    )
    .unwrap();

    write_project(
        workspace,
        "app",
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "files": ["index.js"],
            "dependencies": { "lib": "workspace:*" },
            "devDependencies": { "dev-only": "workspace:*" },
        }),
    );
    write_project(
        workspace,
        "lib",
        &serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "files": ["index.js"],
        }),
    );
    write_project(
        workspace,
        "dev-only",
        &serde_json::json!({
            "name": "dev-only",
            "version": "1.0.0",
            "files": ["index.js"],
        }),
    );
}

fn write_project(workspace: &Path, dirname: &str, manifest: &serde_json::Value) {
    let dir = workspace.join("packages").join(dirname);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("package.json"), manifest.to_string()).unwrap();
    fs::write(dir.join("index.js"), "").unwrap();
    fs::write(dir.join("test.js"), "").unwrap();
}
