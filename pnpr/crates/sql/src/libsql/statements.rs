//! The statements of one libSQL connection, held and sent to the primary in
//! batches.

use super::timed_out;
use crate::adapter::{Connection, Param, Query, Row, Value};
use pnpr_error::Result;
use std::cell::{Cell, RefCell};
use tokio::runtime::Handle;

/// What happened to a primary connection's Hrana stream during an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StreamState {
    Active,
    /// The current uncommitted transaction or unsent batch staged a write.
    StagedWrite,
    /// At least one write completed in autocommit mode on this connection, so
    /// an autocommit write or an earlier write transaction committed.
    Committed,
    /// A read of the primary finished; [`run`](super::run) sends its `COMMIT`.
    CommitLater,
    /// The primary rejected a batch as a stale stream before any write
    /// committed on this connection.
    LostBeforeCommit,
    /// The primary rejected a batch as a stale stream after an earlier write
    /// already committed on this connection.
    LostAfterCommit,
}

impl StreamState {
    pub(super) fn can_replay_write(self) -> bool {
        match self {
            Self::LostBeforeCommit => true,
            Self::Active
            | Self::StagedWrite
            | Self::Committed
            | Self::CommitLater
            | Self::LostAfterCommit => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StepKind {
    Read,
    Write,
    Begin,
    Commit,
    Rollback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TxStatus {
    Autocommit,
    InTransaction,
}

impl TxStatus {
    fn from_conn(conn: &libsql::Connection) -> Self {
        if conn.is_autocommit() { Self::Autocommit } else { Self::InTransaction }
    }
}

pub(super) struct HeldStatement {
    sql: String,
    params: Vec<libsql::Value>,
    step: StepKind,
}

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
    pub(super) held: RefCell<Vec<HeldStatement>>,
    pub(super) state: Cell<StreamState>,
}

impl Statements<'_> {
    /// Wait for `statement`, failing with [`timed_out`] past the deadline.
    fn wait<Output>(&self, statement: impl Future<Output = Result<Output>>) -> Result<Output> {
        let result = match self.deadline {
            Some(deadline) => self.runtime
                .block_on(tokio::time::timeout_at(deadline, statement))
                .unwrap_or_else(|_| Err(timed_out())),
            None => self.runtime.block_on(statement),
        };
        if let Err(ref error) = result
            && pnpr_libsql::is_stale_stream(error)
        {
            let lost = match self.state.get() {
                StreamState::Active
                | StreamState::StagedWrite
                | StreamState::CommitLater
                | StreamState::LostBeforeCommit => StreamState::LostBeforeCommit,
                StreamState::Committed | StreamState::LostAfterCommit => {
                    StreamState::LostAfterCommit
                }
            };
            self.state.set(lost);
        }
        result
    }

    fn advance_after_steps(&self, mut tx: TxStatus, steps: &[StepKind]) {
        if self.local || self.read {
            return;
        }
        let mut state = self.state.get();
        for &step in steps {
            state = step_transition(state, &mut tx, step);
        }
        self.state.set(state);
    }

    fn hold(&self, sql: &str, params: &[Param<'_>], step: StepKind) {
        self.held
            .borrow_mut()
            .push(HeldStatement { sql: sql.to_string(), params: to_params(params), step });
    }

    /// Send every held statement as one batch, and return the rows of each.
    /// An answer for another number of statements than were sent fails.
    pub(super) fn send(&self) -> Result<Vec<Vec<Row>>> {
        let held = std::mem::take(&mut *self.held.borrow_mut());
        if held.is_empty() {
            return Ok(Vec::new());
        }
        let tx = TxStatus::from_conn(self.conn);
        let (steps, statements): (Vec<_>, Vec<_>) = held
            .into_iter()
            .map(|item| (item.step, (item.sql, item.params)))
            .unzip();
        let results = self.wait(async {
            let mut batch = self.conn.execute_batch_with_params(statements).await?;
            self.advance_after_steps(tx, &steps);
            let mut results = Vec::new();
            while let Some(step) = batch.next_stmt_row() {
                results.push(match step {
                    Some(rows) => collect_rows(rows).await?,
                    None => Vec::new(),
                });
            }
            Ok(results)
        })?;
        if results.len() != steps.len() {
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
            self.hold(sql, params, classify_sql(sql));
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
            self.hold(query.sql, &query.params, classify_sql(query.sql));
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
        let tx = TxStatus::from_conn(self.conn);
        self.wait(async {
            self.conn.execute_batch(sql).await?;
            if !self.local && !self.read {
                self.state.set(script_transition(self.state.get(), tx, sql));
            }
            Ok(())
        })
    }

    fn begin_write(&self) -> Result<()> {
        if self.local {
            return self.execute("BEGIN IMMEDIATE", &[]);
        }
        self.hold("BEGIN IMMEDIATE", &[], StepKind::Begin);
        Ok(())
    }

    /// A replica connection sends every statement of a plain `BEGIN`
    /// transaction to the primary; `BEGIN READONLY` keeps it on the replica.
    fn begin_read(&self) -> Result<()> {
        if self.local {
            return self.execute_batch("BEGIN READONLY");
        }
        self.hold("BEGIN", &[], StepKind::Begin);
        Ok(())
    }

    /// A read whose `BEGIN` is still held never reached the primary, and one
    /// that did is ended by [`run`](super::run) after it answers.
    fn commit(&self) -> Result<()> {
        if self.local {
            return self.execute("COMMIT", &[]);
        }
        if self.read {
            let mut held = self.held.borrow_mut();
            if held.is_empty() {
                self.state.set(StreamState::CommitLater);
                return Ok(());
            }
            if let [only] = held.as_slice()
                && is_begin_step(only.step)
            {
                held.clear();
                return Ok(());
            }
        }
        self.hold("COMMIT", &[], StepKind::Commit);
        self.send().map(drop)
    }

    /// A transaction whose `BEGIN` is still held never reached the primary,
    /// and one the primary already ended needs no `ROLLBACK`.
    fn rollback(&self) -> Result<()> {
        if self.local {
            if self.conn.is_autocommit() {
                return Ok(());
            }
            return self.execute("ROLLBACK", &[]);
        }
        let held = std::mem::take(&mut *self.held.borrow_mut());
        let unsent = held.first().is_some_and(|first| is_begin_step(first.step));
        let stream_lost = match self.state.get() {
            StreamState::LostBeforeCommit | StreamState::LostAfterCommit => true,
            StreamState::Active
            | StreamState::StagedWrite
            | StreamState::Committed
            | StreamState::CommitLater => false,
        };
        if unsent || stream_lost || self.conn.is_autocommit() {
            if !stream_lost {
                self.state.set(rollback_state(self.state.get()));
            }
            return Ok(());
        }
        self.execute_batch("ROLLBACK")
    }
}

fn is_begin_step(step: StepKind) -> bool {
    match step {
        StepKind::Begin => true,
        StepKind::Read | StepKind::Write | StepKind::Commit | StepKind::Rollback => false,
    }
}

fn stage_write(state: StreamState) -> StreamState {
    match state {
        StreamState::Active => StreamState::StagedWrite,
        StreamState::StagedWrite
        | StreamState::Committed
        | StreamState::CommitLater
        | StreamState::LostBeforeCommit
        | StreamState::LostAfterCommit => state,
    }
}

fn commit_state(state: StreamState) -> StreamState {
    match state {
        StreamState::StagedWrite | StreamState::Committed => StreamState::Committed,
        StreamState::Active
        | StreamState::CommitLater
        | StreamState::LostBeforeCommit
        | StreamState::LostAfterCommit => state,
    }
}

fn rollback_state(state: StreamState) -> StreamState {
    match state {
        StreamState::StagedWrite => StreamState::Active,
        StreamState::Active
        | StreamState::Committed
        | StreamState::CommitLater
        | StreamState::LostBeforeCommit
        | StreamState::LostAfterCommit => state,
    }
}

pub(super) fn step_transition(
    state: StreamState,
    tx: &mut TxStatus,
    step: StepKind,
) -> StreamState {
    match step {
        StepKind::Read => state,
        StepKind::Begin => {
            *tx = TxStatus::InTransaction;
            state
        }
        StepKind::Write => match *tx {
            TxStatus::InTransaction => stage_write(state),
            TxStatus::Autocommit => StreamState::Committed,
        },
        StepKind::Commit => {
            *tx = TxStatus::Autocommit;
            commit_state(state)
        }
        StepKind::Rollback => {
            *tx = TxStatus::Autocommit;
            rollback_state(state)
        }
    }
}

pub(super) fn script_transition(state: StreamState, mut tx: TxStatus, sql: &str) -> StreamState {
    let body = skip_sql_trivia(sql)
        .trim_end()
        .trim_end_matches(';')
        .trim_end();
    if body.is_empty() {
        return state;
    }
    if !body.contains(';') {
        return step_transition(state, &mut tx, classify_sql(body));
    }
    StreamState::Committed
}

pub(super) fn classify_sql(sql: &str) -> StepKind {
    let trimmed = skip_sql_trivia(sql);
    let keyword = leading_word(trimmed);
    if keyword.eq_ignore_ascii_case("SELECT") {
        return StepKind::Read;
    }
    if keyword.eq_ignore_ascii_case("BEGIN") {
        return StepKind::Begin;
    }
    if keyword.eq_ignore_ascii_case("COMMIT") || keyword.eq_ignore_ascii_case("END") {
        return StepKind::Commit;
    }
    if keyword.eq_ignore_ascii_case("ROLLBACK") {
        return classify_rollback(&trimmed[keyword.len()..]);
    }
    StepKind::Write
}

fn classify_rollback(rest: &str) -> StepKind {
    let rest = skip_sql_trivia(rest);
    let next = leading_word(rest);
    let after = if next.eq_ignore_ascii_case("TRANSACTION") {
        leading_word(skip_sql_trivia(&rest[next.len()..]))
    } else {
        next
    };
    if after.is_empty() { StepKind::Rollback } else { StepKind::Read }
}

fn leading_word(sql: &str) -> &str {
    let end = sql
        .bytes()
        .position(|byte| !byte.is_ascii_alphanumeric() && byte != b'_')
        .unwrap_or(sql.len());
    &sql[..end]
}

fn skip_sql_trivia(mut sql: &str) -> &str {
    loop {
        sql = sql.trim_start();
        if let Some(rest) = sql.strip_prefix("--") {
            sql = rest
                .split_once('\n')
                .map(|(_, tail)| tail)
                .unwrap_or("");
        } else if let Some(rest) = sql.strip_prefix("/*") {
            sql = rest
                .split_once("*/")
                .map(|(_, tail)| tail)
                .unwrap_or("");
        } else {
            return sql;
        }
    }
}

fn to_params(params: &[Param<'_>]) -> Vec<libsql::Value> {
    params
        .iter()
        .map(|param| match *param {
            Param::Null | Param::NullInteger | Param::NullBlob => libsql::Value::Null,
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
