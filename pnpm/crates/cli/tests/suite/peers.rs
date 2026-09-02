use crate::_utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    command_env::CommandTestExt,
};
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
    write_peer_conflict_manifest(&workspace, "peer-conflict");

    let output = pacquet.with_arg("install").output().expect("run pnpm install");
    assert!(output.status.success(), "install must succeed: {output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains(PEERS_CHECK_HINT), "stdout:\n{stdout}");

    drop((root, mock_instance));
}

/// `--lockfile-only` finishes through its own completion path, which
/// has to carry the resolver's peer-issue candidates just like the
/// materializing one.
#[test]
fn a_lockfile_only_install_warns_about_peer_dependency_issues() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_peer_conflict_manifest(&workspace, "peer-conflict");

    let output = pacquet
        .with_args(["install", "--lockfile-only"])
        .output()
        .expect("run pnpm install --lockfile-only");
    assert!(output.status.success(), "lockfile-only install must succeed: {output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains(PEERS_CHECK_HINT), "stdout:\n{stdout}");
    assert!(workspace.join("pnpm-lock.yaml").exists(), "the lockfile must be written");
    assert!(
        !workspace.join("node_modules").exists(),
        "lockfile-only install must not materialize node_modules",
    );

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
    write_peer_conflict_manifest(&workspace, "peer-conflict");
    fs::write(workspace.join("pnpm-workspace.yaml"), "strictPeerDependencies: true\n")
        .expect("write workspace manifest");

    let output = pacquet.with_arg("install").output().expect("run pnpm install");
    assert!(!output.status.success(), "install must fail: {output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(
        stdout.contains("[ERR_PNPM_PEER_DEP_ISSUES] Unmet peer dependencies"),
        "stdout:\n{stdout}",
    );
    assert!(stdout.contains("unmet peer @pnpm.e2e/foo"), "stdout:\n{stdout}");
    assert!(stdout.contains("strictPeerDependencies: false"), "stdout:\n{stdout}");
    assert!(!stdout.contains("autoInstallPeers: true"), "stdout:\n{stdout}");
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
    write_peer_conflict_manifest(&workspace, "peer-conflict");

    pacquet.with_arg("install").assert().success();

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_arg("install")
        .output()
        .expect("re-run pnpm install");
    assert!(output.status.success(), "the repeat install must succeed: {output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(!stdout.contains(PEERS_CHECK_HINT), "stdout:\n{stdout}");

    let peers = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
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
    write_peer_conflict_manifest(&workspace, "peer-conflict");
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
fn write_peer_conflict_manifest(project_dir: &std::path::Path, name: &str) {
    fs::create_dir_all(project_dir).expect("create the project directory");
    fs::write(
        project_dir.join("package.json"),
        serde_json::json!({
            "name": name,
            "version": "1.0.0",
            "dependencies": {
                "@pnpm.e2e/has-foo100-peer": "1.0.0",
                "@pnpm.e2e/foo": "2.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
}

/// A workspace project's own `peerDependencies` are checked against the
/// dependencies of the projects that link to it. Peer resolution never
/// sees them — a `link:` node's peers belong to the linked importer —
/// so the consuming importer reaches the report by a route of its own.
fn write_linked_peer_workspace(workspace: &std::path::Path) {
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    fs::write(workspace.join("package.json"), r#"{ "name": "root", "version": "1.0.0" }"#)
        .expect("write root manifest");
    let lib = workspace.join("packages/lib");
    fs::create_dir_all(&lib).expect("create the linked project");
    fs::write(
        lib.join("package.json"),
        serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "peerDependencies": { "@pnpm.e2e/foo": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write the linked manifest");
    let app = workspace.join("packages/app");
    fs::create_dir_all(&app).expect("create the consuming project");
    fs::write(
        app.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "lib": "workspace:*", "@pnpm.e2e/foo": "2.0.0" },
        })
        .to_string(),
    )
    .expect("write the consuming manifest");
}

#[test]
fn a_linked_workspace_packages_unmet_peer_is_warned_about() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_linked_peer_workspace(&workspace);

    let output = pacquet.with_arg("install").output().expect("run pnpm install");
    assert!(output.status.success(), "install must succeed: {output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains(PEERS_CHECK_HINT), "stdout:\n{stdout}");

    drop((root, mock_instance));
}

#[test]
fn strict_peer_dependencies_fails_on_a_linked_workspace_packages_unmet_peer() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_linked_peer_workspace(&workspace);
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nstrictPeerDependencies: true\n",
    )
    .expect("rewrite workspace manifest");

    let output = pacquet.with_arg("install").output().expect("run pnpm install");
    assert!(!output.status.success(), "install must fail: {output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(
        stdout.contains("[ERR_PNPM_PEER_DEP_ISSUES] Unmet peer dependencies"),
        "stdout:\n{stdout}",
    );
    assert!(stdout.contains("unmet peer @pnpm.e2e/foo"), "stdout:\n{stdout}");

    drop((root, mock_instance));
}

#[test]
fn auto_installed_peer_of_linked_workspace_package_is_not_reported_missing() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nstrictPeerDependencies: true\nautoInstallPeers: true\n",
    )
    .expect("write workspace manifest");
    fs::write(workspace.join("package.json"), r#"{ "name": "root", "private": true }"#)
        .expect("write root manifest");

    let lib = workspace.join("packages/lib");
    fs::create_dir_all(&lib).expect("create the linked project");
    fs::write(
        lib.join("package.json"),
        serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "peerDependencies": { "@pnpm.e2e/foo": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write the linked manifest");

    let app = workspace.join("packages/app");
    fs::create_dir_all(&app).expect("create the consuming project");
    fs::write(
        app.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "lib": "workspace:*" },
        })
        .to_string(),
    )
    .expect("write the consuming manifest");

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();
    let lockfile = _utils::read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(_utils::importer_version(&lockfile, "packages/lib", "@pnpm.e2e/foo"), "1.0.0");
    let _ = run_peers(&workspace, &["peers", "check", "--lockfile-only", "--json"]);

    drop((root, mock_instance));
}

#[test]
fn auto_installed_workspace_peer_is_resolved_from_the_linked_importer() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/**\nstrictPeerDependencies: true\nautoInstallPeers: true\nlinkWorkspacePackages: true\n",
    )
    .expect("write workspace manifest");
    fs::write(workspace.join("package.json"), r#"{ "name": "root", "private": true }"#)
        .expect("write root manifest");

    let peer = workspace.join("packages/peers/foo");
    fs::create_dir_all(&peer).expect("create the peer project");
    fs::write(peer.join("package.json"), r#"{ "name": "@pnpm.e2e/foo", "version": "1.0.0" }"#)
        .expect("write the peer manifest");

    let lib = workspace.join("packages/lib");
    fs::create_dir_all(&lib).expect("create the linked project");
    fs::write(
        lib.join("package.json"),
        serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "peerDependencies": { "@pnpm.e2e/foo": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write the linked manifest");

    let app = workspace.join("packages/apps/app");
    fs::create_dir_all(&app).expect("create the consuming project");
    fs::write(
        app.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "lib": "workspace:*" },
        })
        .to_string(),
    )
    .expect("write the consuming manifest");

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();
    let lockfile = _utils::read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(
        _utils::importer_version(&lockfile, "packages/lib", "@pnpm.e2e/foo"),
        "link:../peers/foo",
    );
    let _ = run_peers(&workspace, &["peers", "check", "--lockfile-only", "--json"]);

    drop((root, mock_instance));
}

#[test]
fn incompatible_injected_auto_installed_peer_is_reported() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/**\nautoInstallPeers: true\ninjectWorkspacePackages: true\ndedupeInjectedDeps: false\n",
    )
    .expect("write workspace manifest");
    fs::write(workspace.join("package.json"), r#"{ "name": "root", "private": true }"#)
        .expect("write root manifest");

    let peer = workspace.join("packages/peers/foo");
    fs::create_dir_all(&peer).expect("create the peer project");
    fs::write(peer.join("package.json"), r#"{ "name": "@pnpm.e2e/foo", "version": "1.0.0" }"#)
        .expect("write the peer manifest");

    let lib = workspace.join("packages/lib");
    fs::create_dir_all(&lib).expect("create the linked project");
    fs::write(
        lib.join("package.json"),
        serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "peerDependencies": { "@pnpm.e2e/foo": "workspace:*" },
        })
        .to_string(),
    )
    .expect("write the linked manifest");

    let app = workspace.join("packages/apps/app");
    fs::create_dir_all(&app).expect("create the consuming project");
    fs::write(
        app.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "lib": "link:../../lib" },
        })
        .to_string(),
    )
    .expect("write the consuming manifest");

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();
    let lockfile = _utils::read_lockfile(&workspace.join("pnpm-lock.yaml"));
    let peer_version = _utils::importer_version(&lockfile, "packages/lib", "@pnpm.e2e/foo");
    assert!(
        peer_version.starts_with("file:"),
        "the auto-installed peer should be injected, got {peer_version}",
    );
    fs::write(
        lib.join("package.json"),
        serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "peerDependencies": { "@pnpm.e2e/foo": "2.0.0" },
        })
        .to_string(),
    )
    .expect("make the injected peer incompatible");

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["peers", "check", "--lockfile-only", "--json"])
        .output()
        .expect("inspect injected peers");
    assert_eq!(output.status.code(), Some(1), "the injected peer is incompatible: {output:?}");
    let issues: Value = serde_json::from_slice(&output.stdout).expect("parse peers JSON");
    assert_eq!(issues["packages/apps/app"]["bad"]["@pnpm.e2e/foo"][0]["foundVersion"], "1.0.0");

    drop((root, mock_instance));
}

#[test]
fn catalog_peer_of_a_linked_workspace_package_is_resolved() {
    assert_catalog_peer_of_workspace_package_is_resolved(false);
}

#[test]
fn catalog_peer_of_an_injected_workspace_package_is_resolved() {
    assert_catalog_peer_of_workspace_package_is_resolved(true);
}

#[test]
fn ignored_workspace_does_not_require_workspace_catalogs_for_peer_inspection() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\ncatalog:\n  '@pnpm.e2e/foo': 1.0.0\n",
    )
    .expect("write workspace manifest");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "dependencies": { "lib": "workspace:*", "@pnpm.e2e/foo": "catalog:" },
        })
        .to_string(),
    )
    .expect("write root manifest");
    let lib = workspace.join("packages/lib");
    fs::create_dir_all(&lib).expect("create the workspace library");
    fs::write(
        lib.join("package.json"),
        serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "peerDependencies": { "@pnpm.e2e/foo": "catalog:" },
        })
        .to_string(),
    )
    .expect("write the library manifest");

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();
    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["--ignore-workspace", "peers", "check", "--lockfile-only", "--json"])
        .output()
        .expect("inspect peers without workspace configuration");
    assert_eq!(output.status.code(), Some(1), "the raw catalog range remains unmet: {output:?}");
    assert!(output.stderr.is_empty(), "inspection must not fail with a diagnostic: {output:?}");
    let issues: Value = serde_json::from_slice(&output.stdout).expect("parse peers JSON");
    assert_eq!(issues["."]["bad"]["@pnpm.e2e/foo"][0]["wantedRange"], "catalog:");

    drop((root, mock_instance));
}

#[test]
fn standalone_install_does_not_require_catalogs_for_linked_peers() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::remove_file(workspace.join("pnpm-workspace.yaml"))
        .expect("remove the mock registry's workspace manifest");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "lib": "link:./lib", "@pnpm.e2e/foo": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write app manifest");
    let lib = workspace.join("lib");
    fs::create_dir_all(&lib).expect("create linked library");
    fs::write(
        lib.join("package.json"),
        serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "peerDependencies": { "@pnpm.e2e/foo": "catalog:" },
        })
        .to_string(),
    )
    .expect("write linked library manifest");

    let output = pacquet.with_arg("install").output().expect("install standalone project");
    assert!(
        output.status.success(),
        "install must report rather than reject the raw range: {output:?}",
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains(PEERS_CHECK_HINT), "stdout:\n{stdout}");

    drop((root, mock_instance));
}

#[test]
fn workspace_without_catalogs_does_not_reject_an_injected_catalog_peer() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write workspace manifest");
    fs::write(workspace.join("package.json"), r#"{ "name": "root", "version": "1.0.0" }"#)
        .expect("write root manifest");

    let lib = workspace.join("packages/lib");
    fs::create_dir_all(&lib).expect("create the workspace library");
    fs::write(
        lib.join("package.json"),
        serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "peerDependencies": { "@pnpm.e2e/foo": "catalog:" },
        })
        .to_string(),
    )
    .expect("write the library manifest");

    let app = workspace.join("packages/app");
    fs::create_dir_all(&app).expect("create the consuming project");
    fs::write(
        app.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "lib": "workspace:*", "@pnpm.e2e/foo": "1.0.0" },
            "dependenciesMeta": { "lib": { "injected": true } },
        })
        .to_string(),
    )
    .expect("write the app manifest");

    let output = pacquet
        .with_args(["--filter", "app", "install"])
        .output()
        .expect("install workspace package");
    assert!(
        output.status.success(),
        "install must report rather than reject the raw range: {output:?}",
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains(PEERS_CHECK_HINT), "stdout:\n{stdout}");

    drop((root, mock_instance));
}

fn assert_catalog_peer_of_workspace_package_is_resolved(injected: bool) {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\ncatalog:\n  '@pnpm.e2e/foo': 1.0.0\nstrictPeerDependencies: true\n",
    )
    .expect("write workspace manifest");
    fs::write(workspace.join("package.json"), r#"{ "name": "root", "version": "1.0.0" }"#)
        .expect("write root manifest");

    let lib = workspace.join("packages/lib");
    fs::create_dir_all(&lib).expect("create the workspace library");
    fs::write(
        lib.join("package.json"),
        serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "peerDependencies": { "@pnpm.e2e/foo": "catalog:" },
        })
        .to_string(),
    )
    .expect("write the library manifest");

    let app = workspace.join("packages/app");
    fs::create_dir_all(&app).expect("create the consuming project");
    let mut manifest = serde_json::json!({
        "name": "app",
        "version": "1.0.0",
        "dependencies": { "lib": "workspace:*", "@pnpm.e2e/foo": "catalog:" },
    });
    if injected {
        manifest["dependenciesMeta"] = serde_json::json!({ "lib": { "injected": true } });
    }
    fs::write(app.join("package.json"), manifest.to_string()).expect("write the app manifest");

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();
    let _ = run_peers(&workspace, &["peers", "check", "--lockfile-only", "--json"]);

    drop((root, mock_instance));
}

/// A `--filter`ed install acts only on the projects it selected, so its
/// verdict covers only those. The lockfile still holds the unselected
/// importers, and an unrelated project's unmet peer must not fail a run
/// that never touched it.
#[test]
fn a_filtered_install_only_reports_the_projects_it_installed() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    fs::write(workspace.join("package.json"), r#"{ "name": "root", "version": "1.0.0" }"#)
        .expect("write root manifest");
    write_peer_conflict_manifest(&workspace.join("packages/dirty"), "dirty");
    let clean = workspace.join("packages/clean");
    fs::create_dir_all(&clean).expect("create the clean project");
    let write_clean = |dependencies: Value| {
        fs::write(
            clean.join("package.json"),
            serde_json::json!({ "name": "clean", "version": "1.0.0", "dependencies": dependencies })
                .to_string(),
        )
        .expect("write the clean manifest");
    };
    write_clean(serde_json::json!({ "@pnpm.e2e/foo": "2.0.0" }));

    pacquet.with_arg("install").assert().success();

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nstrictPeerDependencies: true\n",
    )
    .expect("rewrite workspace manifest");

    // Each run below has to actually resolve — an up-to-date lockfile
    // skips the report, which would pass both assertions for the wrong
    // reason — so every one is preceded by an edit to `clean`.
    let install_and_resolve = |args: &[&str], dependencies: Value| {
        write_clean(dependencies);
        Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
            .with_current_dir(&workspace)
            .without_ambient_pnpm_config()
            .with_args(args)
            .output()
            .expect("run pnpm install")
    };

    // The workspace really does hold a strict-failing peer issue: the
    // control that makes the filtered run below say something about
    // filtering rather than about peer reporting being broken.
    let unfiltered = install_and_resolve(
        &["install"],
        serde_json::json!({ "@pnpm.e2e/foo": "2.0.0", "@pnpm.e2e/bar": "100.0.0" }),
    );
    assert!(!unfiltered.status.success(), "the unfiltered install must fail: {unfiltered:?}");
    let stdout = String::from_utf8(unfiltered.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("[ERR_PNPM_PEER_DEP_ISSUES]"), "stdout:\n{stdout}");

    let filtered = install_and_resolve(
        &["--filter", "clean", "install"],
        serde_json::json!({
            "@pnpm.e2e/foo": "2.0.0",
            "@pnpm.e2e/bar": "100.0.0",
            "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
        }),
    );
    assert!(filtered.status.success(), "the filtered install must succeed: {filtered:?}");

    drop((root, mock_instance));
}
