//! pnpr instances on different schemas of one `PostgreSQL` database.

use super::{Adapter as Backend, Db};
use crate::database::sql::adapter::{Adapter, WriteTransaction};
use pnpr_config::SqlBackendSettings;
use pnpr_error::Result;
use std::time::Duration;

mod on_postgres {
    use super::{Backend, Db, on_schema, write_while_holding};

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires PNPR_TEST_POSTGRES_URL"]
    async fn writers_on_other_schemas_do_not_wait_for_each_other() {
        let db = Db::network("PNPR_TEST_POSTGRES_URL").await;
        let Backend::Network(settings) = &db.adapter else { unreachable!() };
        let pool = sqlx::PgPool::connect(&settings.url).await.unwrap();
        for schema in ["first", "second"] {
            sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
                .execute(&pool)
                .await
                .unwrap();
        }
        pool.close().await;
        let first = super::super::super::network::adapter(&on_schema(settings, "first")).unwrap();
        let second = super::super::super::network::adapter(&on_schema(settings, "second")).unwrap();
        tokio::task::spawn_blocking(move || write_while_holding(&*first, &*second))
            .await
            .unwrap()
            .unwrap();
    }
}

/// Commit a write through `second` while a write through `first` holds its
/// write lock.
fn write_while_holding(first: &dyn Adapter, second: &dyn Adapter) -> Result<()> {
    first.write(&mut |connection| {
        let held = WriteTransaction::begin(connection)?;
        second.write(&mut |other| WriteTransaction::begin(other)?.commit())?;
        held.commit()
    })
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
