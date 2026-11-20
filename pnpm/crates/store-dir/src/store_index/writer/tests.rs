use super::{
    WriteMsg,
    apply_write_msg,
    pending::invalidate_side_effects,
};
use crate::store_index::{
    SideEffectsDiff,
    StoreIndex,
    StoreIndexError,
    tests::sample_index,
};
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use tempfile::tempdir;

#[tokio::test]
async fn checked_writer_propagates_a_failed_index_batch() {
    let dir = tempdir().unwrap();
    let store_dir = crate::StoreDir::new(dir.path());
    let (writer, task) = super::StoreIndexWriter::spawn_checked(
        &store_dir,
        crate::store_index::StoreIndexFormat::Sqlite,
    );
    let index = StoreIndex::open(store_dir.root()).unwrap();
    index
        .sqlite_conn()
        .execute_batch(
            "CREATE TRIGGER fail_write BEFORE INSERT ON package_index
             BEGIN SELECT RAISE(FAIL, 'injected write failure'); END",
        )
        .unwrap();
    writer.queue("key".to_string(), sample_index());
    drop(writer);
    let error = task.await.unwrap().unwrap_err();
    assert!(matches!(error, StoreIndexError::Write { .. }), "{error:?}");
}

#[tokio::test]
async fn checked_writer_propagates_an_index_encoding_failure() {
    let dir = tempdir().unwrap();
    let store_dir = crate::StoreDir::new(dir.path());
    let (writer, task) = super::StoreIndexWriter::spawn_checked(
        &store_dir,
        crate::store_index::StoreIndexFormat::Sqlite,
    );
    let mut index = sample_index();
    let mut manifest = serde_json::Map::new();
    for shape in 0..64 {
        manifest.insert(
            format!("field-{shape}"),
            serde_json::Value::Object(serde_json::Map::from_iter([(
                format!("shape-{shape}"),
                serde_json::Value::from(shape),
            )])),
        );
    }
    index.manifest = Some(serde_json::Value::Object(manifest));
    writer.queue("key".to_string(), index);
    drop(writer);

    let error = task.await.unwrap().unwrap_err();
    assert!(matches!(error, StoreIndexError::Encode { .. }), "{error:?}");
}

#[test]
fn side_effects_invalidation_does_not_queue_unchanged_rows() {
    let dir = tempdir().unwrap();
    let index = StoreIndex::open(dir.path()).unwrap();
    let key = "built-package";
    for side_effects in [
        None,
        Some(HashMap::new()),
        Some(HashMap::from([(
            "other-engine".to_string(),
            SideEffectsDiff { added: None, deleted: None, remote_origin: None },
        )])),
    ] {
        let mut row = sample_index();
        row.side_effects = side_effects;
        index.set(key, &row).unwrap();
        let mut pending = HashMap::new();
        let writes = index.sqlite_conn().total_changes();
        invalidate_side_effects(&index, &mut pending, key, "test-engine").unwrap();
        dbg!(&row, &pending);
        assert!(pending.is_empty());
        assert_eq!(index.sqlite_conn().total_changes(), writes);

        row.requires_build = Some(true);
        let mut replacement = index.get(key).unwrap().unwrap();
        replacement.requires_build = Some(true);
        apply_write_msg(
            &index,
            &mut pending,
            WriteMsg::Replace { key: key.to_string(), value: replacement },
        );
        invalidate_side_effects(&index, &mut pending, key, "test-engine").unwrap();
        dbg!(&pending);
        assert!(pending.is_empty());
        assert_eq!(index.get(key).unwrap().unwrap(), row);
    }
}

#[test]
fn side_effects_invalidation_preserves_pending_uploads_and_ignores_algorithm_mismatch() {
    let dir = tempdir().unwrap();
    let index = StoreIndex::open(dir.path()).unwrap();
    let key = "built-package";
    let mut pending = HashMap::new();
    apply_write_msg(
        &index,
        &mut pending,
        WriteMsg::Replace { key: key.to_string(), value: sample_index() },
    );
    for cache_key in ["test-engine", "other-engine"] {
        apply_write_msg(
            &index,
            &mut pending,
            WriteMsg::SideEffectsUpload {
                key: key.to_string(),
                cache_key: cache_key.to_string(),
                current_files: sample_index().files,
                response: None,
            },
        );
    }
    invalidate_side_effects(&index, &mut pending, key, "test-engine").unwrap();
    assert!(pending.is_empty());
    let mut row = index.get(key).unwrap().unwrap();
    let side_effects = row.side_effects.as_ref().unwrap();
    assert_eq!(side_effects.len(), 1);
    assert!(side_effects.contains_key("other-engine"));
    assert_eq!(row.files, sample_index().files);

    row.algo = "sha256".to_string();
    index.set(key, &row).unwrap();
    invalidate_side_effects(&index, &mut pending, key, "other-engine").unwrap();
    row.side_effects = None;
    assert!(pending.is_empty());
    assert_eq!(index.get(key).unwrap().unwrap(), row);

    invalidate_side_effects(&index, &mut pending, "missing-package", "test-engine").unwrap();
    assert!(pending.is_empty());
}
