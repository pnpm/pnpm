use std::collections::VecDeque;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::auth::{AuthContext, Authorization};
use crate::params::{IntoParams, Params};
use crate::rows::Rows;
use crate::statement::Statement;
use crate::transaction::Transaction;
use crate::{Result, TransactionBehavior};

pub type AuthHook = Arc<dyn Fn(&AuthContext) -> Authorization>;

pub type UpdateHook = dyn Fn(Op, &str, &str, i64) + Send + Sync;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Insert = 0,
    Delete = 1,
    Update = 2,
}

#[async_trait::async_trait]
pub(crate) trait Conn {
    async fn execute(&self, sql: &str, params: Params) -> Result<u64>;

    async fn execute_batch(&self, sql: &str) -> Result<BatchRows>;

    async fn execute_transactional_batch(&self, sql: &str) -> Result<BatchRows>;

    async fn prepare(&self, sql: &str) -> Result<Statement>;

    async fn transaction(&self, tx_behavior: TransactionBehavior) -> Result<Transaction>;

    fn interrupt(&self) -> Result<()>;

    fn busy_timeout(&self, timeout: Duration) -> Result<()>;

    fn is_autocommit(&self) -> bool;

    fn changes(&self) -> u64;

    fn total_changes(&self) -> u64;

    fn last_insert_rowid(&self) -> i64;

    async fn reset(&self);

    fn set_reserved_bytes(&self, _reserved_bytes: i32) -> Result<()> {
        Err(crate::Error::ReservedBytesNotSupported)
    }

    fn get_reserved_bytes(&self) -> Result<i32> {
        Err(crate::Error::ReservedBytesNotSupported)
    }

    fn enable_load_extension(&self, _onoff: bool) -> Result<()> {
        Err(crate::Error::LoadExtensionNotSupported)
    }

    fn load_extension(&self, _dylib_path: &Path, _entry_point: Option<&str>) -> Result<()> {
        Err(crate::Error::LoadExtensionNotSupported)
    }

    fn authorizer(&self, _hook: Option<AuthHook>) -> Result<()> {
        Err(crate::Error::AuthorizerNotSupported)
    }

    fn add_update_hook(&self, _cb: Box<dyn Fn(Op, &str, &str, i64) + Send + Sync>) -> Result<()> {
        Err(crate::Error::UpdateHookNotSupported)
    }
}

/// A set of rows returned from `execute_batch`/`execute_transactional_batch`. It is essentially
/// rows of rows for each statement in the batch call.
///
/// # Note
///
/// All rows will be materialized in memory, if you would like to stream them then use `query`
/// instead as this is optimized better for memory usage.
pub struct BatchRows {
    inner: VecDeque<Option<Rows>>,
    skip_last_amt: usize,
}

impl BatchRows {
    #[allow(unused)]
    pub(crate) fn empty() -> Self {
        Self {
            inner: VecDeque::new(),
            skip_last_amt: 0,
        }
    }

    #[cfg(any(feature = "hrana", feature = "core"))]
    pub(crate) fn new(rows: Vec<Option<Rows>>) -> Self {
        Self {
            inner: rows.into(),
            skip_last_amt: 0,
        }
    }

    #[cfg(feature = "hrana")]
    pub(crate) fn new_skip_last(rows: Vec<Option<Rows>>, skip_last_amt: usize) -> Self {
        Self {
            inner: rows.into(),
            skip_last_amt,
        }
    }

    /// Get the next set of rows, it is wrapped in two options, if the first option returns `None`
    /// then the set of batch statement results has ended. If the inner option returns `None` then
    /// the statement was never executed (potentially due to a conditional).
    pub fn next_stmt_row(&mut self) -> Option<Option<Rows>> {
        if self.inner.len() <= self.skip_last_amt {
            return None;
        }

        self.inner.pop_front()
    }
}

impl fmt::Debug for BatchRows {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BatchRows").finish()
    }
}

/// A connection to some libsql database, this can be a remote one or a local one.
#[derive(Clone)]
pub struct Connection {
    pub(crate) conn: Arc<dyn Conn + Send + Sync>,
}

impl Connection {
    /// Execute sql query provided some type that implements [`IntoParams`] returning
    /// on success the number of rows that were changed.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # async fn run(conn: &libsql::Connection) {
    /// # use libsql::params;
    /// conn.execute("INSERT INTO foo (id) VALUES (?1)", [42]).await.unwrap();
    /// conn.execute("INSERT INTO foo (id, name) VALUES (?1, ?2)", params![42, "baz"]).await.unwrap();
    /// # }
    /// ```
    ///
    /// For more info on how to pass params check [`IntoParams`]'s docs.
    pub async fn execute(&self, sql: &str, params: impl IntoParams) -> Result<u64> {
        tracing::trace!("executing `{}`", sql);
        self.conn.execute(sql, params.into_params()?).await
    }

    /// Execute a batch set of statements.
    ///
    /// # Return
    ///
    /// This returns a `BatchRows` currently only the `remote` and `local` connection supports this feature and
    /// all other connection types will return an empty set always.
    pub async fn execute_batch(&self, sql: &str) -> Result<BatchRows> {
        tracing::trace!("executing batch `{}`", sql);
        self.conn.execute_batch(sql).await
    }

    /// Execute a batch set of statements atomically in a transaction.
    ///
    /// # Return
    ///
    /// This returns a `BatchRows` currently only the `remote` and `local` connection supports this feature and
    /// all other connection types will return an empty set always.
    pub async fn execute_transactional_batch(&self, sql: &str) -> Result<BatchRows> {
        tracing::trace!("executing batch transactional `{}`", sql);
        self.conn.execute_transactional_batch(sql).await
    }

    /// Execute sql query provided some type that implements [`IntoParams`] returning
    /// on success the [`Rows`].
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # async fn run(conn: &libsql::Connection) {
    /// # use libsql::params;
    /// conn.query("SELECT foo FROM bar WHERE id = ?1", [42]).await.unwrap();
    /// conn.query("SELECT foo FROM bar WHERE id = ?1 AND name = ?2", params![42, "baz"]).await.unwrap();
    /// # }
    /// ```
    /// For more info on how to pass params check [`IntoParams`]'s docs and on how to
    /// extract values out of the rows check the [`Rows`] docs.
    pub async fn query(&self, sql: &str, params: impl IntoParams) -> Result<Rows> {
        let stmt = self.prepare(sql).await?;

        stmt.query(params).await
    }

    /// Prepares a cached statement.
    pub async fn prepare(&self, sql: &str) -> Result<Statement> {
        tracing::trace!("preparing `{}`", sql);
        self.conn.prepare(sql).await
    }

    /// Begin a new transaction in `DEFERRED` mode, which is the default.
    pub async fn transaction(&self) -> Result<Transaction> {
        tracing::trace!("starting deferred transaction");
        self.transaction_with_behavior(TransactionBehavior::Deferred)
            .await
    }

    /// Begin a new transaction in the given [`TransactionBehavior`].
    pub async fn transaction_with_behavior(
        &self,
        tx_behavior: TransactionBehavior,
    ) -> Result<Transaction> {
        tracing::trace!("starting {:?} transaction", tx_behavior);
        self.conn.transaction(tx_behavior).await
    }

    /// Cancel ongoing operations and return at earliest opportunity.
    pub fn interrupt(&self) -> Result<()> {
        self.conn.interrupt()
    }

    pub fn busy_timeout(&self, timeout: Duration) -> Result<()> {
        self.conn.busy_timeout(timeout)
    }

    /// Check whether libsql is in `autocommit` or not.
    pub fn is_autocommit(&self) -> bool {
        self.conn.is_autocommit()
    }

    /// Check the amount of changes the last query created.
    pub fn changes(&self) -> u64 {
        self.conn.changes()
    }

    /// Check the total amount of changes the connection has done.
    pub fn total_changes(&self) -> u64 {
        self.conn.total_changes()
    }

    /// Check the last inserted row id.
    pub fn last_insert_rowid(&self) -> i64 {
        self.conn.last_insert_rowid()
    }

    pub async fn reset(&self) {
        self.conn.reset().await
    }

    pub fn set_reserved_bytes(&self, reserved_bytes: i32) -> Result<()> {
        self.conn.set_reserved_bytes(reserved_bytes)
    }

    pub fn get_reserved_bytes(&self) -> Result<i32> {
        self.conn.get_reserved_bytes()
    }

    /// Enable loading SQLite extensions from SQL queries and Rust API.
    ///
    /// See [`load_extension`](Connection::load_extension) documentation for more details.
    pub fn load_extension_enable(&self) -> Result<()> {
        self.conn.enable_load_extension(true)
    }

    /// Disable loading SQLite extensions from SQL queries and Rust API.
    ///
    /// See [`load_extension`](Connection::load_extension) documentation for more details.
    pub fn load_extension_disable(&self) -> Result<()> {
        self.conn.enable_load_extension(false)
    }

    /// Load a SQLite extension from a dynamic library at `dylib_path`, specifying optional
    /// entry point `entry_point`.
    ///
    /// # Security
    ///
    /// Loading extensions from dynamic libraries is a potential security risk, as it allows
    /// arbitrary code execution. Only load extensions that you trust.
    ///
    /// Extension loading is disabled by default. Please use the [`load_extension_enable`](Connection::load_extension_enable)
    /// method to enable it. It's recommended to disable extension loading after you're done
    /// loading extensions to avoid SQL injection attacks from loading extensions.
    ///
    /// See SQLite's documentation on `sqlite3_load_extension` for more information:
    /// https://sqlite.org/c3ref/load_extension.html
    pub fn load_extension<P: AsRef<Path>>(
        &self,
        dylib_path: P,
        entry_point: Option<&str>,
    ) -> Result<()> {
        self.conn.load_extension(dylib_path.as_ref(), entry_point)
    }

    pub fn authorizer(&self, hook: Option<AuthHook>) -> Result<()> {
        self.conn.authorizer(hook)
    }

    pub fn add_update_hook(&self, cb: Box<UpdateHook>) -> Result<()> {
        self.conn.add_update_hook(cb)
    }
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection").finish()
    }
}
