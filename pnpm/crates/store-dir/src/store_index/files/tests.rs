use super::{FilesIndex, INDEX_DIR, TEMP_FILE_PREFIX, encode_entry};
use crate::{
    StoreDir, StoreIndex, StoreIndexFormat, StoreIndexWriter,
    store_index::{store_index_key, tests::sample_index},
};
use pretty_assertions::assert_eq;
use std::{collections::HashSet, fs, path::Path, sync::Arc};
use tempfile::tempdir;

fn files_store(root: &Path) -> StoreIndex {
    StoreIndex::apply_format(root, StoreIndexFormat::Files).unwrap();
    StoreIndex::open(root).unwrap()
}

fn files_index(root: &Path) -> FilesIndex {
    FilesIndex::detect(root).expect("the store has a per-file index")
}

#[test]
fn rows_round_trip_without_an_sqlite_database() {
    let dir = tempdir().unwrap();
    let mut index = files_store(dir.path());
    let lodash = store_index_key("sha512-a", "lodash@4.17.21");
    let react = store_index_key("sha512-b", "react@18.0.0");

    index
        .set(&lodash, &sample_index())
        .unwrap();
    index
        .set_many([(react.clone(), sample_index())])
        .unwrap();

    assert_eq!(index.get(&lodash).unwrap(), Some(sample_index()));
    assert!(index.contains_key(&react).unwrap());
    assert_eq!(
        index
            .contains_many(&[lodash.clone(), "sha512-c\tmissing@1.0.0".to_string()])
            .unwrap(),
        HashSet::from([lodash.clone()]),
    );
    assert_eq!(
        index
            .get_many(std::slice::from_ref(&react))
            .unwrap()
            .len(),
        1,
    );
    assert_eq!(index.get_by_pkg_id("react@18.0.0").unwrap(), Some(sample_index()));
    let mut keys = index.keys().unwrap();
    keys.sort();
    assert_eq!(keys, [lodash.clone(), react.clone()]);

    index
        .delete_many(std::slice::from_ref(&lodash))
        .unwrap();
    assert_eq!(index.get(&lodash).unwrap(), None);
    assert_eq!(index.keys().unwrap(), [react]);
    assert!(!dir.path().join("index.db").exists());
}

#[test]
fn a_store_with_a_per_file_index_is_read_as_one_by_every_open() {
    let dir = tempdir().unwrap();
    let store_dir = StoreDir::new(dir.path());
    let root = store_dir.root();
    let key = store_index_key("sha512-a", "lodash@4.17.21");
    files_store(root)
        .set(&key, &sample_index())
        .unwrap();

    // Opening without asking for the format still finds the per-file index.
    assert_eq!(
        StoreIndex::open(root)
            .unwrap()
            .get(&key)
            .unwrap(),
        Some(sample_index()),
    );
    assert_eq!(
        StoreIndex::open_readonly(root)
            .unwrap()
            .get(&key)
            .unwrap(),
        Some(sample_index()),
    );
    assert_eq!(
        StoreIndex::open_immutable(root)
            .unwrap()
            .get(&key)
            .unwrap(),
        Some(sample_index()),
    );
    let shared = StoreIndex::shared_readonly_in(&store_dir)
        .expect("a store with only a per-file index has an index to share");
    assert_eq!(
        shared
            .lock()
            .unwrap()
            .get(&key)
            .unwrap(),
        Some(sample_index()),
    );
    assert!(!root.join("index.db").exists());
}

#[tokio::test]
async fn the_writer_switches_a_new_store_to_the_requested_format() {
    let dir = tempdir().unwrap();
    let store_dir = StoreDir::new(dir.path());
    let key = store_index_key("sha512-a", "lodash@4.17.21");

    let (writer, task) = StoreIndexWriter::spawn_in_format(&store_dir, StoreIndexFormat::Files);
    writer.queue(key.clone(), sample_index());
    drop(writer);
    task.await.unwrap().unwrap();

    assert!(
        store_dir
            .root()
            .join(INDEX_DIR)
            .is_dir(),
    );
    assert!(
        !store_dir
            .root()
            .join("index.db")
            .exists(),
    );
    assert!(
        files_index(store_dir.root())
            .get(&key)
            .unwrap()
            .is_some(),
    );
}

/// The format belongs to the store, so a writer configured for `SQLite`
/// keeps writing to the per-file index other processes read.
#[tokio::test]
async fn a_writer_keeps_the_format_the_store_has() {
    let dir = tempdir().unwrap();
    let store_dir = StoreDir::new(dir.path());
    StoreIndex::apply_format(store_dir.root(), StoreIndexFormat::Files).unwrap();
    let key = store_index_key("sha512-a", "lodash@4.17.21");

    let (writer, task) = StoreIndexWriter::spawn_in_format(&store_dir, StoreIndexFormat::Sqlite);
    writer.queue(key.clone(), sample_index());
    drop(writer);
    task.await.unwrap().unwrap();

    assert!(
        !store_dir
            .root()
            .join("index.db")
            .exists(),
    );
    assert_eq!(
        StoreIndex::open(store_dir.root())
            .unwrap()
            .get(&key)
            .unwrap(),
        Some(sample_index()),
    );
}

#[test]
fn leftover_temporary_and_foreign_entries_read_as_missing() {
    let dir = tempdir().unwrap();
    let index = files_store(dir.path());
    let key = store_index_key("sha512-a", "lodash@4.17.21");
    let other = store_index_key("sha512-b", "react@18.0.0");
    let files = files_index(dir.path());

    // A row whose file holds another key's entry, as a hash collision would.
    let path = files.entry_path(&key);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, encode_entry(&other, b"value")).unwrap();
    // A crashed writer's temporary file, and a file too short to hold a
    // header.
    fs::write(path.with_file_name(format!("{TEMP_FILE_PREFIX}123")), encode_entry(&key, b"value"))
        .unwrap();
    let short = files.entry_path("short");
    fs::create_dir_all(short.parent().unwrap()).unwrap();
    fs::write(short, [1u8]).unwrap();

    assert_eq!(index.get(&key).unwrap(), None);
    assert!(!index.contains_key(&key).unwrap());
    assert!(
        index
            .contains_many(std::slice::from_ref(&key))
            .unwrap()
            .is_empty(),
    );
    assert_eq!(index.keys().unwrap(), [other]);
}

/// Separate processes on a shared store each hold their own index; their
/// writes to the same and to different rows all land intact.
#[test]
fn concurrent_writers_leave_every_row_readable() {
    let dir = tempdir().unwrap();
    StoreIndex::apply_format(dir.path(), StoreIndexFormat::Files).unwrap();
    let root = Arc::new(dir.path().to_path_buf());
    let shared_key = store_index_key("sha512-shared", "shared@1.0.0");

    let writers: Vec<_> = (0..8)
        .map(|writer| {
            let root = Arc::clone(&root);
            let shared_key = shared_key.clone();
            std::thread::spawn(move || {
                let mut index = StoreIndex::open(&root).unwrap();
                let rows = (0..50).map(|row| {
                    (
                        store_index_key(&format!("sha512-{writer}-{row}"), "pkg@1.0.0"),
                        sample_index(),
                    )
                });
                index
                    .set_many(rows.chain([(shared_key, sample_index())]))
                    .unwrap();
            })
        })
        .collect();
    for writer in writers {
        writer.join().unwrap();
    }

    let index = StoreIndex::open(&root).unwrap();
    let keys = index.keys().unwrap();
    assert_eq!(keys.len(), 8 * 50 + 1);
    assert_eq!(index.get_many(&keys).unwrap().len(), keys.len());
}

#[test]
fn shared_lookups_read_both_formats() {
    let key = store_index_key("sha512-a", "lodash@4.17.21");
    for format in [StoreIndexFormat::Sqlite, StoreIndexFormat::Files] {
        let dir = tempdir().unwrap();
        let store_dir = StoreDir::new(dir.path());
        StoreIndex::apply_format(store_dir.root(), format).unwrap();
        StoreIndex::open(store_dir.root())
            .unwrap()
            .set(&key, &sample_index())
            .unwrap();
        let shared = StoreIndex::shared_readonly_in(&store_dir).unwrap();

        assert_eq!(
            StoreIndex::get_shared(&shared, &key).unwrap(),
            Some(sample_index()),
            "{format:?}",
        );
        assert!(StoreIndex::contains_shared(&shared, &key).unwrap(), "{format:?}");
        assert!(
            !StoreIndex::contains_shared(&shared, "sha512-b\tmissing@1.0.0").unwrap(),
            "{format:?}",
        );
    }
}
