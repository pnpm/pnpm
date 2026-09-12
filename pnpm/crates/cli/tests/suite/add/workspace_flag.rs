use super::{Path, PathBuf, TempDir, saved_spec, workspace_with_lib, write_json};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use pretty_assertions::assert_eq;
use std::process::Command;

const LIB: &str = "@pnpm.e2e/ws-flag-lib";

fn workspace(settings: &str) -> (TempDir, PathBuf) {
    workspace_with_lib(settings, &[(LIB, "2.0.0")], "packages/app/package.json")
}

fn pnpm_add(dir: &Path, args: &[&str]) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(dir)
        .with_arg("add")
        .with_args(args)
}

fn assert_add_fails(dir: &Path, args: &[&str], needle: &str) {
    let output = pnpm_add(dir, args).output().expect("run pnpm add");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}");
    assert!(!output.status.success(), "`pnpm add {}` should fail", args.join(" "));
    assert!(stderr.contains(needle), "stderr did not mention {needle:?}");
}

/// The default settings never link a bare name to the workspace, so
/// without the flag this add would go to the registry.
#[test]
fn links_the_workspace_package_under_the_rolling_protocol() {
    let (root, app_dir) = workspace("");

    pnpm_add(&app_dir, &["--workspace", LIB]).assert().success();

    assert_eq!(saved_spec(&app_dir, LIB).as_deref(), Some("workspace:*"));
    let linked = app_dir.join("node_modules").join(LIB).join("package.json");
    assert!(linked.exists(), "{} should be linked into the app", linked.display());
    drop(root);
}

#[test]
fn writes_the_version_when_linking_and_the_protocol_are_off() {
    let (root, app_dir) = workspace("linkWorkspacePackages: false\nsaveWorkspaceProtocol: false\n");

    pnpm_add(&app_dir, &["--workspace", LIB, "--lockfile-only"]).assert().success();

    assert_eq!(saved_spec(&app_dir, LIB).as_deref(), Some("workspace:^2.0.0"));
    drop(root);
}

#[test]
fn writes_the_version_when_linking_is_on_and_the_protocol_is_off() {
    let (root, app_dir) = workspace("linkWorkspacePackages: true\nsaveWorkspaceProtocol: false\n");

    pnpm_add(&app_dir, &["--workspace", LIB, "--lockfile-only"]).assert().success();

    assert_eq!(saved_spec(&app_dir, LIB).as_deref(), Some("workspace:^2.0.0"));
    drop(root);
}

#[test]
fn keeps_the_typed_range_operator() {
    let (root, app_dir) = workspace("");

    pnpm_add(&app_dir, &["--workspace", &format!("{LIB}@~2.0.0"), "--lockfile-only"])
        .assert()
        .success();

    assert_eq!(saved_spec(&app_dir, LIB).as_deref(), Some("workspace:~"));
    drop(root);
}

/// A filtered add resolves the selectors against the selected
/// workspace's projects rather than the project the command runs in.
#[test]
fn links_the_workspace_package_into_a_filtered_project() {
    let (root, app_dir) = workspace("");
    let workspace_dir = app_dir.parent().and_then(Path::parent).expect("workspace root");

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace_dir)
        .with_args(["--filter", "ws-app", "add", "--workspace", LIB, "--lockfile-only"])
        .assert()
        .success();

    assert_eq!(saved_spec(&app_dir, LIB).as_deref(), Some("workspace:*"));
    drop(root);
}

#[test]
fn links_the_workspace_package_into_every_recursively_selected_project() {
    for shared_workspace_lockfile in [true, false] {
        let (root, app_dir) =
            workspace(&format!("sharedWorkspaceLockfile: {shared_workspace_lockfile}\n"));
        let workspace_dir = app_dir.parent().and_then(Path::parent).expect("workspace root");
        let second_app_dir = workspace_dir.join("packages/app2");
        std::fs::create_dir_all(&second_app_dir).expect("create second app dir");
        write_json(
            &second_app_dir.join("package.json"),
            &serde_json::json!({ "name": "ws-app-2", "version": "1.0.0" }),
        );

        Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
            .with_current_dir(workspace_dir)
            .with_args(["-r", "--filter", "ws-app*", "add", "--workspace", LIB, "--lockfile-only"])
            .assert()
            .success();

        eprintln!("sharedWorkspaceLockfile={shared_workspace_lockfile}");
        assert_eq!(saved_spec(&app_dir, LIB).as_deref(), Some("workspace:*"));
        assert_eq!(saved_spec(&second_app_dir, LIB).as_deref(), Some("workspace:*"));
        drop(root);
    }
}

#[test]
fn rejects_a_package_no_workspace_project_provides() {
    let (root, app_dir) = workspace("");

    assert_add_fails(
        &app_dir,
        &["--workspace", "@pnpm.e2e/not-in-workspace"],
        r#""@pnpm.e2e/not-in-workspace" not found in the workspace"#,
    );

    assert_eq!(saved_spec(&app_dir, "@pnpm.e2e/not-in-workspace"), None);
    drop(root);
}

/// Rejected before the add touches anything, so `--allow-build`
/// is not persisted to `pnpm-workspace.yaml` by a run that fails.
#[test]
fn is_rejected_with_config_dependencies_and_ecosystem_selectors() {
    let (root, app_dir) = workspace("");
    let workspace_dir = app_dir.parent().and_then(Path::parent).expect("workspace root");
    let yaml_path = workspace_dir.join("pnpm-workspace.yaml");
    let yaml_before = std::fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");

    assert_add_fails(
        &app_dir,
        &["--workspace", "--config", LIB, "--allow-build", "esbuild"],
        "cannot be combined with --workspace",
    );
    for ecosystem_selector in ["crate:serde", "pypi:requests"] {
        assert_add_fails(
            &app_dir,
            &["--workspace", ecosystem_selector, "--allow-build", "esbuild"],
            "--workspace cannot be combined with crate: or pypi: dependencies",
        );
    }

    assert_eq!(
        std::fs::read_to_string(&yaml_path).expect("reread pnpm-workspace.yaml"),
        yaml_before,
    );
    drop(root);
}

/// Rejected before `--allow-build` is persisted, like the other
/// `--workspace` invocations that cannot run.
#[test]
fn is_rejected_outside_a_workspace() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_json(
        &workspace.join("package.json"),
        &serde_json::json!({ "name": "standalone", "version": "1.0.0" }),
    );

    assert_add_fails(
        &workspace,
        &["--workspace", LIB, "--allow-build", "esbuild"],
        "--workspace can only be used inside a workspace",
    );

    assert!(
        !workspace.join("pnpm-workspace.yaml").exists(),
        "a rejected add must not persist --allow-build",
    );
    drop(root);
}
