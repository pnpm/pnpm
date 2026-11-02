//! Concurrent writers, restarts, and lost commits.

use super::{config, npm, packument, read, storage, write};
use crate::{DocumentWrite, Storage, metadata::SqliteMetadata};
use pnpr_error::RegistryError;
use serde_json::json;
use tempfile::TempDir;

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
    let (left, right) = tokio::join!(
        first.write_hosted_document_if_current(&name, &ours, None),
        second.write_hosted_document_if_current(&name, &theirs, None),
    );
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
        second.write_hosted_document_if_current(&npm("pkg"), loser, Some(&current.version)).await;
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
    let metadata = restarted.sqlite_metadata().unwrap();
    let before = metadata.catalog_generation("").unwrap();
    assert_eq!(write(&restarted, "pkg", &document).await, DocumentWrite::Written);
    assert_eq!(metadata.catalog_generation("").unwrap(), before);
}

#[tokio::test]
async fn a_transaction_cut_off_before_commit_leaves_nothing() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("metadata.sqlite");
    drop(SqliteMetadata::open(&path).unwrap());
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
            .is_none()
    );
    assert_eq!(
        storage
            .sqlite_metadata()
            .unwrap()
            .catalog_generation("")
            .unwrap(),
        0
    );
}

#[test]
fn a_database_of_another_schema_is_refused() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("metadata.sqlite");
    drop(SqliteMetadata::open(&path).unwrap());
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute("UPDATE schema_version SET version = 999", [])
        .unwrap();
    let err = SqliteMetadata::open(&path).unwrap_err();
    assert!(matches!(err, RegistryError::StorageUnavailable { .. }), "{err:?}");
}
