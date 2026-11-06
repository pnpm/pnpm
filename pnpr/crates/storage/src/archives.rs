//! A namespace's archives: stored once by the identity of their bytes, and
//! reached through the files of packages the metadata maps to them (see
//! [`crate::metadata::BlobRow`]).

#[cfg(test)]
pub(crate) mod testing;

mod reclaim;
#[cfg(test)]
mod tests;

use crate::{
    BlobId, BlobSlot, GetRange, RangedBlob, Storage,
    metadata::{ArtifactBlob, BlobLookup, BlobRef, BlobRow, PublishBase, PublishIncarnation},
};
use axum::body::Body;
use pnpr_error::Result;
use pnpr_package_name::CanonicalPackageName;
use std::collections::BTreeMap;

/// A staged archive, stored or found stored under an incarnation.
#[derive(Debug, Clone)]
pub struct PromotedArchive {
    /// The file and the incarnation holding its bytes, which the commit
    /// depends on and maps.
    pub artifact: ArtifactBlob,
    /// Whether this promotion stored the incarnation, rather than finding it
    /// live: one that was, and whose commit is refused, is discarded.
    pub stored: bool,
}

impl Storage {
    /// Open the hosted archive `filename` of `name`, through the blob its
    /// file is mapped to. `None` when the package maps no such file.
    pub async fn open_hosted_blob(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<Option<(Body, Option<u64>)>> {
        let Some(blob) = self.metadata.artifact_blob(name, filename).await? else {
            return Ok(None);
        };
        self.hosted.open_archive(&blob.key()).await
    }

    /// Open only the requested bytes, without reading the preceding content.
    pub async fn open_hosted_blob_range(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
        range: &GetRange,
    ) -> Result<Option<RangedBlob>> {
        let Some(blob) = self.metadata.artifact_blob(name, filename).await? else {
            return Ok(None);
        };
        self.hosted.open_archive_range(&blob.key(), range).await
    }

    /// The live incarnation of the blob the file `filename` of `name` is
    /// mapped to.
    pub async fn hosted_blob_of(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<Option<BlobRef>> {
        self.metadata.artifact_blob(name, filename).await
    }

    /// Open the archive stored under `blob`'s incarnation, mapped or not.
    pub async fn open_stored_archive(&self, blob: &BlobRef) -> Result<Option<(Body, Option<u64>)>> {
        self.hosted.open_archive(&blob.key()).await
    }

    /// Read `name`'s document for a publish, with the rows `lookup` asks for
    /// in the same round trip.
    pub async fn read_hosted_document_for_publish(
        &self,
        name: &CanonicalPackageName,
        lookup: &BlobLookup,
    ) -> Result<PublishBase> {
        self.metadata.read_for_publish(name, lookup).await
    }

    /// The rows of `ids` in this namespace, read from the primary.
    pub async fn blob_rows(&self, ids: Vec<BlobId>) -> Result<BTreeMap<BlobId, BlobRow>> {
        let (catalog, ecosystem) = (self.metadata.catalog(), self.metadata.ecosystem());
        self.database.blob_rows(catalog, ecosystem, ids).await
    }

    /// Reserve a staging slot for the archive `filename` of `name`. The
    /// publish flow streams the decode + hash + write through `std::fs`
    /// inside `spawn_blocking` and only needs the path; identify it with
    /// [`Self::identify_blob_slot`] and promote it with
    /// [`Self::promote_blob_slot`].
    pub async fn reserve_hosted_blob(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<BlobSlot> {
        let tmp_path = self.hosted.reserve_archive_tmp().await?;
        Ok(BlobSlot {
            tmp_path,
            name: name.clone(),
            filename: filename.to_string(),
            version: String::new(),
        })
    }

    /// The identity and size of the bytes written to `slot`.
    pub async fn identify_blob_slot(&self, slot: &BlobSlot) -> Result<(BlobId, u64)> {
        let path = slot.tmp_path.clone();
        let ecosystem = self.metadata.ecosystem();
        let identified = tokio::task::spawn_blocking(move || {
            let size = std::fs::metadata(&path)?.len();
            Ok::<_, std::io::Error>((BlobId::of_file(ecosystem, &path)?, size))
        });
        Ok(identified.await??)
    }

    /// Promote `slot`, holding the bytes of `id`, whose blob has `row`: the
    /// live incarnation is reused and nothing is stored; otherwise the next
    /// incarnation is stored. Either way the staged file is consumed, and
    /// the commit that maps the file decides whether the incarnation holds
    /// (see [`crate::metadata::BlobFence`]).
    pub async fn promote_blob_slot(
        &self,
        slot: BlobSlot,
        (id, size): (BlobId, u64),
        row: Option<&BlobRow>,
    ) -> Result<PromotedArchive> {
        let (incarnation, store) = match BlobRow::incarnation_for_publish(row) {
            PublishIncarnation::Reuse(incarnation) => (incarnation, false),
            PublishIncarnation::Store(incarnation) => (incarnation, true),
        };
        let blob = BlobRef { id, incarnation };
        if store {
            self.hosted.put_archive(&slot.tmp_path, &blob.key()).await?;
        } else {
            let _ = tokio::fs::remove_file(&slot.tmp_path).await;
        }
        let artifact = ArtifactBlob {
            ecosystem: self.metadata.ecosystem(),
            name: slot.name.as_str().to_string(),
            version: slot.version,
            artifact: slot.filename,
            blob,
            size,
        };
        Ok(PromotedArchive { artifact, stored: store })
    }

    /// Map `artifact` outside a publication commit, as an image blob's
    /// upload does, refused as a commit depending on it would be.
    pub async fn map_hosted_artifact(&self, artifact: ArtifactBlob) -> Result<()> {
        self.database.map_artifact(self.metadata.catalog(), artifact).await
    }

    /// Remove the package's metadata, then reclaim the blobs its files were
    /// mapped to that nothing else maps. Reports whether there was any.
    pub async fn remove_package(&self, name: &CanonicalPackageName) -> Result<bool> {
        let Some(ids) = self.metadata.remove_package(name).await? else {
            return Ok(false);
        };
        if let Err(error) = self.reclaim_blobs(ids).await {
            tracing::warn!(
                package = %name.as_str(),
                %error,
                "the archives of a removed package were not reclaimed; `pnpr gc` reclaims them",
            );
        }
        Ok(true)
    }

    /// The blobs the files of `versions` of `name` are mapped to: what a
    /// write that removes those versions may leave for
    /// [`Self::reclaim_blobs`].
    pub async fn version_blobs(
        &self,
        name: &CanonicalPackageName,
        versions: &[String],
    ) -> Result<Vec<BlobId>> {
        if versions.is_empty() {
            return Ok(Vec::new());
        }
        self.metadata.version_blobs(name, versions).await
    }
}

impl Storage {
    /// Remove the archive staging files of this namespace that a process
    /// stopped before storing, once they are older than `max_age`. Returns
    /// how many went.
    pub async fn sweep_archive_temps(&self, max_age: std::time::Duration) -> Result<usize> {
        crate::atomic_write::remove_stale_temps(&self.hosted.archive_tmp_base(), max_age).await
    }

    /// Store `bytes`, read from another store under `blob`'s key, under the
    /// same key here, as a copy of a store between backends does.
    pub async fn store_archive_copy(&self, blob: &BlobRef, bytes: &[u8]) -> Result<()> {
        let tmp_path = self.hosted.reserve_archive_tmp().await?;
        if let Err(error) = tokio::fs::write(&tmp_path, bytes).await {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(error.into());
        }
        self.hosted.put_archive(&tmp_path, &blob.key()).await
    }
}

impl Storage {
    /// The ecosystem of this namespace.
    #[must_use]
    pub fn ecosystem(&self) -> pnpr_package_name::Ecosystem {
        self.metadata.ecosystem()
    }
}
