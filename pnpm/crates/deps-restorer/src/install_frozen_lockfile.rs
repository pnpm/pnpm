use crate::{
    AllowBuildPolicy, BuildModules, BuildModulesError, CreateVirtualStore, CreateVirtualStoreError,
    CreateVirtualStoreOutput, HoistedDepGraphError, HoistedDependencies, LinkHoistedModulesError,
    LinkHoistedModulesOpts, LinkRootComponentMembersError, LinkVirtualStoreBinsError,
    LockfileToHoistedDepGraphOptions, SkippedSnapshots, SymlinkDirectDependencies,
    SymlinkDirectDependenciesError, SymlinkPackageError, VersionPolicyError, VirtualStoreLayout,
    any_installability_constraint, build_direct_deps_by_importer, direct_dep_names_for_importer,
    get_hoisted_dependencies, link_hoisted_modules, link_top_level_bins,
    lockfile_to_hoisted_dep_graph, symlink_direct_dependencies::importer_root_dir,
};

mod build_phase;
mod hoisted;

pub use build_phase::{
    BuildPhaseError, BuildPhaseInputs, resolve_snapshot_patches, run_build_phase,
};
pub use hoisted::{
    HoistPlan, HoistedLinkerError, HoistedLinkerInputs, HoistedLinkerOutput,
    collect_public_hoist_targets, compute_hoist_plan, find_own_runtime_node_major,
    find_runtime_node_major, parse_major_from_version, run_hoisted_linker,
    workspace_packages_for_hoist,
};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_cmd_shim::{LinkBinsError, LinkBinsOptions};
use pnpm_config::{Config, NodeLinker, matcher::create_matcher};
use pnpm_executor::ScriptsPrependNodePath as ExecScriptsPrependNodePath;
use pnpm_lockfile::{
    Lockfile, PackageKey, PackageMetadata, Prefix, ProjectSnapshot, SnapshotEntry,
};
use pnpm_lockfile_verification::{
    VerifyError, VerifyLockfileResolutionsOptions, verify_lockfile_resolutions,
};
use pnpm_modules_yaml::{Host, IncludedDependencies, read_modules_manifest};
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::DependencyGroup;
use pnpm_patching::{
    ExtendedPatchInfo, PatchKeyConflictError, ResolvePatchedDependenciesError, get_patch_info,
};
use pnpm_reporter::{IgnoredScriptsLog, LogEvent, LogLevel, Reporter, Stage, StageLog};
use pnpm_resolving_resolver_base::ResolutionVerifier;
use pnpm_store_dir::{StoreIndexError, StoreIndexWriter};
use pnpm_tarball::{MemCache, SharedReportedProgressKeys};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    ffi::OsStr,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, atomic::AtomicU8},
};

pub type LockfileVerificationOverride<'a> =
    Pin<Box<dyn Future<Output = Result<(), InstallFrozenLockfileError>> + Send + 'a>>;

/// This subroutine installs dependencies from a frozen lockfile.
///
/// **Brief overview:**
/// * Iterate over each snapshot in the v9 `snapshots:` map.
/// * Fetch the tarball for the matching `packages:` entry.
/// * Extract each tarball into the store directory.
/// * Import the files from the store dir to each `node_modules/.pacquet/{name}@{version}/node_modules/{name}/`.
/// * Create dependency symbolic links in each `node_modules/.pacquet/{name}@{version}/node_modules/`.
/// * Create a symbolic link at each `node_modules/{name}`.
#[must_use]
pub struct InstallFrozenLockfile<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    pub http_client: &'a ThrottledClient,
    pub config: &'static Config,
    pub pnpmfile_hook: Option<&'a Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub importers: &'a HashMap<String, ProjectSnapshot>,
    pub packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    pub snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    /// The fully-deserialized wanted lockfile. Carried alongside
    /// the destructured `importers` / `packages` / `snapshots`
    /// references because the hoisted-linker walker
    /// ([`crate::lockfile_to_hoisted_dep_graph`]) takes a
    /// `&Lockfile` (it threads the lockfile into
    /// [`pnpm_real_hoist::hoist`] which needs every importer's
    /// direct deps plus the full `packages` / `snapshots` maps in
    /// one borrow). Isolated installs ignore the field.
    pub lockfile: &'a Lockfile,
    /// Resolution verifiers to re-apply to every lockfile entry. Run
    /// concurrently with the fetch phase ([`crate::CreateVirtualStore`])
    /// and awaited before any dependency lifecycle script executes, so a
    /// rejected lockfile aborts before [`crate::BuildModules`] runs. Empty
    /// when verification is disabled (`trustLockfile`), in which case the
    /// gate is a no-op. The non-blocking sequencing runs
    /// `verifyLockfileResolutions` concurrently with the fetch and gates
    /// the build on `verifyLockfile`.
    pub resolution_verifiers: &'a [Arc<dyn ResolutionVerifier>],
    /// When set, replaces the local `resolution_verifiers` fan-out as the
    /// trust verdict — used by the pnpr client to delegate verification to
    /// the server's `/-/pnpr/v0/verify-lockfile` while the fetch runs locally. The
    /// same concurrent sequencing and build gate apply.
    pub lockfile_verification_override: Option<LockfileVerificationOverride<'a>>,
    /// Absolute path of the lockfile being verified, for the on-disk
    /// verification cache. `None` disables the cache.
    pub lockfile_path: Option<&'a Path>,
    /// The previous install's persisted current lockfile, threaded
    /// through to the hoisted walker for `prev_graph` (orphan
    /// diff). `None` on a first install.
    pub current_lockfile: Option<&'a Lockfile>,
    /// Snapshots from the previous install's `lock.yaml`, if present.
    /// Threaded through to [`crate::CreateVirtualStore`] to drive the
    /// per-snapshot skip decision (a snapshot whose wiring and
    /// integrity haven't changed and whose virtual-store slot still
    /// exists on disk is dropped from the install graph). `None` on a
    /// first install — the current-lockfile file doesn't exist yet.
    pub current_snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    pub current_packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    pub dependency_groups: DependencyGroupList,
    pub project_manifests: &'a [(PathBuf, &'a pnpm_package_manifest::PackageManifest)],
    pub package_map_project_manifests:
        &'a [(PathBuf, &'a pnpm_package_manifest::PackageManifest)],
    /// Install-scoped dedupe state for `pnpm:package-import-method`.
    /// See `link_file::log_method_once`.
    pub logged_methods: &'a AtomicU8,
    /// Install root — the directory containing `pnpm-lock.yaml`.
    /// For a real workspace, this is the workspace root (the dir
    /// containing `pnpm-workspace.yaml`); for a single-project
    /// install, it's the project dir.
    ///
    /// Reporter envelopes (`pnpm:stage`, `pnpm:summary`, `pnpm:lifecycle`)
    /// use [`requester`], a lossy-UTF-8 string view of this path —
    /// per-importer events like `pnpm:root` use the importer's own
    /// `rootDir` instead. Filesystem operations that need the real
    /// path (the per-importer `node_modules/` write under
    /// `SymlinkDirectDependencies`, the `lockfile_dir` threaded into
    /// `BuildModules`) use `workspace_root` directly so the round-trip
    /// through a lossy string can never corrupt the on-disk path on
    /// hosts with non-UTF-8 filenames.
    ///
    /// [`requester`]: Self::requester
    pub workspace_root: &'a Path,

    /// Lossy-UTF-8 view of [`workspace_root`] for reporter envelopes.
    /// Kept as a separate field rather than recomputed from
    /// `workspace_root` so the caller controls how the conversion is
    /// performed (today: `to_string_lossy().into_owned()` in
    /// `Install::run`).
    ///
    /// [`workspace_root`]: Self::workspace_root
    pub requester: &'a str,
    /// CLI-merged `supportedArchitectures` from
    /// `pnpm-workspace.yaml` plus `--cpu` / `--os` / `--libc`
    /// overrides. Threaded into [`crate::InstallabilityHost`] so the
    /// platform-tagged optional-dependency filter respects user-
    /// supplied architecture overrides.
    pub supported_architectures: Option<&'a pnpm_package_is_installable::SupportedArchitectures>,

    /// When `true`, runtime dependencies (`node@runtime:`,
    /// `deno@runtime:`, `bun@runtime:`) — i.e. packages whose
    /// metadata resolution is `Binary` or `Variations` — are
    /// added to the install-time skip set and the rest of the
    /// install ignores them. Computed at the CLI layer from
    /// `config.skip_runtimes || --no-runtime`.
    pub skip_runtimes: bool,

    /// Effective `nodeVersion`: an explicit config value, otherwise the
    /// minimum version declared by the root manifest's runtime engine.
    pub node_version: Option<String>,

    /// A host detection the install entry point spawned right after
    /// the wanted lockfile parsed (see
    /// [`crate::materialization_plan::HostDetection::spawn`]), so its
    /// `node --version` overlaps the planning here. Must have been
    /// spawned with this install's `node_version` /
    /// `supported_architectures` / `engine_strict`. `None` runs the
    /// detection here.
    pub early_host_detection: Option<crate::materialization_plan::HostDetection>,

    /// `nodeLinker` value to honor for *this* invocation. Threaded
    /// from the package manager's `Install` caller (which has already
    /// applied any `--node-linker` CLI override on top of
    /// [`pnpm_config::Config::node_linker`]).
    ///
    /// Under [`NodeLinker::Hoisted`] the install pipeline routes
    /// through [`crate::lockfile_to_hoisted_dep_graph`] +
    /// [`crate::link_hoisted_modules()`] instead of the isolated
    /// linker's [`crate::SymlinkDirectDependencies`] +
    /// [`crate::LinkVirtualStoreBins`] + [`crate::get_hoisted_dependencies`]
    /// chain, matching the `nodeLinker === 'hoisted'` branch in
    /// `headlessInstall`.
    ///
    /// [`NodeLinker::Pnp`] shares the isolated virtual-store materialization,
    /// then replaces importer dependency links with the project-level `PnP`
    /// loader during the link phase.
    pub node_linker: NodeLinker,

    /// Install-scoped shared in-flight tarball cache, threaded down to
    /// [`crate::CreateVirtualStore`]'s cold-batch downloads. `Some` on
    /// the pnpr client path so the materialization reuses the
    /// the package manager's `TarballPrefetcher` background downloads instead of
    /// re-fetching every tarball; `None` for installs without a shared
    /// prefetch in flight.
    pub tarball_mem_cache: Option<&'a Arc<MemCache>>,
    pub seed_skipped: Option<Vec<String>>,
    /// Forced-rebuild selection threaded from `pacquet rebuild` /
    /// `approve-builds`; `None` for a normal install. Forwarded to
    /// [`run_build_phase`]'s [`BuildPhaseInputs`]. See
    /// [`crate::RebuildOptions`].
    pub rebuild: Option<&'a crate::RebuildOptions>,
    /// `hoistedDependencies` recorded by the previous install's
    /// `.modules.yaml`, for [`crate::PruneStaleModules`]'s orphan
    /// hoist-link cleanup. `None` on a first install or when the file
    /// couldn't be fully parsed.
    pub prior_hoisted_dependencies: Option<&'a crate::HoistedDependencies>,
    /// See [`crate::PruneStaleModules::prune_orphans`].
    pub prune_orphans: bool,
    /// Fetch-evidence cell `CreateVirtualStore` fills after its
    /// warm/cold partition so the concurrent verification fan-out's
    /// age gate can lean on this install's canonical tarball fetches.
    /// See [`pnpm_resolving_resolver_base::PlannedCanonicalFetches`].
    pub planned_canonical_fetches:
        Option<&'a pnpm_resolving_resolver_base::PlannedCanonicalFetches>,
}

/// Error type of [`InstallFrozenLockfile`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum InstallFrozenLockfileError {
    #[diagnostic(transparent)]
    LockfileVerification(#[error(source)] VerifyError),

    #[display("external lockfile verification failed: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_EXTERNAL_LOCKFILE_VERIFICATION))]
    ExternalLockfileVerification(#[error(not(source))] String),

    #[diagnostic(transparent)]
    CreateVirtualStore(#[error(source)] CreateVirtualStoreError),

    /// The pnpmfile threw while loading its custom `fetchers` export.
    /// A throwing pnpmfile aborts the install, matching the
    /// custom-resolver load on the fresh-lockfile path.
    #[display("{_0}")]
    #[diagnostic(code(ERR_PNPM_PNPMFILE_FAIL))]
    CustomFetcherHook(#[error(not(source))] pnpm_hooks::HookError),

    #[diagnostic(transparent)]
    SymlinkDirectDependencies(#[error(source)] SymlinkDirectDependenciesError),

    /// Surfaces a failure while removing stale direct-dep or hoist
    /// links during the pre-link reconciliation pass.
    #[diagnostic(transparent)]
    PruneStaleModules(#[error(source)] crate::PruneDirectDepsError),

    #[diagnostic(transparent)]
    LinkPhase(#[error(source)] crate::linking::LinkPhaseError),

    /// Surfaces a failure to cross-link a Bit root component's injected
    /// members into one another's virtual-store slot. Only reachable
    /// when a project manifest declares
    /// `installConfig.hoistingLimits: "workspaces"`.
    #[diagnostic(transparent)]
    LinkRootComponentMembers(#[error(source)] LinkRootComponentMembersError),

    #[diagnostic(transparent)]
    LinkVirtualStoreBins(#[error(source)] LinkVirtualStoreBinsError),

    /// Surfaces any failure from the shared lifecycle-script build
    /// phase: `patchedDependencies` resolution, the [`BuildModules`]
    /// run itself, or the post-build top-level bin link. Shared with
    /// the fresh-lockfile path via [`run_build_phase`], so both install
    /// modes report the same `ERR_PNPM_*` codes for a failed build.
    #[diagnostic(transparent)]
    BuildPhase(#[error(source)] BuildPhaseError),

    /// Surfaces a failure to create one of the hoist symlinks
    /// (`<private_hoisted_modules_dir>/<alias>` or
    /// `<public_hoisted_modules_dir>/<alias>`). EEXIST is
    /// already swallowed by [`crate::symlink_package()`]; this variant
    /// only fires on genuine IO failures.
    #[diagnostic(transparent)]
    HoistSymlink(#[error(source)] SymlinkPackageError),

    /// Surfaces a failure to link bins of privately-hoisted
    /// dependencies in the `privateHoistedModulesDir` (the
    /// public-side bins go through the existing direct-deps
    /// bin-link pass at the root).
    #[diagnostic(transparent)]
    HoistLinkBins(#[error(source)] LinkBinsError),

    /// Surfaces `ERR_PNPM_INVALID_VERSION_UNION` /
    /// `ERR_PNPM_NAME_PATTERN_IN_VERSION_UNION` when an
    /// `allowBuilds` key in `pnpm-workspace.yaml` can't be parsed.
    #[diagnostic(transparent)]
    VersionPolicy(#[error(source)] VersionPolicyError),

    /// Wraps any error `compute_skipped_snapshots` surfaces from the
    /// installability pass. Three sources, all reachable under
    /// today's default config:
    ///
    /// - `InstallabilityError::InvalidNodeVersion` — the resolved
    ///   `current_node_version` isn't a parseable exact semver.
    ///   Pacquet falls back to a synthetic `99999.0.0` when
    ///   `node --version` fails, so this is currently unreachable
    ///   from production — but a future `nodeVersion` config wiring
    ///   (slice 2) will surface user-supplied bad values here as
    ///   `ERR_PNPM_INVALID_NODE_VERSION`.
    /// - `InstallabilityError::Engine` / `InstallabilityError::Platform`
    ///   from a non-optional incompatible snapshot with
    ///   `engine_strict = true`. Pacquet's default has
    ///   `engine_strict = false`, so this path is currently
    ///   unreachable from production either — wired through so the
    ///   slice that lands the config setting doesn't churn the
    ///   error enum again.
    #[diagnostic(transparent)]
    Installability(#[error(source)] Box<pnpm_package_is_installable::InstallabilityError>),

    /// Surfaces failures from
    /// [`crate::lockfile_to_hoisted_dep_graph`] when the install is
    /// running under `nodeLinker: hoisted`. Includes invalid
    /// snapshot references, multi-importer lockfiles (workspace
    /// support is tracked separately), and installability errors
    /// on required (non-optional) packages.
    #[diagnostic(transparent)]
    HoistedDepGraph(#[error(source)] HoistedDepGraphError),

    /// Surfaces failures from [`crate::link_hoisted_modules()`]
    /// while materializing the on-disk hoisted tree. Includes
    /// missing CAS-paths entries for required packages,
    /// hierarchy/graph mismatches, file-import I/O failures, and
    /// bin-link errors.
    #[diagnostic(transparent)]
    LinkHoistedModules(#[error(source)] LinkHoistedModulesError),

    #[display("failed to write package map: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_WRITE_PACKAGE_MAP))]
    WritePackageMap(#[error(source)] crate::WritePackageMapError),

    #[display("failed to write PnP loader: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_WRITE_PNP_FILE))]
    WritePnpFile(#[error(source)] crate::WritePnpFileError),
}

impl<DependencyGroupList> InstallFrozenLockfile<'_, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    /// Execute the subroutine.
    ///
    /// Returns an [`InstallFrozenLockfileOutput`] carrying the
    /// `HoistedDependencies` map produced by the hoist pass plus
    /// the install-time `SkippedSnapshots` set. The caller
    /// (`Install::run`) feeds both into `.modules.yaml` —
    /// `hoistedDependencies` lets a later install observe the same
    /// hoist decisions, and `skipped` lets the next install seed
    /// the installability re-check against the previously skipped
    /// snapshots.
    pub async fn run<Reporter: self::Reporter>(
        self,
    ) -> Result<InstallFrozenLockfileOutput, InstallFrozenLockfileError> {
        let InstallFrozenLockfile {
            http_client,
            config,
            pnpmfile_hook,
            importers,
            packages,
            snapshots,
            lockfile,
            resolution_verifiers,
            lockfile_verification_override,
            lockfile_path,
            current_lockfile,
            current_snapshots,
            current_packages,
            dependency_groups,
            project_manifests,
            package_map_project_manifests,
            logged_methods,
            workspace_root,
            requester,
            supported_architectures,
            skip_runtimes,
            node_version,
            early_host_detection,
            node_linker,
            tarball_mem_cache,
            seed_skipped,
            rebuild,
            prior_hoisted_dependencies,
            prune_orphans,
            planned_canonical_fetches,
        } = self;

        let is_hoisted = matches!(node_linker, NodeLinker::Hoisted);
        let link_options = crate::shim_link_options(config, node_linker);
        // Cloned so the iterator can be reused below for hoist's
        // direct-deps map. `Vec<DependencyGroup>` is tiny (≤4 enum
        // variants) so the clone is essentially free.
        let dependency_groups: Vec<DependencyGroup> = dependency_groups.into_iter().collect();

        // TODO: check if the lockfile is out-of-date

        // Build the allow-builds policy up front so it can flow into
        // the cold-batch git fetcher in `CreateVirtualStore` as well as
        // the postinstall phase in `BuildModules`. It is a per-install
        // constant.
        let allow_build_policy = AllowBuildPolicy::from_config(config)
            .map_err(InstallFrozenLockfileError::VersionPolicy)?;

        // Spawn the batched store-index writer here so it lives
        // across both the prefetch/download phase (consumers in
        // `CreateVirtualStore`) and the build phase (the new
        // side-effects-cache WRITE-path upload site in
        // `BuildModules`). We drop the orchestrator's clone and
        // await the join handle at the end of `run`, so the final
        // batch flushes once every queued row from both phases has
        // been processed. A writer open / task failure is degraded
        // to a `warn!` and the install still succeeds — pacquet's
        // existing best-effort stance on cache writes.
        // Under `frozenStore` the store is opened read-only, so the
        // writer is replaced with a drain-and-drop stub that never opens
        // `index.db` (no WAL / SHM sidecar under the read-only root).
        let (store_index_writer, writer_task) =
            StoreIndexWriter::spawn_for(&config.store_dir, config.frozen_store);

        // Seed the skip set from the previous install's
        // `.modules.yaml.skipped`. Each entry there is a depPath
        // string a previous run wrote out; on this run we treat each
        // one as already-skipped so its per-snapshot installability
        // check is short-circuited and no
        // `pnpm:skipped-optional-dependency` event is re-emitted for
        // a known-skipped package.
        //
        // A read error (corrupt yaml, permissions) is degraded to
        // an empty seed — `.modules.yaml` is a cache artifact, not
        // an authoritative source. Missing file → empty seed.
        let seed = if config.force {
            // `--force` installs previously-skipped snapshots too, so the
            // recorded skip set must not survive into this install.
            SkippedSnapshots::new()
        } else if let Some(skipped) = seed_skipped {
            SkippedSnapshots::from_strings(&skipped)
        } else {
            match read_modules_manifest::<Host>(&config.modules_dir) {
                Ok(Some(manifest)) => SkippedSnapshots::from_strings(&manifest.skipped),
                Ok(None) => SkippedSnapshots::new(),
                Err(error) => {
                    tracing::warn!(
                        target: "pacquet::install",
                        ?error,
                        "failed to read .modules.yaml for skipped seed; starting from empty",
                    );
                    SkippedSnapshots::new()
                }
            }
        };

        let include_optional = dependency_groups.contains(&DependencyGroup::Optional);
        // Detecting the host is what costs a `node --version`, so it is
        // skipped entirely for the common constraint-free lockfile —
        // otherwise the probe serializes against the extraction that
        // dominates a cold install.
        // `any_installability_constraint` short-circuits on `packages`
        // alone, so the empty-snapshots guard is load-bearing: without
        // it a lockfile with constrained metadata but no snapshots would
        // pay for a `node --version` it has nothing to check.
        let needs_installability_check = !config.force
            && match (snapshots, packages) {
                (Some(snaps), Some(pkgs)) if !snaps.is_empty() => {
                    any_installability_constraint(snaps, pkgs)
                }
                _ => false,
            };
        // The host detection is what costs a `node --version` probe
        // (~150 ms of node startup). The global-virtual-store layout
        // needs the engine name — and so the host — synchronously
        // below, but otherwise the detection stays pending and is only
        // resolved once the skip-set computation needs the host, so
        // the probe runs under the store-side warm-cache prefetch
        // instead of serializing before it. A detection the install
        // entry point already spawned (before the lockfile parse) is
        // adopted so its head start counts; an early detection a
        // constraint-free lockfile turns out not to need is dropped —
        // the probe finishes in the background and its result goes
        // unused.
        let host_detection = if needs_installability_check && !config.enable_global_virtual_store {
            early_host_detection.unwrap_or_else(|| {
                crate::materialization_plan::HostDetection::spawn(
                    config.engine_strict,
                    node_version,
                    supported_architectures.cloned(),
                )
            })
        } else if needs_installability_check {
            crate::materialization_plan::HostDetection::Resolved(match early_host_detection {
                Some(detection) => detection.resolve().await,
                None => {
                    crate::materialization_plan::detect_installability_host(
                        true,
                        config.engine_strict,
                        node_version,
                        supported_architectures,
                    )
                    .await
                }
            })
        } else {
            crate::materialization_plan::HostDetection::Resolved(None)
        };

        // `engine_name` feeds two sites:
        //
        // - The GVS-aware `VirtualStoreLayout` needs it *before*
        //   `CreateVirtualStore::run` to produce per-snapshot
        //   `<scope>/<name>/<version>/<hash>` suffixes under
        //   `<store_dir>/links`. Only matters when GVS is on.
        // - `BuildModules` uses it for the side-effects-cache key
        //   prefix. Read by both the cache read-gate and the
        //   write-gate (see `build_modules.rs:346-350`); when
        //   `None`, both gates close and the cache is bypassed.
        //
        // Honour `engines.runtime` / `devEngines.runtime` pin (if
        // one reached the lockfile): the runtime resolver writes
        // the chosen Node as a `node@runtime:<version>` snapshot, and
        // the engine-name helper anchors the GVS hash and the
        // side-effects-cache key prefix to that pinned Node —
        // otherwise pacquet hashes under whatever
        // `node --version` returns from the shell, splitting the
        // shared store between pinned and non-pinned installs on the
        // same host.
        //
        // Four paths, the first that applies wins:
        // - Runtime pin in the lockfile: the name is known outright.
        // - Host detection still pending (constraint-bearing lockfile,
        //   GVS off): the name is derived from the host once it
        //   resolves below; until then the directory-clone cache reads
        //   it through a shared slot from its lazily built layout.
        // - Host already detected (GVS on, or no check needed): reuse
        //   it synchronously. Synthetic-fallback (`node_detected =
        //   false`) yields `None` so a bogus `99999.0.0`-derived key
        //   can't poison either the cache or the GVS hash.
        // - No host at all: GVS spawns `node --version` synchronously
        //   (its layout needs the result); otherwise the probe is
        //   deferred into the blocking pool, overlaps
        //   `CreateVirtualStore::run`'s I/O, and is awaited right
        //   before `BuildModules`.
        let mut pending_host_engine_slot = None;
        let (engine_name, deferred_engine_name) = match &host_detection {
            crate::materialization_plan::HostDetection::Pending { .. } => {
                if let Some(name) =
                    crate::materialization_plan::engine_name_from_runtime_pin(snapshots)
                {
                    (Some(name), None)
                } else {
                    pending_host_engine_slot =
                        Some(std::sync::Arc::new(std::sync::OnceLock::new()));
                    (None, None)
                }
            }
            crate::materialization_plan::HostDetection::Resolved(host) => {
                let host_node = host.as_ref().map(crate::materialization_plan::HostNode::from);
                crate::materialization_plan::resolve_engine_name(
                    config.enable_global_virtual_store,
                    snapshots,
                    host_node.as_ref(),
                )
                .await
            }
        };

        // Build the install-scoped slot-directory layout. When
        // `enable_global_virtual_store` is on the layout precomputes
        // each snapshot's `<scope>/<name>/<version>/<hash>` suffix
        // from [`pnpm_graph_hasher::calc_graph_node_hash`];
        // otherwise it falls through to the legacy
        // `to_virtual_store_name`-shaped flat name on every
        // `slot_dir` call. Either way every downstream consumer
        // (warm batch, cold batch, direct-dep symlinks, bin linker,
        // build module) routes through this one lookup.
        let layout = VirtualStoreLayout::new(
            config,
            engine_name.as_deref(),
            snapshots,
            packages,
            Some(&allow_build_policy),
            Some(workspace_root),
        );
        // Reject a lockfile whose dependency names, aliases, or
        // virtual-store slots would escape the project or the store once
        // joined into a filesystem path. Runs before any materialization
        // and before the warm-install skip filter, and unconditionally —
        // so it is not bypassed by `trustLockfile`, which disables the
        // resolution-verification fan-out where the offline name check
        // would otherwise run. The slot-containment half needs the
        // install-time `layout`, so it can't live in the verifier crate.
        pnpm_lockfile_verification::verify_lockfile_dependency_names(lockfile)
            .map_err(InstallFrozenLockfileError::LockfileVerification)?;
        crate::validate_virtual_store_slot_containment(snapshots, &layout)
            .map_err(InstallFrozenLockfileError::LockfileVerification)?;

        // Built after the offline lockfile checks above: constructing
        // the cache probes the filesystem (a store-side write), which a
        // rejected lockfile must never reach.
        let dir_clone_cache = crate::DirCloneCache::build(
            config,
            node_linker,
            match (&pending_host_engine_slot, &deferred_engine_name) {
                (Some(slot), _) => crate::EngineNameSource::Pending(std::sync::Arc::clone(slot)),
                (None, Some(deferred)) => crate::EngineNameSource::Pending(deferred.shared()),
                (None, None) => crate::EngineNameSource::Ready(engine_name.clone()),
            },
            snapshots,
            packages,
            Some(&allow_build_policy),
            Some(workspace_root),
        );

        // Kick off the store-side half of `CreateVirtualStore::run`'s
        // planning — like the directory-clone cache above, only after
        // the offline lockfile checks — so its index reads run while a
        // pending host detection finishes its `node --version`.
        let cas_prefetch = crate::create_virtual_store::CasPrefetch::start(
            config,
            snapshots,
            packages,
            supported_architectures,
            None,
        )
        .await;

        let phase_start = std::time::Instant::now();
        let installability_host = host_detection.resolve().await;
        if needs_installability_check {
            tracing::info!(
                target: "pacquet::install::phase",
                phase = "await_installability_host",
                elapsed_ms = phase_start.elapsed().as_millis() as u64,
                "phase complete",
            );
        }
        let host_node =
            installability_host.as_ref().map(crate::materialization_plan::HostNode::from);
        // Deliver the host-derived engine name to the directory-clone
        // cache's shared slot before anything can wait on it, and pick
        // it up for `BuildModules` below.
        let engine_name = match &pending_host_engine_slot {
            Some(slot) => {
                let name =
                    host_node.as_ref().and_then(crate::materialization_plan::engine_name_from_host);
                let _ = slot.set(name.clone());
                name
            }
            None => engine_name,
        };

        let closure_importer_ids: std::collections::HashSet<String> =
            importers.keys().cloned().collect();
        let mut skipped = crate::materialization_plan::compute_skip_set::<Reporter>(
            crate::materialization_plan::SkipSetInputs {
                requester,
                importers,
                snapshots,
                packages,
                installability_host: installability_host.as_ref(),
                seed,
                // The frozen path always installs the groups it was
                // given, so `--no-optional` needs no further
                // qualification here.
                exclude_optional: !include_optional,
                skip_runtimes,
                closure_lockfile: lockfile,
                closure_root: workspace_root,
                closure_importer_ids: &closure_importer_ids,
                included: pnpm_modules_yaml::IncludedDependencies {
                    dependencies: dependency_groups.contains(&DependencyGroup::Prod),
                    dev_dependencies: dependency_groups.contains(&DependencyGroup::Dev),
                    optional_dependencies: include_optional,
                },
            },
        )
        .map_err(InstallFrozenLockfileError::Installability)?;

        // The frozen path runs no resolve-time prefetcher, so the warm
        // batch owns package-status progress for store hits. An empty set
        // leaves every warm package reported as `found_in_store`.
        let progress_reported = SharedReportedProgressKeys::default();

        // Run lockfile verification concurrently with the fetch instead of
        // blocking the install on it: the per-entry registry round trips
        // overlap `CreateVirtualStore`'s downloads. A rejected lockfile
        // aborts the fetch in flight, and a verdict is always reached
        // before linking and the build phase below — no dependency
        // lifecycle script runs on an unverified lockfile. A no-op when
        // `resolution_verifiers` is empty (`trustLockfile`).
        let verify_fut = async {
            if let Some(lockfile_verification_override) = lockfile_verification_override {
                return lockfile_verification_override.await;
            }
            if resolution_verifiers.is_empty() {
                return Ok(());
            }
            verify_lockfile_resolutions::<Reporter>(
                lockfile,
                resolution_verifiers,
                &VerifyLockfileResolutionsOptions {
                    concurrency: None,
                    lockfile_path,
                    cache_dir: Some(&config.cache_dir),
                },
            )
            .await
            .map_err(InstallFrozenLockfileError::LockfileVerification)
        };
        let custom_fetcher_session = load_custom_fetcher_session(pnpmfile_hook).await?;
        let create_virtual_store_fut = async {
            CreateVirtualStore {
                http_client,
                config,
                packages,
                snapshots,
                current_snapshots,
                current_packages,
                layout: &layout,
                logged_methods,
                requester,
                store_index_writer: &store_index_writer,
                store_context: None,
                cas_prefetch: Some(cas_prefetch),
                allow_build_policy: &allow_build_policy,
                skipped: &skipped,
                include_optional_dependencies: include_optional,
                supported_architectures,
                workspace_root,
                node_linker,
                dir_clone_cache: dir_clone_cache.as_ref(),
                progress_reported: &progress_reported,
                tarball_mem_cache,
                custom_fetcher_session: custom_fetcher_session.as_ref(),
                planned_canonical_fetches,
                #[cfg(test)]
                link_concurrency_probe: None,
            }
            .run::<Reporter>()
            .await
            .map_err(InstallFrozenLockfileError::CreateVirtualStore)
        };
        let phase_start = std::time::Instant::now();
        // The verification verdict takes precedence over a concurrent fetch
        // error — a plain `try_join!` would surface whichever error lands
        // first, letting an unrelated fetch failure mask a rejected
        // lockfile. A verification failure still aborts the fetch in
        // flight (the select drops `create_virtual_store_fut`); a fetch
        // failure waits for the verdict and only surfaces once the
        // lockfile is known trusted.
        let CreateVirtualStoreOutput {
            package_manifests,
            side_effects_maps_by_snapshot,
            requires_build_by_snapshot,
            materialized_snapshots,
            fetch_failed,
            cas_paths_by_pkg_id,
        } = {
            let mut verify_fut = std::pin::pin!(verify_fut);
            let mut create_virtual_store_fut = std::pin::pin!(create_virtual_store_fut);
            tokio::select! {
                verify = &mut verify_fut => {
                    verify?;
                    create_virtual_store_fut.await?
                }
                output = &mut create_virtual_store_fut => {
                    verify_fut.await?;
                    output?
                }
            }
        };
        tracing::info!(
            target: "pacquet::install::phase",
            phase = "create_virtual_store",
            elapsed_ms = phase_start.elapsed().as_millis() as u64,
            "phase complete",
        );

        // Fold fetch-failure swallows into the live skip set so
        // downstream consumers (`SymlinkDirectDependencies`,
        // `LinkVirtualStoreBins`, `BuildModules`, the hoist pass)
        // observe the optional fetch-failed snapshots as absent.
        // Tracked in the `fetch_failed` subset of `SkippedSnapshots`
        // which is excluded from `.modules.yaml.skipped` serialization
        // so a subsequent install retries the fetch — the skip set is
        // not updated at the catch site.
        for key in fetch_failed {
            skipped.add_fetch_failed(key);
        }

        // Importer ids backed by the install's own declared projects.
        // These may legitimately live outside the lockfile dir (Bit's
        // capsule installs), so they bypass the malformed-lockfile
        // importer-key rejection.
        let trusted_importer_ids: std::collections::HashSet<String> = project_manifests
            .iter()
            .map(|(project_dir, _)| {
                pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir)
            })
            .collect();
        let root_component_importers: std::collections::HashSet<String> = project_manifests
            .iter()
            .filter(|(_, manifest)| {
                manifest.install_config_hoisting_limits() == Some(crate::HOISTING_LIMITS_WORKSPACES)
            })
            .map(|(project_dir, _)| {
                pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir)
            })
            .collect();
        let sidecar_included = IncludedDependencies {
            dependencies: dependency_groups.contains(&DependencyGroup::Prod),
            dev_dependencies: dependency_groups.contains(&DependencyGroup::Dev),
            optional_dependencies: dependency_groups.contains(&DependencyGroup::Optional),
        };
        let sidecar_lockfile =
            crate::filter_lockfile_for_current(lockfile, sidecar_included, &skipped);

        let phase_start = std::time::Instant::now();
        let crate::linking::LinkPhaseOutput {
            hoisted_dependencies,
            hoisted_locations,
            hoisted_pkg_roots_by_key,
            publicly_hoisted_for_post_build,
        } = crate::linking::run_link_phase::<Reporter>(
            crate::linking::LinkPhaseInputs {
                symlink_root: workspace_root,
                trusted_importer_ids: &trusted_importer_ids,
                root_component_importers: &root_component_importers,
                sidecar_lockfile: &sidecar_lockfile,
                config,
                layout: &layout,
                lockfile,
                current_lockfile,
                snapshots,
                materialized_snapshots: rebuild
                    .is_none()
                    .then_some(materialized_snapshots.as_slice()),
                packages,
                importers,
                project_manifests,
                package_map_project_manifests,
                dependency_groups: &dependency_groups,
                package_manifests: &package_manifests,
                cas_paths_by_pkg_id,
                link_options: &link_options,
                workspace_root,
                requester,
                node_linker,
                is_hoisted,
                prune_orphans,
                prior_hoisted_dependencies,
                host_node: host_node.as_ref(),
                supported_architectures,
                logged_methods,
            },
            &mut skipped,
        )
        .map_err(InstallFrozenLockfileError::LinkPhase)?;
        tracing::info!(
            target: "pacquet::install::phase",
            phase = "link_phase",
            elapsed_ms = phase_start.elapsed().as_millis() as u64,
            "phase complete",
        );

        // `importing_done` fires once extraction and symlink linking
        // are complete, before any build phase. Reporters use it to
        // close the import progress display so subsequent
        // `pnpm:lifecycle` events render in their own section.
        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: requester.to_string(),
            stage: Stage::ImportingDone,
        }));

        // Resolve the deferred `node --version` detection from the
        // GVS-off path, if any. The handle was spawned before
        // `CreateVirtualStore::run` so the `node` startup cost
        // overlapped with install I/O. Falls back to the synchronous
        // value when the spawn was never deferred (GVS on, or host
        // already detected for the installability check).
        let engine_name = match deferred_engine_name {
            Some(deferred) => deferred.handle.await.ok().flatten(),
            None => engine_name,
        };

        let mut build_extra_env = config.extra_env_with_node_options();
        if matches!(node_linker, NodeLinker::Pnp) {
            let node_options = build_extra_env.get("NODE_OPTIONS").map(String::as_str);
            build_extra_env.insert(
                "NODE_OPTIONS".to_string(),
                crate::make_node_require_option(
                    &workspace_root.join(crate::PNP_FILENAME),
                    node_options,
                ),
            );
        }
        if config.node_experimental_package_map && !matches!(node_linker, NodeLinker::Pnp) {
            let package_map_path =
                config.modules_dir.join(crate::package_map::PACKAGE_MAP_FILENAME);
            let node_options = build_extra_env.get("NODE_OPTIONS").map(String::as_str);
            build_extra_env.insert(
                "NODE_OPTIONS".to_string(),
                crate::make_node_package_map_option(&package_map_path, node_options),
            );
        }

        // Run lifecycle scripts, report ignored builds, and re-link
        // top-level bins. `workspace_root` is the `lockfileDir`;
        // pass the real `Path` rather than reconstructing it from the
        // lossy `requester` string so non-UTF-8 filenames survive.
        // `allow_build_policy` was constructed up-front (before
        // `CreateVirtualStore`) so the git fetcher could consult it.
        let phase_start = std::time::Instant::now();
        let crate::BuildModulesOutput { ignored_builds, deferred_builds, mutated_slots: _ } =
            run_build_phase::<Reporter>(&BuildPhaseInputs {
                config,
                workspace_root,
                top_level_bin_root: workspace_root,
                layout: &layout,
                snapshots,
                packages,
                importers,
                dependency_groups: &dependency_groups,
                // Resolved once inside `resolve_snapshot_patches`; the frozen
                // path has no earlier patch resolution to reuse.
                patch_groups: None,
                allow_build_policy: &allow_build_policy,
                side_effects_maps_by_snapshot: &side_effects_maps_by_snapshot,
                requires_build_by_snapshot: &requires_build_by_snapshot,
                materialized_snapshots: &materialized_snapshots,
                engine_name: engine_name.as_deref(),
                extra_env: &build_extra_env,
                store_index_writer: &store_index_writer,
                skipped: &skipped,
                hoisted_pkg_roots_by_key: hoisted_pkg_roots_by_key.as_ref(),
                is_hoisted,
                publicly_hoisted_for_post_build: &publicly_hoisted_for_post_build,
                logged_methods,
                rebuild,
                link_options: &link_options,
            })
            .map_err(InstallFrozenLockfileError::BuildPhase)?;
        tracing::info!(
            target: "pacquet::install::phase",
            phase = "build_phase",
            elapsed_ms = phase_start.elapsed().as_millis() as u64,
            "phase complete",
        );

        // Drop the orchestrator's clone of the writer so the channel
        // closes once every per-snapshot clone has also been dropped
        // and the task starts its final flush and connection close.
        // Nothing after this point reads the index, so the task is
        // handed back as
        // [`InstallFrozenLockfileOutput::store_index_teardown`] and
        // awaited by the install driver after its own tail writes.
        drop(store_index_writer);

        // The injectedDeps payload for `.modules.yaml`: every `file:`
        // snapshot is a materialized copy of an injected workspace
        // project; record the copies per source project so post-install
        // tooling (Bit's build-artifact linker) can reach all of them.
        // Under the hoisted linker the copies live at the walker's
        // hoisted locations rather than in a virtual store.
        let injected_deps = crate::collect_injected_deps(
            &layout,
            workspace_root,
            snapshots,
            packages,
            &skipped,
            is_hoisted.then_some(&hoisted_locations),
        );

        Ok(InstallFrozenLockfileOutput {
            hoisted_dependencies,
            hoisted_locations,
            injected_deps,
            skipped,
            ignored_builds,
            deferred_builds,
            store_index_teardown: writer_task,
        })
    }
}

/// Bundle returned by [`InstallFrozenLockfile::run`] so the caller
/// can drive a single `.modules.yaml` write from one frozen install.
/// Defined as a `struct` rather than a tuple so future fields can
/// land without churning every call site.
#[derive(Debug)]
pub struct InstallFrozenLockfileOutput {
    /// Hoisted-dependencies map produced by the isolated-linker
    /// hoist pass — empty when both hoist patterns are `None` and
    /// always empty under `nodeLinker: hoisted` (the hoisted
    /// linker writes the on-disk tree directly and does not need
    /// the alias-to-`HoistKind` adapter shape).
    pub hoisted_dependencies: HoistedDependencies,
    /// Per-depPath list of lockfile-relative directory paths the
    /// hoisted linker placed each package at. Empty under the
    /// isolated linker — the field is hoisted-only on disk and
    /// only meaningful when `nodeLinker: hoisted`. Round-trips
    /// through [`pnpm_modules_yaml::Modules::hoisted_locations`]
    /// so a follow-up install (or rebuild) can locate every
    /// package without re-running the walker.
    pub hoisted_locations: BTreeMap<String, Vec<String>>,
    /// Per-source-project list of virtual-store package directories
    /// its injected `file:` copies were materialized at. Round-trips
    /// through [`pnpm_modules_yaml::Modules::injected_deps`] —
    /// see [`crate::collect_injected_deps`].
    pub injected_deps: BTreeMap<String, Vec<String>>,
    /// Install-time skip set produced by `compute_skipped_snapshots`,
    /// seeded from the previous install's `.modules.yaml.skipped`
    /// and augmented with snapshots that newly failed the
    /// installability check.
    pub skipped: SkippedSnapshots,
    /// Sorted `name@version` keys whose build scripts were blocked by
    /// the `allowBuilds` policy. The caller raises
    /// `ERR_PNPM_IGNORED_BUILDS` from this list when `strictDepBuilds`
    /// is on (the default).
    pub ignored_builds: Vec<String>,
    /// Dep paths whose build `--ignore-scripts` deferred — see
    /// [`crate::BuildModulesOutput::deferred_builds`]. The caller folds
    /// them into `.modules.yaml.pendingBuilds`.
    pub deferred_builds: Vec<String>,
    /// The store-index writer task, already winding down: every writer
    /// handle was dropped before this output was built. Await it via
    /// [`StoreIndexWriter::drain`] after any tail writes it can
    /// overlap with; dropping it instead (error paths) detaches the
    /// teardown, which is safe. The full rationale lives at the await
    /// site in the install driver.
    pub store_index_teardown: tokio::task::JoinHandle<Result<(), StoreIndexError>>,
}

impl From<HoistedLinkerError> for InstallFrozenLockfileError {
    fn from(error: HoistedLinkerError) -> Self {
        match error {
            HoistedLinkerError::HoistedDepGraph(error) => {
                InstallFrozenLockfileError::HoistedDepGraph(error)
            }
            HoistedLinkerError::LinkHoistedModules(error) => {
                InstallFrozenLockfileError::LinkHoistedModules(error)
            }
            HoistedLinkerError::SymlinkDirectDependencies(error) => {
                InstallFrozenLockfileError::SymlinkDirectDependencies(error)
            }
            HoistedLinkerError::WritePackageMap(error) => {
                InstallFrozenLockfileError::WritePackageMap(error)
            }
        }
    }
}

/// Load custom fetchers from the install's pnpmfiles, if any.
/// Returns `Ok(None)` when no pnpmfile exists or it exports no
/// fetchers, so the install path can skip the IPC overhead entirely.
/// A pnpmfile that fails to load or evaluate aborts the install, like
/// the custom-resolver load on the fresh-lockfile path.
async fn load_custom_fetcher_session(
    hook: Option<&Arc<dyn pnpm_hooks::PnpmfileHooks>>,
) -> Result<Option<Arc<crate::CustomFetcherSession>>, InstallFrozenLockfileError> {
    let Some(hook) = hook else { return Ok(None) };
    let fetchers = hook.get_custom_fetchers().await.map_err(|err| {
        tracing::error!(
            target: "pacquet::install",
            "Failed to get custom fetchers from pnpmfile: {err}",
        );
        InstallFrozenLockfileError::CustomFetcherHook(err)
    })?;
    if fetchers.is_empty() {
        return Ok(None);
    }
    Ok(Some(Arc::new(crate::CustomFetcherSession::new(fetchers))))
}

#[cfg(test)]
mod tests;
