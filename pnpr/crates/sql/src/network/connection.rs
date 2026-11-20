macro_rules! network_connection {
    ($database:ty, $row:ty) => {
        struct NetworkConnection {
            connection: RefCell<Checkout<$database>>,
            dialect: NetworkDialect,
            runtime: tokio::runtime::Handle,
            timeout: Duration,
            /// Whether the connection holds the schema lock: in its
            /// session, or in each write transaction it begins when the
            /// dialect has no [`NetworkDialect::session_locks`].
            schema_locked: Cell<bool>,
            /// Whether the session holds [`NetworkDialect::lock_writes`].
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
                let rollback = if self.transaction_open.get() { self.rollback() } else { Ok(()) };
                let unlock_writes = if self.writes_locked.get() {
                    self.execute_batch(NetworkDialect::UNLOCK_WRITES)
                } else {
                    Ok(())
                };
                let session_locks = self.dialect.session_locks();
                let unlock = if session_locks && self.schema_locked.get() {
                    self.execute_batch(NetworkDialect::UNLOCK_SCHEMA)
                } else {
                    Ok(())
                };
                if rollback.is_err() || unlock_writes.is_err() || unlock.is_err() {
                    self.connection.borrow_mut().close_on_drop();
                }
                result
                    .and(rollback)
                    .and(unlock_writes)
                    .and(unlock)
            }

            fn rows(&self, sql: &str, params: &[Param<'_>]) -> Result<Vec<Row>> {
                crate::adapter::reject_nul(params)?;
                let (sql, params) = self.dialect.statement(sql, params)?;
                self.fetch(sql, params)?
                    .iter()
                    .map(|row| decode_row(row, 0))
                    .collect()
            }

            fn fetch(&self, sql: String, params: Vec<Param<'_>>) -> Result<Vec<$row>> {
                let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
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
                        tokio::time::timeout(self.timeout, query.fetch_all(&mut **connection)).await
                    })
                    .map_err(|_| self.mark_timed_out(&mut connection))??;
                Ok(rows)
            }

            fn mark_timed_out(&self, connection: &mut Checkout<$database>) -> RegistryError {
                // A cancelled future leaves an unread response on the wire.
                connection.close_on_drop();
                self.transaction_open.set(false);
                self.schema_locked.set(false);
                self.writes_locked.set(false);
                RegistryError::StorageUnavailable {
                    reason: "database operation timed out".to_string(),
                }
            }

            /// Take the session lock `sql` selects `1` for once held.
            /// `lock` names it in the error when its wait timed out.
            fn take_session_lock(&self, sql: &str, lock: &str) -> Result<()> {
                let mut row = self
                    .query_one(sql, &[])
                    .inspect_err(|_| {
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

            /// Begin the transaction that holds the schema lock, unless
            /// the connection is in it.
            fn begin_migration(&self) -> Result<()> {
                if self.transaction_open.get() {
                    return Ok(());
                }
                self.transaction_open.set(true);
                self.execute_batch(NetworkDialect::BEGIN_MIGRATION)
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
            fn dialect(&self) -> crate::Dialect {
                self.dialect.dialect()
            }

            fn transactional_ddl(&self) -> bool {
                !matches!(self.dialect, NetworkDialect::Mysql)
            }

            fn is_recoverable_empty_migrations(&self) -> Result<bool> {
                if matches!(self.dialect, NetworkDialect::Mysql) {
                    return super::mysql_schema::is_recoverable(self);
                }
                Ok(false)
            }

            fn lock_schema(&self) -> Result<()> {
                if self.dialect.session_locks() {
                    let lock = NetworkDialect::lock_schema(self.timeout);
                    self.take_session_lock(&lock, "schema")?;
                    self.schema_locked.set(true);
                    return Ok(());
                }
                if self.transaction_open.get() && !self.schema_locked.get() {
                    return Err(RegistryError::Internal {
                        reason: "schema lock requested inside a transaction".to_string(),
                    });
                }
                self.schema_locked.set(true);
                self.begin_migration()
            }

            fn execute(&self, sql: &str, params: &[Param<'_>]) -> Result<()> {
                self.rows(sql, params)
                    .map(|_| ())
            }

            fn execute_ddl(&self, sql: &str) -> Result<()> {
                if matches!(self.dialect, NetworkDialect::Mysql) {
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
                for query in queries {
                    crate::adapter::reject_nul(&query.params)?;
                }
                let (sql, params) = self.dialect.batch(queries)?;
                let mut results = vec![Vec::new(); queries.len()];
                for row in self.fetch(sql, params)? {
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
                            sqlx::raw_sql(sqlx::AssertSqlSafe(sql)).execute(&mut **connection),
                        )
                        .await
                    })
                    .map_err(|_| self.mark_timed_out(&mut connection))??;
                Ok(())
            }

            fn begin_write(&self) -> Result<()> {
                if !self.dialect.session_locks() && self.schema_locked.get() {
                    self.begin_migration()?;
                    return self.execute_batch(&NetworkDialect::lock_writes_of_a_migration());
                }
                if self.dialect.session_locks() && !self.writes_locked.get() {
                    let lock = NetworkDialect::lock_writes(self.timeout);
                    self.take_session_lock(&lock, "write")?;
                    self.writes_locked.set(true);
                }
                self.transaction_open.set(true);
                self.execute_batch(&self.dialect.begin_write())
            }

            fn begin_read(&self) -> Result<()> {
                self.transaction_open.set(true);
                self.execute_batch(self.dialect.begin_read())
            }

            fn commit(&self) -> Result<()> {
                self.end_transaction("COMMIT", NetworkDialect::COMMIT_AND_UNLOCK_WRITES)
            }

            fn rollback(&self) -> Result<()> {
                self.end_transaction("ROLLBACK", NetworkDialect::ROLLBACK_AND_UNLOCK_WRITES)
            }

            fn schema_table_names(&self) -> Result<Vec<Row>> {
                self.query(self.dialect.table_names(), &[])
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
                        "INT4" | "INT" => Value::Integer(i64::from(row.try_get::<i32, _>(index)?)),
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
    };
}
