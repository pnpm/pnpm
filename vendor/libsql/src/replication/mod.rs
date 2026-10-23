//! Utilities used when using a replicated version of libsql.

use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

pub use libsql_replication::frame::{Frame, FrameNo};
use libsql_replication::injector::SqliteInjector;
use libsql_replication::replicator::{Either, Replicator};
pub use libsql_replication::snapshot::SnapshotFile;

use libsql_replication::rpc::proxy::{
    query::Params, DescribeRequest, DescribeResult, ExecuteResults, Positional, Program,
    ProgramReq, Query, Step,
};
use tokio::sync::Mutex;
use tokio::task::AbortHandle;
use tracing::Instrument;

use crate::database::EncryptionConfig;
use crate::parser::Statement;
use crate::{errors, Result};

use libsql_replication::replicator::ReplicatorClient;

pub(crate) use connection::RemoteConnection;

use self::local_client::LocalClient;
use self::remote_client::RemoteClient;

pub(crate) mod client;
pub(crate) mod connection;
pub(crate) mod local_client;
pub(crate) mod remote_client;

pub use crate::database::Replicated;

/// A set of rames to be injected via `sync_frames`.
pub enum Frames {
    /// A set of frames, in increasing frame_no.
    Vec(Vec<Frame>),
    /// A stream of snapshot frames. The frames must be in reverse frame_no, and the pages
    /// deduplicated. The snapshot is expected to be a single commit unit.
    Snapshot(SnapshotFile),
}

/// Detailed logs about bytes synced with primary
pub struct SyncUsageStats {
    prefetched_bytes: u64,
    prefetched_bytes_discarded_due_to_new_session: u64,
    prefetched_bytes_discarded_due_to_consecutive_handshake: u64,
    prefetched_bytes_discarded_due_to_invalid_frame_header: u64,
    synced_bytes_discarded_due_to_invalid_frame_header: u64,
    prefetched_bytes_used: u64,
    synced_bytes_used: u64,
    snapshot_bytes: u64,
}

impl SyncUsageStats {
    /// Number of bytes prefetched while doing handshake
    pub fn prefetched_bytes(&self) -> u64 {
        self.prefetched_bytes
    }

    /// Number of bytes prefetched and discarded due to the change of the client session
    pub fn prefetched_bytes_discarded_due_to_new_session(&self) -> u64 {
        self.prefetched_bytes_discarded_due_to_new_session
    }

    /// Number of bytes prefetched and discarded due to consecutive handshake with new prefetch
    pub fn prefetched_bytes_discarded_due_to_consecutive_handshake(&self) -> u64 {
        self.prefetched_bytes_discarded_due_to_consecutive_handshake
    }

    /// Number of bytes prefetched and discarded due to invalid frame header in received frames
    pub fn prefetched_bytes_discarded_due_to_invalid_frame_header(&self) -> u64 {
        self.prefetched_bytes_discarded_due_to_invalid_frame_header
    }

    /// Number of bytes synced and discarded due to invalid frame header in received frames
    pub fn synced_bytes_discarded_due_to_invalid_frame_header(&self) -> u64 {
        self.synced_bytes_discarded_due_to_invalid_frame_header
    }

    /// Number of bytes prefetched and used
    pub fn prefetched_bytes_used(&self) -> u64 {
        self.prefetched_bytes_used
    }

    /// Number of bytes synced and used
    pub fn synced_bytes_used(&self) -> u64 {
        self.synced_bytes_used
    }

    /// Number of bytes downloaded as snapshots
    pub fn snapshot_bytes(&self) -> u64 {
        self.snapshot_bytes
    }
}

#[derive(Clone)]
pub(crate) struct Writer {
    pub(crate) client: client::Client,
    pub(crate) replicator: Option<EmbeddedReplicator>,
}

impl Writer {
    pub(crate) async fn execute_program(
        &self,
        steps: Vec<Statement>,
        params: impl Into<Params>,
    ) -> anyhow::Result<ExecuteResults> {
        let mut params = Some(params.into());

        let steps = steps
            .into_iter()
            .map(|stmt| Step {
                query: Some(Query {
                    stmt: stmt.stmt,
                    // TODO(lucio): Pass params
                    params: Some(
                        params
                            .take()
                            .unwrap_or(Params::Positional(Positional::default())),
                    ),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .collect();

        self.execute_steps(steps).await
    }

    pub(crate) async fn execute_steps(&self, steps: Vec<Step>) -> anyhow::Result<ExecuteResults> {
        self.client
            .execute_program(ProgramReq {
                client_id: self.client.client_id(),
                pgm: Some(Program { steps }),
            })
            .await
    }

    pub(crate) async fn describe(&self, stmt: impl Into<String>) -> anyhow::Result<DescribeResult> {
        let stmt = stmt.into();

        self.client
            .describe(DescribeRequest {
                client_id: self.client.client_id(),
                stmt,
            })
            .await
    }

    pub(crate) fn replicator(&self) -> Option<&EmbeddedReplicator> {
        self.replicator.as_ref()
    }

    pub(crate) fn new_client_id(&mut self) {
        self.client.new_client_id()
    }
}

#[derive(Clone)]
pub(crate) struct EmbeddedReplicator {
    replicator: Arc<Mutex<Replicator<Either<RemoteClient, LocalClient>, SqliteInjector>>>,
    bg_abort: Option<Arc<DropAbort>>,
    last_frames_synced: Arc<AtomicUsize>,
}

impl From<libsql_replication::replicator::Error> for errors::Error {
    fn from(err: libsql_replication::replicator::Error) -> Self {
        errors::Error::Replication(err.into())
    }
}

impl EmbeddedReplicator {
    pub async fn with_remote(
        client: RemoteClient,
        db_path: PathBuf,
        auto_checkpoint: u32,
        encryption_config: Option<EncryptionConfig>,
        perodic_sync: Option<Duration>,
    ) -> Result<Self> {
        let mut replicator =
            Replicator::new_sqlite(
                Either::Left(client),
                db_path,
                auto_checkpoint,
                encryption_config,
            ).await?;
        replicator.set_primary_handshake_retries(3);
        let replicator = Arc::new(Mutex::new(replicator));

        let mut replicator = Self {
            replicator,
            bg_abort: None,
            last_frames_synced: Arc::new(AtomicUsize::new(0)),
        };

        if let Some(sync_duration) = perodic_sync {
            let replicator2 = replicator.clone();

            let jh = tokio::spawn(
                async move {
                    loop {
                        if let Err(e) = replicator2.sync_oneshot().await {
                            tracing::error!("replicator sync error: {}", e);
                        }

                        tokio::time::sleep(sync_duration).await;
                    }
                }
                .instrument(tracing::info_span!("sync_interval")),
            );

            replicator.bg_abort = Some(Arc::new(DropAbort(jh.abort_handle())));
        }

        Ok(replicator)
    }

    pub async fn with_local(
        client: LocalClient,
        db_path: PathBuf,
        auto_checkpoint: u32,
        encryption_config: Option<EncryptionConfig>,
    ) -> Result<Self> {
        let replicator = Arc::new(Mutex::new(
            Replicator::new_sqlite(
                Either::Right(client),
                db_path,
                auto_checkpoint,
                encryption_config,
            )
            .await?,
        ));

        Ok(Self {
            replicator,
            bg_abort: None,
            last_frames_synced: Arc::new(AtomicUsize::new(0)),
        })
    }

    pub async fn get_sync_usage_stats(&self) -> Result<SyncUsageStats> {
        let mut replicator = self.replicator.lock().await;
        match replicator.client_mut() {
            Either::Right(_) => {
                Err(crate::errors::Error::Misuse(
                    "Trying to get sync usage stats, but this is a local replicator".into(),
                ))
            }
            Either::Left(c) => {
                let stats = c.sync_stats();
                Ok(SyncUsageStats {
                    prefetched_bytes: stats.prefetched_bytes.load(std::sync::atomic::Ordering::SeqCst),
                    prefetched_bytes_discarded_due_to_new_session: stats
                        .prefetched_bytes_discarded_due_to_new_session.load(std::sync::atomic::Ordering::SeqCst),
                    prefetched_bytes_discarded_due_to_consecutive_handshake: stats
                        .prefetched_bytes_discarded_due_to_consecutive_handshake.load(std::sync::atomic::Ordering::SeqCst),
                    prefetched_bytes_discarded_due_to_invalid_frame_header: stats
                        .prefetched_bytes_discarded_due_to_invalid_frame_header.load(std::sync::atomic::Ordering::SeqCst),
                    synced_bytes_discarded_due_to_invalid_frame_header: stats
                        .synced_bytes_discarded_due_to_invalid_frame_header.load(std::sync::atomic::Ordering::SeqCst),
                    prefetched_bytes_used: stats.prefetched_bytes_used.load(std::sync::atomic::Ordering::SeqCst),
                    synced_bytes_used: stats.synced_bytes_used.load(std::sync::atomic::Ordering::SeqCst),
                    snapshot_bytes: stats.snapshot_bytes.load(std::sync::atomic::Ordering::SeqCst),
                })
            }
        }

    }

    pub async fn sync_oneshot(&self) -> Result<Replicated> {
        use libsql_replication::replicator::ReplicatorClient;

        let mut replicator = self.replicator.lock().await;
        if !matches!(replicator.client_mut(), Either::Left(_)) {
            return Err(crate::errors::Error::Misuse(
                "Trying to replicate from HTTP, but this is a local replicator".into(),
            ));
        }

        // we force a handshake to get the most up to date replication index from the primary.
        replicator.force_handshake();

        loop {
            match replicator.replicate().await {
                Err(libsql_replication::replicator::Error::Meta(
                    libsql_replication::meta::Error::LogIncompatible,
                )) => {
                    // The meta must have been marked as dirty, replicate again from scratch
                    // this time.
                    tracing::debug!("re-replicating database after LogIncompatible error");
                    replicator
                        .replicate()
                        .await
                        .map_err(|e| crate::Error::Replication(e.into()))?;
                }
                Err(e) => return Err(crate::Error::Replication(e.into())),
                Ok(_) => {
                    let Either::Left(client) = replicator.client_mut() else {
                        unreachable!()
                    };
                    let Some(primary_index) = client.last_handshake_replication_index() else {
                        return Ok(Replicated {
                            frame_no: None,
                            frames_synced: 0,
                        });
                    };
                    if let Some(replica_index) = replicator.client_mut().committed_frame_no() {
                        if replica_index >= primary_index {
                            break;
                        }
                    }
                }
            }
        }

        let current_frames_synced = replicator.frames_synced();

        let mut last_frames_synced = self
            .last_frames_synced
            .load(std::sync::atomic::Ordering::Relaxed);

        while current_frames_synced > last_frames_synced {
            match self.last_frames_synced.compare_exchange(
                last_frames_synced,
                current_frames_synced,
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current_value) => last_frames_synced = current_value,
            }
        }

        let frames_synced =
            ((current_frames_synced as i64 - last_frames_synced as i64).abs()) as usize;

        let replicated = Replicated {
            frame_no: replicator.client_mut().committed_frame_no(),
            frames_synced,
        };

        Ok(replicated)
    }

    pub async fn sync_frames(&self, frames: Frames) -> Result<Option<FrameNo>> {
        let mut replicator = self.replicator.lock().await;

        match replicator.client_mut() {
            Either::Right(c) => {
                c.load_frames(frames);
            }
            Either::Left(_) => {
                return Err(crate::errors::Error::Misuse(
                    "Trying to call sync_frames with an HTTP replicator".into(),
                ))
            }
        }
        replicator
            .replicate()
            .await
            .map_err(|e| crate::Error::Replication(e.into()))?;

        Ok(replicator.client_mut().committed_frame_no())
    }

    pub async fn flush(&self) -> Result<Option<FrameNo>> {
        let mut replicator = self.replicator.lock().await;
        replicator
            .flush()
            .await
            .map_err(|e| crate::Error::Replication(e.into()))?;
        Ok(replicator.client_mut().committed_frame_no())
    }

    pub async fn committed_frame_no(&self) -> Option<FrameNo> {
        self.replicator
            .lock()
            .await
            .client_mut()
            .committed_frame_no()
    }
}

struct DropAbort(AbortHandle);

impl Drop for DropAbort {
    fn drop(&mut self) {
        self.0.abort();
    }
}
