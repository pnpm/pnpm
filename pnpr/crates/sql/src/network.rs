//! Network SQL connections used from the blocking pool.

use super::{
    adapter::{Adapter, Connection, Param, Query, ReadSource, Row, Value},
    schema,
};
use pnpr_config::SqlBackendSettings;
use pnpr_error::{RegistryError, Result};
use sqlx::{TypeInfo, ValueRef, pool::PoolConnection};
use std::{
    cell::{Cell, RefCell},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

mod dialect;
mod mysql_schema;
use dialect::Dialect;

macro_rules! driver {
    ($module:ident, $database:ty, $pool:ty, $pool_fn:path, $row:ty, $dialect:expr, $timeout_sql:path) => {
        pub(super) mod $module {
            use super::{
                Adapter, Arc, Cell, Connection, Dialect, Duration, Mutex, Param, PoisonError,
                PoolConnection, Query, ReadSource, RefCell, RegistryError, Result, Row,
                SqlBackendSettings, TypeInfo, Value, ValueRef,
            };
            use sqlx::Row as _;

            #[derive(Debug)]
            pub(in super::super) struct NetworkAdapter {
                pool: Arc<$pool>,
                dialect: Dialect,
                settings: SqlBackendSettings,
                /// Held by a write of this instance, so one connection at a
                /// time waits for the database's write lock and the others
                /// stay free for reads.
                writer: Mutex<()>,
            }

            impl NetworkAdapter {
                pub(in super::super) fn new(settings: &SqlBackendSettings) -> Result<Self> {
                    let dialect = Dialect::from_url(&settings.url)?;
                    let pool = $pool_fn(settings)?;
                    Ok(Self { pool, dialect, settings: settings.clone(), writer: Mutex::new(()) })
                }

                pub(super) fn from_pool(pool: Arc<$pool>, timeout: Duration) -> Self {
                    Self {
                        pool,
                        dialect: $dialect,
                        settings: SqlBackendSettings {
                            url: String::new(),
                            max_connections: None,
                            timeout,
                            startup_timeout: timeout,
                        },
                        writer: Mutex::new(()),
                    }
                }

                fn run(
                    &self,
                    timeout: Duration,
                    operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
                ) -> Result<()> {
                    let runtime = tokio::runtime::Handle::current();
                    let connection = runtime.block_on(crate::acquire(&self.pool, timeout))?;
                    let connection = NetworkConnection {
                        connection: RefCell::new(connection),
                        dialect: self.dialect,
                        runtime,
                        timeout,
                        schema_locked: Cell::new(false),
                        writes_locked: Cell::new(false),
                        transaction_open: Cell::new(false),
                    };
                    if timeout != self.settings.timeout {
                        // A startup deadline must never reach another pool borrower.
                        connection.connection.borrow_mut().close_on_drop();
                        connection.execute_batch(&$timeout_sql(timeout))?;
                    }
                    let result = operation(&connection);
                    // A failed commit or rollback must not return an open transaction to the pool.
                    let rollback = if connection.transaction_open.get() {
                        connection.rollback()
                    } else {
                        Ok(())
                    };
                    let unlock_writes = if connection.writes_locked.get() {
                        connection.execute_batch(connection.dialect.unlock_writes())
                    } else {
                        Ok(())
                    };
                    let unlock = if connection.schema_locked.get() {
                        connection.execute_batch(connection.dialect.unlock_schema())
                    } else {
                        Ok(())
                    };
                    if rollback.is_err() || unlock_writes.is_err() || unlock.is_err() {
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
                    self.run(self.settings.startup_timeout, operation)
                }

                fn write(
                    &self,
                    operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
                ) -> Result<()> {
                    let _writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
                    self.run(self.settings.timeout, operation)
                }

                fn read(
                    &self,
                    _source: ReadSource,
                    operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
                ) -> Result<()> {
                    self.run(self.settings.timeout, operation)
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
                /// Whether the session holds [`Dialect::lock_writes`].
                writes_locked: Cell<bool>,
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
                        .map_err(|_| self.mark_timed_out(&mut connection))??;
                    Ok(rows)
                }

                fn mark_timed_out(
                    &self,
                    connection: &mut PoolConnection<$database>,
                ) -> RegistryError {
                    // A cancelled future leaves an unread response on the wire.
                    connection.close_on_drop();
                    self.transaction_open.set(false);
                    self.schema_locked.set(false);
                    self.writes_locked.set(false);
                    RegistryError::Internal {
                        reason: "metadata database operation timed out".to_string(),
                    }
                }

                /// Take the session lock `sql` selects `1` for once held.
                /// `lock` names it in the error when its wait timed out.
                fn take_session_lock(&self, sql: &str, lock: &str) -> Result<()> {
                    let mut row = self.query_one(sql, &[]).inspect_err(|_| {
                        // A lost response may still have acquired the session lock.
                        self.connection.borrow_mut().close_on_drop();
                    })?;
                    if row.get::<i64>(0)? != 1 {
                        return Err(RegistryError::Internal {
                            reason: format!("metadata {lock} lock timed out"),
                        });
                    }
                    Ok(())
                }

                /// End the transaction with `end`, or with `end_write` when
                /// the session holds the write lock, which it releases.
                fn end_transaction(&self, end: &str, end_write: &str) -> Result<()> {
                    let writes_locked = self.writes_locked.get();
                    self.execute_batch(if writes_locked { end_write } else { end })?;
                    self.transaction_open.set(false);
                    self.writes_locked.set(false);
                    Ok(())
                }
            }

            impl Connection for NetworkConnection {
                fn transactional_ddl(&self) -> bool {
                    !matches!(self.dialect, Dialect::Mysql)
                }

                fn is_recoverable_empty_migrations(&self) -> Result<bool> {
                    if matches!(self.dialect, Dialect::Mysql) {
                        return super::mysql_schema::is_recoverable(self);
                    }
                    Ok(false)
                }

                fn lock_schema(&self) -> Result<()> {
                    self.take_session_lock(&self.dialect.lock_schema(self.timeout), "schema")?;
                    self.schema_locked.set(true);
                    Ok(())
                }

                fn execute(&self, sql: &str, params: &[Param<'_>]) -> Result<()> {
                    self.rows(sql, params)
                        .map(|_| ())
                }

                fn execute_ddl(&self, sql: &str) -> Result<()> {
                    if matches!(self.dialect, Dialect::Mysql) {
                        return super::mysql_schema::execute_ddl(self, sql);
                    }
                    self.execute(sql, &[])
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
                        .map_err(|_| self.mark_timed_out(&mut connection))??;
                    Ok(())
                }

                fn begin_write(&self) -> Result<()> {
                    if !self.writes_locked.get() {
                        self.take_session_lock(&self.dialect.lock_writes(self.timeout), "write")?;
                        self.writes_locked.set(true);
                    }
                    self.transaction_open.set(true);
                    self.execute_batch(self.dialect.begin_write())
                }

                fn begin_read(&self) -> Result<()> {
                    self.transaction_open.set(true);
                    self.execute_batch(self.dialect.begin_read())
                }

                fn commit(&self) -> Result<()> {
                    self.end_transaction("COMMIT", self.dialect.commit_write())
                }

                fn rollback(&self) -> Result<()> {
                    self.end_transaction("ROLLBACK", self.dialect.rollback_write())
                }

                fn schema_tables(&self) -> Option<String> {
                    Some(self.dialect.tables())
                }

                fn schema_table_names(&self) -> Result<Vec<Row>> {
                    self.query(self.dialect.table_names(), &[])
                }

                fn schema_marker(&self) -> Option<&'static str> {
                    Some(match self.dialect {
                        Dialect::Postgres => {
                            "CREATE TABLE IF NOT EXISTS schema_migrations (version BIGINT NOT NULL PRIMARY KEY)"
                        }
                        Dialect::Mysql => super::mysql_schema::MARKER_TABLE,
                    })
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
driver!(
    postgres,
    sqlx::Postgres,
    sqlx::PgPool,
    crate::postgres_pool,
    sqlx::postgres::PgRow,
    Dialect::Postgres,
    crate::postgres_timeout_sql
);
#[cfg(feature = "backend-mysql")]
driver!(
    mysql,
    sqlx::MySql,
    sqlx::MySqlPool,
    crate::mysql_pool,
    sqlx::mysql::MySqlRow,
    Dialect::Mysql,
    crate::mysql_timeout_sql
);

/// Open a network [`Adapter`] for the database URL in `settings`.
pub fn adapter(settings: &SqlBackendSettings) -> Result<Box<dyn Adapter>> {
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

/// Migrate a `PostgreSQL` pool to [`schema::SCHEMA_VERSION`].
#[cfg(feature = "backend-postgres")]
pub async fn ensure_postgres_schema(
    pool: Arc<sqlx::PgPool>,
    timeout: Duration,
    name: &str,
) -> Result<()> {
    let adapter = postgres::NetworkAdapter::from_pool(pool, timeout);
    let name = name.to_string();
    tokio::task::spawn_blocking(move || {
        if let Some(stored) = schema::prepare_adapter_with_steps(&adapter, schema::STEPS)? {
            return Err(schema::refused_error(&name, &stored));
        }
        Ok(())
    })
    .await
    .map_err(|error| RegistryError::Internal {
        reason: format!("PostgreSQL schema migration task panicked: {error}"),
    })?
}

/// Migrate a `MySQL` pool to [`schema::SCHEMA_VERSION`].
#[cfg(feature = "backend-mysql")]
pub async fn ensure_mysql_schema(
    pool: Arc<sqlx::MySqlPool>,
    timeout: Duration,
    name: &str,
) -> Result<()> {
    let adapter = mysql::NetworkAdapter::from_pool(pool, timeout);
    let name = name.to_string();
    tokio::task::spawn_blocking(move || {
        if let Some(stored) = schema::prepare_adapter_with_steps(&adapter, schema::STEPS)? {
            return Err(schema::refused_error(&name, &stored));
        }
        Ok(())
    })
    .await
    .map_err(|error| RegistryError::Internal {
        reason: format!("MySQL schema migration task panicked: {error}"),
    })?
}
