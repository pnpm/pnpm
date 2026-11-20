macro_rules! driver {
    ($module:ident, $database:ty, $pool:ty, $pool_fn:path, $row:ty, $dialect:expr, $timeout_sql:path, $configure:path) => {
        pub(super) mod $module {
            use super::{
                Adapter,
                Arc,
                Cell,
                Checkout,
                Connection,
                Duration,
                NetworkDialect,
                Param,
                Query,
                ReadSource,
                RefCell,
                RegistryError,
                Result,
                Row,
                SqlBackendSettings,
                StartupConnection,
                TypeInfo,
                Value,
                ValueRef,
            };
            use sqlx::Row as _;
            use tokio::sync::Mutex;

            #[derive(Debug)]
            pub(in super::super) struct NetworkAdapter {
                pool: Arc<$pool>,
                startup: Option<StartupConnection<$database>>,
                dialect: NetworkDialect,
                settings: SqlBackendSettings,
                /// Held by a write of this instance, so one connection at a
                /// time waits for the database's write lock and the others
                /// stay free for reads. A write waits for it as long as for a
                /// pool connection.
                writer: Mutex<()>,
            }

            impl NetworkAdapter {
                pub(in super::super) fn new(settings: &SqlBackendSettings) -> Result<Self> {
                    let dialect = NetworkDialect::from_url(&settings.url)?;
                    let pool = $pool_fn(settings)?;
                    Ok(Self {
                        pool,
                        startup: None,
                        dialect,
                        settings: settings.clone(),
                        writer: Mutex::new(()),
                    })
                }

                pub(super) fn from_pool(
                    pool: Arc<$pool>,
                    startup: StartupConnection<$database>,
                    timeout: Duration,
                ) -> Self {
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
                        connection.runtime
                            .block_on(tokio::time::timeout(
                                timeout,
                                $configure(&mut **conn, timeout),
                            ))
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
                        if !self.settings.url.is_empty()
                            && self.pool.size() == 0
                            && !self.pool.is_closed()
                        {
                            let connection =
                                crate::connect_startup(&self.pool, &self.settings.url, timeout)
                                    .await?;
                            return Checkout::startup(
                                Arc::new(Mutex::new(Some(connection))),
                                timeout,
                            )
                            .await;
                        }
                        Ok(Checkout::Pooled(crate::acquire(&self.pool, timeout).await?))
                    })?;
                    self.run_connection(
                        connection,
                        timeout.saturating_sub(started.elapsed()),
                        operation,
                    )
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
                    NetworkDialect::retryable(&**error)
                }
            }

            network_connection!($database, $row);
        }
    };
}
