use crate::StoreDir;
use pnpm_fs::symlink_dir;
use std::fs;
use tempfile::tempdir;

#[test]
fn prune_removes_the_cached_metadata_and_keeps_the_rest_of_the_cache() {
    let root = tempdir().unwrap();
    let store_dir = StoreDir::new(root.path().join("store"));
    let cache_dir = root.path().join("cache");
    let removed = ["metadata", "metadata-v1.3", "v11/metadata", "v12/metadata-full"];
    let kept = ["dlx/key", "vendor", "v", "v12a", "python-metadata-v1"];
    for dir in removed.iter().chain(&kept) {
        fs::create_dir_all(cache_dir.join(dir)).unwrap();
    }
    fs::write(cache_dir.join("lockfile-verified.jsonl"), "").unwrap();

    store_dir.prune(&cache_dir).expect("prune");

    for dir in ["metadata", "metadata-v1.3", "v11", "v12"] {
        assert!(!cache_dir.join(dir).exists(), "{dir} must be removed");
    }
    for dir in kept {
        assert!(cache_dir.join(dir).exists(), "{dir} must be kept");
    }
    assert!(cache_dir.join("lockfile-verified.jsonl").exists());
}

#[test]
fn prune_keeps_a_cache_layout_dir_that_holds_the_store() {
    let root = tempdir().unwrap();
    let store_dir = StoreDir::new(root.path());
    fs::create_dir_all(store_dir.root().join("files")).unwrap();
    fs::create_dir_all(root.path().join("v12/metadata")).unwrap();

    store_dir.prune(root.path()).expect("prune");

    assert!(store_dir.root().join("files").exists(), "the store must survive");
    assert!(!root.path().join("v12").exists());
}

#[test]
fn prune_keeps_a_symlinked_metadata_dir_and_its_target() {
    let root = tempdir().unwrap();
    let store_dir = StoreDir::new(root.path().join("store"));
    let cache_dir = root.path().join("cache");
    let target = root.path().join("elsewhere");
    fs::create_dir_all(target.join("registry")).unwrap();
    fs::create_dir_all(&cache_dir).unwrap();
    symlink_dir(&target, &cache_dir.join("metadata")).unwrap();

    store_dir.prune(&cache_dir).expect("prune");

    assert!(cache_dir.join("metadata").exists());
    assert!(target.join("registry").exists());
}

#[test]
fn prune_succeeds_without_a_cache_dir() {
    let root = tempdir().unwrap();
    let store_dir = StoreDir::new(root.path().join("store"));
    store_dir
        .prune(&root.path().join("missing-cache"))
        .expect("prune");
}
