//! `pnpr gc`: reclaiming the archives nothing maps, in every hosted
//! registry, while the registry keeps serving and other replicas keep
//! publishing.
//!
//! A namespace stores each archive's bytes once, under an incarnation of
//! their identity, and its metadata maps every published file to one (see
//! [`pnpr_storage::metadata::BlobRow`]). A collection reclaims what no file
//! maps: the bytes of unpublished versions and packages, image blobs no
//! retained manifest reaches, incarnations a refused or interrupted publish
//! stored, and reclaims that stopped before they finished. Every removal is
//! fenced by the metadata database, never by timing, so a commit that
//! depends on bytes being removed is refused rather than left pointing at
//! nothing.

pub(crate) mod images;
pub(crate) mod liveness;

use crate::{Config, Ecosystem, RegistryError, Result};
use pnpr_storage::{
    Storage, StoredArchive,
    metadata::{BlobRef, BlobRow, BlobState, Uncommitted},
};
use std::{collections::BTreeMap, time::Duration};

/// What `pnpr gc` collects, and how.
#[derive(Debug, Default, Clone)]
pub struct GcOptions {
    /// The hosted registries to collect in; every hosted registry when
    /// empty.
    pub registries: Vec<String>,
    /// Archives stored, and image blobs mapped, more recently than this are
    /// kept, so a publish or push still on its way is not refused for them.
    pub min_age: Duration,
    /// Report what would be removed without removing it.
    pub dry_run: bool,
}

/// What a collection removed, or would remove in a dry run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GcReport {
    pub archives: usize,
    pub bytes: u64,
}

impl GcReport {
    fn add(&mut self, other: Self) {
        self.archives += other.archives;
        self.bytes += other.bytes;
    }
}

/// Collect unreferenced archives as `options` say.
pub async fn collect_archives(config: &Config, options: &GcOptions) -> Result<GcReport> {
    let registries = selected_registries(config, options)?;
    let storage = Storage::from_config(&config.storage)?;
    let mut report = GcReport::default();
    for (org, ecosystem) in registries {
        let view = storage.for_hosted(&org, ecosystem);
        let collection = Collection {
            storage: &view,
            manifest_limit: config.http.oci.max_manifest_bytes,
            lease: config.gc.lease,
        };
        report.add(collection.run(options).await?);
    }
    Ok(report)
}

/// The `(org, ecosystem)` namespaces of the registries `options` selects,
/// each once.
fn selected_registries(config: &Config, options: &GcOptions) -> Result<Vec<(String, Ecosystem)>> {
    let names: Vec<&String> = if options.registries.is_empty() {
        config.routing.hosted.keys().collect()
    } else {
        options.registries.iter().collect()
    };
    let mut selected = Vec::new();
    for name in names {
        let hosted = config.routing.hosted
            .get(name)
            .ok_or_else(|| RegistryError::BadRequest {
                reason: format!("{name:?} is not a hosted registry"),
            })?;
        let ecosystem = config.routing.registries
            .ecosystem(name)
            .ok_or_else(|| RegistryError::BadRequest {
                reason: format!("{name:?} is not a concrete registry"),
            })?;
        let namespace = (hosted.org.clone(), ecosystem);
        if !selected.contains(&namespace) {
            selected.push(namespace);
        }
    }
    Ok(selected)
}

/// One `(org, ecosystem)` namespace being collected.
pub(crate) struct Collection<'storage> {
    /// The namespace's view of the hosted store.
    pub(crate) storage: &'storage Storage,
    pub(crate) manifest_limit: usize,
    /// How long an incarnation stays `reclaiming` before the collection
    /// finishes it as left behind by a reclaim that stopped.
    pub(crate) lease: Duration,
}

impl Collection<'_> {
    /// Collect the namespace as `options` say. A step that fails leaves the
    /// rest to run; the first failure is returned.
    pub(crate) async fn run(&self, options: &GcOptions) -> Result<GcReport> {
        if options.dry_run {
            self.storage.check_hosted_layout().await?;
            return self.plan(options.min_age).await;
        }
        self.storage.ensure_hosted_layout().await?;
        let mut tally = Tally::default();
        if self.storage.ecosystem() == Ecosystem::Oci {
            let unmapped = self.unmap_unreachable_images(options.min_age).await;
            tally.record(unmapped);
        }
        tally.record(self.reclaim_unmapped().await);
        tally.record(self.finish_stale().await);
        tally.record(self.remove_uncommitted(options.min_age).await);
        tally.finish()
    }

    /// Reclaim every live blob nothing maps.
    async fn reclaim_unmapped(&self) -> Result<GcReport> {
        let unmapped = self.storage.unmapped_blobs().await?;
        let ids = unmapped.keys().cloned().collect();
        let removed = self.storage.reclaim_blobs(ids).await?;
        Ok(report_of(&removed, |blob| unmapped.get(&blob.id).map_or(0, |row| row.size)))
    }

    /// Finish the reclaims that stopped more than the lease ago.
    async fn finish_stale(&self) -> Result<GcReport> {
        let finished = self.storage.finish_stale_reclaims(self.lease).await?;
        for blob in &finished {
            tracing::info!(key = %blob.key(), "finished an interrupted archive reclaim");
        }
        Ok(report_of(&finished, |_| 0))
    }

    /// Remove the stored incarnations no commit made live. One a publish on
    /// its way may still commit is fenced off first, and only once it was
    /// stored more than `min_age` ago.
    async fn remove_uncommitted(&self, min_age: Duration) -> Result<GcReport> {
        let stored = self.uncommitted().await?;
        let sizes: BTreeMap<BlobRef, u64> = stored
            .iter()
            .map(|(blob, file)| (blob.clone(), file.size))
            .collect();
        let candidates = stored
            .into_iter()
            .map(|(blob, file)| Uncommitted { may_fence: older_than(&file, min_age), blob })
            .collect();
        let removed = self.storage.remove_uncommitted(candidates).await?;
        for blob in &removed {
            tracing::info!(key = %blob.key(), "removed an archive no commit made live");
        }
        Ok(report_of(&removed, |blob| {
            sizes
                .get(blob)
                .copied()
                .unwrap_or_default()
        }))
    }

    /// The stored incarnations that are not the live one of their blob.
    async fn uncommitted(&self) -> Result<Vec<(BlobRef, StoredArchive)>> {
        let stored = self.storage.stored_incarnations().await?;
        let ids = stored
            .iter()
            .map(|(blob, _)| blob.id.clone())
            .collect();
        let rows = self.storage.blob_rows(ids).await?;
        Ok(stored
            .into_iter()
            .filter(|(blob, _)| !is_live(rows.get(&blob.id), blob.incarnation))
            .collect())
    }

    /// What a collection would remove, without changing anything: the live
    /// blobs nothing maps, the image blobs no retained manifest reaches, and
    /// the stored incarnations no commit made live.
    async fn plan(&self, min_age: Duration) -> Result<GcReport> {
        let mut unmapped = self.storage.unmapped_blobs().await?;
        if self.storage.ecosystem() == Ecosystem::Oci {
            for (id, row) in self.unreachable_images(min_age).await? {
                unmapped.insert(id, row);
            }
        }
        let mut report = GcReport::default();
        for (id, row) in &unmapped {
            tracing::info!(key = %id.key(row.incarnation), size = row.size, "unmapped archive");
            report.add(GcReport { archives: 1, bytes: row.size });
        }
        for (blob, file) in self.uncommitted().await? {
            if older_than(&file, min_age) {
                tracing::info!(key = %blob.key(), size = file.size, "uncommitted archive");
                report.add(GcReport { archives: 1, bytes: file.size });
            }
        }
        Ok(report)
    }
}

/// Whether `incarnation` is the live one of a blob with `row`.
fn is_live(row: Option<&BlobRow>, incarnation: u64) -> bool {
    row.is_some_and(|row| row.state == BlobState::Live && row.incarnation == incarnation)
}

/// Whether `file` was stored more than `min_age` ago.
fn older_than(file: &StoredArchive, min_age: Duration) -> bool {
    file.modified.elapsed().unwrap_or_default() >= min_age
}

fn report_of(removed: &[BlobRef], size_of: impl Fn(&BlobRef) -> u64) -> GcReport {
    GcReport { archives: removed.len(), bytes: removed.iter().map(size_of).sum() }
}

/// What a collection removed so far, and the first failure.
#[derive(Default)]
struct Tally {
    report: GcReport,
    failure: Option<RegistryError>,
}

impl Tally {
    fn record(&mut self, step: Result<GcReport>) {
        match step {
            Ok(report) => self.report.add(report),
            Err(error) => {
                tracing::warn!(%error, "archives not collected");
                self.failure.get_or_insert(error);
            }
        }
    }

    fn finish(self) -> Result<GcReport> {
        self.failure.map_or(Ok(self.report), Err)
    }
}

#[cfg(test)]
mod tests;
