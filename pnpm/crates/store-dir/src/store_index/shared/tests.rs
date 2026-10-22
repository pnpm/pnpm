use super::{Detached, StoreIndex};
use crate::{
    StoreDir, StoreIndexFormat,
    store_index::{store_index_key, tests::sample_index},
};
use tempfile::tempdir;

fn store_with_one_row(format: StoreIndexFormat) -> (tempfile::TempDir, StoreDir, String) {
    let dir = tempdir().unwrap();
    let store_dir = StoreDir::new(dir.path());
    StoreIndex::apply_format(store_dir.root(), format).unwrap();
    let key = store_index_key("sha512-a", "lodash@4.17.21");
    StoreIndex::open(store_dir.root())
        .unwrap()
        .set(&key, &sample_index())
        .unwrap();
    (dir, store_dir, key)
}

/// A per-file lookup is handed its index with the shared lock already
/// released: the entry read, the round trip on a network filesystem,
/// neither queues behind another lookup nor holds one up.
#[test]
fn a_per_file_lookup_reads_its_entry_with_the_lock_released() {
    let (_dir, store_dir, key) = store_with_one_row(StoreIndexFormat::Files);
    let shared = StoreIndex::shared_readonly_in(&store_dir).unwrap();

    let Detached::Files(files) = StoreIndex::lock_or_detach(&shared, |_| ()) else {
        panic!("a per-file index is handed out for reading outside the lock");
    };
    let held_elsewhere = shared.try_lock().expect("the lock is released before the entry is read");
    assert!(
        files.get(&key).unwrap().is_some(),
        "the entry is read while another lookup holds the lock",
    );
    drop(held_elsewhere);
}

/// `SQLite`'s connection is queried under the lock, as before.
#[test]
fn an_sqlite_lookup_runs_under_the_lock() {
    let (_dir, store_dir, key) = store_with_one_row(StoreIndexFormat::Sqlite);
    let shared = StoreIndex::shared_readonly_in(&store_dir).unwrap();

    let found = StoreIndex::lock_or_detach(&shared, |index| {
        assert!(shared.try_lock().is_err(), "the query runs with the lock held");
        index.contains_key(&key).unwrap()
    });
    assert!(matches!(found, Detached::Locked(Some(true))));
}
