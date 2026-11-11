use sqlx::{Database, Pool, Transaction};
use std::{fmt, sync::Arc, time::Duration};

/// Bound checkout before a write starts, without canceling an in-flight write.
pub(super) struct RequestPool<Db: Database> {
    pool: Arc<Pool<Db>>,
    timeout: Duration,
    startup: Option<pnpr_sql::StartupConnection<Db>>,
}

impl<Db: Database> fmt::Debug for RequestPool<Db> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestPool")
            .field("pool", &self.pool)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl<Db: Database> RequestPool<Db> {
    pub(super) fn new(pool: Arc<Pool<Db>>, timeout: Duration) -> Self {
        Self { pool, timeout, startup: None }
    }

    /// Initialization queries share this connection; transactions use the pool.
    pub(super) fn with_startup(
        pool: Arc<Pool<Db>>,
        startup: pnpr_sql::StartupConnection<Db>,
        timeout: Duration,
    ) -> Self {
        Self { pool, timeout, startup: Some(startup) }
    }

    pub(super) async fn acquire(&self) -> pnpr_error::Result<pnpr_sql::Checkout<Db>> {
        if let Some(startup) = &self.startup {
            return pnpr_sql::Checkout::startup(Arc::clone(startup), self.timeout).await;
        }
        Ok(pnpr_sql::Checkout::Pooled(pnpr_sql::acquire(&self.pool, self.timeout).await?))
    }

    pub(super) async fn begin(&self) -> Result<Transaction<'static, Db>, sqlx::Error> {
        Transaction::begin(pnpr_sql::acquire(&self.pool, self.timeout).await?, None).await
    }
}
