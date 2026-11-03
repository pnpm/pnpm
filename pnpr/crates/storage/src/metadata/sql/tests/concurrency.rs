//! Concurrent writers, restarts, and lost commits.

use super::{config, npm, packument, read, storage, write};
use crate::{DocumentWrite, Storage, metadata::SqlMetadata};
use pnpr_error::RegistryError;
use serde_json::json;
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};
use tempfile::TempDir;

#[tokio::test]
async fn a_read_completes_while_a_write_transaction_is_open() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    write(&storage, "pkg", &packument("pkg", &["1.0.0"])).await;
    // Hold the write connection inside an uncommitted transaction, the way a
    // slow commit holds it.
    let metadata = Arc::clone(storage.sql_metadata().unwrap());
    let (held, wait_held) = mpsc::channel();
    let (release, wait_release) = mpsc::channel::<()>();
    let writer = std::thread::spawn(move || {
        metadata.adapter
            .write(&mut |connection| {
                connection.begin_write()?;
                connection.execute_batch("DELETE FROM packages")?;
                held.send(()).unwrap();
                wait_release.recv().unwrap();
                connection.execute_batch("ROLLBACK")
            })
            .unwrap();
    });
    wait_held.recv().unwrap();

    let read = tokio::time::timeout(Duration::from_secs(10), read(&storage, "pkg")).await;
    release.send(()).unwrap();
    writer.join().unwrap();
    let document = read.expect("a read does not wait for the write connection");
    assert_eq!(document["versions"]["1.0.0"]["version"], "1.0.0", "the committed snapshot");
}

#[tokio::test]
async fn concurrent_publishers_of_one_version_cannot_both_commit() {
    let tmp = TempDir::new().unwrap();
    // Two stores over one database: two writers that share no in-process lock.
    let (first, second) = (storage(&tmp), storage(&tmp));
    let mut theirs = packument("pkg", &["1.0.0"]);
    theirs["versions"]["1.0.0"]["description"] = json!("theirs");
    let ours = serde_json::to_vec(&packument("pkg", &["1.0.0"])).unwrap();
    let theirs = serde_json::to_vec(&theirs).unwrap();
    let name = npm("pkg");
    let left = first.write_hosted_document_if_current(&name, &ours, None, None);
    let right = second.write_hosted_document_if_current(&name, &theirs, None, None);
    let (left, right) = tokio::join!(left, right);
    let loser = match (left.unwrap(), right.unwrap()) {
        (DocumentWrite::Written, DocumentWrite::Conflict) => &theirs,
        (DocumentWrite::Conflict, DocumentWrite::Written) => &ours,
        outcomes => panic!("exactly one writer commits, got {outcomes:?}"),
    };
    // The loser's retry reads the winner and cannot replace its version.
    let current = second
        .read_hosted_document_for_update(&npm("pkg"))
        .await
        .unwrap()
        .unwrap();
    let retry =
        second.write_hosted_document_if_current(&npm("pkg"), loser, Some(&current.version), None)
            .await;
    assert!(matches!(retry, Err(RegistryError::VersionAlreadyPublished { .. })), "{retry:?}");
}

#[tokio::test]
async fn a_cache_that_missed_an_invalidation_still_serves_the_current_view() {
    let tmp = TempDir::new().unwrap();
    let (reader, writer) = (storage(&tmp), storage(&tmp));
    write(&writer, "pkg", &packument("pkg", &["1.0.0"])).await;
    read(&reader, "pkg").await;
    write(&writer, "pkg", &packument("pkg", &["1.0.0", "2.0.0"])).await;
    let document = read(&reader, "pkg").await;
    assert_eq!(document["dist-tags"]["latest"], "2.0.0");
    assert!(document["versions"].get("2.0.0").is_some());
}

#[tokio::test]
async fn a_commit_survives_a_restart_and_its_retry_changes_nothing() {
    let tmp = TempDir::new().unwrap();
    let document = packument("pkg", &["1.0.0"]);
    {
        let storage = storage(&tmp);
        write(&storage, "pkg", &document).await;
    }
    // The publisher never saw the response and retries against a restarted
    // server: the commit is there, and the identical retry is a no-op.
    let restarted = storage(&tmp);
    assert_eq!(read(&restarted, "pkg").await, document);
    let metadata = restarted.sql_metadata().unwrap();
    let before = metadata.catalog_generation("").unwrap();
    assert_eq!(write(&restarted, "pkg", &document).await, DocumentWrite::Written);
    assert_eq!(metadata.catalog_generation("").unwrap(), before);
}

#[tokio::test]
async fn a_transaction_cut_off_before_commit_leaves_nothing() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("metadata.sqlite");
    drop(SqlMetadata::open_sqlite(&path).unwrap());
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("BEGIN IMMEDIATE").unwrap();
        connection
            .execute(
                "INSERT INTO packages (catalog, ecosystem, name, generation, state)
                 VALUES ('', 'npm', 'pkg', 1, X'7B7D')",
                [],
            )
            .unwrap();
        // Dropped without COMMIT, as a crash would leave it.
    }
    let storage: Storage = Storage::from_config(&config(&tmp)).unwrap();
    assert!(
        storage
            .read_hosted_document(&npm("pkg"))
            .await
            .unwrap()
            .is_none(),
    );
    assert_eq!(
        storage
            .sql_metadata()
            .unwrap()
            .catalog_generation("")
            .unwrap(),
        0,
    );
}

#[test]
fn a_database_of_another_schema_is_refused() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("metadata.sqlite");
    drop(SqlMetadata::open_sqlite(&path).unwrap());
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute("UPDATE schema_version SET version = 999", [])
        .unwrap();
    let err = SqlMetadata::open_sqlite(&path).unwrap_err();
    assert!(matches!(err, RegistryError::StorageUnavailable { .. }), "{err:?}");
}

#[test]
fn a_busy_database_is_a_conflict_that_a_rerun_may_clear() {
    use super::super::{adapter::Adapter, sqlite::SqliteAdapter};
    let tmp = TempDir::new().unwrap();
    let adapter = SqliteAdapter::open(&tmp.path().join("metadata.sqlite")).unwrap();
    let failure = |code| {
        RegistryError::Sqlite(rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None))
    };
    assert!(adapter.is_retryable(&failure(rusqlite::ffi::SQLITE_BUSY)));
    assert!(adapter.is_retryable(&failure(rusqlite::ffi::SQLITE_LOCKED)));
    assert!(!adapter.is_retryable(&failure(rusqlite::ffi::SQLITE_CONSTRAINT)));
}

#[test]
fn a_failed_commit_is_rolled_back() {
    use super::super::{adapter::Adapter, batch::WriteTransaction, sqlite::SqliteAdapter};
    let tmp = TempDir::new().unwrap();
    let adapter = SqliteAdapter::open(&tmp.path().join("metadata.sqlite")).unwrap();
    adapter
        .write(&mut |connection| {
            // A deferred foreign key fails the `COMMIT` itself, which leaves
            // the transaction open.
            connection.execute_batch(
                "CREATE TEMP TABLE parent (id INTEGER PRIMARY KEY);
                 CREATE TEMP TABLE child (
                     parent INTEGER REFERENCES parent (id) DEFERRABLE INITIALLY DEFERRED
                 );",
            )?;
            let transaction = WriteTransaction::begin(connection)?;
            transaction.execute_batch("INSERT INTO child VALUES (1)")?;
            assert!(transaction.commit().is_err());
            connection.begin_write()?;
            connection.execute_batch("ROLLBACK")
        })
        .unwrap();
}

#[test]
fn a_write_left_in_its_transaction_does_not_block_the_next() {
    use super::super::{adapter::Adapter, sqlite::SqliteAdapter};
    let tmp = TempDir::new().unwrap();
    let adapter = SqliteAdapter::open(&tmp.path().join("metadata.sqlite")).unwrap();
    let err = adapter
        .write(&mut |connection| {
            connection.begin_write()?;
            Err(RegistryError::Internal { reason: "cut off".to_string() })
        })
        .unwrap_err();
    assert!(matches!(err, RegistryError::Internal { .. }), "{err:?}");
    adapter
        .write(&mut |connection| {
            connection.begin_write()?;
            connection.execute_batch("COMMIT")
        })
        .unwrap();
}

#[test]
fn a_read_left_in_its_transaction_is_not_reused() {
    use super::super::adapter::ReadSource;
    let tmp = TempDir::new().unwrap();
    let metadata = SqlMetadata::open_sqlite(&tmp.path().join("metadata.sqlite")).unwrap();
    metadata.adapter
        .read(ReadSource::Primary, &mut |connection| connection.begin_read())
        .unwrap();
    metadata.adapter
        .read(ReadSource::Primary, &mut |connection| {
            connection.begin_read()?;
            connection.execute_batch("COMMIT")
        })
        .unwrap();
}
