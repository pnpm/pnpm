//! pnpr instances on different schemas of one `PostgreSQL` database.

use super::{Adapter, Db};
use crate::metadata::sql::adapter::WriteTransaction;
use pnpr_config::SqlBackendSettings;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires PNPR_TEST_POSTGRES_URL"]
async fn on_postgres_writers_on_other_schemas_do_not_wait_for_each_other() {
    let db = Db::network("PNPR_TEST_POSTGRES_URL").await;
    let Adapter::Network(settings) = &db.adapter else { unreachable!() };
    let pool = sqlx::PgPool::connect(&settings.url).await.unwrap();
    for schema in ["first", "second"] {
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&pool)
            .await
            .unwrap();
    }
    pool.close().await;
    let first = super::super::network::adapter(&on_schema(settings, "first")).unwrap();
    let second = super::super::network::adapter(&on_schema(settings, "second")).unwrap();
    tokio::task::spawn_blocking(move || {
        first.write(&mut |connection| {
            let held = WriteTransaction::begin(connection)?;
            second.write(&mut |other| WriteTransaction::begin(other)?.commit())?;
            held.commit()
        })
    })
    .await
    .unwrap()
    .unwrap();
}

/// `settings` with `schema` as the session's schema, and a timeout short
/// enough that waiting for another schema's lock fails the test.
fn on_schema(settings: &SqlBackendSettings, schema: &str) -> SqlBackendSettings {
    SqlBackendSettings {
        url: format!("{}?options=-c%20search_path%3D{schema}", settings.url),
        timeout: Duration::from_secs(2),
        ..settings.clone()
    }
}
