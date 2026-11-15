use super::{Ecosystem, LOG_NAMESPACE, LogId, Storage};
use pnpm_transparency_log::NoteSigner;
use pnpr_config::{DatabaseBackend, HostedStoreConfig};
use tempfile::TempDir;

const LOG_SIGNING_KEY: &str =
    "PRIVATE+KEY+PeterNeumann+c74f20a3+AYEKFALVFGyNhPJEMzD1QIDr+Y7hfZx09iUvxdXHKDFz";

fn test_storage(dir: &TempDir) -> Storage {
    let hosted = dir.path().join("hosted");
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&hosted).unwrap();
    std::fs::create_dir_all(&cache).unwrap();
    let db = DatabaseBackend::Sqlite(dir.path().join("metadata.sqlite"));
    Storage::new(&HostedStoreConfig::Fs, hosted, cache, &db).unwrap()
}

fn corrupt_signature(note: &[u8]) -> Vec<u8> {
    let mut damaged = note.to_vec();
    let sep = damaged
        .windows(2)
        .position(|pair| pair == b"\n\n")
        .unwrap();
    let sig_byte = damaged[sep + 2..]
        .iter_mut()
        .rfind(|byte| byte.is_ascii_alphanumeric())
        .unwrap();
    *sig_byte = if *sig_byte == b'A' { b'B' } else { b'A' };
    damaged
}

#[tokio::test]
async fn log_record_copy_protects_existing_checkpoints_and_preserves_keyless_copy() {
    let src_dir = TempDir::new().unwrap();
    let tgt_dir = TempDir::new().unwrap();
    let src = test_storage(&src_dir);
    let tgt = test_storage(&tgt_dir);
    let log = LogId { namespace: "acme".to_string(), ecosystem: Ecosystem::Npm };
    let key = NoteSigner::from_private_key(LOG_SIGNING_KEY).unwrap();

    src.sql_database()
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

    // Keyless initial copy into an empty target and keyless identical retry both succeed.
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
        tgt.store_log_record_copy(record_key, &bytes, None).await.unwrap();
        tgt.store_log_record_copy(record_key, &bytes, None).await.unwrap();
    }
    let first_checkpoint = tgt
        .read_log_record("npm/~acme/checkpoint")
        .await
        .unwrap()
        .unwrap();

    // A checkpoint with the same head and a damaged signature cannot replace the valid target
    // checkpoint either without a key or with the signing key configured.
    let damaged_same_head = corrupt_signature(&first_checkpoint);
    let keyless_damaged =
        tgt.store_log_record_copy("npm/~acme/checkpoint", &damaged_same_head, None).await;
    assert!(keyless_damaged.is_err());
    let signed_damaged =
        tgt.store_log_record_copy("npm/~acme/checkpoint", &damaged_same_head, Some(&key)).await;
    assert!(signed_damaged.is_err());
    let preserved = tgt
        .read_log_record("npm/~acme/checkpoint")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(preserved, first_checkpoint);

    // Grow source to 2 entries: keyless replacement is refused, while signed advancement succeeds.
    src.sql_database()
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
        if record_key == "npm/~acme/checkpoint" {
            let keyless_advance = tgt.store_log_record_copy(record_key, &bytes, None).await;
            assert!(keyless_advance.is_err());
        }
        tgt.store_log_record_copy(record_key, &bytes, Some(&key)).await
            .unwrap();
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
    let downgraded =
        tgt.store_log_record_copy("npm/~acme/checkpoint", &first_checkpoint, Some(&key)).await;
    assert!(downgraded.is_err());

    // Invalid checkpoint keys and checkpoints whose origin names another log are refused.
    let bad_key = tgt.store_log_record_copy("npm/bad/checkpoint", &first_checkpoint, None).await;
    assert!(bad_key.is_err());
    let wrong_log = tgt.store_log_record_copy("npm/checkpoint", &first_checkpoint, None).await;
    assert!(wrong_log.is_err());

    // Overwriting an immutable tile with differing bytes is refused.
    let tile_key = second_keys
        .iter()
        .find(|k| k.contains("/tile/"))
        .unwrap();
    let overwritten = tgt.store_log_record_copy(tile_key, b"corrupted", Some(&key)).await;
    assert!(overwritten.is_err());
    let has_records = src.root.has_any_record(LOG_NAMESPACE).await.unwrap();
    assert!(has_records);
}
