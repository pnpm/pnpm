//! The libSQL adapter: a primary every instance writes to, such as Turso, and
//! an optional embedded replica on this instance's disk.
//!
//! - Writes run on the primary as `BEGIN IMMEDIATE` transactions, one at a
//!   time per process, so commits stay serialized as they are on one disk.
//! - [`ReadSource::Primary`] reads the primary.
//! - [`ReadSource::Replica`] reads the replica while its last sync is within
//!   the sync interval and no write of this process came after it. Any other
//!   replica syncs first, and a replica that cannot sync, or that is not
//!   configured, leaves the read to the primary. So a process reads its own
//!   writes, release notes included, which no generation tracks.
//!
//! Each round trip to the primary costs a network round trip, so statements
//! reach it in batches: a transaction's `BEGIN` and every statement whose
//! answer is not needed wait for the next statement that answers, or for the
//! `COMMIT`. The shared layer reads what a write compares with in one batch,
//! so a write takes two round trips: one that takes the write lock and reads,
//! and one that writes and commits. A read of the primary is answered before
//! its `COMMIT` is sent.
//!
//! The database opens on first use rather than at startup, since the shared
//! layer is synchronous and opening reaches the network. Every statement runs
//! on the blocking pool and waits there for libsql's async client.

use super::adapter::{Adapter, Connection, ReadSource};
use pnpr_config::LibsqlSettings;
use pnpr_error::{RegistryError, Result};
use pnpr_libsql::{Checkout, ConnectionPool};
use statements::Statements;
use std::{
    cell::{Cell, RefCell},
    fmt,
    sync::{Arc, Mutex, MutexGuard, PoisonError, Weak},
    time::{Duration, Instant},
};
use tokio::runtime::Handle;

/// How long a wait for a connection may take, and then a read's statements.
/// A write's statements have no deadline.
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
    /// When the replica's last sync started, and the writes since.
    synced: Mutex<SyncState>,
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
            // Opening the replica syncs it, so it holds what the primary held
            // when the sync started.
            let opening = Instant::now();
            let db = pnpr_libsql::open(&unsynced, connector).await?;
            let interval =
                settings.sync_interval_secs.unwrap_or(LibsqlSettings::DEFAULT_SYNC_INTERVAL_SECS);
            Some(Replica {
                pool: ConnectionPool::new(db, max_connections, OPERATION_TIMEOUT, timed_out),
                bound: Duration::from_secs(interval),
                synced: Mutex::new(SyncState { synced: Some(opening), writes: 0 }),
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
            && let Err(error) = replica.sync(Instant::now()).await
        {
            tracing::warn!(%error, "the metadata replica could not sync; reads use the primary");
        }
    }
}

/// What a replica's reads may rely on: when its last sync started, unless a
/// write of this process committed after that.
#[derive(Debug, Default)]
struct SyncState {
    /// When the last sync that no write overtook started; its pull holds at
    /// least what the primary held then. `None` once a write of this process
    /// committed after it.
    synced: Option<Instant>,
    /// Writes of this process so far. A sync that a write overtook pulled a
    /// snapshot that may predate it.
    writes: u64,
}

/// A sync in progress, from [`SyncState::start_sync`].
#[derive(Debug, Clone, Copy)]
struct SyncStart {
    at: Instant,
    writes: u64,
}

impl SyncState {
    fn is_fresh(&self, bound: Duration) -> bool {
        self.synced.is_some_and(|synced| synced.elapsed() <= bound)
    }

    /// Whether a sync that started at or after `requested` finished.
    fn synced_since(&self, requested: Instant) -> bool {
        self.synced.is_some_and(|synced| synced >= requested)
    }

    /// Record that the primary holds a write the replica has not pulled.
    fn mark_stale(&mut self) {
        self.synced = None;
        self.writes += 1;
    }

    /// Note a sync starting now, for [`Self::finish_sync`].
    fn start_sync(&self) -> SyncStart {
        SyncStart { at: Instant::now(), writes: self.writes }
    }

    /// Record the sync `started` as finished, unless a write came after its
    /// start.
    fn finish_sync(&mut self, started: SyncStart) {
        if self.writes == started.writes {
            self.synced = Some(started.at);
        }
    }
}

impl Replica {
    fn is_fresh(&self) -> bool {
        lock(&self.synced).is_fresh(self.bound)
    }

    /// Record that the primary holds a write the replica has not pulled.
    fn mark_stale(&self) {
        lock(&self.synced).mark_stale();
    }

    /// Pull the primary's latest frames, unless a sync that started at or
    /// after `requested` already did, such as one that ran while this one
    /// waited for its turn. A write of this process that commits during the
    /// pull leaves the replica stale.
    async fn sync(&self, requested: Instant) -> Result<()> {
        let _turn = self.syncing.lock().await;
        if lock(&self.synced).synced_since(requested) {
            return Ok(());
        }
        let started = lock(&self.synced).start_sync();
        self.pool.sync().await?;
        lock(&self.synced).finish_sync(started);
        Ok(())
    }
}

impl Adapter for LibsqlAdapter {
    fn write(&self, operation: &mut dyn FnMut(&dyn Connection) -> Result<()>) -> Result<()> {
        let runtime = Handle::current();
        let opened = self.opened(&runtime)?;
        let _writer = lock(&self.writer);
        let checkout = runtime.block_on(opened.primary.checkout())?;
        // A write cut off after it reached the primary leaves its commit
        // state unknown, so its statements run without a deadline.
        let result = run(&runtime, checkout, Session::Write, operation);
        if let Some(replica) = &opened.replica {
            replica.mark_stale();
        }
        result
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
        // A sync that a write of this process overtook leaves the replica
        // stale, and the read goes to the primary.
        let replica = replica.filter(|replica| {
            replica.is_fresh()
                || (runtime
                    .block_on(replica.sync(Instant::now()))
                    .is_ok()
                    && replica.is_fresh())
        });
        let (pool, local) = match replica {
            Some(replica) => (&replica.pool, true),
            None => (&opened.primary, false),
        };
        let checkout = runtime.block_on(pool.checkout())?;
        let deadline = tokio::time::Instant::now() + OPERATION_TIMEOUT;
        run(&runtime, checkout, Session::Read { local, deadline }, operation)
    }

    fn is_retryable(&self, error: &RegistryError) -> bool {
        matches!(error, RegistryError::Libsql(error) if pnpr_libsql::is_transaction_conflict(error))
    }

    fn has_replica(&self) -> bool {
        self.settings.replica_path.is_some()
    }

    fn sync_replica(&self, since: Instant) -> Result<()> {
        let runtime = Handle::current();
        let opened = self.opened(&runtime)?;
        let Some(replica) = &opened.replica else { return Ok(()) };
        let synced = runtime.block_on(replica.sync(since));
        if synced.is_err() {
            replica.mark_stale();
        }
        synced
    }

    fn is_database_failure(&self, error: &RegistryError) -> bool {
        matches!(error, RegistryError::Libsql(_) | RegistryError::StorageUnavailable { .. })
    }
}

/// What a checked-out connection runs.
#[derive(Debug, Clone, Copy)]
enum Session {
    /// A write on the primary. Its statements have no deadline: a write cut
    /// off after it reached the primary leaves its commit state unknown.
    Write,
    /// A read of the local replica when `local`, otherwise of the primary,
    /// whose statements fail once `deadline` passes.
    Read { local: bool, deadline: tokio::time::Instant },
}

/// Run `operation` on the checked-out connection, and return the connection
/// to its pool. A connection whose operation failed is closed.
///
/// The `COMMIT` that ends a read of the primary is sent once the operation
/// has its answer, without waiting for it: nothing the read returns depends
/// on it. The connection keeps its slot in the pool until the `COMMIT` is
/// answered, and is closed if it is not.
fn run(
    runtime: &Handle,
    checkout: Checkout,
    session: Session,
    operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
) -> Result<()> {
    let (local, deadline, read) = match session {
        Session::Write => (false, None, false),
        Session::Read { local, deadline } => (local, Some(deadline), true),
    };
    let (result, end_later) = {
        let statements = Statements {
            runtime,
            conn: checkout.connection(),
            local,
            deadline,
            read,
            held: RefCell::new(Vec::new()),
            end_later: Cell::new(false),
        };
        // A statement still held reaches the primary before the connection
        // goes back to the pool.
        let result = operation(&statements).and_then(|()| statements.send().map(drop));
        (result, statements.end_later.get())
    };
    if result.is_ok() && end_later {
        let conn = checkout.connection().clone();
        checkout.release_after(runtime, async move {
            let commit = conn.execute_batch_with_params([("COMMIT", ())]);
            matches!(tokio::time::timeout(OPERATION_TIMEOUT, commit).await, Ok(Ok(_)))
        });
        return result;
    }
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

mod statements;
#[cfg(test)]
mod tests;
