//! Timestamp storage and the protocol representation at its boundaries.

use super::{Db, npm, packument, read, storage, try_write, write};
use crate::{
    metadata::sql::adapter::ReadSource,
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
        adapter.read(ReadSource::Primary, &mut |connection| {
            for (table, column) in [
                ("versions", "published_at"),
                ("publications", "committed_at"),
                ("tag_history", "changed_at"),
                ("notes", "modified_at"),
                ("note_history", "changed_at"),
            ] {
                let rows = connection.query(
                    &format!("SELECT {column}, typeof({column}) FROM {table}"),
                    params![],
                )?;
                assert!(!rows.is_empty(), "{table}");
                for mut row in rows {
                    assert_eq!(row.get::<String>(1)?, "integer", "{table}");
                    let millis: i64 = row.get(0)?;
                    if table == "versions" {
                        assert_eq!(iso_from_unix_millis(millis), "2026-01-01T00:00:00.000Z");
                    } else {
                        assert!((before..=now_millis()).contains(&millis), "{table}: {millis}");
                    }
                }
            }
            Ok(())
        })?;
        Ok::<_, pnpr_error::RegistryError>(())
    })
    .await
    .unwrap()
    .unwrap();
}

async fn every_timestamp_column_has_integer_affinity(db: Db) {
    let storage = storage(&db);
    let org = storage.for_hosted("", pnpr_package_name::Ecosystem::Npm);
    write(&org, "example", &packument("example", &["1.0.0"])).await;
    let adapter = db.adapter();
    tokio::task::spawn_blocking(move || {
        adapter.read(ReadSource::Primary, &mut |connection| {
            for (table, column) in [
                ("versions", "published_at"),
                ("publications", "committed_at"),
                ("publication_outcomes", "committed_at"),
                ("tag_history", "changed_at"),
                ("notes", "modified_at"),
                ("note_history", "changed_at"),
                ("blobs", "changed_at"),
                ("artifact_blobs", "mapped_at"),
                ("staged_archives", "staged_at"),
            ] {
                let rows = connection.query(&format!("PRAGMA table_info({table})"), params![])?;
                let mut found = false;
                for mut row in rows {
                    if row.get::<String>(1)? == column {
                        assert_eq!(row.get::<String>(2)?, "INTEGER", "{table}.{column}");
                        found = true;
                    }
                }
                assert!(found, "{table}.{column}");
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
            .is_none()
    );
}

conformance! {
    publication_times_keep_milliseconds_and_normalize_offsets,
    metadata_timestamps_are_integer_milliseconds,
    every_timestamp_column_has_integer_affinity,
    an_invalid_publication_time_refuses_the_write,
}
