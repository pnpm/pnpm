//! The image blobs of an OCI namespace no retained manifest reaches.
//!
//! An image blob is mapped into its repository when its upload completes,
//! before any manifest names it, so its mapping is what keeps it until a
//! manifest does. A collection drops the mappings of blobs mapped longer
//! than the minimum age ago that no retained manifest reaches, while the
//! repository is still at the generation they were found unreachable at;
//! the bytes nothing else maps are then reclaimed.

use super::{Collection, GcReport, liveness, report_of};
use crate::{Ecosystem, Result};
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::{
    BlobId,
    metadata::{BlobLookup, BlobRow, BlobState, Mapping},
};
use std::{collections::HashSet, time::Duration};

impl Collection<'_> {
    /// Drop the mappings of every repository's unreachable image blobs, and
    /// reclaim the bytes nothing maps any more. A repository that fails is
    /// skipped; the first failure is returned once the rest are done.
    pub(super) async fn unmap_unreachable_images(&self, min_age: Duration) -> Result<GcReport> {
        let mut report = GcReport::default();
        let mut failure = None;
        for name in self.storage.mapped_packages().await? {
            match self.unmap_repository(&name, min_age).await {
                Ok(reclaimed) => report.add(reclaimed),
                Err(error) => {
                    tracing::warn!(repository = %name, %error, "image blobs not collected");
                    failure.get_or_insert(error);
                }
            }
        }
        failure.map_or(Ok(report), Err)
    }

    async fn unmap_repository(&self, name: &str, min_age: Duration) -> Result<GcReport> {
        let repository = CanonicalPackageName::from_stored(name, Ecosystem::Oci);
        let (expected, unreachable) = self.unreachable_in(&repository, min_age).await?;
        if unreachable.is_empty() {
            return Ok(GcReport::default());
        }
        let ids = unreachable
            .iter()
            .map(|mapping| mapping.id.clone())
            .collect();
        let rows = self.storage.blob_rows(ids).await?;
        // Each mapping is dropped only while it is still the one found here:
        // a blob another collection unmapped and an upload mapped again since
        // keeps its new mapping, which no repository generation tracks.
        let unmapped = self.storage.unmap_hosted_artifacts(&repository, expected, unreachable);
        // A repository that moved on since it was read is left for a later
        // collection.
        let Some(removed) = unmapped.await? else { return Ok(GcReport::default()) };
        Ok(report_of(&removed, |blob| rows.get(&blob.id).map_or(0, |row| row.size)))
    }

    /// The live rows of the image blobs no retained manifest reaches, for a
    /// dry run. Bytes another repository maps as well are counted too.
    pub(super) async fn unreachable_images(
        &self,
        min_age: Duration,
    ) -> Result<Vec<(BlobId, BlobRow)>> {
        let mut found = Vec::new();
        for name in self.storage.mapped_packages().await? {
            let repository = CanonicalPackageName::from_stored(&name, Ecosystem::Oci);
            let (_, unreachable) = self.unreachable_in(&repository, min_age).await?;
            let ids = unreachable
                .into_iter()
                .map(|mapping| mapping.id)
                .collect();
            let rows = self.storage.blob_rows(ids).await?;
            found.extend(
                rows.into_iter()
                    .filter(|(_, row)| row.state == BlobState::Live),
            );
        }
        Ok(found)
    }

    /// The files of `repository` mapped more than `min_age` ago that no
    /// retained manifest reaches, with the repository's generation they were
    /// found unreachable at, `None` when it has no metadata.
    async fn unreachable_in(
        &self,
        repository: &CanonicalPackageName,
        min_age: Duration,
    ) -> Result<(Option<u64>, Vec<Mapping>)> {
        let (_, mappings) = self.storage.package_mappings(repository).await?;
        let lookup = BlobLookup::default();
        let base = self.storage.read_hosted_document_for_publish(repository, &lookup).await?;
        let reachable = match &base.stored {
            Some(stored) => {
                let document = liveness::parse_image_document(&stored.bytes)?;
                liveness::referenced_document_blobs(
                    self.storage,
                    repository,
                    &document,
                    self.manifest_limit,
                )
                .await?
            }
            None => HashSet::new(),
        };
        let unreachable = mappings
            .into_iter()
            .filter(|mapping| !reachable.contains(&mapping.artifact))
            .filter(|mapping| mapping.mapped_at.elapsed().unwrap_or_default() >= min_age)
            .collect();
        Ok((base.package_generation(), unreachable))
    }
}
