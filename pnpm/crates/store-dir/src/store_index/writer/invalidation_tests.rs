use super::{StoreIndexWriter, WriteMsg, apply_write_msg, flush_batch};
use crate::{
    StoreDir, StoreIndex, UploadError,
    store_index::{SideEffectsDiff, StoreIndexError, tests::sample_index},
    upload, upload_with_diff,
};
use std::{collections::HashMap, path::Path, sync::atomic::AtomicBool, time::Duration};
use tempfile::{TempDir, tempdir};

const KEY: &str = "symlink-output";
const CACHE_KEY: &str = "test-engine";

#[test]
fn symlink_upload_reports_sqlite_read_failure() {
    check_read_failure(false);
}

#[test]
fn symlink_upload_with_diff_reports_sqlite_read_failure() {
    check_read_failure(true);
}

fn check_read_failure(with_diff: bool) {
    let (_root, package, store_dir, index) = fixture();
    index.conn.execute_batch("PRAGMA journal_mode=DELETE").unwrap();
    index.conn.busy_timeout(Duration::ZERO).unwrap();
    let lock = rusqlite::Connection::open(store_dir.root().join("index.db")).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let error = index.get(KEY).unwrap_err();
    assert!(matches!(error, StoreIndexError::Read { source }
        if source.sqlite_error_code() == Some(rusqlite::ErrorCode::DatabaseBusy)));

    let (result, index) = run_upload(index, &store_dir, package.path(), with_diff);
    lock.execute_batch("ROLLBACK").unwrap();
    assert!(
        index
            .get(KEY)
            .unwrap()
            .unwrap()
            .side_effects
            .unwrap()
            .contains_key(CACHE_KEY),
    );
    assert!(matches!(result, Err(UploadError::StoreIndex(StoreIndexError::Read { .. }))));
}

#[test]
fn symlink_upload_reports_sqlite_flush_failure() {
    check_flush_failure(false);
}

#[test]
fn symlink_upload_with_diff_reports_sqlite_flush_failure() {
    check_flush_failure(true);
}

fn check_flush_failure(with_diff: bool) {
    let (_root, package, store_dir, index) = fixture();
    index.conn
        .execute_batch(
            "CREATE TRIGGER fail_write BEFORE INSERT ON package_index
         BEGIN SELECT RAISE(FAIL, 'injected write failure'); END",
        )
        .unwrap();
    assert!(index.get(KEY).unwrap().is_some());
    let (result, index) = run_upload(index, &store_dir, package.path(), with_diff);
    index.conn.execute_batch("DROP TRIGGER fail_write").unwrap();
    assert!(
        index
            .get(KEY)
            .unwrap()
            .unwrap()
            .side_effects
            .unwrap()
            .contains_key(CACHE_KEY),
    );
    assert!(matches!(result, Err(UploadError::StoreIndex(StoreIndexError::Write { .. }))));
}

#[test]
fn symlink_upload_missing_row_is_a_noop() {
    for with_diff in [false, true] {
        let (_root, package, store_dir, index) = fixture();
        index.conn
            .execute("DELETE FROM package_index WHERE key = ?", [KEY])
            .unwrap();
        index.conn.execute_batch("PRAGMA query_only=ON").unwrap();
        let writes = index.conn.total_changes();
        let (result, index) = run_upload(index, &store_dir, package.path(), with_diff);
        result.unwrap();
        assert!(index.get(KEY).unwrap().is_none());
        assert_eq!(index.conn.total_changes(), writes);
    }
}

#[test]
fn symlink_upload_unchanged_row_needs_no_write() {
    for with_diff in [false, true] {
        let (_root, package, store_dir, index) = fixture();
        let mut row = index.get(KEY).unwrap().unwrap();
        let diff = row.side_effects
            .as_mut()
            .unwrap()
            .remove(CACHE_KEY)
            .unwrap();
        row.side_effects
            .as_mut()
            .unwrap()
            .insert("other-engine".to_string(), diff);
        index.set(KEY, &row).unwrap();
        index.conn.execute_batch("PRAGMA query_only=ON").unwrap();
        let writes = index.conn.total_changes();
        let (result, index) = run_upload(index, &store_dir, package.path(), with_diff);
        result.unwrap();
        assert_eq!(index.get(KEY).unwrap().unwrap(), row);
        assert_eq!(index.conn.total_changes(), writes);
    }
}

#[test]
fn pending_replacement_is_persisted_before_successful_invalidation() {
    let (_root, _package, _store_dir, index) = fixture();
    let replacement = sample_index();
    assert!(replacement.side_effects.is_none());
    let mut pending = HashMap::from([
        (KEY.to_string(), replacement),
        ("other-package".to_string(), sample_index()),
    ]);
    apply_invalidation(&index, &mut pending).unwrap();
    assert_eq!(index.get(KEY).unwrap().unwrap(), sample_index());
    assert!(!pending.contains_key(KEY));
    assert!(pending.contains_key("other-package"));
}

#[test]
fn failed_invalidation_keeps_pending_writes_for_the_batch() {
    let (_root, _package, _store_dir, mut index) = fixture();
    let mut replacement = index.get(KEY).unwrap().unwrap();
    replacement.requires_build = Some(true);
    let mut pending = HashMap::from([
        (KEY.to_string(), replacement),
        ("other-package".to_string(), sample_index()),
    ]);
    index.conn.execute_batch("PRAGMA query_only=ON").unwrap();
    assert!(matches!(apply_invalidation(&index, &mut pending), Err(StoreIndexError::Write { .. })));
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[KEY].requires_build, Some(true));
    assert!(pending[KEY].side_effects.is_none());
    index.conn.execute_batch("PRAGMA query_only=OFF").unwrap();
    index.set_many(pending.drain()).unwrap();
    assert_eq!(
        index
            .get(KEY)
            .unwrap()
            .unwrap()
            .requires_build,
        Some(true),
    );
    assert!(
        index
            .get(KEY)
            .unwrap()
            .unwrap()
            .side_effects
            .is_none(),
    );
    assert_eq!(
        index
            .get("other-package")
            .unwrap()
            .unwrap(),
        sample_index(),
    );
}

fn apply_invalidation(
    index: &StoreIndex,
    pending: &mut HashMap<String, crate::PackageFilesIndex>,
) -> Result<(), StoreIndexError> {
    let (response, result) = std::sync::mpsc::sync_channel(1);
    apply_write_msg(
        index,
        pending,
        WriteMsg::InvalidateSideEffects {
            key: KEY.to_string(),
            cache_key: CACHE_KEY.to_string(),
            response,
        },
    );
    result.recv().unwrap()
}

#[test]
fn symlink_upload_reports_closed_writer() {
    let (_root, package, store_dir, _index) = fixture();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    drop(rx);
    let writer =
        StoreIndexWriter { tx, disabled: false, warn_on_send_failure: AtomicBool::new(true) };
    assert_unavailable_writer(&store_dir, package.path(), &writer);
}

#[tokio::test]
async fn symlink_upload_reports_disabled_writer_without_waiting_for_its_task() {
    let (_root, package, store_dir, _index) = fixture();
    let (writer, task) = StoreIndexWriter::spawn_disabled();
    assert_unavailable_writer(&store_dir, package.path(), &writer);
    drop(writer);
    task.await.unwrap().unwrap();
}

#[test]
fn symlink_upload_reports_lost_invalidation_response() {
    let (_root, package, store_dir, _index) = fixture();
    for with_diff in [false, true] {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let writer =
            StoreIndexWriter { tx, disabled: false, warn_on_send_failure: AtomicBool::new(true) };
        let task = std::thread::spawn(move || drop(rx.blocking_recv().unwrap()));
        let result = if with_diff {
            upload_with_diff(&store_dir, package.path(), KEY, CACHE_KEY, &writer).map(|_| ())
        } else {
            upload(&store_dir, package.path(), KEY, CACHE_KEY, &writer)
        };
        task.join().unwrap();
        assert!(matches!(result, Err(UploadError::StoreIndex(StoreIndexError::WriterUnavailable))));
    }
}

fn assert_unavailable_writer(store_dir: &StoreDir, package: &Path, writer: &StoreIndexWriter) {
    assert!(matches!(
        upload(store_dir, package, KEY, CACHE_KEY, writer),
        Err(UploadError::StoreIndex(StoreIndexError::WriterUnavailable))
    ));
    assert!(matches!(
        upload_with_diff(store_dir, package, KEY, CACHE_KEY, writer),
        Err(UploadError::StoreIndex(StoreIndexError::WriterUnavailable))
    ));
}

fn run_upload(
    mut index: StoreIndex,
    store_dir: &StoreDir,
    package: &Path,
    with_diff: bool,
) -> (Result<(), UploadError>, StoreIndex) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let writer =
        StoreIndexWriter { tx, disabled: false, warn_on_send_failure: AtomicBool::new(true) };
    let task = std::thread::spawn(move || {
        while let Some(message) = rx.blocking_recv() {
            flush_batch(&mut index, &mut vec![message]);
        }
        index
    });
    let result = if with_diff {
        upload_with_diff(store_dir, package, KEY, CACHE_KEY, &writer).map(|_| ())
    } else {
        upload(store_dir, package, KEY, CACHE_KEY, &writer)
    };
    drop(writer);
    (result, task.join().unwrap())
}

fn fixture() -> (TempDir, TempDir, StoreDir, StoreIndex) {
    let root = tempdir().unwrap();
    let store_dir = StoreDir::from(root.path().to_path_buf());
    store_dir.init().unwrap();
    let package = tempdir().unwrap();
    let target = package.path().join("generated");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("index.js"), "module.exports = true").unwrap();
    let link = package.path().join("generated-link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    #[cfg(windows)]
    junction::create(&target, &link).unwrap();
    let index = StoreIndex::open(store_dir.root()).unwrap();
    let mut row = sample_index();
    row.side_effects = Some(HashMap::from([(
        CACHE_KEY.to_string(),
        SideEffectsDiff {
            added: None,
            deleted: Some(vec!["stale.js".to_string()]),
            remote_origin: None,
        },
    )]));
    index.set(KEY, &row).unwrap();
    (root, package, store_dir, index)
}
