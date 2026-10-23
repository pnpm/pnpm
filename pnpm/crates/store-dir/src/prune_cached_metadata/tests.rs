use crate::StoreDir;
use pnpm_fs::symlink_dir;
use std::{fs, path::Path};
use tempfile::tempdir;

fn create_dirs(root: &Path, dirs: &[&str]) {
    for dir in dirs {
        fs::create_dir_all(root.join(dir)).unwrap();
    }
}

fn assert_removed(root: &Path, dirs: &[&str]) {
    for dir in dirs {
        assert!(!root.join(dir).exists(), "{dir} must be removed");
    }
}

fn assert_kept(root: &Path, dirs: &[&str]) {
    for dir in dirs {
        assert!(root.join(dir).exists(), "{dir} must be kept");
    }
}

#[test]
fn prune_removes_only_the_metadata_dirs_of_the_cache() {
    let root = tempdir().unwrap();
    let store_dir = StoreDir::new(root.path().join("store"));
    let cache_dir = root.path().join("cache");
    let removed = [
        "metadata",
        "metadata-v1.3",
        "v11/metadata",
        "v11/metadata-private",
        "v12/metadata",
        "v12/metadata-full",
        "v12/metadata-full-filtered",
        "v12/metadata-private",
    ];
    let kept = [
        "dlx/key",
        "vendor",
        "v",
        "v12a/metadata",
        "python-metadata-v1",
        "v11/cargo-index/crates-io",
        "v11/runtime-shasums",
        "v11/tarball-resolutions",
    ];
    create_dirs(&cache_dir, &removed);
    create_dirs(&cache_dir, &kept);
    fs::write(cache_dir.join("lockfile-verified.jsonl"), "").unwrap();

    store_dir
        .prune(&cache_dir, &[])
        .expect("prune");

    assert_removed(&cache_dir, &removed);
    assert_kept(&cache_dir, &kept);
    assert!(cache_dir.join("lockfile-verified.jsonl").exists());
}

#[test]
fn prune_keeps_metadata_dirs_that_hold_the_store() {
    let root = tempdir().unwrap();
    let cache_dir = root.path();
    let store_dir = StoreDir::new(cache_dir.join("v12/metadata-store"));
    create_dirs(store_dir.root(), &["files"]);
    create_dirs(cache_dir, &["v12/metadata"]);

    store_dir
        .prune(cache_dir, &[])
        .expect("prune");

    assert!(store_dir.root().join("files").exists(), "the store must survive");
    assert_removed(cache_dir, &["v12/metadata"]);
}

/// `cacheDir` set to the same directory as `globalDir`.
#[test]
fn prune_keeps_the_global_packages_in_the_cache_dir() {
    let root = tempdir().unwrap();
    let store_dir = StoreDir::new(root.path().join("store"));
    let global_dir = root.path().join("global");
    let global_pkg_dir = global_dir.join("v11");
    let protected_metadata = global_pkg_dir.join("metadata-group");
    create_dirs(&global_pkg_dir, &["group/node_modules/pkg", "metadata-group/node_modules"]);
    create_dirs(&global_dir, &["v12/metadata"]);

    store_dir
        .prune(&global_dir, &[&global_dir, &global_pkg_dir, &protected_metadata])
        .expect("prune");

    assert_kept(&global_pkg_dir, &["group/node_modules/pkg", "metadata-group/node_modules"]);
    assert_removed(&global_dir, &["v12/metadata"]);
}

/// `cacheDir` set to a home directory that holds pnpm's other directories
/// and the stores of other pnpm versions.
#[test]
fn prune_keeps_pnpm_dirs_under_a_home_cache_dir() {
    let home = tempdir().unwrap();
    let home = home.path();
    let store_dir = StoreDir::new(home.join("store"));
    let pnpm_home = home.join("metadata-pnpm");
    let state_dir = home.join("v3/metadata-state");
    create_dirs(home, &["metadata-pnpm/bin", "v3/metadata-state/x", "v10/files/00", "v3/metadata"]);

    store_dir
        .prune(home, &[&pnpm_home, &state_dir])
        .expect("prune");

    assert_kept(home, &["metadata-pnpm/bin", "v3/metadata-state/x", "v10/files/00"]);
    assert_removed(home, &["v3/metadata"]);
}

#[test]
fn prune_keeps_a_symlinked_metadata_dir_and_its_target() {
    let root = tempdir().unwrap();
    let store_dir = StoreDir::new(root.path().join("store"));
    let cache_dir = root.path().join("cache");
    let target = root.path().join("elsewhere");
    create_dirs(&target, &["registry"]);
    create_dirs(&cache_dir, &["v12"]);
    symlink_dir(&target, &cache_dir.join("metadata")).unwrap();
    symlink_dir(&target, &cache_dir.join("v12/metadata")).unwrap();

    store_dir
        .prune(&cache_dir, &[])
        .expect("prune");

    assert_kept(&cache_dir, &["metadata", "v12/metadata"]);
    assert!(target.join("registry").exists());
}

#[test]
fn prune_succeeds_without_a_cache_dir() {
    let root = tempdir().unwrap();
    let store_dir = StoreDir::new(root.path().join("store"));
    store_dir
        .prune(&root.path().join("missing-cache"), &[])
        .expect("prune");
}

#[test]
fn prune_keeps_a_symlinked_layout_dir_and_its_target() {
    let root = tempdir().unwrap();
    let store_dir = StoreDir::new(root.path().join("store"));
    let cache_dir = root.path().join("cache");
    let target = root.path().join("elsewhere");
    create_dirs(&target, &["metadata/registry"]);
    create_dirs(&cache_dir, &["v11/metadata"]);
    symlink_dir(&target, &cache_dir.join("v12")).unwrap();

    store_dir
        .prune(&cache_dir, &[])
        .expect("prune");

    assert_kept(&cache_dir, &["v12/metadata/registry"]);
    assert_removed(&cache_dir, &["v11/metadata"]);
}
