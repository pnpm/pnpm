use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::fs;

#[test]
fn frozen_install_accepts_default_catalog_peers_with_a_shared_lockfile() {
    assert_frozen_install_accepts_catalog_peers(true, "catalog:");
}

#[test]
fn frozen_install_accepts_default_catalog_peers_with_dedicated_lockfiles() {
    assert_frozen_install_accepts_catalog_peers(false, "catalog:");
}

#[test]
fn frozen_install_accepts_named_catalog_peers_with_a_shared_lockfile() {
    assert_frozen_install_accepts_catalog_peers(true, "catalog:peers");
}

#[test]
fn frozen_install_accepts_named_catalog_peers_with_dedicated_lockfiles() {
    assert_frozen_install_accepts_catalog_peers(false, "catalog:peers");
}

fn assert_frozen_install_accepts_catalog_peers(shared_lockfile: bool, peer_specifier: &str) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - app\n  - lib\ninjectWorkspacePackages: true\n\
             dedupeInjectedDeps: false\nsharedWorkspaceLockfile: {shared_lockfile}\n\
             catalog:\n  '@pnpm.e2e/foo': 1.0.0\n\
             catalogs:\n  peers:\n    '@pnpm.e2e/foo': 1.0.0\n",
        ),
    )
    .expect("write workspace manifest");
    fs::write(workspace.join("package.json"), r#"{"name":"root","private":true}"#)
        .expect("write root manifest");
    fs::create_dir_all(workspace.join("lib")).expect("create library");
    let lib_manifest = serde_json::json!({
        "name": "lib",
        "version": "1.0.0",
        "peerDependencies": { "@pnpm.e2e/foo": peer_specifier },
    });
    fs::write(workspace.join("lib/package.json"), lib_manifest.to_string())
        .expect("write library manifest");
    fs::create_dir_all(workspace.join("app")).expect("create app");
    fs::write(
        workspace.join("app/package.json"),
        serde_json::json!({
            "name": "app",
            "private": true,
            "dependencies": { "lib": "workspace:*", "@pnpm.e2e/foo": peer_specifier },
        })
        .to_string(),
    )
    .expect("write app manifest");

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    let lockfile_path = if shared_lockfile {
        workspace.join("pnpm-lock.yaml")
    } else {
        workspace.join("app/pnpm-lock.yaml")
    };
    let lockfile = fs::read_to_string(&lockfile_path).expect("read generated lockfile");
    let mut changed_manifest = lib_manifest;
    changed_manifest["peerDependencies"]["@pnpm.e2e/foo"] = serde_json::json!("2.0.0");
    fs::write(workspace.join("lib/package.json"), changed_manifest.to_string())
        .expect("change library peer range");
    let output = pacquet_in(&workspace)
        .with_args(["--filter", "app", "install", "--frozen-lockfile"])
        .output()
        .expect("run frozen install after changing peer range");
    assert!(!output.status.success(), "a changed peer range must be rejected: {output:?}");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.contains("ERR_PNPM_OUTDATED_LOCKFILE"), "stderr: {stderr}");
    assert!(stderr.contains(r#"local dependency "lib""#), "stderr: {stderr}");
    assert_eq!(fs::read_to_string(lockfile_path).expect("read unchanged lockfile"), lockfile);

    drop((root, mock_instance));
}
