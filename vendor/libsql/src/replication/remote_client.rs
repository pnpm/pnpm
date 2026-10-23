use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures::{StreamExt as _, TryStreamExt};
use libsql_replication::frame::{FrameHeader, FrameNo};
use libsql_replication::meta::WalIndexMeta;
use libsql_replication::replicator::{Error, ReplicatorClient};
use libsql_replication::rpc::replication::{
    Frame as RpcFrame, verify_session_token, Frames, HelloRequest, HelloResponse, LogOffset, SESSION_TOKEN_KEY,
};
use tokio_stream::Stream;
use tonic::metadata::AsciiMetadataValue;
use tonic::{Response, Status};
use zerocopy::FromBytes;

pub(crate) async fn time<O>(fut: impl Future<Output = O>) -> (O, Duration) {
    let before = Instant::now();
    let out = fut.await;
    (out, before.elapsed())
}

pub(crate) struct SyncStats {
    pub prefetched_bytes: AtomicU64,
    pub prefetched_bytes_discarded_due_to_new_session: AtomicU64,
    pub prefetched_bytes_discarded_due_to_consecutive_handshake: AtomicU64,
    pub prefetched_bytes_discarded_due_to_invalid_frame_header: AtomicU64,
    pub synced_bytes_discarded_due_to_invalid_frame_header: AtomicU64,
    pub prefetched_bytes_used: AtomicU64,
    pub synced_bytes_used: AtomicU64,
    pub snapshot_bytes: AtomicU64,
}

impl SyncStats {
    fn new() -> Self {
        Self {
            prefetched_bytes: AtomicU64::new(0),
            prefetched_bytes_discarded_due_to_new_session: AtomicU64::new(0),
            prefetched_bytes_discarded_due_to_consecutive_handshake: AtomicU64::new(0),
            prefetched_bytes_discarded_due_to_invalid_frame_header: AtomicU64::new(0),
            synced_bytes_discarded_due_to_invalid_frame_header: AtomicU64::new(0),
            prefetched_bytes_used: AtomicU64::new(0),
            synced_bytes_used: AtomicU64::new(0),
            snapshot_bytes: AtomicU64::new(0),
        }
    }

    fn add_prefetched_bytes(&self, bytes: u64) {
        self.prefetched_bytes.fetch_add(bytes, std::sync::atomic::Ordering::SeqCst);
    }

    fn add_prefetched_bytes_discarded_due_to_new_session(&self, bytes: u64) {
        self.prefetched_bytes_discarded_due_to_new_session.fetch_add(bytes, std::sync::atomic::Ordering::SeqCst);
    }

    fn add_prefetched_bytes_discarded_due_to_consecutive_handshake(&self, bytes: u64) {
        self.prefetched_bytes_discarded_due_to_consecutive_handshake.fetch_add(bytes, std::sync::atomic::Ordering::SeqCst);
    }

    fn add_prefetched_bytes_discarded_due_to_invalid_frame_header(&self, bytes: u64) {
        self.prefetched_bytes_discarded_due_to_invalid_frame_header.fetch_add(bytes, std::sync::atomic::Ordering::SeqCst);
    }

    fn add_synced_bytes_discarded_due_to_invalid_frame_headear(&self, bytes: u64) {
        self.synced_bytes_discarded_due_to_invalid_frame_header.fetch_add(bytes, std::sync::atomic::Ordering::SeqCst);
    }

    fn add_prefetched_bytes_used(&self, bytes: u64) {
        self.prefetched_bytes_used.fetch_add(bytes, std::sync::atomic::Ordering::SeqCst);
    }

    fn add_synced_bytes_used(&self, bytes: u64) {
        self.synced_bytes_used.fetch_add(bytes, std::sync::atomic::Ordering::SeqCst);
    }
    fn add_snapshot_bytes(&self, bytes: u64) {
        self.snapshot_bytes.fetch_add(bytes, std::sync::atomic::Ordering::SeqCst);
    }
}

/// A remote replicator client, that pulls frames over RPC
pub struct RemoteClient {
    remote: super::client::Client,
    meta: WalIndexMeta,
    last_received: Option<FrameNo>,
    session_token: Option<Bytes>,
    last_handshake_replication_index: Option<FrameNo>,
    // the replication log is dirty, reset the meta on next handshake
    dirty: bool,
    prefetched_batch_log_entries: Option<(Result<Response<Frames>, Status>, Duration)>,
    handshake_latency_sum: Duration,
    handshake_latency_count: u128,
    frames_latency_sum: Duration,
    frames_latency_count: u128,
    snapshot_latency_sum: Duration,
    snapshot_latency_count: u128,
    sync_stats: Arc<SyncStats>,
}

impl RemoteClient {
    pub(crate) async fn new(remote: super::client::Client, path: &Path) -> anyhow::Result<Self> {
        let meta = WalIndexMeta::open_prefixed(path).await?;
        Ok(Self {
            remote,
            last_received: meta.current_frame_no(),
            meta,
            session_token: None,
            dirty: false,
            last_handshake_replication_index: None,
            prefetched_batch_log_entries: None,
            handshake_latency_sum: Duration::default(),
            handshake_latency_count: 0,
            frames_latency_sum: Duration::default(),
            frames_latency_count: 0,
            snapshot_latency_sum: Duration::default(),
            snapshot_latency_count: 0,
            sync_stats: Arc::new(SyncStats::new()),
        })
    }

    pub(crate) fn sync_stats(&self) -> Arc<SyncStats> {
        self.sync_stats.clone()
    }

    fn next_offset(&self) -> FrameNo {
        match self.last_received {
            Some(fno) => fno + 1,
            None => 0,
        }
    }

    fn make_request<T>(&self, req: T) -> tonic::Request<T> {
        let mut req = tonic::Request::new(req);
        if let Some(token) = self.session_token.clone() {
            // SAFETY: we always validate the token
            req.metadata_mut().insert(SESSION_TOKEN_KEY, unsafe {
                AsciiMetadataValue::from_shared_unchecked(token)
            });
        }

        req
    }

    pub fn last_handshake_replication_index(&self) -> Option<u64> {
        self.last_handshake_replication_index
    }

    async fn handle_handshake_response(
        &mut self,
        hello: Result<Response<HelloResponse>, Status>,
    ) -> Result<bool, Error> {
        let hello = hello?.into_inner();
        verify_session_token(&hello.session_token).map_err(Error::Client)?;
        let new_session = self.session_token.as_ref() != Some(&hello.session_token);
        self.session_token = Some(hello.session_token.clone());
        let current_replication_index = hello.current_replication_index;
        if let Err(e) = self.meta.init_from_hello(hello) {
            // set the meta as dirty. The caller should catch the error and clean the db
            // file. On the next call to replicate, the db will be replicated from the new
            // log.
            if let libsql_replication::meta::Error::LogIncompatible = e {
                self.dirty = true;
            }

            Err(e)?;
        }
        self.last_handshake_replication_index = current_replication_index;
        self.meta.flush().await?;
        Ok(new_session)
    }

    async fn do_handshake_with_prefetch(&mut self) -> (Result<(), Error>, Duration) {
        tracing::info!("Attempting to perform handshake with primary.");
        if let Some((Ok(frames), _)) = &self.prefetched_batch_log_entries {
            // TODO: check if it's ok to just do 4096 * frames.len()
            let bytes = frames.get_ref().frames.iter().map(|f| f.data.len() as u64).sum();
            self.sync_stats.add_prefetched_bytes_discarded_due_to_consecutive_handshake(bytes);
        }
        if self.dirty {
            self.prefetched_batch_log_entries = None;
            self.meta.reset();
            self.last_received = self.meta.current_frame_no();
            self.dirty = false;
        }
        let prefetch = self.session_token.is_some();
        let hello_req = self.make_request(HelloRequest::new());
        let log_offset_req = self.make_request(LogOffset {
            next_offset: self.next_offset(),
            wal_flavor: None,
        });
        let mut client_clone = self.remote.clone();
        let hello_fut = time(async {
            let res = self.remote.replication.hello(hello_req).await;
            self.handle_handshake_response(res).await
        });
        let (hello, frames) = if prefetch {
            let (hello, frames) = tokio::join!(
                hello_fut,
                time(client_clone.replication.batch_log_entries(log_offset_req))
            );
            (hello, Some(frames))
        } else {
            (hello_fut.await, None)
        };
        let mut prefetched_bytes = None;
        if let Some((Ok(frames), _)) = &frames {
            let bytes = frames.get_ref().frames.iter().map(|f| f.data.len() as u64).sum();
            self.sync_stats.add_prefetched_bytes(bytes);
            prefetched_bytes = Some(bytes);
        }
        self.prefetched_batch_log_entries = if let Ok(true) = hello.0 {
            tracing::debug!(
                "Frames prefetching failed because of new session token returned by handshake"
            );
            if let Some(bytes) = prefetched_bytes {
                self.sync_stats.add_prefetched_bytes_discarded_due_to_new_session(bytes);
            }
            None
        } else {
            frames
        };

        (hello.0.map(|_| ()), hello.1)
    }

    async fn handle_next_frames_response(
        &mut self,
        frames: Result<Response<Frames>, Status>,
        prefetched: bool,
    ) -> Result<<Self as ReplicatorClient>::FrameStream, Error> {
        let frames = frames?.into_inner().frames;
        let bytes = frames.iter().map(|f| f.data.len() as u64).sum();

        if let Some(f) = frames.last() {
            let header_result = FrameHeader::read_from_prefix(&f.data);
            if header_result.is_none() {
                if prefetched {
                    self.sync_stats.add_prefetched_bytes_discarded_due_to_invalid_frame_header(bytes);
                } else {
                    self.sync_stats.add_synced_bytes_discarded_due_to_invalid_frame_headear(bytes);
                }
            }
            let header: FrameHeader = header_result
                .ok_or_else(|| Error::Internal("invalid frame header".into()))?;
            self.last_received = Some(header.frame_no.get());
        }

        if prefetched {
            self.sync_stats.add_prefetched_bytes_used(bytes);
        } else {
            self.sync_stats.add_synced_bytes_used(bytes);
        }

        let frames_iter = frames
            .into_iter()
            .map(Ok);

        let stream = tokio_stream::iter(frames_iter);

        Ok(Box::pin(stream))
    }

    async fn do_next_frames(
        &mut self,
    ) -> (
        Result<<Self as ReplicatorClient>::FrameStream, Error>,
        Duration,
    ) {
        let ((frames, time), prefetched) = match self.prefetched_batch_log_entries.take() {
            Some((result, time)) => ((result, time), true),
            None => {
                let req = self.make_request(LogOffset {
                    next_offset: self.next_offset(),
                    wal_flavor: None,
                });
                let result = time(self.remote.replication.batch_log_entries(req)).await;
                (result, false)
            }
        };
        let res = self.handle_next_frames_response(frames, prefetched).await;
        (res, time)
    }

    async fn do_snapshot(&mut self) -> Result<<Self as ReplicatorClient>::FrameStream, Error> {
        let req = self.make_request(LogOffset {
            next_offset: self.next_offset(),
            wal_flavor: None,
        });
        let sync_stats = self.sync_stats.clone();
        let mut frames = self
            .remote
            .replication
            .snapshot(req)
            .await?
            .into_inner()
            .map_err(|e| e.into())
            .map_ok(move |f| {
                sync_stats.add_snapshot_bytes(f.data.len() as u64);
                f
            })
            .peekable();

        {
            let frames = Pin::new(&mut frames);

            // the first frame is the one with the highest frame_no in the snapshot
            if let Some(Ok(f)) = frames.peek().await {
                let header: FrameHeader = FrameHeader::read_from_prefix(&f.data[..]).unwrap();
                self.last_received = Some(header.frame_no.get());
            }
        }


        Ok(Box::pin(frames))
    }
}

fn maybe_log<T>(
    time: Duration,
    sum: &mut Duration,
    count: &mut u128,
    result: &Result<T, Error>,
    op_name: &str,
) {
    if let Err(e) = &result {
        tracing::warn!("Failed {} in {} ms: {:?}", op_name, time.as_millis(), e);
    } else {
        *sum += time;
        *count += 1;
        let avg = (*sum).as_millis() / *count;
        let time = time.as_millis();
        if *count > 10 && time > 2 * avg {
            tracing::warn!(
                "Unusually long {}. Took {} ms, average {} ms",
                op_name,
                time,
                avg
            );
        }
    }
}

#[async_trait::async_trait]
impl ReplicatorClient for RemoteClient {
    type FrameStream = Pin<Box<dyn Stream<Item = Result<RpcFrame, Error>> + Send + 'static>>;

    /// Perform handshake with remote
    async fn handshake(&mut self) -> Result<(), Error> {
        let (result, time) = self.do_handshake_with_prefetch().await;
        maybe_log(
            time,
            &mut self.handshake_latency_sum,
            &mut self.handshake_latency_count,
            &result,
            "handshake",
        );
        result
    }

    /// Return a stream of frames to apply to the database
    async fn next_frames(&mut self) -> Result<Self::FrameStream, Error> {
        let (result, time) = self.do_next_frames().await;
        maybe_log(
            time,
            &mut self.frames_latency_sum,
            &mut self.frames_latency_count,
            &result,
            "frames fetch",
        );
        result
    }

    /// Return a snapshot for the current replication index. Called after next_frame has returned a
    /// NeedSnapshot error
    async fn snapshot(&mut self) -> Result<Self::FrameStream, Error> {
        let (snapshot, time) = time(self.do_snapshot()).await;
        maybe_log(
            time,
            &mut self.snapshot_latency_sum,
            &mut self.snapshot_latency_count,
            &snapshot,
            "snapshot fetch",
        );
        snapshot
    }

    /// set the new commit frame_no
    async fn commit_frame_no(&mut self, frame_no: FrameNo) -> Result<(), Error> {
        self.meta.set_commit_frame_no(frame_no).await?;
        Ok(())
    }

    fn committed_frame_no(&self) -> Option<FrameNo> {
        self.meta.current_frame_no()
    }

    fn rollback(&mut self) {
        self.last_received = self.committed_frame_no()
    }
}
