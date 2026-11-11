//! What a database must hold for pnpr to open its metadata and auth tables,
//! how pending numbered migrations run, and what a database pnpr refuses is
//! left with.

use super::{
    super::{
        SqlMetadata,
        adapter::{Connection, ReadSource, WriteTransaction},
        schema::{self, LEGACY_AUTH_MARKER, LEGACY_MARKER, MARKER, SCHEMA_VERSION, Step, TABLES},
    },
    Db,
};
use pnpr_error::{RegistryError, Result};
use std::{sync::Arc, time::Duration};

pub(super) const PAYLOAD_TABLES: [&str; 9] = [
    "packages",
    "accepted_manifests",
    "release_records",
    "log_entries",
    "readmes",
    "scim_users",
    "cargo_versions",
    "pypi_files",
    "oci_manifests",
];

const V1_TABLES: &str = include_str!("fixtures/v1_tables.sql");
const V1_MYSQL_TABLES: &str = include_str!("fixtures/v1_mysql.sql");

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

/// What the `schema_migrations` table of `db` holds, in ascending order.
async fn marker(db: &Db) -> Vec<i64> {
    let adapter = db.adapter();
    tokio::task::spawn_blocking(move || {
        let mut versions = Vec::new();
        adapter.read(ReadSource::Primary, &mut |connection| {
            versions = connection
                .query(&format!("SELECT version FROM {MARKER} ORDER BY version"), params![])?
                .into_iter()
                .map(|mut row| row.get::<i64>(0))
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
    let RegistryError::InvalidConfig { reason } = error else {
        panic!("a schema refusal is not a transient failure: {error:?}");
    };
    assert!(reason.contains("the database the test database holds"), "{reason}");
    assert!(reason.contains(&format!("(expects schema version {SCHEMA_VERSION}).")), "{reason}");
    reason
}

#[derive(Debug, PartialEq, Eq)]
struct SchemaSnapshot {
    tables: Vec<String>,
    columns: Vec<(String, String, String, i64)>,
    indexes: Vec<(String, String)>,
    migrations: Vec<i64>,
    generation: i64,
}

fn read_table_columns(
    connection: &dyn Connection,
    column_sql: &str,
    tables: &[String],
) -> Result<Vec<(String, String, String, i64)>> {
    let mut columns = Vec::new();
    for table in tables {
        for mut row in connection.query(column_sql, params![table.as_str()])? {
            columns.push((
                table.clone(),
                row.get::<String>(0)?,
                row.get::<String>(1)?,
                row.get::<i64>(2)?,
            ));
        }
    }
    Ok(columns)
}

async fn snapshot(db: &Db) -> SchemaSnapshot {
    let tables = tables(db).await;
    let column_sql = db
        .sql(
            r#"SELECT name, lower(type), "notnull" FROM pragma_table_info(?1) ORDER BY cid"#,
            "SELECT column_name, lower(data_type), CASE WHEN is_nullable = 'NO' THEN 1 ELSE 0 END FROM information_schema.columns WHERE table_schema = current_schema() AND table_name = ?1 ORDER BY ordinal_position",
            "SELECT CAST(column_name AS CHAR), lower(CAST(data_type AS CHAR)), CASE WHEN is_nullable = 'NO' THEN 1 ELSE 0 END FROM information_schema.columns WHERE table_schema = DATABASE() AND table_name = ?1 ORDER BY ordinal_position",
        )
        .to_string();
    let index_sql = db
        .sql(
            "SELECT tbl_name, name FROM sqlite_master WHERE type = 'index' AND name NOT LIKE 'sqlite_autoindex_%' ORDER BY tbl_name, name",
            "SELECT tablename, indexname FROM pg_indexes WHERE schemaname = current_schema() AND indexname NOT LIKE '%_pkey' AND indexname NOT LIKE 'users_%_key' ORDER BY tablename, indexname",
            "SELECT DISTINCT CAST(table_name AS CHAR), CAST(index_name AS CHAR) FROM information_schema.statistics WHERE table_schema = DATABASE() AND index_name <> 'PRIMARY' AND index_name NOT IN ('username', 'username_skeleton_hash') ORDER BY 1, 2",
        )
        .to_string();
    let adapter = db.adapter();
    let table_list = tables.clone();
    let (columns, indexes, migrations, generation) = tokio::task::spawn_blocking(move || {
        let mut columns = Vec::new();
        let mut indexes = Vec::new();
        let mut migrations = Vec::new();
        let mut generation = 0;
        adapter.read(ReadSource::Primary, &mut |connection| {
            columns = read_table_columns(connection, &column_sql, &table_list)?;
            for mut row in connection.query(&index_sql, params![])? {
                indexes.push((row.get::<String>(0)?, row.get::<String>(1)?));
            }
            migrations = connection
                .query(&format!("SELECT version FROM {MARKER} ORDER BY version"), params![])?
                .into_iter()
                .map(|mut row| row.get::<i64>(0))
                .collect::<Result<Vec<_>>>()?;
            generation = connection
                .query_one(
                    "SELECT generation FROM record_generation WHERE singleton = 1",
                    params![],
                )?
                .get::<i64>(0)?;
            Ok(())
        })?;
        Ok::<_, RegistryError>((columns, indexes, migrations, generation))
    })
    .await
    .unwrap()
    .unwrap();
    SchemaSnapshot { tables, columns, indexes, migrations, generation }
}

async fn an_empty_database_initializes(db: Db) {
    // Tables of anything else sharing the database leave it empty as far as
    // pnpr is concerned.
    execute(&db, &["CREATE TABLE external_audit (name TEXT NOT NULL)"]).await;
    open(&db).await.unwrap();
    let tables = tables(&db).await;
    for table in schema::table_names(TABLES) {
        assert!(tables.contains(&table), "{table} is missing from {tables:?}");
    }
    assert!(tables.contains(&MARKER.to_string()));
    assert_eq!(marker(&db).await, (1..=SCHEMA_VERSION).collect::<Vec<_>>());
}

async fn wipe_tables(db: &Db, table_names: &[String]) {
    let drop_statements: Vec<String> = table_names
        .iter()
        .map(|table| format!("DROP TABLE {table}"))
        .collect();
    let drop_refs: Vec<&str> = drop_statements
        .iter()
        .map(String::as_str)
        .collect();
    execute(db, &drop_refs).await;
    assert!(tables(db).await.is_empty());
}

async fn applying_every_step_matches_the_fresh_schema(db: Db) {
    let adapter = db.adapter();
    tokio::task::spawn_blocking(move || {
        adapter.write(&mut |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            let ddl = transaction.schema_tables().unwrap_or_else(|| TABLES.to_string());
            for statement in schema::statements(&ddl) {
                transaction.execute_ddl(&statement)?;
            }
            let marker_ddl = transaction.schema_marker().unwrap_or(schema::MARKER_TABLE);
            transaction.execute(marker_ddl, params![])?;
            for step in &schema::STEPS[1..] {
                step(&*transaction)?;
            }
            for version in 1..=SCHEMA_VERSION {
                transaction.execute(
                    &format!("INSERT INTO {MARKER} (version, applied_at) VALUES (?1, 1)"),
                    params![version],
                )?;
            }
            transaction.commit()
        })
    })
    .await
    .unwrap()
    .unwrap();
    let fresh = snapshot(&db).await;

    wipe_tables(&db, &fresh.tables).await;

    let adapter = db.instance_adapter();
    tokio::task::spawn_blocking(move || {
        let refused = schema::prepare_adapter_with_steps(&*adapter, schema::STEPS)?;
        assert_eq!(refused, None);
        Ok::<_, RegistryError>(())
    })
    .await
    .unwrap()
    .unwrap();
    let migrated = snapshot(&db).await;
    assert_eq!(migrated, fresh);
}

fn step_2_ddl(connection: &dyn Connection) -> Result<()> {
    connection.execute_ddl("ALTER TABLE auth_counters ADD COLUMN note TEXT")?;
    connection.execute_ddl(
        "CREATE INDEX IF NOT EXISTS auth_counters_by_value ON auth_counters (value)",
    )
}

fn step_3_rust_data_fix(connection: &dyn Connection) -> Result<()> {
    let rows = connection.query("SELECT name, value FROM auth_counters", params![])?;
    for mut row in rows {
        let name: String = row.get(0)?;
        let value: i64 = row.get(1)?;
        let note = format!("{name}:{value}");
        connection.execute(
            "UPDATE auth_counters SET note = ?1 WHERE name = ?2",
            params![note.as_str(), name.as_str()],
        )?;
    }
    connection.execute(
        "UPDATE record_generation SET generation = generation + 10 WHERE singleton = 1 AND generation < 10",
        params![],
    )
}

fn insert_v1_fixture_rows(connection: &dyn Connection) -> Result<()> {
    connection.execute(
        &format!("INSERT INTO {MARKER} (version, applied_at) VALUES (1, 1)"),
        params![],
    )?;
    connection.execute(
        "UPDATE record_generation SET generation = 5 WHERE singleton = 1",
        params![],
    )?;
    connection.execute(
        "INSERT INTO namespaces (namespace, generation) VALUES (?1, ?2)",
        params!["acme", 3_i64],
    )?;
    connection.execute(
        "INSERT INTO users (id, username, username_skeleton_hash, bcrypt_hash) VALUES (?1, ?2, ?3, ?4)",
        params!["u-1", "alice", "skel-alice", "$2b$12$hash"],
    )?;
    connection.execute(
        "INSERT INTO tokens (token_hash, user_id, created_at, last_used_at, readonly, cidr_whitelist, name) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params!["tok-1", "u-1", 1000_i64, 1000_i64, 0_i64, "[]", "ci"],
    )?;
    connection.execute(
        "INSERT INTO auth_counters (name, value) VALUES (?1, ?2)",
        params!["users", 1_i64],
    )
}

async fn seed_v1_fixture(db: &Db) {
    let pg_ddl = V1_TABLES
        .replace(" STRICT, WITHOUT ROWID", "")
        .replace(" STRICT", "")
        .replace(" BLOB", " BYTEA")
        .replace(" INTEGER", " BIGINT");
    let ddl = db.sql(V1_TABLES, &pg_ddl, V1_MYSQL_TABLES).to_string();
    let adapter = db.adapter();
    tokio::task::spawn_blocking(move || {
        adapter.write(&mut |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            for statement in schema::statements(&ddl) {
                transaction.execute_ddl(&statement)?;
            }
            let marker_ddl = transaction.schema_marker().unwrap_or(schema::MARKER_TABLE);
            transaction.execute(marker_ddl, params![])?;
            insert_v1_fixture_rows(&*transaction)?;
            transaction.commit()
        })
    })
    .await
    .unwrap()
    .unwrap();
}

async fn assert_migrated_v1_fixture_rows(db: &Db) {
    let adapter = db.adapter();
    tokio::task::spawn_blocking(move || {
        adapter.read(ReadSource::Primary, &mut |connection| {
            let note: String = connection
                .query_one("SELECT note FROM auth_counters WHERE name = ?1", params!["users"])?
                .get(0)?;
            assert_eq!(note, "users:1");
            let generation: i64 = connection
                .query_one(
                    "SELECT generation FROM record_generation WHERE singleton = 1",
                    params![],
                )?
                .get(0)?;
            assert_eq!(generation, 15);
            let username: String = connection
                .query_one("SELECT username FROM users WHERE id = ?1", params!["u-1"])?
                .get(0)?;
            assert_eq!(username, "alice");
            let namespace_gen: i64 = connection
                .query_one(
                    "SELECT generation FROM namespaces WHERE namespace = ?1",
                    params!["acme"],
                )?
                .get(0)?;
            assert_eq!(namespace_gen, 3);
            Ok(())
        })
    })
    .await
    .unwrap()
    .unwrap();
}

async fn applying_from_each_released_version_fixture_preserves_data_and_advances(db: Db) {
    open(&db).await.unwrap();
    let mut expected = snapshot(&db).await;
    wipe_tables(&db, &expected.tables).await;

    seed_v1_fixture(&db).await;
    open(&db).await.unwrap();
    let migrated_from_v1 = snapshot(&db).await;
    expected.generation = migrated_from_v1.generation;
    assert_eq!(migrated_from_v1, expected);
    wipe_tables(&db, &expected.tables).await;
    seed_v1_fixture(&db).await;

    let adapter = db.instance_adapter();
    tokio::task::spawn_blocking(move || {
        let steps: &[Step] = &[schema::step_1, schema::step_2, step_2_ddl, step_3_rust_data_fix];
        let refused = schema::prepare_adapter_with_steps(&*adapter, steps)?;
        assert_eq!(refused, None);
        Ok::<_, RegistryError>(())
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(marker(&db).await, [1, 2, 3, 4]);
    assert_migrated_v1_fixture_rows(&db).await;
}

fn failing_step_2(connection: &dyn Connection) -> Result<()> {
    connection.execute_ddl("ALTER TABLE auth_counters ADD COLUMN note TEXT")?;
    connection.execute(
        "UPDATE record_generation SET generation = 99 WHERE singleton = 1",
        params![],
    )?;
    Err(RegistryError::Internal { reason: "simulated step 2 failure".to_string() })
}

fn recovering_step_2(connection: &dyn Connection) -> Result<()> {
    connection.execute_ddl("ALTER TABLE auth_counters ADD COLUMN note TEXT")?;
    connection.execute(
        "UPDATE record_generation SET generation = 42 WHERE singleton = 1",
        params![],
    )
}

async fn a_failed_step_is_not_recorded_and_can_be_rerun(db: Db) {
    let adapter = db.instance_adapter();
    let error = tokio::task::spawn_blocking(move || {
        let steps: &[Step] = &[schema::step_1, failing_step_2];
        schema::prepare_adapter_with_steps(&*adapter, steps)
    })
    .await
    .unwrap()
    .unwrap_err();
    assert!(error.to_string().contains("simulated step 2 failure"), "{error}");
    assert_eq!(marker(&db).await, [1]);

    // On databases with transactional DDL, the partial DDL and data change rolled back.
    let adapter = db.adapter();
    tokio::task::spawn_blocking(move || {
        adapter.read(ReadSource::Primary, &mut |connection| {
            if connection.transactional_ddl() {
                let generation: i64 = connection
                    .query_one(
                        "SELECT generation FROM record_generation WHERE singleton = 1",
                        params![],
                    )?
                    .get(0)?;
                assert_eq!(generation, 0);
            }
            Ok(())
        })
    })
    .await
    .unwrap()
    .unwrap();

    // Rerunning with a fixed step 2 succeeds on every adapter (including MySQL).
    let adapter = db.instance_adapter();
    tokio::task::spawn_blocking(move || {
        let steps: &[Step] = &[schema::step_1, recovering_step_2];
        let refused = schema::prepare_adapter_with_steps(&*adapter, steps)?;
        assert_eq!(refused, None);
        Ok::<_, RegistryError>(())
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(marker(&db).await, [1, 2]);
}

fn step_2_increment_generation(connection: &dyn Connection) -> Result<()> {
    schema::step_2(connection)?;
    std::thread::sleep(Duration::from_millis(25));
    connection.execute(
        "UPDATE record_generation SET generation = generation + 1 WHERE singleton = 1",
        params![],
    )
}

async fn two_replicas_starting_at_once_do_not_both_migrate(db: Db) {
    let adapter_a = db.instance_adapter();
    let adapter_b = db.instance_adapter();
    let steps: &[Step] = &[schema::step_1, step_2_increment_generation];
    let task_a =
        tokio::task::spawn_blocking(move || schema::prepare_adapter_with_steps(&*adapter_a, steps));
    let task_b =
        tokio::task::spawn_blocking(move || schema::prepare_adapter_with_steps(&*adapter_b, steps));
    let (res_a, res_b) = tokio::join!(task_a, task_b);
    assert_eq!(res_a.unwrap().unwrap(), None);
    assert_eq!(res_b.unwrap().unwrap(), None);
    assert_eq!(marker(&db).await, [1, 2]);

    let adapter = db.adapter();
    tokio::task::spawn_blocking(move || {
        adapter.read(ReadSource::Primary, &mut |connection| {
            let generation: i64 = connection
                .query_one(
                    "SELECT generation FROM record_generation WHERE singleton = 1",
                    params![],
                )?
                .get(0)?;
            assert_eq!(generation, 1, "step_2 ran more than once");
            Ok(())
        })
    })
    .await
    .unwrap()
    .unwrap();
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

async fn unused_metadata_objects_are_absent(db: Db) {
    open(&db).await.unwrap();
    let existing_tables = tables(&db).await;
    assert!(!existing_tables.contains(&"note_history".to_string()));
    assert!(!existing_tables.contains(&"registry_records".to_string()));
    let index_names = db.sql(
        "SELECT name FROM sqlite_master WHERE type = 'index'",
        "SELECT indexname FROM pg_indexes WHERE schemaname = current_schema()",
        "SELECT DISTINCT index_name FROM information_schema.statistics WHERE table_schema = DATABASE()",
    );
    let adapter = db.instance_adapter();
    tokio::task::spawn_blocking(move || {
        adapter.read(ReadSource::Primary, &mut |connection| {
            let rows = connection.query(index_names, params![])?;
            let names = rows
                .into_iter()
                .map(|mut row| row.get::<String>(0))
                .collect::<Result<Vec<_>>>()?;
            for removed in ["publication_outcomes_by_key", "versions_by_accepted_generation"] {
                assert!(!names.contains(&removed.to_string()), "{removed}: {names:?}");
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
    assert_eq!(marker(&db).await, (1..=SCHEMA_VERSION).collect::<Vec<_>>());
}

async fn a_future_version_is_refused(db: Db) {
    open(&db).await.unwrap();
    let future = SCHEMA_VERSION + 1;
    execute(&db, &[&format!("INSERT INTO {MARKER} (version, applied_at) VALUES ({future}, 1)")])
        .await;
    let reason = refused(&db).await;
    assert!(reason.contains(&format!("holds schema version {future},")), "{reason}");
    assert!(!reason.contains("Pre-1.0"), "{reason}");
}

async fn another_version_is_refused(db: Db) {
    let insert = format!("INSERT INTO {LEGACY_MARKER} (version) VALUES ('0.5')");
    execute(
        &db,
        &[
            "CREATE TABLE schema_version (version TEXT NOT NULL)",
            &insert,
            "CREATE TABLE namespaces (namespace VARCHAR(512) NOT NULL PRIMARY KEY)",
        ],
    )
    .await;
    let reason = refused(&db).await;
    assert!(reason.contains("holds pre-1.0 schema version 0.5,"), "{reason}");
    assert!(reason.contains("wipe pnpr's tables"), "{reason}");
    // Once the operator wipes the tables, pnpr creates them afresh.
    execute(&db, &[&format!("DROP TABLE {LEGACY_MARKER}"), "DROP TABLE namespaces"]).await;
    open(&db).await.unwrap();
    assert_eq!(marker(&db).await, (1..=SCHEMA_VERSION).collect::<Vec<_>>());
}

async fn an_integer_schema_version_is_refused(db: Db) {
    execute(
        &db,
        &[
            &format!("CREATE TABLE {LEGACY_MARKER} (version INTEGER NOT NULL)"),
            &format!("INSERT INTO {LEGACY_MARKER} (version) VALUES (4)"),
            "CREATE TABLE namespaces (namespace VARCHAR(512) NOT NULL PRIMARY KEY)",
        ],
    )
    .await;
    let reason = refused(&db).await;
    assert!(reason.contains("holds the pre-1.0 integer schema version 4,"), "{reason}");
}

async fn a_pre_1_0_auth_migrations_table_is_refused(db: Db) {
    execute(
        &db,
        &[
            &format!(
                "CREATE TABLE {LEGACY_AUTH_MARKER} (version BIGINT PRIMARY KEY, applied_at BIGINT NOT NULL)",
            ),
            &format!("INSERT INTO {LEGACY_AUTH_MARKER} (version, applied_at) VALUES (4, 100)"),
        ],
    )
    .await;
    let reason = refused(&db).await;
    assert!(
        reason.contains(&format!(
            "holds pre-1.0 {LEGACY_AUTH_MARKER} tables without a {MARKER} table,"
        )),
        "{reason}",
    );
}

async fn a_matching_marker_without_its_tables_is_refused(db: Db) {
    open(&db).await.unwrap();
    // A wipe that drops metadata or auth tables but leaves the marker.
    execute(&db, &["DROP TABLE namespaces", "DROP TABLE notes", "DROP TABLE users"]).await;
    let reason = refused(&db).await;
    assert!(
        reason.contains(&format!(
            "holds schema version {SCHEMA_VERSION} without the tables namespaces, \
             notes, users,"
        )),
        "{reason}",
    );
    assert!(!reason.contains("Pre-1.0"), "{reason}");
}

async fn an_unreadable_schema_migrations_table_is_refused(db: Db) {
    open(&db).await.unwrap();
    execute(
        &db,
        &[&format!(
            "INSERT INTO {MARKER} (version, applied_at) VALUES ({}, 1)",
            SCHEMA_VERSION + 2,
        )],
    )
    .await;
    let reason = refused(&db).await;
    assert!(reason.contains(&format!("holds an unreadable {MARKER} table,")), "{reason}");
    assert!(!reason.contains("Pre-1.0"), "{reason}");
}

async fn an_unreadable_legacy_schema_version_table_is_refused(db: Db) {
    execute(&db, &[&format!("CREATE TABLE {LEGACY_MARKER} (version TEXT NOT NULL)")]).await;
    let reason = refused(&db).await;
    assert!(reason.contains(&format!("holds an unreadable {LEGACY_MARKER} table,")), "{reason}");
    assert!(reason.contains("Pre-1.0"), "{reason}");
}

async fn metadata_tables_without_a_marker_are_refused(db: Db) {
    // `SQLite` table names match without regard to case.
    execute(&db, &["CREATE TABLE PACKAGES (name TEXT NOT NULL)"]).await;
    let reason = refused(&db).await;
    assert!(
        reason.contains(&format!("holds metadata or auth tables without a {MARKER} table,")),
        "{reason}",
    );
}

async fn auth_tables_without_a_marker_are_refused(db: Db) {
    execute(&db, &["CREATE TABLE TOKENS (token_hash TEXT NOT NULL)"]).await;
    let reason = refused(&db).await;
    assert!(
        reason.contains(&format!("holds metadata or auth tables without a {MARKER} table,")),
        "{reason}",
    );
}

conformance! {
    an_empty_database_initializes,
    unused_metadata_objects_are_absent,
    applying_every_step_matches_the_fresh_schema,
    applying_from_each_released_version_fixture_preserves_data_and_advances,
    a_failed_step_is_not_recorded_and_can_be_rerun,
    two_replicas_starting_at_once_do_not_both_migrate,
    a_matching_marker_without_its_tables_is_refused,
    an_unreadable_schema_migrations_table_is_refused,
    an_unreadable_legacy_schema_version_table_is_refused,
    a_matching_version_opens,
    a_future_version_is_refused,
    another_version_is_refused,
    an_integer_schema_version_is_refused,
    a_pre_1_0_auth_migrations_table_is_refused,
    metadata_tables_without_a_marker_are_refused,
    auth_tables_without_a_marker_are_refused,
}

#[test]
fn every_schema_table_is_known() {
    let tables = schema::table_names(TABLES);
    assert_eq!(tables.len(), TABLES.matches("CREATE TABLE").count(), "{tables:?}");
    assert!(tables.contains(&"namespaces".to_string()));
    assert!(tables.contains(&"notes".to_string()));
    assert!(tables.contains(&"removed_oci_manifests".to_string()));
    assert!(!tables.contains(&"note_history".to_string()));
    assert!(tables.contains(&"users".to_string()));
    assert!(tables.contains(&"tokens".to_string()));
    assert!(tables.contains(&"auth_counters".to_string()));
    assert!(!tables.contains(&MARKER.to_string()));
    assert_eq!(schema::table_names(schema::MARKER_TABLE), [MARKER]);
    let upper = TABLES.to_ascii_uppercase();
    assert!(!upper.contains("FOREIGN KEY"), "schema still declares a foreign key");
    assert!(!upper.contains("REFERENCES "), "schema still declares a foreign key reference");
}

#[test]
fn a_table_is_known_however_its_statement_is_spaced_or_cased() {
    assert_eq!(
        schema::table_names("create table\n  if not exists\tnamespaces(namespace TEXT);"),
        ["namespaces"],
    );
    assert_eq!(schema::table_names("CREATE\nTABLE packages (name TEXT);"), ["packages"]);
    assert!(schema::table_names("CREATE INDEX packages_by_name ON packages (name);").is_empty());
}

mod embedded_layout {
    use super::payload_tables_have_rowids;
    embedded_conformance!(payload_tables_have_rowids);
}

#[cfg(feature = "backend-mysql")]
mod mysql_recovery {
    use super::{
        Connection, Db, MARKER, ReadSource, RegistryError, Result, SCHEMA_VERSION, SqlMetadata,
        schema,
    };
    use std::sync::Arc;

    async fn mysql_schema_statements(db: &Db) -> Vec<String> {
        let adapter = db.adapter();
        tokio::task::spawn_blocking(move || {
            let mut out = Vec::new();
            adapter.read(ReadSource::Primary, &mut |conn| {
                let ddl = conn.schema_tables().unwrap();
                out = schema::statements(&ddl).collect();
                Ok(())
            })?;
            Ok::<_, RegistryError>(out)
        })
        .await
        .unwrap()
        .unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires PNPR_TEST_MYSQL_URL"]
    async fn on_mysql_interrupted_schema_creation_resumes() {
        let adapter = Db::sqlite().adapter();
        drop(adapter);
        let db = Db::network("PNPR_TEST_MYSQL_URL").await;
        let schema_statements = mysql_schema_statements(&db).await;
        let first_index = schema_statements
            .iter()
            .position(|sql| sql.starts_with("CREATE INDEX"))
            .unwrap();
        for completed in [0, first_index + 1, schema_statements.len()] {
            let db = Db::network("PNPR_TEST_MYSQL_URL").await;
            let mut partial = schema_statements[..completed].to_vec();
            let all_created = completed == schema_statements.len();
            if all_created {
                partial.push("UPDATE record_generation SET generation = 7".to_string());
            }
            interrupt(&db, partial).await;
            let metadata =
                Arc::new(SqlMetadata::with_adapter(db.instance_adapter(), "test".into()));
            tokio::task::spawn_blocking(move || metadata.ensure_schema()).await.unwrap().unwrap();
            assert_initialized(&db, if all_created { 7 } else { 0 }).await;
        }
    }

    async fn interrupt(db: &Db, partial: Vec<String>) {
        let adapter = db.adapter();
        let interrupted = tokio::task::spawn_blocking(move || {
            adapter.write(&mut |connection| {
                initialize_prefix(connection, &partial)?;
                panic!("schema initialization interrupted");
            })
        })
        .await
        .unwrap_err();
        assert!(interrupted.is_panic());
    }

    fn initialize_prefix(connection: &dyn Connection, partial: &[String]) -> Result<()> {
        connection.lock_schema()?;
        connection.begin_write()?;
        let marker = connection.schema_marker().unwrap();
        connection.execute(marker, params![])?;
        for statement in partial {
            connection.execute_ddl(statement)?;
        }
        Ok(())
    }

    async fn assert_initialized(db: &Db, generation: i64) {
        assert_eq!(super::marker(db).await, (1..=SCHEMA_VERSION).collect::<Vec<_>>());
        let adapter = db.adapter();
        tokio::task::spawn_blocking(move || {
            adapter.read(ReadSource::Primary, &mut |connection| {
                let stored = connection
                    .query_one("SELECT generation FROM record_generation", params![])?
                    .get::<i64>(0)?;
                assert_eq!(stored, generation);
                Ok(())
            })
        })
        .await
        .unwrap()
        .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires PNPR_TEST_MYSQL_URL"]
    async fn on_mysql_incompatible_initialization_marker_is_refused() {
        let db = Db::network("PNPR_TEST_MYSQL_URL").await;
        let adapter = db.adapter();
        tokio::task::spawn_blocking(move || {
            adapter.write(&mut |connection| {
                connection.execute(
                    &format!(
                        "CREATE TABLE {MARKER} (version BIGINT NOT NULL PRIMARY KEY) COMMENT='pnpr-init:0'",
                    ),
                    params![],
                )
            })
        })
        .await
        .unwrap()
        .unwrap();
        let metadata = Arc::new(SqlMetadata::with_adapter(db.instance_adapter(), "test".into()));
        let error = tokio::task::spawn_blocking(move || metadata.ensure_schema())
            .await
            .unwrap()
            .unwrap_err();
        assert!(matches!(error, RegistryError::InvalidConfig { .. }));
        let adapter = db.adapter();
        tokio::task::spawn_blocking(move || {
            adapter.read(ReadSource::Primary, &mut |connection| {
                let tables = connection.schema_table_names()?;
                assert_eq!(tables.len(), 1);
                Ok(())
            })
        })
        .await
        .unwrap()
        .unwrap();
    }
}

#[tokio::test]
async fn an_open_database_refuses_a_schema_advanced_by_another_server() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path().join("metadata.sqlite");
    let metadata = Arc::new(SqlMetadata::open_sqlite(&path).unwrap());
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer
        .execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, 1)",
            [SCHEMA_VERSION + 1],
        )
        .unwrap();
    metadata.schema_monitor.expire();
    let error = metadata
        .ensure_schema()
        .unwrap_err()
        .to_string();
    assert!(error.contains("Upgrade pnpr"), "{error}");
    writer
        .execute("DELETE FROM schema_migrations WHERE version = ?1", [SCHEMA_VERSION + 1])
        .unwrap();
    assert_eq!(
        metadata
            .check_schema()
            .await
            .unwrap_err()
            .to_string(),
        error,
    );
    assert_eq!(
        metadata
            .ensure_schema()
            .unwrap_err()
            .to_string(),
        error,
    );
}

#[tokio::test]
async fn fresh_migration_records_have_application_times() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path().join("metadata.sqlite");
    let before = crate::publish::now_millis();
    SqlMetadata::open_sqlite(&path).unwrap();
    let after = crate::publish::now_millis();
    let connection = rusqlite::Connection::open(&path).unwrap();
    let applied_at: i64 = connection
        .query_row("SELECT applied_at FROM schema_migrations WHERE version = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!((before..=after).contains(&applied_at), "{applied_at} not in {before}..={after}");
}

#[tokio::test]
async fn a_confirmed_malformed_schema_remains_refused_after_a_runtime_probe() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path().join("metadata.sqlite");
    let metadata = Arc::new(SqlMetadata::open_sqlite(&path).unwrap());
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer
        .execute("DELETE FROM schema_migrations WHERE version = 1", [])
        .unwrap();
    metadata.schema_monitor.expire();
    let error = metadata
        .ensure_schema()
        .unwrap_err()
        .to_string();
    assert!(error.contains("incompatible with this pnpr"), "{error}");
    assert!(matches!(metadata.ensure_schema(), Err(RegistryError::StorageUnavailable { .. })));
    writer
        .execute("INSERT INTO schema_migrations (version, applied_at) VALUES (1, 1)", [])
        .unwrap();
    assert_eq!(
        metadata
            .check_schema()
            .await
            .unwrap_err()
            .to_string(),
        error,
    );
}
