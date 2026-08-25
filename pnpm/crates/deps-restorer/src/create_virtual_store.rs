use crate::{
    CasPathsByPkgId, CustomFetcherSession, InstallPackageBySnapshot, InstallPackageBySnapshotError,
    SkippedSnapshots,
    install_package_by_snapshot::{runtime_platform_selector, unverified_fetch_is_allowed},
    store_index_key_for_resolution,
    store_init::init_store_dir_best_effort,
};
use derive_more::{Display, Error};
use futures_util::{StreamExt, stream::FuturesUnordered};
use miette::Diagnostic;
use pnpm_config::{Config, NodeLinker, PackageImportMethod};
use pnpm_deps_path::get_pkg_id_with_patch_hash;
use pnpm_git_fetcher::{GitFetcherError, assert_package_build_allowed};
use pnpm_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, PkgIdWithPatchHash, PkgName, PkgNameVerPeer,
    PlatformSelector, SnapshotEntry, select_platform_variant,
};
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::{
    files_include_install_scripts, manifest_requires_build, parse_manifest,
};
use pnpm_reporter::{
    LogEvent, LogLevel, ProgressLog, ProgressMessage, Reporter, StatsLog, StatsMessage,
};
use pnpm_store_dir::{
    SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreIndex, StoreIndexWriter,
    store_index_key,
};
use pnpm_tarball::{MemCache, PrefetchResult, SharedReportedProgressKeys, prefetch_cas_paths};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicU8},
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

/// Per-snapshot `requiresBuild` flags recovered from the store index
/// during the warm-cache prefetch. `BuildModules` consumes this to
/// avoid re-inspecting every package directory after materialization.
pub type RequiresBuildBySnapshot = HashMap<PackageKey, bool>;

/// Store handles that a fresh resolution and dependency materialization share.
pub struct CreateVirtualStoreStoreContext<'a> {
    pub index: Option<&'a SharedReadonlyStoreIndex>,
    pub verified_files_cache: &'a SharedVerifiedFilesCache,
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
/// (`build_sequence`, `link_bins`, hoisting, etc.) treat them as
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
    /// [`CreateVirtualStore::node_linker`] is
    /// [`NodeLinker::Hoisted`]. Threaded into
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
    pub http_client: &'a ThrottledClient,
    pub config: &'static Config,
    pub packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    pub snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    /// Snapshots and per-version metadata recorded by the previous
    /// install, parsed from `<virtual_store_dir>/lock.yaml`. `None`
    /// on a first install (the file doesn't exist). When present,
    /// per-snapshot lookups against this drive the warm-reinstall
    /// skip decision — see [`CreateVirtualStore::run`].
    pub current_snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    pub current_packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    /// Install-scoped precomputed slot-directory mapping (GVS-aware).
    /// Used by both the warm batch and the cold batch to decide where
    /// each snapshot's `node_modules/<pkg>` lands. See
    /// [`crate::VirtualStoreLayout`].
    pub layout: &'a crate::VirtualStoreLayout,
    /// Install-scoped dedupe state for `pnpm:package-import-method`.
    /// See `link_file::log_method_once`.
    pub logged_methods: &'a AtomicU8,
    /// Install root, threaded into reporter `requester` fields.
    pub requester: &'a str,
    /// Shared store-index writer for the install. Owned by
    /// `InstallFrozenLockfile`, threaded down here for the cold-batch
    /// download path's `InstallPackageBySnapshot` and also reused by
    /// `BuildModules` for the side-effects-cache WRITE path.
    pub store_index_writer: &'a std::sync::Arc<StoreIndexWriter>,
    pub store_context: Option<CreateVirtualStoreStoreContext<'a>>,
    /// `allowBuilds` gate, shared with `BuildModules`. The cold-batch
    /// path threads this into the git fetcher so `preparePackage` can
    /// reject `ERR_PNPM_GIT_DEP_PREPARE_NOT_ALLOWED` for packages that aren't
    /// allowlisted. Computed once per install in
    /// [`crate::InstallFrozenLockfile::run`].
    pub allow_build_policy: &'a crate::AllowBuildPolicy,
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
    /// Lockfile / workspace root (`lockfileDir`). Threaded into the
    /// per-snapshot
    /// [`InstallPackageBySnapshot`] so the directory fetcher can
    /// resolve `LockfileResolution::Directory` entries (e.g.
    /// `directory: "../local-pkg"`) against the same base pnpm uses.
    pub workspace_root: &'a Path,
    /// Selects between the isolated and hoisted install layouts.
    /// Under [`NodeLinker::Isolated`] the warm and cold batches
    /// populate per-snapshot virtual-store slot directories. Under
    /// [`NodeLinker::Hoisted`] the slot writes are skipped entirely
    /// — the hoisted linker
    /// ([`crate::link_hoisted_modules()`]) consumes the per-package
    /// CAS index threaded through
    /// [`CreateVirtualStoreOutput::cas_paths_by_pkg_id`] instead.
    /// Tarball downloads and CAS writes still happen for both
    /// linkers; only the slot-materialization step differs.
    pub node_linker: NodeLinker,
    /// Cache keys whose package status (`fetched` or `found_in_store`)
    /// has already been emitted earlier in this install. The warm batch
    /// still emits `resolved` for those packages, but skips the second
    /// status event so resolve-time prefetch progress is visible without
    /// being double-counted.
    pub progress_reported: &'a SharedReportedProgressKeys,
    /// Install-scoped shared in-flight tarball cache, threaded into each
    /// per-snapshot [`InstallPackageBySnapshot`] so the cold-batch
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
}

impl CreateVirtualStore<'_> {
    /// Execute the subroutine. Returns the set of bundled manifests
    /// recovered from `index.db` for the warm-batch slots — the
    /// bin linker uses these to avoid re-reading `package.json` per
    /// child. See [`PackageManifests`].
    pub async fn run<Reporter: self::Reporter>(
        self,
    ) -> Result<CreateVirtualStoreOutput, CreateVirtualStoreError> {
        let CreateVirtualStore {
            http_client,
            config,
            packages,
            snapshots,
            current_snapshots,
            current_packages,
            layout,
            logged_methods,
            requester,
            store_index_writer,
            store_context,
            allow_build_policy,
            skipped,
            include_optional_dependencies,
            supported_architectures,
            workspace_root,
            node_linker,
            progress_reported,
            tarball_mem_cache,
            custom_fetcher_session,
            planned_canonical_fetches,
            #[cfg(test)]
            link_concurrency_probe,
        } = self;

        let is_hoisted = matches!(node_linker, NodeLinker::Hoisted);
        let runtime_platform_selector = runtime_platform_selector(supported_architectures);

        let Some(snapshots) = snapshots else {
            // No snapshots to install. If the lockfile also has no project deps
            // this is a valid no-op; if it does, pnpm would have populated
            // `snapshots`, so bailing out here is safe enough for v9.
            return Ok(CreateVirtualStoreOutput {
                package_manifests: PackageManifests::new(),
                side_effects_maps_by_snapshot: SideEffectsMapsBySnapshot::new(),
                requires_build_by_snapshot: RequiresBuildBySnapshot::new(),
                materialized_snapshots: Vec::new(),
                fetch_failed: HashSet::new(),
                cas_paths_by_pkg_id: is_hoisted.then(CasPathsByPkgId::new),
            });
        };
        let packages = packages.ok_or(CreateVirtualStoreError::MissingPackagesSection)?;

        // Open the read-only SQLite index once for the whole run instead of
        // per snapshot. Every `InstallPackageBySnapshot` performs a cache
        // lookup against this index before falling through to the network;
        // on a 1352-package lockfile the per-snapshot reopen accounted for
        // ~1.3 s of wall time even with a fully populated store (see <https://github.com/pnpm/pacquet/issues/260>).
        // A `None` here means the store has no `index.db` yet (first install
        // against an empty store), in which case every lookup would miss —
        // so we keep the handle `Option`al and short-circuit.
        //
        // The open itself is synchronous SQLite I/O (`Connection::open_with_flags`
        // + a `PRAGMA busy_timeout`), so park it on the blocking pool instead
        // of stalling the reactor thread, even for the sub-millisecond it
        // usually takes.
        //
        // A `JoinError` here (blocking-task panic, or cancellation during
        // runtime shutdown) is degraded into `None` so the install still
        // makes progress — cache lookups just miss. `shared_readonly_in`
        // already yields `None` for a first-time install against an empty
        // store, and downstream callers handle that shape correctly. We
        // surface the error at `warn!` so a silent task panic or
        // cancellation is still diagnosable in the log.
        let store_dir: &'static _ = &config.store_dir;

        // Eagerly create `files/00..ff` under the v11 store root so per-
        // tarball CAFS writes never pay a `create_dir_all` syscall on the
        // hot path.
        // See [`init_store_dir_best_effort`] for the error-degradation
        // policy shared with `install_without_lockfile.rs`. Skipped under
        // `frozenStore`: the store is read-only and complete, so no
        // directory creation is attempted under its root.
        if store_context.is_none() && !config.frozen_store {
            init_store_dir_best_effort(store_dir).await;
        }

        let needs_build_marker_source =
            if !config.frozen_store && layout.enable_global_virtual_store() {
                Some(tempfile::NamedTempFile::new_in(store_dir.root()).map_err(|error| {
                    CreateVirtualStoreError::CreateBuildMarker {
                        path: store_dir.root().to_path_buf(),
                        error,
                    }
                })?)
            } else {
                None
            };

        let store_index = match store_context.as_ref().and_then(|context| context.index) {
            Some(index) => Some(Arc::clone(index)),
            None => StoreIndex::open_shared(store_dir, config.frozen_store).await,
        };
        let store_index_ref = store_index.as_ref();

        // The batched store-index writer is owned by the caller
        // (`InstallFrozenLockfile::run`) so it survives past
        // `CreateVirtualStore::run` and gets reused by the build
        // phase's side-effects-cache WRITE path, which queues rows
        // after the install path finishes.
        //
        // The cold-batch download path uses the same writer through
        // `InstallPackageBySnapshot.store_index_writer`.
        let store_index_writer_ref = Some(store_index_writer);

        // Install-scoped `verifiedFilesCache`. One `Arc<DashSet>` lives
        // for the duration of the install; every per-snapshot fetch
        // gets the same handle. A CAFS path verified on snapshot A
        // populates the set so snapshot B's verify pass skips the stat
        // / re-hash cost.
        let verified_files_cache = store_context
            .map_or_else(SharedVerifiedFilesCache::default, |context| {
                Arc::clone(context.verified_files_cache)
            });

        // Batch every cache lookup the per-snapshot futures would otherwise
        // each fan into `tokio::task::spawn_blocking`. With 1352 snapshots
        // hitting the default 512-thread blocking pool, each task's actual
        // work (≈40 µs SELECT + per-file integrity stats) gets dwarfed by
        // OS context-switching among hundreds of competing threads
        // (sample-profiling: 20-60 ms wall per call, sum 26-82 s). Doing
        // the same `SELECT`s and integrity checks on one thread holding the
        // index mutex once is dramatically faster — and turns each
        // per-snapshot future's cache lookup into a synchronous
        // `HashMap::get`.
        //
        // Compute the cache keys upfront from `(integrity, pkg_id)` for
        // every snapshot whose metadata has a tarball-style resolution.
        // Tarball-and-Registry resolutions both ship an `Integrity`;
        // Directory and Git resolutions don't go through CAFS at all,
        // so skipping them here matches the per-snapshot path's check.
        // [`snapshot_cache_key`] is the shared key-derivation helper —
        // a future change to the resolution-type handling or key
        // shape stays in one place (Copilot review on <https://github.com/pnpm/pacquet/pull/292>).
        //
        // Walk `snapshots` once, stash the per-snapshot cache key
        // alongside its `(snapshot_key, snapshot)` tuple, and reuse
        // the stashed key for both the prefetch input and the
        // warm/cold partition below. A separate pass to recompute
        // each key would re-allocate two strings per snapshot for
        // nothing (Copilot follow-up review on <https://github.com/pnpm/pacquet/pull/292>).
        //
        // Lockfiles with peer-dependency variants of the same package
        // (e.g. `react-dom@17.0.2(react@17.0.2)` plus
        // `react-dom@17.0.2(react@18.2.0)`) collapse to one cache key
        // because the key is built from `metadata_key.without_peer()`.
        // Sort + dedup the prefetch input so `prefetch_cas_paths`
        // doesn't redo identical SELECT + integrity-check work for
        // every peer variant.
        let snapshot_plan::SnapshotPlan {
            survivors: mut snapshot_entries,
            skipped_entries,
            marker_rebuilds,
            has_git_hosted_survivor,
        } = snapshot_plan::plan_snapshots::<Reporter>(&snapshot_plan::SnapshotPlanInputs {
            snapshots,
            packages,
            current_snapshots,
            current_packages,
            layout,
            allow_build_policy,
            skipped,
            link_dependencies: !is_hoisted && config.symlink,
            is_hoisted,
            include_optional_dependencies,
            ignore_scripts: config.ignore_scripts,
            runtime_platform_selector: &runtime_platform_selector,
        })?;
        let materialized_snapshots =
            snapshot_entries.iter().map(|(snapshot_key, _, _)| (*snapshot_key).clone()).collect();

        // `pnpm:stats added` fires one event per project once the
        // orchestrator has decided how many packages will land in the
        // virtual store. The value is the *delta* between current and
        // wanted lockfile, computed as the post-skip-filter snapshot
        // count so a warm reinstall against an unchanged lockfile
        // reports `added: 0`.
        //
        // The paired `pnpm:stats removed` event is emitted by the
        // caller from [`crate::PruneStaleModules`]'s result, so each
        // install carries exactly one `added` and one `removed`.
        Reporter::emit(&LogEvent::Stats(StatsLog {
            level: LogLevel::Debug,
            message: StatsMessage::Added {
                prefix: requester.to_owned(),
                added: snapshot_entries.len() as u64,
            },
        }));

        // Union the cache keys from survivors and skipped snapshots
        // so the prefetch covers everyone the build phase might need
        // to gate on. Sorted + deduplicated to avoid redundant SQL
        // queries in `prefetch_cas_paths`.
        let mut cache_key_refs: Vec<&str> = snapshot_entries
            .iter()
            .chain(skipped_entries.iter())
            .filter_map(|(_, _, k)| k.as_deref())
            .collect();
        cache_key_refs.sort_unstable();
        cache_key_refs.dedup();
        let cache_keys: Vec<String> = cache_key_refs.into_iter().map(String::from).collect();
        let prefetch = prefetch_cas_paths(
            store_index.clone(),
            store_dir,
            cache_keys,
            config.verify_store_integrity,
            SharedVerifiedFilesCache::clone(&verified_files_cache),
        )
        .await;
        enforce_cached_git_prepare_policy(
            &mut snapshot_entries,
            packages,
            &prefetch,
            allow_build_policy,
            config.ignore_scripts,
            has_git_hosted_survivor,
        )?;
        let partition::Partition {
            warm,
            cold,
            package_manifests,
            side_effects_maps_by_snapshot,
            mut requires_build_by_snapshot,
        } = partition::partition_snapshots(
            &snapshot_entries,
            &skipped_entries,
            &prefetch,
            &marker_rebuilds,
            node_linker,
        );

        // Publish the cold-batch fetch plan for the concurrent
        // verification fan-out: every cold registry-resolved snapshot
        // with a pinned hash is downloaded from its canonical registry
        // URL by this run (or fails the install / is dropped as an
        // uninstallable optional), which is the existence evidence the
        // npm verifier's age gate may substitute for a metadata body.
        // First fill wins; entries outside the plan keep the
        // metadata-backed path.
        if let Some(cell) = planned_canonical_fetches {
            let mut planned = HashSet::with_capacity(cold.len());
            for (snapshot_key, _snapshot) in &cold {
                let metadata_key = snapshot_key.without_peer();
                let Some(metadata) = packages.get(&metadata_key) else { continue };
                // A custom fetcher can replace the canonical download, so
                // its result cannot establish registry-side existence.
                if custom_fetcher_session.is_some()
                    || !matches!(metadata.resolution, LockfileResolution::Registry(_))
                    || metadata.resolution.checkable_integrity().is_none()
                {
                    continue;
                }
                // Mirror the verification runner's candidate keying: a
                // registry-qualified key contributes its bare semver
                // plus its registry alias, so entries routed to
                // different registries never share a key.
                let (registry_alias, version) = match metadata_key.suffix.registry_qualified() {
                    Some((alias, version)) => (Some(alias.to_string()), version.to_string()),
                    None => (None, metadata_key.suffix.version().to_string()),
                };
                planned.insert((metadata_key.name.to_string(), version, registry_alias));
            }
            let _ = cell.set(planned);
        }

        // Hoisted-mode CAS index assembly. Collected here, *before*
        // the warm-batch closure consumes `warm` under the
        // isolated branch below, so the borrow checker doesn't
        // need to reason across the two branches. Cold-batch
        // entries are appended at the bottom of the function once
        // the cold-batch fetch finishes.
        let mut cas_paths_by_pkg_id: Option<CasPathsByPkgId> = is_hoisted.then(|| {
            let mut map = CasPathsByPkgId::with_capacity(warm.len());
            for (snapshot_key, _snapshot, cas_paths, _cache_key, _needs_build_marker) in &warm {
                // `get_pkg_id_with_patch_hash` strips the peer-graph
                // suffix but keeps `(patch_hash=...)` so patched
                // packages share one CAS-paths entry across their peer
                // variants.
                let pkg_id = PkgIdWithPatchHash::from(
                    get_pkg_id_with_patch_hash(&snapshot_key.to_string()).to_string(),
                );
                map.entry(pkg_id).or_insert_with(|| (***cas_paths).clone());
            }
            map
        });

        // Per-slot obsolete child aliases for the link pass. Only
        // survivors that already existed in `current_snapshots` and
        // dropped a child contribute an entry; fresh packages and
        // addition-only changes map to the empty slice. Computed once
        // here so both the warm and cold `SlotLink` batches can borrow
        // it.
        let removed_aliases_by_key: HashMap<PackageKey, Vec<PkgName>> = match current_snapshots {
            Some(current_snapshots) => snapshot_entries
                .iter()
                .filter_map(|(snapshot_key, snapshot, _)| {
                    let current_snapshot = current_snapshots.get(*snapshot_key)?;
                    let removed =
                        removed_child_aliases(current_snapshot, snapshot, &snapshot_key.name);
                    (!removed.is_empty()).then(|| ((*snapshot_key).clone(), removed))
                })
                .collect(),
            None => HashMap::new(),
        };

        let import_method = config.package_import_method;
        if is_hoisted {
            // Hoisted still wants the progress reporter to fire so
            // `pnpm:progress imported`-style updates render the warm
            // hits — the link work just happens later, in
            // `link_hoisted_modules`.
            for (snapshot_key, _, _, cache_key, _) in &warm {
                let package_id = snapshot_key.pkg_id();
                emit_warm_snapshot_progress::<Reporter>(
                    &package_id,
                    requester,
                    progress_reported.contains(*cache_key),
                );
            }
        } else {
            // Hoisted skips this batch entirely: no virtual-store slot
            // gets written, so there's no per-snapshot link work to
            // do — the CAS paths captured below are the only output
            // the link phase consumes. Under `nodeLinker: hoisted` all
            // link work is routed into the hoisted linker instead.
            let warm_slots: Vec<SlotLink<'_>> = warm
                .iter()
                .map(|(snapshot_key, snapshot, cas_paths, cache_key, needs_build_marker)| {
                    SlotLink {
                        snapshot_key,
                        snapshot,
                        cas_paths: cas_paths.as_ref(),
                        warm_cache_key: Some(cache_key),
                        // A cache key means the file map is CAS-backed,
                        // and `snapshot_cache_key` yields none for a
                        // directory resolution, so a warm slot's source
                        // is immutable by construction.
                        source_is_mutable: false,
                        force_import: package_content_changed(
                            current_packages,
                            packages,
                            snapshot_key,
                        ),
                        needs_build_marker_source: needs_build_marker
                            .then_some(
                                needs_build_marker_source
                                    .as_ref()
                                    .map(tempfile::NamedTempFile::path),
                            )
                            .flatten(),
                        removed_aliases: removed_aliases_for(&removed_aliases_by_key, snapshot_key),
                    }
                })
                .collect();
            link_slots_parallel::<Reporter>(LinkSlotsParallel {
                batch: "warm",
                slots: &warm_slots,
                layout,
                symlink: config.symlink,
                import_method,
                logged_methods,
                requester,
                skipped,
                include_optional_dependencies,
                progress_reported,
                #[cfg(test)]
                link_concurrency_probe,
            })?;
        }

        // Cold batch: snapshots that didn't prefetch — fall through to the
        // existing tokio + download path.
        //
        // Per-snapshot result is `(Option<PackageKey>, Option<HashMap>)`:
        // - `Some(key)` in the first slot flags a fetch/extract failure
        //   that was silently swallowed because the snapshot is
        //   `optional: true` — an optional snapshot whose fetch fails is
        //   dropped rather than aborting the install.
        //   Aggregated into `fetch_failed` for the caller to fold into
        //   its [`crate::SkippedSnapshots`] so downstream walkers
        //   (`build_sequence`, `link_bins`, hoist) treat the snapshot
        //   as absent.
        // - The second slot is the per-snapshot CAS index returned by
        //   [`InstallPackageBySnapshot::run`], threaded into
        //   `cas_paths_by_pkg_id` under hoisted (the linker consumes
        //   it directly). `None` for the isolated linker — its
        //   per-slot import has already happened by the time the
        //   future returns; under hoisted no slot was written and the
        //   CAS index is the only output.
        let mut fetch_failed: HashSet<PackageKey> = HashSet::new();
        let mut cold_cas_paths: Vec<ColdCapture<'_>> = Vec::new();
        if !cold.is_empty() {
            let prefetched_ref = Some(&prefetch.cas_paths);
            let verified_files_cache_ref = &verified_files_cache;
            let runtime_platform_selector_ref = &runtime_platform_selector;
            type ColdOutcome<'a> = (Option<PackageKey>, Option<ColdCapture<'a>>);
            let mut downloads: FuturesUnordered<_> = cold
                .iter()
                .map(|(snapshot_key, snapshot)| async move {
                    let metadata_key = snapshot_key.without_peer();
                    let metadata = packages.get(&metadata_key).ok_or_else(|| {
                        CreateVirtualStoreError::MissingPackageMetadata {
                            snapshot_key: snapshot_key.to_string(),
                            metadata_key: metadata_key.to_string(),
                        }
                    })?;
                    let result = InstallPackageBySnapshot {
                        http_client,
                        config,
                        layout,
                        store_index: store_index_ref,
                        store_index_writer: store_index_writer_ref,
                        prefetched_cas_paths: prefetched_ref,
                        tarball_mem_cache,
                        progress_reported: Some(progress_reported),
                        verified_files_cache: verified_files_cache_ref,
                        logged_methods,
                        requester,
                        package_key: snapshot_key,
                        metadata,
                        snapshot,
                        allow_build_policy,
                        skipped,
                        include_optional_dependencies,
                        runtime_platform_selector: runtime_platform_selector_ref,
                        workspace_root,
                        node_linker,
                        custom_fetcher_session,
                        // The slot link is deferred to the parallel pass
                        // below so it doesn't serialize inside this
                        // cooperative `try_join_all` task.
                        defer_link: true,
                        #[cfg(test)]
                        link_concurrency_probe,
                    }
                    .run::<Reporter>()
                    .await;
                    match result {
                        Ok(installed) => {
                            let crate::InstalledPackage { cas_paths, source_is_mutable } =
                                installed;
                            let requires_build = requires_build_from_cas_paths(&cas_paths);
                            Ok((
                                None,
                                Some(ColdCapture {
                                    snapshot_key,
                                    snapshot,
                                    cas_paths,
                                    requires_build,
                                    source_is_mutable,
                                    force_import: package_content_changed(
                                        current_packages,
                                        packages,
                                        snapshot_key,
                                    ),
                                }),
                            ))
                        }
                        Err(err) if snapshot.optional && is_fetch_side_failure(&err) => {
                            // Silent swallow. `tracing::warn!` gives
                            // operator visibility without polluting the
                            // reporter wire: the frozen path emits
                            // nothing here; only the resolver-side emit
                            // site fires `pnpm:skipped-optional-
                            // dependency reason=resolution_failure`.
                            //
                            // Scoped via [`is_fetch_side_failure`] to the
                            // tarball-fetch / git-fetch / CAS-write
                            // variants — the fetch-side surface an
                            // optional snapshot is allowed to swallow.
                            // Local materialization (`CreateVirtualDir`)
                            // and config-shape errors
                            // (`MissingTarballIntegrity`,
                            // `UnsupportedResolution`) abort even for
                            // optional snapshots — they sit outside the
                            // swallowed fetch surface.
                            tracing::warn!(
                                target: "pacquet::install",
                                snapshot = %snapshot_key,
                                error = %err,
                                "optional snapshot fetch/extract failed; dropping from install",
                            );
                            Ok((Some((*snapshot_key).clone()), None))
                        }
                        Err(err) => Err(CreateVirtualStoreError::InstallPackageBySnapshot(err)),
                    }
                })
                .collect();

            // The downloads deferred their slot links (`defer_link:
            // true`) because a blocking link inside this single
            // cooperative task would serialize them; linking chunks
            // between completions keeps that work off the tail without
            // starving the pipe — a chunk's `block_in_place` pause is
            // milliseconds, absorbed by kernel socket buffers. GVS
            // peer variants sharing one slot dir may split across
            // chunks: chunks run sequentially, and a later pass over a
            // complete slot short-circuits on its completion marker.
            let marker_path = needs_build_marker_source.as_ref().map(tempfile::NamedTempFile::path);
            let mut ready: Vec<ColdCapture<'_>> = Vec::new();
            while let Some(outcome) = downloads.next().await {
                let (failure, captured): ColdOutcome<'_> = outcome?;
                if let Some(key) = failure {
                    fetch_failed.insert(key);
                }
                let Some(captured) = captured else { continue };
                requires_build_by_snapshot
                    .insert((*captured.snapshot_key).clone(), captured.requires_build);
                if is_hoisted {
                    cold_cas_paths.push(captured);
                    continue;
                }
                ready.push(captured);
                if ready.len() >= COLD_LINK_CHUNK {
                    let chunk = std::mem::take(&mut ready);
                    link_cold_chunk::<Reporter>(
                        &chunk,
                        marker_path,
                        &removed_aliases_by_key,
                        &LinkSlotsParallel {
                            batch: "cold",
                            slots: &[],
                            layout,
                            symlink: config.symlink,
                            import_method,
                            logged_methods,
                            requester,
                            skipped,
                            include_optional_dependencies,
                            progress_reported,
                            #[cfg(test)]
                            link_concurrency_probe,
                        },
                    )?;
                }
            }
            drop(downloads);
            if !ready.is_empty() {
                link_cold_chunk::<Reporter>(
                    &ready,
                    marker_path,
                    &removed_aliases_by_key,
                    &LinkSlotsParallel {
                        batch: "cold",
                        slots: &[],
                        layout,
                        symlink: config.symlink,
                        import_method,
                        logged_methods,
                        requester,
                        skipped,
                        include_optional_dependencies,
                        progress_reported,
                        #[cfg(test)]
                        link_concurrency_probe,
                    },
                )?;
            }
        }

        // Build the per-pkg CAS index when the install is targeting
        // the hoisted linker. Pacquet's fetcher and walker run
        // independently, so the CAS index is collected here and
        // handed to the linker in [`crate::link_hoisted_modules()`]
        // through this output field.
        //
        // Key shape: [`PkgIdWithPatchHash`] mirrors the
        // `pkg_id_with_patch_hash` field that the slice 4 walker
        // assigns to each [`crate::DependenciesGraphNode`] (see
        // [`crate::hoisted_dep_graph`]). Until pacquet has end-to-end
        // patch support, the value equals the snapshot key including
        // any peer suffix; that matches what the walker writes, so
        // `<linker>.cas_paths_by_pkg_id.get(&node.pkg_id_with_patch_hash)`
        // hits.
        //
        // Peer-variants of the same package share a single
        // [`std::sync::Arc<HashMap>`] in the warm batch (see
        // `package_manifests` at the loop above for the same Arc
        // sharing pattern). The linker takes an owned
        // `HashMap<String, PathBuf>` per package, so each variant
        // gets a (cheap) clone of the underlying map — `PathBuf`
        // clones are short string copies, and the per-variant
        // duplication only matters when the lockfile has many
        // peer-resolved variants, which is a small fraction of any
        // real install.
        if let Some(map) = cas_paths_by_pkg_id.as_mut() {
            map.reserve(cold_cas_paths.len());
            for ColdCapture { snapshot_key, cas_paths: paths, .. } in cold_cas_paths {
                // `get_pkg_id_with_patch_hash` strips the peer-graph
                // suffix but keeps `(patch_hash=...)` so patched
                // packages share one CAS-paths entry across their peer
                // variants.
                let pkg_id = PkgIdWithPatchHash::from(
                    get_pkg_id_with_patch_hash(&snapshot_key.to_string()).to_string(),
                );
                map.entry(pkg_id).or_insert(paths);
            }
        }

        // The writer is owned by the caller now. They drop their
        // sender and await the join handle after the build phase
        // finishes, so the final batch flushes after every queued
        // row from both the download path and the WRITE-path
        // upload.

        Ok(CreateVirtualStoreOutput {
            package_manifests,
            side_effects_maps_by_snapshot,
            requires_build_by_snapshot,
            materialized_snapshots,
            fetch_failed,
            cas_paths_by_pkg_id,
        })
    }
}

/// Look up the obsolete child aliases for a slot, defaulting to an
/// empty slice. The extra indirection lets the [`SlotLink`] builders
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

fn enforce_cached_git_prepare_policy(
    snapshots: &mut [SnapshotWithCacheKey<'_>],
    packages: &HashMap<PackageKey, PackageMetadata>,
    prefetch: &PrefetchResult,
    allow_build_policy: &crate::AllowBuildPolicy,
    ignore_scripts: bool,
    has_git_hosted_survivor: bool,
) -> Result<(), CreateVirtualStoreError> {
    if ignore_scripts || !has_git_hosted_survivor {
        return Ok(());
    }
    for (snapshot_key, _snapshot, cache_key) in snapshots {
        let Some(key) = cache_key.as_deref() else { continue };
        let Some(cas_paths) = prefetch.cas_paths.get(key) else { continue };
        let metadata_key = snapshot_key.without_peer();
        let metadata = packages.get(&metadata_key).ok_or_else(|| {
            CreateVirtualStoreError::MissingPackageMetadata {
                snapshot_key: snapshot_key.to_string(),
                metadata_key: metadata_key.to_string(),
            }
        })?;
        if !is_git_hosted_resolution(&metadata.resolution) {
            continue;
        }
        if prefetch.requires_prepare.get(key) == Some(&false) {
            continue;
        }
        let manifest = if let Some(manifest) = prefetch.manifests.get(key) {
            Cow::Borrowed(manifest.as_ref())
        } else {
            let Some(package_json) = cas_paths.get("package.json") else {
                *cache_key = None;
                continue;
            };
            let Ok(contents) = fs::read_to_string(package_json) else {
                *cache_key = None;
                continue;
            };
            let Ok(manifest) = parse_manifest(&contents) else {
                *cache_key = None;
                continue;
            };
            Cow::Owned(manifest)
        };
        let package_id = metadata_key.pkg_id();
        let name = manifest.get("name").and_then(serde_json::Value::as_str).unwrap_or("");
        let dep_path = format!("{name}@{package_id}");
        if allow_build_policy.check(&dep_path) == Some(true) {
            continue;
        }
        if !prefetch.requires_prepare.contains_key(key) {
            *cache_key = None;
            continue;
        }
        let allow_build = |dep_path: &str| allow_build_policy.check(dep_path).unwrap_or(false);
        assert_package_build_allowed(&allow_build, &package_id, &manifest).map_err(|error| {
            CreateVirtualStoreError::InstallPackageBySnapshot(
                InstallPackageBySnapshotError::GitFetch(GitFetcherError::Prepare(error)),
            )
        })?;
    }
    Ok(())
}

fn is_git_hosted_resolution(resolution: &LockfileResolution) -> bool {
    match resolution {
        LockfileResolution::Git(_) => true,
        LockfileResolution::Tarball(tarball) => tarball.is_git_hosted(),
        _ => false,
    }
}

fn requires_build_from_cas_paths(cas_paths: &HashMap<String, PathBuf>) -> bool {
    if files_include_install_scripts(cas_paths.keys()) {
        return true;
    }
    let Some(package_json) = cas_paths.get("package.json") else { return false };
    let Ok(contents) = fs::read_to_string(package_json) else { return false };
    let Ok(manifest) = parse_manifest(&contents) else {
        return false;
    };
    manifest_requires_build(&manifest)
}

fn snapshot_needs_build_marker(snapshot_key: &PackageKey, requires_build: bool) -> bool {
    requires_build || crate::snapshot_has_patch(snapshot_key)
}

fn gvs_slot_needs_rebuild(
    layout: &crate::VirtualStoreLayout,
    allow_build_policy: &crate::AllowBuildPolicy,
    snapshot_key: &PackageKey,
) -> bool {
    if !layout.enable_global_virtual_store() {
        return false;
    }
    let can_build = crate::snapshot_has_patch(snapshot_key)
        || allow_build_policy.check(&snapshot_key.without_peer().to_string()) == Some(true);
    can_build
        && layout
            .slot_dir(snapshot_key)
            .join("node_modules")
            .join(snapshot_key.name.to_string())
            .join(crate::NEEDS_BUILD_MARKER)
            .is_file()
}

/// A cold snapshot whose CAS paths are staged and whose slot link was
/// deferred to [`link_slots_parallel`].
struct ColdCapture<'a> {
    snapshot_key: &'a PackageKey,
    snapshot: &'a SnapshotEntry,
    cas_paths: HashMap<String, PathBuf>,
    requires_build: bool,
    source_is_mutable: bool,
    force_import: bool,
}

struct SlotLink<'a> {
    snapshot_key: &'a PackageKey,
    snapshot: &'a SnapshotEntry,
    cas_paths: &'a HashMap<String, PathBuf>,
    warm_cache_key: Option<&'a str>,
    source_is_mutable: bool,
    force_import: bool,
    needs_build_marker_source: Option<&'a Path>,
    /// Child aliases dropped since the previous install, threaded into
    /// [`crate::CreateVirtualDirBySnapshot::removed_aliases`] so their
    /// stale symlinks are unlinked during the link pass.
    removed_aliases: &'a [PkgName],
}

/// One unique slot directory and every [`SlotLink`] that resolved to
/// it. Under the global virtual store, hash-equal peer variants share
/// a slot path, and `stage_and_swap` in
/// [`fn@crate::import_indexed_dir`] assumes an exclusive owner per
/// directory — so the link pass runs one task per group, with the
/// `removed_aliases` of every member unioned for cleanup.
struct SlotDirGroup<'a> {
    representative: &'a SlotLink<'a>,
    /// Kept so each warm variant still emits its own progress line.
    duplicates: Vec<&'a SlotLink<'a>>,
    /// `None` until a duplicate contributes an alias the
    /// representative lacks.
    merged_removed_aliases: Option<Vec<PkgName>>,
}

impl SlotDirGroup<'_> {
    fn removed_aliases(&self) -> &[PkgName] {
        self.merged_removed_aliases.as_deref().unwrap_or(self.representative.removed_aliases)
    }
}

/// Group `slots` by [`crate::VirtualStoreLayout::slot_dir`], preserving
/// first-occurrence order.
fn group_slots_by_dir<'a>(
    slots: &'a [SlotLink<'a>],
    layout: &crate::VirtualStoreLayout,
) -> Vec<SlotDirGroup<'a>> {
    if !layout.enable_global_virtual_store() {
        // Project-local slot names embed the peer-suffixed key: every
        // group is a singleton, so skip the path construction.
        return slots
            .iter()
            .map(|slot| SlotDirGroup {
                representative: slot,
                duplicates: Vec::new(),
                merged_removed_aliases: None,
            })
            .collect();
    }
    let mut index_by_dir: HashMap<PathBuf, usize> = HashMap::with_capacity(slots.len());
    let mut groups: Vec<SlotDirGroup<'a>> = Vec::with_capacity(slots.len());
    for slot in slots {
        match index_by_dir.entry(layout.slot_dir(slot.snapshot_key)) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(groups.len());
                groups.push(SlotDirGroup {
                    representative: slot,
                    duplicates: Vec::new(),
                    merged_removed_aliases: None,
                });
            }
            std::collections::hash_map::Entry::Occupied(entry) => {
                let group = &mut groups[*entry.get()];
                group.duplicates.push(slot);
                if !slot.removed_aliases.is_empty() {
                    let merged = group
                        .merged_removed_aliases
                        .get_or_insert_with(|| group.representative.removed_aliases.to_vec());
                    for alias in slot.removed_aliases {
                        if !merged.contains(alias) {
                            merged.push(alias.clone());
                        }
                    }
                }
            }
        }
    }
    groups
}

/// How many completed cold snapshots accumulate before a link chunk
/// runs. Small enough that each chunk's `block_in_place` pause stays in
/// the low milliseconds; large enough that the rayon pass has real
/// parallelism to spend it on.
const COLD_LINK_CHUNK: usize = 32;

/// Build [`SlotLink`]s for one chunk of cold captures and run the
/// parallel link pass over them. `template` carries the pass-invariant
/// fields; its `slots` are ignored.
fn link_cold_chunk<Reporter: self::Reporter>(
    chunk: &[ColdCapture<'_>],
    marker_path: Option<&Path>,
    removed_aliases_by_key: &HashMap<PackageKey, Vec<PkgName>>,
    template: &LinkSlotsParallel<'_>,
) -> Result<(), CreateVirtualStoreError> {
    let cold_slots: Vec<SlotLink<'_>> = chunk
        .iter()
        .map(|capture| SlotLink {
            snapshot_key: capture.snapshot_key,
            snapshot: capture.snapshot,
            cas_paths: &capture.cas_paths,
            warm_cache_key: None,
            source_is_mutable: capture.source_is_mutable,
            force_import: capture.force_import,
            needs_build_marker_source: snapshot_needs_build_marker(
                capture.snapshot_key,
                capture.requires_build,
            )
            .then_some(marker_path)
            .flatten(),
            removed_aliases: removed_aliases_for(removed_aliases_by_key, capture.snapshot_key),
        })
        .collect();
    link_slots_parallel::<Reporter>(LinkSlotsParallel { slots: &cold_slots, ..*template })
}

#[derive(Clone, Copy)]
struct LinkSlotsParallel<'a> {
    batch: &'static str,
    slots: &'a [SlotLink<'a>],
    layout: &'a crate::VirtualStoreLayout,
    symlink: bool,
    import_method: PackageImportMethod,
    logged_methods: &'a AtomicU8,
    requester: &'a str,
    skipped: &'a SkippedSnapshots,
    include_optional_dependencies: bool,
    progress_reported: &'a SharedReportedProgressKeys,
    #[cfg(test)]
    link_concurrency_probe:
        Option<&'a crate::create_virtual_dir_by_snapshot::tests::LinkConcurrencyProbe>,
}

fn link_slots_parallel<Reporter: self::Reporter>(
    opts: LinkSlotsParallel<'_>,
) -> Result<(), CreateVirtualStoreError> {
    use rayon::prelude::*;

    let LinkSlotsParallel {
        batch,
        slots,
        layout,
        symlink,
        import_method,
        logged_methods,
        requester,
        skipped,
        include_optional_dependencies,
        progress_reported,
        #[cfg(test)]
        link_concurrency_probe,
    } = opts;

    let phase_start = std::time::Instant::now();
    let groups = group_slots_by_dir(slots, layout);
    let link_work = || {
        groups.par_iter().try_for_each(|group| {
            let slot = group.representative;
            let package_id = slot.snapshot_key.pkg_id();
            for reported in std::iter::once(slot).chain(group.duplicates.iter().copied()) {
                if let Some(cache_key) = reported.warm_cache_key {
                    emit_warm_snapshot_progress::<Reporter>(
                        &reported.snapshot_key.pkg_id(),
                        requester,
                        progress_reported.contains(cache_key),
                    );
                }
            }

            crate::CreateVirtualDirBySnapshot {
                layout,
                cas_paths: slot.cas_paths,
                import_method,
                logged_methods,
                requester,
                package_id: &package_id,
                package_key: slot.snapshot_key,
                snapshot: slot.snapshot,
                source_is_mutable: slot.source_is_mutable,
                force_import: slot.force_import,
                include_optional_dependencies,
                symlink,
                skipped,
                removed_aliases: group.removed_aliases(),
                needs_build_marker_source: slot.needs_build_marker_source,
                #[cfg(test)]
                link_concurrency_probe,
            }
            .run::<Reporter>()
            .map_err(|error| {
                CreateVirtualStoreError::InstallPackageBySnapshot(
                    InstallPackageBySnapshotError::CreateVirtualDir(error),
                )
            })
        })
    };
    // Driving the link pass from inside an `async fn` means the
    // `par_iter` blocks the calling tokio worker for the duration. On
    // the production multi-thread runtime, `block_in_place` migrates
    // other futures off this worker so async progress continues; it
    // panics on the `current_thread` runtime that `#[tokio::test]`
    // defaults to, so fall back to a plain call there.
    let on_multi_thread = tokio::runtime::Handle::try_current()
        .is_ok_and(|handle| handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread);
    if on_multi_thread {
        tokio::task::block_in_place(link_work)?;
    } else {
        link_work()?;
    }
    tracing::info!(
        target: "pacquet::install::phase",
        phase = "link_slots",
        batch,
        slots = slots.len(),
        unique_dirs = groups.len(),
        elapsed_ms = phase_start.elapsed().as_millis() as u64,
        "phase complete",
    );

    Ok(())
}

/// Build the store-index cache key for a snapshot.
///
/// Returns `Err` for missing metadata — a condition the install would
/// fail on anyway — so the orchestrator can short-circuit *before* the
/// warm rayon batch runs; otherwise a malformed lockfile does up to
/// ~6 s of warm-batch linking before the actual error fires.
///
/// Shared by the upfront prefetch-keys loop and the warm/cold
/// partition in [`CreateVirtualStore::run`], so a future change to
/// the resolution-type handling or key shape stays in one place.
/// A drift between the two loops would silently misclassify warm
/// entries as cold and quietly halve install speed.
fn snapshot_cache_key(
    snapshot_key: &PackageKey,
    packages: &HashMap<PackageKey, PackageMetadata>,
    ignore_scripts: bool,
    runtime_platform_selector: &PlatformSelector,
) -> Result<SnapshotCacheKey, CreateVirtualStoreError> {
    let metadata_key = snapshot_key.without_peer();
    let metadata = packages.get(&metadata_key).ok_or_else(|| {
        CreateVirtualStoreError::MissingPackageMetadata {
            snapshot_key: snapshot_key.to_string(),
            metadata_key: metadata_key.to_string(),
        }
    })?;
    let pkg_id = metadata_key.pkg_id();
    match &metadata.resolution {
        LockfileResolution::Tarball(t) => {
            // A tarball with no integrity that isn't one of the shapes
            // exempt from verification never reaches the store: the
            // fetch path refuses it. Give it no warm key at all, so a
            // row already sitting at the shared `pkg_id\tbuilt` key
            // (written for a git-hosted package of the same id) can't
            // skip that refusal — pnpm likewise asserts fetchability
            // before it consults the store.
            if t.integrity.is_none() && !unverified_fetch_is_allowed(&t.tarball) {
                return Ok(SnapshotCacheKey { value: None, is_git_hosted: false });
            }
            // Git-hosted tarballs land in the CAS via
            // `pnpm_git_fetcher::GitHostedTarballFetcher`, which
            // writes the row under `gitHostedStoreIndexKey(pkg_id,
            // built)` rather than the integrity-based key — and so
            // does a tarball with no integrity to key a row by.
            // `pick_store_index_key` picks the same shape pnpm picks,
            // so the warm prefetch finds the row on a re-install.
            // `built` tracks `!ignore_scripts` in lock-step with the
            // dispatcher's write key, so the prefetch and the write
            // address the same slot.
            Ok(SnapshotCacheKey {
                value: store_index_key_for_resolution(
                    &metadata.resolution,
                    &pkg_id,
                    !ignore_scripts,
                ),
                is_git_hosted: t.is_git_hosted(),
            })
        }
        LockfileResolution::Registry(r) => Ok(SnapshotCacheKey {
            value: Some(store_index_key(&r.integrity.to_string(), &pkg_id)),
            is_git_hosted: false,
        }),
        LockfileResolution::Directory(_) => {
            // Directory resolutions are injected workspace deps and
            // bypass the CAFS entirely (the directory-fetcher returns
            // source-path entries; no `write_cas_file` happens, no
            // `PackageFilesIndex` row is written). There is therefore
            // no warm-cache key to recover the install from — every
            // install re-walks the source dir (the source may have
            // changed since the last install). Returning `Ok(None)`
            // routes the snapshot
            // through the cold path which runs the fetcher.
            Ok(SnapshotCacheKey { value: None, is_git_hosted: false })
        }
        LockfileResolution::Git(_) => {
            // `Git` resolutions land in CAS via
            // `pnpm_git_fetcher::GitFetcher`, which writes the
            // row under the same `gitHostedStoreIndexKey` shape as
            // the git-hosted tarball path. Returning the key here
            // lets the warm prefetch reuse a previous install's
            // clone + checkout + prepare + packlist work — without
            // this, every git install cold-paths regardless of
            // whether the snapshot is already in `index.db`. `built`
            // tracks `!ignore_scripts` to match the dispatcher's
            // write key.
            Ok(SnapshotCacheKey {
                value: store_index_key_for_resolution(
                    &metadata.resolution,
                    &pkg_id,
                    !ignore_scripts,
                ),
                is_git_hosted: true,
            })
        }
        // Runtime artifacts (Node.js / Bun / Deno): the per-archive
        // integrity is the warm-cache key, same shape as the
        // registry / tarball arms above. Mirrors the per-snapshot
        // dispatch in [`InstallPackageBySnapshot::run`]; the cold
        // path's variant selector + binary fetcher writes the row
        // under this key when it succeeds, so a re-install hits
        // here instead of cold-fetching the runtime archive again.
        LockfileResolution::Binary(binary) => Ok(SnapshotCacheKey {
            value: Some(store_index_key(&binary.integrity.to_string(), &pkg_id)),
            is_git_hosted: false,
        }),
        // `Variations` is a meta-shape: its integrity lives on the
        // *picked* variant, not the wrapper. Run the same host-
        // matching selector the cold path runs so the warm key
        // resolves to the variant that would actually be installed.
        // No variant matched → return `Ok(None)` and let the cold
        // path surface the typed `NoMatchingPlatformVariant` error
        // (a warm-key miss is the right shape; the warm prefetch
        // is best-effort and the cold path is where errors are
        // raised).
        LockfileResolution::Variations(variations) => {
            let Some(variant) =
                select_platform_variant(&variations.variants, runtime_platform_selector)
            else {
                return Ok(SnapshotCacheKey { value: None, is_git_hosted: false });
            };
            match &variant.resolution {
                LockfileResolution::Binary(binary) => Ok(SnapshotCacheKey {
                    value: Some(store_index_key(&binary.integrity.to_string(), &pkg_id)),
                    is_git_hosted: false,
                }),
                // Non-`Binary` variant (corrupt lockfile, or a
                // future shape pacquet doesn't recognise). The
                // cold path raises the typed
                // `VariantHasNonBinaryResolution` error; we just
                // skip the warm key.
                _ => Ok(SnapshotCacheKey { value: None, is_git_hosted: false }),
            }
        }
        // Custom resolutions have no built-in warm-cache key — the
        // cold path consults the pnpmfile custom fetchers, and the
        // delegated resolution (unknowable here) determines the row
        // that gets written.
        LockfileResolution::Custom(_) => Ok(SnapshotCacheKey { value: None, is_git_hosted: false }),
    }
}

struct SnapshotCacheKey {
    value: Option<String>,
    is_git_hosted: bool,
}

/// Two snapshots agree on dependency wiring when both their
/// `dependencies` and `optionalDependencies` maps are equal (an
/// absent map and an empty map count as equal).
fn snapshot_deps_equal(current: &SnapshotEntry, wanted: &SnapshotEntry) -> bool {
    fn maps_equal<Key, Value>(
        lhs: Option<&HashMap<Key, Value>>,
        rhs: Option<&HashMap<Key, Value>>,
    ) -> bool
    where
        Key: std::cmp::Eq + std::hash::Hash,
        Value: PartialEq,
    {
        match (lhs, rhs) {
            (None, None) => true,
            (Some(map), None) | (None, Some(map)) => map.is_empty(),
            (Some(x), Some(y)) => x == y,
        }
    }
    maps_equal(current.dependencies.as_ref(), wanted.dependencies.as_ref())
        && maps_equal(current.optional_dependencies.as_ref(), wanted.optional_dependencies.as_ref())
}

/// Compare the `integrity` field on two `packages:` entries.
fn integrity_equal(current: Option<&PackageMetadata>, wanted: Option<&PackageMetadata>) -> bool {
    let current_integrity = current.and_then(|meta| meta.resolution.integrity());
    let wanted_integrity = wanted.and_then(|meta| meta.resolution.integrity());
    current_integrity == wanted_integrity
}

fn package_content_changed(
    current_packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    wanted_packages: &HashMap<PackageKey, PackageMetadata>,
    snapshot_key: &PackageKey,
) -> bool {
    let current = current_packages.and_then(|packages| packages.get(&snapshot_key.without_peer()));
    let wanted = wanted_packages.get(&snapshot_key.without_peer());
    current.is_some() && !integrity_equal(current, wanted)
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
fn is_fetch_side_failure(err: &InstallPackageBySnapshotError) -> bool {
    matches!(
        err,
        InstallPackageBySnapshotError::DownloadTarball(_)
            | InstallPackageBySnapshotError::GitFetch(_)
            | InstallPackageBySnapshotError::DirectoryFetch(_)
            | InstallPackageBySnapshotError::CustomFetcher(_),
    )
}

fn emit_warm_snapshot_progress<Reporter: self::Reporter>(
    package_id: &str,
    requester: &str,
    progress_reported: bool,
) {
    Reporter::emit(&LogEvent::Progress(ProgressLog {
        level: LogLevel::Debug,
        message: ProgressMessage::Resolved {
            package_id: package_id.to_owned(),
            requester: requester.to_owned(),
        },
    }));
    if !progress_reported {
        Reporter::emit(&LogEvent::Progress(ProgressLog {
            level: LogLevel::Debug,
            message: ProgressMessage::FoundInStore {
                package_id: package_id.to_owned(),
                requester: requester.to_owned(),
            },
        }));
    }
}

mod partition;
mod snapshot_plan;

#[cfg(test)]
mod tests;
