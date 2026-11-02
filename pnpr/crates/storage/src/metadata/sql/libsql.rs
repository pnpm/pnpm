//! The libSQL adapter: a primary every instance writes to, such as Turso, and
//! an optional embedded replica on this instance's disk.
//!
//! - Writes run on the primary as `BEGIN IMMEDIATE` transactions, one at a
//!   time per process, so commits stay serialized as they are on one disk.
//! - [`ReadSource::Primary`] reads the primary.
//! - [`ReadSource::Replica`] reads the replica while its last sync is within
//!   the sync interval. A replica past it syncs first, and a replica that
//!   cannot sync, or that is not configured, leaves the read to the primary.
//!
//! The database opens on first use rather than at startup, since the shared
//! layer is synchronous and opening reaches the network. Every statement runs
//! on the blocking pool and waits there for libsql's async client.

use super::adapter::{Adapter, Connection, Param, ReadSource, Row, Value};
use pnpr_config::LibsqlSettings;
use pnpr_error::{RegistryError, Result};
use pnpr_libsql::{Checkout, ConnectionPool};
use std::{
    fmt,
    sync::{Arc, Mutex, MutexGuard, PoisonError, Weak},
    time::{Duration, Instant},
};
use tokio::runtime::Handle;

/// How long a read, or a write's wait for a connection, may take.
const OPERATION_TIMEOUT: Duration = Duration::from_secs(30);

/// How long opening the database may take, the replica's initial sync
/// included.
const STARTUP_TIMEOUT: Duration = Duration::from_mins(5);

/// A metadata database in libSQL.
pub(crate) struct LibsqlAdapter {
    settings: LibsqlSettings,
    opened: tokio::sync::OnceCell<Arc<Opened>>,
    /// Held by the write in progress. The primary serializes the writes of
    /// every process; this keeps one process's writes from waiting on each
    /// other there.
    writer: Mutex<()>,
}

struct Opened {
    primary: ConnectionPool,
    replica: Option<Replica>,
}

struct Replica {
    pool: ConnectionPool,
    /// How stale a read of the replica may be.
    bound: Duration,
    /// When the replica last finished a sync.
    synced: Mutex<Instant>,
    /// Held by the sync in progress, so concurrent reads that find the
    /// replica stale wait for one sync.
    syncing: tokio::sync::Mutex<()>,
}

impl fmt::Debug for LibsqlAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LibsqlAdapter")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

impl LibsqlAdapter {
    /// The database `settings` name. Nothing is opened until the first
    /// operation.
    pub(crate) fn new(settings: LibsqlSettings) -> Self {
        Self { settings, opened: tokio::sync::OnceCell::new(), writer: Mutex::new(()) }
    }

    /// The opened database, opening it on first use.
    fn opened(&self, runtime: &Handle) -> Result<Arc<Opened>> {
        let opened = runtime.block_on(self.opened.get_or_try_init(|| async {
            tokio::time::timeout(STARTUP_TIMEOUT, Opened::open(&self.settings))
                .await
                .map_err(|_| RegistryError::StorageUnavailable {
                    reason: "the libSQL metadata database did not open in time".to_string(),
                })?
        }))?;
        Ok(Arc::clone(opened))
    }
}

impl Opened {
    async fn open(settings: &LibsqlSettings) -> Result<Arc<Self>> {
        let connector = pnpr_libsql::connector(settings, "metadata")?;
        let _startup_dials = connector.retry_timed_out_dials();
        let max_connections =
            settings.max_connections.unwrap_or(LibsqlSettings::DEFAULT_MAX_CONNECTIONS) as usize;
        let primary_db = pnpr_libsql::open_primary(settings, connector.clone()).await?;
        let primary =
            ConnectionPool::new(primary_db, max_connections, OPERATION_TIMEOUT, timed_out);
        let replica = if settings.replica_path.is_some() {
            // The adapter syncs the replica itself, so it knows when the last
            // sync finished; libsql's own background sync does not say.
            let unsynced = LibsqlSettings { sync_interval_secs: Some(0), ..settings.clone() };
            let db = pnpr_libsql::open(&unsynced, connector).await?;
            let interval =
                settings.sync_interval_secs.unwrap_or(LibsqlSettings::DEFAULT_SYNC_INTERVAL_SECS);
            Some(Replica {
                pool: ConnectionPool::new(db, max_connections, OPERATION_TIMEOUT, timed_out),
                bound: Duration::from_secs(interval),
                synced: Mutex::new(Instant::now()),
                syncing: tokio::sync::Mutex::new(()),
            })
        } else {
            None
        };
        let opened = Arc::new(Self { primary, replica });
        if let Some(replica) = &opened.replica {
            tokio::spawn(keep_synced(Arc::downgrade(&opened), replica.bound));
        }
        Ok(opened)
    }
}

/// Sync the replica every `interval` while the database is open.
async fn keep_synced(opened: Weak<Opened>, interval: Duration) {
    loop {
        tokio::time::sleep(interval).await;
        let Some(opened) = opened.upgrade() else { return };
        if let Some(replica) = &opened.replica
            && let Err(error) = replica.sync().await
        {
            tracing::warn!(%error, "the metadata replica could not sync; reads use the primary");
        }
    }
}

impl Replica {
    fn is_fresh(&self) -> bool {
        lock(&self.synced).elapsed() <= self.bound
    }

    /// Pull the primary's latest frames, unless a sync that finished while
    /// this one waited for its turn already did.
    async fn sync(&self) -> Result<()> {
        let requested = Instant::now();
        let _turn = self.syncing.lock().await;
        if *lock(&self.synced) >= requested {
            return Ok(());
        }
        self.pool.sync().await?;
        *lock(&self.synced) = Instant::now();
        Ok(())
    }
}

impl Adapter for LibsqlAdapter {
    fn write(&self, operation: &mut dyn FnMut(&dyn Connection) -> Result<()>) -> Result<()> {
        let runtime = Handle::current();
        let opened = self.opened(&runtime)?;
        let _writer = lock(&self.writer);
        let checkout = runtime.block_on(opened.primary.checkout())?;
        run(&runtime, checkout, false, operation)
    }

    fn read(
        &self,
        source: ReadSource,
        operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
    ) -> Result<()> {
        let runtime = Handle::current();
        let opened = self.opened(&runtime)?;
        let replica = match (source, &opened.replica) {
            (ReadSource::Replica, Some(replica)) => Some(replica),
            _ => None,
        };
        let replica = replica.filter(|replica| {
            replica.is_fresh() || runtime.block_on(replica.sync()).is_ok()
        });
        let (pool, local) = match replica {
            Some(replica) => (&replica.pool, true),
            None => (&opened.primary, false),
        };
        let checkout = runtime.block_on(pool.checkout())?;
        run(&runtime, checkout, local, operation)
    }

    fn is_retryable(&self, error: &RegistryError) -> bool {
        matches!(error, RegistryError::Libsql(error) if pnpr_libsql::is_transaction_conflict(error))
    }

    fn has_replica(&self) -> bool {
        self.settings.replica_path.is_some()
    }

    fn sync_replica(&self) -> Result<()> {
        let runtime = Handle::current();
        let opened = self.opened(&runtime)?;
        match &opened.replica {
            Some(replica) => runtime.block_on(replica.sync()),
            None => Ok(()),
        }
    }
}

/// Run `operation` on the checked-out connection, which reads the local
/// replica when `local`, and return the connection to its pool.
fn run(
    runtime: &Handle,
    checkout: Checkout<'_>,
    local: bool,
    operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
) -> Result<()> {
    let statements = Statements { runtime, conn: checkout.connection(), local };
    let result = operation(&statements);
    checkout.release(result.is_ok());
    result
}

/// What a metadata operation past [`OPERATION_TIMEOUT`] fails with.
fn timed_out() -> RegistryError {
    RegistryError::StorageUnavailable {
        reason: "the libSQL metadata database did not answer in time".to_string(),
    }
}

fn lock<Value>(mutex: &Mutex<Value>) -> MutexGuard<'_, Value> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One libSQL connection, run from a blocking thread.
struct Statements<'conn> {
    runtime: &'conn Handle,
    conn: &'conn libsql::Connection,
    /// Whether the connection reads the embedded replica.
    local: bool,
}

impl Connection for Statements<'_> {
    fn execute(&self, sql: &str, params: &[Param<'_>]) -> Result<usize> {
        let changed = self.runtime.block_on(self.conn.execute(sql, to_params(params)))?;
        Ok(usize::try_from(changed).unwrap_or(usize::MAX))
    }

    fn query(&self, sql: &str, params: &[Param<'_>]) -> Result<Vec<Row>> {
        self.runtime.block_on(async {
            let mut rows = self.conn.query(sql, to_params(params)).await?;
            let columns = rows.column_count();
            let mut collected = Vec::new();
            while let Some(row) = rows.next().await? {
                let values = (0..columns)
                    .map(|index| row.get_value(index).map(to_value))
                    .collect::<libsql::Result<_>>()?;
                collected.push(Row::new(values));
            }
            Ok(collected)
        })
    }

    fn execute_batch(&self, sql: &str) -> Result<()> {
        self.runtime.block_on(self.conn.execute_batch(sql))?;
        Ok(())
    }

    fn begin_write(&self) -> Result<()> {
        self.execute_batch("BEGIN IMMEDIATE")
    }

    /// A replica connection sends every statement of a plain `BEGIN`
    /// transaction to the primary; `BEGIN READONLY` keeps it on the replica.
    fn begin_read(&self) -> Result<()> {
        self.execute_batch(if self.local { "BEGIN READONLY" } else { "BEGIN" })
    }
}

fn to_params(params: &[Param<'_>]) -> Vec<libsql::Value> {
    params
        .iter()
        .map(|param| match *param {
            Param::Null => libsql::Value::Null,
            Param::Integer(value) => libsql::Value::Integer(value),
            Param::Text(value) => libsql::Value::Text(value.to_string()),
            Param::Blob(value) => libsql::Value::Blob(value.to_vec()),
        })
        .collect()
}

fn to_value(value: libsql::Value) -> Value {
    match value {
        libsql::Value::Null => Value::Null,
        libsql::Value::Integer(value) => Value::Integer(value),
        libsql::Value::Real(value) => Value::Real(value),
        libsql::Value::Text(value) => Value::Text(value),
        libsql::Value::Blob(value) => Value::Blob(value),
    }
}
