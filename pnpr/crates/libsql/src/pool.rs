//! The connections a libSQL database's operations run on.

use crate::{is_stale_stream, with_deadline};
use libsql::{Connection, Database, Error as LibsqlError, Row};
use pnpr_error::{RegistryError, Result};
use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    runtime::Handle,
    sync::{OwnedSemaphorePermit, Semaphore},
};

/// `read` applied to the first of `rows`, which are then read to their end.
/// A remote query's rows stream in one HTTP response, and a response left
/// unread closes its connection, so the next query would open a new one. A
/// local row reads the live statement, so `read` runs before the next step.
pub async fn first_row<Output>(
    mut rows: libsql::Rows,
    read: impl FnOnce(&Row) -> Result<Output>,
) -> Result<Option<Output>> {
    let first = match rows.next().await? {
        Some(row) => Some(read(&row)?),
        None => None,
    };
    while rows.next().await?.is_some() {}
    Ok(first)
}

/// How long a connection may sit in the pool before the pool closes it
/// instead of handing it out. `sqld`, which Turso runs, expires a stream 10
/// seconds after it last answered on it, and rejects the stream's next request.
/// Opening a remote connection costs no round trip: its first request opens
/// a new stream.
const IDLE_EXPIRY: Duration = Duration::from_secs(5);

/// Connections to one database, each running one operation at a time.
///
/// A remote connection is a stream on the server, and each response hands the
/// client the token for its next request on that stream. An operation dropped
/// while a request is in flight (a client that disconnected, a deadline)
/// never receives that token, so the stream rejects every later request. A
/// failed operation can leave its stream in a state just as unknown, and one
/// still inside a transaction would pull the next operation into it. A
/// connection therefore returns to the pool only after its operation succeeded
/// outside a transaction; any other is closed, and a later checkout opens a
/// fresh one. A connection idle for `IDLE_EXPIRY` is closed as well.
///
/// A stream can still be gone when its connection is checked out, such as
/// after the server restarted. The server rejects the stream's request
/// without running it, and discards the stream with any transaction it held.
/// [`Self::read`] and [`Self::write`] then run the operation once more on a
/// new connection, and every idle connection is closed, since they share the
/// server's fate.
///
/// Reads and writes are bounded differently, because cancelling a write after
/// it reached the database leaves its outcome unknown. [`Self::read`] runs under the deadline end to end.
/// [`Self::write`] is bounded only while it gets a connection, before anything
/// is sent, and then runs to completion. Opening a connection can block (an
/// embedded replica waits for its background sync), so it runs on the
/// blocking thread pool, where the deadline can still give up on it.
pub struct ConnectionPool {
    db: Arc<Database>,
    idle: Arc<Mutex<Vec<Idle>>>,
    /// `None` for an embedded replica; see [`Self::replica`].
    idle_expiry: Option<Duration>,
    slots: Arc<Semaphore>,
    /// Connection opens in flight. An open the deadline gave up on keeps its
    /// permit until it returns, so a stalled database cannot pile up blocked
    /// threads beyond the pool's size.
    opens: Arc<Semaphore>,
    timeout: Duration,
    /// The error an operation past `timeout` fails with.
    timed_out: fn() -> RegistryError,
}

impl ConnectionPool {
    /// A pool that runs at most `max_connections` operations at once, with
    /// `timeout` as the deadline [`Self::read`] and [`Self::write`] apply.
    /// An operation past its deadline fails with the error `timed_out`
    /// returns, which names the database the pool's owner keeps there.
    pub fn new(
        db: Database,
        max_connections: usize,
        timeout: Duration,
        timed_out: fn() -> RegistryError,
    ) -> Self {
        Self {
            db: Arc::new(db),
            idle: Arc::new(Mutex::new(Vec::new())),
            idle_expiry: Some(IDLE_EXPIRY),
            slots: Arc::new(Semaphore::new(max_connections)),
            opens: Arc::new(Semaphore::new(max_connections)),
            timeout,
            timed_out,
        }
    }

    /// Run the read `operation` on a pooled connection, and once more on a
    /// new one if the server rejected the stream as stale. Past the deadline,
    /// waiting for a connection included, it fails with the pool's timeout
    /// error.
    ///
    /// `operation` must not use the pool itself: with every connection held
    /// by an operation waiting for another, the pool deadlocks.
    pub async fn read<Loaded, Operation, Pending>(&self, mut operation: Operation) -> Result<Loaded>
    where
        Operation: FnMut(Connection) -> Pending,
        Pending: Future<Output = Result<Loaded>>,
    {
        with_deadline::<_, RegistryError>(self.timeout, self.timed_out, async {
            match self.take().await?.run(&mut operation).await {
                Err(error) if is_stale_stream(&error) => {
                    self.open().await?.run(&mut operation).await
                }
                result => result,
            }
        })
        .await
    }

    /// Run the write `operation` on a pooled connection, and once more on a
    /// new one if the server rejected the stream as stale. It fails with the
    /// pool's timeout error if no connection frees up within the deadline,
    /// and otherwise runs to completion.
    ///
    /// `operation` must send one statement, or run its statements in one
    /// transaction: a rejected stream takes its transaction with it, but a
    /// statement that committed on its own before the rejection would run
    /// twice. It must not use the pool itself; see [`Self::read`].
    pub async fn write<Loaded, Operation, Pending>(
        &self,
        mut operation: Operation,
    ) -> Result<Loaded>
    where
        Operation: FnMut(Connection) -> Pending,
        Pending: Future<Output = Result<Loaded>>,
    {
        match self.checkout().await?.run(&mut operation).await {
            Err(error) if is_stale_stream(&error) => {
                self.checkout_fresh().await?.run(&mut operation).await
            }
            result => result,
        }
    }

    /// A connection to run statements on outside an async operation, such as
    /// from a blocking thread. It fails with the pool's timeout error if no
    /// connection frees up within the deadline. [`Checkout::release`] returns
    /// the connection.
    pub async fn checkout(&self) -> Result<Checkout> {
        with_deadline::<_, RegistryError>(self.timeout, self.timed_out, self.take()).await
    }

    /// A new connection, closing every idle one, for retrying an operation
    /// whose stream the server rejected as stale. It fails with the pool's
    /// timeout error if no connection opens within the deadline.
    pub async fn checkout_fresh(&self) -> Result<Checkout> {
        with_deadline::<_, RegistryError>(self.timeout, self.timed_out, self.open()).await
    }

    /// A slot and a connection to run one operation on.
    async fn take(&self) -> Result<Checkout> {
        let slot = self.acquire_slot().await;
        match self.pop_idle() {
            Some(conn) => Ok(self.checkout_of(slot, conn)),
            None => self.connect_in(slot).await,
        }
    }

    /// A slot and a new connection, closing every idle one.
    async fn open(&self) -> Result<Checkout> {
        let slot = self.acquire_slot().await;
        let idle = std::mem::take(&mut *self.idle.lock().expect("idle connections lock"));
        drop(idle);
        self.connect_in(slot).await
    }

    async fn acquire_slot(&self) -> OwnedSemaphorePermit {
        Arc::clone(&self.slots).acquire_owned().await.expect("the pool never closes its semaphore")
    }

    /// The connection idle the shortest, unless it idled past the expiry:
    /// then it and every connection idle longer are closed.
    fn pop_idle(&self) -> Option<Connection> {
        let mut idle = self.idle.lock().expect("idle connections lock");
        let newest = idle.pop()?;
        if self.idle_expiry.is_none_or(|expiry| newest.since.elapsed() < expiry) {
            return Some(newest.conn);
        }
        let expired = std::mem::take(&mut *idle);
        drop(idle);
        drop((newest, expired));
        None
    }

    fn checkout_of(&self, slot: OwnedSemaphorePermit, conn: Connection) -> Checkout {
        Checkout { idle: Arc::clone(&self.idle), _slot: slot, conn }
    }

    async fn connect_in(&self, slot: OwnedSemaphorePermit) -> Result<Checkout> {
        let open = Arc::clone(&self.opens)
            .acquire_owned()
            .await
            .expect("the pool never closes its semaphore");
        let db = Arc::clone(&self.db);
        let conn = tokio::task::spawn_blocking(move || {
            let _open = open;
            db.connect()
        })
        .await
        .expect("opening a libsql connection panicked")?;
        Ok(self.checkout_of(slot, conn))
    }

    /// Keep idle connections however long they idle. An embedded replica's
    /// connection reads a local file and holds no stream that could expire,
    /// and opening one can wait for a background sync.
    #[must_use]
    pub fn replica(mut self) -> Self {
        self.idle_expiry = None;
        self
    }

    /// Close connections idle for `expiry` rather than [`IDLE_EXPIRY`].
    #[cfg(test)]
    pub(crate) fn with_idle_expiry(mut self, expiry: Duration) -> Self {
        self.idle_expiry = Some(expiry);
        self
    }

    /// How many connection opens are still running.
    #[cfg(test)]
    pub(crate) fn opens_in_flight(&self, max_connections: usize) -> usize {
        max_connections - self.opens.available_permits()
    }

    /// An unpooled connection, for setup that runs before any operation.
    pub fn connect(&self) -> std::result::Result<Connection, LibsqlError> {
        self.db.connect()
    }

    /// Pull the primary's latest frames into the embedded replica, on a
    /// connection of the pull's own.
    ///
    /// A pull only brings frames the primary already has, so cutting one short
    /// at the deadline loses nothing.
    pub async fn sync(&self) -> Result<()> {
        with_deadline::<_, LibsqlError>(self.timeout, self.timed_out, async {
            self.db.sync().await?;
            Ok(())
        })
        .await
    }
}

struct Idle {
    conn: Connection,
    since: Instant,
}

/// A connection checked out of a [`ConnectionPool`], holding one of its
/// slots until it is released or dropped.
pub struct Checkout {
    idle: Arc<Mutex<Vec<Idle>>>,
    /// Declared before the slot, so a dropped checkout closes its connection
    /// before the slot lets another operation open one.
    conn: Connection,
    _slot: OwnedSemaphorePermit,
}

impl Checkout {
    #[must_use]
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Give the connection back to the pool when its work `succeeded` outside
    /// a transaction, and close it otherwise.
    pub fn release(self, succeeded: bool) {
        if succeeded && self.conn.is_autocommit() {
            self.idle
                .lock()
                .expect("idle connections lock")
                .push(Idle { conn: self.conn, since: Instant::now() });
        }
    }

    /// Run `end` on `runtime` without waiting for it, such as the statement
    /// that ends a finished read, and then release the connection as
    /// [`Self::release`] does, succeeded when `end` reports it did. The
    /// connection holds its slot until then.
    pub fn release_after<End>(self, runtime: &Handle, end: End)
    where
        End: Future<Output = bool> + Send + 'static,
    {
        runtime.spawn(async move {
            let succeeded = end.await;
            self.release(succeeded);
        });
    }

    async fn run<Loaded, Operation, Pending>(self, operation: &mut Operation) -> Result<Loaded>
    where
        Operation: FnMut(Connection) -> Pending,
        Pending: Future<Output = Result<Loaded>>,
    {
        let result = operation(self.conn.clone()).await;
        self.release(result.is_ok());
        result
    }
}

#[cfg(test)]
mod tests;
