//! Connection pools shared by accounts, tokens, and package metadata.
#![cfg(any(feature = "backend-postgres", feature = "backend-mysql"))]

use pnpr_config::SqlBackendSettings;
use pnpr_error::{RegistryError, Result};
use std::{
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

#[cfg(feature = "backend-postgres")]
mod postgres;
#[cfg(feature = "backend-postgres")]
pub use postgres::{
    options as postgres_options, pool as postgres_pool, timeout_sql as postgres_timeout_sql,
};
#[cfg(feature = "backend-mysql")]
mod mysql;
#[cfg(feature = "backend-mysql")]
pub use mysql::{options as mysql_options, pool as mysql_pool, timeout_sql as mysql_timeout_sql};

fn invalid_pool_size() -> RegistryError {
    RegistryError::InvalidConfig {
        reason: "database.maxConnections must be greater than 0".to_string(),
    }
}

fn timeout_millis(timeout: Duration) -> u128 {
    timeout.as_millis().max(1)
}
#[cfg(feature = "backend-mysql")]
fn timeout_seconds(timeout: Duration) -> u64 {
    timeout.as_secs().max(1)
}

#[cfg(test)]
mod tests;

struct Pools<Database: sqlx::Database> {
    entries: Mutex<Vec<PoolEntry<Database>>>,
}

struct PoolEntry<Database: sqlx::Database> {
    runtime: tokio::runtime::Id,
    settings: SqlBackendSettings,
    pool: Weak<sqlx::Pool<Database>>,
}

impl<Database: sqlx::Database> Pools<Database> {
    const fn new() -> Self {
        Self { entries: Mutex::new(Vec::new()) }
    }

    fn get(
        &self,
        settings: &SqlBackendSettings,
        open: impl FnOnce() -> Result<sqlx::Pool<Database>>,
    ) -> Result<Arc<sqlx::Pool<Database>>> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| RegistryError::Internal {
                reason: "opening a database pool requires an async runtime".to_string(),
            })?
            .id();
        let mut entries = self.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        entries.retain(|entry| entry.pool.strong_count() > 0);
        if let Some(pool) = entries
            .iter()
            .find(|entry| entry.runtime == runtime && &entry.settings == settings)
            .and_then(|entry| entry.pool.upgrade())
        {
            return Ok(pool);
        }
        let pool = Arc::new(open()?);
        entries.push(PoolEntry {
            runtime,
            settings: settings.clone(),
            pool: Arc::downgrade(&pool),
        });
        Ok(pool)
    }
}
