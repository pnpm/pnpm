//! Removing archives no package references while replicas keep publishing,
//! fenced by the metadata database (see
//! [`pnpr_storage::metadata::ArchiveFence`]).

use super::liveness::Live;
use crate::Result;
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::{
    Storage,
    metadata::{ArchiveRef, ReclaimTarget},
};

/// Mark the archives of each target whose package is still as it was read,
/// remove them from the hosted store, and record them gone. Returns the
/// archives removed. One whose removal failed stays marked, so no commit
/// depends on it until a later reclaim finishes it, and the error is
/// returned once the rest are recorded.
pub(crate) async fn reclaim(
    storage: &Storage,
    catalog: &str,
    targets: Vec<ReclaimTarget>,
) -> Result<Vec<ArchiveRef>> {
    let targets: Vec<ReclaimTarget> = targets
        .into_iter()
        .filter(|target| !target.artifacts.is_empty())
        .collect();
    if targets.is_empty() {
        return Ok(Vec::new());
    }
    let marked = storage.sql_metadata().mark_reclaims(catalog, targets).await?;
    finish(storage, catalog, marked).await
}

/// Remove the marked `archives` and record the ones removed as gone.
pub(crate) async fn finish(
    storage: &Storage,
    catalog: &str,
    archives: Vec<ArchiveRef>,
) -> Result<Vec<ArchiveRef>> {
    let mut removed = Vec::with_capacity(archives.len());
    let mut failure = None;
    for archive in archives {
        let name = CanonicalPackageName::from_stored(&archive.name, archive.ecosystem);
        let view = storage.for_hosted(catalog, archive.ecosystem);
        match view.remove_hosted_blob(&name, &archive.artifact).await {
            Ok(_) => removed.push(archive),
            Err(error) => {
                tracing::warn!(
                    package = %archive.name,
                    artifact = %archive.artifact,
                    %error,
                    "a reclaimed archive was not removed; it stays marked",
                );
                failure = Some(error);
            }
        }
    }
    storage.sql_metadata().finish_reclaims(catalog, removed.clone()).await?;
    failure.map_or(Ok(removed), Err)
}

/// Remove the archives among `artifacts` of `name` in `catalog` that the
/// package does not reference now, as a publish whose commit was refused
/// cleans up what it wrote. Another replica may have found one of them in
/// place and committed it; that one is referenced, or its commit landed
/// after the package was read here, and it stays.
pub(crate) async fn reclaim_unreferenced(
    storage: &Storage,
    catalog: &str,
    name: &CanonicalPackageName,
    artifacts: Vec<String>,
    manifest_limit: usize,
) -> Result<Vec<ArchiveRef>> {
    let view = storage.for_hosted(catalog, name.ecosystem());
    let base = view.read_hosted_document_for_publish(name).await?;
    let bytes = base.stored.as_ref().map(|stored| stored.bytes.as_slice());
    let live = Live::of(&view, name, bytes, manifest_limit).await?;
    let artifacts = artifacts
        .into_iter()
        .filter(|artifact| live.is_unreferenced(name, artifact))
        .collect();
    let target = ReclaimTarget {
        ecosystem: name.ecosystem(),
        name: name.as_str().to_string(),
        expected: base.package_generation(),
        artifacts,
    };
    reclaim(storage, catalog, vec![target]).await
}
