// https://github.com/tursodatabase/libsql/blob/main/docs/HRANA_3_SPEC.md#cursor-entries

use crate::hrana::proto::{Batch, BatchResult, Col, StmtResult, Value};
use crate::hrana::{CursorResponseError, HranaError, Result, Row};
use bytes::Bytes;
use futures::{ready, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::fmt::Formatter;
use std::future::poll_fn;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufReadExt, Lines};
use tokio_util::io::StreamReader;

#[derive(Serialize, Debug)]
pub struct CursorReq {
    pub baton: Option<String>,
    pub batch: Batch,
}

#[derive(Deserialize, Debug)]
pub struct CursorResp {
    pub baton: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CursorEntry {
    StepBegin(StepBeginEntry),
    StepEnd(StepEndEntry),
    StepError(StepErrorEntry),
    Row(RowEntry),
    Error(ErrorEntry),
}

#[derive(Deserialize, Debug)]
pub struct StepBeginEntry {
    pub step: u32,
    pub cols: Vec<Col>,
}

#[derive(Deserialize, Debug)]
pub struct StepEndEntry {
    pub affected_row_count: u32,
    pub last_inserted_rowid: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct RowEntry {
    pub row: Vec<Value>,
}

#[derive(Deserialize, Debug)]
pub struct StepErrorEntry {
    pub step: u32,
    pub error: Error,
}

#[derive(Deserialize, Debug)]
pub struct Error {
    pub message: String,
    pub code: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "(error code: {}) `{}`", self.code, self.message)
    }
}

#[derive(Deserialize, Debug)]
pub struct ErrorEntry {
    pub error: String,
}

pub struct Cursor<S> {
    stream: Lines<StreamReader<S, Bytes>>,
}

impl<S> Cursor<S>
where
    S: Stream<Item = std::io::Result<Bytes>> + Unpin,
{
    pub(super) async fn open(stream: S) -> Result<(Self, CursorResp)> {
        let stream = StreamReader::new(stream).lines();
        let mut cursor = Cursor { stream };
        match cursor.next_line().await? {
            None => Err(HranaError::CursorError(CursorResponseError::CursorClosed)),
            Some(line) => {
                let response: CursorResp = serde_json::from_str(&line)?;
                Ok((cursor, response))
            }
        }
    }

    pub async fn into_batch_result(mut self) -> Result<BatchResult> {
        use serde::de::Error;
        //FIXME: this is for the compatibility with the current libsql client API,
        //       which expects BatchResult to be returned
        let mut step_results = Vec::new();
        let mut step_errors = Vec::new();

        while let Ok(mut step) = self.next_step().await {
            let cols = step.state.cols.to_vec();
            let mut rows = Vec::new();
            while let Some(res) = step.next().await {
                match res {
                    Ok(row) => {
                        rows.push(crate::hrana::proto::Row { values: row.inner });
                    }
                    Err(err) => step_errors.push(Some(crate::hrana::proto::Error {
                        message: err.to_string(),
                        code: String::default(),
                    })),
                }
            }
            let affected_row_count = step.affected_rows() as u64;
            let last_insert_rowid = if let Some(rowid) = step.last_inserted_rowid() {
                let id = rowid.parse::<i64>().map_err(|_| {
                    serde_json::Error::invalid_value(
                        serde::de::Unexpected::Str(rowid),
                        &"decimal integer as a string",
                    )
                })?;
                Some(id)
            } else {
                None
            };
            let step_res = StmtResult {
                cols,
                rows,
                affected_row_count,
                last_insert_rowid,
                replication_index: None,
                rows_read: 0,
                rows_written: 0,
                query_duration_ms: 0.0,
            };
            step_results.push(Some(step_res));
        }
        Ok(BatchResult {
            step_results,
            step_errors,
            replication_index: None,
        })
    }

    pub async fn next_step(&mut self) -> Result<CursorStep<S>> {
        CursorStep::new(self).await
    }

    pub async fn next_step_owned(self) -> Result<OwnedCursorStep<S>> {
        OwnedCursorStep::new(self).await
    }

    pub async fn next_line(&mut self) -> Result<Option<String>> {
        let mut pin = Pin::new(self);
        poll_fn(move |cx| pin.as_mut().poll_next_line(cx)).await
    }

    fn poll_next_line(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<Option<String>>> {
        let ret = ready!(Pin::new(&mut self.stream).poll_next_line(cx))
            .map_err(|e| HranaError::CursorError(CursorResponseError::Other(e.to_string())));
        Poll::Ready(ret)
    }
}

impl<S> Stream for Cursor<S>
where
    S: Stream<Item = std::io::Result<Bytes>> + Unpin,
{
    type Item = Result<CursorEntry>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let res = ready!(self.poll_next_line(cx));
        match res {
            Err(e) => Poll::Ready(Some(Err(e))),
            Ok(None) => Poll::Ready(None),
            Ok(Some(line)) => {
                let entry: CursorEntry = match serde_json::from_str(line.as_str()) {
                    Ok(entry) => entry,
                    Err(e) => return Poll::Ready(Some(Err(e.into()))),
                };
                Poll::Ready(Some(Ok(entry)))
            }
        }
    }
}

pub struct OwnedCursorStep<S> {
    cursor: Option<Cursor<S>>,
    state: CursorStepState,
    /// Set once the step's `StepEnd` arrived. The rest of the response is then
    /// read to its end, so the HTTP connection can be reused.
    step_ended: bool,
}

impl<S> OwnedCursorStep<S>
where
    S: Stream<Item = std::io::Result<Bytes>> + Unpin,
{
    async fn new(mut cursor: Cursor<S>) -> Result<Self> {
        let begin = get_next_step(&mut cursor).await?;
        Ok(OwnedCursorStep {
            cursor: Some(cursor),
            state: CursorStepState {
                cols: begin.cols.into(),
                step_no: begin.step,
                affected_rows: 0,
                last_inserted_rowid: None,
            },
            step_ended: false,
        })
    }

    pub fn cursor(&self) -> Option<&Cursor<S>> {
        self.cursor.as_ref()
    }

    pub fn cursor_mut(&mut self) -> Option<&mut Cursor<S>> {
        self.cursor.as_mut()
    }

    /// Consume and discard all rows, fast running current cursor step to the end.
    pub async fn consume(&mut self) -> Result<()> {
        while let Some(res) = self.next().await {
            res?;
        }
        self.cursor.take();
        Ok(())
    }

    pub fn cols(&self) -> &[Col] {
        &self.state.cols
    }

    pub fn step_no(&self) -> u32 {
        self.state.step_no
    }

    pub fn affected_rows(&self) -> u32 {
        self.state.affected_rows
    }

    pub fn last_inserted_rowid(&self) -> Option<&str> {
        self.state.last_inserted_rowid.as_deref()
    }
}

impl<S> Stream for OwnedCursorStep<S>
where
    S: Stream<Item = std::io::Result<Bytes>> + Unpin,
{
    type Item = Result<Row>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            let cursor = if let Some(cursor) = &mut self.cursor {
                Pin::new(cursor)
            } else {
                return Poll::Ready(None);
            };
            match ready!(cursor.poll_next(cx)) {
                None => {
                    self.cursor = None;
                    return Poll::Ready(None);
                }
                // The step's rows are complete, so a failure while reading
                // the rest of the response only loses the connection.
                Some(Err(_)) if self.step_ended => {
                    self.cursor = None;
                    return Poll::Ready(None);
                }
                Some(Err(e)) => {
                    return Poll::Ready(Some(Err(HranaError::CursorError(
                        CursorResponseError::Other(e.to_string()),
                    ))))
                }
                Some(Ok(_)) if self.step_ended => {}
                Some(Ok(entry)) => match self.state.update(entry) {
                    Some(result) => return Poll::Ready(Some(result)),
                    None => self.step_ended = true,
                },
            }
        }
    }
}

pub struct CursorStep<'a, S> {
    cursor: Option<&'a mut Cursor<S>>,
    state: CursorStepState,
}

impl<'a, S> CursorStep<'a, S>
where
    S: Stream<Item = std::io::Result<Bytes>> + Unpin,
{
    async fn new(cursor: &'a mut Cursor<S>) -> Result<CursorStep<'a, S>> {
        let begin = get_next_step(cursor).await?;
        Ok(CursorStep {
            cursor: Some(cursor),
            state: CursorStepState {
                cols: begin.cols.into(),
                step_no: begin.step,
                affected_rows: 0,
                last_inserted_rowid: None,
            },
        })
    }

    /// Consume and discard all rows, fast running current cursor step to the end.
    pub async fn consume(&mut self) -> Result<()> {
        while let Some(res) = self.next().await {
            res?;
        }
        Ok(())
    }

    pub fn cols(&self) -> &[Col] {
        &self.state.cols
    }

    pub fn step_no(&self) -> u32 {
        self.state.step_no
    }

    pub fn affected_rows(&self) -> u32 {
        self.state.affected_rows
    }

    pub fn last_inserted_rowid(&self) -> Option<&str> {
        self.state.last_inserted_rowid.as_deref()
    }
}

async fn get_next_step<S>(cursor: &mut Cursor<S>) -> Result<StepBeginEntry>
where
    S: Stream<Item = std::io::Result<Bytes>> + Unpin,
{
    let mut begin = None;
    while let Some(res) = cursor.next().await {
        match res? {
            CursorEntry::StepBegin(entry) => {
                begin = Some(entry);
                break;
            }
            CursorEntry::Row(_) => {
                tracing::trace!("skipping over row message for previous cursor step")
            }
            CursorEntry::StepEnd(_) => {
                tracing::debug!("skipping over StepEnd message for previous cursor step")
            }
            CursorEntry::StepError(e) => {
                return Err(HranaError::CursorError(CursorResponseError::StepError {
                    step: e.step,
                    error: e.error,
                }))
            }
            CursorEntry::Error(e) => {
                return Err(HranaError::CursorError(CursorResponseError::Other(e.error)))
            }
        }
    }
    if let Some(begin) = begin {
        tracing::trace!("begin cursor step: {}", begin.step);
        Ok(begin)
    } else {
        Err(HranaError::CursorError(CursorResponseError::CursorClosed))
    }
}

impl<'a, S> Stream for CursorStep<'a, S>
where
    S: Stream<Item = std::io::Result<Bytes>> + Unpin,
{
    type Item = Result<Row>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let cursor = if let Some(cursor) = &mut self.cursor {
            Pin::new(cursor)
        } else {
            return Poll::Ready(None);
        };
        match ready!(cursor.poll_next(cx)) {
            None => {
                self.cursor = None;
                Poll::Ready(None)
            }
            Some(Err(e)) => Poll::Ready(Some(Err(e))),
            Some(Ok(entry)) => {
                let result = self.state.update(entry);
                if result.is_none() {
                    self.cursor.take();
                }
                Poll::Ready(result)
            }
        }
    }
}

#[derive(Debug)]
struct CursorStepState {
    cols: Arc<[Col]>,
    step_no: u32,
    affected_rows: u32,
    last_inserted_rowid: Option<String>,
}

impl CursorStepState {
    fn update(&mut self, entry: CursorEntry) -> Option<Result<Row>> {
        match entry {
            CursorEntry::Row(row) => {
                let row = Row::new(self.cols.clone(), row.row);
                Some(Ok(row))
            }
            CursorEntry::StepEnd(end) => {
                self.affected_rows = end.affected_row_count;
                self.last_inserted_rowid = end.last_inserted_rowid;
                None
            }
            CursorEntry::StepBegin(begin) => Some(Err(HranaError::CursorError(
                CursorResponseError::NotClosed {
                    expected: self.step_no,
                    actual: begin.step,
                },
            ))),
            CursorEntry::StepError(e) => Some(Err(HranaError::CursorError(
                CursorResponseError::StepError {
                    step: e.step,
                    error: e.error,
                },
            ))),
            CursorEntry::Error(e) => Some(Err(HranaError::CursorError(
                CursorResponseError::Other(e.error),
            ))),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::hrana::cursor::Cursor;
    use crate::rows::RowInner;
    use crate::Value;
    use bytes::Bytes;
    use futures::{Stream, StreamExt};
    use serde_json::json;

    type ByteStream = Box<dyn Stream<Item = std::io::Result<Bytes>> + Unpin>;

    fn byte_stream(entries: impl IntoIterator<Item = serde_json::Value>) -> ByteStream {
        let mut payload = Vec::new();
        const NEW_LINE: &[u8] = "\n".as_bytes();
        for v in entries.into_iter() {
            serde_json::to_writer(&mut payload, &v).unwrap();
            payload.extend_from_slice(NEW_LINE);
        }
        let chunks: Vec<_> = Bytes::from(payload)
            .chunks(23)
            .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
            .collect();
        let stream = futures::stream::iter(chunks);
        Box::new(stream)
    }
    #[tokio::test]
    async fn cursor_streaming() {
        let byte_stream = byte_stream(vec![
            json!({"baton": null, "base_url": null}),
            json!({"type": "step_begin", "step": 0, "cols": [{"name": "id"}, {"name": "email"}]}),
            json!({"type": "row", "row": [{"type": "integer", "value": "1"}, {"type": "text", "value": "alice@test.com"}]}),
            json!({"type": "row", "row": [{"type": "integer", "value": "2"}, {"type": "text", "value": "bob@test.com"}]}),
            json!({"type": "step_end", "affected_row_count": 0, "last_insert_rowid": null}),
        ]);
        let (mut cursor, resp) = Cursor::open(byte_stream).await.unwrap();
        assert_eq!(resp.baton, None);
        assert_eq!(resp.base_url, None);

        let mut step = cursor.next_step().await.unwrap();
        assert_eq!(step.step_no(), 0);
        {
            let cols: Vec<_> = step
                .cols()
                .iter()
                .map(|col| col.name.as_deref().unwrap_or(""))
                .collect();
            assert_eq!(cols, vec!["id", "email"]);
        }

        let row = step.next().await.unwrap().unwrap();
        assert_eq!(row.column_value(0).unwrap(), Value::from(1));
        assert_eq!(row.column_value(1).unwrap(), Value::from("alice@test.com"));

        let row = step.next().await.unwrap().unwrap();
        assert_eq!(row.column_value(0).unwrap(), Value::from(2));
        assert_eq!(row.column_value(1).unwrap(), Value::from("bob@test.com"));

        let row = step.next().await;
        assert!(row.is_none(), "last row should be None: {:?}", row);
    }

    #[tokio::test]
    async fn owned_step_reads_the_response_to_its_end() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let ended = Arc::new(AtomicBool::new(false));
        let ended_flag = ended.clone();
        let entries = byte_stream(vec![
            json!({"baton": null, "base_url": null}),
            json!({"type": "step_begin", "step": 0, "cols": [{"name": "id"}]}),
            json!({"type": "row", "row": [{"type": "integer", "value": "1"}]}),
            json!({"type": "step_end", "affected_row_count": 0, "last_insert_rowid": null}),
        ]);
        let byte_stream: ByteStream =
            Box::new(entries.chain(futures::stream::poll_fn(move |_| {
                ended_flag.store(true, Ordering::SeqCst);
                std::task::Poll::Ready(None)
            })));
        let (cursor, _) = Cursor::open(byte_stream).await.unwrap();
        let mut step = cursor.next_step_owned().await.unwrap();

        assert!(step.next().await.unwrap().is_ok());
        assert!(!ended.load(Ordering::SeqCst));
        assert!(step.next().await.is_none());
        assert!(ended.load(Ordering::SeqCst));
    }
}
