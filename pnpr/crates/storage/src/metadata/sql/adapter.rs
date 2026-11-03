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
//! - where a read runs, the primary or a replica ([`ReadSource`]).
//!
//! Every method blocks. The shared layer calls them on the blocking pool.

use pnpr_error::{RegistryError, Result};
use std::fmt;

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
pub(crate) trait Connection {
    /// Run one statement and report how many rows it changed.
    fn execute(&self, sql: &str, params: &[Param<'_>]) -> Result<usize>;

    /// Run one statement and return every row it produced.
    fn query(&self, sql: &str, params: &[Param<'_>]) -> Result<Vec<Row>>;

    /// Run statements that take no parameters, such as a schema.
    fn execute_batch(&self, sql: &str) -> Result<()>;

    /// Begin a transaction that holds the database's write lock from its
    /// first statement, so it reads what its commit replaces.
    fn begin_write(&self) -> Result<()>;

    /// Begin a transaction that reads one committed snapshot.
    fn begin_read(&self) -> Result<()>;

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

/// What differs between the databases the shared SQL layer runs on.
pub(crate) trait Adapter: fmt::Debug + Send + Sync {
    /// Run `operation` on the connection writes commit through. Writes of
    /// one process run one at a time.
    fn write(&self, operation: &mut dyn FnMut(&dyn Connection) -> Result<()>) -> Result<()>;

    /// Run `operation` on a connection that reads from `source`. Reads never
    /// wait for a write.
    fn read(
        &self,
        source: ReadSource,
        operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
    ) -> Result<()>;

    /// Whether `error` refused a write transaction for a conflict with
    /// another writer, after which the whole transaction may run again.
    fn is_retryable(&self, error: &RegistryError) -> bool;
}

/// A value bound to a statement's parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Param<'value> {
    Null,
    Integer(i64),
    Text(&'value str),
    Blob(&'value [u8]),
}

/// A type that binds to a statement's parameter.
pub(crate) trait ToParam {
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
    fn to_param(&self) -> Param<'_> {
        Param::Integer(*self)
    }
}

impl ToParam for [u8] {
    fn to_param(&self) -> Param<'_> {
        Param::Blob(self)
    }
}

impl ToParam for Vec<u8> {
    fn to_param(&self) -> Param<'_> {
        Param::Blob(self)
    }
}

impl<Inner: ToParam> ToParam for Option<Inner> {
    fn to_param(&self) -> Param<'_> {
        self.as_ref().map_or(Param::Null, ToParam::to_param)
    }
}

impl<Inner: ToParam + ?Sized> ToParam for &Inner {
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
