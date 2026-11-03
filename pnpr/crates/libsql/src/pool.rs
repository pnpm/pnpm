//! The connections a libSQL database's operations run on.

use crate::with_deadline;
use libsql::{Connection, Database, Error as LibsqlError, Row};
use pnpr_error::{RegistryError, Result};
use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
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
/// fresh one.
///
/// Reads and writes are bounded differently, because cancelling a write after
/// it reached the database leaves its outcome unknown. [`Self::read`] runs under the deadline end to end.
/// [`Self::write`] is bounded only while it gets a connection, before anything
/// is sent, and then runs to completion. Opening a connection can block (an
/// embedded replica waits for its background sync), so it runs on the
/// blocking thread pool, where the deadline can still give up on it.
pub struct ConnectionPool {
    db: Arc<Database>,
    idle: Arc<Mutex<Vec<Connection>>>,
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
            slots: Arc::new(Semaphore::new(max_connections)),
            opens: Arc::new(Semaphore::new(max_connections)),
            timeout,
            timed_out,
        }
    }

    /// Run the read `operation` on a pooled connection. Past the deadline,
    /// waiting for a connection included, it fails with the pool's timeout
    /// error.
    ///
    /// `operation` must not use the pool itself: with every connection held
    /// by an operation waiting for another, the pool deadlocks.
    pub async fn read<Loaded, Operation, Pending>(&self, operation: Operation) -> Result<Loaded>
    where
        Operation: FnOnce(Connection) -> Pending,
        Pending: Future<Output = Result<Loaded>>,
    {
        with_deadline::<_, RegistryError>(self.timeout, self.timed_out, async {
            let checkout = self.take().await?;
            checkout.run(operation).await
        })
        .await
    }

    /// Run the write `operation` on a pooled connection. It fails with the
    /// pool's timeout error if no connection frees up within the deadline,
    /// and otherwise runs to completion.
    ///
    /// `operation` must not use the pool itself; see [`Self::read`].
    pub async fn write<Loaded, Operation, Pending>(&self, operation: Operation) -> Result<Loaded>
    where
        Operation: FnOnce(Connection) -> Pending,
        Pending: Future<Output = Result<Loaded>>,
    {
        self.checkout().await?.run(operation).await
    }

    /// A connection to run statements on outside an async operation, such as
    /// from a blocking thread. It fails with the pool's timeout error if no
    /// connection frees up within the deadline. [`Checkout::release`] returns
    /// the connection.
    pub async fn checkout(&self) -> Result<Checkout> {
        with_deadline::<_, RegistryError>(self.timeout, self.timed_out, self.take()).await
    }

    /// A slot and a connection to run one operation on.
    async fn take(&self) -> Result<Checkout> {
        let slot = Arc::clone(&self.slots)
            .acquire_owned()
            .await
            .expect("the pool never closes its semaphore");
        let pooled = self.idle
            .lock()
            .expect("idle connections lock")
            .pop();
        if let Some(conn) = pooled {
            return Ok(Checkout { idle: Arc::clone(&self.idle), _slot: slot, conn });
        }
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
        Ok(Checkout { idle: Arc::clone(&self.idle), _slot: slot, conn })
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

/// A connection checked out of a [`ConnectionPool`], holding one of its
/// slots until it is released or dropped.
pub struct Checkout {
    idle: Arc<Mutex<Vec<Connection>>>,
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
                .push(self.conn);
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

    async fn run<Loaded, Operation, Pending>(self, operation: Operation) -> Result<Loaded>
    where
        Operation: FnOnce(Connection) -> Pending,
        Pending: Future<Output = Result<Loaded>>,
    {
        let result = operation(self.conn.clone()).await;
        self.release(result.is_ok());
        result
    }
}

#[cfg(test)]
mod tests;
