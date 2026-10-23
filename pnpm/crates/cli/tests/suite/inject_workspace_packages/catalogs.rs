use crate::_utils::{append_workspace_yaml_key, pacquet_in};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::Lockfile;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{collections::HashMap, fs, path::Path};

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

/// A `catalog:` dependency of a package injected through a relative path
/// (`workspace:../lib`) resolves through the catalogs, as one of a package
/// injected by name does. The package sits outside the workspace, so
/// nothing else resolves its specifiers. Both resolutions that walk the
/// package are covered: recalculating the graph over a lockfile that
/// recorded the version the package pinned itself, and a fresh lockfile.
#[test]
fn install_resolves_the_catalog_deps_of_a_package_injected_by_relative_path() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "packages", "[]");
    append_workspace_yaml_key(&workspace, "injectWorkspacePackages", "true");
    append_workspace_yaml_key(&workspace, "preferFrozenLockfile", "false");
    append_workspace_yaml_key(
        &workspace,
        "catalogs",
        "{ project/common: { '@pnpm.e2e/foo': 1.0.0 } }",
    );
    let lib = root.path().join("lib");
    fs::create_dir_all(&lib).expect("create lib");
    let lib_manifest = |foo_specifier: &str| {
        serde_json::json!({
            "name": "lib",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/foo": foo_specifier },
        })
        .to_string()
    };
    fs::write(lib.join("package.json"), lib_manifest("1.0.0")).expect("write lib manifest");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "app",
            "private": true,
            "dependencies": { "lib": "workspace:../lib", "@pnpm.e2e/foo": "catalog:project/common" },
        })
        .to_string(),
    )
    .expect("write app manifest");
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let lib_manifest = lib_manifest("catalog:project/common");
    fs::write(lib.join("package.json"), &lib_manifest).expect("switch lib to the catalog");
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove the lockfile");
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(lockfile.contains("lib@file:../lib"), "lockfile:\n{lockfile}");
    let injected_copy =
        fs::canonicalize(workspace.join("node_modules/lib")).expect("resolve node_modules/lib");
    let foo_manifest = fs::read_to_string(
        injected_copy
            .parent()
            .expect("the injected copy sits in a node_modules directory")
            .join("@pnpm.e2e/foo/package.json"),
    )
    .expect("read the manifest of the injected copy's dependency");
    let foo: serde_json::Value = serde_json::from_str(&foo_manifest).expect("parse foo manifest");
    assert_eq!(foo["version"], "1.0.0", "the catalog pins the dependency of the injected copy");
    assert_eq!(
        fs::read_to_string(lib.join("package.json")).expect("read lib manifest"),
        lib_manifest,
        "the manifest keeps its catalog reference",
    );

    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

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
