//! What the shared SQL layer needs from a database.
//!
//! The shared layer runs every statement through [`Connection`], with `?N`
//! placeholders and the [`Param`] and [`Value`] types, in the subset of SQL
//! that `SQLite`, libSQL, and `PostgreSQL` share. An [`Adapter`] supplies what
//! differs between the databases:
//!
//! - the connections, opened from its configuration;
//! - the statements that begin a write or a read transaction;
//! - which errors are conflicts that a retry of the whole transaction clears;
//! - placeholders and column types, translated by its [`Connection`];
//! - where a read runs, the primary or a replica ([`ReadSource`]);
//! - how statements reach the database: one at a time, or a remote
//!   database's in batches of one round trip each.
//!
//! Every method blocks. The shared layer calls them on the blocking pool.

use pnpr_error::{RegistryError, Result};
use std::{fmt, time::Instant};

/// The database a read runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadSource {
    /// The database every write commits to.
    Primary,
    /// A copy of the primary that may lag it. An adapter without one reads
    /// the primary.
    Replica,
}

/// A database connection of an [`Adapter`].
///
/// A connection to a remote database may hold a statement that returns
/// nothing, and the statement that begins a transaction, until a statement
/// whose answer the caller needs, and send them together in one round trip.
/// The error of a held statement then surfaces from that later call, which
/// fails the transaction just the same.
pub(crate) trait Connection {
    fn lock_schema(&self) -> Result<()> {
        Ok(())
    }

    fn recover_schema_initialization(&self) -> Result<bool> {
        Ok(false)
    }

    fn create_schema(&self) -> Result<()> {
        super::schema::create(self)
    }

    fn schema_tables(&self) -> String {
        super::schema::TABLES.to_string()
    }

    fn schema_table_names(&self) -> Result<Vec<Row>> {
        self.query("SELECT name FROM sqlite_master WHERE type = 'table'", &[])
    }

    fn schema_marker(&self) -> &'static str {
        super::schema::MARKER_TABLE
    }

    /// Run one statement whose rows and count the caller does not need. It
    /// may run as late as the next call that answers.
    fn execute(&self, sql: &str, params: &[Param<'_>]) -> Result<()>;

    /// Run one statement and return every row it produced.
    fn query(&self, sql: &str, params: &[Param<'_>]) -> Result<Vec<Row>>;

    /// Run `queries` in order and return the rows of each. Remote adapters
    /// combine compatible queries into one round trip.
    fn query_all(&self, queries: &[Query<'_>]) -> Result<Vec<Vec<Row>>> {
        queries
            .iter()
            .map(|query| self.query(query.sql, &query.params))
            .collect()
    }

    /// Run statements that take no parameters, such as a schema, after
    /// every statement held so far.
    fn execute_batch(&self, sql: &str) -> Result<()>;

    /// Begin a serializable write transaction. A conflict may abort it for retry.
    fn begin_write(&self) -> Result<()>;

    /// Begin a transaction that reads one committed snapshot.
    fn begin_read(&self) -> Result<()>;

    /// Commit the transaction, with every statement held so far.
    fn commit(&self) -> Result<()> {
        self.execute_batch("COMMIT")
    }

    /// Roll the transaction back, dropping every statement held so far.
    fn rollback(&self) -> Result<()> {
        self.execute_batch("ROLLBACK")
    }

    /// The first row of the statement's result, if any.
    fn query_optional(&self, sql: &str, params: &[Param<'_>]) -> Result<Option<Row>> {
        Ok(self
            .query(sql, params)?
            .into_iter()
            .next())
    }

    /// The first row of the statement's result, which must have one.
    fn query_one(&self, sql: &str, params: &[Param<'_>]) -> Result<Row> {
        self.query_optional(sql, params)?
            .ok_or_else(|| RegistryError::Internal {
                reason: "a metadata query that always returns a row returned none".to_string(),
            })
    }
}

/// A statement whose rows [`Connection::query_all`] returns.
#[derive(Debug, Clone)]
pub(crate) struct Query<'query> {
    pub(crate) sql: &'query str,
    pub(crate) params: Vec<Param<'query>>,
}

impl<'query> Query<'query> {
    pub(crate) fn new(sql: &'query str, params: &[Param<'query>]) -> Self {
        Self { sql, params: params.to_vec() }
    }
}

/// What differs between the databases the shared SQL layer runs on.
pub(crate) trait Adapter: fmt::Debug + Send + Sync {
    /// Run schema initialization, using startup deadlines when the adapter has them.
    fn initialize(&self, operation: &mut dyn FnMut(&dyn Connection) -> Result<()>) -> Result<()> {
        self.write(operation)
    }

    /// Run `operation` on a primary connection.
    fn write(&self, operation: &mut dyn FnMut(&dyn Connection) -> Result<()>) -> Result<()>;

    /// Run `operation` on a connection that reads from `source`.
    fn read(
        &self,
        source: ReadSource,
        operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
    ) -> Result<()>;

    /// Whether `error` refused a write transaction for a conflict with
    /// another writer, after which the whole transaction may run again.
    fn is_retryable(&self, error: &RegistryError) -> bool;

    /// Whether [`ReadSource::Replica`] reads a copy that may lag the primary.
    fn has_replica(&self) -> bool {
        false
    }

    /// Bring the replica up to date with the primary, unless a sync that
    /// started at or after `since` already did. A replica that cannot sync is left
    /// stale, so its reads sync first or go to the primary.
    fn sync_replica(&self, since: Instant) -> Result<()> {
        let _ = since;
        Ok(())
    }

    /// Whether `error` came from the database rather than from the
    /// operation, so the same read may succeed on another source.
    fn is_database_failure(&self, error: &RegistryError) -> bool {
        let _ = error;
        false
    }
}

/// A value bound to a statement's parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Param<'value> {
    Null,
    NullInteger,
    NullBlob,
    Integer(i64),
    Text(&'value str),
    Blob(&'value [u8]),
}

/// A type that binds to a statement's parameter.
pub(crate) trait ToParam {
    fn null_param() -> Param<'static> {
        Param::Null
    }

    fn to_param(&self) -> Param<'_>;
}

impl ToParam for str {
    fn to_param(&self) -> Param<'_> {
        Param::Text(self)
    }
}

impl ToParam for String {
    fn to_param(&self) -> Param<'_> {
        Param::Text(self)
    }
}

impl ToParam for i64 {
    fn null_param() -> Param<'static> {
        Param::NullInteger
    }

    fn to_param(&self) -> Param<'_> {
        Param::Integer(*self)
    }
}

impl ToParam for [u8] {
    fn null_param() -> Param<'static> {
        Param::NullBlob
    }

    fn to_param(&self) -> Param<'_> {
        Param::Blob(self)
    }
}

impl ToParam for Vec<u8> {
    fn null_param() -> Param<'static> {
        Param::NullBlob
    }

    fn to_param(&self) -> Param<'_> {
        Param::Blob(self)
    }
}

impl<Inner: ToParam> ToParam for Option<Inner> {
    fn null_param() -> Param<'static> {
        Inner::null_param()
    }

    fn to_param(&self) -> Param<'_> {
        match self {
            Some(value) => value.to_param(),
            None => Inner::null_param(),
        }
    }
}

impl<Inner: ToParam + ?Sized> ToParam for &Inner {
    fn null_param() -> Param<'static> {
        Inner::null_param()
    }

    fn to_param(&self) -> Param<'_> {
        (**self).to_param()
    }
}

/// A value a statement returned.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) enum Value {
    #[default]
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

/// A row a statement returned.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row(Vec<Value>);

impl Row {
    pub(crate) fn new(values: Vec<Value>) -> Self {
        Self(values)
    }

    /// Take the value of column `index` out of the row.
    pub(crate) fn get<Column: FromValue>(&mut self, index: usize) -> Result<Column> {
        let value = self.0
            .get_mut(index)
            .map(std::mem::take)
            .ok_or_else(|| column_error(index, "missing"))?;
        Column::from_value(value).map_err(|value| column_error(index, &format!("{value:?}")))
    }
}

fn column_error(index: usize, found: &str) -> RegistryError {
    RegistryError::Internal { reason: format!("metadata column {index} holds {found}") }
}

/// A type a returned column converts to. Gives the value back when it holds
/// another type.
pub(crate) trait FromValue: Sized {
    fn from_value(value: Value) -> Result<Self, Value>;
}

impl FromValue for i64 {
    fn from_value(value: Value) -> Result<Self, Value> {
        match value {
            Value::Integer(value) => Ok(value),
            other => Err(other),
        }
    }
}

impl FromValue for String {
    fn from_value(value: Value) -> Result<Self, Value> {
        match value {
            Value::Text(value) => Ok(value),
            other => Err(other),
        }
    }
}

impl FromValue for Vec<u8> {
    fn from_value(value: Value) -> Result<Self, Value> {
        match value {
            Value::Blob(value) => Ok(value),
            other => Err(other),
        }
    }
}

impl<Inner: FromValue> FromValue for Option<Inner> {
    fn from_value(value: Value) -> Result<Self, Value> {
        match value {
            Value::Null => Ok(None),
            other => Inner::from_value(other).map(Some),
        }
    }
}

impl FromValue for Value {
    fn from_value(value: Value) -> Result<Self, Value> {
        Ok(value)
    }
}
