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

struct RepositoryScan {
    generation: Option<u64>,
    unreachable: Vec<Mapping>,
    kept: Vec<BlobId>,
}

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
        let scan = self.scan_repository(&repository, min_age).await?;
        if scan.unreachable.is_empty() {
            return Ok(GcReport::default());
        }
        let ids = scan.unreachable
            .iter()
            .map(|mapping| mapping.id.clone())
            .collect();
        let rows = self.storage.blob_rows(ids).await?;
        // Each mapping is dropped only while it is still the one found here:
        // a blob another collection unmapped and an upload mapped again since
        // keeps its new mapping, which no repository generation tracks.
        let unmapped = self.storage.unmap_hosted_artifacts_with(
            &repository,
            scan.generation,
            scan.unreachable,
            |blob| {
                let size = rows.get(&blob.id).map_or(0, |row| row.size);
                tracing::info!(target: "pnpr::audit", action = "gc.remove_archive", actor = "-", key = %blob.key(), size, repository = %name, "removed archive");
            },
        );
        // A repository that moved on since it was read is left for a later
        // collection.
        let Some(removed) = unmapped.await? else { return Ok(GcReport::default()) };
        Ok(report_of(&removed, |blob| rows.get(&blob.id).map_or(0, |row| row.size)))
    }

    /// The live rows of the image blobs that unmapping every repository's
    /// unreachable files would leave unmapped, for a dry run.
    pub(super) async fn unreachable_images(
        &self,
        min_age: Duration,
    ) -> Result<Vec<(BlobId, BlobRow)>> {
        let mut candidates = HashSet::new();
        let mut kept = HashSet::new();
        for name in self.storage.mapped_packages().await? {
            let repository = CanonicalPackageName::from_stored(&name, Ecosystem::Oci);
            let scan = self.scan_repository(&repository, min_age).await?;
            candidates.extend(scan.unreachable.into_iter().map(|mapping| mapping.id));
            kept.extend(scan.kept);
        }
        let ids = candidates
            .into_iter()
            .filter(|id| !kept.contains(id))
            .collect();
        let rows = self.storage.blob_rows(ids).await?;
        Ok(rows
            .into_iter()
            .filter(|(_, row)| row.state == BlobState::Live)
            .collect())
    }

    /// Partition `repository`'s mapped files into those mapped at least
    /// `min_age` ago that no retained manifest reaches, and those `repository`
    /// keeps, at the generation they were read at (`None` without metadata).
    async fn scan_repository(
        &self,
        repository: &CanonicalPackageName,
        min_age: Duration,
    ) -> Result<RepositoryScan> {
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
        let mut unreachable = Vec::new();
        let mut kept = Vec::new();
        for mapping in mappings {
            if !reachable.contains(&mapping.artifact)
                && mapping.mapped_at.elapsed().unwrap_or_default() >= min_age
            {
                unreachable.push(mapping);
            } else {
                kept.push(mapping.id);
            }
        }
        Ok(RepositoryScan { generation: base.package_generation(), unreachable, kept })
    }
}
