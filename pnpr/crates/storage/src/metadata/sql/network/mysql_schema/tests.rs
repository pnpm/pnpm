use super::{SCHEMA_VERSION, begin, statements};
use crate::metadata::sql::{
    SqlMetadata,
    adapter::{Connection, ReadSource},
    tests::Db,
};
use pnpr_error::{RegistryError, Result};
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires PNPR_TEST_MYSQL_URL"]
async fn on_mysql_interrupted_schema_creation_resumes() {
    let schema: Vec<_> = statements(include_str!("../mysql.sql")).collect();
    let first_index = schema
        .iter()
        .position(|sql| sql.starts_with("CREATE INDEX"))
        .unwrap();
    for completed in [0, first_index + 1, schema.len()] {
        let db = Db::network("PNPR_TEST_MYSQL_URL").await;
        let mut partial = schema[..completed].to_vec();
        let all_created = completed == schema.len();
        if all_created {
            partial.push("UPDATE record_generation SET generation = 7".to_string());
        }
        interrupt(&db, partial).await;
        let metadata = Arc::new(SqlMetadata::with_adapter(db.instance_adapter(), "test".into()));
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
    begin(connection)?;
    for statement in partial {
        connection.execute(statement, params![])?;
    }
    Ok(())
}

async fn assert_initialized(db: &Db, generation: i64) {
    let adapter = db.adapter();
    tokio::task::spawn_blocking(move || {
        adapter.read(ReadSource::Primary, &mut |connection| {
            let version = connection
                .query_one("SELECT version FROM schema_version", params![])?
                .get::<String>(0)?;
            assert_eq!(version, SCHEMA_VERSION);
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
                "CREATE TABLE schema_version (version TEXT NOT NULL) COMMENT='pnpr-init:0.2'",
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
    assert!(matches!(error, RegistryError::Internal { .. }));
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
