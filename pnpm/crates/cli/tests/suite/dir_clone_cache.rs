//! The macOS directory-clone materialization cache
//! (`pnpm-deps-restorer/src/dir_clone_cache.rs`): a local-virtual-store
//! install materializes each package's canonical slot under
//! `<store_dir>/dir-clone-cache` and projects it into `node_modules/.pnpm` with
//! one directory `clonefile(2)`. Regression coverage for the macOS
//! hot-install slowdown of
//! <https://github.com/pnpm/pnpm/issues/14231>.
#![cfg(target_os = "macos")]

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_store_dir::StoreDir;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// The disposable canonical slots used by local-store installs.
fn cache_root(store_dir: &Path) -> PathBuf {
    StoreDir::new(store_dir).dir_clone_cache()
}

/// The package's materialized directory inside its sole canonical slot.
fn canonical_pkg_dir(store_dir: &Path, name: &str, version: &str) -> PathBuf {
    let version_dir = cache_root(store_dir).join(name).join(version);
    let hashes: Vec<PathBuf> = fs::read_dir(&version_dir)
        .unwrap_or_else(|err| panic!("read hash dirs under {version_dir:?}: {err}"))
        .map(|entry| entry.expect("read hash dir entry").path())
        .collect();
    assert_eq!(hashes.len(), 1, "expected one hash dir under {version_dir:?}, got {hashes:?}");
    hashes[0].join("node_modules").join(name)
}

fn pacquet(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn write_manifest(workspace: &Path) {
    let manifest = serde_json::json!({
        "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
}

fn project_pkg_manifest(workspace: &Path) -> PathBuf {
    workspace.join(
        "node_modules/.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0/node_modules/@pnpm.e2e/pkg-with-1-dep/package.json",
    )
}

/// The planted bytes are what prove the source: only a clone of the
/// canonical slot — not a re-import from the CAS — can carry them into
/// the project copy.
#[test]
fn warm_reinstall_is_served_from_the_canonical_slot() {
    let CommandTempCwd {
        root: _root, workspace, npmrc_info, ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;
    write_manifest(&workspace);

    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(
        project_pkg_manifest(&workspace).exists(),
        "the package must land on the flat project-local layout",
    );
    let canonical_manifest =
        canonical_pkg_dir(&store_dir, "@pnpm.e2e/pkg-with-1-dep", "100.0.0").join("package.json");
    assert!(canonical_manifest.exists(), "the install must populate the canonical slot");

    // Unlink-then-write so the plant never reaches a store file the
    // slot might share an inode with.
    fs::remove_file(&canonical_manifest).expect("unlink the canonical manifest");
    fs::write(&canonical_manifest, r#"{"planted":true}"#).expect("plant the canonical manifest");

    fs::remove_dir_all(workspace.join("node_modules")).expect("wipe node_modules");
    pacquet(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(project_pkg_manifest(&workspace)).expect("read the project manifest"),
        r#"{"planted":true}"#,
        "the warm reinstall must clone the canonical slot",
    );

    drop(mock_instance);
}

/// A hoisted install that re-resolves an existing lockfile, as
/// `autoDedupe` does on every install, must clone from the canonical
/// slot just like the frozen hoisted install does.
#[test]
fn hoisted_re_resolve_is_served_from_the_canonical_slot() {
    let CommandTempCwd {
        root: _root, workspace, npmrc_info, ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;
    write_manifest(&workspace);
    let mut yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .expect("read pnpm-workspace.yaml");
    yaml.push_str("nodeLinker: hoisted\n");
    fs::write(workspace.join("pnpm-workspace.yaml"), yaml).expect("write pnpm-workspace.yaml");
    let hoisted_manifest = workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep/package.json");

    pacquet(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    pacquet(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    let canonical_manifest =
        canonical_pkg_dir(&store_dir, "@pnpm.e2e/pkg-with-1-dep", "100.0.0").join("package.json");
    assert!(canonical_manifest.exists(), "the frozen install must populate the canonical slot");

    // Unlink-then-write so the plant never reaches a store file the
    // slot might share an inode with.
    fs::remove_file(&canonical_manifest).expect("unlink the canonical manifest");
    fs::write(&canonical_manifest, r#"{"planted":true}"#).expect("plant the canonical manifest");

    fs::remove_dir_all(workspace.join("node_modules")).expect("wipe node_modules");
    pacquet(&workspace)
        .with_args(["install", "--no-prefer-frozen-lockfile"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(&hoisted_manifest).expect("read the hoisted manifest"),
        r#"{"planted":true}"#,
        "the re-resolving hoisted install must clone the canonical slot",
    );

    drop(mock_instance);
}

/// An explicit `packageImportMethod` promises a specific on-disk form
/// a clone of the canonical copy could not deliver.
#[test]
fn explicit_copy_method_bypasses_the_cache() {
    let CommandTempCwd {
        root: _root, workspace, npmrc_info, ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;
    write_manifest(&workspace);
    let mut yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .expect("read pnpm-workspace.yaml");
    yaml.push_str("packageImportMethod: copy\n");
    fs::write(workspace.join("pnpm-workspace.yaml"), yaml).expect("write pnpm-workspace.yaml");

    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(
        project_pkg_manifest(&workspace).exists(),
        "the package must land on the flat project-local layout",
    );
    assert!(
        !cache_root(&store_dir).exists(),
        "an explicit copy install must not populate canonical slots",
    );

    drop(mock_instance);
}

#[test]
fn prune_removes_cached_clones_without_global_virtual_store() {
    let CommandTempCwd {
        root: _root, workspace, npmrc_info, ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;
    write_manifest(&workspace);
    pacquet(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let cached_package = canonical_pkg_dir(&store_dir, "@pnpm.e2e/pkg-with-1-dep", "100.0.0");
    assert!(cached_package.join("package.json").exists());
    assert!(!StoreDir::new(&store_dir).links().exists());
    let installed_manifest = project_pkg_manifest(&workspace);
    let manifest = fs::read(&installed_manifest).unwrap();

    pacquet(&workspace)
        .with_args(["store", "prune"])
        .assert()
        .success();

    assert!(!cached_package.exists(), "prune must remove the unreferenced clone cache");
    assert_eq!(fs::read(&installed_manifest).unwrap(), manifest);
    drop(mock_instance);
    fs::remove_dir_all(workspace.join("node_modules")).unwrap();
    pacquet(&workspace)
        .with_args(["install", "--offline", "--frozen-lockfile"])
        .assert()
        .success();
    assert_eq!(fs::read(installed_manifest).unwrap(), manifest);
}
