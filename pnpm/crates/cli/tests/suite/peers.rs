use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use serde_json::Value;
use std::{fs, process::Command};

fn write_project(workspace: &std::path::Path, relative_dir: &str, name: &str) {
    let project_dir = workspace.join(relative_dir);
    fs::create_dir_all(&project_dir).expect("create project directory");
    fs::write(
        project_dir.join("package.json"),
        serde_json::json!({ "name": name, "version": "1.0.0" }).to_string(),
    )
    .expect("write project manifest");
}

fn run_peers(workspace: &std::path::Path, args: &[&str]) -> Value {
    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_args(args)
        .output()
        .expect("run pnpm peers");
    assert!(output.status.success(), "peers should succeed: {output:?}");
    serde_json::from_slice(&output.stdout).expect("parse peers JSON")
}

#[test]
fn peers_is_recursive_by_default_and_honors_filters() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    fs::write(workspace.join("package.json"), r#"{ "name": "root", "version": "1.0.0" }"#)
        .expect("write root manifest");
    write_project(&workspace, "packages/app-a", "app-a");
    write_project(&workspace, "packages/app-b", "app-b");
    fs::write(
        workspace.join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\nimporters:\n  .: {}\n  packages/app-a: {}\n  packages/app-b: {}\n",
    )
    .expect("write lockfile");

    let all = run_peers(&workspace, &["peers", "--lockfile-only", "--json"]);
    assert_eq!(all.as_object().map(serde_json::Map::len), Some(3));

    let filtered =
        run_peers(&workspace, &["--filter", "app-a", "peers", "--lockfile-only", "--json"]);
    let filtered = filtered.as_object().expect("filtered peer issues object");
    assert_eq!(filtered.len(), 1);
    assert!(filtered.contains_key("packages/app-a"));

    drop(root);
}

#[test]
fn recursive_peers_uses_the_active_dedicated_lockfile() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write workspace manifest");
    fs::write(workspace.join("package.json"), r#"{ "name": "root", "version": "1.0.0" }"#)
        .expect("write root manifest");
    write_project(&workspace, "packages/app", "app");
    let app = workspace.join("packages/app");
    fs::write(app.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\nimporters:\n  .: {}\n")
        .expect("write dedicated lockfile");

    let issues = run_peers(&app, &["peers", "--lockfile-only", "--json"]);
    let issues = issues.as_object().expect("peer issues object");
    assert_eq!(issues.len(), 1);
    assert!(issues.contains_key("."));

    drop(root);
}

/// `pnpm peers check` is the documented spelling — pnpm's own dedupe output
/// tells users to run it — so it must behave like the bare command.
#[test]
fn peers_accepts_the_check_subcommand() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{ "name": "root", "version": "1.0.0" }"#)
        .expect("write root manifest");
    fs::write(workspace.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\nimporters:\n  .: {}\n")
        .expect("write lockfile");

    let bare = run_peers(&workspace, &["peers", "--lockfile-only", "--json"]);
    let checked = run_peers(&workspace, &["peers", "check", "--lockfile-only", "--json"]);
    assert_eq!(bare, checked);

    drop(root);
}

#[test]
fn peers_rejects_an_unknown_subcommand() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{ "name": "root", "version": "1.0.0" }"#)
        .expect("write root manifest");

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["peers", "list"])
        .output()
        .expect("run pnpm peers list");

    assert_eq!(output.status.code(), Some(1), "an unknown subcommand must exit 1: {output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("Usage: pnpm peers"), "the help must be printed; got:\n{stdout}");

    drop(root);
}

/// A resolving install reports what its resolution turned up, matching
/// pnpm: one line naming `pnpm peers check`, and the install still
/// succeeds.
#[test]
fn a_resolving_install_warns_about_peer_dependency_issues() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_peer_conflict_manifest(&workspace);

    let output = pacquet.with_arg("install").output().expect("run pnpm install");
    assert!(output.status.success(), "install must succeed: {output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains(PEERS_CHECK_HINT), "stdout:\n{stdout}");

    drop((root, mock_instance));
}

/// `strictPeerDependencies` turns the same verdict into
/// `ERR_PNPM_PEER_DEP_ISSUES`. pnpm fails after the artifacts are
/// written, so `node_modules` is still there for the user to inspect
/// while they decide how to answer the hints.
#[test]
fn strict_peer_dependencies_fails_a_resolving_install() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_peer_conflict_manifest(&workspace);
    fs::write(workspace.join("pnpm-workspace.yaml"), "strictPeerDependencies: true\n")
        .expect("write workspace manifest");

    let output = pacquet.with_arg("install").output().expect("run pnpm install");
    assert!(!output.status.success(), "install must fail: {output:?}");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.contains("ERR_PNPM_PEER_DEP_ISSUES"), "stderr:\n{stderr}");
    assert!(stderr.contains("unmet peer @pnpm.e2e/foo"), "stderr:\n{stderr}");
    assert!(stderr.contains("strictPeerDependencies: false"), "stderr:\n{stderr}");
    assert!(workspace.join("node_modules").exists(), "the install must still have materialized");

    drop((root, mock_instance));
}

/// The gap tracked as pnpm/pnpm#14098, kept deliberately: peer issues
/// are a byproduct of resolution, so an install that skips resolution
/// reports nothing and `pnpm peers check` is what answers for the tree.
/// Both stacks behave this way.
#[test]
fn an_up_to_date_install_does_not_recheck_peers() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_peer_conflict_manifest(&workspace);

    pacquet.with_arg("install").assert().success();

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_arg("install")
        .output()
        .expect("re-run pnpm install");
    assert!(output.status.success(), "the repeat install must succeed: {output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(!stdout.contains(PEERS_CHECK_HINT), "stdout:\n{stdout}");

    let peers = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["peers", "check"])
        .output()
        .expect("run pnpm peers check");
    assert_eq!(peers.status.code(), Some(1), "peers check still reports them: {peers:?}");

    drop((root, mock_instance));
}

/// `peerDependencyRules` are applied before the verdict, so a rule that
/// covers every issue leaves the install with nothing to report — and
/// nothing to fail over under `strictPeerDependencies`.
#[test]
fn peer_dependency_rules_settle_the_install_verdict() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_peer_conflict_manifest(&workspace);
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "strictPeerDependencies: true\npeerDependencyRules:\n  allowAny:\n    - '@pnpm.e2e/foo'\n",
    )
    .expect("write workspace manifest");

    let output = pacquet.with_arg("install").output().expect("run pnpm install");
    assert!(output.status.success(), "install must succeed: {output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(!stdout.contains(PEERS_CHECK_HINT), "stdout:\n{stdout}");

    drop((root, mock_instance));
}

const PEERS_CHECK_HINT: &str =
    r#"Issues with peer dependencies found. Run "pnpm peers check" to list them."#;

/// `@pnpm.e2e/has-foo100-peer` wants `@pnpm.e2e/foo@100.0.0`; pinning
/// `2.0.0` alongside it makes that peer resolvable but unsatisfying — a
/// bad peer rather than a missing one.
fn write_peer_conflict_manifest(workspace: &std::path::Path) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/has-foo100-peer": "1.0.0",
                "@pnpm.e2e/foo": "2.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
}
