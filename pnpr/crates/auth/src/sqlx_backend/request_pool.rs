use sqlx::{Database, Pool, Transaction, pool::PoolConnection};
use std::{sync::Arc, time::Duration};

/// Bound checkout before a write starts, without canceling an in-flight write.
#[derive(Debug)]
pub(super) struct RequestPool<Db: Database> {
    pool: Arc<Pool<Db>>,
    timeout: Duration,
}

impl<Db: Database> RequestPool<Db> {
    pub(super) fn new(pool: Arc<Pool<Db>>, timeout: Duration) -> Self {
        Self { pool, timeout }
    }

    pub(super) async fn acquire(&self) -> Result<PoolConnection<Db>, sqlx::Error> {
        pnpr_sql::acquire(&self.pool, self.timeout).await
    }

    pub(super) async fn begin(&self) -> Result<Transaction<'static, Db>, sqlx::Error> {
        Transaction::begin(self.acquire().await?, None).await
    }
}
