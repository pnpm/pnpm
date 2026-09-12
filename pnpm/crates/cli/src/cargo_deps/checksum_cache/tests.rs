use super::{CHECKSUM_FILE, ChecksumCache, checksum_cache_key};
use pnpm_store_dir::{SharedVerifiedFilesCache, StoreDir, StoreIndex, StoreIndexWriter};
use std::{collections::HashMap, fs, path::PathBuf};

async fn add_cached_checksum(
    store: &StoreDir,
    files: &mut HashMap<String, PathBuf>,
    checksum: &str,
) {
    let (writer, task) = StoreIndexWriter::spawn(store);
    let index = StoreIndex::shared_readonly_in(store);
    ChecksumCache {
        store_dir: store,
        index: index.as_ref(),
        writer: &writer,
        verified_files: &SharedVerifiedFilesCache::default(),
    }
    .add(files, checksum)
    .unwrap();
    drop(writer);
    StoreIndexWriter::drain(task, "").await;
}

#[tokio::test]
async fn repairs_a_corrupted_cached_checksum() {
    let temp = tempfile::tempdir().unwrap();
    let store = StoreDir::from(temp.path().join("store"));
    let (source, _) = store.write_cas_file(b"pub fn demo() {}", false).unwrap();
    let mut files = HashMap::from([("src/lib.rs".to_string(), source)]);
    let checksum = "a".repeat(64);
    add_cached_checksum(&store, &mut files, &checksum).await;
    let path = files[CHECKSUM_FILE].clone();
    let expected = fs::read(&path).unwrap();
    fs::write(&path, b"corrupted checksum").unwrap();

    add_cached_checksum(&store, &mut files, &checksum).await;

    assert_eq!(files[CHECKSUM_FILE], path);
    assert_eq!(fs::read(&path).unwrap(), expected);
}

#[tokio::test]
async fn invalidates_checksums_when_verified_files_change() {
    let temp = tempfile::tempdir().unwrap();
    let store = StoreDir::from(temp.path().join("store"));
    let (source, _) = store.write_cas_file(b"first", false).unwrap();
    let mut files = HashMap::from([("src/lib.rs".to_string(), source)]);
    let checksum = "a".repeat(64);
    add_cached_checksum(&store, &mut files, &checksum).await;
    let first = files[CHECKSUM_FILE].clone();
    let (source, _) = store.write_cas_file(b"second", false).unwrap();
    files.insert("src/lib.rs".to_string(), source);

    add_cached_checksum(&store, &mut files, &checksum).await;

    assert_ne!(files[CHECKSUM_FILE], first);
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&files[CHECKSUM_FILE]).unwrap()).unwrap();
    assert_eq!(manifest["files"]["src/lib.rs"], pnpm_crypto_hash::create_hex_hash("second"));
}

#[tokio::test]
async fn invalidates_checksums_when_the_archive_changes() {
    let temp = tempfile::tempdir().unwrap();
    let store = StoreDir::from(temp.path().join("store"));
    let (source, _) = store.write_cas_file(b"unchanged", false).unwrap();
    let mut files = HashMap::from([("src/lib.rs".to_string(), source)]);
    add_cached_checksum(&store, &mut files, &"a".repeat(64)).await;
    let first = files[CHECKSUM_FILE].clone();

    add_cached_checksum(&store, &mut files, &"b".repeat(64)).await;

    assert_ne!(files[CHECKSUM_FILE], first);
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&files[CHECKSUM_FILE]).unwrap()).unwrap();
    assert_eq!(manifest["package"], "b".repeat(64));
}

#[tokio::test]
async fn reuses_a_persisted_checksum() {
    let temp = tempfile::tempdir().unwrap();
    let store = StoreDir::from(temp.path().join("store"));
    let (source, _) = store.write_cas_file(b"pub fn demo() {}", false).unwrap();
    let mut files = HashMap::from([("src/lib.rs".to_string(), source)]);
    let checksum = "a".repeat(64);
    let key = checksum_cache_key(&files, &checksum);
    add_cached_checksum(&store, &mut files, &checksum).await;

    let (writer, task) = StoreIndexWriter::spawn(&store);
    let index = StoreIndex::shared_readonly_in(&store);
    let cached = ChecksumCache {
        store_dir: &store,
        index: index.as_ref(),
        writer: &writer,
        verified_files: &SharedVerifiedFilesCache::default(),
    }
    .read(&key);

    assert_eq!(cached, Some(files[CHECKSUM_FILE].clone()));
    drop(writer);
    StoreIndexWriter::drain(task, "").await;
}

#[tokio::test]
async fn invalidates_checksums_when_files_are_renamed() {
    let temp = tempfile::tempdir().unwrap();
    let store = StoreDir::from(temp.path().join("store"));
    let (source, _) = store.write_cas_file(b"unchanged", false).unwrap();
    let mut files = HashMap::from([("src/lib.rs".to_string(), source)]);
    let checksum = "a".repeat(64);
    add_cached_checksum(&store, &mut files, &checksum).await;
    let first = files[CHECKSUM_FILE].clone();
    let source = files.remove("src/lib.rs").unwrap();
    files.insert("src/main.rs".to_string(), source);

    add_cached_checksum(&store, &mut files, &checksum).await;

    assert_ne!(files[CHECKSUM_FILE], first);
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&files[CHECKSUM_FILE]).unwrap()).unwrap();
    assert_eq!(
        manifest["files"],
        serde_json::json!({"src/main.rs": pnpm_crypto_hash::create_hex_hash("unchanged")}),
    );
}
