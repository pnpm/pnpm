//! The connections the libsql backend runs its operations on.

use super::{Connection, Database, LibsqlError, RegistryError, Result, with_auth_timeout};
use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{Semaphore, SemaphorePermit};

/// Connections to one database, each running one operation at a time.
///
/// A remote connection is a stream on the server, and each response hands the
/// client the token for its next request on that stream. An operation dropped
/// while a request is in flight (a client that disconnected, an auth timeout)
/// never receives that token, so the stream rejects every later request. A
/// failed operation can leave its stream in a state just as unknown, and one
/// still inside a transaction would pull the next operation into it. A
/// connection therefore returns to the pool only after its operation succeeded
/// outside a transaction; any other is closed, and a later checkout opens a
/// fresh one.
///
/// Reads and writes are bounded differently, because cancelling a write after
/// it reached the database leaves its outcome unknown (see
/// [`with_auth_timeout`]). [`Self::read`] runs under the deadline end to end.
/// [`Self::write`] is bounded only while it gets a connection, before anything
/// is sent, and then runs to completion. Opening a connection can block (an
/// embedded replica waits for its background sync), so it runs on the
/// blocking thread pool, where the deadline can still give up on it.
pub(super) struct ConnectionPool {
    db: Arc<Database>,
    idle: Mutex<Vec<Connection>>,
    slots: Semaphore,
    /// Connection opens in flight. An open the deadline gave up on keeps its
    /// permit until it returns, so a stalled database cannot pile up blocked
    /// threads beyond the pool's size.
    opens: Arc<Semaphore>,
    timeout: Duration,
}

impl ConnectionPool {
    /// A pool that runs at most `max_connections` operations at once, with
    /// `timeout` as the deadline [`Self::read`] and [`Self::write`] apply.
    pub(super) fn new(db: Database, max_connections: usize, timeout: Duration) -> Self {
        Self {
            db: Arc::new(db),
            idle: Mutex::new(Vec::new()),
            slots: Semaphore::new(max_connections),
            opens: Arc::new(Semaphore::new(max_connections)),
            timeout,
        }
    }

    /// Run the read `operation` on a pooled connection. Past the deadline,
    /// waiting for a connection included, it fails with
    /// [`RegistryError::AuthDatabaseTimeout`].
    ///
    /// `operation` must not use the pool itself: with every connection held
    /// by an operation waiting for another, the pool deadlocks.
    pub(super) async fn read<Loaded, Operation, Pending>(
        &self,
        operation: Operation,
    ) -> Result<Loaded>
    where
        Operation: FnOnce(Connection) -> Pending,
        Pending: Future<Output = Result<Loaded>>,
    {
        with_auth_timeout::<_, RegistryError>(self.timeout, async {
            let (slot, conn) = self.checkout().await?;
            self.run_holding(slot, conn, operation).await
        })
        .await
    }

    /// Run the write `operation` on a pooled connection. It fails with
    /// [`RegistryError::AuthDatabaseTimeout`] if no connection frees up within
    /// the deadline, and otherwise runs to completion.
    ///
    /// `operation` must not use the pool itself; see [`Self::read`].
    pub(super) async fn write<Loaded, Operation, Pending>(
        &self,
        operation: Operation,
    ) -> Result<Loaded>
    where
        Operation: FnOnce(Connection) -> Pending,
        Pending: Future<Output = Result<Loaded>>,
    {
        let (slot, conn) =
            with_auth_timeout::<_, RegistryError>(self.timeout, self.checkout()).await?;
        self.run_holding(slot, conn, operation).await
    }

    /// A slot and a connection to run one operation on.
    async fn checkout(&self) -> Result<(SemaphorePermit<'_>, Connection)> {
        let slot = self.slots.acquire().await.expect("the pool never closes its semaphore");
        let pooled = self.idle
            .lock()
            .expect("idle connections lock")
            .pop();
        if let Some(conn) = pooled {
            return Ok((slot, conn));
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
        Ok((slot, conn))
    }

    async fn run_holding<Loaded, Operation, Pending>(
        &self,
        _slot: SemaphorePermit<'_>,
        conn: Connection,
        operation: Operation,
    ) -> Result<Loaded>
    where
        Operation: FnOnce(Connection) -> Pending,
        Pending: Future<Output = Result<Loaded>>,
    {
        let result = operation(conn.clone()).await;
        if result.is_ok() && conn.is_autocommit() {
            self.idle
                .lock()
                .expect("idle connections lock")
                .push(conn);
        }
        result
    }

    /// How many connection opens are still running.
    #[cfg(test)]
    pub(super) fn opens_in_flight(&self, max_connections: usize) -> usize {
        max_connections - self.opens.available_permits()
    }

    /// An unpooled connection, for setup that runs before any operation.
    pub(super) fn connect(&self) -> std::result::Result<Connection, LibsqlError> {
        self.db.connect()
    }

    /// Pull the primary's latest frames into the embedded replica, on a
    /// connection of the pull's own.
    ///
    /// A pull only brings frames the primary already has, so cutting one short
    /// at the deadline loses nothing.
    pub(super) async fn sync(&self) -> Result<()> {
        with_auth_timeout::<_, LibsqlError>(self.timeout, async {
            self.db.sync().await?;
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests;
