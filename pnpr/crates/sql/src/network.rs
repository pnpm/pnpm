//! Network SQL connections used from the blocking pool.

use super::adapter::{Adapter, Connection, Param, Query, ReadSource, Row, Value};
use crate::{Checkout, StartupConnection};
#[cfg(feature = "backend-mysql")]
pub use migration::ensure_mysql_schema;
#[cfg(feature = "backend-postgres")]
pub use migration::ensure_postgres_schema;
use pnpr_config::SqlBackendSettings;
use pnpr_error::{RegistryError, Result};
use sqlx::{TypeInfo, ValueRef};
use std::{
    cell::{Cell, RefCell},
    sync::Arc,
    time::Duration,
};

mod dialect;
#[macro_use]
mod driver;
#[cfg(any(feature = "backend-postgres", feature = "backend-mysql"))]
mod migration;
mod mysql_schema;
use dialect::NetworkDialect;

#[cfg(feature = "backend-postgres")]
driver!(
    postgres,
    sqlx::Postgres,
    sqlx::PgPool,
    crate::postgres_pool,
    sqlx::postgres::PgRow,
    NetworkDialect::Postgres,
    crate::postgres_timeout_sql,
    crate::postgres::configure
);
#[cfg(feature = "backend-mysql")]
driver!(
    mysql,
    sqlx::MySql,
    sqlx::MySqlPool,
    crate::mysql_pool,
    sqlx::mysql::MySqlRow,
    NetworkDialect::Mysql,
    crate::mysql_timeout_sql,
    crate::mysql::configure
);

/// Open a network [`Adapter`] for the database URL in `settings`.
pub fn adapter(settings: &SqlBackendSettings) -> Result<Box<dyn Adapter>> {
    match NetworkDialect::from_url(&settings.url)? {
        #[cfg(feature = "backend-postgres")]
        NetworkDialect::Postgres => Ok(Box::new(postgres::NetworkAdapter::new(settings)?)),
        #[cfg(feature = "backend-mysql")]
        NetworkDialect::Mysql => Ok(Box::new(mysql::NetworkAdapter::new(settings)?)),
        #[cfg_attr(
            all(feature = "backend-postgres", feature = "backend-mysql"),
            expect(unreachable_patterns, reason = "Both enabled drivers cover every dialect.")
        )]
        _ => Err(RegistryError::InvalidConfig {
            reason: "this build does not include the configured database driver".to_string(),
        }),
    }
}
