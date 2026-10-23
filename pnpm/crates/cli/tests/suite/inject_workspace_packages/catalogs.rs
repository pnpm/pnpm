use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::Lockfile;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{collections::HashMap, fs, path::Path};

#[derive(Clone, Copy)]
enum Injection {
    Global,
    DependencyMetadata,
}

#[test]
fn relative_injected_workspace_dependencies_resolve_catalogs() {
    for (relative_path, catalog_specifier, injection) in [
        ("../peer", "catalog:project/common", Injection::Global),
        ("./peer", "catalog:", Injection::Global),
        ("../peer", "catalog:", Injection::DependencyMetadata),
        ("./peer", "catalog:project/common", Injection::DependencyMetadata),
    ] {
        let fixture = relative_catalog_fixture(relative_path, catalog_specifier, injection);
        let workspace = &fixture.workspace;
        let peer_manifest_path = workspace.join(relative_path).join("package.json");
        let peer_manifest = fs::read_to_string(&peer_manifest_path).expect("read peer manifest");
        for args in [
            vec!["install", "--lockfile-only", "--no-frozen-lockfile"],
            vec!["install", "--no-frozen-lockfile"],
            vec!["install", "--frozen-lockfile"],
        ] {
            pacquet_in(workspace)
                .with_args(args)
                .assert()
                .success();
        }
        let lockfile = Lockfile::load_wanted_from_dir(workspace)
            .expect("read lockfile")
            .expect("lockfile exists");
        let snapshot = lockfile.snapshots
            .expect("lockfile has snapshots")
            .into_iter()
            .find(|(key, _)| key.to_string().starts_with("peer@file:"))
            .expect("lockfile records the injected peer")
            .1;
        let dependencies = snapshot.dependencies.expect("peer has dependencies");
        let foo: pnpm_lockfile::PkgName = "@pnpm.e2e/foo".parse().expect("parse package name");
        assert_eq!(dependencies[&foo].to_string(), "1.0.0");
        assert_eq!(
            fs::read_to_string(&peer_manifest_path).expect("reread peer manifest"),
            peer_manifest,
        );
    }
}

#[test]
fn ordinary_file_dependencies_do_not_inherit_importer_catalogs() {
    for injection in [Injection::Global, Injection::DependencyMetadata] {
        let fixture = relative_catalog_fixture("../peer", "catalog:project/common", injection);
        let manifest_path = fixture.workspace.join("package.json");
        let mut manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read app manifest"))
                .expect("parse app manifest");
        manifest["dependencies"]["peer"] = serde_json::json!("file:../peer");
        fs::write(manifest_path, manifest.to_string()).expect("write file dependency");
        let output = pacquet_in(&fixture.workspace)
            .with_args(["install", "--lockfile-only", "--no-frozen-lockfile"])
            .output()
            .expect("run install");
        assert!(!output.status.success(), "ordinary file dependency resolved: {output:?}");
        let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
        assert!(stderr.contains("ERR_PNPM_UNSUPPORTED_PROTOCOL"), "stderr: {stderr}");
        assert!(stderr.contains("catalog:project/common"), "stderr: {stderr}");
    }
}

fn relative_catalog_fixture(
    relative_path: &str,
    catalog_specifier: &str,
    injection: Injection,
) -> CommandTempCwd<AddMockedRegistry> {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    let workspace_yaml_path = fixture.workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml = fs::read_to_string(&workspace_yaml_path).expect("read workspace");
    workspace_yaml.push_str(
        "packages: []\npreferFrozenLockfile: false\ncatalog:\n  '@pnpm.e2e/foo': 1.0.0\n\
         catalogs:\n  project/common:\n    '@pnpm.e2e/foo': 1.0.0\n",
    );
    let mut manifest = serde_json::json!({
        "name": "app",
        "version": "1.0.0",
        "private": true,
        "dependencies": {
            "peer": format!("workspace:{relative_path}"),
            "@pnpm.e2e/foo": catalog_specifier,
        },
    });
    match injection {
        Injection::Global => workspace_yaml.push_str("injectWorkspacePackages: true\n"),
        Injection::DependencyMetadata => {
            manifest["dependenciesMeta"] = serde_json::json!({ "peer": { "injected": true } });
        }
    }
    fs::write(workspace_yaml_path, workspace_yaml).expect("write workspace");
    fs::write(fixture.workspace.join("package.json"), manifest.to_string())
        .expect("write app manifest");
    let peer_dir = fixture.workspace.join(relative_path);
    fs::create_dir_all(&peer_dir).expect("create peer directory");
    fs::write(
        peer_dir.join("package.json"),
        serde_json::json!({
            "name": "peer",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/foo": catalog_specifier },
        })
        .to_string(),
    )
    .expect("write peer manifest");
    fixture
}

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

    let lockfile_dir = if shared_lockfile { workspace.clone() } else { workspace.join("app") };
    let lockfile_path = lockfile_dir.join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read generated lockfile");
    assert_eq!(
        recorded_lib_peers(&lockfile_dir),
        HashMap::from([("@pnpm.e2e/foo".to_string(), peer_specifier.to_string())]),
        "the lockfile records the peer as the manifest writes it",
    );
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

/// The `peerDependencies` of the `packages` entry of the injected `lib`.
fn recorded_lib_peers(lockfile_dir: &Path) -> HashMap<String, String> {
    Lockfile::load_wanted_from_dir(lockfile_dir)
        .expect("read the lockfile")
        .expect("the lockfile exists")
        .packages
        .expect("the lockfile has packages")
        .into_iter()
        .find(|(key, _)| key.to_string().starts_with("lib@"))
        .expect("the lockfile has an entry for lib")
        .1
        .peer_dependencies
        .expect("lib records its peers")
}
