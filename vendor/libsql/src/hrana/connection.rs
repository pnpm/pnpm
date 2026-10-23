use crate::hrana::stream::{parse_hrana_urls, HranaStream};
use crate::hrana::{HttpSend, Statement};
use crate::util::coerce_url_scheme;
use std::ops::Deref;
use std::sync::Arc;

#[derive(Debug)]
pub struct HttpConnection<T>(Arc<InnerClient<T>>)
where
    T: HttpSend;

#[derive(Debug)]
struct InnerClient<T>
where
    T: HttpSend,
{
    /// Actual implementation of a client used to send HTTP requests.
    inner: T,
    /// Hrana stream used to execute statements directly on the connection itself.
    conn_stream: HranaStream<T>,
    /// URL of a pipeline API: `{base_url}/v3/pipeline`.
    pipeline_url: Arc<str>,
    /// URL of a cursor API: `{base_url}/v3/cursor`.
    cursor_url: Arc<str>,
    /// Authentication token.
    auth: Arc<str>,
}

impl<T> HttpConnection<T>
where
    T: HttpSend + Send + Sync + 'static,
{
    pub fn new(url: String, token: String, inner: T) -> Self {
        // The `libsql://` protocol is an alias for `https://`.
        let base_url = coerce_url_scheme(url);
        let (pipeline_url, cursor_url) = parse_hrana_urls(&base_url);
        let auth: Arc<str> = Arc::from(format!("Bearer {token}"));
        let conn_stream = HranaStream::open(
            inner.clone(),
            pipeline_url.clone(),
            cursor_url.clone(),
            auth.clone(),
        );
        HttpConnection(Arc::new(InnerClient {
            inner,
            pipeline_url,
            cursor_url,
            conn_stream,
            auth,
        }))
    }

    pub fn affected_row_count(&self) -> u64 {
        self.current_stream().affected_row_count()
    }

    pub fn total_changes(&self) -> u64 {
        self.current_stream().total_changes()
    }

    pub fn last_insert_rowid(&self) -> i64 {
        self.current_stream().last_insert_rowid()
    }

    pub fn is_autocommit(&self) -> bool {
        self.current_stream().is_autocommit()
    }

    pub(crate) fn current_stream(&self) -> &HranaStream<T> {
        &self.0.conn_stream
    }

    pub(crate) fn open_stream(&self) -> HranaStream<T> {
        let client = self.0.deref();
        HranaStream::open(
            client.inner.clone(),
            client.pipeline_url.clone(),
            client.cursor_url.clone(),
            client.auth.clone(),
        )
    }

    pub async fn prepare(&self, sql: &str) -> crate::Result<Statement<T>> {
        let stream = self.current_stream().clone();
        Statement::new(stream, sql.to_string(), true).await
    }
}

impl<T> Clone for HttpConnection<T>
where
    T: HttpSend,
{
    fn clone(&self) -> Self {
        HttpConnection(self.0.clone())
    }
}

pub(crate) enum CommitBehavior {
    Commit,
    Rollback,
}
