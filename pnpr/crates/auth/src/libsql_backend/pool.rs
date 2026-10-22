//! The connections the libsql backend runs its operations on.

use super::{Connection, Database, LibsqlError};
use std::{future::Future, sync::Mutex};
use tokio::sync::Semaphore;

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
/// fresh one. A pooled connection keeps its HTTP client, so reusing it costs
/// no new TCP or TLS handshake.
pub(super) struct ConnectionPool {
    db: Database,
    idle: Mutex<Vec<Connection>>,
    slots: Semaphore,
}

impl ConnectionPool {
    /// A pool that runs at most `max_connections` operations at once.
    pub(super) fn new(db: Database, max_connections: usize) -> Self {
        Self { db, idle: Mutex::new(Vec::new()), slots: Semaphore::new(max_connections) }
    }

    /// Run `operation` on a pooled connection, waiting for one when every
    /// connection is busy.
    ///
    /// `operation` must not call [`Self::run`] itself: with every connection
    /// held by an operation waiting for another, the pool deadlocks.
    pub(super) async fn run<Loaded, DbError, Operation, Pending>(
        &self,
        operation: Operation,
    ) -> Result<Loaded, DbError>
    where
        Operation: FnOnce(Connection) -> Pending,
        Pending: Future<Output = Result<Loaded, DbError>>,
        DbError: From<LibsqlError>,
    {
        let _slot = self.slots.acquire().await.expect("the pool never closes its semaphore");
        let pooled = self.idle
            .lock()
            .expect("idle connections lock")
            .pop();
        let conn = match pooled {
            Some(conn) => conn,
            None => self.db.connect()?,
        };
        let result = operation(conn.clone()).await;
        if result.is_ok() && conn.is_autocommit() {
            self.idle
                .lock()
                .expect("idle connections lock")
                .push(conn);
        }
        result
    }

    /// An unpooled connection, for setup that runs before any operation.
    pub(super) fn connect(&self) -> Result<Connection, LibsqlError> {
        self.db.connect()
    }
}

#[cfg(test)]
mod tests;
