use super::{
    PEERS_CHECK_HINT,
    run_peers,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{
    AddMockedRegistry,
    CommandTempCwd,
};
use serde_json::Value;
use std::{
    fs,
    process::Command,
};

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
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
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

    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
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
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
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

    let output = pacquet
        .with_arg("install")
        .output()
        .expect("install standalone project");
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
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
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
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
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

    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    let _ = run_peers(&workspace, &["peers", "check", "--lockfile-only", "--json"]);

    drop((root, mock_instance));
}
