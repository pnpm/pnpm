//! A journal of the requests this test process's mock registries receive,
//! so the report of a stalled command shows whether it waits on one of them.

use axum::{
    body::{Body, HttpBody},
    extract::{Request, State},
    http::{HeaderValue, Method, header},
    middleware::Next,
    response::Response,
};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use std::{
    fmt::Write as _,
    pin::Pin,
    sync::{LazyLock, Mutex, PoisonError},
    task::{Context, Poll},
    time::Instant,
};

/// How many finished requests the report lists, most recent last.
const FINISHED_IN_REPORT: usize = 10;

static JOURNAL: LazyLock<Mutex<Vec<Entry>>> = LazyLock::new(Mutex::default);

#[derive(Debug)]
struct Entry {
    registry: String,
    method: Method,
    path: String,
    started: Instant,
    /// The status and when the response head was ready.
    response: Option<(u16, Instant)>,
    body: BodyState,
}

#[derive(Debug, Clone, Copy)]
enum BodyState {
    Streaming { bytes: u64 },
    Ended { bytes: u64, at: Instant, how: BodyEnd },
}

#[derive(Debug, Clone, Copy)]
enum BodyEnd {
    Done,
    Failed,
    /// The client went away before the body ended.
    Dropped,
}

impl Entry {
    fn is_finished(&self) -> bool {
        matches!(self.body, BodyState::Ended { how: BodyEnd::Done, .. })
    }
}

/// Middleware recording each request of the registry at `registry` in the
/// journal, through the end of its response body.
pub(crate) async fn track(
    State(registry): State<String>,
    request: Request,
    next: Next,
) -> Response {
    let index = record_start(registry, &request);
    let response = next.run(request).await;
    let status = response.status().as_u16();
    update(index, |entry| entry.response = Some((status, Instant::now())));
    let (mut parts, body) = response.into_parts();
    let exact_len = body.size_hint().exact();
    if exact_len == Some(0) {
        // An empty body is left alone: a HEAD, 204, or 304 response must
        // not gain a `Content-Length`.
        let done = BodyState::Ended { bytes: 0, at: Instant::now(), how: BodyEnd::Done };
        update(index, |entry| entry.body = done);
        return Response::from_parts(parts, body);
    }
    if let Some(len) = exact_len {
        // A stream has no exact size, so keep the framing the body had.
        parts.headers.entry(header::CONTENT_LENGTH).or_insert_with(|| HeaderValue::from(len));
    }
    let declared = parts.headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok()?.parse().ok());
    let tracked =
        TrackedBody { inner: body.into_data_stream(), index, bytes: 0, declared, ended: false };
    Response::from_parts(parts, Body::from_stream(tracked))
}

fn record_start(registry: String, request: &Request) -> usize {
    let entry = Entry {
        registry,
        method: request.method().clone(),
        path: request.uri().path().to_string(),
        started: Instant::now(),
        response: None,
        body: BodyState::Streaming { bytes: 0 },
    };
    let mut journal = JOURNAL.lock().unwrap_or_else(PoisonError::into_inner);
    journal.push(entry);
    journal.len() - 1
}

fn update(index: usize, change: impl FnOnce(&mut Entry)) {
    let mut journal = JOURNAL.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(entry) = journal.get_mut(index) {
        change(entry);
    }
}

/// The response body, passed through unchanged while its progress is
/// recorded.
struct TrackedBody {
    inner: axum::body::BodyDataStream,
    index: usize,
    bytes: u64,
    /// The `Content-Length`. The server stops polling once it has sent that
    /// many bytes, so such a body never reports its end.
    declared: Option<u64>,
    ended: bool,
}

impl TrackedBody {
    fn end(&mut self, how: BodyEnd) {
        self.ended = true;
        let ended = BodyState::Ended { bytes: self.bytes, at: Instant::now(), how };
        update(self.index, |entry| entry.body = ended);
    }
}

impl Stream for TrackedBody {
    type Item = Result<Bytes, axum::Error>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let polled = self.inner.poll_next_unpin(context);
        match &polled {
            Poll::Ready(Some(Ok(chunk))) => {
                self.bytes += chunk.len() as u64;
                if self.declared == Some(self.bytes) {
                    self.end(BodyEnd::Done);
                } else {
                    let streaming = BodyState::Streaming { bytes: self.bytes };
                    update(self.index, |entry| entry.body = streaming);
                }
            }
            Poll::Ready(Some(Err(_))) => self.end(BodyEnd::Failed),
            Poll::Ready(None) => self.end(BodyEnd::Done),
            Poll::Pending => {}
        }
        polled
    }
}

impl Drop for TrackedBody {
    fn drop(&mut self) {
        if !self.ended {
            self.end(BodyEnd::Dropped);
        }
    }
}

/// The requests that have not finished, and the last ones that have, or an
/// empty string when no mock registry of this process received a request.
pub(crate) fn report() -> String {
    let journal = JOURNAL.lock().unwrap_or_else(PoisonError::into_inner);
    if journal.is_empty() {
        return String::new();
    }
    let now = Instant::now();
    let (finished, unfinished) = journal.iter().partition::<Vec<_>, _>(|entry| entry.is_finished());
    let mut text = format!(
        "mock registry requests: {} unfinished, {} finished\n",
        unfinished.len(),
        finished.len(),
    );
    let recent = finished.len().saturating_sub(FINISHED_IN_REPORT);
    for entry in unfinished
        .iter()
        .chain(&finished[recent..])
    {
        let _ = writeln!(text, "  {}", describe(entry, now));
    }
    text
}

fn describe(entry: &Entry, now: Instant) -> String {
    let since_start = |at: Instant| at.duration_since(entry.started).as_secs_f32();
    let response = match entry.response {
        Some((status, at)) => format!("status {status} after {:.1}s", since_start(at)),
        None => "no response yet".to_string(),
    };
    let body = match entry.body {
        BodyState::Streaming { bytes } if entry.response.is_some() => {
            format!("body still streaming, {bytes} bytes sent")
        }
        BodyState::Streaming { .. } => String::new(),
        BodyState::Ended { bytes, at, how } => {
            format!("body {how:?} after {:.1}s, {bytes} bytes", since_start(at))
        }
    };
    format!(
        "{} {}{}: started {:.1}s ago, {response}{}{body}",
        entry.method,
        entry.registry,
        entry.path,
        now.duration_since(entry.started).as_secs_f32(),
        if body.is_empty() { "" } else { ", " },
    )
}

#[cfg(test)]
mod tests;
