#![allow(dead_code)]

pub mod connection;

cfg_remote! {
    pub mod hyper;
}

mod cursor;
mod stream;
pub mod transaction;

use crate::hrana::cursor::{Cursor, Error, OwnedCursorStep};
use crate::hrana::stream::HranaStream;
use crate::parser::StmtKind;
use crate::{params::Params, ValueType};
use bytes::Bytes;
use futures::{Stream, StreamExt};
pub use libsql_hrana::proto;
use libsql_hrana::proto::{Batch, BatchResult, Col, Stmt, StmtResult};
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::task::{Context, Poll};

use super::rows::{ColumnsInner, RowInner, RowsInner};

pub(crate) type Result<T> = std::result::Result<T, HranaError>;

/// Information about the current session: the server-generated cookie
/// and the URL that should be used for further communication.
#[derive(Clone, Debug, Default)]
struct Cookie {
    baton: Option<String>,
    base_url: Option<String>,
}

pub trait HttpSend: Clone {
    type Stream: Stream<Item = std::io::Result<Bytes>> + Unpin;
    type Result: Future<Output = Result<Self::Stream>>;
    fn http_send(&self, url: Arc<str>, auth: Arc<str>, body: String) -> Self::Result;

    /// Schedule sending a HTTP post request without waiting for the completion.
    fn oneshot(self, url: Arc<str>, auth: Arc<str>, body: String);
}

pub enum HttpBody<S> {
    Body(Option<Bytes>),
    Stream(S),
}

impl<S> From<Bytes> for HttpBody<S> {
    fn from(value: Bytes) -> Self {
        HttpBody::Body(Some(value))
    }
}

impl<S> Stream for HttpBody<S>
where
    S: Stream<Item = std::io::Result<Bytes>> + Unpin,
{
    type Item = std::io::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.get_mut() {
            HttpBody::Body(bytes) => {
                if let Some(bytes) = bytes.take() {
                    Poll::Ready(Some(Ok(bytes)))
                } else {
                    Poll::Ready(None)
                }
            }
            HttpBody::Stream(stream) => {
                let pinned = Pin::new(stream);
                pinned.poll_next(cx)
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HranaError {
    #[error("unexpected response: `{0}`")]
    UnexpectedResponse(String),
    #[error("stream closed: `{0}`")]
    StreamClosed(String),
    #[error("stream error: `{0:?}`")]
    StreamError(proto::Error),
    #[error("cursor error: `{0}`")]
    CursorError(CursorResponseError),
    #[error("json error: `{0}`")]
    Json(#[from] serde_json::Error),
    #[error("http error: `{0}`")]
    Http(String),
    #[error("api error: `{0}`")]
    Api(String),
}

#[derive(Debug, thiserror::Error)]
pub enum CursorResponseError {
    #[error("cursor step {actual} arrived before step {expected} end message")]
    NotClosed { expected: u32, actual: u32 },
    #[error("error at step {step}: {error}")]
    StepError { step: u32, error: Error },
    #[error("cursor stream ended prematurely")]
    CursorClosed,
    #[error("cursor hasn't fetched any rows yet")]
    NoRowsFetched,
    #[error("{0}")]
    Other(String),
}

pub struct Statement<T>
where
    T: HttpSend,
{
    stream: HranaStream<T>,
    close_stream: bool,
    inner: Stmt,
    cols: Vec<String>,
}

impl<T> Statement<T>
where
    T: HttpSend + Send + Sync + 'static,
{
    pub(crate) async fn new(
        stream: HranaStream<T>,
        sql: String,
        want_rows: bool,
    ) -> crate::Result<Self> {
        let desc = stream.describe(&sql).await?;

        let cols: Vec<_> = desc.cols.into_iter().map(|col| col.name).collect();

        // in SQLite when a multiple statements are glued together into one string, only the first one is
        // executed and then a handle to continue execution is returned. However Hrana API doesn't allow
        // passing multi-statement strings, so we just pick first one.
        let mut parse = crate::parser::Statement::parse(&sql);
        match parse.next() {
            None => Err(crate::Error::Misuse(
                "no SQL statement provided".to_string(),
            )),
            Some(Err(e)) => Err(e),
            Some(Ok(stmt)) => {
                // if we're already in transaction scope (non-autocommit) or we're starting
                // a transaction, we DON'T want to close the stream
                let in_tx_scope = !stream.is_autocommit()
                    || matches!(stmt.kind, StmtKind::TxnBegin | StmtKind::TxnBeginReadOnly);
                // if we're at COMMIT/ROLLBACK statement, we DO want to close the stream
                let close_stream = !in_tx_scope || matches!(stmt.kind, StmtKind::TxnEnd);
                let inner = Stmt::new(stmt.stmt, want_rows);
                Ok(Statement {
                    stream,
                    close_stream,
                    inner,
                    cols,
                })
            }
        }
    }

    pub async fn execute(&self, params: &Params) -> crate::Result<usize> {
        let mut stmt = self.inner.clone();
        bind_params(params.clone(), &mut stmt);

        let result = self.stream.execute_inner(stmt, self.close_stream).await?;
        Ok(result.affected_row_count as usize)
    }

    pub async fn run(&self, params: &Params) -> crate::Result<()> {
        let mut stmt = self.inner.clone();
        bind_params(params.clone(), &mut stmt);

        let _ = self.stream.execute_inner(stmt, self.close_stream).await?;
        Ok(())
    }

    pub(crate) async fn query_raw(
        &self,
        params: &Params,
    ) -> crate::Result<HranaRows<T::Stream, T>> {
        let mut stmt = self.inner.clone();
        bind_params(params.clone(), &mut stmt);

        let cursor = self.stream.cursor(Batch::single(stmt)).await?;
        let rows = HranaRows::from_cursor(cursor, self.stream.clone()).await?;

        Ok(rows)
    }
}

impl<T> Statement<T>
where
    T: HttpSend + Send + Sync + 'static,
    <T as HttpSend>::Stream: Send + Sync + 'static,
{
    pub async fn query(&self, params: &Params) -> crate::Result<super::Rows> {
        let rows = self.query_raw(params).await?;
        Ok(super::Rows::new(rows))
    }
}

pub struct HranaRows<S, T: HttpSend> {
    cursor_step: OwnedCursorStep<S>,
    column_types: Option<Vec<ValueType>>,
    stream: HranaStream<T>,
}

impl<S, T> HranaRows<S, T>
where
    T: HttpSend + Send + Sync + 'static,
    S: Stream<Item = std::io::Result<Bytes>> + Unpin,
{
    async fn from_cursor(cursor: Cursor<S>, stream: HranaStream<T>) -> Result<Self> {
        let cursor_step = cursor.next_step_owned().await?;
        Ok(HranaRows {
            cursor_step,
            column_types: None,
            stream,
        })
    }

    pub async fn next(&mut self) -> crate::Result<Option<super::Row>> {
        let row = match self.cursor_step.next().await {
            Some(Ok(row)) => row,
            Some(Err(e)) => return Err(crate::Error::Hrana(Box::new(e))),
            None => {
                self.stream
                    .inner
                    .affected_row_count
                    .store(self.cursor_step.affected_rows().into(), Ordering::SeqCst);
                return Ok(None);
            }
        };

        if self.column_types.is_none() {
            self.init_column_types(&row);
        }

        Ok(Some(super::Row {
            inner: Box::new(row),
        }))
    }

    fn init_column_types(&mut self, row: &Row) {
        self.column_types = Some(
            row.inner
                .iter()
                .map(|value| match value {
                    proto::Value::Null | proto::Value::None => ValueType::Null,
                    proto::Value::Integer { value: _ } => ValueType::Integer,
                    proto::Value::Float { value: _ } => ValueType::Real,
                    proto::Value::Text { value: _ } => ValueType::Text,
                    proto::Value::Blob { value: _ } => ValueType::Blob,
                })
                .collect(),
        );
    }

    pub fn column_count(&self) -> i32 {
        self.cursor_step.cols().len() as i32
    }

    pub fn column_name(&self, idx: i32) -> Option<&str> {
        self.cursor_step
            .cols()
            .get(idx as usize)
            .and_then(|c| c.name.as_ref())
            .map(|s| s.as_str())
    }
}

#[async_trait::async_trait]
impl<S, T> RowsInner for HranaRows<S, T>
where
    T: HttpSend + Send + Sync + 'static,
    S: Stream<Item = std::io::Result<Bytes>> + Send + Sync + Unpin,
{
    async fn next(&mut self) -> crate::Result<Option<super::Row>> {
        self.next().await
    }
}

impl<S, T> ColumnsInner for HranaRows<S, T>
where
    T: HttpSend + Send + Sync + 'static,
    S: Stream<Item = std::io::Result<Bytes>> + Send + Sync + Unpin,
{
    fn column_count(&self) -> i32 {
        self.column_count()
    }

    fn column_name(&self, idx: i32) -> Option<&str> {
        self.column_name(idx)
    }

    fn column_type(&self, idx: i32) -> crate::Result<ValueType> {
        if let Some(col_types) = &self.column_types {
            if let Some(t) = col_types.get(idx as usize) {
                Ok(*t)
            } else {
                Err(crate::Error::ColumnNotFound(idx))
            }
        } else {
            Err(crate::Error::Hrana(Box::new(HranaError::CursorError(
                CursorResponseError::NoRowsFetched,
            ))))
        }
    }
}

#[derive(Debug)]
pub struct Row {
    cols: Arc<[Col]>,
    inner: Vec<proto::Value>,
}

impl Row {
    pub(super) fn new(cols: Arc<[Col]>, inner: Vec<proto::Value>) -> Self {
        Row { cols, inner }
    }
}

impl RowInner for Row {
    fn column_value(&self, idx: i32) -> crate::Result<crate::Value> {
        let v = self.inner.get(idx as usize).cloned().unwrap();
        Ok(into_value2(v))
    }

    fn column_str(&self, idx: i32) -> crate::Result<&str> {
        if let Some(value) = self.inner.get(idx as usize) {
            if let proto::Value::Text { value } = value {
                Ok(value)
            } else {
                Err(crate::Error::InvalidColumnType)
            }
        } else {
            Err(crate::Error::ColumnNotFound(idx))
        }
    }
}

impl ColumnsInner for Row {
    fn column_name(&self, idx: i32) -> Option<&str> {
        self.cols
            .get(idx as usize)
            .and_then(|c| c.name.as_ref())
            .map(|s| s.as_str())
    }

    fn column_type(&self, idx: i32) -> crate::Result<ValueType> {
        if let Some(value) = self.inner.get(idx as usize) {
            Ok(match value {
                proto::Value::Null => ValueType::Null,
                proto::Value::Integer { value: _ } => ValueType::Integer,
                proto::Value::Float { value: _ } => ValueType::Real,
                proto::Value::Text { value: _ } => ValueType::Text,
                proto::Value::Blob { value: _ } => ValueType::Blob,
                proto::Value::None => return Err(crate::Error::InvalidColumnType),
            })
        } else {
            Err(crate::Error::ColumnNotFound(idx))
        }
    }

    fn column_count(&self) -> i32 {
        self.cols.len() as i32
    }
}

pub(super) fn bind_params(params: Params, stmt: &mut Stmt) {
    match params {
        Params::None => {}
        Params::Positional(values) => {
            for value in values {
                stmt.bind(into_value(value));
            }
        }
        Params::Named(values) => {
            for (name, value) in values {
                stmt.bind_named(name, into_value(value));
            }
        }
    }
}

fn into_value(value: crate::Value) -> proto::Value {
    match value {
        crate::Value::Null => proto::Value::Null,
        crate::Value::Integer(value) => proto::Value::Integer { value },
        crate::Value::Real(value) => proto::Value::Float { value },
        crate::Value::Text(value) => proto::Value::Text {
            value: value.into(),
        },
        crate::Value::Blob(value) => proto::Value::Blob {
            value: value.into(),
        },
    }
}

fn into_value2(value: proto::Value) -> crate::Value {
    match value {
        proto::Value::Null | proto::Value::None => crate::Value::Null,
        proto::Value::Integer { value } => crate::Value::Integer(value),
        proto::Value::Float { value } => crate::Value::Real(value),
        proto::Value::Text { value } => crate::Value::Text(value.to_string()),
        proto::Value::Blob { value } => crate::Value::Blob(value.into()),
    }
}

pub(crate) fn unwrap_err(batch_res: &BatchResult) -> crate::Result<()> {
    batch_res
        .step_errors
        .iter()
        .find_map(|e| e.clone())
        .map(|e| Err(crate::Error::Hrana(Box::new(HranaError::Api(e.message)))))
        .unwrap_or(Ok(()))
}

struct StmtResultRows {
    cols: Arc<[Col]>,
    rows: VecDeque<libsql_hrana::proto::Row>,
}

impl StmtResultRows {
    pub(crate) fn new(stmt: StmtResult) -> Self {
        Self {
            rows: stmt.rows.into(),
            cols: stmt.cols.into(),
        }
    }
}

#[async_trait::async_trait]
impl RowsInner for StmtResultRows {
    async fn next(&mut self) -> crate::Result<Option<super::Row>> {
        let Some(row) = self.rows.pop_front() else {
            return Ok(None);
        };
        let row = Row::new(self.cols.clone(), row.values);

        Ok(Some(super::Row {
            inner: Box::new(row),
        }))
    }
}

impl ColumnsInner for StmtResultRows {
    fn column_count(&self) -> i32 {
        self.cols.len() as i32
    }

    fn column_name(&self, idx: i32) -> Option<&str> {
        self.cols
            .get(idx as usize)
            .and_then(|r| r.name.as_ref())
            .map(|n| n.as_str())
    }

    fn column_type(&self, idx: i32) -> crate::Result<ValueType> {
        self.cols
            .get(idx as usize)
            .and_then(|r| r.decltype.as_ref())
            .ok_or(crate::Error::InvalidColumnType)
            .and_then(|v| v.parse().map_err(|_| crate::Error::InvalidColumnType))
    }
}
