//! `pnpm store prune` must keep the store files of every package a
//! registered project installed, including the packages that pnpm copied
//! into `node_modules`, which hold no hard link to the store.

use crate::{
    _utils::enable_gvs_in_workspace_yaml,
    install_runtimes::{
        prepare_workspace, runtime_fixture, write_runtime_lockfile, write_runtime_manifest,
    },
    patch::MARKER_PATCH,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_graph_hasher::{host_arch, host_platform};
use pnpm_store_dir::{PackageFilesIndex, StoreDir, StoreIndex};
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    command_env::CommandTestExt,
};
use pretty_assertions::assert_eq;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
}

fn write_manifest(workspace: &Path) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
                "is-positive": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
}

fn append_workspace_yaml(workspace: &Path, text: &str) {
    let path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str(text);
    fs::write(&path, yaml).expect("write pnpm-workspace.yaml");
}

fn sorted_index_keys(store_dir: &StoreDir) -> Vec<String> {
    let mut keys = StoreIndex::open_readonly_in(store_dir)
        .expect("open the store index")
        .keys()
        .expect("read the store index keys");
    keys.sort();
    keys
}

fn is_positive_row(store_dir: &StoreDir) -> PackageFilesIndex {
    let index = StoreIndex::open_readonly_in(store_dir).expect("open the store index");
    let key = index
        .keys()
        .expect("read the store index keys")
        .into_iter()
        .find(|key| key.ends_with("\tis-positive@1.0.0"))
        .expect("a store index row for is-positive@1.0.0");
    index
        .get(&key)
        .expect("read the store index row")
        .expect("the row is present")
}

/// Every store file the row names: its base files and the files each
/// side-effects entry adds.
fn row_blob_paths(store_dir: &StoreDir, row: &PackageFilesIndex) -> Vec<PathBuf> {
    let side_effects_files = row.side_effects
        .iter()
        .flat_map(|side_effects| side_effects.values())
        .filter_map(|diff| diff.added.as_ref())
        .flat_map(|added| added.values());
    row.files
        .values()
        .chain(side_effects_files)
        .map(|info| {
            store_dir
                .cas_file_path_by_mode(&info.digest, info.mode)
                .expect("an index row holds hex digests")
        })
        .collect()
}

/// Every store file the rows under `keys` name.
fn blob_paths_of_rows(store_dir: &StoreDir, keys: &[String]) -> Vec<PathBuf> {
    let index = StoreIndex::open_readonly_in(store_dir).expect("open the store index");
    keys.iter()
        .flat_map(|key| {
            let row = index
                .get(key)
                .expect("read the store index row")
                .expect("the row is present");
            row_blob_paths(store_dir, &row)
        })
        .collect()
}

fn prune(workspace: &Path) {
    let output = pacquet_at(workspace)
        .with_args(["store", "prune"])
        .output()
        .expect("run store prune");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(output.status.success(), "store prune failed: {stderr}");
}

/// A patched package gets its own copy of the files even under `hardlink`,
/// so no store file of it has a hard link outside the store. `hardlink`
/// keeps every other package linked on each platform, which `auto` does
/// not do on a copy-on-write filesystem.
#[test]
fn store_prune_keeps_the_files_of_a_patched_dependency() {
    let CommandTempCwd {
        root: _root, workspace, npmrc_info, ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry {
        mock_instance: _mock_instance,
        store_dir,
        ..
    } = npmrc_info;
    write_manifest(&workspace);
    fs::create_dir_all(workspace.join("patches")).expect("create the patches dir");
    fs::write(workspace.join("patches/is-positive@1.0.0.patch"), MARKER_PATCH)
        .expect("write the patch file");
    append_workspace_yaml(
        &workspace,
        concat!(
            "packageImportMethod: hardlink\n",
            "patchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n",
        ),
    );
    pacquet_at(&workspace)
        .with_args(["install", "--reporter=silent"])
        .assert()
        .success();

    let store_dir = StoreDir::new(store_dir);
    let row = is_positive_row(&store_dir);
    let patched_entries = row.side_effects
        .as_ref()
        .map_or(0, |side_effects| {
            side_effects
                .keys()
                .filter(|key| key.contains(";patch="))
                .count()
        });
    assert_eq!(patched_entries, 1, "the patch must be cached as a side-effects entry");
    let blob_paths = row_blob_paths(&store_dir, &row);
    let keys_before = sorted_index_keys(&store_dir);

    prune(&workspace);

    assert_eq!(sorted_index_keys(&store_dir), keys_before);
    let missing: Vec<_> = blob_paths
        .iter()
        .filter(|path| !path.exists())
        .collect();
    assert_eq!(missing, Vec::<&PathBuf>::new(), "store prune removed files the project uses");
}

/// With `packageImportMethod: copy` no store file has a hard link outside
/// the store, so the hard-link count alone marks every package unused.
#[test]
fn store_prune_keeps_every_package_installed_by_copy() {
    assert_prune_keeps_every_package_installed_by_copy(|_| {}, "node_modules/.pnpm/lock.yaml");
}

#[test]
fn store_prune_keeps_every_package_installed_by_copy_in_a_custom_virtual_store_dir() {
    assert_prune_keeps_every_package_installed_by_copy(
        |workspace| append_workspace_yaml(workspace, "virtualStoreDir: vs\n"),
        "vs/lock.yaml",
    );
}

#[test]
fn store_prune_keeps_every_package_installed_by_copy_in_a_custom_virtual_store_dir_without_a_lockfile()
 {
    assert_prune_keeps_every_package_installed_by_copy(
        |workspace| append_workspace_yaml(workspace, "virtualStoreDir: vs\nlockfile: false\n"),
        "vs/lock.yaml",
    );
}

/// The project records the shared store as its `virtualStoreDir`, and the
/// install keeps the lockfile in `node_modules/.pnpm`.
#[test]
fn store_prune_keeps_every_package_installed_by_copy_in_the_global_virtual_store() {
    assert_prune_keeps_every_package_installed_by_copy(
        |workspace| enable_gvs_in_workspace_yaml(workspace, ""),
        "node_modules/.pnpm/lock.yaml",
    );
}

fn assert_prune_keeps_every_package_installed_by_copy(
    configure: impl FnOnce(&Path),
    installed_lockfile: &str,
) {
    let CommandTempCwd {
        root: _root, workspace, npmrc_info, ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry {
        mock_instance: _mock_instance,
        store_dir,
        ..
    } = npmrc_info;
    write_manifest(&workspace);
    append_workspace_yaml(&workspace, "packageImportMethod: copy\n");
    configure(&workspace);
    pacquet_at(&workspace)
        .with_args(["install", "--reporter=silent"])
        .assert()
        .success();
    assert!(
        workspace.join(installed_lockfile).is_file(),
        "the install must write {installed_lockfile}",
    );

    let store_dir = StoreDir::new(store_dir);
    let keys_before = sorted_index_keys(&store_dir);
    dbg!(&keys_before);
    assert!(keys_before.len() > 1, "the install must index every package it fetched");
    let blob_paths = blob_paths_of_rows(&store_dir, &keys_before);

    prune(&workspace);

    assert_eq!(sorted_index_keys(&store_dir), keys_before);
    let missing: Vec<_> = blob_paths
        .iter()
        .filter(|path| !path.exists())
        .collect();
    assert_eq!(missing, Vec::<&PathBuf>::new(), "store prune removed files the project uses");
}

/// The install keys a runtime row by its synthesized manifest as well as
/// its integrity, and the resolution lists one archive per platform.
#[test]
fn store_prune_keeps_a_runtime_installed_by_copy() {
    let root = tempfile::tempdir().expect("create a temp dir");
    let workspace = prepare_workspace(&root, "packageImportMethod: copy\n");
    let mut server = mockito::Server::new();
    let fixtures = [runtime_fixture(&mut server, "node", "22.0.0", host_platform(), host_arch())];
    write_runtime_manifest(&workspace, &fixtures);
    write_runtime_lockfile(&workspace, &fixtures);
    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile", "--reporter=silent"])
        .assert()
        .success();

    let store_dir = StoreDir::new(root.path().join("store"));
    let keys_before = sorted_index_keys(&store_dir);
    dbg!(&keys_before);
    let runtime_keys: Vec<String> = keys_before
        .iter()
        .filter(|key| key.ends_with("\tnode@runtime:22.0.0"))
        .cloned()
        .collect();
    let blob_paths = blob_paths_of_rows(&store_dir, &runtime_keys);
    assert!(!blob_paths.is_empty(), "the install must index the runtime");

    prune(&workspace);

    assert_eq!(sorted_index_keys(&store_dir), keys_before);
    let missing: Vec<_> = blob_paths
        .iter()
        .filter(|path| !path.exists())
        .collect();
    assert_eq!(missing, Vec::<&PathBuf>::new(), "store prune removed the runtime's files");
}

/// A lockfile written by a pnpm that uses another lockfile format, such as
/// pnpm 8.
const LOCKFILE_V6: &str = "lockfileVersion: '6.0'

dependencies:
  is-positive:
    specifier: 1.0.0
    version: 1.0.0

packages:

  /is-positive@1.0.0:
    resolution: {integrity: sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==}
    dev: false
";

#[test]
fn store_prune_skips_an_installed_lockfile_of_an_unsupported_version_with_a_warning() {
    let CommandTempCwd {
        root: _root, workspace, npmrc_info, ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance: _mock_instance, .. } = npmrc_info;
    write_manifest(&workspace);
    append_workspace_yaml(&workspace, "packageImportMethod: copy\n");
    pacquet_at(&workspace)
        .with_args(["install", "--reporter=silent"])
        .assert()
        .success();
    fs::write(workspace.join("node_modules/.pnpm/lock.yaml"), LOCKFILE_V6)
        .expect("write the old lockfile");

    let output = pacquet_at(&workspace)
        .with_args(["store", "prune"])
        .output()
        .expect("run store prune");

    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(output.status.success(), "store prune failed: {stderr}");
    let expected = format!(
        "Skipped {} because it was installed with lockfileVersion 6.0, \
         which this version of pnpm does not support",
        dunce::canonicalize(&workspace).expect("canonicalize the workspace").display(),
    );
    let warnings: Vec<_> = stderr
        .lines()
        .filter(|line| line.contains("lockfileVersion"))
        .collect();
    assert_eq!(warnings, [expected.as_str()]);
}

#[test]
fn store_prune_fails_on_a_broken_installed_lockfile_and_removes_nothing() {
    assert_prune_fails_on_a_broken_file_and_removes_nothing(
        "node_modules/.pnpm/lock.yaml",
        "packages: [",
    );
}

#[test]
fn store_prune_fails_on_a_broken_modules_manifest_and_removes_nothing() {
    assert_prune_fails_on_a_broken_file_and_removes_nothing("node_modules/.modules.yaml", "{");
}

fn assert_prune_fails_on_a_broken_file_and_removes_nothing(file: &str, content: &str) {
    let CommandTempCwd {
        root: _root, workspace, npmrc_info, ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry {
        mock_instance: _mock_instance,
        store_dir,
        ..
    } = npmrc_info;
    write_manifest(&workspace);
    append_workspace_yaml(&workspace, "packageImportMethod: copy\n");
    pacquet_at(&workspace)
        .with_args(["install", "--reporter=silent"])
        .assert()
        .success();
    fs::write(workspace.join(file), content).expect("break the file");
    let store_dir = StoreDir::new(store_dir);
    let keys_before = sorted_index_keys(&store_dir);

    let output = pacquet_at(&workspace)
        .with_args(["store", "prune"])
        .output()
        .expect("run store prune");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "store prune must fail: {stderr}");
    assert!(stderr.contains("ERR_PNPM_STORE_DIR_PRUNE_IN_USE_FILES"), "stderr={stderr}");
    assert_eq!(sorted_index_keys(&store_dir), keys_before);
}
