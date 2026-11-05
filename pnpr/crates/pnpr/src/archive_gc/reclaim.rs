//! Removing archives no package references while replicas keep publishing,
//! fenced by the metadata database (see
//! [`pnpr_storage::metadata::ArchiveFence`]).

use super::liveness::Live;
use crate::Result;
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::{
    Storage,
    metadata::{ArchiveRef, ReclaimTarget, new_reclaim_owner},
};
use std::time::{Duration, Instant};

/// How long a reclaim may hold its marks before another reclaim takes them
/// over as left behind by one that stopped.
pub(crate) const RECLAIM_LEASE: Duration = Duration::from_hours(1);

/// How much of [`RECLAIM_LEASE`], measured on this process's own clock from
/// before it marked, a reclaim spends removing. It stops removing once that
/// is used up and leaves the rest marked for a later reclaim, so a removal
/// never starts while another reclaim could take the mark over. Only a
/// removal that stalls for more than the rest of the lease after it started
/// could still land after a takeover: the hosted stores offer no conditional
/// delete that would tie a removal to the mark.
const REMOVAL_BUDGET: Duration = Duration::from_mins(30);

/// Mark the archives of each target whose package is still as it was read,
/// remove them from the hosted store, and record them gone. Returns the
/// archives removed. An archive another reclaim holds is left to it. One
/// whose removal failed stays marked, so no commit depends on it until a
/// later reclaim takes it over, and the error is returned once the rest are
/// recorded.
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
    let owner = new_reclaim_owner();
    let started = Instant::now();
    let marked = storage.sql_metadata().mark_reclaims(catalog, &owner, targets).await?;
    remove_owned(storage, catalog, (&owner, started), marked).await
}

/// Take over the marks of `catalog` that `filter` keeps and that a reclaim
/// left behind longer than `lease` ago, and finish them.
pub(crate) async fn finish_stale(
    storage: &Storage,
    catalog: &str,
    lease: Duration,
    filter: impl Fn(&ArchiveRef) -> bool + Send + 'static,
) -> Result<Vec<ArchiveRef>> {
    let owner = new_reclaim_owner();
    let started = Instant::now();
    let database = storage.sql_metadata();
    let claimed = database.claim_stale_reclaims(catalog, &owner, lease, filter).await?;
    remove_owned(storage, catalog, (&owner, started), claimed).await
}

/// Remove the `archives` `owner` marked, those still its own, within the
/// [`REMOVAL_BUDGET`] from `started`, before it marked them, and record the
/// ones removed as gone.
async fn remove_owned(
    storage: &Storage,
    catalog: &str,
    (owner, started): (&str, Instant),
    archives: Vec<ArchiveRef>,
) -> Result<Vec<ArchiveRef>> {
    if archives.is_empty() {
        return Ok(archives);
    }
    let database = storage.sql_metadata();
    let archives = database.owned_reclaims(catalog, owner, archives).await?;
    let mut removed = Vec::with_capacity(archives.len());
    let mut failure = None;
    for archive in archives {
        if started.elapsed() >= REMOVAL_BUDGET {
            tracing::warn!(
                package = %archive.name,
                artifact = %archive.artifact,
                "an archive reclaim ran out of time; the rest stay marked for a later one",
            );
            break;
        }
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
    database.finish_reclaims(catalog, owner, removed.clone()).await?;
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
