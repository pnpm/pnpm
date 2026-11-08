use super::{Adapter, Db};
use pnpr_error::Result;

impl Drop for Db {
    fn drop(&mut self) {
        let Adapter::Network(settings) = &self.adapter else { return };
        let (server, database) = settings.url.rsplit_once('/').unwrap();
        let runtime = tokio::runtime::Handle::current();
        let result =
            tokio::task::block_in_place(|| runtime.block_on(drop_database(server, database)));
        if let Err(error) = result {
            if std::thread::panicking() {
                eprintln!("failed to clean up test database: {error}");
            } else {
                panic!("failed to clean up test database: {error}");
            }
        }
    }
}

async fn drop_database(server: &str, database: &str) -> Result<()> {
    #[cfg(feature = "backend-postgres")]
    if server.starts_with("postgres:") || server.starts_with("postgresql:") {
        let pool = sqlx::PgPool::connect(&format!("{server}/postgres")).await?;
        sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)")).execute(&pool).await?;
        pool.close().await;
    }
    #[cfg(feature = "backend-mysql")]
    if server.starts_with("mysql:") {
        let pool = sqlx::MySqlPool::connect(&format!("{server}/mysql")).await?;
        sqlx::query(&format!("DROP DATABASE {database}")).execute(&pool).await?;
        pool.close().await;
    }
    Ok(())
}
