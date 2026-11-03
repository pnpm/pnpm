//! The statements of one libSQL connection, held and sent to the primary in
//! batches.

use super::{
    super::adapter::{Connection, Param, Query, Row, Value},
    timed_out,
};
use pnpr_error::Result;
use std::cell::{Cell, RefCell};
use tokio::runtime::Handle;

/// One libSQL connection, run from a blocking thread.
///
/// A connection to the primary sends its statements in batches, one HTTP
/// round trip each: a statement whose answer is not needed, and the one that
/// begins a transaction, waits for the next statement that answers, or for
/// the commit. Each step of a batch runs only when the one before it
/// succeeded, so a failed `BEGIN IMMEDIATE` runs nothing after it, and a
/// failed write leaves the commit after it unrun. A connection to the
/// embedded replica reads a local file and runs each statement as it comes.
pub(super) struct Statements<'conn> {
    pub(super) runtime: &'conn Handle,
    pub(super) conn: &'conn libsql::Connection,
    /// Whether the connection reads the embedded replica.
    pub(super) local: bool,
    /// When a read's statements stop waiting for the database. A write's
    /// have none.
    pub(super) deadline: Option<tokio::time::Instant>,
    /// Whether the connection runs a read rather than a write.
    pub(super) read: bool,
    /// The statements waiting for the next batch to the primary.
    pub(super) held: RefCell<Vec<(String, Vec<libsql::Value>)>>,
    /// Set by the commit of a read of the primary, which [`run`](super::run) sends once
    /// the read has its answer.
    pub(super) end_later: Cell<bool>,
}

impl Statements<'_> {
    /// Wait for `statement`, failing with [`timed_out`] past the deadline.
    fn wait<Output>(&self, statement: impl Future<Output = Result<Output>>) -> Result<Output> {
        match self.deadline {
            Some(deadline) => self.runtime
                .block_on(tokio::time::timeout_at(deadline, statement))
                .unwrap_or_else(|_| Err(timed_out())),
            None => self.runtime.block_on(statement),
        }
    }

    fn hold(&self, sql: &str, params: &[Param<'_>]) {
        self.held
            .borrow_mut()
            .push((sql.to_string(), to_params(params)));
    }

    /// Send every held statement as one batch, and return the rows of each.
    /// An answer for another number of statements than were sent fails.
    pub(super) fn send(&self) -> Result<Vec<Vec<Row>>> {
        let statements = std::mem::take(&mut *self.held.borrow_mut());
        if statements.is_empty() {
            return Ok(Vec::new());
        }
        let sent = statements.len();
        let results = self.wait(async {
            let mut batch = self.conn.execute_batch_with_params(statements).await?;
            let mut results = Vec::new();
            while let Some(step) = batch.next_stmt_row() {
                results.push(match step {
                    Some(rows) => collect_rows(rows).await?,
                    None => Vec::new(),
                });
            }
            Ok(results)
        })?;
        if results.len() != sent {
            return Err(pnpr_error::RegistryError::Internal {
                reason: "the libSQL primary did not answer every statement of a batch".to_string(),
            });
        }
        Ok(results)
    }
}

async fn collect_rows(mut rows: libsql::Rows) -> Result<Vec<Row>> {
    let columns = rows.column_count();
    let mut collected = Vec::new();
    while let Some(row) = rows.next().await? {
        let values = (0..columns)
            .map(|index| row.get_value(index).map(to_value))
            .collect::<libsql::Result<_>>()?;
        collected.push(Row::new(values));
    }
    Ok(collected)
}

impl Connection for Statements<'_> {
    fn execute(&self, sql: &str, params: &[Param<'_>]) -> Result<()> {
        if !self.local {
            self.hold(sql, params);
            return Ok(());
        }
        self.wait(async { Ok(self.conn.execute(sql, to_params(params)).await?) })?;
        Ok(())
    }

    fn query(&self, sql: &str, params: &[Param<'_>]) -> Result<Vec<Row>> {
        if !self.local {
            let query = Query::new(sql, params);
            return Ok(self
                .query_all(&[query])?
                .pop()
                .unwrap_or_default());
        }
        self.wait(async { collect_rows(self.conn.query(sql, to_params(params)).await?).await })
    }

    fn query_all(&self, queries: &[Query<'_>]) -> Result<Vec<Vec<Row>>> {
        if self.local {
            return queries
                .iter()
                .map(|query| self.query(query.sql, &query.params))
                .collect();
        }
        for query in queries {
            self.hold(query.sql, &query.params);
        }
        // `send` answers every statement it sent, the held ones before these.
        let mut results = self.send()?;
        let held = results.len() - queries.len();
        Ok(results.split_off(held))
    }

    /// Runs `sql` as a batch of its own: libsql renders the statements of a
    /// script again, and its rendering can differ from them.
    fn execute_batch(&self, sql: &str) -> Result<()> {
        self.send()?;
        self.wait(async {
            self.conn.execute_batch(sql).await?;
            Ok(())
        })
    }

    fn begin_write(&self) -> Result<()> {
        self.hold("BEGIN IMMEDIATE", &[]);
        Ok(())
    }

    /// A replica connection sends every statement of a plain `BEGIN`
    /// transaction to the primary; `BEGIN READONLY` keeps it on the replica.
    fn begin_read(&self) -> Result<()> {
        if self.local {
            return self.execute_batch("BEGIN READONLY");
        }
        self.hold("BEGIN", &[]);
        Ok(())
    }

    /// A read whose `BEGIN` is still held never reached the primary, and one
    /// that did is ended by [`run`](super::run) after it answers.
    fn commit(&self) -> Result<()> {
        if self.local {
            return self.execute_batch("COMMIT");
        }
        if self.read {
            let mut held = self.held.borrow_mut();
            if held.is_empty() {
                self.end_later.set(true);
                return Ok(());
            }
            if let [(begin, _)] = held.as_slice()
                && begin.starts_with("BEGIN")
            {
                held.clear();
                return Ok(());
            }
        }
        self.hold("COMMIT", &[]);
        self.send().map(drop)
    }

    /// A transaction whose `BEGIN` is still held never reached the primary,
    /// and one the primary already ended needs no `ROLLBACK`.
    fn rollback(&self) -> Result<()> {
        let held = std::mem::take(&mut *self.held.borrow_mut());
        let unsent = held
            .first()
            .is_some_and(|(sql, _)| sql.starts_with("BEGIN"));
        if unsent || (!self.local && self.conn.is_autocommit()) {
            return Ok(());
        }
        self.execute_batch("ROLLBACK")
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
