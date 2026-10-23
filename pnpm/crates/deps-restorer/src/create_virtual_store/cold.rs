use super::{
    ConcurrentCheck, CreateVirtualStoreError, CreateVirtualStoreStoreContext, LinkPlan,
    PackageBins, PackageManifests, RequiresBuildBySnapshot, WantedEntries,
    cache_keys::SlotReuse,
    cas_paths_key, requires_build_from_cas_paths, run_checked,
    slot_linking::{LinkSlotsParallel, link_cold_chunk},
};
use crate::{CasPathsByPkgId, InstallPackageBySnapshot, InstallPackageBySnapshotError};
use futures_util::{StreamExt, stream::FuturesUnordered};
use miette::Diagnostic;
use pnpm_lockfile::{PackageKey, PackageMetadata, PkgName, SnapshotEntry};
use pnpm_reporter::{
    LogEvent, LogLevel, Reporter, SkippedOptionalDependencyLog, SkippedOptionalPackage,
    SkippedOptionalReason,
};
use pnpm_store_dir::store_index_key;
use pnpm_tarball::{PrefetchResult, pending_progress_key};
use std::{
    collections::{HashMap, HashSet},
    ops::ControlFlow,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
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
    pub(super) installed: crate::InstalledPackage,
    pub(super) requires_build: bool,
    pub(super) force_import: bool,
}
pub(super) fn add_cold_cas_paths(map: &mut CasPathsByPkgId, cold_cas_paths: Vec<ColdCapture<'_>>) {
    map.reserve(cold_cas_paths.len());
    for ColdCapture { snapshot_key, installed, .. } in cold_cas_paths {
        map.entry(cas_paths_key(snapshot_key))
            .or_insert_with(|| crate::HoistedPackageFiles {
                cas_paths: Arc::new(installed.cas_paths),
                source_is_mutable: installed.source_is_mutable,
                source_exists: installed.source_exists,
            });
    }
}
/// An optional snapshot whose fetch fails is dropped rather than aborting the
/// install, and reported as a `pnpm:skipped-optional-dependency` with
/// `reason=fetch_failure` so the user learns the package is missing.
///
/// Scoped via [`is_fetch_side_failure`] to the tarball-fetch / git-fetch /
/// CAS-write variants — the fetch-side surface an optional snapshot is allowed
/// to swallow. Local materialization (`CreateVirtualDir`) and config-shape
/// errors (`MissingTarballIntegrity`, `UnsupportedResolution`) abort even for
/// optional snapshots — they sit outside the swallowed fetch surface.
pub(super) fn swallow_optional_fetch_failure<Reporter: self::Reporter, Captured>(
    snapshot_key: &PackageKey,
    snapshot: &SnapshotEntry,
    err: InstallPackageBySnapshotError,
    prefix: &str,
) -> Result<(Option<PackageKey>, Option<Captured>), CreateVirtualStoreError> {
    if !snapshot.optional || !is_fetch_side_failure(&err) {
        return Err(CreateVirtualStoreError::InstallPackageBySnapshot(err));
    }
    let details = match err.code() {
        Some(code) => format!("{code}: {err}"),
        None => err.to_string(),
    };
    Reporter::emit(&LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
        level: LogLevel::Debug,
        details: Some(details),
        package: SkippedOptionalPackage::Installed {
            id: snapshot_key.without_peer().to_string(),
            name: snapshot_key.name.to_string(),
            version: snapshot_key.suffix.version().to_string(),
        },
        parents: None,
        prefix: prefix.to_string(),
        reason: SkippedOptionalReason::FetchFailure,
    }));
    Ok((Some(snapshot_key.clone()), None))
}
/// See [`crate::unlink_fetch_failed_children`]. The hoisted linker has no
/// slots, and a `frozenStore` store is read-only.
pub(super) fn unlink_fetch_failed_children(
    ctx: &crate::InstallContext<'_>,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    fetch_failed: &HashSet<PackageKey>,
) -> Result<(), CreateVirtualStoreError> {
    if fetch_failed.is_empty() || ctx.is_hoisted() || ctx.config.frozen_store {
        return Ok(());
    }
    crate::unlink_fetch_failed_children(snapshots, fetch_failed, ctx.linker.layout)
        .map_err(|error| {
            CreateVirtualStoreError::InstallPackageBySnapshot(
                InstallPackageBySnapshotError::CreateVirtualDir(error),
            )
        })
}
/// The cold batch: snapshots whose tarball was not already in the store.
pub(super) struct ColdBatch<'a> {
    pub(super) cold: &'a [(&'a PackageKey, &'a SnapshotEntry)],
    pub(super) installer: InstallPackageBySnapshot<'a>,
    pub(super) reuse: SlotReuse<'a>,
    /// Kept alive by the caller for the whole batch: every slot that
    /// needs a build marker hard-links this one file.
    pub(super) marker_source: Option<&'a tempfile::NamedTempFile>,
    pub(super) removed_aliases_by_key: &'a HashMap<PackageKey, Vec<PkgName>>,
    pub(super) link_template: &'a LinkSlotsParallel<'a>,
    pub(super) shared_packages: Option<&'a HashSet<&'a str>>,
}
/// Download every cold snapshot and link each one as it lands, polling
/// `check` alongside the downloads.
///
/// The downloads run as one cooperative fan-out on this task. Their slot
/// links (`defer_link: true`) run on a thread of their own, because a
/// blocking link pass inside this task would leave every in-flight
/// download unpolled for as long as the pass takes — seconds on a slow
/// volume. See [`link_while_downloading`].
pub(super) async fn run_cold_batch<'a, Reporter: self::Reporter>(
    batch: ColdBatch<'a>,
    state: &mut ColdBatchState<'_>,
    cold_cas_paths: &mut Vec<ColdCapture<'a>>,
    check: &mut Option<ConcurrentCheck<'_>>,
) -> Result<(), CreateVirtualStoreError> {
    if batch.cold.is_empty() {
        return Ok(());
    }

    let batch = &batch;
    let mut downloads: FuturesUnordered<_> = batch.cold
        .iter()
        .map(|&(snapshot_key, snapshot)| download_one::<Reporter>(batch, snapshot_key, snapshot))
        .collect();
    let shared_packages = batch.shared_packages;

    if batch.installer.ctx.is_hoisted() {
        let drain = drain_cold_downloads(&mut downloads, state, shared_packages, |captured| {
            cold_cas_paths.push(captured);
            ControlFlow::Continue(())
        });
        return run_checked(check, drain).await;
    }
    let cold_template = LinkSlotsParallel { batch: "cold", ..*batch.link_template };
    let linker = ColdLinker {
        packages: batch.reuse.packages,
        marker_path: batch.marker_source.map(tempfile::NamedTempFile::path),
        removed_aliases_by_key: batch.removed_aliases_by_key,
        template: &cold_template,
    };
    if crate::on_multi_thread_runtime() {
        return link_while_downloading::<Reporter, _>(
            &mut downloads,
            (state, shared_packages),
            &linker,
            check,
        );
    }
    // The `current_thread` runtime cannot hand this task's downloads to
    // another thread, so they finish before the one link pass.
    let mut ready = Vec::new();
    let drain = drain_cold_downloads(&mut downloads, state, shared_packages, |captured| {
        ready.push(captured);
        ControlFlow::Continue(())
    });
    run_checked(check, drain).await?;
    linker.link_pass::<Reporter>(&ready)
}
/// Drive the downloads and `check` on the calling thread while a scoped
/// thread links each batch of completed downloads.
///
/// The link thread's inputs borrow from the batch, so it is scoped, and
/// the scope blocks this task: `block_in_place` moves the worker's other
/// tasks elsewhere, and `block_on` keeps the downloads running. Nothing
/// else in this task is polled until the drain ends, which is why the
/// pipeline spawns the work it wants running meanwhile. A download that
/// fails, or an aborting `check`, stops the drain and raises `stop`, so
/// the link thread finishes its current pass and leaves the queued
/// captures unlinked. A pass that fails drops the receiver, which stops
/// the drain.
fn link_while_downloading<'a, Reporter: self::Reporter, Download>(
    downloads: &mut FuturesUnordered<Download>,
    (state, shared_packages): (&mut ColdBatchState<'_>, Option<&HashSet<&str>>),
    linker: &ColdLinker<'_>,
    check: &mut Option<ConcurrentCheck<'_>>,
) -> Result<(), CreateVirtualStoreError>
where
    Download: Future<
        Output = Result<(Option<PackageKey>, Option<ColdCapture<'a>>), CreateVirtualStoreError>,
    >,
{
    let runtime = tokio::runtime::Handle::current();
    let stop = &AtomicBool::new(false);
    tokio::task::block_in_place(|| {
        std::thread::scope(|scope| {
            let (sender, receiver) = std::sync::mpsc::channel::<ColdCapture<'a>>();
            let link_thread = std::thread::Builder::new()
                .name("pnpm-cold-link".to_string())
                .spawn_scoped(scope, move || linker.link_as_delivered::<Reporter>(&receiver, stop))
                .expect("spawn the cold link thread");
            let drain = drain_cold_downloads(downloads, state, shared_packages, |captured| {
                linker.deliver(&sender, captured)
            });
            let drained = runtime.block_on(run_checked(check, drain));
            if drained.is_err() {
                stop.store(true, Ordering::Release);
            }
            drop(sender);
            let linked =
                link_thread.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic));
            drained.and(linked)
        })
    })
}
/// The pass-invariant inputs of the cold link passes.
struct ColdLinker<'a> {
    packages: &'a HashMap<PackageKey, PackageMetadata>,
    marker_path: Option<&'a Path>,
    removed_aliases_by_key: &'a HashMap<PackageKey, Vec<PkgName>>,
    template: &'a LinkSlotsParallel<'a>,
}
impl ColdLinker<'_> {
    /// Hand `captured` to the link thread. A closed receiver means a pass
    /// failed, and the drain stops so that error surfaces.
    #[cfg_attr(
        not(test),
        expect(clippy::unused_self, reason = "`self` only carries the test probe")
    )]
    fn deliver<'a>(
        &self,
        sender: &std::sync::mpsc::Sender<ColdCapture<'a>>,
        captured: ColdCapture<'a>,
    ) -> ControlFlow<()> {
        if sender.send(captured).is_err() {
            return ControlFlow::Break(());
        }
        #[cfg(test)]
        if let Some(probe) = self.template.link_concurrency_probe {
            probe.cold_capture_delivered();
        }
        ControlFlow::Continue(())
    }

    /// Link the captures as `receiver` delivers them, one pass per batch,
    /// until the channel closes or `stop` is raised.
    ///
    /// A pass takes every capture that completed while the previous pass
    /// ran, so a slow volume links larger batches and a fast one links
    /// downloads as they land. Passes run in sequence: GVS peer variants
    /// sharing one slot dir may split across passes, and a later pass
    /// over a complete slot short-circuits on its completion marker.
    fn link_as_delivered<Reporter: self::Reporter>(
        &self,
        receiver: &std::sync::mpsc::Receiver<ColdCapture<'_>>,
        stop: &AtomicBool,
    ) -> Result<(), CreateVirtualStoreError> {
        while let Ok(first) = receiver.recv() {
            if stop.load(Ordering::Acquire) {
                return Ok(());
            }
            #[cfg(test)]
            if let Some(probe) = self.template.link_concurrency_probe {
                probe.before_cold_link_pass();
            }
            let batch: Vec<_> = std::iter::once(first).chain(receiver.try_iter()).collect();
            self.link_pass::<Reporter>(&batch)?;
        }
        Ok(())
    }

    fn link_pass<Reporter: self::Reporter>(
        &self,
        batch: &[ColdCapture<'_>],
    ) -> Result<(), CreateVirtualStoreError> {
        link_cold_chunk::<Reporter>(
            batch,
            self.packages,
            self.marker_path,
            self.removed_aliases_by_key,
            self.template,
        )
    }
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
    let metadata = batch.reuse.packages
        .get(&metadata_key)
        .ok_or_else(|| CreateVirtualStoreError::MissingPackageMetadata {
            snapshot_key: snapshot_key.to_string(),
            metadata_key: metadata_key.to_string(),
        })?;
    let installed = match batch.installer.run::<Reporter>(snapshot_key, metadata, snapshot).await {
        Ok(installed) => installed,
        Err(err) => {
            return drop_failed_download::<Reporter, _>(
                batch,
                snapshot_key,
                snapshot,
                metadata,
                err,
            );
        }
    };
    let requires_build = (!crate::snapshot_has_patch(snapshot_key))
        .then_some(installed.requires_build)
        .flatten()
        .unwrap_or_else(|| requires_build_from_cas_paths(&installed.cas_paths));
    Ok((
        None,
        Some(ColdCapture {
            snapshot_key,
            snapshot,
            installed,
            requires_build,
            force_import: batch.reuse.must_replace(snapshot_key),
        }),
    ))
}
/// A failed download that [`swallow_optional_fetch_failure`] keeps from
/// failing the batch still settles the progress row its fetch announced.
fn drop_failed_download<Reporter: self::Reporter, Captured>(
    batch: &ColdBatch<'_>,
    snapshot_key: &PackageKey,
    snapshot: &SnapshotEntry,
    metadata: &PackageMetadata,
    err: InstallPackageBySnapshotError,
) -> Result<(Option<PackageKey>, Option<Captured>), CreateVirtualStoreError> {
    let failure = swallow_optional_fetch_failure::<Reporter, _>(
        snapshot_key,
        snapshot,
        err,
        batch.installer.ctx.requester,
    );
    if failure.is_ok()
        && let Some(integrity) = metadata.resolution.integrity()
    {
        let key = store_index_key(&integrity.to_string(), &snapshot_key.without_peer().pkg_id());
        if batch.link_template.progress_reported.contains(&pending_progress_key(&key)) {
            batch.link_template.progress_reported.insert(key);
        }
    }
    failure
}
/// Consume the cold downloads as they finish, handing each capture to
/// `deliver` until it asks to stop.
pub(super) async fn drain_cold_downloads<'a, Download>(
    downloads: &mut FuturesUnordered<Download>,
    state: &mut ColdBatchState<'_>,
    shared_packages: Option<&HashSet<&str>>,
    mut deliver: impl FnMut(ColdCapture<'a>) -> ControlFlow<()>,
) -> Result<(), CreateVirtualStoreError>
where
    Download: Future<
        Output = Result<(Option<PackageKey>, Option<ColdCapture<'a>>), CreateVirtualStoreError>,
    >,
{
    while let Some(outcome) = downloads.next().await {
        let Some(captured) = record_cold_outcome(outcome?, state, shared_packages) else {
            continue;
        };
        if deliver(captured).is_break() {
            break;
        }
    }
    Ok(())
}
/// The per-snapshot state the cold drain accumulates into.
pub(super) struct ColdBatchState<'a> {
    pub(super) fetch_failed: &'a mut HashSet<PackageKey>,
    pub(super) package_manifests: &'a mut PackageManifests,
    pub(super) package_bins: &'a mut PackageBins,
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
        state.requires_build_by_snapshot.remove(&key);
        state.package_bins.remove(&key.without_peer());
        state.fetch_failed.insert(key);
    }
    let mut captured = captured?;
    state.requires_build_by_snapshot.insert(
        (*captured.snapshot_key).clone(),
        captured.requires_build,
    );
    if !crate::snapshot_has_patch(captured.snapshot_key) {
        let metadata_key = captured.snapshot_key.without_peer();
        if let Some(manifest) = captured.installed.manifest.take() {
            state.package_manifests.insert(metadata_key.clone(), manifest);
        }
        if !captured.requires_build
            && let Some(bins) = captured.installed.bins.take()
        {
            state.package_bins.insert(metadata_key, bins);
        } else {
            state.package_bins.remove(&metadata_key);
        }
    }
    if shared_packages.is_some_and(|packages| {
        packages.contains(captured.snapshot_key.name.to_string().as_str())
    }) {
        state.shared_base_cas_paths.insert(
            (*captured.snapshot_key).clone(),
            captured.installed.cas_paths.clone(),
        );
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
