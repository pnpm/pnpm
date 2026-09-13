use super::{
    Duration, HashMap, HashSet, Path, PutMode, RegistryError, RemoteUploadStore, Result, StreamExt,
    UpdateVersion, VersionedRecord, is_upload_id,
};
/// What one sweep has learned about the uploads it has listed so far.
#[derive(Default)]
struct SweepState {
    /// Whether an upload id's session is still open.
    sessions: HashMap<String, bool>,
    /// Upload ids whose session object could not be read this sweep.
    unreadable: HashSet<String>,
}

/// An upload's session as this sweep found it.
enum SweepSession {
    Record(VersionedRecord),
    /// The session object is gone, so anything left under the id is orphaned.
    Missing,
    /// The session could not be read; leave the whole upload alone.
    Unreadable,
}

impl RemoteUploadStore {
    pub(in crate::upload) async fn sweep(&self, max_age: Duration) -> Result<usize> {
        let mut listing = self.store.list(Some(&Path::from(self.prefix.clone())));
        let mut swept = 0;
        let mut state = SweepState::default();
        while let Some(meta) = listing.next().await {
            let meta = meta?;
            let Some(relative) = meta.location.as_ref().strip_prefix(&self.prefix) else {
                continue;
            };
            let Some((id, object)) = relative.split_once('/') else {
                continue;
            };
            let age = std::time::SystemTime::from(meta.last_modified).elapsed().unwrap_or_default();
            if !is_upload_id(id) || age <= max_age || state.unreadable.contains(id) {
                continue;
            }
            if self.expire_upload_object(id, object, &meta, &mut state).await? {
                swept += 1;
            }
        }
        Ok(swept)
    }

    /// Expire one listed object of an upload no longer being written to,
    /// reporting whether the upload itself was reclaimed.
    async fn expire_upload_object(
        &self,
        id: &str,
        object: &str,
        meta: &object_store::ObjectMeta,
        state: &mut SweepState,
    ) -> Result<bool> {
        if object == "session.json" {
            return self.expire_upload_session(id, meta, state).await;
        }
        self.expire_upload_chunk(id, meta, state).await?;
        Ok(false)
    }

    /// Drop a chunk whose session is already closed or gone. A chunk of a live
    /// session stays: the push may still be running.
    async fn expire_upload_chunk(
        &self,
        id: &str,
        meta: &object_store::ObjectMeta,
        state: &mut SweepState,
    ) -> Result<()> {
        if let Some(live) = state.sessions.get(id) {
            if !live {
                self.remove(&meta.location).await?;
            }
            return Ok(());
        }
        let record = match self.sweep_session(id, state).await? {
            SweepSession::Record(record) => record,
            SweepSession::Missing => {
                state.sessions.insert(id.to_string(), false);
                self.remove(&meta.location).await?;
                return Ok(());
            }
            SweepSession::Unreadable => return Ok(()),
        };
        state.sessions.insert(id.to_string(), !record.record.closed);
        if record.record.closed {
            self.remove(&meta.location).await?;
        }
        Ok(())
    }

    /// Close an expired session and reclaim what it left behind.
    async fn expire_upload_session(
        &self,
        id: &str,
        meta: &object_store::ObjectMeta,
        state: &mut SweepState,
    ) -> Result<bool> {
        let mut record = match self.sweep_session(id, state).await? {
            SweepSession::Record(record) => record,
            SweepSession::Missing => {
                state.sessions.insert(id.to_string(), false);
                self.remove(&meta.location).await?;
                return Ok(false);
            }
            SweepSession::Unreadable => return Ok(false),
        };
        // The listing may predate a successful append. Compare the listed
        // version, never a freshly read version, when expiring it.
        record.record.closed = true;
        let version = UpdateVersion {
            e_tag: meta.e_tag.clone(),
            version: meta.version.clone(),
        };
        match self.write(id, &record.record, PutMode::Update(version)).await {
            Ok(_) => {}
            Err(RegistryError::BlobUploadConflict { .. }) => return Ok(false),
            Err(error) => return Err(error),
        }
        state.sessions.insert(id.to_string(), false);
        self.remove_chunks(id).await?;
        self.remove(&meta.location).await?;
        Ok(true)
    }

    /// Read an upload's session record, remembering an unreadable one so the
    /// rest of the sweep leaves that upload alone.
    async fn sweep_session(&self, id: &str, state: &mut SweepState) -> Result<SweepSession> {
        match self.read(id).await {
            Ok(Some(record)) => Ok(SweepSession::Record(record)),
            Ok(None) => Ok(SweepSession::Missing),
            Err(error) => {
                tracing::warn!(error = %error.log_message(), upload = id, "skipping unreadable upload session");
                state.unreadable.insert(id.to_string());
                Ok(SweepSession::Unreadable)
            }
        }
    }
}
