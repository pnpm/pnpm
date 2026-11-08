//! Timestamp storage and the protocol representation at its boundaries.

use super::{Db, npm, packument, read, storage, try_write, write};
use crate::{
    metadata::sql::adapter::{Connection, ReadSource},
    publish::{iso_from_unix_millis, now_millis},
};
use serde_json::json;

async fn publication_times_keep_milliseconds_and_normalize_offsets(db: Db) {
    let storage = storage(&db);
    let org = storage.for_hosted("", pnpr_package_name::Ecosystem::Npm);
    let mut document = packument("example", &["1.0.0"]);
    document["time"]["1.0.0"] = json!("2026-01-01T01:00:00.123+01:00");
    write(&org, "example", &document).await;
    document["time"]["1.0.0"] = json!("2026-01-01T00:00:00.123Z");
    assert_eq!(read(&org, "example").await, document);
    let publication = storage
        .sql_metadata()
        .initial_publication(&npm("example"), "", "1.0.0")
        .await
        .unwrap()
        .unwrap();
    assert!(chrono::DateTime::parse_from_rfc3339(&publication.committed_at).is_ok());
}

async fn metadata_timestamps_are_integer_milliseconds(db: Db) {
    let storage = storage(&db);
    let org = storage.for_hosted("", pnpr_package_name::Ecosystem::Npm);
    let before = now_millis();
    write(&org, "example", &packument("example", &["1.0.0"])).await;
    storage
        .sql_metadata()
        .set_version_note(
            &npm("example"),
            "",
            "1.0.0",
            crate::metadata::NoteChange {
                text: Some("a note".to_string()),
                principal: "publisher".to_string(),
            },
        )
        .await
        .unwrap();
    let adapter = db.adapter();
    tokio::task::spawn_blocking(move || {
        adapter.read(ReadSource::Primary, &mut |connection| assert_timestamps(connection, before))?;
        Ok::<_, pnpr_error::RegistryError>(())
    })
    .await
    .unwrap()
    .unwrap();
}

fn assert_timestamps(connection: &dyn Connection, before: i64) -> pnpr_error::Result<()> {
    for columns in [
        ("versions", "published_at"),
        ("publications", "committed_at"),
        ("tag_history", "changed_at"),
        ("notes", "modified_at"),
    ] {
        assert_timestamp_column(connection, columns, before)?;
    }
    Ok(())
}

fn assert_timestamp_column(
    connection: &dyn Connection,
    (table, column): (&str, &str),
    before: i64,
) -> pnpr_error::Result<()> {
    let rows = connection.query(&format!("SELECT {column} FROM {table}"), params![])?;
    assert!(!rows.is_empty(), "{table}");
    for mut row in rows {
        let millis: i64 = row.get(0)?;
        if table == "versions" {
            assert_eq!(iso_from_unix_millis(millis), "2026-01-01T00:00:00.000Z");
        } else {
            assert!((before..=now_millis()).contains(&millis), "{table}: {millis}");
        }
    }
    Ok(())
}

async fn every_timestamp_column_has_integer_affinity(db: Db) {
    let storage = storage(&db);
    let org = storage.for_hosted("", pnpr_package_name::Ecosystem::Npm);
    write(&org, "example", &packument("example", &["1.0.0"])).await;
    let adapter = db.adapter();
    let column_type_sql = db.sql(
        "SELECT type FROM pragma_table_info(?1) WHERE name = ?2",
        "SELECT data_type FROM information_schema.columns WHERE table_schema = current_schema() AND table_name = ?1 AND column_name = ?2",
        "SELECT CAST(data_type AS CHAR) FROM information_schema.columns WHERE table_schema = DATABASE() AND table_name = ?1 AND column_name = ?2",
    );
    let integer_type = db.sql("integer", "bigint", "bigint");
    tokio::task::spawn_blocking(move || {
        adapter.read(ReadSource::Primary, &mut |connection| {
            for (table, column) in [
                ("versions", "published_at"),
                ("publications", "committed_at"),
                ("publication_outcomes", "committed_at"),
                ("tag_history", "changed_at"),
                ("notes", "modified_at"),
                ("blobs", "changed_at"),
                ("artifact_blobs", "mapped_at"),
                ("staged_archives", "staged_at"),
                ("removed_packages", "unpublished_at"),
            ] {
                let mut row = connection.query_one(column_type_sql, params![table, column])?;
                assert_eq!(
                    row.get::<String>(0)?.to_ascii_lowercase(),
                    integer_type,
                    "{table}.{column}",
                );
            }
            Ok(())
        })?;
        Ok::<_, pnpr_error::RegistryError>(())
    })
    .await
    .unwrap()
    .unwrap();
}

async fn an_invalid_publication_time_refuses_the_write(db: Db) {
    let storage = storage(&db);
    let org = storage.for_hosted("", pnpr_package_name::Ecosystem::Npm);
    let mut document = packument("example", &["1.0.0"]);
    document["time"]["1.0.0"] = json!("not a timestamp");
    let error = try_write(&org, "example", &document).await.unwrap_err();
    assert!(matches!(error, pnpr_error::RegistryError::BadRequest { .. }), "{error:?}");
    assert!(
        org.read_hosted_document(&npm("example")).await
            .unwrap()
            .is_none(),
    );
}

async fn unpublish_times_use_integer_milliseconds(db: Db) {
    let storage = storage(&db);
    let org = storage.for_hosted("", pnpr_package_name::Ecosystem::Npm);
    let before = now_millis();
    write(&org, "example", &packument("example", &["1.0.0"])).await;
    assert!(
        org.remove_package(&npm("example"), Some("publisher")).await
            .unwrap(),
    );
    let tombstone = org
        .read_tombstone(&npm("example"))
        .await
        .unwrap()
        .unwrap();
    let unpublished = chrono::DateTime::parse_from_rfc3339(&tombstone.unpublished_at).unwrap();
    assert!((before..=now_millis()).contains(&unpublished.timestamp_millis()));
    let adapter = db.adapter();
    tokio::task::spawn_blocking(move || {
        adapter.read(ReadSource::Primary, &mut |connection| {
            assert_timestamp_column(connection, ("removed_packages", "unpublished_at"), before)
        })
    })
    .await
    .unwrap()
    .unwrap();
}

conformance! {
    publication_times_keep_milliseconds_and_normalize_offsets,
    metadata_timestamps_are_integer_milliseconds,
    every_timestamp_column_has_integer_affinity,
    an_invalid_publication_time_refuses_the_write,
    unpublish_times_use_integer_milliseconds,
}
