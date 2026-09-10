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
    LockfileEntries, LockfileResolution, PackageKey, PackageMetadata, PkgIdWithPatchHash, PkgName,
    PkgNameVerPeer, PlatformSelector, SnapshotEntry, select_platform_variant,
};
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::{
    files_include_install_scripts, manifest_requires_build, parse_manifest,
};
use pnpm_reporter::{
    LogEvent, LogLevel, ProgressLog, ProgressMessage, Reporter, StatsLog, StatsMessage,
};
use pnpm_store_dir::{
    SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreDir, StoreIndex, StoreIndexWriter,
    store_index_key,
};
use pnpm_tarball::{
    MemCache, PrefetchIntegrityCheck, PrefetchResult, SharedReportedProgressKeys,
    prefetch_cas_paths,
};
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

fn derive_cache_keys(
    config: &Config,
    entries: LockfileEntries<'_>,
    supported_architectures: Option<&pnpm_package_is_installable::SupportedArchitectures>,
) -> HashMap<PackageKey, Result<SnapshotCacheKey, CreateVirtualStoreError>> {
    let (Some(snapshots), Some(packages)) = (entries.snapshots, entries.packages) else {
        return HashMap::new();
    };
    let selector = runtime_platform_selector(supported_architectures);
    snapshots
        .keys()
        .map(|snapshot_key| {
            let cache_key =
                snapshot_cache_key(snapshot_key, packages, config.ignore_scripts, &selector);
            (snapshot_key.clone(), cache_key)
        })
        .collect()
}

/// Sorted + deduplicated so `prefetch_cas_paths` doesn't redo identical
/// SELECT + integrity-check work for peer variants of one package.
/// Derived leniently over the whole lockfile — the superset over what
/// the plan pass will keep merely prefetches a few rows nothing reads.
fn prefetch_keys(
    cache_keys: &HashMap<PackageKey, Result<SnapshotCacheKey, CreateVirtualStoreError>>,
) -> Vec<String> {
    let mut refs: Vec<&str> = cache_keys
        .values()
        .filter_map(|cache_key| cache_key.as_ref().ok())
        .filter_map(|cache_key| cache_key.value.as_deref())
        .collect();
    refs.sort_unstable();
    refs.dedup();
    refs.into_iter().map(String::from).collect()
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

    #[display("Failed to inspect the virtual store slot at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_INSPECT_VIRTUAL_STORE_SLOT))]
    InspectVirtualStoreSlot {
        path: PathBuf,
        #[error(source)]
        error: std::io::Error,
    },
}

impl<'a> CreateVirtualStore<'a> {
    /// Execute the subroutine. Returns the set of bundled manifests
    /// recovered from `index.db` for the warm-batch slots — the
    /// bin linker uses these to avoid re-reading `package.json` per
    /// child. See [`PackageManifests`].
    pub async fn run<Reporter: self::Reporter>(
        mut self,
    ) -> Result<CreateVirtualStoreOutput, CreateVirtualStoreError> {
        let Some(wanted) = self.wanted()? else {
            return Ok(nothing_to_materialize(self.is_hoisted()));
        };
        let prefetch = self.prefetch(wanted).await;
        let marker_source = self.prepare_store().await?;
        let mut plan = self.plan::<Reporter>(wanted, prefetch.cache_keys)?;
        let prefetched = self
            .settle_prefetch(
                prefetch.task,
                &prefetch.verified_files_cache,
                wanted.packages,
                &mut plan,
            )
            .await?;
        self.materialize_plan::<Reporter>(
            wanted,
            plan,
            &prefetched,
            CreateVirtualStoreStoreContext {
                index: prefetch.store_index.as_ref(),
                verified_files_cache: &prefetch.verified_files_cache,
            },
            marker_source.as_ref(),
        )
        .await
    }

    async fn materialize_plan<Reporter: self::Reporter>(
        &self,
        wanted: WantedEntries<'a>,
        plan: snapshot_plan::SnapshotPlan<'a>,
        prefetched: &PrefetchResult,
        store: CreateVirtualStoreStoreContext<'_>,
        marker_source: Option<&tempfile::NamedTempFile>,
    ) -> Result<CreateVirtualStoreOutput, CreateVirtualStoreError> {
        let mut partition = partition::partition_snapshots(
            &plan.survivors,
            &plan.skipped_entries,
            prefetched,
            &plan.marker_rebuilds,
            self.ctx.node_linker,
        );

        // Publish the cold-batch fetch plan for the concurrent
        // verification fan-out: every cold registry-resolved snapshot
        // with a pinned hash is downloaded from its canonical registry
        // URL by this run (or fails the install / is dropped as an
        // uninstallable optional), which is the existence evidence the
        // npm verifier's age gate may substitute for a metadata body.
        // First fill wins; entries outside the plan keep the
        // metadata-backed path.
        publish_planned_canonical_fetches(
            self.planned_canonical_fetches,
            &partition.cold,
            wanted.packages,
            self.custom_fetcher_session.is_some(),
        );

        let links = self.link_plan(&plan);
        let mut indexes =
            CasIndexes::warm(links.shared_packages.as_ref(), &partition.warm, self.is_hoisted());
        self.link_warm::<Reporter>(
            wanted,
            &partition,
            &links,
            marker_source.map(tempfile::NamedTempFile::path),
        )?;
        let fetch_failed = self
            .download_cold::<Reporter>(
                ColdInputs { wanted, store, prefetched, marker_source, links: &links },
                &mut partition,
                &mut indexes,
            )
            .await?;
        self.apply_side_effects(wanted, &mut partition, &indexes.shared_base).await;

        // The writer is owned by the caller now. They drop their
        // sender and await the join handle after the build phase
        // finishes, so the final batch flushes after every queued
        // row from both the download path and the WRITE-path
        // upload.

        Ok(CreateVirtualStoreOutput {
            package_manifests: partition.package_manifests,
            side_effects_maps_by_snapshot: partition.side_effects_maps_by_snapshot,
            requires_build_by_snapshot: partition.requires_build_by_snapshot,
            materialized_snapshots: plan.materialized_keys(),
            fetch_failed,
            cas_paths_by_pkg_id: indexes.by_pkg_id,
        })
    }

    fn is_hoisted(&self) -> bool {
        matches!(self.ctx.node_linker, NodeLinker::Hoisted)
    }

    fn wanted(&self) -> Result<Option<WantedEntries<'a>>, CreateVirtualStoreError> {
        // No snapshots to install. If the lockfile also has no project deps
        // this is a valid no-op; if it does, pnpm would have populated
        // `snapshots`, so bailing out here is safe enough for v9.
        let Some(snapshots) = self.entries.snapshots else { return Ok(None) };
        let packages =
            self.entries.packages.ok_or(CreateVirtualStoreError::MissingPackagesSection)?;
        Ok(Some(WantedEntries { packages, snapshots }))
    }

    async fn prefetch(&mut self, wanted: WantedEntries<'a>) -> CasPrefetch {
        match self.cas_prefetch.take() {
            Some(prefetch) => prefetch,
            None => {
                CasPrefetch::start(
                    self.ctx.config,
                    LockfileEntries {
                        packages: Some(wanted.packages),
                        snapshots: Some(wanted.snapshots),
                    },
                    self.supported_architectures,
                    self.store_context.as_ref(),
                )
                .await
            }
        }
    }

    async fn prepare_store(
        &self,
    ) -> Result<Option<tempfile::NamedTempFile>, CreateVirtualStoreError> {
        let config = self.ctx.config;
        let store_dir: &'static _ = &config.store_dir;
        if self.store_context.is_none() {
            init_store_dir_unless_frozen(config, store_dir).await;
        }
        create_build_marker_source(config, self.ctx.layout, store_dir)
    }

    /// The plan pass consumes the keys [`CasPrefetch::start`] derived
    /// (one per snapshot, kept as `Result`s so the strict/lenient
    /// asymmetry documented on `plan_snapshots` survives the early
    /// derivation), stashing each survivor's key alongside its
    /// `(snapshot_key, snapshot)` tuple for the warm/cold partition.
    /// Its slot probes run while the prefetch task reads the store
    /// index.
    fn plan<Reporter: self::Reporter>(
        &self,
        wanted: WantedEntries<'a>,
        mut cache_keys: HashMap<PackageKey, Result<SnapshotCacheKey, CreateVirtualStoreError>>,
    ) -> Result<snapshot_plan::SnapshotPlan<'a>, CreateVirtualStoreError> {
        let config = self.ctx.config;
        let plan = snapshot_plan::plan_snapshots::<Reporter>(snapshot_plan::SnapshotPlanInputs {
            snapshots: wanted.snapshots,
            packages: wanted.packages,
            current_entries: self.current_entries,
            layout: self.ctx.layout,
            allow_build_policy: self.ctx.allow_build_policy,
            skipped: self.skipped,
            link_dependencies: !self.is_hoisted() && config.symlink,
            force: config.force,
            is_hoisted: self.is_hoisted(),
            include_optional_dependencies: self.include_optional_dependencies,
            cache_keys: &mut cache_keys,
        })?;

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
        //
        // Under the hoisted linker every snapshot survives the skip
        // filter (no slot to probe), and which packages are already on
        // disk is only known once its walker has run, so
        // [`crate::link_hoisted_modules()`] emits both stats there. A
        // `virtualStoreOnly` install never runs the linker, so it keeps
        // the count from here.
        if !self.is_hoisted() || self.ctx.config.virtual_store_only {
            Reporter::emit(&LogEvent::Stats(StatsLog {
                level: LogLevel::Debug,
                message: StatsMessage::Added {
                    prefix: self.ctx.requester.to_owned(),
                    added: plan.survivors.len() as u64,
                },
            }));
        }
        Ok(plan)
    }

    /// A joined-task failure degrades to an empty result: every lookup
    /// misses and the snapshots fall through to their per-snapshot
    /// path, the same shape as a store with no index.
    async fn settle_prefetch(
        &self,
        task: tokio::task::JoinHandle<PrefetchResult>,
        verified_files_cache: &SharedVerifiedFilesCache,
        packages: &HashMap<PackageKey, PackageMetadata>,
        plan: &mut snapshot_plan::SnapshotPlan<'_>,
    ) -> Result<PrefetchResult, CreateVirtualStoreError> {
        let prefetched = task.await.unwrap_or_else(|error| {
            tracing::warn!(
                target: "pacquet::install",
                ?error,
                "warm-cache prefetch task failed; treating every lookup as a miss",
            );
            PrefetchResult::default()
        });
        let prefetched = self.verify_imported_rows(prefetched, verified_files_cache, plan).await;
        enforce_cached_git_prepare_policy(
            &mut plan.survivors,
            packages,
            &prefetched,
            self.ctx.allow_build_policy,
            self.ctx.config.ignore_scripts,
            plan.has_git_hosted_survivor,
        )?;
        Ok(prefetched)
    }

    /// Run the files checks the prefetch deferred for the rows whose
    /// files this install may import: every survivor, and every skipped
    /// snapshot whose row carries a side-effects overlay, which the
    /// build phase's cache hit materializes into the slot. A row whose
    /// CAFS files have gone missing is dropped and re-fetched. The other
    /// skipped snapshots keep their rows unchecked: nothing imports
    /// their files, and their manifests and `requiresBuild` flags come
    /// from the row itself.
    async fn verify_imported_rows(
        &self,
        mut prefetched: PrefetchResult,
        verified_files_cache: &SharedVerifiedFilesCache,
        plan: &snapshot_plan::SnapshotPlan<'_>,
    ) -> PrefetchResult {
        if prefetched.pending_checks.is_empty() {
            return prefetched;
        }
        let imported_keys = imported_cache_keys(plan, &prefetched);
        let store_dir: &'static StoreDir = &self.ctx.config.store_dir;
        let verified_files_cache = SharedVerifiedFilesCache::clone(verified_files_cache);
        tokio::task::spawn_blocking(move || {
            let verify_start = std::time::Instant::now();
            let failed = prefetched.verify_rows(
                imported_keys.iter().map(String::as_str),
                store_dir,
                &verified_files_cache,
            );
            tracing::debug!(
                target: "pacquet::download",
                rows = imported_keys.len(),
                failed,
                verify_ms = verify_start.elapsed().as_millis() as u64,
                "store rows of the imported snapshots verified",
            );
            prefetched
        })
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(
                target: "pacquet::install",
                ?error,
                "store-row verification task failed; treating every lookup as a miss",
            );
            PrefetchResult::default()
        })
    }

    fn link_plan(&self, plan: &snapshot_plan::SnapshotPlan<'_>) -> LinkPlan<'a> {
        let config = self.ctx.config;
        LinkPlan {
            removed_aliases_by_key: removed_aliases_by_key(
                self.current_entries.snapshots,
                &plan.survivors,
            ),
            shared_packages: config
                .remote_side_effects_cache
                .as_ref()
                .map(|settings| settings.packages.iter().map(String::as_str).collect()),
            template: LinkSlotsParallel {
                batch: "warm",
                slots: &[],
                layout: self.ctx.layout,
                dir_clone_cache: self.dir_clone_cache,
                symlink: config.symlink,
                import_method: config.package_import_method,
                logged_methods: self.ctx.logged_methods,
                requester: self.ctx.requester,
                skipped: self.skipped,
                include_optional_dependencies: self.include_optional_dependencies,
                progress_reported: self.progress_reported,
                #[cfg(test)]
                link_concurrency_probe: self.link_concurrency_probe,
            },
        }
    }

    fn link_warm<Reporter: self::Reporter>(
        &self,
        wanted: WantedEntries<'a>,
        partition: &partition::Partition<'_>,
        links: &LinkPlan<'_>,
        marker_source: Option<&Path>,
    ) -> Result<(), CreateVirtualStoreError> {
        link_warm_batch::<Reporter>(
            &partition.warm,
            &WarmLinkBatch {
                packages: wanted.packages,
                current_packages: self.current_entries.packages,
                is_hoisted: self.is_hoisted(),
                needs_build_marker_source: marker_source,
                removed_aliases_by_key: &links.removed_aliases_by_key,
                template: &links.template,
            },
        )
    }

    /// Snapshots that did not prefetch fall through to the tokio +
    /// download path. An optional snapshot whose fetch fails is dropped
    /// rather than aborting the install; the returned set holds those
    /// keys for the caller to fold into its [`crate::SkippedSnapshots`],
    /// so downstream walkers (`build_graph`, `link_bins`, hoist) treat
    /// the snapshot as absent. Under the hoisted linker no slot is
    /// written and each download's CAS index is the only output, folded
    /// into [`CasIndexes::by_pkg_id`]; the isolated linker's slot import
    /// has already happened by the time the download future returns.
    async fn download_cold<Reporter: self::Reporter>(
        &self,
        inputs: ColdInputs<'_, 'a>,
        partition: &mut partition::Partition<'_>,
        indexes: &mut CasIndexes,
    ) -> Result<HashSet<PackageKey>, CreateVirtualStoreError> {
        let runtime_platform_selector = runtime_platform_selector(self.supported_architectures);
        let mut fetch_failed = HashSet::new();
        let mut cold_cas_paths = Vec::new();
        run_cold_batch::<Reporter>(
            ColdBatch {
                cold: &partition.cold,
                installer: self.cold_installer(&inputs, &runtime_platform_selector),
                packages: inputs.wanted.packages,
                current_packages: self.current_entries.packages,
                marker_source: inputs.marker_source,
                removed_aliases_by_key: &inputs.links.removed_aliases_by_key,
                link_template: &inputs.links.template,
                shared_packages: inputs.links.shared_packages.as_ref(),
                is_hoisted: self.is_hoisted(),
            },
            &mut ColdBatchState {
                fetch_failed: &mut fetch_failed,
                requires_build_by_snapshot: &mut partition.requires_build_by_snapshot,
                shared_base_cas_paths: &mut indexes.shared_base,
            },
            &mut cold_cas_paths,
        )
        .await?;
        indexes.add_cold(cold_cas_paths);
        Ok(fetch_failed)
    }

    // Defer slot links to the parallel drain, outside the cooperative download tasks.
    fn cold_installer<'i>(
        &'i self,
        inputs: &'i ColdInputs<'_, 'a>,
        runtime_platform_selector: &'i PlatformSelector,
    ) -> InstallPackageBySnapshot<'i> {
        InstallPackageBySnapshot {
            ctx: self.ctx,
            http_client: self.http_client,
            store_index: inputs.store.index,
            store_index_writer: Some(self.store_index_writer),
            prefetched_cas_paths: Some(&inputs.prefetched.cas_paths),
            tarball_mem_cache: self.tarball_mem_cache,
            progress_reported: Some(self.progress_reported),
            verified_files_cache: inputs.store.verified_files_cache,
            skipped: self.skipped,
            include_optional_dependencies: self.include_optional_dependencies,
            runtime_platform_selector,
            custom_fetcher_session: self.custom_fetcher_session,
            // The slot link is deferred to the parallel pass in
            // `drain_cold_downloads` so it doesn't serialize
            // inside this cooperative task.
            defer_link: true,
            #[cfg(test)]
            link_concurrency_probe: self.link_concurrency_probe,
        }
    }

    async fn apply_side_effects(
        &self,
        wanted: WantedEntries<'a>,
        partition: &mut partition::Partition<'_>,
        base_cas_paths: &crate::shared_side_effects::BaseCasPaths,
    ) {
        crate::shared_side_effects::apply_shared_side_effects(
            crate::shared_side_effects::ApplySharedSideEffectsOptions {
                config: self.ctx.config,
                snapshots: wanted.snapshots,
                packages: wanted.packages,
                requires_build_by_snapshot: &partition.requires_build_by_snapshot,
                allow_build_policy: self.ctx.allow_build_policy,
                base_cas_paths,
                side_effects_maps_by_snapshot: &mut partition.side_effects_maps_by_snapshot,
                side_effects_by_snapshot: &partition.side_effects_by_snapshot,
                remote_side_effects_quarantine_by_snapshot: &partition
                    .remote_side_effects_quarantine_by_snapshot,
                store_index_keys_by_snapshot: &partition.store_index_keys_by_snapshot,
                store_index_writer: self.store_index_writer,
            },
        )
        .await;
    }
}

fn imported_cache_keys(
    plan: &snapshot_plan::SnapshotPlan<'_>,
    prefetched: &PrefetchResult,
) -> Vec<String> {
    plan.survivors
        .iter()
        .filter_map(|(_, _, cache_key)| cache_key.clone())
        .chain(
            plan.skipped_entries
                .iter()
                .filter_map(|(_, _, cache_key)| cache_key.clone())
                .filter(|cache_key| prefetched.side_effects_maps.contains_key(cache_key)),
        )
        .collect()
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

struct ColdInputs<'i, 'a> {
    wanted: WantedEntries<'a>,
    store: CreateVirtualStoreStoreContext<'i>,
    prefetched: &'i PrefetchResult,
    marker_source: Option<&'i tempfile::NamedTempFile>,
    links: &'i LinkPlan<'a>,
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
        if !cached_git_prepare_allowed(
            snapshot_key,
            cache_key.as_deref(),
            (packages, prefetch),
            allow_build_policy,
        )? {
            *cache_key = None;
        }
    }
    Ok(())
}

/// Whether the warm slot of one git-hosted snapshot may be reused. `false`
/// drops the cache key so the snapshot is fetched and prepared afresh.
fn cached_git_prepare_allowed(
    snapshot_key: &PackageKey,
    cache_key: Option<&str>,
    lockfile: (&HashMap<PackageKey, PackageMetadata>, &PrefetchResult),
    allow_build_policy: &crate::AllowBuildPolicy,
) -> Result<bool, CreateVirtualStoreError> {
    let (packages, prefetch) = lockfile;
    let Some(key) = cache_key else { return Ok(true) };
    let Some(cas_paths) = prefetch.cas_paths.get(key) else { return Ok(true) };
    let metadata_key = snapshot_key.without_peer();
    let metadata = packages.get(&metadata_key).ok_or_else(|| {
        CreateVirtualStoreError::MissingPackageMetadata {
            snapshot_key: snapshot_key.to_string(),
            metadata_key: metadata_key.to_string(),
        }
    })?;
    if !is_git_hosted_resolution(&metadata.resolution)
        || prefetch.requires_prepare.get(key) == Some(&false)
    {
        return Ok(true);
    }
    let Some(manifest) = cached_git_manifest(prefetch, key, cas_paths) else {
        return Ok(false);
    };
    let package_id = metadata_key.pkg_id();
    let name = manifest.get("name").and_then(serde_json::Value::as_str).unwrap_or("");
    if allow_build_policy.check(&format!("{name}@{package_id}")) == Some(true) {
        return Ok(true);
    }
    if !prefetch.requires_prepare.contains_key(key) {
        return Ok(false);
    }
    let allow_build = |dep_path: &str| allow_build_policy.check(dep_path).unwrap_or(false);
    assert_package_build_allowed(&allow_build, &package_id, &manifest).map_err(|error| {
        CreateVirtualStoreError::InstallPackageBySnapshot(InstallPackageBySnapshotError::GitFetch(
            GitFetcherError::Prepare(error),
        ))
    })?;
    Ok(true)
}

/// The prefetched manifest, or the one the warm slot's `package.json` holds.
/// `None` when neither can be read, which leaves the slot unusable.
fn cached_git_manifest<'a>(
    prefetch: &'a PrefetchResult,
    key: &str,
    cas_paths: &HashMap<String, PathBuf>,
) -> Option<Cow<'a, serde_json::Value>> {
    if let Some(manifest) = prefetch.manifests.get(key) {
        return Some(Cow::Borrowed(manifest.as_ref()));
    }
    let package_json = cas_paths.get("package.json")?;
    let contents = fs::read_to_string(package_json).ok()?;
    parse_manifest(&contents).ok().map(Cow::Owned)
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
    /// Whether the directory-clone cache may serve this slot — see
    /// [`dir_clone_cacheable`].
    dir_clone_cacheable: bool,
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

/// The pristine CAS paths of every warm snapshot whose package may publish
/// shared side effects.
fn warm_shared_base_cas_paths(
    shared_packages: Option<&HashSet<&str>>,
    warm: &[partition::WarmEntry<'_>],
) -> crate::shared_side_effects::BaseCasPaths {
    let mut base_cas_paths = crate::shared_side_effects::BaseCasPaths::new();
    let Some(shared_packages) = shared_packages else { return base_cas_paths };
    for (snapshot_key, _, cas_paths, _, _) in warm {
        if shared_packages.contains(snapshot_key.name.to_string().as_str()) {
            base_cas_paths.insert((*snapshot_key).clone(), (***cas_paths).clone());
        }
    }
    base_cas_paths
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

fn warm_cas_paths_by_pkg_id(warm: &[partition::WarmEntry<'_>]) -> CasPathsByPkgId {
    let mut map = CasPathsByPkgId::with_capacity(warm.len());
    for (snapshot_key, _snapshot, cas_paths, _cache_key, _needs_build_marker) in warm {
        map.entry(cas_paths_key(snapshot_key)).or_insert_with(|| (***cas_paths).clone());
    }
    map
}

/// `get_pkg_id_with_patch_hash` strips the peer-graph suffix but keeps
/// `(patch_hash=...)` so patched packages share one CAS-paths entry across
/// their peer variants.
fn cas_paths_key(snapshot_key: &PackageKey) -> PkgIdWithPatchHash {
    PkgIdWithPatchHash::from(get_pkg_id_with_patch_hash(&snapshot_key.to_string()).to_string())
}

fn add_cold_cas_paths(map: &mut CasPathsByPkgId, cold_cas_paths: Vec<ColdCapture<'_>>) {
    map.reserve(cold_cas_paths.len());
    for ColdCapture { snapshot_key, cas_paths: paths, .. } in cold_cas_paths {
        map.entry(cas_paths_key(snapshot_key)).or_insert(paths);
    }
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

/// What the warm batch links, beyond the slots themselves.
struct WarmLinkBatch<'a> {
    packages: &'a HashMap<PackageKey, PackageMetadata>,
    current_packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    is_hoisted: bool,
    needs_build_marker_source: Option<&'a Path>,
    removed_aliases_by_key: &'a HashMap<PackageKey, Vec<PkgName>>,
    template: &'a LinkSlotsParallel<'a>,
}

/// Link every warm slot. Hoisted skips the batch entirely: no virtual-store
/// slot gets written, so there's no per-snapshot link work to do — the CAS
/// paths captured by the caller are the only output the link phase consumes,
/// and all link work is routed into the hoisted linker instead. It still wants
/// the progress reporter to fire so `pnpm:progress imported`-style updates
/// render the warm hits.
fn link_warm_batch<Reporter: self::Reporter>(
    warm: &[partition::WarmEntry<'_>],
    batch: &WarmLinkBatch<'_>,
) -> Result<(), CreateVirtualStoreError> {
    if batch.is_hoisted {
        emit_hoisted_warm_progress::<Reporter>(warm, batch);
        return Ok(());
    }
    let warm_slots: Vec<SlotLink<'_>> = warm
        .iter()
        .map(|(snapshot_key, snapshot, cas_paths, cache_key, needs_build_marker)| {
            let force_import =
                package_content_changed(batch.current_packages, batch.packages, snapshot_key);
            SlotLink {
                snapshot_key,
                snapshot,
                cas_paths: cas_paths.as_ref(),
                warm_cache_key: Some(cache_key),
                // A cache key means the file map is CAS-backed, and
                // `snapshot_cache_key` yields none for a directory resolution,
                // so a warm slot's source is immutable by construction.
                source_is_mutable: false,
                force_import,
                needs_build_marker_source: needs_build_marker
                    .then_some(batch.needs_build_marker_source)
                    .flatten(),
                dir_clone_cacheable: dir_clone_cacheable(
                    batch.packages,
                    snapshot_key,
                    *needs_build_marker,
                    false,
                    force_import,
                ),
                removed_aliases: removed_aliases_for(batch.removed_aliases_by_key, snapshot_key),
            }
        })
        .collect();
    link_slots_parallel::<Reporter>(LinkSlotsParallel {
        batch: "warm",
        slots: &warm_slots,
        ..*batch.template
    })
}

fn emit_hoisted_warm_progress<Reporter: self::Reporter>(
    warm: &[partition::WarmEntry<'_>],
    batch: &WarmLinkBatch<'_>,
) {
    for (snapshot_key, _, _, cache_key, _) in warm {
        emit_warm_snapshot_progress::<Reporter>(
            &snapshot_key.pkg_id(),
            batch.template.requester,
            batch.template.progress_reported.contains(*cache_key),
        );
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
fn swallow_optional_fetch_failure<Captured>(
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
struct ColdBatch<'a> {
    cold: &'a [(&'a PackageKey, &'a SnapshotEntry)],
    installer: InstallPackageBySnapshot<'a>,
    packages: &'a HashMap<PackageKey, PackageMetadata>,
    current_packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    /// Kept alive by the caller for the whole batch: every slot that
    /// needs a build marker hard-links this one file.
    marker_source: Option<&'a tempfile::NamedTempFile>,
    removed_aliases_by_key: &'a HashMap<PackageKey, Vec<PkgName>>,
    link_template: &'a LinkSlotsParallel<'a>,
    shared_packages: Option<&'a HashSet<&'a str>>,
    is_hoisted: bool,
}

/// Download every cold snapshot and link each one as it lands.
///
/// The downloads run as one cooperative fan-out and the links happen in
/// chunks between completions — see [`drain_cold_downloads`] for why the
/// two are interleaved rather than run in sequence.
async fn run_cold_batch<'a, Reporter: self::Reporter>(
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
async fn download_one<'a, Reporter: self::Reporter>(
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

struct ColdDrain<'a> {
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
async fn drain_cold_downloads<'a, Reporter: self::Reporter, Download>(
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
struct ColdBatchState<'a> {
    fetch_failed: &'a mut HashSet<PackageKey>,
    requires_build_by_snapshot: &'a mut RequiresBuildBySnapshot,
    shared_base_cas_paths: &'a mut crate::shared_side_effects::BaseCasPaths,
}

/// Fold one completed download into the batch state, handing back the capture
/// the link pass still has to place.
fn record_cold_outcome<'a>(
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
                merge_into_slot_group(&mut groups[*entry.get()], slot);
            }
        }
    }
    groups
}

/// Every slot sharing a directory has to have its removed aliases unlinked,
/// so the group carries their union.
fn merge_into_slot_group<'a>(group: &mut SlotDirGroup<'a>, slot: &'a SlotLink<'a>) {
    group.duplicates.push(slot);
    if slot.removed_aliases.is_empty() {
        return;
    }
    let merged = group
        .merged_removed_aliases
        .get_or_insert_with(|| group.representative.removed_aliases.to_vec());
    for alias in slot.removed_aliases {
        if !merged.contains(alias) {
            merged.push(alias.clone());
        }
    }
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
    packages: &HashMap<PackageKey, PackageMetadata>,
    marker_path: Option<&Path>,
    removed_aliases_by_key: &HashMap<PackageKey, Vec<PkgName>>,
    template: &LinkSlotsParallel<'_>,
) -> Result<(), CreateVirtualStoreError> {
    if chunk.is_empty() {
        return Ok(());
    }
    let cold_slots: Vec<SlotLink<'_>> = chunk
        .iter()
        .map(|capture| {
            let needs_build =
                snapshot_needs_build_marker(capture.snapshot_key, capture.requires_build);
            SlotLink {
                snapshot_key: capture.snapshot_key,
                snapshot: capture.snapshot,
                cas_paths: &capture.cas_paths,
                warm_cache_key: None,
                source_is_mutable: capture.source_is_mutable,
                force_import: capture.force_import,
                needs_build_marker_source: needs_build.then_some(marker_path).flatten(),
                dir_clone_cacheable: dir_clone_cacheable(
                    packages,
                    capture.snapshot_key,
                    needs_build,
                    capture.source_is_mutable,
                    capture.force_import,
                ),
                removed_aliases: removed_aliases_for(removed_aliases_by_key, capture.snapshot_key),
            }
        })
        .collect();
    link_slots_parallel::<Reporter>(LinkSlotsParallel { slots: &cold_slots, ..*template })
}

#[derive(Clone, Copy)]
struct LinkSlotsParallel<'a> {
    batch: &'static str,
    slots: &'a [SlotLink<'a>],
    layout: &'a crate::VirtualStoreLayout,
    dir_clone_cache: Option<&'a crate::DirCloneCache<'a>>,
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

    let phase_start = std::time::Instant::now();
    let groups = group_slots_by_dir(opts.slots, opts.layout);
    let link_work =
        || groups.par_iter().try_for_each(|group| link_slot_group::<Reporter>(group, &opts));
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
        batch = opts.batch,
        slots = opts.slots.len(),
        unique_dirs = groups.len(),
        elapsed_ms = phase_start.elapsed().as_millis() as u64,
        "phase complete",
    );

    Ok(())
}

fn link_slot_group<Reporter: self::Reporter>(
    group: &SlotDirGroup<'_>,
    opts: &LinkSlotsParallel<'_>,
) -> Result<(), CreateVirtualStoreError> {
    let slot = group.representative;
    let package_id = slot.snapshot_key.pkg_id();
    emit_group_warm_progress::<Reporter>(group, opts.requester, opts.progress_reported);

    crate::CreateVirtualDirBySnapshot {
        layout: opts.layout,
        cas_paths: slot.cas_paths,
        import_method: opts.import_method,
        logged_methods: opts.logged_methods,
        requester: opts.requester,
        package_id: &package_id,
        package_key: slot.snapshot_key,
        snapshot: slot.snapshot,
        source_is_mutable: slot.source_is_mutable,
        force_import: slot.force_import,
        include_optional_dependencies: opts.include_optional_dependencies,
        symlink: opts.symlink,
        skipped: opts.skipped,
        removed_aliases: group.removed_aliases(),
        needs_build_marker_source: slot.needs_build_marker_source,
        dir_clone_cache: if slot.dir_clone_cacheable { opts.dir_clone_cache } else { None },
        #[cfg(test)]
        link_concurrency_probe: opts.link_concurrency_probe,
    }
    .run::<Reporter>()
    .map_err(|error| {
        CreateVirtualStoreError::InstallPackageBySnapshot(
            InstallPackageBySnapshotError::CreateVirtualDir(error),
        )
    })
}

fn emit_group_warm_progress<Reporter: self::Reporter>(
    group: &SlotDirGroup<'_>,
    requester: &str,
    progress_reported: &SharedReportedProgressKeys,
) {
    let reported = std::iter::once(group.representative).chain(group.duplicates.iter().copied());
    for slot in reported {
        if let Some(cache_key) = slot.warm_cache_key {
            emit_warm_snapshot_progress::<Reporter>(
                &slot.snapshot_key.pkg_id(),
                requester,
                progress_reported.contains(cache_key),
            );
        }
    }
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
            tarball_cache_key(t, &metadata.resolution, &pkg_id, ignore_scripts)
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
            variant_cache_key(variations, runtime_platform_selector, &pkg_id)
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

/// Whether a slot may be served by the macOS directory-clone cache
/// ([`crate::DirCloneCache`]).
///
/// The canonical slot is trusted by its completion marker alone, so
/// everything that identifies the slot's contents must be inside its
/// graph-hash path. That rules out:
///
/// - slots that need a build or patch marker — the canonical copy must
///   stay plain pre-build CAS content;
/// - mutable local sources, which reuse one slot for changing contents;
/// - forced re-imports, whose existing slot is known stale;
/// - any resolution without a checkable integrity. A git dependency
///   hashes to the same slot whether or not its fetch-time `prepare`
///   ran (`--ignore-scripts` versus a build-allowed install), so a
///   cached copy could serve the wrong variant.
fn dir_clone_cacheable(
    packages: &HashMap<PackageKey, PackageMetadata>,
    snapshot_key: &PackageKey,
    needs_build: bool,
    source_is_mutable: bool,
    force_import: bool,
) -> bool {
    !needs_build
        && !source_is_mutable
        && !force_import
        && packages
            .get(&snapshot_key.without_peer())
            .and_then(|metadata| metadata.resolution.checkable_integrity())
            .is_some()
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

fn variant_cache_key(
    variations: &pnpm_lockfile::VariationsResolution,
    runtime_platform_selector: &PlatformSelector,
    pkg_id: &str,
) -> Result<SnapshotCacheKey, CreateVirtualStoreError> {
    let Some(variant) = select_platform_variant(&variations.variants, runtime_platform_selector)
    else {
        return Ok(SnapshotCacheKey { value: None, is_git_hosted: false });
    };
    match &variant.resolution {
        LockfileResolution::Binary(binary) => Ok(SnapshotCacheKey {
            value: Some(store_index_key(&binary.integrity.to_string(), pkg_id)),
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

/// Rejects warm reuse when the downloader would refuse missing integrity.
/// The key must match the fetcher's git-hosted and script-policy variants.
fn tarball_cache_key(
    tarball: &pnpm_lockfile::TarballResolution,
    resolution: &LockfileResolution,
    pkg_id: &str,
    ignore_scripts: bool,
) -> Result<SnapshotCacheKey, CreateVirtualStoreError> {
    if tarball.integrity.is_none() && !unverified_fetch_is_allowed(&tarball.tarball) {
        return Ok(SnapshotCacheKey { value: None, is_git_hosted: false });
    }
    Ok(SnapshotCacheKey {
        value: store_index_key_for_resolution(resolution, pkg_id, !ignore_scripts),
        is_git_hosted: tarball.is_git_hosted(),
    })
}
