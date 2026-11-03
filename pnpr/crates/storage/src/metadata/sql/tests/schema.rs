//! What a database must hold for pnpr to open its metadata, and what a
//! database pnpr refuses is left with.

use super::{
    super::{
        SqlMetadata,
        adapter::ReadSource,
        batch::WriteTransaction,
        schema::{self, MARKER, TABLES},
    },
    Db,
};
use pnpr_error::{RegistryError, Result};
use std::sync::Arc;

/// Run `statements` on `db` in one transaction, as an operator would.
async fn execute(db: &Db, statements: &[&str]) {
    let adapter = db.adapter();
    let statements: Vec<String> = statements
        .iter()
        .map(|statement| (*statement).to_string())
        .collect();
    tokio::task::spawn_blocking(move || {
        adapter.write(&mut |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            for statement in &statements {
                transaction.execute(statement, params![])?;
            }
            transaction.commit()
        })
    })
    .await
    .unwrap()
    .unwrap();
}

/// The tables of `db`, sorted.
async fn tables(db: &Db) -> Vec<String> {
    let adapter = db.adapter();
    tokio::task::spawn_blocking(move || {
        let mut tables = Vec::new();
        adapter.read(ReadSource::Primary, &mut |connection| {
            tables = connection
                .query("SELECT name FROM sqlite_master WHERE type = 'table'", params![])?
                .into_iter()
                .map(|mut row| row.get::<String>(0))
                .collect::<Result<Vec<_>>>()?;
            Ok(())
        })?;
        tables.sort();
        Ok::<_, RegistryError>(tables)
    })
    .await
    .unwrap()
    .unwrap()
}

/// What the marker of `db` holds.
async fn marker(db: &Db) -> Vec<String> {
    let adapter = db.adapter();
    tokio::task::spawn_blocking(move || {
        let mut fingerprints = Vec::new();
        adapter.read(ReadSource::Primary, &mut |connection| {
            fingerprints = connection
                .query(&format!("SELECT fingerprint FROM {MARKER}"), params![])?
                .into_iter()
                .map(|mut row| row.get::<String>(0))
                .collect::<Result<Vec<_>>>()?;
            Ok(())
        })?;
        Ok::<_, RegistryError>(fingerprints)
    })
    .await
    .unwrap()
    .unwrap()
}

/// Open the metadata of `db` and check its schema.
async fn open(db: &Db) -> Result<()> {
    let metadata = Arc::new(SqlMetadata::with_adapter(db.adapter(), "the test database".into()));
    tokio::task::spawn_blocking(move || metadata.ensure_schema()).await.unwrap()
}

/// Open `db`, which pnpr must refuse without changing its tables, and
/// return the reason it gives.
async fn refused(db: &Db) -> String {
    let before = tables(db).await;
    let error = open(db).await.unwrap_err();
    assert_eq!(tables(db).await, before, "a refused database gained tables");
    let RegistryError::Internal { reason } = error else {
        panic!("a schema change is not a transient failure: {error:?}");
    };
    assert!(reason.contains("the metadata schema changed"), "{reason}");
    assert!(reason.contains("wipe pnpr's metadata tables"), "{reason}");
    assert!(
        reason.contains(&format!("expects schema {}", schema::short(schema::fingerprint()))),
        "{reason}",
    );
    reason
}

async fn an_empty_database_initializes(db: Db) {
    // Tables of anything else sharing the database, such as pnpr's auth
    // backend, leave it empty as far as the metadata is concerned.
    execute(&db, &["CREATE TABLE users (name TEXT NOT NULL)"]).await;
    open(&db).await.unwrap();
    let tables = tables(&db).await;
    for table in schema::table_names(TABLES) {
        assert!(tables.contains(&table), "{table} is missing from {tables:?}");
    }
    assert!(tables.contains(&MARKER.to_string()));
    assert_eq!(marker(&db).await, [schema::fingerprint()]);
}

async fn a_matching_fingerprint_opens(db: Db) {
    open(&db).await.unwrap();
    let created = tables(&db).await;
    open(&db).await.unwrap();
    assert_eq!(tables(&db).await, created);
    assert_eq!(marker(&db).await, [schema::fingerprint()]);
}

async fn a_different_fingerprint_is_refused(db: Db) {
    let stored = "0123456789abcdef".repeat(4);
    let insert = format!("INSERT INTO {MARKER} (fingerprint) VALUES ('{stored}')");
    execute(
        &db,
        &[
            &format!("CREATE TABLE {MARKER} (fingerprint TEXT NOT NULL) STRICT"),
            &insert,
            "CREATE TABLE catalogs (catalog TEXT NOT NULL PRIMARY KEY)",
        ],
    )
    .await;
    let reason = refused(&db).await;
    assert!(reason.contains("holds schema 0123456789ab,"), "{reason}");
    // Once the operator wipes the tables, pnpr creates them afresh.
    execute(&db, &[&format!("DROP TABLE {MARKER}"), "DROP TABLE catalogs"]).await;
    open(&db).await.unwrap();
    assert_eq!(marker(&db).await, [schema::fingerprint()]);
}

async fn an_integer_schema_version_is_refused(db: Db) {
    execute(
        &db,
        &[
            &format!("CREATE TABLE {MARKER} (version INTEGER NOT NULL) STRICT"),
            &format!("INSERT INTO {MARKER} (version) VALUES (3)"),
            "CREATE TABLE catalogs (catalog TEXT NOT NULL PRIMARY KEY)",
        ],
    )
    .await;
    let reason = refused(&db).await;
    assert!(reason.contains("holds the integer schema version 3,"), "{reason}");
}

async fn metadata_tables_without_a_marker_are_refused(db: Db) {
    execute(&db, &["CREATE TABLE packages (name TEXT NOT NULL)"]).await;
    let reason = refused(&db).await;
    assert!(reason.contains("holds metadata tables without a schema marker,"), "{reason}");
}

conformance! {
    an_empty_database_initializes,
    a_matching_fingerprint_opens,
    a_different_fingerprint_is_refused,
    an_integer_schema_version_is_refused,
    metadata_tables_without_a_marker_are_refused,
}

#[test]
fn the_fingerprint_changes_with_the_statements() {
    assert_eq!(schema::fingerprint_of(TABLES), schema::fingerprint());
    let added_column = TABLES.replacen(
        "generation INTEGER NOT NULL\n)",
        "generation INTEGER NOT NULL,\n    note TEXT\n)",
        1,
    );
    assert_ne!(added_column, TABLES);
    assert_ne!(schema::fingerprint_of(&added_column), schema::fingerprint());
    let added_index = format!("{TABLES}\nCREATE INDEX tags_by_version ON tags (version);");
    assert_ne!(schema::fingerprint_of(&added_index), schema::fingerprint());
}

#[test]
fn the_fingerprint_ignores_comments_and_whitespace() {
    let reformatted = TABLES.replace("    ", "\t\t").replace("-- ", "-- reworded: ");
    assert_ne!(reformatted, TABLES);
    assert_eq!(schema::fingerprint_of(&reformatted), schema::fingerprint());
}

#[test]
fn every_metadata_table_is_known() {
    let tables = schema::table_names(TABLES);
    assert_eq!(tables.len(), TABLES.matches("CREATE TABLE").count(), "{tables:?}");
    assert!(tables.contains(&"catalogs".to_string()));
    assert!(tables.contains(&"note_history".to_string()));
    assert!(!tables.contains(&MARKER.to_string()));
}
