pub use build_phase::{
    BuildPhaseError, BuildPhaseInputs, resolve_snapshot_patches, run_build_phase,
};
pub use hoisted::{
    HoistPlan, HoistedLinkerError, HoistedLinkerInputs, HoistedLinkerOutput,
    collect_public_hoist_targets, compute_hoist_plan, find_own_runtime_node_major,
    find_runtime_node_major, parse_major_from_version, run_hoisted_linker,
    workspace_packages_for_hoist,
};

mod verification;
use verification::{ConcurrentVerification, fetch_verified, load_custom_fetcher_session};

mod planning;
use planning::{
    BuildInputs, FetchInputs, FrozenInputs, HostDetectionInputs, HostPlan, LinkInputs,
    MaterializationPlan, OwnedInputs, SkipSetPlan, detect_host, needs_installability_check,
    plan_engine_name, seed_skip_set, settle_engine_name,
};

mod materialization;

use crate::{
    AllowBuildPolicy, BuildModules, BuildModulesError, CreateVirtualStoreError,
    CreateVirtualStoreOutput, HoistedDepGraphError, HoistedDependencies, LinkHoistedModulesError,
    LinkHoistedModulesOpts, LinkRootComponentMembersError, LinkVirtualStoreBinsError,
    LockfileToHoistedDepGraphOptions, SkippedSnapshots, SymlinkDirectDependencies,
    SymlinkDirectDependenciesError, SymlinkPackageError, VersionPolicyError, VirtualStoreLayout,
    build_direct_deps_by_importer, direct_dep_names_for_importer, get_hoisted_dependencies,
    link_hoisted_modules, link_top_level_bins, lockfile_to_hoisted_dep_graph,
    symlink_direct_dependencies::importer_root_dir,
};

mod build_phase;
mod hoisted;

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_cmd_shim::{LinkBinsError, LinkBinsOptions};
use pnpm_config::{Config, NodeLinker};
use pnpm_lockfile::{
    Lockfile, LockfileEntries, PackageKey, PackageMetadata, Prefix, ProjectSnapshot, SnapshotEntry,
};
use pnpm_lockfile_verification::VerifyError;
use pnpm_matcher::create_matcher;
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::DependencyGroup;
use pnpm_patching::{
    ExtendedPatchInfo, PatchKeyConflictError, ResolvePatchedDependenciesError, get_patch_info,
};
use pnpm_reporter::{IgnoredScriptsLog, LogEvent, LogLevel, Reporter, Stage, StageLog};
use pnpm_resolving_resolver_base::ResolutionVerifier;
use pnpm_store_dir::{StoreIndexError, StoreIndexWriter};
use pnpm_tarball::MemCache;
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
pub struct InstallFrozenLockfile<'a> {
    pub http_client: &'a ThrottledClient,
    pub config: &'static Config,
    pub pnpmfile_hook: Option<&'a Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    /// The fully-deserialized wanted lockfile. Its `importers`,
    /// `packages` and `snapshots` are read straight off it, so a caller
    /// cannot pair one lockfile's entries with another's maps.
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
    /// Entries from the previous install's `lock.yaml`, threaded through
    /// to [`crate::CreateVirtualStore`] to drive the per-snapshot skip
    /// decision. See [`LockfileEntries::of_previous_install`], which is
    /// how a caller builds this: it is empty on a first install and
    /// under `--force`.
    pub current_entries: LockfileEntries<'a>,
    pub dependency_groups: &'a [DependencyGroup],
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
    /// `hoistedLocations` recorded by the previous install's
    /// `.modules.yaml`, for the hoisted linker's already-in-place check.
    /// `None` on a first install or when the file couldn't be fully
    /// parsed.
    pub prior_hoisted_locations: Option<&'a crate::HoistedLocations>,
    /// `allowBuilds` changed since the previous install: a build it
    /// ignored may now be allowed, or one it ran may no longer be. The
    /// hoisted linker then hands every package to the build phase, present
    /// or not. See [`crate::HoistedLinkerInputs::build_present_packages`].
    pub allow_builds_changed: bool,
    /// See [`crate::HoistedLinkerInputs::prior_unbuilt_builds`].
    pub prior_unbuilt_builds: &'a crate::UnbuiltBuilds,
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

/// The environment the build phase's lifecycle scripts run under. The
/// `PnP` and package-map linkers each prepend their own loader to
/// `NODE_OPTIONS`.
fn build_extra_env(
    config: &pnpm_config::Config,
    node_linker: NodeLinker,
    workspace_root: &std::path::Path,
) -> HashMap<String, String> {
    let mut extra_env = config.extra_env_with_node_options();
    if matches!(node_linker, NodeLinker::Pnp) {
        let node_options = extra_env.get("NODE_OPTIONS").map(String::as_str);
        extra_env.insert(
            "NODE_OPTIONS".to_string(),
            crate::make_node_require_option(
                &workspace_root.join(crate::PNP_FILENAME),
                node_options,
            ),
        );
    }
    if config.node_experimental_package_map && !matches!(node_linker, NodeLinker::Pnp) {
        let package_map_path = config.modules_dir.join(crate::package_map::PACKAGE_MAP_FILENAME);
        let node_options = extra_env.get("NODE_OPTIONS").map(String::as_str);
        extra_env.insert(
            "NODE_OPTIONS".to_string(),
            crate::make_node_package_map_option(&package_map_path, node_options),
        );
    }
    extra_env
}

#[cfg(test)]
mod tests;

impl<'a> InstallFrozenLockfile<'a> {
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
        mut self,
    ) -> Result<InstallFrozenLockfileOutput, InstallFrozenLockfileError> {
        let owned = self.take_owned();
        // Built up front so it can flow into the cold-batch git fetcher
        // in `CreateVirtualStore` as well as the postinstall phase in
        // `BuildModules`; the directory-clone cache borrows it, which is
        // why it lives here rather than in the plan.
        let allow_build_policy = AllowBuildPolicy::from_config(self.config)
            .map_err(InstallFrozenLockfileError::VersionPolicy)?;
        let plan = self
            .plan_materialization(
                &allow_build_policy,
                owned.early_host_detection,
                owned.node_version,
            )
            .await?;

        self.run_plan::<Reporter>(
            &allow_build_policy,
            plan,
            owned.seed_skipped,
            owned.verification_override,
        )
        .await
    }

    async fn run_plan<Reporter: self::Reporter>(
        self,
        allow_build_policy: &AllowBuildPolicy,
        plan: MaterializationPlan<'_>,
        seed_skipped: Option<Vec<String>>,
        verification_override: Option<LockfileVerificationOverride<'_>>,
    ) -> Result<InstallFrozenLockfileOutput, InstallFrozenLockfileError> {
        let ctx = crate::InstallContext {
            config: self.config,
            workspace_root: self.workspace_root,
            requester: self.requester,
            layout: &plan.layout,
            node_linker: self.node_linker,
            allow_build_policy,
            link_options: &plan.link_options,
            logged_methods: self.logged_methods,
            git_source_cache: &plan.git_source_cache,
        };

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
            StoreIndexWriter::spawn_for(&ctx.config.store_dir, ctx.config.frozen_store);

        let settled = self.settle_skip_set::<Reporter>(plan.host, seed_skipped).await?;

        let fetched = self
            .fetch::<Reporter>(
                &ctx,
                FetchInputs {
                    cas_prefetch: plan.cas_prefetch,
                    dir_clone_cache: plan.dir_clone_cache.as_ref(),
                    store_index_writer: &store_index_writer,
                    skipped: &settled.skipped,
                    verification_override,
                },
            )
            .await?;

        self.finish_materialization::<Reporter>(
            &ctx,
            fetched,
            settled,
            plan.deferred_engine_name,
            store_index_writer,
            writer_task,
        )
        .await
    }

    /// Optional fetch failures are absent from linking and builds, but remain retryable on later installs.
    async fn finish_materialization<Reporter: self::Reporter>(
        self,
        ctx: &crate::InstallContext<'_>,
        mut fetched: CreateVirtualStoreOutput,
        mut settled: SkipSetPlan,
        deferred_engine_name: Option<crate::materialization_plan::DeferredEngineName>,
        store_index_writer: Arc<StoreIndexWriter>,
        writer_task: tokio::task::JoinHandle<Result<(), StoreIndexError>>,
    ) -> Result<InstallFrozenLockfileOutput, InstallFrozenLockfileError> {
        settled.skipped.add_fetch_failed_all(fetched.fetch_failed.drain());

        let linked = self.link_fetched::<Reporter>(ctx, &mut fetched, &mut settled)?;

        let phase_start = std::time::Instant::now();
        let built = self
            .build::<Reporter>(
                ctx,
                BuildInputs {
                    fetched: &fetched,
                    linked: &linked,
                    skipped: &settled.skipped,
                    store_index_writer: &store_index_writer,
                    engine_name: settled.engine_name,
                    deferred_engine_name,
                },
            )
            .await?;
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
        Ok(InstallFrozenLockfileOutput {
            injected_deps: crate::collect_injected_deps(
                ctx.layout,
                ctx.workspace_root,
                LockfileEntries::from(self.lockfile),
                &settled.skipped,
                ctx.is_hoisted().then_some(&linked.hoisted_locations),
            ),
            hoisted_dependencies: linked.hoisted_dependencies,
            hoisted_locations: linked.hoisted_locations,
            skipped: settled.skipped,
            ignored_builds: built.ignored_builds,
            deferred_builds: built.deferred_builds,
            store_index_teardown: writer_task,
        })
    }

    fn link_fetched<Reporter: self::Reporter>(
        &self,
        ctx: &crate::InstallContext<'_>,
        fetched: &mut CreateVirtualStoreOutput,
        settled: &mut SkipSetPlan,
    ) -> Result<crate::linking::LinkPhaseOutput, InstallFrozenLockfileError> {
        let cas_paths_by_pkg_id = fetched.cas_paths_by_pkg_id.take();
        let phase_start = std::time::Instant::now();
        let linked = self.link::<Reporter>(
            ctx,
            LinkInputs { fetched, cas_paths_by_pkg_id, host_node: settled.host_node.as_ref() },
            &mut settled.skipped,
        )?;
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
            prefix: ctx.requester.to_string(),
            stage: Stage::ImportingDone,
        }));

        Ok(linked)
    }

    /// The borrowed inputs as one `Copy` value. See [`FrozenInputs`].
    fn inputs(&self) -> FrozenInputs<'a> {
        FrozenInputs {
            http_client: self.http_client,
            config: self.config,
            pnpmfile_hook: self.pnpmfile_hook,
            lockfile: self.lockfile,
            resolution_verifiers: self.resolution_verifiers,
            lockfile_path: self.lockfile_path,
            current_lockfile: self.current_lockfile,
            current_entries: self.current_entries,
            dependency_groups: self.dependency_groups,
            project_manifests: self.project_manifests,
            package_map_project_manifests: self.package_map_project_manifests,
            workspace_root: self.workspace_root,
            requester: self.requester,
            supported_architectures: self.supported_architectures,
            skip_runtimes: self.skip_runtimes,
            node_linker: self.node_linker,
            tarball_mem_cache: self.tarball_mem_cache,
            rebuild: self.rebuild,
            prior_hoisted_dependencies: self.prior_hoisted_dependencies,
            prior_hoisted_locations: self.prior_hoisted_locations,
            allow_builds_changed: self.allow_builds_changed,
            prior_unbuilt_builds: self.prior_unbuilt_builds,
            prune_orphans: self.prune_orphans,
            planned_canonical_fetches: self.planned_canonical_fetches,
        }
    }

    /// Move the inputs `run` consumes out of `self`, so the phases can
    /// borrow the rest of it whole.
    fn take_owned(&mut self) -> OwnedInputs<'a> {
        OwnedInputs {
            early_host_detection: self.early_host_detection.take(),
            node_version: self.node_version.take(),
            seed_skipped: self.seed_skipped.take(),
            verification_override: self.lockfile_verification_override.take(),
        }
    }
}
