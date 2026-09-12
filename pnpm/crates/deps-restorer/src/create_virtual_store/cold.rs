use super::{
    CreateVirtualStoreError, CreateVirtualStoreStoreContext, LinkPlan, RequiresBuildBySnapshot,
    WantedEntries,
    cache_keys::package_content_changed,
    cas_paths_key, requires_build_from_cas_paths,
    slot_linking::{COLD_LINK_CHUNK, LinkSlotsParallel, link_cold_chunk},
};
use crate::{CasPathsByPkgId, InstallPackageBySnapshot, InstallPackageBySnapshotError};
use futures_util::{StreamExt, stream::FuturesUnordered};
use pnpm_lockfile::{PackageKey, PackageMetadata, PkgName, SnapshotEntry};
use pnpm_reporter::Reporter;
use pnpm_tarball::PrefetchResult;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

pub(super) struct ColdInputs<'i, 'a> {
    pub(super) wanted: WantedEntries<'a>,
    pub(super) store: CreateVirtualStoreStoreContext<'i>,
    pub(super) prefetched: &'i PrefetchResult,
    pub(super) marker_source: Option<&'i tempfile::NamedTempFile>,
    pub(super) links: &'i LinkPlan<'a>,
}
/// A cold snapshot whose CAS paths are staged and whose slot link was
/// deferred to [`link_slots_parallel`](crate::create_virtual_store::slot_linking::link_slots_parallel).
pub(super) struct ColdCapture<'a> {
    pub(super) snapshot_key: &'a PackageKey,
    pub(super) snapshot: &'a SnapshotEntry,
    pub(super) cas_paths: HashMap<String, PathBuf>,
    pub(super) requires_build: bool,
    pub(super) source_is_mutable: bool,
    pub(super) force_import: bool,
}
pub(super) fn add_cold_cas_paths(map: &mut CasPathsByPkgId, cold_cas_paths: Vec<ColdCapture<'_>>) {
    map.reserve(cold_cas_paths.len());
    for ColdCapture { snapshot_key, cas_paths: paths, .. } in cold_cas_paths {
        map.entry(cas_paths_key(snapshot_key)).or_insert(paths);
    }
}
/// An optional snapshot whose fetch fails is dropped rather than aborting the
/// install.
///
/// Silent swallow. `tracing::warn!` gives operator visibility without
/// polluting the reporter wire: the frozen path emits nothing here; only the
/// resolver-side emit site fires `pnpm:skipped-optional-dependency
/// reason=resolution_failure`.
///
/// Scoped via [`is_fetch_side_failure`] to the tarball-fetch / git-fetch /
/// CAS-write variants — the fetch-side surface an optional snapshot is allowed
/// to swallow. Local materialization (`CreateVirtualDir`) and config-shape
/// errors (`MissingTarballIntegrity`, `UnsupportedResolution`) abort even for
/// optional snapshots — they sit outside the swallowed fetch surface.
pub(super) fn swallow_optional_fetch_failure<Captured>(
    snapshot_key: &PackageKey,
    snapshot: &SnapshotEntry,
    err: InstallPackageBySnapshotError,
) -> Result<(Option<PackageKey>, Option<Captured>), CreateVirtualStoreError> {
    if !snapshot.optional || !is_fetch_side_failure(&err) {
        return Err(CreateVirtualStoreError::InstallPackageBySnapshot(err));
    }
    tracing::warn!(
        target: "pacquet::install",
        snapshot = %snapshot_key,
        error = %err,
        "optional snapshot fetch/extract failed; dropping from install",
    );
    Ok((Some(snapshot_key.clone()), None))
}
/// The invariant inputs of one cold-batch drain.
/// The cold batch: snapshots whose tarball was not already in the store.
pub(super) struct ColdBatch<'a> {
    pub(super) cold: &'a [(&'a PackageKey, &'a SnapshotEntry)],
    pub(super) installer: InstallPackageBySnapshot<'a>,
    pub(super) packages: &'a HashMap<PackageKey, PackageMetadata>,
    pub(super) current_packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    /// Kept alive by the caller for the whole batch: every slot that
    /// needs a build marker hard-links this one file.
    pub(super) marker_source: Option<&'a tempfile::NamedTempFile>,
    pub(super) removed_aliases_by_key: &'a HashMap<PackageKey, Vec<PkgName>>,
    pub(super) link_template: &'a LinkSlotsParallel<'a>,
    pub(super) shared_packages: Option<&'a HashSet<&'a str>>,
    pub(super) is_hoisted: bool,
}
/// Download every cold snapshot and link each one as it lands.
///
/// The downloads run as one cooperative fan-out and the links happen in
/// chunks between completions — see [`drain_cold_downloads`] for why the
/// two are interleaved rather than run in sequence.
pub(super) async fn run_cold_batch<'a, Reporter: self::Reporter>(
    batch: ColdBatch<'a>,
    state: &mut ColdBatchState<'_>,
    cold_cas_paths: &mut Vec<ColdCapture<'a>>,
) -> Result<(), CreateVirtualStoreError> {
    if batch.cold.is_empty() {
        return Ok(());
    }

    let batch = &batch;
    let mut downloads: FuturesUnordered<_> = batch
        .cold
        .iter()
        .map(|&(snapshot_key, snapshot)| download_one::<Reporter>(batch, snapshot_key, snapshot))
        .collect();

    let cold_template = LinkSlotsParallel { batch: "cold", ..*batch.link_template };
    drain_cold_downloads::<Reporter, _>(
        &mut downloads,
        ColdDrain {
            packages: batch.packages,
            marker_path: batch.marker_source.map(tempfile::NamedTempFile::path),
            removed_aliases_by_key: batch.removed_aliases_by_key,
            template: &cold_template,
            shared_packages: batch.shared_packages,
            is_hoisted: batch.is_hoisted,
        },
        state,
        cold_cas_paths,
    )
    .await
}
/// One cold download. A failed optional snapshot lands in the first
/// slot instead of failing the batch; the second carries what the link
/// pass still has to place.
pub(super) async fn download_one<'a, Reporter: self::Reporter>(
    batch: &ColdBatch<'a>,
    snapshot_key: &'a PackageKey,
    snapshot: &'a SnapshotEntry,
) -> Result<(Option<PackageKey>, Option<ColdCapture<'a>>), CreateVirtualStoreError> {
    let metadata_key = snapshot_key.without_peer();
    let metadata = batch.packages.get(&metadata_key).ok_or_else(|| {
        CreateVirtualStoreError::MissingPackageMetadata {
            snapshot_key: snapshot_key.to_string(),
            metadata_key: metadata_key.to_string(),
        }
    })?;
    let installed = match batch.installer.run::<Reporter>(snapshot_key, metadata, snapshot).await {
        Ok(installed) => installed,
        Err(err) => return swallow_optional_fetch_failure(snapshot_key, snapshot, err),
    };
    let crate::InstalledPackage { cas_paths, source_is_mutable } = installed;
    Ok((
        None,
        Some(ColdCapture {
            snapshot_key,
            snapshot,
            requires_build: requires_build_from_cas_paths(&cas_paths),
            cas_paths,
            source_is_mutable,
            force_import: package_content_changed(
                batch.current_packages,
                batch.packages,
                snapshot_key,
            ),
        }),
    ))
}
pub(super) struct ColdDrain<'a> {
    packages: &'a HashMap<PackageKey, PackageMetadata>,
    marker_path: Option<&'a Path>,
    removed_aliases_by_key: &'a HashMap<PackageKey, Vec<PkgName>>,
    template: &'a LinkSlotsParallel<'a>,
    shared_packages: Option<&'a HashSet<&'a str>>,
    is_hoisted: bool,
}
/// Consume the cold downloads as they finish, linking each ready chunk.
///
/// The downloads deferred their slot links (`defer_link: true`) because a
/// blocking link inside this single cooperative task would serialize them;
/// linking chunks between completions keeps that work off the tail without
/// starving the pipe — a chunk's `block_in_place` pause is milliseconds,
/// absorbed by kernel socket buffers. GVS peer variants sharing one slot dir
/// may split across chunks: chunks run sequentially, and a later pass over a
/// complete slot short-circuits on its completion marker.
pub(super) async fn drain_cold_downloads<'a, Reporter: self::Reporter, Download>(
    downloads: &mut FuturesUnordered<Download>,
    drain: ColdDrain<'_>,
    state: &mut ColdBatchState<'_>,
    cold_cas_paths: &mut Vec<ColdCapture<'a>>,
) -> Result<(), CreateVirtualStoreError>
where
    Download: Future<
        Output = Result<(Option<PackageKey>, Option<ColdCapture<'a>>), CreateVirtualStoreError>,
    >,
{
    let mut ready: Vec<ColdCapture<'a>> = Vec::new();
    while let Some(outcome) = downloads.next().await {
        let Some(captured) = record_cold_outcome(outcome?, state, drain.shared_packages) else {
            continue;
        };
        if drain.is_hoisted {
            cold_cas_paths.push(captured);
            continue;
        }
        ready.push(captured);
        if ready.len() >= COLD_LINK_CHUNK {
            let chunk = std::mem::take(&mut ready);
            link_cold_chunk::<Reporter>(
                &chunk,
                drain.packages,
                drain.marker_path,
                drain.removed_aliases_by_key,
                drain.template,
            )?;
        }
    }
    link_cold_chunk::<Reporter>(
        &ready,
        drain.packages,
        drain.marker_path,
        drain.removed_aliases_by_key,
        drain.template,
    )
}
/// The per-snapshot state the cold drain accumulates into.
pub(super) struct ColdBatchState<'a> {
    pub(super) fetch_failed: &'a mut HashSet<PackageKey>,
    pub(super) requires_build_by_snapshot: &'a mut RequiresBuildBySnapshot,
    pub(super) shared_base_cas_paths: &'a mut crate::shared_side_effects::BaseCasPaths,
}
/// Fold one completed download into the batch state, handing back the capture
/// the link pass still has to place.
pub(super) fn record_cold_outcome<'a>(
    outcome: (Option<PackageKey>, Option<ColdCapture<'a>>),
    state: &mut ColdBatchState<'_>,
    shared_packages: Option<&HashSet<&str>>,
) -> Option<ColdCapture<'a>> {
    let (failure, captured) = outcome;
    if let Some(key) = failure {
        state.fetch_failed.insert(key);
    }
    let captured = captured?;
    state
        .requires_build_by_snapshot
        .insert((*captured.snapshot_key).clone(), captured.requires_build);
    if shared_packages
        .is_some_and(|packages| packages.contains(captured.snapshot_key.name.to_string().as_str()))
    {
        state
            .shared_base_cas_paths
            .insert((*captured.snapshot_key).clone(), captured.cas_paths.clone());
    }
    Some(captured)
}
/// True for the [`InstallPackageBySnapshotError`] variants pacquet
/// classifies as **fetch-side** — the failures that happen while
/// fetching a package into the CAS. These are the ones an optional
/// snapshot is allowed to swallow:
///
/// - `DownloadTarball` — HTTP fetch, integrity check, gzip decode,
///   CAS write.
/// - `GitFetch` — `git` CLI clone / checkout / preparePackage /
///   packlist / CAS import.
/// - `DirectoryFetch` — local-directory walk / manifest read /
///   packlist for injected workspace deps. Swallowed for optional
///   snapshots uniformly with the tarball / git paths.
///
/// Excluded (propagate even for optional snapshots — they happen
/// after the fetch, while linking the package into its slot):
///
/// - `CreateVirtualDir` — local materialization (clone / hardlink /
///   copy / symlink from CAS into the slot dir).
/// - `MissingTarballIntegrity`, `UnsupportedResolution` —
///   config/shape errors raised before any fetch runs.
pub(super) fn is_fetch_side_failure(err: &InstallPackageBySnapshotError) -> bool {
    matches!(
        err,
        InstallPackageBySnapshotError::DownloadTarball(_)
            | InstallPackageBySnapshotError::GitFetch(_)
            | InstallPackageBySnapshotError::DirectoryFetch(_)
            | InstallPackageBySnapshotError::CustomFetcher(_),
    )
}
