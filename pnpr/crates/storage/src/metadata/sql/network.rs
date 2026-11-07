//! Network SQL connections used from the metadata blocking pool.

use super::adapter::{Adapter, Connection, Param, ReadSource, Row, Value};
use pnpr_config::SqlBackendSettings;
use pnpr_error::{RegistryError, Result};
use sqlx::{TypeInfo, ValueRef, pool::PoolConnection};
use std::{
    cell::{Cell, RefCell},
    time::Duration,
};

mod dialect;
use dialect::Dialect;

macro_rules! driver {
    ($module:ident, $database:ty, $pool:ty, $pool_fn:path, $row:ty) => {
        pub(super) mod $module {
            use super::{
                Adapter, Cell, Connection, Dialect, Duration, Param, PoolConnection, ReadSource,
                RefCell, RegistryError, Result, Row, SqlBackendSettings, TypeInfo, Value, ValueRef,
            };
            use sqlx::Row as _;
            #[derive(Debug)]
            pub(in super::super) struct NetworkAdapter {
                pool: std::sync::Arc<$pool>,
                dialect: Dialect,
                timeout: Duration,
            }

            impl NetworkAdapter {
                pub(in super::super) fn new(settings: &SqlBackendSettings) -> Result<Self> {
                    let dialect = Dialect::from_url(&settings.url)?;
                    let pool = $pool_fn(settings)?;
                    Ok(Self { pool, dialect, timeout: settings.timeout })
                }

                fn run(
                    &self,
                    operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
                ) -> Result<()> {
                    let runtime = tokio::runtime::Handle::current();
                    let connection = runtime.block_on(self.pool.acquire())?;
                    let connection = NetworkConnection {
                        connection: RefCell::new(connection),
                        dialect: self.dialect,
                        runtime,
                        timeout: self.timeout,
                        schema_locked: Cell::new(false),
                    };
                    let result = operation(&connection);
                    // A failed commit or rollback must not return an open transaction to the pool.
                    let rollback = connection.execute_batch("ROLLBACK");
                    let unlock = if connection.schema_locked.get() {
                        connection.execute_batch(connection.dialect.unlock_schema())
                    } else {
                        Ok(())
                    };
                    if rollback.is_err() || unlock.is_err() {
                        connection.connection.borrow_mut().close_on_drop();
                    }
                    result
                }
            }

            impl Adapter for NetworkAdapter {
                fn write(
                    &self,
                    operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
                ) -> Result<()> {
                    self.run(operation)
                }

                fn read(
                    &self,
                    _source: ReadSource,
                    operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
                ) -> Result<()> {
                    self.run(operation)
                }

                fn is_retryable(&self, error: &RegistryError) -> bool {
                    let RegistryError::Sqlx(sqlx::Error::Database(error)) = error else {
                        return false;
                    };
                    Dialect::retryable(&**error)
                }
            }

            struct NetworkConnection {
                connection: RefCell<PoolConnection<$database>>,
                dialect: Dialect,
                runtime: tokio::runtime::Handle,
                timeout: Duration,
                schema_locked: Cell<bool>,
            }

            impl Drop for NetworkConnection {
                fn drop(&mut self) {
                    if std::thread::panicking() {
                        self.connection.get_mut().close_on_drop();
                    }
                }
            }

            impl NetworkConnection {
                fn rows(&self, sql: &str, params: &[Param<'_>]) -> Result<Vec<Row>> {
                    let (sql, params) = self.dialect.statement(sql, params)?;
                    let mut query = sqlx::query(&sql);
                    for param in params {
                        query = match param {
                            Param::Null => query.bind(Option::<String>::None),
                            Param::NullInteger => query.bind(Option::<i64>::None),
                            Param::NullBlob => query.bind(Option::<Vec<u8>>::None),
                            Param::Integer(value) => query.bind(value),
                            Param::Text(value) => query.bind(value),
                            Param::Blob(value) => query.bind(value),
                        };
                    }
                    let mut connection = self.connection.borrow_mut();
                    let rows = self.runtime
                        .block_on(async {
                            tokio::time::timeout(self.timeout, query.fetch_all(&mut **connection))
                                .await
                        })
                        .map_err(|_| RegistryError::Internal {
                            reason: "metadata database operation timed out".to_string(),
                        })??;
                    rows.iter()
                        .map(decode_row)
                        .collect()
                }
            }

            impl Connection for NetworkConnection {
                fn lock_schema(&self) -> Result<()> {
                    // A lost response may still have acquired the session lock.
                    self.schema_locked.set(true);
                    let mut row = self.query_one(self.dialect.lock_schema(), &[])?;
                    if row.get::<i64>(0)? != 1 {
                        return Err(RegistryError::Internal {
                            reason: "metadata schema lock timed out".to_string(),
                        });
                    }
                    Ok(())
                }

                fn execute(&self, sql: &str, params: &[Param<'_>]) -> Result<()> {
                    self.rows(sql, params)
                        .map(|_| ())
                }

                fn query(&self, sql: &str, params: &[Param<'_>]) -> Result<Vec<Row>> {
                    self.rows(sql, params)
                }

                fn execute_batch(&self, sql: &str) -> Result<()> {
                    let mut connection = self.connection.borrow_mut();
                    self.runtime
                        .block_on(async {
                            tokio::time::timeout(
                                self.timeout,
                                sqlx::raw_sql(sql).execute(&mut **connection),
                            )
                            .await
                        })
                        .map_err(|_| RegistryError::Internal {
                            reason: "metadata database operation timed out".to_string(),
                        })??;
                    Ok(())
                }

                fn begin_write(&self) -> Result<()> {
                    self.execute_batch(self.dialect.begin_write())
                }

                fn begin_read(&self) -> Result<()> {
                    self.execute_batch(self.dialect.begin_read())
                }

                fn schema_tables(&self) -> String {
                    self.dialect.tables()
                }

                fn schema_table_names(&self) -> Result<Vec<Row>> {
                    self.query(self.dialect.table_names(), &[])
                }

                fn schema_marker(&self) -> &'static str {
                    "CREATE TABLE schema_version (version TEXT NOT NULL)"
                }
            }

            fn decode_row(row: &$row) -> Result<Row> {
                let mut values = Vec::with_capacity(row.len());
                for index in 0..row.len() {
                    let raw = row.try_get_raw(index)?;
                    let value = if raw.is_null() {
                        Value::Null
                    } else {
                        match raw.type_info().name() {
                            "INT2" | "SMALLINT" => {
                                Value::Integer(i64::from(row.try_get::<i16, _>(index)?))
                            }
                            "INT4" | "INT" => {
                                Value::Integer(i64::from(row.try_get::<i32, _>(index)?))
                            }
                            "INT8" | "BIGINT" => Value::Integer(row.try_get(index)?),
                            "BOOL" | "BOOLEAN" => {
                                Value::Integer(i64::from(row.try_get::<bool, _>(index)?))
                            }
                            "BYTEA" | "BLOB" | "LONGBLOB" => Value::Blob(row.try_get(index)?),
                            "VARBINARY" | "BINARY" => Value::Text(
                                String::from_utf8(row.try_get(index)?)
                                    .map_err(|_| RegistryError::Internal {
                                        reason: "metadata text is not UTF-8".to_string(),
                                    })?,
                            ),
                            _ => Value::Text(row.try_get(index)?),
                        }
                    };
                    values.push(value);
                }
                Ok(Row::new(values))
            }
        }
    };
}
#[cfg(feature = "backend-postgres")]
driver!(postgres, sqlx::Postgres, sqlx::PgPool, pnpr_sql::postgres_pool, sqlx::postgres::PgRow);
#[cfg(feature = "backend-mysql")]
driver!(mysql, sqlx::MySql, sqlx::MySqlPool, pnpr_sql::mysql_pool, sqlx::mysql::MySqlRow);

pub(super) fn adapter(settings: &SqlBackendSettings) -> Result<Box<dyn Adapter>> {
    match Dialect::from_url(&settings.url)? {
        #[cfg(feature = "backend-postgres")]
        Dialect::Postgres => Ok(Box::new(postgres::NetworkAdapter::new(settings)?)),
        #[cfg(feature = "backend-mysql")]
        Dialect::Mysql => Ok(Box::new(mysql::NetworkAdapter::new(settings)?)),
        #[cfg_attr(
            all(feature = "backend-postgres", feature = "backend-mysql"),
            expect(unreachable_patterns, reason = "Both enabled drivers cover every dialect.")
        )]
        _ => Err(RegistryError::InvalidConfig {
            reason: "this build does not include the configured database driver".to_string(),
        }),
    }
}
