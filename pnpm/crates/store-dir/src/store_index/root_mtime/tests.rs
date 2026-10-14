use crate::store_index::{StoreIndex, store_index_key, tests::sample_index};
use pretty_assertions::assert_eq;
use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime},
};
use tempfile::tempdir;

fn store_root_with_old_mtime() -> (tempfile::TempDir, SystemTime) {
    let dir = tempdir().unwrap();
    StoreIndex::open(dir.path())
        .unwrap()
        .set(&store_index_key("sha512-a", "lodash@4.17.21"), &sample_index())
        .unwrap();
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
    super::set_dir_mtime(dir.path(), old).unwrap();
    (dir, old)
}

fn mtime(path: &Path) -> SystemTime {
    fs::metadata(path)
        .unwrap()
        .modified()
        .unwrap()
}

#[test]
fn reading_and_writing_leave_the_store_root_mtime_unchanged() {
    let (dir, old) = store_root_with_old_mtime();

    let reader = StoreIndex::open_readonly(dir.path()).unwrap();
    let writer = StoreIndex::open(dir.path()).unwrap();
    reader
        .get(&store_index_key("sha512-a", "lodash@4.17.21"))
        .unwrap()
        .unwrap();
    writer
        .set(&store_index_key("sha512-b", "is-positive@3.1.0"), &sample_index())
        .unwrap();
    drop(reader);
    assert!(dir.path().join("index.db-wal").exists());
    drop(writer);

    assert!(!dir.path().join("index.db-wal").exists());
    assert_eq!(mtime(dir.path()), old);
}

#[test]
fn a_new_store_root_entry_keeps_the_new_mtime() {
    let (dir, old) = store_root_with_old_mtime();

    let index = StoreIndex::open_readonly(dir.path()).unwrap();
    fs::create_dir(dir.path().join("projects")).unwrap();
    drop(index);

    assert_ne!(mtime(dir.path()), old);
}

#[test]
fn connections_through_a_store_alias_share_one_snapshot() {
    let (dir, old) = store_root_with_old_mtime();
    let aliases = tempdir().unwrap();
    let alias = aliases.path().join("store");
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path(), &alias).unwrap();
    #[cfg(windows)]
    junction::create(dir.path(), &alias).unwrap();

    let direct = StoreIndex::open_readonly(dir.path()).unwrap();
    let key = store_index_key("sha512-a", "lodash@4.17.21");
    direct.get(&key).unwrap().unwrap();
    let aliased = StoreIndex::open(&alias).unwrap();
    aliased
        .set(&store_index_key("sha512-b", "is-positive@3.1.0"), &sample_index())
        .unwrap();
    drop(direct);
    drop(aliased);

    assert_eq!(mtime(dir.path()), old);
}
