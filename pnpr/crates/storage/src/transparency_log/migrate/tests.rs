use super::{Ecosystem, LOG_NAMESPACE, LogId, Storage};
use pnpm_transparency_log::NoteSigner;
use pnpr_config::{HostedStoreConfig, MetadataBackend};
use tempfile::TempDir;

const LOG_SIGNING_KEY: &str =
    "PRIVATE+KEY+PeterNeumann+c74f20a3+AYEKFALVFGyNhPJEMzD1QIDr+Y7hfZx09iUvxdXHKDFz";

fn test_storage(dir: &TempDir) -> Storage {
    let hosted = dir.path().join("hosted");
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&hosted).unwrap();
    std::fs::create_dir_all(&cache).unwrap();
    let db = MetadataBackend::Sqlite(dir.path().join("metadata.sqlite"));
    Storage::new(&HostedStoreConfig::Fs, hosted, cache, &db).unwrap()
}

#[tokio::test]
async fn log_record_copy_orders_tiles_first_advances_prefixes_and_rejects_conflicts() {
    let src_dir = TempDir::new().unwrap();
    let tgt_dir = TempDir::new().unwrap();
    let src = test_storage(&src_dir);
    let tgt = test_storage(&tgt_dir);
    let log = LogId { namespace: "acme".to_string(), ecosystem: Ecosystem::Npm };
    let key = NoteSigner::from_private_key(LOG_SIGNING_KEY).unwrap();

    src.sql_metadata()
        .import_log_entries(
            &log,
            0,
            vec![crate::transparency_log::ImportedLogEntry {
                bytes: b"entry-0".to_vec(),
                npm_version: None,
            }],
        )
        .await
        .unwrap();
    src.integrate_log(&log, &key).await.unwrap();

    let first_keys = src.stored_log_record_keys().await.unwrap();
    assert!(
        first_keys
            .last()
            .is_some_and(|k| k == "npm/~acme/checkpoint"),
        "{first_keys:?}",
    );
    for record_key in &first_keys {
        let bytes = src
            .read_log_record(record_key)
            .await
            .unwrap()
            .unwrap();
        tgt.store_log_record_copy(record_key, &bytes).await.unwrap();
    }
    let first_checkpoint = tgt
        .read_log_record("npm/~acme/checkpoint")
        .await
        .unwrap()
        .unwrap();

    // Grow source to 2 entries and copy again: target's size-1 checkpoint advances cleanly.
    src.sql_metadata()
        .import_log_entries(
            &log,
            1,
            vec![crate::transparency_log::ImportedLogEntry {
                bytes: b"entry-1".to_vec(),
                npm_version: None,
            }],
        )
        .await
        .unwrap();
    src.integrate_log(&log, &key).await.unwrap();

    let second_keys = src.stored_log_record_keys().await.unwrap();
    assert!(
        second_keys
            .last()
            .is_some_and(|k| k == "npm/~acme/checkpoint"),
        "{second_keys:?}",
    );
    for record_key in &second_keys {
        let bytes = src
            .read_log_record(record_key)
            .await
            .unwrap()
            .unwrap();
        tgt.store_log_record_copy(record_key, &bytes).await.unwrap();
    }
    let second_size = tgt
        .log_checkpoint(&log)
        .await
        .unwrap()
        .unwrap()
        .tree_head
        .size;
    assert_eq!(second_size, 2);

    // Downgrading target's size-2 checkpoint back to size-1 is refused.
    let downgraded = tgt.store_log_record_copy("npm/~acme/checkpoint", &first_checkpoint).await;
    assert!(downgraded.is_err());

    // Overwriting an immutable tile with differing bytes is refused.
    let tile_key = second_keys
        .iter()
        .find(|k| k.contains("/tile/"))
        .unwrap();
    let overwritten = tgt.store_log_record_copy(tile_key, b"corrupted").await;
    assert!(overwritten.is_err());
    let has_records = src.root.has_any_record(LOG_NAMESPACE).await.unwrap();
    assert!(has_records);
}
