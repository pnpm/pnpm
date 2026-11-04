//! The embedded `SQLite` adapter.
//!
//! Writes share one connection behind a mutex and take `BEGIN IMMEDIATE`, so
//! commits are serialized. Reads use connections of their own, so a read
//! never waits for a commit or for another read: in WAL mode each read
//! transaction sees one committed snapshot while a write is in progress. The
//! database has no replica, so every read source is the database itself.

use super::adapter::{Adapter, Connection, Param, ReadSource, Row, Value};
use pnpr_error::{RegistryError, Result};
use rusqlite::{
    ErrorCode, ToSql,
    types::{ToSqlOutput, ValueRef},
};
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, PoisonError},
    time::Duration,
};

/// Read connections kept open between reads. More concurrent reads open
/// more, and close them when they are done.
const IDLE_READERS: usize = 8;

/// How long a statement waits for another process's write lock before it
/// fails as busy.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// A metadata database in one `SQLite` file.
#[derive(Debug)]
pub(crate) struct SqliteAdapter {
    path: PathBuf,
    /// The connection every write commits through.
    writer: Mutex<rusqlite::Connection>,
    /// Read connections not in use.
    readers: Mutex<Vec<rusqlite::Connection>>,
}

impl SqliteAdapter {
    /// Open the database at `path`, creating it and its directory if needed.
    pub(crate) fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = rusqlite::Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        Ok(Self {
            path: path.to_path_buf(),
            writer: Mutex::new(connection),
            readers: Mutex::new(Vec::new()),
        })
    }

    /// Open a connection that only reads. The database is in WAL mode
    /// already, so its reads see committed snapshots and never wait for a
    /// writer.
    fn open_reader(&self) -> rusqlite::Result<rusqlite::Connection> {
        let connection = rusqlite::Connection::open(&self.path)?;
        connection.pragma_update(None, "query_only", "ON")?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        Ok(connection)
    }
}

impl Adapter for SqliteAdapter {
    fn write(&self, operation: &mut dyn FnMut(&dyn Connection) -> Result<()>) -> Result<()> {
        let writer = lock(&self.writer);
        // A write whose `COMMIT` and `ROLLBACK` both failed leaves its
        // transaction open, and `BEGIN IMMEDIATE` would fail inside it.
        if !writer.is_autocommit() {
            writer.execute_batch("ROLLBACK")?;
        }
        operation(&*writer)
    }

    fn read(
        &self,
        _source: ReadSource,
        operation: &mut dyn FnMut(&dyn Connection) -> Result<()>,
    ) -> Result<()> {
        let idle = lock(&self.readers).pop();
        let connection = match idle {
            Some(connection) => connection,
            None => self.open_reader()?,
        };
        let output = operation(&connection);
        // A read whose `COMMIT` or `ROLLBACK` failed may still be in its
        // transaction. Such a connection is closed rather than reused.
        if connection.is_autocommit() {
            let mut readers = lock(&self.readers);
            if readers.len() < IDLE_READERS {
                readers.push(connection);
            }
        }
        output
    }

    fn is_retryable(&self, error: &RegistryError) -> bool {
        matches!(
            error,
            RegistryError::Sqlite(rusqlite::Error::SqliteFailure(failure, _))
                if matches!(failure.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked),
        )
    }
}

fn lock<Value>(mutex: &Mutex<Value>) -> MutexGuard<'_, Value> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Connection for rusqlite::Connection {
    fn execute(&self, sql: &str, params: &[Param<'_>]) -> Result<()> {
        let mut statement = self.prepare_cached(sql)?;
        statement.execute(rusqlite::params_from_iter(params))?;
        Ok(())
    }

    fn query(&self, sql: &str, params: &[Param<'_>]) -> Result<Vec<Row>> {
        let mut statement = self.prepare_cached(sql)?;
        let columns = statement.column_count();
        let mut rows = statement.query(rusqlite::params_from_iter(params))?;
        let mut collected = Vec::new();
        while let Some(row) = rows.next()? {
            let values = (0..columns)
                .map(|index| Ok(to_value(row.get_ref(index)?)))
                .collect::<rusqlite::Result<_>>()?;
            collected.push(Row::new(values));
        }
        Ok(collected)
    }

    fn execute_batch(&self, sql: &str) -> Result<()> {
        Ok(rusqlite::Connection::execute_batch(self, sql)?)
    }

    fn begin_write(&self) -> Result<()> {
        Connection::execute_batch(self, "BEGIN IMMEDIATE")
    }

    fn begin_read(&self) -> Result<()> {
        Connection::execute_batch(self, "BEGIN")
    }
}

impl ToSql for Param<'_> {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(match *self {
            Self::Null => ValueRef::Null,
            Self::Integer(value) => ValueRef::Integer(value),
            Self::Text(value) => ValueRef::Text(value.as_bytes()),
            Self::Blob(value) => ValueRef::Blob(value),
        }))
    }
}

fn to_value(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => Value::Integer(value),
        ValueRef::Real(value) => Value::Real(value),
        ValueRef::Text(value) => Value::Text(String::from_utf8_lossy(value).into_owned()),
        ValueRef::Blob(value) => Value::Blob(value.to_vec()),
    }
}
