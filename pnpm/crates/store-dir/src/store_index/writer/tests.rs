use super::{WriteMsg, apply_write_msg, invalidate_side_effects};
use crate::store_index::{
    MAX_WRITE_RETRIES, SideEffectsDiff, StoreIndex, StoreIndexError, retry_sqlite_write,
    tests::sample_index,
};
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use tempfile::tempdir;

fn busy_error() -> StoreIndexError {
    sqlite_write_error(rusqlite::ffi::SQLITE_BUSY)
}

fn locked_error() -> StoreIndexError {
    sqlite_write_error(rusqlite::ffi::SQLITE_LOCKED)
}

fn sqlite_write_error(code: i32) -> StoreIndexError {
    StoreIndexError::Write {
        source: rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None),
    }
}

#[test]
fn sqlite_write_retries_contention_and_succeeds() {
    let mut attempts = 0;
    let result = retry_sqlite_write(|| {
        attempts += 1;
        if attempts < 3 { Err(busy_error()) } else { Ok(()) }
    });
    result.unwrap();
    assert_eq!(attempts, 3);
}

#[test]
fn sqlite_write_retries_locked_errors() {
    let mut attempts = 0;
    retry_sqlite_write(|| {
        attempts += 1;
        if attempts == 1 { Err(locked_error()) } else { Ok(()) }
    })
    .unwrap();
    assert_eq!(attempts, 2);
}

#[test]
fn sqlite_write_retries_after_a_real_database_lock_is_released() {
    let dir = tempdir().unwrap();
    let mut index = StoreIndex::open(dir.path()).unwrap();
    index
        .sqlite_conn()
        .busy_timeout(std::time::Duration::ZERO)
        .unwrap();
    let lock = rusqlite::Connection::open(dir.path().join("index.db")).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let unlock = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(60));
        lock.execute_batch("ROLLBACK").unwrap();
    });
    index
        .set_many([("key".to_string(), sample_index())])
        .unwrap();
    unlock.join().unwrap();
    assert_eq!(index.get("key").unwrap(), Some(sample_index()));
}

#[test]
fn sqlite_write_returns_error_after_retry_budget() {
    let mut attempts = 0;
    let result = retry_sqlite_write(|| {
        attempts += 1;
        Err::<(), _>(busy_error())
    });
    assert!(matches!(result, Err(StoreIndexError::Write { .. })));
    assert_eq!(attempts, MAX_WRITE_RETRIES + 1);
}

#[test]
fn sqlite_write_does_not_retry_non_contention_errors() {
    let mut attempts = 0;
    let result = retry_sqlite_write(|| {
        attempts += 1;
        Err::<(), _>(StoreIndexError::WriterUnavailable)
    });
    assert!(matches!(result, Err(StoreIndexError::WriterUnavailable)));
    assert_eq!(attempts, 1);
}

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
    let result = task.await.unwrap();
    assert!(matches!(result, Err(StoreIndexError::Write { .. })));
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
