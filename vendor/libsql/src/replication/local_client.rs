use std::path::Path;
use std::pin::Pin;

use futures::{StreamExt, TryStreamExt};
use libsql_replication::{
    rpc::replication::Frame as RpcFrame,
    frame::{Frame, FrameNo},
    meta::WalIndexMeta,
    replicator::{Error, ReplicatorClient},
};
use tokio_stream::Stream;

use crate::replication::Frames;

/// A replicator client that doesn't perform networking, and is instead _loaded_ with the frames to
/// inject
pub struct LocalClient {
    frames: Option<Frames>,
    meta: WalIndexMeta,
}

impl LocalClient {
    pub(crate) async fn new(path: &Path) -> anyhow::Result<Self> {
        let mut meta = WalIndexMeta::open_prefixed(path).await?;
        meta.init_default();
        Ok(Self { frames: None, meta })
    }

    /// Load `frames` into the client. The caller must ensure that client was flushed before
    /// calling this function again.
    pub(crate) fn load_frames(&mut self, frames: Frames) {
        assert!(self.frames.is_none(), "frames not flushed before loading");
        self.frames.replace(frames);
    }
}

#[async_trait::async_trait]
impl ReplicatorClient for LocalClient {
    type FrameStream = Pin<Box<dyn Stream<Item = Result<RpcFrame, Error>> + Send + 'static>>;

    /// Perform handshake with remote
    async fn handshake(&mut self) -> Result<(), Error> {
        Ok(())
    }

    /// Return a stream of frames to apply to the database
    async fn next_frames(&mut self) -> Result<Self::FrameStream, Error> {
        match self.frames.take() {
            Some(Frames::Vec(f)) => {
                let iter = f.into_iter().map(|f| RpcFrame { data: f.bytes(), timestamp: None, durable_frame_no: None }).map(Ok);
                Ok(Box::pin(tokio_stream::iter(iter)))
            }
            Some(f @ Frames::Snapshot(_)) => {
                self.frames.replace(f);
                Err(Error::NeedSnapshot)
            }
            None => Ok(Box::pin(tokio_stream::empty())),
        }
    }

    /// Return a snapshot for the current replication index. Called after next_frame has returned a
    /// NeedSnapshot error
    async fn snapshot(&mut self) -> Result<Self::FrameStream, Error> {
        match self.frames.take() {
            Some(Frames::Snapshot(frames)) => {
                let size_after = frames.header().size_after.get();
                let stream = async_stream::try_stream! {
                    let s = frames.into_stream_mut().map_err(|e| Error::Client(Box::new(e))).peekable();
                    tokio::pin!(s);
                    while let Some(mut next) = s.as_mut().next().await.transpose()? {
                        if s.as_mut().peek().await.is_none() {
                            next.header_mut().size_after = size_after.into();
                        }
                        let frame = Frame::from(next);
                        yield RpcFrame { data: frame.bytes(), timestamp: None, durable_frame_no: None };
                    }
                };

                Ok(Box::pin(stream))
            }
            Some(Frames::Vec(_)) | None => Ok(Box::pin(tokio_stream::empty())),
        }
    }

    /// set the new commit frame_no
    async fn commit_frame_no(&mut self, frame_no: FrameNo) -> Result<(), Error> {
        self.meta.set_commit_frame_no(frame_no).await?;
        Ok(())
    }

    fn committed_frame_no(&self) -> Option<FrameNo> {
        self.meta.current_frame_no()
    }

    fn rollback(&mut self) {}
}

#[cfg(test)]
mod test {
    use libsql_replication::{frame::FrameHeader, snapshot::SnapshotFile};
    use tempfile::tempdir;
    use zerocopy::FromBytes;

    use super::*;

    #[tokio::test]
    async fn snapshot_stream_commited() {
        let tmp = tempdir().unwrap();
        let snapshot = SnapshotFile::open("assets/test/snapshot.snap", None)
            .await
            .unwrap();
        let mut client = LocalClient::new(&tmp.path().join("data")).await.unwrap();
        client.load_frames(Frames::Snapshot(snapshot));
        let mut s = client.snapshot().await.unwrap();
        assert!(matches!(s.next().await, Some(Ok(_))));
        let last = s.next().await.unwrap().unwrap();
        let header: FrameHeader = FrameHeader::read_from_prefix(&last.data[..]).unwrap();
        assert_eq!(header.size_after.get(), 2);
        assert!(s.next().await.is_none());
    }
}
