macro_rules! driver {
    ($module:ident, $database:ty, $pool:ty, $pool_fn:path, $row:ty, $dialect:expr, $timeout_sql:path, $configure:path) => {
        pub(super) mod $module {
            use super::{
                Adapter, Arc, Cell, Connection, Dialect, Duration, Param, Checkout, StartupConnection, Query,
                ReadSource, RefCell, RegistryError, Result, Row, SqlBackendSettings, TypeInfo,
                Value, ValueRef,
            };
            use tokio::sync::Mutex;
            use sqlx::Row as _;

            #[derive(Debug)]
            pub(in super::super) struct NetworkAdapter {
                pool: Arc<$pool>,
                startup: Option<StartupConnection<$database>>,
                dialect: Dialect,
                settings: SqlBackendSettings,
                /// Held by a write of this instance, so one connection at a
                /// time waits for the database's write lock and the others
                /// stay free for reads. A write waits for it as long as for a
                /// pool connection.
                writer: Mutex<()>,
            }

            impl NetworkAdapter {
                pub(in super::super) fn new(settings: &SqlBackendSettings) -> Result<Self> {
                    let dialect = Dialect::from_url(&settings.url)?;
                    let pool = $pool_fn(settings)?;
                    Ok(Self { pool, startup: None, dialect, settings: settings.clone(), writer: Mutex::new(()) })
                }

                pub(super) fn from_pool(pool: Arc<$pool>, startup: StartupConnection<$database>, timeout: Duration) -> Self {
                    Self {
                        pool,
                        startup: Some(startup),
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
                    let connection = tokio::runtime::Handle::current()
                        .block_on(crate::acquire(&self.pool, timeout))?;
                    self.run_connection(Checkout::Pooled(connection), timeout, operation)
                }

                fn run_connection(
                    &self,
                    connection: Checkout<$database>,
                    timeout: Duration,
                    operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
                ) -> Result<()> {
                    let runtime = tokio::runtime::Handle::current();
                    let startup = connection.is_startup();
                    let connection = NetworkConnection {
                        connection: RefCell::new(connection),
                        dialect: self.dialect,
                        runtime,
                        timeout,
                        schema_locked: Cell::new(false),
                        writes_locked: Cell::new(false),
                        transaction_open: Cell::new(false),
                    };
                    if startup {
                        let mut conn = connection.connection.borrow_mut();
                        connection.runtime.block_on(tokio::time::timeout(timeout, $configure(&mut **conn, timeout)))
                            .map_err(|_| sqlx::Error::PoolTimedOut)??;
                    } else if timeout != self.settings.timeout {
                        // A startup deadline must never reach another pool borrower.
                        connection.connection.borrow_mut().close_on_drop();
                        connection.execute_batch(&$timeout_sql(timeout))?;
                    }
                    let result = operation(&connection);
                    connection.finish(result)
                }
            }

            impl Adapter for NetworkAdapter {
                fn initialize(
                    &self,
                    operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
                ) -> Result<()> {
                    let started = std::time::Instant::now();
                    let timeout = self.settings.startup_timeout;
                    let runtime = tokio::runtime::Handle::current();
                    let connection = runtime.block_on(async {
                        if let Some(startup) = &self.startup {
                            return Checkout::startup(Arc::clone(startup), timeout).await;
                        }
                        if !self.settings.url.is_empty() && self.pool.size() == 0 && !self.pool.is_closed() {
                            let connection = crate::connect_startup(&self.pool, &self.settings.url, timeout).await?;
                            return Checkout::startup(Arc::new(Mutex::new(Some(connection))), timeout).await;
                        }
                        Ok(Checkout::Pooled(crate::acquire(&self.pool, timeout).await?))
                    })?;
                    self.run_connection(connection, timeout.saturating_sub(started.elapsed()), operation)
                }

                fn write(
                    &self,
                    operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
                ) -> Result<()> {
                    let timeout = self.settings.timeout;
                    let _writer = tokio::runtime::Handle::current()
                        .block_on(tokio::time::timeout(timeout, self.writer.lock()))
                        .map_err(|_| sqlx::Error::PoolTimedOut)?;
                    self.run(timeout, operation)
                }

                fn read(
                    &self,
                    _source: ReadSource,
                    operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
                ) -> Result<crate::adapter::Snapshot> {
                    self.run(self.settings.timeout, operation)
                        .map(|()| crate::adapter::Snapshot::Current)
                }

                fn is_retryable(&self, error: &RegistryError) -> bool {
                    let RegistryError::Sqlx(sqlx::Error::Database(error)) = error else {
                        return false;
                    };
                    Dialect::retryable(&**error)
                }

                fn is_unavailable(&self, error: &RegistryError) -> bool {
                    if self.is_retryable(error) {
                        return true;
                    }
                    match error {
                        RegistryError::StorageUnavailable { .. }
                        | RegistryError::Sqlx(sqlx::Error::Io(_) | sqlx::Error::PoolTimedOut) => true,
                        RegistryError::Sqlx(sqlx::Error::Database(error)) => error.is_transient_in_connect_phase(),
                        _ => false,
                    }
                }

            }

            struct NetworkConnection {
                connection: RefCell<Checkout<$database>>,
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
                fn finish(&self, result: Result<()>) -> Result<()> {
                    // A failed commit or rollback must not return an open transaction to the pool.
                    let rollback = if self.transaction_open.get() {
                        self.rollback()
                    } else {
                        Ok(())
                    };
                    let unlock_writes = if self.writes_locked.get() {
                        self.execute_batch(self.dialect.unlock_writes())
                    } else {
                        Ok(())
                    };
                    let unlock = if self.schema_locked.get() {
                        self.execute_batch(self.dialect.unlock_schema())
                    } else {
                        Ok(())
                    };
                    if rollback.is_err() || unlock_writes.is_err() || unlock.is_err() {
                        self.connection.borrow_mut().close_on_drop();
                    }
                    result.and(rollback).and(unlock_writes).and(unlock)
                }

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
                    connection: &mut Checkout<$database>,
                ) -> RegistryError {
                    // A cancelled future leaves an unread response on the wire.
                    connection.close_on_drop();
                    self.transaction_open.set(false);
                    self.schema_locked.set(false);
                    self.writes_locked.set(false);
                    RegistryError::StorageUnavailable {
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
                            "CREATE TABLE IF NOT EXISTS schema_migrations (version BIGINT NOT NULL PRIMARY KEY, applied_at BIGINT NOT NULL DEFAULT 0)"
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
