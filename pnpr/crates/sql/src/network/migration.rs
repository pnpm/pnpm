//! Schema migration of a network database pool.

use super::super::{adapter::Adapter, schema};
use pnpr_error::{RegistryError, Result};
use std::{sync::Arc, time::Duration};

/// Migrate a `PostgreSQL` pool to [`schema::SCHEMA_VERSION`].
#[cfg(feature = "backend-postgres")]
pub async fn ensure_postgres_schema(
    pool: Arc<sqlx::PgPool>,
    startup: crate::StartupConnection<sqlx::Postgres>,
    timeout: Duration,
    name: &str,
) -> Result<()> {
    let adapter = super::postgres::NetworkAdapter::from_pool(pool, startup, timeout);
    migrate(adapter, name, "PostgreSQL").await
}

/// Migrate a `MySQL` pool to [`schema::SCHEMA_VERSION`].
#[cfg(feature = "backend-mysql")]
pub async fn ensure_mysql_schema(
    pool: Arc<sqlx::MySqlPool>,
    startup: crate::StartupConnection<sqlx::MySql>,
    timeout: Duration,
    name: &str,
) -> Result<()> {
    let adapter = super::mysql::NetworkAdapter::from_pool(pool, startup, timeout);
    migrate(adapter, name, "MySQL").await
}

async fn migrate(
    adapter: impl Adapter + 'static,
    name: &str,
    database: &'static str,
) -> Result<()> {
    let name = name.to_string();
    tokio::task::spawn_blocking(move || {
        if let Some(stored) = schema::prepare_adapter_with_steps(&adapter, schema::STEPS)? {
            return Err(schema::refused_error(&name, &stored));
        }
        Ok(())
    })
    .await
    .map_err(|error| RegistryError::Internal {
        reason: format!("{database} schema migration task panicked: {error}"),
    })?
}
