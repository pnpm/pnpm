use std::{fmt::Display, time::Instant};

use bytes::Bytes;
use futures_util::{Stream, StreamExt as _};
use object_store::{ObjectStoreExt as _, PutPayload, WriteMultipart};
use pnpm_shared_artifact_protocol::{
    ArtifactBlobRequest, MAX_BLOB_SIZE, OwnerScope, blob_id, verify_blob_digest,
};
use pnpr_error::{RegistryError, Result};
use sha2::{Digest as _, Sha512};

use super::{
    ACTIVE_PUBLICATION_EXPIRY, OrgAccess, PUBLICATION_RENEWAL_INTERVAL, SharedArtifactStore,
    artifact_operation_id, bad_request, owner_key, protocol_error, publisher_owner_key,
    staged_record_path,
};

/// A blob up to this size is collected and written in one request. A larger
/// one is written in parts as it arrives, so the server never holds it whole.
const SINGLE_WRITE_LIMIT: u64 = 8 * 1024 * 1024;

/// Parts of one blob in flight to the object store at once.
const PART_CONCURRENCY: usize = 4;

/// One blob being uploaded, and where in its owner's namespace it goes.
struct UploadedBlob<'a> {
    owner: String,
    id: String,
    integrity: &'a str,
    size: u64,
}

impl UploadedBlob<'_> {
    fn path(&self) -> String {
        format!("{}/blobs/{}", self.owner, self.id)
    }
}

/// Why a blob was not stored.
enum WriteFailure {
    /// The upload was refused or interrupted before anything was committed.
    Rejected(RegistryError),
    /// The object store failed while committing, so the blob may be stored.
    Store(RegistryError),
}

impl SharedArtifactStore {
    /// The stored size of the blob `integrity` names in `owner`'s namespace.
    /// `None` when it is not stored or `caller` may not read it.
    pub async fn blob_size(
        &self,
        caller: &(impl OrgAccess + ?Sized),
        owner: &OwnerScope,
        integrity: &str,
    ) -> Result<Option<u64>> {
        let request =
            ArtifactBlobRequest { owner: owner.clone(), integrity: integrity.to_string() };
        request.validate().map_err(|err| protocol_error(&err))?;
        let owner = match owner_key(caller, owner) {
            Ok(owner) => owner,
            Err(RegistryError::Forbidden { .. }) => return Ok(None),
            Err(err) => return Err(err),
        };
        let id = blob_id(integrity).map_err(|err| protocol_error(&err))?;
        self.stored_size(&format!("{owner}/blobs/{id}")).await
    }

    /// Store one blob ahead of the publication that names it, streamed from
    /// `body`. Nothing becomes visible unless the bytes match `integrity` and
    /// `size`. `Ok(false)` when the blob is already stored.
    ///
    /// The blob is charged to `owner` at once. Reclamation leaves it alone
    /// for as long as a publication may take, so the publication that
    /// references it has time to arrive.
    pub async fn store_blob<StreamError: Display>(
        &self,
        caller: &(impl OrgAccess + ?Sized),
        owner: &OwnerScope,
        integrity: &str,
        size: u64,
        body: impl Stream<Item = std::result::Result<Bytes, StreamError>> + Unpin,
    ) -> Result<bool> {
        let request =
            ArtifactBlobRequest { owner: owner.clone(), integrity: integrity.to_string() };
        request.validate().map_err(|err| protocol_error(&err))?;
        if size > MAX_BLOB_SIZE {
            return Err(bad_request(format!("blob exceeds the {MAX_BLOB_SIZE}-byte limit")));
        }
        let blob = UploadedBlob {
            owner: publisher_owner_key(caller, owner)?,
            id: blob_id(integrity).map_err(|err| protocol_error(&err))?,
            integrity,
            size,
        };
        if self
            .stored_size(&blob.path())
            .await?
            .is_some()
        {
            return Ok(false);
        }
        let publication = artifact_operation_id()?;
        self.begin_publication(&publication).await?;
        let mut reclamation_needed = false;
        let result = self.while_renewing(
            &publication,
            PUBLICATION_RENEWAL_INTERVAL,
            self.store_registered_blob(&blob, body, &publication, &mut reclamation_needed),
        )
        .await;
        self.complete_publication(&publication, reclamation_needed, result).await
    }

    /// [`Self::store_blob`] once the upload is registered as a publication,
    /// which keeps reclamation from running beside it.
    async fn store_registered_blob<StreamError: Display>(
        &self,
        blob: &UploadedBlob<'_>,
        body: impl Stream<Item = std::result::Result<Bytes, StreamError>> + Unpin,
        publication: &str,
        reclamation_needed: &mut bool,
    ) -> Result<bool> {
        let started = Instant::now();
        let (owner, size) = (blob.owner.as_str(), blob.size);
        if let Err(error) = self.reserve_quota(owner, size).await {
            *reclamation_needed = matches!(&error, RegistryError::ObjectStore(_));
            return Err(error);
        }
        let created = match self.write_blob(&blob.path(), blob.integrity, size, body).await {
            Ok(created) => created,
            Err(WriteFailure::Rejected(error)) => {
                self.release_uncommitted(owner, size, 0).await?;
                return Err(error);
            }
            // A commit that failed can have landed anyway. Keep its
            // reservation until reclamation counts the stored objects.
            Err(WriteFailure::Store(error)) => {
                *reclamation_needed = true;
                return Err(error);
            }
        };
        self.release_uncommitted(owner, size, if created { size } else { 0 }).await?;
        // Until now this registered upload keeps reclamation from running.
        // From now on the record keeps it off the blob, for as long as a
        // publication may take to arrive, so its age has to start here.
        let record = self.object_path(&staged_record_path(owner, &blob.id));
        self.store.put(&record, PutPayload::new()).await?;
        *reclamation_needed = started.elapsed() >= ACTIVE_PUBLICATION_EXPIRY;
        if *reclamation_needed {
            self.begin_publication(publication).await?;
        }
        Ok(created)
    }

    /// Write `body` to `path` once it proves to be `size` bytes matching
    /// `integrity`, reporting whether this write created the object.
    ///
    /// A large blob is written in parts, and the parts are committed only
    /// after the last byte is checked and only when no other upload stored
    /// the blob meanwhile. Two uploads that commit at the same moment both
    /// store identical bytes, and both are charged until reclamation
    /// recounts.
    async fn write_blob<StreamError: Display>(
        &self,
        path: &str,
        integrity: &str,
        size: u64,
        mut body: impl Stream<Item = std::result::Result<Bytes, StreamError>> + Unpin,
    ) -> std::result::Result<bool, WriteFailure> {
        let mut received = ReceivedBlob { integrity, size, hasher: Sha512::new(), bytes: 0 };
        if size <= SINGLE_WRITE_LIMIT {
            let mut collected = Vec::with_capacity(usize::try_from(size).unwrap_or_default());
            while let Some(chunk) = body.next().await {
                let chunk = received.accept(chunk).map_err(WriteFailure::Rejected)?;
                collected.extend_from_slice(&chunk);
            }
            received.finish().map_err(WriteFailure::Rejected)?;
            return self.create_object(path, collected).await.map_err(WriteFailure::Store);
        }
        let upload = self.store
            .put_multipart(&self.object_path(path))
            .await
            .map_err(|error| WriteFailure::Rejected(error.into()))?;
        let mut writer = WriteMultipart::new(upload);
        let streamed = async {
            while let Some(chunk) = body.next().await {
                let chunk = received.accept(chunk)?;
                writer.wait_for_capacity(PART_CONCURRENCY).await?;
                writer.put(chunk);
            }
            received.finish()
        }
        .await;
        if let Err(error) = streamed {
            if let Err(abort_error) = writer.abort().await {
                tracing::warn!(%abort_error, "an abandoned artifact blob upload was not cleaned up");
            }
            return Err(WriteFailure::Rejected(error));
        }
        if self.stored_size(path).await.is_ok_and(|stored| stored.is_some()) {
            if let Err(abort_error) = writer.abort().await {
                tracing::warn!(%abort_error, "a duplicate artifact blob upload was not cleaned up");
            }
            return Ok(false);
        }
        writer.finish().await.map_err(|error| WriteFailure::Store(error.into()))?;
        Ok(true)
    }

    pub(super) async fn stored_size(&self, relative: &str) -> Result<Option<u64>> {
        match self.store.head(&self.object_path(relative)).await {
            Ok(meta) => Ok(Some(meta.size)),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

/// What has arrived of one uploaded blob, checked as it arrives.
struct ReceivedBlob<'a> {
    integrity: &'a str,
    size: u64,
    hasher: Sha512,
    bytes: u64,
}

impl ReceivedBlob<'_> {
    fn accept<StreamError: Display>(
        &mut self,
        chunk: std::result::Result<Bytes, StreamError>,
    ) -> Result<Bytes> {
        let chunk = chunk.map_err(|error| {
            bad_request(format!("the blob upload was interrupted: {error}"))
        })?;
        self.bytes = self.bytes.saturating_add(chunk.len() as u64);
        if self.bytes > self.size {
            return Err(bad_request(format!(
                "the blob is larger than its declared {} bytes",
                self.size,
            )));
        }
        self.hasher.update(&chunk);
        Ok(chunk)
    }

    fn finish(&mut self) -> Result<()> {
        if self.bytes != self.size {
            return Err(bad_request(format!(
                "the blob has {} bytes but declares {}",
                self.bytes, self.size,
            )));
        }
        verify_blob_digest(self.integrity, &std::mem::take(&mut self.hasher).finalize())
            .map_err(|err| protocol_error(&err))
    }
}
