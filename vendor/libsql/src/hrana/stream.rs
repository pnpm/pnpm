use crate::hrana::cursor::{Cursor, CursorReq};
use crate::hrana::proto::{Batch, BatchResult, DescribeResult, Stmt, StmtResult};
use crate::hrana::{CursorResponseError, HranaError, HttpSend, Result};
use bytes::{Bytes, BytesMut};
use futures::Stream;
use libsql_hrana::proto::{
    BatchStreamReq, CloseSqlStreamReq, CloseStreamReq, CloseStreamResp, DescribeStreamReq,
    GetAutocommitStreamReq, PipelineReqBody, PipelineRespBody, SequenceStreamReq,
    StoreSqlStreamReq, StreamRequest, StreamResponse, StreamResult,
};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

macro_rules! unexpected {
    ($value:ident) => {
        return Err(HranaError::UnexpectedResponse(format!(
            "Unexpected response from server: {:?}",
            $value
        )))
    };
}

pub type SqlId = i32;

/// A representation of Hrana HTTP stream. Since streams rely on sequential execution of requests,
/// it's realized internally as a mutex lock.
#[derive(Debug)]
pub struct HranaStream<T>
where
    T: HttpSend,
{
    pub inner: Arc<Inner<T>>,
}

impl<T> Clone for HranaStream<T>
where
    T: HttpSend,
{
    fn clone(&self) -> Self {
        HranaStream {
            inner: self.inner.clone(),
        }
    }
}

impl<T> HranaStream<T>
where
    T: HttpSend,
{
    pub(super) fn open(
        client: T,
        pipeline_url: Arc<str>,
        cursor_url: Arc<str>,
        auth_token: Arc<str>,
    ) -> Self {
        tracing::trace!("opening stream");
        HranaStream {
            inner: Arc::new(Inner {
                affected_row_count: AtomicU64::new(0),
                total_changes: AtomicU64::new(0),
                last_insert_rowid: AtomicI64::new(0),
                is_autocommit: AtomicBool::new(true),
                stream: Mutex::new(RawStream {
                    client,
                    pipeline_url,
                    cursor_url,
                    auth_token,
                    sql_id_generator: RefCell::new(0),
                    baton: RefCell::new(None),
                }),
            }),
        }
    }

    /// Executes a final request and immediately closes current stream - all in one request
    /// Returns true if request was finalized correctly, false if stream was already closed.
    pub(super) async fn finalize(&mut self, req: StreamRequest) -> Result<bool> {
        let mut client = self.inner.stream.lock().await;
        if client.baton.borrow().is_none() {
            tracing::trace!("baton not found - skipping finalize for {:?}", req);
            return Ok(false);
        }
        let (resp, is_autocommit) = client.finalize(req).await?;
        self.inner
            .is_autocommit
            .store(is_autocommit, Ordering::SeqCst);
        let (affected_row_count, last_insert_rowid) = if let StreamResponse::Execute(resp) = resp {
            (
                resp.result.affected_row_count,
                resp.result.last_insert_rowid.unwrap_or(0),
            )
        } else {
            (0, 0)
        };

        self.inner
            .total_changes
            .fetch_add(affected_row_count, Ordering::SeqCst);
        self.inner
            .affected_row_count
            .store(affected_row_count, Ordering::SeqCst);
        self.inner
            .last_insert_rowid
            .store(last_insert_rowid, Ordering::SeqCst);
        Ok(true)
    }

    pub(super) async fn execute_inner(&self, stmt: Stmt, close_stream: bool) -> Result<StmtResult> {
        let mut batch_res = self.batch_inner(Batch::single(stmt), close_stream).await?;
        if let Some(Some(error)) = batch_res.step_errors.pop() {
            return Err(HranaError::StreamError(error));
        }
        if let Some(Some(resp)) = batch_res.step_results.pop() {
            Ok(resp)
        } else {
            Err(HranaError::CursorError(CursorResponseError::Other(
                "no result has been returned".to_string(),
            )))
        }
    }

    pub(crate) async fn batch_inner(
        &self,
        batch: Batch,
        close_stream: bool,
    ) -> Result<BatchResult> {
        let mut client = self.inner.stream.lock().await;
        let (resp, get_autocommit) = if close_stream {
            tracing::trace!("send Hrana SQL batch (with closing the stream)");
            let [resp, get_autocommit, _] = client
                .send_requests([
                    StreamRequest::Batch(BatchStreamReq { batch }),
                    StreamRequest::GetAutocommit(GetAutocommitStreamReq {}),
                    StreamRequest::Close(CloseStreamReq {}),
                ])
                .await?;
            client.reset();
            (resp, get_autocommit)
        } else {
            tracing::trace!("send Hrana SQL batch (leave the stream open)");
            let [resp, get_autocommit] = client
                .send_requests([
                    StreamRequest::Batch(BatchStreamReq { batch }),
                    StreamRequest::GetAutocommit(GetAutocommitStreamReq {}),
                ])
                .await?;
            (resp, get_autocommit)
        };
        drop(client);
        match get_autocommit {
            StreamResponse::GetAutocommit(r) => {
                self.inner
                    .is_autocommit
                    .store(r.is_autocommit, Ordering::SeqCst);
            }
            other => unexpected!(other),
        };
        match resp {
            StreamResponse::Batch(resp) => {
                if let Some(Some(result)) = resp.result.step_results.last() {
                    self.inner
                        .affected_row_count
                        .store(result.affected_row_count, Ordering::SeqCst);
                    self.inner
                        .total_changes
                        .fetch_add(result.affected_row_count, Ordering::SeqCst);
                    if let Some(last_insert_rowid) = result.last_insert_rowid {
                        self.inner
                            .last_insert_rowid
                            .store(last_insert_rowid, Ordering::SeqCst);
                    }
                }
                Ok(resp.result)
            }
            other => unexpected!(other),
        }
    }

    pub async fn cursor(&self, batch: Batch) -> Result<Cursor<T::Stream>> {
        let mut client = self.inner.stream.lock().await;
        let cursor = client.open_cursor(batch).await?;
        Ok(cursor)
    }

    pub async fn store_sql(&self, sql: String) -> Result<StoredSql<T>> {
        let mut client = self.inner.stream.lock().await;
        let sql_id = client.next_sql_id();
        let resp = client
            .send(StreamRequest::StoreSql(StoreSqlStreamReq { sql, sql_id }))
            .await?;
        match resp {
            StreamResponse::StoreSql(_) => {
                drop(client);
                Ok(StoredSql::new(self.clone(), sql_id))
            }
            other => unexpected!(other),
        }
    }

    async fn close_sql(&self, sql_id: SqlId) -> Result<()> {
        let mut client = self.inner.stream.lock().await;
        let resp = client
            .send(StreamRequest::CloseSql(CloseSqlStreamReq { sql_id }))
            .await?;
        match resp {
            StreamResponse::CloseSql(_) => Ok(()),
            other => unexpected!(other),
        }
    }

    pub async fn describe<D: SqlDescriptor>(&self, descriptor: &D) -> Result<DescribeResult> {
        let mut client = self.inner.stream.lock().await;
        let req = match descriptor.sql_description() {
            SqlDescription::Sql(sql) => DescribeStreamReq {
                sql: Some(sql),
                sql_id: None,
                replication_index: None,
            },
            SqlDescription::SqlId(sql_id) => DescribeStreamReq {
                sql: None,
                sql_id: Some(sql_id),
                replication_index: None,
            },
        };
        let resp = client.send(StreamRequest::Describe(req)).await?;
        match resp {
            StreamResponse::Describe(resp) => Ok(resp.result),
            other => unexpected!(other),
        }
    }

    pub async fn sequence<D: SqlDescriptor>(&self, descriptor: &D) -> Result<()> {
        let mut client = self.inner.stream.lock().await;
        let req = match descriptor.sql_description() {
            SqlDescription::Sql(sql) => SequenceStreamReq {
                sql: Some(sql),
                sql_id: None,
                replication_index: None,
            },
            SqlDescription::SqlId(sql_id) => SequenceStreamReq {
                sql: None,
                sql_id: Some(sql_id),
                replication_index: None,
            },
        };
        let resp = client.send(StreamRequest::Sequence(req)).await?;
        match resp {
            StreamResponse::Sequence(_) => Ok(()),
            other => unexpected!(other),
        }
    }

    pub async fn get_autocommit(&self) -> Result<bool> {
        let mut client = self.inner.stream.lock().await;
        let resp = client
            .send(StreamRequest::GetAutocommit(GetAutocommitStreamReq {}))
            .await?;
        match resp {
            StreamResponse::GetAutocommit(resp) => Ok(resp.is_autocommit),
            other => unexpected!(other),
        }
    }

    pub fn affected_row_count(&self) -> u64 {
        self.inner.affected_row_count.load(Ordering::SeqCst)
    }

    pub fn total_changes(&self) -> u64 {
        self.inner.total_changes.load(Ordering::SeqCst)
    }

    pub fn last_insert_rowid(&self) -> i64 {
        self.inner.last_insert_rowid.load(Ordering::SeqCst)
    }

    pub fn is_autocommit(&self) -> bool {
        self.inner.is_autocommit.load(Ordering::SeqCst)
    }

    pub async fn reset(&self) {
        (*self.inner).stream.lock().await.reset();
    }
}

#[derive(Debug)]
pub struct Inner<T>
where
    T: HttpSend,
{
    pub affected_row_count: AtomicU64,
    total_changes: AtomicU64,
    last_insert_rowid: AtomicI64,
    is_autocommit: AtomicBool,
    stream: Mutex<RawStream<T>>,
}

#[derive(Debug)]
struct RawStream<T>
where
    T: HttpSend,
{
    client: T,
    baton: RefCell<Option<String>>,
    pipeline_url: Arc<str>,
    cursor_url: Arc<str>,
    auth_token: Arc<str>,
    sql_id_generator: RefCell<SqlId>,
}

impl<T> RawStream<T>
where
    T: HttpSend,
{
    async fn send(&mut self, req: StreamRequest) -> Result<StreamResponse> {
        let [resp] = self.send_requests([req]).await?;
        Ok(resp)
    }

    pub async fn open_cursor(&mut self, batch: Batch) -> Result<Cursor<T::Stream>> {
        let msg = CursorReq {
            baton: self.baton.borrow().clone(),
            batch,
        };
        let body = serde_json::to_string(&msg).map_err(HranaError::Json)?;
        let stream = self
            .client
            .http_send(self.cursor_url.clone(), self.auth_token.clone(), body)
            .await?;
        let (cursor, mut response) = Cursor::open(stream).await?;
        if let Some(base_url) = response.base_url.take() {
            self.pipeline_url = Arc::from(format!("{base_url}/v3/pipeline"));
            self.cursor_url = Arc::from(format!("{base_url}/v3/cursor"));
        }
        match response.baton.take() {
            None => {
                tracing::trace!("client stream has been closed by the server");
                self.reset();
            } // stream has been closed by the server
            Some(baton) => {
                tracing::trace!("client stream has been assigned with baton: `{}`", baton);
                *self.baton.borrow_mut() = Some(baton)
            }
        }
        Ok(cursor)
    }

    async fn send_requests<const N: usize>(
        &mut self,
        requests: [StreamRequest; N],
    ) -> Result<[StreamResponse; N]> {
        tracing::trace!(
            "client stream sending {} requests with baton `{}`: {:?}",
            N,
            self.baton.borrow().as_deref().unwrap_or_default(),
            requests
        );
        let msg = PipelineReqBody {
            baton: self.baton.borrow().clone(),
            requests: Vec::from(requests),
        };
        let body = serde_json::to_string(&msg).map_err(HranaError::Json)?;
        let body = self
            .client
            .http_send(self.pipeline_url.clone(), self.auth_token.clone(), body)
            .await?;
        let body = stream_to_bytes(body).await?;
        let mut response: PipelineRespBody = serde_json::from_slice(&body)?;
        if let Some(base_url) = response.base_url.take() {
            let (pipeline_url, cursor_url) = parse_hrana_urls(&base_url);
            self.pipeline_url = pipeline_url;
            self.cursor_url = cursor_url;
        }
        match response.baton.take() {
            None => {
                tracing::trace!("client stream has been closed by the server");
                self.reset();
            } // stream has been closed by the server
            Some(baton) => {
                tracing::trace!("client stream has been assigned with baton: `{}`", baton);
                *self.baton.borrow_mut() = Some(baton)
            }
        }

        if response.results.is_empty() {
            Err(HranaError::UnexpectedResponse(format!(
                "Unexpected empty response from server: {:?}",
                response.results
            )))?;
        }
        if response.results.len() != N {
            // One with actual results, one closing the stream
            Err(HranaError::UnexpectedResponse(format!(
                "Unexpected multiple responses from server: {:?}",
                response.results
            )))?;
        }
        let mut responses = std::array::from_fn(|_| StreamResponse::Close(CloseStreamResp {}));
        for (i, result) in response.results.into_iter().enumerate() {
            match result {
                StreamResult::Ok { response } => responses[i] = response,
                StreamResult::Error { error } => return Err(HranaError::StreamError(error)),
                StreamResult::None => {}
            }
        }
        Ok(responses)
    }

    async fn finalize(&mut self, req: StreamRequest) -> Result<(StreamResponse, bool)> {
        let [resp, get_autocommit, _] = self
            .send_requests([
                req,
                StreamRequest::GetAutocommit(GetAutocommitStreamReq {}),
                StreamRequest::Close(CloseStreamReq {}),
            ])
            .await?;
        let is_autocommit = match get_autocommit {
            StreamResponse::GetAutocommit(resp) => resp.is_autocommit,
            other => {
                return Err(HranaError::UnexpectedResponse(format!(
                    "expected GetAutocommitResp but got {:?}",
                    other
                )))
            }
        };
        self.reset();
        Ok((resp, is_autocommit))
    }

    fn reset(&self) {
        if let Some(baton) = self.baton.borrow_mut().take() {
            tracing::trace!("closing client stream (baton: `{}`)", baton);
        }
        *self.sql_id_generator.borrow_mut() = 0;
    }

    fn next_sql_id(&mut self) -> SqlId {
        let mut gen = self.sql_id_generator.borrow_mut();
        *gen = gen.wrapping_add(1);
        *gen
    }
}

#[cfg(feature = "remote")]
impl<T> Drop for RawStream<T>
where
    T: HttpSend,
{
    fn drop(&mut self) {
        let baton = self.baton.get_mut().take();
        if let Some(baton) = baton {
            // only send a close request if stream was ever used to send the data
            tracing::trace!("closing client stream (baton: `{}`)", baton);
            let req = serde_json::to_string(&PipelineReqBody {
                baton: Some(baton),
                requests: vec![StreamRequest::Close(CloseStreamReq {})],
            })
            .unwrap();
            self.client
                .clone()
                .oneshot(self.pipeline_url.clone(), self.auth_token.clone(), req);
            self.reset();
        }
    }
}

pub(super) fn parse_hrana_urls(url: &str) -> (Arc<str>, Arc<str>) {
    let (mut base_url, query) = match url.rfind('?') {
        Some(i) => url.split_at(i),
        None => (url, ""),
    };
    if base_url.ends_with('/') {
        base_url = &base_url[0..(base_url.len() - 1)];
    }
    let pipeline_url = Arc::from(format!("{base_url}/v3/pipeline{query}"));
    let cursor_url = Arc::from(format!("{base_url}/v3/cursor{query}"));
    (pipeline_url, cursor_url)
}

async fn stream_to_bytes<S>(mut stream: S) -> Result<Bytes>
where
    S: Stream<Item = std::io::Result<Bytes>> + Unpin,
{
    use futures::StreamExt;

    let mut buf = BytesMut::new();
    while let Some(chunk) = stream.next().await {
        buf.extend_from_slice(&chunk.map_err(|e| HranaError::Http(e.to_string()))?);
    }
    Ok(buf.freeze())
}

#[derive(Debug, Clone)]
pub struct StoredSql<T>
where
    T: HttpSend,
{
    stream: HranaStream<T>,
    sql_id: SqlId,
}

impl<T> StoredSql<T>
where
    T: HttpSend,
{
    fn new(stream: HranaStream<T>, sql_id: SqlId) -> Self {
        StoredSql { stream, sql_id }
    }

    pub async fn describe(&self) -> Result<DescribeResult> {
        self.stream.describe(self).await
    }

    pub async fn execute(&self) -> Result<()> {
        self.stream.sequence(self).await
    }

    pub async fn close(self) -> Result<()> {
        self.stream.close_sql(self.sql_id).await
    }
}

/// Trait that allows to refer to stored SQL statements. Stored SQL is first send by
/// the prepared statements as raw string and can be cached on the server side in scope of
/// a current transaction using a number identifier. This identifier can be send in subsequent
/// calls in place of SQL string to reduce the size of a message.
pub trait SqlDescriptor {
    fn sql_description(&self) -> SqlDescription;
}

impl SqlDescriptor for String {
    fn sql_description(&self) -> SqlDescription {
        SqlDescription::Sql(self.clone())
    }
}

impl<T> SqlDescriptor for StoredSql<T>
where
    T: HttpSend,
{
    fn sql_description(&self) -> SqlDescription {
        SqlDescription::SqlId(self.sql_id)
    }
}

/// Enum used by stored SQL statements. It can refer to either fresh SQL statement text, or a
/// unique identifier assigned to that statement.
///
/// For the same statement executed many times over it's better to cache it first under [SqlId] key
/// and then re-execute it in subsequent calls using only identifier.
#[derive(Debug)]
pub enum SqlDescription {
    /// Non-cached SQL statement string.
    Sql(String),
    /// Key identifier of a SQL statement cached by the server. Key is valid only in
    /// the scope of current transaction/prepared statement, which sent a store SQL request.
    SqlId(SqlId),
}
