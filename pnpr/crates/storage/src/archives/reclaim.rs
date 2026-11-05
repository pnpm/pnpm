//! Reclaiming a namespace's archives while replicas keep publishing (see
//! [`crate::metadata::BlobRow`]).
//!
//! A reclaim moves a live incarnation nothing maps to `reclaiming`, removes
//! exactly its key, and records it `gone`. An incarnation never committed,
//! such as one a refused publish stored, is first recorded `gone` itself when
//! a commit could still make it live, and then removed. No step depends on
//! timing: a key is never written again once a reclaim may remove it, so a
//! removal that lands late removes bytes nothing can reach.

use crate::{
    BlobId, Storage, StoredArchive,
    metadata::{BlobRef, BlobRow, Uncommitted, Unmapping},
    parse_blob_key,
};
use futures_util::TryStreamExt as _;
use pnpr_error::Result;
use std::{collections::BTreeMap, time::Duration};

impl Storage {
    /// Reclaim the blobs among `ids` that are live and that nothing maps.
    /// Returns the incarnations removed. One whose removal failed stays
    /// `reclaiming` for a later reclaim, and the error is returned once the
    /// rest are recorded.
    pub async fn reclaim_blobs(&self, ids: Vec<BlobId>) -> Result<Vec<BlobRef>> {
        let (catalog, ecosystem) = (self.metadata.catalog(), self.metadata.ecosystem());
        let marked = self.database.mark_reclaiming(catalog, ecosystem, ids).await?;
        self.remove_reclaiming(marked).await
    }

    /// The live blobs of this namespace that nothing maps, with their rows.
    pub async fn unmapped_blobs(&self) -> Result<BTreeMap<BlobId, BlobRow>> {
        let (catalog, ecosystem) = (self.metadata.catalog(), self.metadata.ecosystem());
        self.database.unmapped_blobs(catalog, ecosystem).await
    }

    /// Finish the reclaims of this namespace that have been `reclaiming` for
    /// at least `lease`: reclaims that stopped before they recorded their
    /// removal. Returns the incarnations removed.
    pub async fn finish_stale_reclaims(&self, lease: Duration) -> Result<Vec<BlobRef>> {
        let (catalog, ecosystem) = (self.metadata.catalog(), self.metadata.ecosystem());
        let stale = self.database.stale_reclaims(catalog, ecosystem, lease).await?;
        self.remove_reclaiming(stale).await
    }

    /// Remove the keys of `stored`, incarnations stored in this namespace
    /// that no commit made the live one, when they can never be committed;
    /// one that may still be, by a publish on its way, is first fenced off
    /// when its flag says so (see [`crate::metadata::SqlMetadata::settle_stored`]).
    /// Returns the incarnations removed.
    pub async fn remove_uncommitted(&self, stored: Vec<Uncommitted>) -> Result<Vec<BlobRef>> {
        let (catalog, ecosystem) = (self.metadata.catalog(), self.metadata.ecosystem());
        let removable = self.database.settle_stored(catalog, ecosystem, stored).await?;
        let mut removed = Vec::with_capacity(removable.len());
        let mut failure = None;
        for blob in removable {
            match self.hosted.remove_archive(&blob.key()).await {
                Ok(_) => removed.push(blob),
                Err(error) => failure = Some(error),
            }
        }
        failure.map_or(Ok(removed), Err)
    }

    /// The incarnations stored in this namespace, with their sizes and
    /// when they were stored. A name that is no key pnpr stores archives
    /// under is left out, and never removed.
    pub async fn stored_incarnations(&self) -> Result<Vec<(BlobRef, StoredArchive)>> {
        self.hosted_archives()
            .try_filter_map(|stored| async move {
                Ok(parse_blob_key(&stored.name)
                    .map(|(id, incarnation)| (BlobRef { id, incarnation }, stored)))
            })
            .try_collect()
            .await
    }

    /// Remove the keys of `reclaiming` and record them gone.
    async fn remove_reclaiming(&self, reclaiming: Vec<BlobRef>) -> Result<Vec<BlobRef>> {
        let mut removed = Vec::with_capacity(reclaiming.len());
        let mut failure = None;
        for blob in reclaiming {
            match self.hosted.remove_archive(&blob.key()).await {
                Ok(_) => removed.push(blob),
                Err(error) => {
                    tracing::warn!(
                        key = %blob.key(),
                        %error,
                        "a reclaimed archive was not removed; it stays reclaiming",
                    );
                    failure = Some(error);
                }
            }
        }
        let (catalog, ecosystem) = (self.metadata.catalog(), self.metadata.ecosystem());
        self.database.finish_reclaims(catalog, ecosystem, removed.clone()).await?;
        failure.map_or(Ok(removed), Err)
    }

    /// Drop the mappings of `artifacts` of `name`, while the package is still
    /// at `expected`, and reclaim the blobs nothing maps any more. `None`
    /// when the package moved on and nothing was dropped.
    pub async fn unmap_hosted_artifacts(
        &self,
        name: &pnpr_package_name::CanonicalPackageName,
        expected: Option<u64>,
        artifacts: Vec<impl Into<Unmapping>>,
    ) -> Result<Option<Vec<BlobRef>>> {
        let (catalog, ecosystem) = (self.metadata.catalog(), self.metadata.ecosystem());
        let unmappings = artifacts
            .into_iter()
            .map(Into::into)
            .collect();
        let unmapped = self.database.unmap_artifacts(
            catalog,
            ecosystem,
            name.as_str(),
            (expected, unmappings),
        )
        .await?;
        match unmapped {
            Some(ids) => Ok(Some(self.reclaim_blobs(ids).await?)),
            None => Ok(None),
        }
    }

    /// The mapped files of `name` and the package's generation, `None`
    /// without metadata.
    pub async fn package_mappings(
        &self,
        name: &pnpr_package_name::CanonicalPackageName,
    ) -> Result<(Option<u64>, Vec<crate::metadata::Mapping>)> {
        let (catalog, ecosystem) = (self.metadata.catalog(), self.metadata.ecosystem());
        self.database.package_mappings(catalog, ecosystem, name.as_str()).await
    }

    /// The packages of this namespace with a mapped file.
    pub async fn mapped_packages(&self) -> Result<Vec<String>> {
        let (catalog, ecosystem) = (self.metadata.catalog(), self.metadata.ecosystem());
        self.database.mapped_packages(catalog, ecosystem).await
    }
}
