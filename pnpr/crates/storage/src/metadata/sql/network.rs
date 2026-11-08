//! Network SQL connections used from the metadata blocking pool.

use super::adapter::{Adapter, Connection, Param, Query, ReadSource, Row, Value};
use pnpr_config::SqlBackendSettings;
use pnpr_error::{RegistryError, Result};
use sqlx::{TypeInfo, ValueRef, pool::PoolConnection};
use std::{
    cell::{Cell, RefCell},
    time::Duration,
};

mod dialect;
mod mysql_schema;
use dialect::Dialect;

macro_rules! driver {
    ($module:ident, $database:ty, $pool:ty, $pool_fn:path, $row:ty) => {
        pub(super) mod $module {
            use super::{
                Adapter, Cell, Connection, Dialect, Duration, Param, PoolConnection, Query,
                ReadSource, RefCell, RegistryError, Result, Row, SqlBackendSettings, TypeInfo,
                Value, ValueRef,
            };
            use sqlx::Row as _;
            #[derive(Debug)]
            pub(in super::super) struct NetworkAdapter {
                pool: std::sync::Arc<$pool>,
                dialect: Dialect,
                settings: SqlBackendSettings,
            }

            impl NetworkAdapter {
                pub(in super::super) fn new(settings: &SqlBackendSettings) -> Result<Self> {
                    let dialect = Dialect::from_url(&settings.url)?;
                    let pool = $pool_fn(settings)?;
                    Ok(Self { pool, dialect, settings: settings.clone() })
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
                        timeout: self.settings.timeout,
                        schema_locked: Cell::new(false),
                        transaction_open: Cell::new(false),
                    };
                    let result = operation(&connection);
                    // A failed commit or rollback must not return an open transaction to the pool.
                    let rollback = if connection.transaction_open.get() {
                        connection.rollback()
                    } else {
                        Ok(())
                    };
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
                fn initialize(
                    &self,
                    operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
                ) -> Result<()> {
                    // Keep startup deadlines off the shared request connections.
                    let mut settings = self.settings.clone();
                    settings.timeout = settings.startup_timeout;
                    settings.max_connections = Some(1);
                    Self::new(&settings)?.run(operation)
                }

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
                transaction_open: Cell<bool>,
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
                    self.fetch(&sql, params)?
                        .iter()
                        .map(|row| decode_row(row, 0))
                        .collect()
                }

                fn fetch(&self, sql: &str, params: Vec<Param<'_>>) -> Result<Vec<$row>> {
                    let mut query = sqlx::query(sql);
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
                    Ok(rows)
                }
            }

            impl Connection for NetworkConnection {
                fn recover_schema_initialization(&self) -> Result<bool> {
                    if matches!(self.dialect, Dialect::Mysql) {
                        return super::mysql_schema::recover(self);
                    }
                    Ok(false)
                }

                fn create_schema(&self) -> Result<()> {
                    if matches!(self.dialect, Dialect::Mysql) {
                        super::mysql_schema::begin(self)?;
                        super::mysql_schema::finish(self)
                    } else {
                        super::super::schema::create(self)
                    }
                }

                fn lock_schema(&self) -> Result<()> {
                    // A lost response may still have acquired the session lock.
                    self.schema_locked.set(true);
                    let mut row = self.query_one(&self.dialect.lock_schema(self.timeout), &[])?;
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

                fn query_all(&self, queries: &[Query<'_>]) -> Result<Vec<Vec<Row>>> {
                    let Some(first) = queries.first() else { return Ok(Vec::new()) };
                    if queries.len() == 1
                        || queries
                            .iter()
                            .any(|query| query.sql != first.sql)
                    {
                        return queries
                            .iter()
                            .map(|query| self.rows(query.sql, &query.params))
                            .collect();
                    }
                    let (sql, params) = self.dialect.batch(queries)?;
                    let mut results = vec![Vec::new(); queries.len()];
                    for row in self.fetch(&sql, params)? {
                        let index: i64 = row.try_get(0)?;
                        results[usize::try_from(index).expect("batch index")].push(decode_row(
                            &row, 1,
                        )?);
                    }
                    Ok(results)
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
                    self.transaction_open.set(true);
                    self.execute_batch(self.dialect.begin_write())
                }

                fn begin_read(&self) -> Result<()> {
                    self.transaction_open.set(true);
                    self.execute_batch(self.dialect.begin_read())
                }

                fn commit(&self) -> Result<()> {
                    self.execute_batch("COMMIT")?;
                    self.transaction_open.set(false);
                    Ok(())
                }

                fn rollback(&self) -> Result<()> {
                    self.execute_batch("ROLLBACK")?;
                    self.transaction_open.set(false);
                    Ok(())
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

            fn decode_row(row: &$row, start: usize) -> Result<Row> {
                let mut values = Vec::with_capacity(row.len());
                for index in start..row.len() {
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
