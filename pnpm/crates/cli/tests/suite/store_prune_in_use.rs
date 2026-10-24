//! `pnpm store prune` must keep the store files of every package a
//! registered project installed, including the packages whose files were
//! copied into `node_modules` and so hold no hard link to the store.

use crate::patch::MARKER_PATCH;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
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

    let store_dir = StoreDir::new(store_dir);
    let keys_before = sorted_index_keys(&store_dir);
    dbg!(&keys_before);
    assert!(keys_before.len() > 1, "the install must index every package it fetched");

    prune(&workspace);

    assert_eq!(sorted_index_keys(&store_dir), keys_before);
}
