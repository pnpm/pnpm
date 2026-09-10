mod pipeline;

mod warm;
use warm::{
    gvs_slot_needs_rebuild, requires_build_from_cas_paths, snapshot_needs_build_marker,
    warm_cas_paths_by_pkg_id, warm_shared_base_cas_paths,
};

mod cold;
use cold::{ColdCapture, add_cold_cas_paths};

mod slot_linking;
use slot_linking::LinkSlotsParallel;

mod cache_keys;
use cache_keys::{
    SnapshotCacheKey, derive_cache_keys, integrity_equal, prefetch_keys, snapshot_deps_equal,
};

use crate::{
    CasPathsByPkgId, CustomFetcherSession, InstallPackageBySnapshotError, SkippedSnapshots,
    store_init::init_store_dir_best_effort,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_deps_path::get_pkg_id_with_patch_hash;
use pnpm_lockfile::{
    LockfileEntries, LockfileResolution, PackageKey, PackageMetadata, PkgIdWithPatchHash, PkgName,
    PkgNameVerPeer, SnapshotEntry,
};
use pnpm_network::ThrottledClient;
use pnpm_store_dir::{
    SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreIndex, StoreIndexWriter,
};
use pnpm_tarball::{
    MemCache, PrefetchIntegrityCheck, PrefetchResult, SharedReportedProgressKeys,
    prefetch_cas_paths,
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

/// Bundled package manifests recovered from the `SQLite` store index
/// during [`CreateVirtualStore::run`], keyed by the same
/// `PkgNameVerPeer` (without peer suffix) that
/// [`pnpm_lockfile::Lockfile::packages`] uses. Consumed by the
/// bin-linker so it doesn't have to re-read `package.json` per child
/// during [`crate::LinkVirtualStoreBins::run`].
///
/// Only covers the warm-batch packages (those whose tarball was
/// already in the CAFS at install start). Cold-batch packages — ones
/// pacquet had to download — are absent and the bin linker falls
/// back to disk reads for them. That matches pnpm's behaviour for
/// installs that mix warm and cold packages: pnpm's bin linker
/// reads from `pkgFilesIndex.manifest` for warm fetches and from
/// `dep.fetching()?.bundledManifest` for cold ones, but the cold
/// path's `bundledManifest` isn't plumbed through pacquet yet.
pub type PackageManifests = HashMap<PkgNameVerPeer, std::sync::Arc<serde_json::Value>>;

/// Per-snapshot side-effects-cache overlays, keyed by the snapshot's
/// `PackageKey` and then by the dep-state cache key (the string
/// `pnpm_graph_hasher::calc_dep_state` produces). The inner map
/// is the post-build files map for that cache key — already with
/// the `added` / `deleted` overlay applied against the base files
/// (see `pnpm_store_dir::VerifyResult.side_effects_maps`).
///
/// Multiple snapshot peer-variants of the same package share one
/// `Arc<_>` value — the store-index row is keyed peer-stripped, so
/// each `PackageKey::without_peer()` lookup returns the same
/// underlying map.
///
/// Hands off to `BuildModules`'s `is_built` gate (pnpm/pacquet#421):
/// for a snapshot whose `calc_dep_state` cache key matches an entry
/// here, the build is skipped — pacquet treats the package as
/// already built (typically because pnpm seeded the cache on a
/// previous install).
pub type SideEffectsMapsBySnapshot =
    HashMap<PackageKey, std::sync::Arc<HashMap<String, HashMap<String, PathBuf>>>>;

pub type SideEffectsBySnapshot =
    HashMap<PackageKey, std::sync::Arc<HashMap<String, pnpm_store_dir::SideEffectsDiff>>>;

pub type RemoteSideEffectsQuarantineBySnapshot =
    HashMap<PackageKey, std::sync::Arc<HashMap<String, Vec<String>>>>;

pub type StoreIndexKeysBySnapshot = HashMap<PackageKey, String>;

/// Per-snapshot `requiresBuild` flags recovered from the store index
/// during the warm-cache prefetch. `BuildModules` consumes this to
/// avoid re-inspecting every package directory after materialization.
pub type RequiresBuildBySnapshot = HashMap<PackageKey, bool>;

/// Store handles that a fresh resolution and dependency materialization share.
pub struct CreateVirtualStoreStoreContext<'a> {
    pub index: Option<&'a SharedReadonlyStoreIndex>,
    pub verified_files_cache: &'a SharedVerifiedFilesCache,
}

/// The store-side half of [`CreateVirtualStore::run`]'s planning,
/// started ahead of the rest: the store-index open, the per-snapshot
/// cache-key derivation, and the warm-cache prefetch task those keys
/// feed.
///
/// [`CreateVirtualStore::run`] starts one itself when the caller passes
/// none. A caller that has independent async work to do between
/// planning and materialization — the frozen install path's
/// installability host detection, whose `node --version` costs more
/// than the entire prefetch — starts it early via [`Self::start`] and
/// hands it over through [`CreateVirtualStore::cas_prefetch`], so the
/// prefetch's store reads run under that work instead of after it.
pub struct CasPrefetch {
    store_index: Option<SharedReadonlyStoreIndex>,
    verified_files_cache: SharedVerifiedFilesCache,
    /// One entry per lockfile snapshot. Values keep the derivation
    /// `Result` so [`snapshot_plan::plan_snapshots`] can preserve the
    /// strict/lenient asymmetry documented there: a survivor propagates
    /// its error, a skipped snapshot swallows it.
    cache_keys: HashMap<PackageKey, Result<SnapshotCacheKey, CreateVirtualStoreError>>,
    task: tokio::task::JoinHandle<PrefetchResult>,
}

impl CasPrefetch {
    /// Derive every snapshot's store-index cache key and spawn the
    /// warm-cache prefetch over the keys that exist. Read-only against
    /// the store apart from the `SQLite` open, so it is safe to run
    /// concurrently with anything that does not write store rows —
    /// but, like the rest of the store planning, only after the
    /// offline lockfile checks.
    ///
    /// `entries` must be the same value later given to
    /// [`CreateVirtualStore::run`]: the plan pass consumes one derived
    /// key per snapshot.
    pub async fn start(
        config: &'static Config,
        entries: LockfileEntries<'_>,
        supported_architectures: Option<&pnpm_package_is_installable::SupportedArchitectures>,
        store_context: Option<&CreateVirtualStoreStoreContext<'_>>,
    ) -> Self {
        let store_dir: &'static _ = &config.store_dir;
        // Open the read-only SQLite index once for the whole run instead
        // of per snapshot. Every `InstallPackageBySnapshot` performs a
        // cache lookup against this index before falling through to the
        // network; on a 1352-package lockfile the per-snapshot reopen
        // accounted for ~1.3 s of wall time even with a fully populated
        // store (see <https://github.com/pnpm/pacquet/issues/260>). A
        // `None` here means the store has no `index.db` yet (first
        // install against an empty store), in which case every lookup
        // would miss — so the handle stays `Option`al and lookups
        // short-circuit. The open itself is synchronous SQLite I/O
        // parked on the blocking pool; `open_shared` degrades a
        // blocking-task failure to `None` so the install still makes
        // progress with cache misses.
        let store_index = match store_context.and_then(|context| context.index) {
            Some(index) => Some(Arc::clone(index)),
            None => StoreIndex::open_shared(store_dir, config.frozen_store).await,
        };
        // Install-scoped `verifiedFilesCache`: one `Arc<DashSet>` for
        // the duration of the install, so a CAFS path verified for one
        // snapshot is not re-stat'd for another.
        let verified_files_cache = store_context
            .map_or_else(SharedVerifiedFilesCache::default, |context| {
                Arc::clone(context.verified_files_cache)
            });
        let cache_keys = derive_cache_keys(config, entries, supported_architectures);
        // The files check waits for the plan: only the snapshots this
        // run materializes have their CAFS files stat'd, in
        // `CreateVirtualStore::settle_prefetch`. Under a global virtual
        // store or an unchanged lockfile that is few or none of the rows
        // read here.
        let task = tokio::spawn(prefetch_cas_paths(
            store_index.clone(),
            store_dir,
            prefetch_keys(&cache_keys),
            PrefetchIntegrityCheck::deferred_if(config.verify_store_integrity),
            SharedVerifiedFilesCache::clone(&verified_files_cache),
        ));
        CasPrefetch { store_index, verified_files_cache, cache_keys, task }
    }
}

/// A snapshot paired with the store-index cache key it is looked up
/// by. `None` for a resolution that never goes through the CAFS
/// (directory and git), which therefore has no row to prefetch.
pub(crate) type SnapshotWithCacheKey<'a> = (&'a PackageKey, &'a SnapshotEntry, Option<String>);

/// Output of [`CreateVirtualStore::run`]. Bundles the bin-link
/// manifest cache, the per-snapshot side-effects-cache overlays the
/// build-phase needs, and the per-install fetch-failure set.
///
/// `fetch_failed` is the set of `optional: true` snapshots whose
/// tarball / metadata / extract step blew up during this install.
/// The caller (`InstallFrozenLockfile::run`) folds these into its
/// own [`crate::SkippedSnapshots`] so downstream consumers
/// (`build_graph`, `link_bins`, hoisting, etc.) treat them as
/// absent — a failed-fetch optional snapshot is simply not present
/// in the install graph.
pub struct CreateVirtualStoreOutput {
    pub package_manifests: PackageManifests,
    pub side_effects_maps_by_snapshot: SideEffectsMapsBySnapshot,
    pub requires_build_by_snapshot: RequiresBuildBySnapshot,
    /// Snapshot keys whose package directories this run materialized.
    /// The build phase uses this list to record only newly deferred
    /// builds under `ignoreScripts`; earlier debt is retained from
    /// `.modules.yaml` by the install orchestrator.
    pub materialized_snapshots: Vec<PackageKey>,
    pub fetch_failed: HashSet<PackageKey>,
    /// Per-package CAS index, populated only when
    /// [`crate::InstallContext::node_linker`] is
    /// [`NodeLinker::Hoisted`](pnpm_modules_yaml::NodeLinker::Hoisted). Threaded into
    /// [`crate::link_hoisted_modules()`] which materializes the
    /// hoisted `node_modules/` tree directly from these CAS paths
    /// — there is no virtual store under hoisted, so this is the
    /// only output that survives into the link phase. `None` for
    /// the isolated and pnp linkers (their slot directories are
    /// the bridge into the link phase instead). Pacquet decouples
    /// fetch and walk, so the index is built here at fetch time.
    pub cas_paths_by_pkg_id: Option<CasPathsByPkgId>,
}

/// This subroutine generates filesystem layout for the virtual store at `node_modules/.pacquet`.
#[must_use]
pub struct CreateVirtualStore<'a> {
    pub ctx: &'a crate::InstallContext<'a>,
    pub http_client: &'a ThrottledClient,
    /// The wanted lockfile's entries — what this run materializes.
    pub entries: LockfileEntries<'a>,
    /// Entries recorded by the previous install, parsed from
    /// `<virtual_store_dir>/lock.yaml`. Empty on a first install (the
    /// file doesn't exist) and under `--force`. When present,
    /// per-snapshot lookups against these drive the warm-reinstall skip
    /// decision — see [`CreateVirtualStore::run`] and
    /// [`LockfileEntries::of_previous_install`].
    pub current_entries: LockfileEntries<'a>,
    /// Shared store-index writer for the install. Owned by
    /// `InstallFrozenLockfile`, threaded down here for the cold-batch
    /// download path's `InstallPackageBySnapshot` and also reused by
    /// `BuildModules` for the side-effects-cache WRITE path.
    pub store_index_writer: &'a std::sync::Arc<StoreIndexWriter>,
    pub store_context: Option<CreateVirtualStoreStoreContext<'a>>,
    /// A [`CasPrefetch`] the caller started early so its store reads
    /// overlap caller-side async work; `None` makes [`Self::run`] start
    /// one itself. Must have been started with this run's `snapshots` /
    /// `packages`.
    pub cas_prefetch: Option<CasPrefetch>,
    /// Snapshots the installability pass marked optional+incompatible
    /// on this host. Their virtual-store slots are not created — the
    /// warm/cold partition skips them, and the bundled-manifest +
    /// side-effects-cache lookups they would feed downstream phases
    /// are likewise omitted: only non-skipped snapshots are
    /// materialized into the graph passed to the build phase.
    pub skipped: &'a SkippedSnapshots,
    /// Whether snapshot `optionalDependencies` are included in this
    /// materialization.
    pub include_optional_dependencies: bool,
    pub supported_architectures: Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    /// macOS directory-clone materialization cache
    /// ([`crate::dir_clone_cache`]), built by the install entry points
    /// when [`crate::DirCloneCache::eligible`] holds. Threaded into
    /// every slot link that passes `dir_clone_cacheable`.
    pub dir_clone_cache: Option<&'a crate::DirCloneCache<'a>>,
    /// Cache keys whose package status (`fetched` or `found_in_store`)
    /// has already been emitted earlier in this install. The warm batch
    /// still emits `resolved` for those packages, but skips the second
    /// status event so resolve-time prefetch progress is visible without
    /// being double-counted.
    pub progress_reported: &'a SharedReportedProgressKeys,
    /// Install-scoped shared in-flight tarball cache, threaded into each
    /// per-snapshot [`InstallPackageBySnapshot`](crate::InstallPackageBySnapshot) so the cold-batch
    /// download reuses a background prefetcher's in-flight download
    /// instead of re-fetching. `Some` whenever a prefetcher is active —
    /// the pnpr client's `TarballPrefetcher` (frozen path) or
    /// the fresh-resolve path's `PrefetchingResolver` (closing
    /// <https://github.com/pnpm/pnpm/issues/12241>); `None` otherwise.
    pub tarball_mem_cache: Option<&'a std::sync::Arc<MemCache>>,
    /// Custom fetchers from the pnpmfile. Consulted per snapshot
    /// before the built-in resolution-type dispatch.
    pub custom_fetcher_session: Option<&'a Arc<CustomFetcherSession>>,
    /// Fetch-evidence cell filled right after the warm/cold partition
    /// with the cold registry-resolved snapshots this run downloads —
    /// see [`pnpm_resolving_resolver_base::PlannedCanonicalFetches`].
    /// `None` for callers with no concurrent verification fan-out to
    /// feed (the fresh-resolve path, `--filter` passes, tests).
    pub planned_canonical_fetches:
        Option<&'a pnpm_resolving_resolver_base::PlannedCanonicalFetches>,
    #[cfg(test)]
    pub link_concurrency_probe:
        Option<&'a crate::create_virtual_dir_by_snapshot::tests::LinkConcurrencyProbe>,
}

/// Error type of [`CreateVirtualStore`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum CreateVirtualStoreError {
    #[diagnostic(transparent)]
    InstallPackageBySnapshot(#[error(source)] InstallPackageBySnapshotError),

    #[display(
        "Lockfile has a snapshot entry `{snapshot_key}` with no matching metadata entry (`{metadata_key}`) in `packages:`."
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_MISSING_PACKAGE_METADATA))]
    MissingPackageMetadata { snapshot_key: String, metadata_key: String },

    #[display(
        "Lockfile has a `snapshots:` section but no `packages:` section; every entry in `snapshots:` must have a matching metadata entry. The lockfile is malformed."
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_MISSING_PACKAGES_SECTION))]
    MissingPackagesSection,

    #[display("Failed to create the global virtual store build marker source at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_CREATE_BUILD_MARKER))]
    CreateBuildMarker {
        path: PathBuf,
        #[error(source)]
        error: std::io::Error,
    },

    #[display("Failed to inspect optional dependency at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_INSPECT_OPTIONAL_DEPENDENCY))]
    InspectOptionalDependency {
        path: PathBuf,
        #[error(source)]
        error: std::io::Error,
    },

    #[display("Failed to inspect the virtual store slot at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_INSPECT_VIRTUAL_STORE_SLOT))]
    InspectVirtualStoreSlot {
        path: PathBuf,
        #[error(source)]
        error: std::io::Error,
    },
}

/// The wanted lockfile's two sections, once both are known to exist.
#[derive(Clone, Copy)]
struct WantedEntries<'a> {
    packages: &'a HashMap<PackageKey, PackageMetadata>,
    snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
}

/// What the warm and cold link batches share beyond their slots.
struct LinkPlan<'a> {
    /// Per-slot obsolete child aliases. Only survivors that already
    /// existed in the current lockfile and dropped a child contribute an
    /// entry; fresh packages and addition-only changes map to the empty
    /// slice.
    removed_aliases_by_key: HashMap<PackageKey, Vec<PkgName>>,
    template: LinkSlotsParallel<'a>,
    /// The packages the remote side-effects cache shares, when it is
    /// configured.
    shared_packages: Option<HashSet<&'a str>>,
}

/// The CAS indexes the two batches fill in.
struct CasIndexes {
    /// Base paths of the shared side-effects packages, for the apply
    /// pass.
    shared_base: crate::shared_side_effects::BaseCasPaths,
    /// Built only for the hoisted linker, which materializes
    /// `node_modules/` straight from these paths — see
    /// [`CreateVirtualStoreOutput::cas_paths_by_pkg_id`]. Keyed by
    /// [`PkgIdWithPatchHash`], the key the walker assigns each
    /// [`crate::DependenciesGraphNode`]; until pacquet has end-to-end
    /// patch support that equals the snapshot key including any peer
    /// suffix. Peer variants of one package share a single `Arc`ed map
    /// in the warm batch, and each gets a cheap clone here because the
    /// linker takes an owned map per package.
    by_pkg_id: Option<CasPathsByPkgId>,
}

impl CasIndexes {
    fn warm(
        shared_packages: Option<&HashSet<&str>>,
        warm: &[partition::WarmEntry<'_>],
        is_hoisted: bool,
    ) -> Self {
        Self {
            shared_base: warm_shared_base_cas_paths(shared_packages, warm),
            by_pkg_id: is_hoisted.then(|| warm_cas_paths_by_pkg_id(warm)),
        }
    }

    fn add_cold(&mut self, cold: Vec<ColdCapture<'_>>) {
        if let Some(map) = self.by_pkg_id.as_mut() {
            add_cold_cas_paths(map, cold);
        }
    }
}

/// Look up the obsolete child aliases for a slot, defaulting to an
/// empty slice. The extra indirection lets the [`SlotLink`](crate::create_virtual_store::slot_linking::SlotLink) builders
/// pass their multiply-borrowed `snapshot_key` straight through —
/// deref coercion narrows it to `&PackageKey` at the call site.
fn removed_aliases_for<'a>(
    removed_aliases_by_key: &'a HashMap<PackageKey, Vec<PkgName>>,
    snapshot_key: &PackageKey,
) -> &'a [PkgName] {
    removed_aliases_by_key.get(snapshot_key).map_or(&[], Vec::as_slice)
}

/// Child aliases linked by the previous install (`current`) that are
/// absent from the wanted snapshot's `dependencies ∪
/// optional_dependencies`. The slot's own name is excluded so a
/// self-referential dependency never targets `node_modules/<self>`,
/// the directory the CAS import owns.
fn removed_child_aliases(
    current: &SnapshotEntry,
    wanted: &SnapshotEntry,
    self_name: &PkgName,
) -> Vec<PkgName> {
    fn child_aliases(snapshot: &SnapshotEntry) -> impl Iterator<Item = &PkgName> {
        let deps = snapshot.dependencies.iter().flatten();
        let opt_deps = snapshot.optional_dependencies.iter().flatten();
        deps.chain(opt_deps).map(|(alias, _)| alias)
    }
    let wanted_aliases: HashSet<&PkgName> = child_aliases(wanted).collect();
    let mut seen: HashSet<&PkgName> = HashSet::new();
    let mut removed = Vec::new();
    for alias in child_aliases(current) {
        if alias == self_name || wanted_aliases.contains(alias) {
            continue;
        }
        if seen.insert(alias) {
            removed.push(alias.clone());
        }
    }
    removed
}

/// Eagerly create `files/00..ff` under the v11 store root so per-tarball CAFS
/// writes never pay a `create_dir_all` syscall on the hot path. See
/// [`init_store_dir_best_effort`] for the error-degradation policy shared with
/// `install_without_lockfile.rs`. Skipped under `frozenStore`: the store is
/// read-only and complete, so no directory creation is attempted under its
/// root.
async fn init_store_dir_unless_frozen(
    config: &Config,
    store_dir: &'static pnpm_store_dir::StoreDir,
) {
    if config.frozen_store {
        return;
    }
    init_store_dir_best_effort(store_dir).await;
}

/// The file a slot's `.pnpm-needs-build` marker is imported from, when this
/// run can write one.
fn create_build_marker_source(
    config: &Config,
    layout: &crate::VirtualStoreLayout,
    store_dir: &pnpm_store_dir::StoreDir,
) -> Result<Option<tempfile::NamedTempFile>, CreateVirtualStoreError> {
    if config.frozen_store || !layout.enable_global_virtual_store() {
        return Ok(None);
    }
    tempfile::NamedTempFile::new_in(store_dir.root()).map(Some).map_err(|error| {
        CreateVirtualStoreError::CreateBuildMarker { path: store_dir.root().to_path_buf(), error }
    })
}

/// Publish the cold-batch fetch plan for the concurrent verification fan-out:
/// every cold registry-resolved snapshot with a pinned hash is downloaded from
/// its canonical registry URL by this run (or fails the install / is dropped
/// as an uninstallable optional), which is the existence evidence the npm
/// verifier's age gate may substitute for a metadata body. First fill wins;
/// entries outside the plan keep the metadata-backed path.
fn publish_planned_canonical_fetches(
    planned_canonical_fetches: Option<&pnpm_resolving_resolver_base::PlannedCanonicalFetches>,
    cold: &[(&PackageKey, &SnapshotEntry)],
    packages: &HashMap<PackageKey, PackageMetadata>,
    has_custom_fetcher: bool,
) {
    let Some(cell) = planned_canonical_fetches else { return };
    let mut planned = HashSet::with_capacity(cold.len());
    for (snapshot_key, _snapshot) in cold {
        if let Some(entry) = planned_canonical_fetch(snapshot_key, packages, has_custom_fetcher) {
            planned.insert(entry);
        }
    }
    let _ = cell.set(planned);
}

/// Mirrors the verification runner's candidate keying: a registry-qualified
/// key contributes its bare semver plus its registry alias, so entries routed
/// to different registries never share a key. A custom fetcher can replace the
/// canonical download, so its result cannot establish registry-side existence.
fn planned_canonical_fetch(
    snapshot_key: &PackageKey,
    packages: &HashMap<PackageKey, PackageMetadata>,
    has_custom_fetcher: bool,
) -> Option<(String, String, Option<String>)> {
    if has_custom_fetcher {
        return None;
    }
    let metadata_key = snapshot_key.without_peer();
    let metadata = packages.get(&metadata_key)?;
    if !matches!(metadata.resolution, LockfileResolution::Registry(_))
        || metadata.resolution.checkable_integrity().is_none()
    {
        return None;
    }
    let (registry_alias, version) = match metadata_key.suffix.registry_qualified() {
        Some((alias, version)) => (Some(alias.to_string()), version.to_string()),
        None => (None, metadata_key.suffix.version().to_string()),
    };
    Some((metadata_key.name.to_string(), version, registry_alias))
}

/// `get_pkg_id_with_patch_hash` strips the peer-graph suffix but keeps
/// `(patch_hash=...)` so patched packages share one CAS-paths entry across
/// their peer variants.
fn cas_paths_key(snapshot_key: &PackageKey) -> PkgIdWithPatchHash {
    PkgIdWithPatchHash::from(get_pkg_id_with_patch_hash(&snapshot_key.to_string()).to_string())
}

/// Per-slot obsolete child aliases for the link pass. Only survivors that
/// already existed in `current_snapshots` and dropped a child contribute an
/// entry; fresh packages and addition-only changes map to the empty slice.
fn removed_aliases_by_key(
    current_snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    snapshot_entries: &[SnapshotWithCacheKey<'_>],
) -> HashMap<PackageKey, Vec<PkgName>> {
    let Some(current_snapshots) = current_snapshots else { return HashMap::new() };
    snapshot_entries
        .iter()
        .filter_map(|(snapshot_key, snapshot, _)| {
            let current_snapshot = current_snapshots.get(*snapshot_key)?;
            let removed = removed_child_aliases(current_snapshot, snapshot, &snapshot_key.name);
            (!removed.is_empty()).then(|| ((*snapshot_key).clone(), removed))
        })
        .collect()
}

fn nothing_to_materialize(is_hoisted: bool) -> CreateVirtualStoreOutput {
    CreateVirtualStoreOutput {
        package_manifests: PackageManifests::new(),
        side_effects_maps_by_snapshot: SideEffectsMapsBySnapshot::new(),
        requires_build_by_snapshot: RequiresBuildBySnapshot::new(),
        materialized_snapshots: Vec::new(),
        fetch_failed: HashSet::new(),
        cas_paths_by_pkg_id: is_hoisted.then(CasPathsByPkgId::new),
    }
}

mod partition;
mod snapshot_plan;

#[cfg(test)]
mod tests;
