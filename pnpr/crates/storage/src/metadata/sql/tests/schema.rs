//! What a database must hold for pnpr to open its metadata, and what a
//! database pnpr refuses is left with.

use super::{
    super::{
        SqlMetadata,
        adapter::ReadSource,
        batch::WriteTransaction,
        schema::{self, MARKER, SCHEMA_VERSION, TABLES},
    },
    Db,
};
use pnpr_error::{RegistryError, Result};
use std::sync::Arc;

pub(super) const PAYLOAD_TABLES: [&str; 5] =
    ["packages", "accepted_manifests", "release_records", "log_entries", "readmes"];

/// Run `statements` on `db` in one transaction, as an operator would.
pub(super) async fn execute(db: &Db, statements: &[&str]) {
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
                .schema_table_names()?
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
        let mut versions = Vec::new();
        adapter.read(ReadSource::Primary, &mut |connection| {
            versions = connection
                .query(&format!("SELECT version FROM {MARKER}"), params![])?
                .into_iter()
                .map(|mut row| row.get::<String>(0))
                .collect::<Result<Vec<_>>>()?;
            Ok(())
        })?;
        Ok::<_, RegistryError>(versions)
    })
    .await
    .unwrap()
    .unwrap()
}

/// Open the metadata of `db` and check its schema.
async fn open(db: &Db) -> Result<()> {
    let metadata =
        Arc::new(SqlMetadata::with_adapter(db.instance_adapter(), "the test database".into()));
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
    assert!(reason.contains(&format!("expects schema version {SCHEMA_VERSION}.")), "{reason}");
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
    assert_eq!(marker(&db).await, [SCHEMA_VERSION]);
}

async fn payload_tables_have_rowids(db: Db) {
    open(&db).await.unwrap();
    let adapter = db.instance_adapter();
    tokio::task::spawn_blocking(move || {
        adapter.read(ReadSource::Replica, &mut |connection| {
            for table in PAYLOAD_TABLES {
                connection.query(&format!("SELECT rowid FROM {table} LIMIT 1"), params![])?;
            }
            Ok(())
        })
    })
    .await
    .unwrap()
    .unwrap();
}

async fn a_matching_version_opens(db: Db) {
    open(&db).await.unwrap();
    let created = tables(&db).await;
    open(&db).await.unwrap();
    assert_eq!(tables(&db).await, created);
    assert_eq!(marker(&db).await, [SCHEMA_VERSION]);
}

async fn another_version_is_refused(db: Db) {
    let insert = format!("INSERT INTO {MARKER} (version) VALUES ('0.4')");
    execute(
        &db,
        &[
            "CREATE TABLE schema_version (version TEXT NOT NULL)",
            &insert,
            "CREATE TABLE catalogs (catalog VARCHAR(512) NOT NULL PRIMARY KEY)",
        ],
    )
    .await;
    let reason = refused(&db).await;
    assert!(reason.contains("holds schema version 0.4,"), "{reason}");
    // Once the operator wipes the tables, pnpr creates them afresh.
    execute(&db, &[&format!("DROP TABLE {MARKER}"), "DROP TABLE catalogs"]).await;
    open(&db).await.unwrap();
    assert_eq!(marker(&db).await, [SCHEMA_VERSION]);
}

async fn an_integer_schema_version_is_refused(db: Db) {
    execute(
        &db,
        &[
            &format!("CREATE TABLE {MARKER} (version INTEGER NOT NULL)"),
            &format!("INSERT INTO {MARKER} (version) VALUES (4)"),
            "CREATE TABLE catalogs (catalog VARCHAR(512) NOT NULL PRIMARY KEY)",
        ],
    )
    .await;
    let reason = refused(&db).await;
    assert!(reason.contains("holds the integer schema version 4,"), "{reason}");
}

async fn a_matching_marker_without_its_tables_is_refused(db: Db) {
    open(&db).await.unwrap();
    // A wipe that drops the tables but leaves the marker.
    execute(&db, &["DROP TABLE catalogs", "DROP TABLE note_history"]).await;
    let reason = refused(&db).await;
    assert!(
        reason.contains(&format!(
            "holds schema version {SCHEMA_VERSION} without the metadata tables catalogs, \
             note_history,"
        )),
        "{reason}",
    );
}

async fn metadata_tables_without_a_marker_are_refused(db: Db) {
    // `SQLite` table names match without regard to case.
    execute(&db, &["CREATE TABLE PACKAGES (name TEXT NOT NULL)"]).await;
    let reason = refused(&db).await;
    assert!(reason.contains("holds metadata tables without a schema marker,"), "{reason}");
}

conformance! {
    an_empty_database_initializes,
    a_matching_marker_without_its_tables_is_refused,
    a_matching_version_opens,
    another_version_is_refused,
    an_integer_schema_version_is_refused,
    metadata_tables_without_a_marker_are_refused,
}

#[test]
fn every_metadata_table_is_known() {
    let tables = schema::table_names(TABLES);
    assert_eq!(tables.len(), TABLES.matches("CREATE TABLE").count(), "{tables:?}");
    assert!(tables.contains(&"catalogs".to_string()));
    assert!(tables.contains(&"note_history".to_string()));
    assert!(!tables.contains(&MARKER.to_string()));
    assert_eq!(schema::table_names(schema::MARKER_TABLE), [MARKER]);
    let upper = TABLES.to_ascii_uppercase();
    assert!(!upper.contains("FOREIGN KEY"), "schema still declares a foreign key");
    assert!(!upper.contains("REFERENCES "), "schema still declares a foreign key reference");
}

#[test]
fn a_table_is_known_however_its_statement_is_spaced_or_cased() {
    assert_eq!(
        schema::table_names("create table\n  if not exists\tcatalogs(catalog TEXT);"),
        ["catalogs"],
    );
    assert_eq!(schema::table_names("CREATE\nTABLE packages (name TEXT);"), ["packages"]);
    assert!(schema::table_names("CREATE INDEX packages_by_name ON packages (name);").is_empty());
}

mod embedded_layout {
    use super::payload_tables_have_rowids;
    embedded_conformance!(payload_tables_have_rowids);
}
