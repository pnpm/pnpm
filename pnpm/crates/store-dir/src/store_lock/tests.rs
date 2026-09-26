use super::{StoreDir, global_operation_lock_path, operation_lock_path};
use std::fs::{File, TryLockError};
use tempfile::tempdir;

fn temp_store(root: &tempfile::TempDir) -> StoreDir {
    StoreDir::new(root.path().join("store/v11"))
}

fn open_lock_file(store: &StoreDir) -> File {
    let path = operation_lock_path(store).unwrap();
    pnpm_fs::open_secure_lock_file(&path).unwrap()
}

fn assert_global_prune_barrier_is_blocked() {
    let file = pnpm_fs::open_secure_lock_file(&global_operation_lock_path().unwrap()).unwrap();
    assert!(matches!(file.try_lock(), Err(TryLockError::WouldBlock)));
}

#[test]
fn prune_waits_for_store_consumers() {
    let root = tempdir().unwrap();
    let store = temp_store(&root);
    let consumer = store.lock_for_use().unwrap();

    assert_global_prune_barrier_is_blocked();

    drop(consumer);
    open_lock_file(&store).try_lock().unwrap();
}

#[test]
fn store_consumers_may_overlap() {
    let root = tempdir().unwrap();
    let store = temp_store(&root);
    let first = store.lock_for_use().unwrap();
    let second = store.lock_for_use().unwrap();
    drop((first, second));
}

#[test]
fn prune_waits_for_frozen_store_consumers() {
    let root = tempdir().unwrap();
    let store = temp_store(&root);
    let consumer = store.lock_for_frozen_use().unwrap();

    assert_global_prune_barrier_is_blocked();

    drop(consumer);
    open_lock_file(&store).try_lock().unwrap();
}

#[test]
fn equivalent_store_paths_share_an_operation_lock() {
    let root = tempdir().unwrap();
    let physical = root.path().join("store");
    std::fs::create_dir_all(&physical).unwrap();
    let direct = StoreDir::new(&physical);
    let lexical_alias = StoreDir::new(root.path().join("nested/../store"));

    assert_eq!(operation_lock_path(&direct).unwrap(), operation_lock_path(&lexical_alias).unwrap());
}

#[cfg(unix)]
#[test]
fn symlinked_store_paths_share_an_operation_lock() {
    let root = tempdir().unwrap();
    let physical = root.path().join("store");
    std::fs::create_dir_all(&physical).unwrap();
    std::os::unix::fs::symlink(&physical, root.path().join("store-link")).unwrap();

    assert_eq!(
        operation_lock_path(&StoreDir::new(&physical)).unwrap(),
        operation_lock_path(&StoreDir::new(root.path().join("store-link"))).unwrap(),
    );
}

#[cfg(unix)]
#[test]
fn prune_stays_blocked_when_a_store_symlink_changes_target() {
    let root = tempdir().unwrap();
    let first = root.path().join("first");
    let second = root.path().join("second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    let alias = root.path().join("store-link");
    std::os::unix::fs::symlink(&first, &alias).unwrap();
    let aliased_store = StoreDir::new(&alias);
    let first_lock_path = operation_lock_path(&aliased_store).unwrap();
    let consumer = aliased_store.lock_for_use().unwrap();

    std::fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(&second, &alias).unwrap();
    assert_ne!(first_lock_path, operation_lock_path(&StoreDir::new(&second)).unwrap());
    assert_global_prune_barrier_is_blocked();

    drop(consumer);
}

#[cfg(windows)]
#[test]
fn differently_cased_missing_store_paths_cannot_bypass_the_prune_barrier() {
    let root = tempdir().unwrap();
    let lower = StoreDir::new(root.path().join("missing-store"));
    let upper = StoreDir::new(root.path().join("MISSING-STORE"));
    let consumer = lower.lock_for_use().unwrap();

    assert_ne!(operation_lock_path(&lower).unwrap(), operation_lock_path(&upper).unwrap());
    assert_global_prune_barrier_is_blocked();

    drop(consumer);
}

#[test]
fn operation_locks_stay_outside_the_store() {
    let root = tempdir().unwrap();
    let store = temp_store(&root);
    let path = operation_lock_path(&store).unwrap();
    assert!(!path.starts_with(store.root()));
}

#[cfg(unix)]
#[test]
fn non_unicode_store_paths_keep_distinct_lock_identities() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt as _};

    let root = tempdir().unwrap();
    let first = StoreDir::new(
        root.path()
            .join(OsStr::from_bytes(b"store-\xfe")),
    );
    let second = StoreDir::new(
        root.path()
            .join(OsStr::from_bytes(b"store-\xff")),
    );

    assert_ne!(operation_lock_path(&first).unwrap(), operation_lock_path(&second).unwrap());
}
