use crate::{
    AllowBuildPolicy, BuildModules, BuildModulesError, CreateVirtualStore, CreateVirtualStoreError,
    CreateVirtualStoreOutput, HoistedDepGraphError, HoistedDependencies, InstallabilityHost,
    LinkHoistedModulesError, LinkHoistedModulesOpts, LinkRootComponentMembersError,
    LinkVirtualStoreBins, LinkVirtualStoreBinsError, LockfileToHoistedDepGraphOptions,
    MaterializeGlobalVirtualStoreContextError, SkippedSnapshots, SymlinkDirectDependencies,
    SymlinkDirectDependenciesError, SymlinkPackageError, VersionPolicyError, VirtualStoreLayout,
    any_installability_constraint, build_direct_deps_by_importer, build_hoist_graph,
    compute_skipped_snapshots, direct_dep_names_for_importer, get_hoisted_dependencies,
    link_direct_dep_bins_resolved, link_hoisted_modules, link_root_component_members,
    link_top_level_bins, lockfile_to_hoisted_dep_graph, materialize_global_virtual_store_context,
    symlink_direct_dependencies::importer_root_dir, symlink_hoisted_dependencies,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_cmd_shim::LinkBinsError;
use pacquet_config::{Config, NodeLinker, matcher::create_matcher};
use pacquet_executor::ScriptsPrependNodePath as ExecScriptsPrependNodePath;
use pacquet_lockfile::{
    Lockfile, PackageKey, PackageMetadata, Prefix, ProjectSnapshot, SnapshotEntry,
};
use pacquet_lockfile_verification::{
    VerifyError, VerifyLockfileResolutionsOptions, verify_lockfile_resolutions,
};
use pacquet_modules_yaml::{Host, IncludedDependencies, read_modules_manifest};
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::DependencyGroup;
use pacquet_patching::{
    ExtendedPatchInfo, PatchKeyConflictError, ResolvePatchedDependenciesError, get_patch_info,
};
use pacquet_reporter::{
    IgnoredScriptsLog, LogEvent, LogLevel, Reporter, Stage, StageLog, StatsLog, StatsMessage,
};
use pacquet_resolving_resolver_base::ResolutionVerifier;
use pacquet_store_dir::StoreIndexWriter;
use pacquet_tarball::{MemCache, SharedReportedProgressKeys};
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
    pub importers: &'a HashMap<String, ProjectSnapshot>,
    pub packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    pub snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    /// The fully-deserialized wanted lockfile. Carried alongside
    /// the destructured `importers` / `packages` / `snapshots`
    /// references because the hoisted-linker walker
    /// ([`crate::lockfile_to_hoisted_dep_graph`]) takes a
    /// `&Lockfile` (it threads the lockfile into
    /// [`pacquet_real_hoist::hoist`] which needs every importer's
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
    pub project_manifests: &'a [(PathBuf, &'a pacquet_package_manifest::PackageManifest)],
    pub package_map_project_manifests:
        &'a [(PathBuf, &'a pacquet_package_manifest::PackageManifest)],
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
    /// overrides. Threaded into [`InstallabilityHost`] so the
    /// platform-tagged optional-dependency filter respects user-
    /// supplied architecture overrides.
    pub supported_architectures: Option<&'a pacquet_package_is_installable::SupportedArchitectures>,

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

    /// `nodeLinker` value to honor for *this* invocation. Threaded
    /// from the [`crate::Install`] caller (which has already
    /// applied any `--node-linker` CLI override on top of
    /// [`pacquet_config::Config::node_linker`]).
    ///
    /// Under [`NodeLinker::Hoisted`] the install pipeline routes
    /// through [`crate::lockfile_to_hoisted_dep_graph`] +
    /// [`crate::link_hoisted_modules()`] instead of the isolated
    /// linker's [`crate::SymlinkDirectDependencies`] +
    /// [`crate::LinkVirtualStoreBins`] + [`crate::get_hoisted_dependencies`]
    /// chain, matching the `nodeLinker === 'hoisted'` branch in
    /// `headlessInstall`.
    ///
    /// Pacquet's [`NodeLinker::Pnp`] is a config / serde
    /// placeholder today; an install request with `Pnp` reaches
    /// the isolated linker in this branch (no `PnP` code path
    /// exists yet). `nodeLinker: 'pnp'` is out-of-scope and tracked
    /// separately.
    pub node_linker: NodeLinker,

    /// Install-scoped shared in-flight tarball cache, threaded down to
    /// [`crate::CreateVirtualStore`]'s cold-batch downloads. `Some` on
    /// the pnpr client path so the materialization reuses the
    /// [`crate::TarballPrefetcher`]'s background downloads instead of
    /// re-fetching every tarball; `None` for installs without a shared
    /// prefetch in flight.
    pub tarball_mem_cache: Option<&'a Arc<MemCache>>,
    pub seed_skipped: Option<Vec<String>>,
    /// Forced-rebuild selection threaded from `pacquet rebuild` /
    /// `approve-builds`; `None` for a normal install. Forwarded to
    /// `run_build_phase`'s `BuildPhaseInputs`. See
    /// [`crate::RebuildOptions`].
    pub rebuild: Option<&'a crate::RebuildOptions>,
    /// `hoistedDependencies` recorded by the previous install's
    /// `.modules.yaml`, for [`crate::PruneStaleModules`]'s orphan
    /// hoist-link cleanup. `None` on a first install or when the file
    /// couldn't be fully parsed.
    pub prior_hoisted_dependencies: Option<&'a crate::HoistedDependencies>,
    /// See [`crate::PruneStaleModules::prune_orphans`].
    pub prune_orphans: bool,
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
    CustomFetcherHook(#[error(not(source))] pacquet_hooks::HookError),

    #[diagnostic(transparent)]
    SymlinkDirectDependencies(#[error(source)] SymlinkDirectDependenciesError),

    #[diagnostic(transparent)]
    MaterializeGlobalVirtualStoreContext(
        #[error(source)] MaterializeGlobalVirtualStoreContextError,
    ),

    /// Surfaces a failure while removing stale direct-dep or hoist
    /// links during the pre-link reconciliation pass.
    #[diagnostic(transparent)]
    PruneStaleModules(#[error(source)] crate::PruneDirectDepsError),

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
    /// the fresh-lockfile path via `run_build_phase`, so both install
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
    Installability(#[error(source)] Box<pacquet_package_is_installable::InstallabilityError>),

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

    #[diagnostic(transparent)]
    InstallError(#[error(source)] Box<crate::InstallError>),
}

/// Error type of `run_build_phase` and `resolve_snapshot_patches`.
///
/// Each variant is `#[diagnostic(transparent)]` so the surfaced
/// `ERR_PNPM_*` code comes from the wrapped error — the two install
/// paths embed this in their own error enums (also transparently), so
/// a failed build reports identically regardless of which path ran it.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum BuildPhaseError {
    /// `patchedDependencies` couldn't be resolved from
    /// `pnpm-workspace.yaml`.
    #[diagnostic(transparent)]
    ResolvePatchedDependencies(#[error(source)] ResolvePatchedDependenciesError),

    /// Surfaces `ERR_PNPM_PATCH_KEY_CONFLICT` when more
    /// than one configured version range matches a snapshot. Refuses
    /// to silently pick one — the user must add an exact-version
    /// entry to disambiguate.
    #[diagnostic(transparent)]
    PatchKeyConflict(#[error(source)] PatchKeyConflictError),

    /// A lifecycle script (`preinstall` / `install` / `postinstall`)
    /// failed, or the build phase hit an I/O / frozen-store error.
    #[diagnostic(transparent)]
    BuildModules(#[error(source)] BuildModulesError),

    /// Surfaces a failure from the post-`BuildModules` per-importer
    /// top-level bin link. This pass mixes direct + publicly-hoisted
    /// candidates so `pacquet_cmd_shim::pick_winner` (private)'s
    /// [`pacquet_cmd_shim::BinOrigin::Direct`] tier resolves
    /// conflicts in a single call (pnpm/pacquet#342). The failure
    /// surface is the project-tree top-level
    /// `<importer>/node_modules/.bin`.
    #[diagnostic(transparent)]
    TopLevelBinLink(#[error(source)] LinkBinsError),
}

/// Resolve `pnpm-workspace.yaml`'s `patchedDependencies` into a
/// per-snapshot map keyed by the peer-stripped [`PackageKey`].
///
/// Yields `None` when nothing is configured (no yaml, no key, or empty
/// map) or when there are no snapshots; an empty map when patches exist
/// but match nothing in the current install. Computed from pacquet's
/// lockfile-driven flow: the patch hashes are resolved after the
/// lockfile is built/loaded rather than during resolution.
pub(crate) fn resolve_snapshot_patches(
    config: &Config,
    pre_resolved: Option<&pacquet_patching::PatchGroupRecord>,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
) -> Result<Option<HashMap<PackageKey, ExtendedPatchInfo>>, BuildPhaseError> {
    // Reuse the caller's grouped record when it already resolved it (the
    // fresh-lockfile path builds it to feed the resolver), so the patch
    // files aren't re-hashed; otherwise resolve it once here (frozen path).
    let resolved_owned = match pre_resolved {
        Some(_) => None,
        None => config
            .resolved_patched_dependencies()
            .map_err(BuildPhaseError::ResolvePatchedDependencies)?,
    };
    let patch_groups = pre_resolved.or(resolved_owned.as_ref());
    let patches = match (patch_groups, snapshots) {
        (Some(groups), Some(snaps)) => {
            let mut map = HashMap::new();
            for key in snaps.keys() {
                let metadata_key = key.without_peer();
                let metadata_key_str = metadata_key.to_string();
                let (name, version) =
                    crate::build_modules::parse_name_version_from_key(&metadata_key_str);
                // Propagate `ERR_PNPM_PATCH_KEY_CONFLICT` rather than
                // silently skipping the snapshot. Failing here makes the
                // user add an exact-version entry to disambiguate.
                if let Some(info) = get_patch_info(Some(groups), &name, &version)
                    .map_err(BuildPhaseError::PatchKeyConflict)?
                {
                    map.insert(metadata_key, info.clone());
                }
            }
            Some(map)
        }
        _ => None,
    };
    Ok(patches)
}

/// Inputs to [`run_build_phase`]. Bundled so both install paths
/// ([`InstallFrozenLockfile::run`] and the fresh-lockfile path) can
/// drive the shared lifecycle-script + post-build top-level bin link
/// without a long positional argument list.
pub(crate) struct BuildPhaseInputs<'a> {
    pub(crate) config: &'static Config,
    /// `lockfileDir` — the project root. Threaded to
    /// `BuildModules` as `lockfile_dir`, where it sets each script's
    /// `INIT_CWD` and the lifecycle log prefix.
    pub(crate) workspace_root: &'a Path,
    /// Directory each importer's `node_modules/.bin` is anchored under
    /// in the post-build top-level bin pass. Equals `workspace_root`
    /// in production (and on the frozen path); the fresh path passes
    /// its `symlink_root` (`config.modules_dir.parent()`), which can
    /// differ when a test relocates `modules_dir`.
    pub(crate) top_level_bin_root: &'a Path,
    pub(crate) layout: &'a VirtualStoreLayout,
    pub(crate) snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    pub(crate) packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    pub(crate) importers: &'a HashMap<String, ProjectSnapshot>,
    pub(crate) dependency_groups: &'a [DependencyGroup],
    /// `patchedDependencies` already resolved + grouped by the caller, so
    /// the build phase doesn't re-hash the patch files. `None` on the
    /// frozen path, which resolves it inside [`resolve_snapshot_patches`].
    pub(crate) patch_groups: Option<&'a pacquet_patching::PatchGroupRecord>,
    pub(crate) allow_build_policy: &'a AllowBuildPolicy,
    pub(crate) side_effects_maps_by_snapshot: &'a crate::SideEffectsMapsBySnapshot,
    pub(crate) requires_build_by_snapshot: &'a crate::RequiresBuildBySnapshot,
    pub(crate) engine_name: Option<&'a str>,
    pub(crate) extra_env: &'a HashMap<String, String>,
    pub(crate) store_index_writer: &'a Arc<StoreIndexWriter>,
    pub(crate) skipped: &'a SkippedSnapshots,
    pub(crate) hoisted_pkg_roots_by_key: Option<&'a HashMap<PackageKey, Vec<PathBuf>>>,
    pub(crate) is_hoisted: bool,
    /// Publicly-hoisted aliases (with bins) competing for the root
    /// importer's `node_modules/.bin`. Empty under the hoisted linker
    /// and when no public-hoist pattern is set.
    pub(crate) publicly_hoisted_for_post_build: &'a [String],
    pub(crate) logged_methods: &'a AtomicU8,
    /// Forced-rebuild selection threaded from `pacquet rebuild` /
    /// `approve-builds`; `None` for a normal install. See
    /// [`crate::RebuildOptions`].
    pub(crate) rebuild: Option<&'a crate::RebuildOptions>,
    /// [`crate::shim_extra_node_paths`] output, for the post-build
    /// top-level bin pass.
    pub(crate) extra_node_paths: &'a [String],
}

/// Run dependency lifecycle scripts, report ignored builds, and
/// re-link top-level bins — the shared tail both install paths run
/// after the virtual store is materialized.
///
/// Runs a single `buildModules` + `pnpm:ignored-scripts` emit +
/// `linkBinsOfImporter` sequence. Always emits the `IgnoredScripts`
/// event (with an empty list when nothing was ignored) so the reporter
/// renders a consistent state.
pub(crate) fn run_build_phase<Reporter: self::Reporter>(
    inputs: &BuildPhaseInputs,
) -> Result<crate::BuildModulesOutput, BuildPhaseError> {
    // Every field is a `Copy` reference / scalar, so destructuring
    // through the shared borrow copies them out without a move.
    let &BuildPhaseInputs {
        config,
        workspace_root,
        top_level_bin_root,
        layout,
        snapshots,
        packages,
        importers,
        dependency_groups,
        patch_groups,
        allow_build_policy,
        side_effects_maps_by_snapshot,
        requires_build_by_snapshot,
        engine_name,
        extra_env,
        store_index_writer,
        skipped,
        hoisted_pkg_roots_by_key,
        is_hoisted,
        publicly_hoisted_for_post_build,
        logged_methods,
        rebuild,
        extra_node_paths,
    } = inputs;

    let patches = resolve_snapshot_patches(config, patch_groups, snapshots)?;

    // Convert `pacquet-config`'s mirror enum to the executor's
    // canonical type. Config's enum carries the yaml-deserialize impl;
    // the executor's stays free of serde wiring.
    let scripts_prepend_node_path = match config.scripts_prepend_node_path {
        pacquet_config::ScriptsPrependNodePath::Always => ExecScriptsPrependNodePath::Always,
        pacquet_config::ScriptsPrependNodePath::Never => ExecScriptsPrependNodePath::Never,
        pacquet_config::ScriptsPrependNodePath::WarnOnly => ExecScriptsPrependNodePath::WarnOnly,
    };

    // BuildModules walks per-snapshot package directories and runs
    // `preinstall` / `install` / `postinstall` lifecycle scripts.
    // Under isolated, the directories live under the virtual-store slot
    // layout; under hoisted, they live at the project-tree paths the
    // walker assigned — threaded in via `pkg_roots_by_key`.
    let build_output = BuildModules {
        layout,
        modules_dir: &config.modules_dir,
        lockfile_dir: workspace_root,
        snapshots,
        packages,
        importers,
        allow_build_policy,
        side_effects_maps_by_snapshot: Some(side_effects_maps_by_snapshot),
        requires_build_by_snapshot: Some(requires_build_by_snapshot),
        engine_name,
        side_effects_cache: config.side_effects_cache_read(),
        side_effects_cache_write: config.side_effects_cache_write(),
        store_dir: Some(&config.store_dir),
        store_index_writer: Some(store_index_writer),
        patches: patches.as_ref(),
        scripts_prepend_node_path,
        extra_env,
        unsafe_perm: config.unsafe_perm,
        child_concurrency: config.child_concurrency,
        skipped,
        pkg_roots_by_key: hoisted_pkg_roots_by_key,
        gather_ancestor_bin_paths: is_hoisted,
        frozen_store: config.frozen_store,
        ignore_scripts: config.ignore_scripts,
        import_method: config.package_import_method,
        logged_methods,
        rebuild,
    }
    .run::<Reporter>()
    .map_err(BuildPhaseError::BuildModules)?;

    // Always emit the `pnpm:ignored-scripts` event with the package
    // names, unconditionally, so structured / NDJSON consumers always
    // see the list. The event
    // carries `strict_dep_builds` (the final, post-`updateConfig` value
    // the strict-failure check also reads) so the default reporter can
    // suppress the rendered warning box under strict mode — where the
    // install fails with `ERR_PNPM_IGNORED_BUILDS` and the box would only
    // duplicate the error — without a stale reporter-side flag. The
    // display is gated on `!strictDepBuilds`; the strict path throws.
    Reporter::emit(&LogEvent::IgnoredScripts(IgnoredScriptsLog {
        level: LogLevel::Debug,
        package_names: build_output.ignored_builds.clone(),
        strict_dep_builds: config.strict_dep_builds,
    }));

    // `virtual_store_only` links no bins at all, so there is nothing for
    // the pass below to re-resolve. Dependency *build* scripts still ran
    // above — only the linking stops, matching `pnpm fetch`.
    if config.virtual_store_only {
        return Ok(build_output);
    }

    // Post-`BuildModules` per-importer top-level bin link
    // (pnpm/pacquet#342). Resolves direct-over-hoisted precedence and
    // shims lifecycle-script-created bins that didn't exist at extract
    // time. Idempotent for unchanged shims. Runs after `buildModules`.
    let modules_dir_basename: &OsStr =
        config.modules_dir.file_name().unwrap_or_else(|| OsStr::new("node_modules"));
    for (importer_id, importer_snapshot) in importers {
        let project_dir = importer_root_dir(top_level_bin_root, importer_id);
        let modules_dir = project_dir.join(modules_dir_basename);
        // Same filter the symlink phase used so the post-build pass sees
        // the same candidate set (skipping installability-skipped deps
        // avoids dangling shims at a slot that was never extracted).
        let direct_names = direct_dep_names_for_importer(
            importer_snapshot,
            dependency_groups.iter().copied(),
            skipped,
            false,
        );
        // Public-hoist promotes transitives into the workspace root's
        // `<root>/node_modules/<alias>`, so only the root importer's
        // `.bin` sees `BinOrigin::Hoisted` candidates.
        let hoisted_names: &[String] = if importer_id == Lockfile::ROOT_IMPORTER_KEY {
            publicly_hoisted_for_post_build
        } else {
            &[]
        };
        link_top_level_bins(&modules_dir, &direct_names, hoisted_names, extra_node_paths)
            .map_err(BuildPhaseError::TopLevelBinLink)?;
    }

    Ok(build_output)
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
            node_linker,
            tarball_mem_cache,
            seed_skipped,
            rebuild,
            prior_hoisted_dependencies,
            prune_orphans,
        } = self;

        let is_hoisted = matches!(node_linker, NodeLinker::Hoisted);
        let extra_node_paths = crate::shim_extra_node_paths(config, node_linker);
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
        let (store_index_writer, writer_task) = if config.frozen_store {
            StoreIndexWriter::spawn_disabled()
        } else {
            StoreIndexWriter::spawn(&config.store_dir)
        };

        // Caller-side fast-path for the installability check. The
        // common case (no lockfile metadata row declares an
        // `engines` / `cpu` / `os` / `libc` constraint) lets us skip
        // both [`InstallabilityHost::detect`] and
        // [`compute_skipped_snapshots`] entirely. Spawning
        // `node --version` here would otherwise serialize the
        // node-binary startup with `CreateVirtualStore::run` (the
        // dominant cost of a cold install), giving up the overlap.
        //
        // When constraints DO exist, the host is needed before
        // extraction (so `CreateVirtualStore` can suppress slots for
        // skipped snapshots), and the spawn cost is unavoidable.
        // `--force` bypasses the check outright: every snapshot in the
        // lockfile is materialized regardless of platform / engine
        // constraints, mirroring pnpm's `!opts.force &&
        // packageIsInstallable(...)` gate.
        let needs_installability_check = !config.force
            && match (snapshots, packages) {
                (Some(snaps), Some(pkgs)) if !snaps.is_empty() => {
                    any_installability_constraint(snaps, pkgs)
                }
                _ => false,
            };

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

        // Build the per-install [`SkippedSnapshots`] set. For every
        // lockfile snapshot, run the installability check against
        // the host triple; optional+incompatible entries land in
        // the set and fire `pnpm:skipped-optional-dependency`.
        //
        // `host` is built only when needed. The detection path runs
        // `node --version` on the blocking pool so it doesn't stall
        // the reactor thread.
        let (mut skipped, host_node) = if needs_installability_check {
            let engine_strict = config.engine_strict;
            let mut host = match node_version {
                // An explicit `nodeVersion` needs no `node --version` probe, so
                // build the host directly off the reactor thread.
                node_version @ Some(_) => {
                    InstallabilityHost::detect_with(engine_strict, node_version)
                }
                None => tokio::task::spawn_blocking(move || {
                    InstallabilityHost::detect_with(engine_strict, None)
                })
                .await
                .unwrap_or_else(|_| InstallabilityHost {
                    node_version: "99999.0.0".to_string(),
                    node_detected: false,
                    os: pacquet_graph_hasher::host_platform(),
                    cpu: pacquet_graph_hasher::host_arch(),
                    libc: pacquet_graph_hasher::host_libc(),
                    supported_architectures: None,
                    engine_strict,
                }),
            };
            // Plant the CLI-merged `supportedArchitectures` (yaml +
            // `--cpu`/`--os`/`--libc`) onto the host context so
            // `check_platform`'s `dedupe_current` substitution picks
            // up user-supplied OS/CPU/libc accept lists instead of
            // only the host triple. Clone is cheap (three short
            // `Option<Vec<String>>`).
            if let Some(supp) = supported_architectures {
                host.supported_architectures = Some(supp.clone());
            }
            let skipped = compute_skipped_snapshots::<Reporter>(
                importers,
                snapshots.expect("guarded by needs_installability_check"),
                packages.expect("guarded by needs_installability_check"),
                &host,
                requester,
                seed,
            )
            .map_err(InstallFrozenLockfileError::Installability)?;
            // Preserve `node_detected` + `node_version` for the
            // engine-name derivation below. Dropping the rest of the
            // host struct frees the allocations early.
            (skipped, Some((host.node_detected, host.node_version)))
        } else {
            // Constraint-free lockfile: keep the seed verbatim so a
            // snapshot recorded as skipped on the previous install
            // survives the constraint having been removed from the
            // lockfile.
            (seed, None)
        };

        // `--no-optional` enforcement (umbrella slice 5).
        // When `include.optionalDependencies` is false, every
        // snapshot whose `optional` flag is true gets dropped from
        // the install graph. The lockfile's
        // [`SnapshotEntry::optional`] is set by the resolver when
        // the snapshot is reachable **only** through optional
        // edges; a snapshot reachable through any non-optional
        // edge carries `optional: false` and survives the filter
        // (a dependency that is both optional and non-optional is
        // installed). The exclusions land in the transient
        // `optional_excluded` subset of [`SkippedSnapshots`] so
        // they propagate to every downstream filter
        // (`CreateVirtualStore`, `SymlinkDirectDependencies`,
        // `BuildModules`, hoist) through the same gate
        // installability skips use — and stay out of
        // `.modules.yaml.skipped` so a future install without
        // `--no-optional` brings them back.
        let include_optional = dependency_groups.contains(&DependencyGroup::Optional);
        if !include_optional && let Some(snaps) = snapshots {
            for (key, snap) in snaps {
                if snap.optional {
                    skipped.add_optional_excluded(key.clone());
                }
            }
        }

        if skip_runtimes && let Some(pkgs) = packages {
            crate::add_direct_runtime_skips(&mut skipped, importers, pkgs);
        }

        // The recorded skip set must be the reachability closure of the
        // direct skips (see
        // [`crate::extend_skipped_with_dependency_closure`]); extend it
        // before `CreateVirtualStore`, the hoist pass, and the symlink /
        // bin passes consume it.
        {
            let importer_ids: std::collections::HashSet<String> =
                importers.keys().cloned().collect();
            crate::extend_skipped_with_dependency_closure(
                &mut skipped,
                lockfile,
                workspace_root,
                &importer_ids,
                pacquet_modules_yaml::IncludedDependencies {
                    dependencies: dependency_groups.contains(&DependencyGroup::Prod),
                    dev_dependencies: dependency_groups.contains(&DependencyGroup::Dev),
                    optional_dependencies: include_optional,
                },
            );
        }

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
        // Three paths:
        // - Already detected the host for the installability check
        //   (constraint-bearing lockfile): reuse the cached version
        //   synchronously. Synthetic-fallback (`node_detected = false`)
        //   yields `None` so a bogus `99999.0.0`-derived key can't
        //   poison either the cache or the GVS hash.
        // - GVS on, no host yet: spawn `node --version` synchronously
        //   — layout construction below needs the result.
        // - GVS off, no host yet: spawn into the blocking pool and
        //   keep the join handle. The spawn runs concurrently with
        //   `CreateVirtualStore::run`'s I/O, so the `node --version`
        //   cost (~tens of ms) is hidden under the install. The
        //   handle is awaited right before `BuildModules` —
        //   `VirtualStoreLayout` is built with `None` here, which
        //   is fine because GVS is off and the layout ignores the
        //   field in that path.
        // Honour `engines.runtime` / `devEngines.runtime` pin (if
        // one reached the lockfile): the runtime resolver writes
        // the chosen Node as a `node@runtime:<version>` snapshot, and
        // the engine-name helper anchors the GVS hash and the
        // side-effects-cache key prefix to that pinned Node —
        // otherwise pacquet hashes under whatever
        // `node --version` returns from the shell, splitting the
        // shared store between pinned and non-pinned installs on the
        // same host.
        let runtime_pinned_major = find_runtime_node_major(snapshots);
        let (initial_engine_name, deferred_engine_handle): (
            Option<String>,
            Option<tokio::task::JoinHandle<Option<String>>>,
        ) = if let Some(major) = runtime_pinned_major {
            // Lockfile-driven major wins outright; skip the host
            // probe / `node --version` spawn entirely.
            (Some(pacquet_graph_hasher::engine_name(major, None, None)), None)
        } else {
            match &host_node {
                Some((true, ver)) => (
                    parse_major_from_version(ver)
                        .map(|major| pacquet_graph_hasher::engine_name(major, None, None)),
                    None,
                ),
                Some((false, _)) => (None, None),
                None if config.enable_global_virtual_store => (
                    tokio::task::spawn_blocking(|| {
                        pacquet_graph_hasher::detect_node_major()
                            .map(|major| pacquet_graph_hasher::engine_name(major, None, None))
                    })
                    .await
                    .ok()
                    .flatten(),
                    None,
                ),
                None => (
                    None,
                    Some(tokio::task::spawn_blocking(|| {
                        pacquet_graph_hasher::detect_node_major()
                            .map(|major| pacquet_graph_hasher::engine_name(major, None, None))
                    })),
                ),
            }
        };
        let engine_name = initial_engine_name;

        let hoisted_workspace_packages = config
            .hoist_workspace_packages
            .then(|| workspace_packages_for_hoist(workspace_root, project_manifests));
        let (mut pre_hoist, context_projection) = compute_hoist_plan_and_context_projection(
            config,
            snapshots,
            packages,
            importers,
            &dependency_groups,
            &skipped,
            is_hoisted,
            hoisted_workspace_packages.as_ref(),
        );

        // Build the install-scoped slot-directory layout. When
        // `enable_global_virtual_store` is on the layout precomputes
        // each snapshot's `<scope>/<name>/<version>/<hash>` suffix
        // from [`pacquet_graph_hasher::calc_graph_node_hash`];
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
            Some(&context_projection),
        );

        // Reject a lockfile whose dependency names, aliases, or
        // virtual-store slots would escape the project or the store once
        // joined into a filesystem path. Runs before any materialization
        // and before the warm-install skip filter, and unconditionally —
        // so it is not bypassed by `trustLockfile`, which disables the
        // resolution-verification fan-out where the offline name check
        // would otherwise run. The slot-containment half needs the
        // install-time `layout`, so it can't live in the verifier crate.
        pacquet_lockfile_verification::verify_lockfile_dependency_names(lockfile)
            .map_err(InstallFrozenLockfileError::LockfileVerification)?;
        crate::validate_virtual_store_slot_containment(snapshots, &layout)
            .map_err(InstallFrozenLockfileError::LockfileVerification)?;

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
        let custom_fetcher_picker = load_custom_fetcher_picker(workspace_root).await?;
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
                allow_build_policy: &allow_build_policy,
                skipped: &skipped,
                supported_architectures,
                workspace_root,
                node_linker,
                progress_reported: &progress_reported,
                tarball_mem_cache,
                custom_fetcher_picker: custom_fetcher_picker.as_ref(),
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
        let had_fetch_failures = !fetch_failed.is_empty();
        for key in fetch_failed {
            skipped.add_fetch_failed(key);
        }
        if had_fetch_failures && (!config.enable_global_virtual_store || is_hoisted) {
            pre_hoist = compute_hoist_plan(
                config,
                snapshots,
                packages,
                importers,
                &dependency_groups,
                &skipped,
                is_hoisted,
                hoisted_workspace_packages.as_ref(),
            );
        }

        let public_hoist_targets: Option<BTreeMap<String, PathBuf>> =
            pre_hoist.as_ref().map(|plan| {
                collect_public_hoist_targets(&plan.result, &plan.graph, &layout, &plan.skipped)
            });

        // Reconcile before linking: stale direct-dep links and
        // orphaned hoist links must vacate their slots so the relink +
        // rehoist below can claim them. The hoisted linker is excluded
        // — its previous-graph diff removes orphans and emits the
        // `pnpm:stats` `removed` event itself (see
        // [`crate::link_hoisted_modules()`]); on the isolated linker
        // the event fires here, so every install carries exactly one,
        // pairing the `added` emitted in `CreateVirtualStore`.
        //
        // `virtual_store_only` skips reconciliation for the same reason
        // it skips linking below: it never creates importer or hoist
        // links, so there is nothing of its own to reconcile.
        if !is_hoisted && !config.virtual_store_only {
            let removed_count = match current_lockfile {
                Some(current) => crate::PruneStaleModules {
                    config,
                    workspace_root,
                    wanted_lockfile: lockfile,
                    current_lockfile: current,
                    prior_hoisted_dependencies,
                    included_groups: &dependency_groups,
                    prune_orphans,
                }
                .run::<Reporter>()
                .map_err(InstallFrozenLockfileError::PruneStaleModules)?,
                None => 0,
            };
            Reporter::emit(&LogEvent::Stats(StatsLog {
                level: LogLevel::Debug,
                message: StatsMessage::Removed {
                    prefix: requester.to_owned(),
                    removed: removed_count,
                },
            }));
        }

        // `virtual_store_only` stops here: the virtual store is
        // populated, but nothing downstream of it — importer symlinks,
        // per-slot bins, root components — gets linked.
        if !is_hoisted && !config.virtual_store_only {
            if config.symlink {
                materialize_global_virtual_store_context(&layout, &skipped, &extra_node_paths)
                    .map_err(InstallFrozenLockfileError::MaterializeGlobalVirtualStoreContext)?;
            }
            // Importer ids backed by the install's own declared
            // projects. These may legitimately live outside the
            // lockfile dir (Bit's capsule installs pass such
            // projects), so they bypass the malformed-lockfile
            // importer-key rejection.
            let trusted_importer_ids: std::collections::HashSet<String> = project_manifests
                .iter()
                .map(|(project_dir, _)| {
                    pacquet_workspace::importer_id_from_root_dir(workspace_root, project_dir)
                })
                .collect();
            SymlinkDirectDependencies {
                config,
                layout: &layout,
                importers,
                packages,
                dependency_groups: dependency_groups.iter().copied(),
                workspace_root,
                skipped: &skipped,
                link_only: false,
                public_hoist_targets: public_hoist_targets.as_ref(),
                trusted_importer_ids: Some(&trusted_importer_ids),
                extra_node_paths: &extra_node_paths,
            }
            .run::<Reporter>()
            .map_err(InstallFrozenLockfileError::SymlinkDirectDependencies)?;

            // Bit "root components": make each root's injected members
            // mutually reachable. Gated on
            // `installConfig.hoistingLimits: "workspaces"`, so it is a
            // no-op for every non-Bit install. See
            // [`link_root_component_members`]. `project_manifests` keys
            // are project directories; map each back to its lockfile
            // importer id so the set lines up with `importers`.
            let root_component_importers: std::collections::HashSet<String> = project_manifests
                .iter()
                .filter(|(_, manifest)| {
                    manifest.install_config_hoisting_limits()
                        == Some(crate::HOISTING_LIMITS_WORKSPACES)
                })
                .map(|(project_dir, _)| {
                    pacquet_workspace::importer_id_from_root_dir(workspace_root, project_dir)
                })
                .collect();
            link_root_component_members(
                &layout,
                importers,
                &root_component_importers,
                &dependency_groups,
                &skipped,
            )
            .map_err(InstallFrozenLockfileError::LinkRootComponentMembers)?;

            // Link the bins of each virtual-store slot's children into the
            // slot's own `node_modules/.bin`.
            // Done before `importing_done` so reporters see the import phase
            // close only after every link (including per-slot bins) is in
            // place. The manifest map threaded from `CreateVirtualStore`
            // lets the linker hit `pkgFilesIndex.manifest` directly instead
            // of re-reading every child's `package.json` from disk.
            //
            // Both passes are gated by `!is_hoisted`: under
            // `nodeLinker: hoisted` there is no virtual store
            // (`CreateVirtualStore` skipped slot writes), and the
            // bin links go into `<parent>/node_modules/.bin` for
            // every hoist location instead. The hoisted linker
            // ([`crate::link_hoisted_modules()`], called below) does
            // its own per-`node_modules` bin pass while walking the
            // hierarchy, routing both link phases through the hoisted
            // linker.
            LinkVirtualStoreBins {
                layout: &layout,
                snapshots,
                packages,
                package_manifests: &package_manifests,
                skipped: &skipped,
                extra_node_paths: &extra_node_paths,
            }
            .run()
            .map_err(InstallFrozenLockfileError::LinkVirtualStoreBins)?;
        }

        // Hoisted-linker materialization. Replaces the isolated
        // [`crate::SymlinkDirectDependencies`] +
        // [`crate::LinkVirtualStoreBins`] pair when
        // `nodeLinker: hoisted` is in effect: the dep-graph walker
        // computes per-package directories (with conflict-aware
        // nesting), and the linker imports CAS files into those
        // directories from
        // [`CreateVirtualStoreOutput::cas_paths_by_pkg_id`] which
        // was populated above with `node_linker = Hoisted`.
        //
        // `hoisted_locations` is the per-depPath list of
        // lockfile-relative directories the walker emits. Threaded
        // through [`InstallFrozenLockfileOutput`] so
        // [`crate::Install::run`] can persist it into
        // `.modules.yaml.hoisted_locations` (rebuild reads it back
        // and surfaces `MISSING_HOISTED_LOCATIONS` if it's gone).
        //
        // `pkg_roots_by_key` is a per-snapshot override for
        // `BuildModules`'s `pkgRoot` lookup. Populated from the
        // walker's [`crate::DependenciesGraphNode::dir`] values so
        // the build phase can `cd` into the on-disk hoisted
        // directory instead of computing a virtual-store slot path
        // that doesn't exist under hoisted. `None` (and an empty
        // `hoisted_locations`) for the isolated linker. See
        // [`crate::BuildModules::pkg_roots_by_key`] for why a snapshot
        // can map to more than one directory and which writes have to
        // reach all of them.
        let HoistedLinkerOutput { hoisted_locations, hoisted_pkg_roots_by_key } =
            if is_hoisted && !config.virtual_store_only {
                run_hoisted_linker::<Reporter>(
                    HoistedLinkerInputs {
                        config,
                        lockfile,
                        current_lockfile,
                        layout: &layout,
                        importers,
                        dependency_groups: &dependency_groups,
                        project_manifests,
                        package_map_project_manifests,
                        walker_lockfile_dir: workspace_root,
                        symlink_workspace_root: workspace_root,
                        host_node: host_node.as_ref(),
                        supported_architectures,
                        cas_paths_by_pkg_id,
                        logged_methods,
                        requester,
                    },
                    &mut skipped,
                )
                .map_err(InstallFrozenLockfileError::from)?
            } else {
                HoistedLinkerOutput::default()
            };

        // Hoist transitive deps into `<virtual_store>/node_modules`
        // (private hoist) and/or `<root>/node_modules` (public hoist).
        //
        // The guard is `hoistPattern != null || publicHoistPattern != null`
        // — `Some(empty)` is a valid disabled state for one side but
        // not the other, so the guard checks `is_some()` on the field
        // (not `Vec` length). With pacquet's defaults both sides are
        // `Some(non-empty)`, so the pass runs by default.
        // Stashed across the hoist pass for the post-`BuildModules`
        // top-level bin link. Isolated-linker public-hoist promotes
        // a transitive dep alias to `<root>/node_modules/<alias>`
        // where it competes for the same `<root>/node_modules/.bin`
        // slot as the root importer's direct deps. Per
        // pnpm/pacquet#342 the direct dep's bin must win. The post-build pass below
        // takes both direct + hoisted candidate lists so
        // `pacquet_cmd_shim::pick_winner` (private)'s [`BinOrigin`] tier
        // resolves the conflict in one call. Empty means there's
        // no public-hoist (no patterns set, hoisted linker, or
        // `Some(empty)`-vs-`None` short-circuit).
        let mut publicly_hoisted_for_post_build: Vec<String> = Vec::new();
        // Isolated-linker hoist pass: shamefully-hoist + private
        // hoist into the virtual store. Skipped under hoisted —
        // the hoisted linker materialized the project tree above
        // and there's no virtual store to point hoist symlinks at,
        // so no new isolated-hoist results are produced when no
        // `hoistPattern` / `publicHoistPattern` is configured.
        //
        // The BFS itself ran upthread (`pre_hoist`) so the dedupe
        // pass in `SymlinkDirectDependencies` could see public-hoist
        // targets; here we consume the same plan to write the
        // symlinks on disk and emit the per-side bin shims.
        let hoisted_dependencies = if let Some(plan) = pre_hoist {
            let HoistPlan { graph, result, skipped: hoist_skipped, .. } = plan;
            // Public-hoist target is the project's root
            // `node_modules` (= `config.modules_dir`).
            // Private-hoist target is the project-local
            // `<root>/node_modules/.pnpm/node_modules` —
            // pacquet's `config.virtual_store_dir` always
            // resolves there even with GVS enabled: pacquet keeps
            // `virtual_store_dir` project-local and
            // routes the GVS-shared root through
            // `global_virtual_store_dir` instead — see
            // [`Config::apply_global_virtual_store_derivation`].
            // The symlink *target* (under the slot dir)
            // does need to be GVS-aware, which the
            // `VirtualStoreLayout` handle below provides.
            let private_dir = config.virtual_store_dir.join("node_modules");
            let public_dir = config.modules_dir.clone();
            symlink_hoisted_dependencies(
                &result.hoisted_dependencies_by_node_id,
                &result.hoisted_workspace_aliases,
                &graph,
                &layout,
                &private_dir,
                &public_dir,
                &hoist_skipped,
            )
            .map_err(InstallFrozenLockfileError::HoistSymlink)?;
            // Private-side bins → `<vs>/node_modules/.bin`.
            // Reuses the rayon-parallel `link_direct_dep_bins`
            // shape (read each location's `package.json`, fan out
            // to `link_bins_of_packages`).
            link_direct_dep_bins_resolved(
                &private_dir,
                &crate::resolve_hoisted_bin_deps(&layout, &result.hoisted_aliases_with_bins),
                &extra_node_paths,
            )
            .map_err(InstallFrozenLockfileError::HoistLinkBins)?;
            // Stash the public-hoist alias list for the
            // post-`BuildModules` top-level bin link, which re-links
            // with the [`BinOrigin`] tier so a direct dep's bin wins
            // outright over a publicly-hoisted bin with a lexically
            // smaller name. The re-link runs after `buildModules`.
            publicly_hoisted_for_post_build = result.publicly_hoisted_aliases_with_bins;
            result.hoisted_dependencies
        } else {
            BTreeMap::new()
        };

        let included = IncludedDependencies {
            dependencies: dependency_groups.contains(&DependencyGroup::Prod),
            dev_dependencies: dependency_groups.contains(&DependencyGroup::Dev),
            optional_dependencies: dependency_groups.contains(&DependencyGroup::Optional),
        };
        if crate::should_write_package_map(config, node_linker) {
            let filtered_lockfile =
                crate::filter_lockfile_for_current(lockfile, included, &skipped);
            crate::package_map::write_package_map(
                &filtered_lockfile,
                &crate::package_map::PackageMapOptions {
                    lockfile_dir: workspace_root,
                    modules_dir: &config.modules_dir,
                    package_map_type: config.node_package_map_type,
                    layout: &layout,
                    project_manifests,
                },
            )
            .map_err(InstallFrozenLockfileError::WritePackageMap)?;
        }
        if matches!(node_linker, NodeLinker::Pnp) {
            let filtered_lockfile =
                crate::filter_lockfile_for_current(lockfile, included, &skipped);
            crate::write_pnp_file(
                &filtered_lockfile,
                workspace_root,
                config,
                &layout,
                project_manifests,
            )
            .map_err(InstallFrozenLockfileError::WritePnpFile)?;
        }

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
        let engine_name = match deferred_engine_handle {
            Some(handle) => handle.await.ok().flatten(),
            None => engine_name,
        };

        let mut build_extra_env = config.extra_env.clone();
        if let Some(node_options) = &config.node_options {
            build_extra_env.insert("NODE_OPTIONS".to_string(), node_options.clone());
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
        let crate::BuildModulesOutput { ignored_builds, deferred_builds } =
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
                engine_name: engine_name.as_deref(),
                extra_env: &build_extra_env,
                store_index_writer: &store_index_writer,
                skipped: &skipped,
                hoisted_pkg_roots_by_key: hoisted_pkg_roots_by_key.as_ref(),
                is_hoisted,
                publicly_hoisted_for_post_build: &publicly_hoisted_for_post_build,
                logged_methods,
                rebuild,
                extra_node_paths: &extra_node_paths,
            })
            .map_err(InstallFrozenLockfileError::BuildPhase)?;

        // Drop the orchestrator's clone of the writer so the channel
        // closes once every per-snapshot clone has also been dropped;
        // then await the task so the final batch flushes before
        // returning. Swallow any error with `warn!` — the install is
        // complete and a missed cache write just forces a re-fetch
        // on the next install.
        drop(store_index_writer);
        match writer_task.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(
                target: "pacquet::install",
                ?error,
                "store-index writer task returned an error; some rows may not be persisted",
            ),
            Err(error) => tracing::warn!(
                target: "pacquet::install",
                ?error,
                "store-index writer task panicked; some rows may not be persisted",
            ),
        }

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
    /// through [`pacquet_modules_yaml::Modules::hoisted_locations`]
    /// so a follow-up install (or rebuild) can locate every
    /// package without re-running the walker.
    pub hoisted_locations: BTreeMap<String, Vec<String>>,
    /// Per-source-project list of virtual-store package directories
    /// its injected `file:` copies were materialized at. Round-trips
    /// through [`pacquet_modules_yaml::Modules::injected_deps`] —
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
}

/// Internal handoff between the hoisted-linker walker/linker pass
/// and the downstream `BuildModules` + `.modules.yaml` writes. Bundled
/// as a struct so the hoisted branch in [`InstallFrozenLockfile::run`]
/// can return both fields in one binding without tripping
/// `clippy::type_complexity`. Always [`Default`]-empty for the
/// isolated linker.
#[derive(Debug, Default)]
pub(crate) struct HoistedLinkerOutput {
    /// `LockfileToDepGraphResult::hoisted_locations` from the slice
    /// 4 walker. Persisted into `.modules.yaml.hoisted_locations`
    /// when non-empty.
    pub(crate) hoisted_locations: BTreeMap<String, Vec<String>>,
    /// Per-snapshot `pkgRoot` override for the build phase — snapshot
    /// key → every directory the hoisted graph placed it in, in walker
    /// order. `None` for the isolated linker (the layout-based lookup in
    /// `BuildModules` is used instead). See
    /// [`crate::BuildModules::pkg_roots_by_key`] for how the list is
    /// consumed.
    pub(crate) hoisted_pkg_roots_by_key: Option<HashMap<PackageKey, Vec<std::path::PathBuf>>>,
}

/// Inputs to [`run_hoisted_linker`]. Bundled so the two install
/// paths ([`InstallFrozenLockfile`] and `InstallWithFreshLockfile`)
/// can feed the shared hoisted-linker materialization without a
/// long positional argument list. The frozen path passes the
/// loaded `pnpm-lock.yaml`; the fresh path passes the freshly-built
/// lockfile and `current_lockfile: None`.
pub(crate) struct HoistedLinkerInputs<'a> {
    pub(crate) config: &'static Config,
    /// Lockfile the walker reads `snapshots:` / `packages:` /
    /// `importers:` from. `&built_lockfile` on the fresh path,
    /// the loaded wanted lockfile on the frozen path.
    pub(crate) lockfile: &'a Lockfile,
    /// Previous install's `<virtual_store_dir>/lock.yaml`, used by the
    /// walker to diff orphans. `None` on the fresh path (no analogue
    /// yet).
    pub(crate) current_lockfile: Option<&'a Lockfile>,
    pub(crate) layout: &'a VirtualStoreLayout,
    pub(crate) importers: &'a HashMap<String, ProjectSnapshot>,
    pub(crate) dependency_groups: &'a [DependencyGroup],
    /// Selected project anchors whose direct dependencies and workspace
    /// links are written by this filtered run.
    pub(crate) project_manifests: &'a [(PathBuf, &'a pacquet_package_manifest::PackageManifest)],
    /// Every real importer manifest represented in the full hoisted graph.
    /// The shared package map needs all project names for self-reference
    /// entries even though direct links are limited to selected anchors.
    pub(crate) package_map_project_manifests:
        &'a [(PathBuf, &'a pacquet_package_manifest::PackageManifest)],
    /// Lockfile root the walker resolves hoisted directories against.
    pub(crate) walker_lockfile_dir: &'a Path,
    /// Anchor for [`crate::SymlinkDirectDependencies`]'s per-importer
    /// `node_modules` lookup. Equals `walker_lockfile_dir` on the
    /// frozen path; the fresh path passes `config.modules_dir.parent()`
    /// so relocated `modules_dir` test configs land symlinks where the
    /// rest of the install writes.
    pub(crate) symlink_workspace_root: &'a Path,
    /// `(node_detected, node_version)` from the installability host
    /// probe. `None` when no installability check ran (the fresh
    /// path, and constraint-free frozen lockfiles).
    pub(crate) host_node: Option<&'a (bool, String)>,
    pub(crate) supported_architectures:
        Option<&'a pacquet_package_is_installable::SupportedArchitectures>,
    /// Per-package CAS index produced by [`crate::CreateVirtualStore`]
    /// under `node_linker == Hoisted`. The linker imports files from
    /// these paths into the on-disk hoisted tree.
    pub(crate) cas_paths_by_pkg_id: Option<crate::CasPathsByPkgId>,
    pub(crate) logged_methods: &'a AtomicU8,
    pub(crate) requester: &'a str,
}

/// Error type of [`run_hoisted_linker`]. Each install path maps these
/// back onto its own error enum's matching variant so the user-facing
/// error code is identical regardless of which path drove the hoist.
#[derive(Debug, Display, Error, Diagnostic)]
pub(crate) enum HoistedLinkerError {
    #[diagnostic(transparent)]
    HoistedDepGraph(#[error(source)] HoistedDepGraphError),
    #[diagnostic(transparent)]
    LinkHoistedModules(#[error(source)] LinkHoistedModulesError),
    #[diagnostic(transparent)]
    SymlinkDirectDependencies(#[error(source)] SymlinkDirectDependenciesError),
    #[display("failed to write package map: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_WRITE_PACKAGE_MAP))]
    WritePackageMap(#[error(source)] crate::WritePackageMapError),
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

/// Materialize the `nodeLinker: hoisted` on-disk tree from a lockfile.
///
/// Runs the [`crate::lockfile_to_hoisted_dep_graph`] walker over the
/// lockfile's snapshots, materializes the resulting graph with
/// [`crate::link_hoisted_modules()`] (real directories under each
/// importer's tree, fed from `cas_paths_by_pkg_id`), then layers
/// [`crate::SymlinkDirectDependencies`] with `link_only: true` to wire
/// `workspace:` / `link:` deps the hoist walker skips. Folds the
/// walker's newly-discovered installability skips into `skipped`.
///
/// Shared by both install paths so the hoisted layout, skip-set
/// accounting, and `pkg_roots_by_key` derivation stay identical.
pub(crate) fn run_hoisted_linker<Reporter: self::Reporter>(
    inputs: HoistedLinkerInputs<'_>,
    skipped: &mut SkippedSnapshots,
) -> Result<HoistedLinkerOutput, HoistedLinkerError> {
    let HoistedLinkerInputs {
        config,
        lockfile,
        current_lockfile,
        layout,
        importers,
        dependency_groups,
        project_manifests,
        package_map_project_manifests,
        walker_lockfile_dir,
        symlink_workspace_root,
        host_node,
        supported_architectures,
        cas_paths_by_pkg_id,
        logged_methods,
        requester,
    } = inputs;

    // The hoist tree seeds from every importer dep map, so groups the
    // user excluded (`--prod`, `--dev`, `--no-optional`) must be cleared
    // from the lockfile before the walk — otherwise their whole subgraph
    // materializes as real directories. Mirrors pnpm, which hands its
    // hoisted walker an include-filtered lockfile.
    let included = IncludedDependencies {
        dependencies: dependency_groups.contains(&DependencyGroup::Prod),
        dev_dependencies: dependency_groups.contains(&DependencyGroup::Dev),
        optional_dependencies: dependency_groups.contains(&DependencyGroup::Optional),
    };
    let filtered_lockfile;
    let lockfile =
        if included.dependencies && included.dev_dependencies && included.optional_dependencies {
            lockfile
        } else {
            filtered_lockfile = exclude_importer_groups(lockfile, included);
            &filtered_lockfile
        };

    // Walker installability inputs come straight from the optional
    // `host_node` the caller built for the `compute_skipped_snapshots`
    // pass. When `host_node` is `None` no per-snapshot constraint
    // exists, so the host triple values pass through as defaults the
    // walker won't actually consult.
    let walker_skipped: BTreeSet<String> =
        skipped.iter().map(std::string::ToString::to_string).collect();
    let walker_opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: walker_lockfile_dir.to_path_buf(),
        auto_install_peers: config.auto_install_peers,
        skipped: walker_skipped.clone(),
        force: config.force,
        // Matches the `engineStrict` policy `compute_skipped_snapshots`
        // used upthread (both read `config.engine_strict`): an engine
        // mismatch on a required package is a hard error under strict,
        // otherwise a skip-optional / warning.
        engine_strict: config.engine_strict,
        current_node_version: host_node.map(|(_, ver)| ver.clone()).unwrap_or_default(),
        current_os: pacquet_graph_hasher::host_platform().to_string(),
        current_cpu: pacquet_graph_hasher::host_arch().to_string(),
        current_libc: pacquet_graph_hasher::host_libc().to_string(),
        supported_architectures: supported_architectures.cloned(),
        hoist_workspace_packages: config.hoist_workspace_packages,
        hoisting_limits: crate::get_hoisting_limits(&lockfile.importers, config.hoisting_limits),
        external_dependencies: config.external_dependencies.clone(),
    };
    let walker_result = lockfile_to_hoisted_dep_graph(lockfile, current_lockfile, &walker_opts)
        .map_err(HoistedLinkerError::HoistedDepGraph)?;
    // Augment the live skip set with the walker's *new* skips only —
    // entries already in `walker_skipped` came from the input
    // `SkippedSnapshots`, where each one already lives in its proper
    // subset (installability / fetch-failed / optional-excluded).
    // Re-inserting them as installability would promote transient
    // `fetch_failed` / `optional_excluded` entries into the
    // persisted-on-disk `.modules.yaml.skipped` set, which would
    // survive into the next install — exactly the contract those
    // subsets exist to prevent. Diffing against the input set keeps
    // the persistence boundary intact: only walker-discovered
    // installability skips (optional + unsupported platform) flow
    // into [`SkippedSnapshots::insert_installability`].
    for skipped_dep_path in walker_result.skipped.difference(&walker_skipped) {
        if let Ok(key) = skipped_dep_path.parse::<PackageKey>() {
            skipped.insert_installability(key);
        }
    }
    // Empty CAS index → linker would refuse every non-optional node.
    // Only happens when the install has no snapshots, in which case
    // the linker is a no-op.
    let cas_index = cas_paths_by_pkg_id.expect("hoisted CreateVirtualStore populates cas_paths");
    let link_opts = LinkHoistedModulesOpts {
        graph: &walker_result.graph,
        prev_graph: walker_result.prev_graph.as_ref(),
        hierarchy: &walker_result.hierarchy,
        cas_paths_by_pkg_id: &cas_index,
        import_method: config.package_import_method,
        logged_methods,
        requester,
        confine_root: walker_lockfile_dir,
    };
    link_hoisted_modules::<Reporter>(&link_opts).map_err(HoistedLinkerError::LinkHoistedModules)?;
    link_selected_hoisted_direct_dependencies(
        config,
        walker_lockfile_dir,
        project_manifests,
        &walker_result.direct_dependencies_by_importer_id,
    )?;
    crate::package_map::write_hoisted_package_map(
        lockfile,
        &walker_result,
        &crate::package_map::HoistedPackageMapOptions {
            lockfile_dir: walker_lockfile_dir,
            modules_dir: &config.modules_dir,
            package_map_type: config.node_package_map_type,
            project_manifests: package_map_project_manifests,
        },
    )
    .map_err(HoistedLinkerError::WritePackageMap)?;
    // Workspace `link:` deps still need symlinks under each importer's
    // `node_modules/<alias>` even though the regular deps now live as
    // real directories. The hoisted dep-graph walker skips
    // `workspace:`-prefixed references entirely (they're not in the
    // hoist tree), so without this pass workspace siblings would be
    // missing from each project's `node_modules/`. `link_only: true`
    // filters every other dep out so the call doesn't try to re-create
    // symlinks for packages that the hoisted linker already wrote as
    // real dirs.
    // Importer ids backed by the install's own declared projects —
    // allowed outside the lockfile dir (see the isolated-path use).
    // Ids are lockfile-dir-relative, so derive them against
    // `walker_lockfile_dir`.
    let trusted_importer_ids: std::collections::HashSet<String> = project_manifests
        .iter()
        .map(|(project_dir, _)| {
            pacquet_workspace::importer_id_from_root_dir(walker_lockfile_dir, project_dir)
        })
        .collect();
    SymlinkDirectDependencies {
        config,
        layout,
        importers,
        packages: lockfile.packages.as_ref(),
        dependency_groups: dependency_groups.iter().copied(),
        workspace_root: symlink_workspace_root,
        skipped: &*skipped,
        link_only: true,
        // Hoisted-linker path has no public-hoist virtual store to
        // dedupe against; the real-directory tree is the hoist layout.
        public_hoist_targets: None,
        trusted_importer_ids: Some(&trusted_importer_ids),
        // pnpm gates `extraNodePaths` on the isolated linker, so the
        // hoisted linker's shims never carry `NODE_PATH`.
        extra_node_paths: &[],
    }
    .run::<Reporter>()
    .map_err(HoistedLinkerError::SymlinkDirectDependencies)?;
    // Map snapshot key → every recorded directory, in walker order. The
    // walker emits multiple [`crate::DependenciesGraphNode`]s with the
    // same `dep_path` when the package nests under a sibling (version
    // conflict). Postinstall scripts and the side-effects-cache key both
    // depend only on the package contents (identical across locations),
    // so `BuildModules` runs those once at the head of the list; patch
    // application and cache-overlay re-imports walk the whole list.
    let mut pkg_roots_by_key: HashMap<PackageKey, Vec<std::path::PathBuf>> = HashMap::new();
    for node in walker_result.graph.values() {
        if let Ok(key) = node.dep_path.as_str().parse::<PackageKey>() {
            pkg_roots_by_key.entry(key).or_default().push(node.dir.clone());
        }
    }
    Ok(HoistedLinkerOutput {
        hoisted_locations: walker_result.hoisted_locations,
        hoisted_pkg_roots_by_key: Some(pkg_roots_by_key),
    })
}

fn link_selected_hoisted_direct_dependencies(
    config: &Config,
    lockfile_dir: &Path,
    project_manifests: &[(PathBuf, &pacquet_package_manifest::PackageManifest)],
    direct_dependencies_by_importer_id: &crate::DirectDependenciesByImporterId,
) -> Result<(), HoistedLinkerError> {
    let modules_dir_name =
        config.modules_dir.file_name().unwrap_or_else(|| OsStr::new("node_modules"));
    for (project_dir, _) in project_manifests {
        let importer_id = pacquet_workspace::importer_id_from_root_dir(lockfile_dir, project_dir);
        let Some(direct_dependencies) = direct_dependencies_by_importer_id.get(&importer_id) else {
            continue;
        };
        let modules_dir = project_dir.join(modules_dir_name);
        let mut linked_names = Vec::new();
        for (alias, target) in direct_dependencies {
            let link_path =
                crate::safe_join_modules_dir::safe_join_modules_dir(&modules_dir, alias).map_err(
                    |source| {
                        HoistedLinkerError::SymlinkDirectDependencies(
                            SymlinkDirectDependenciesError::SymlinkPackage {
                                importer_id: importer_id.clone(),
                                name: alias.clone(),
                                source: SymlinkPackageError::InvalidAlias(source),
                            },
                        )
                    },
                )?;
            if pacquet_fs::lexical_normalize(&link_path) == pacquet_fs::lexical_normalize(target) {
                linked_names.push(alias.clone());
                continue;
            }
            crate::symlink_package(target, &link_path).map_err(|source| {
                HoistedLinkerError::SymlinkDirectDependencies(
                    SymlinkDirectDependenciesError::SymlinkPackage {
                        importer_id: importer_id.clone(),
                        name: alias.clone(),
                        source,
                    },
                )
            })?;
            linked_names.push(alias.clone());
        }
        crate::link_direct_dep_bins(&modules_dir, &linked_names, &[]).map_err(|source| {
            HoistedLinkerError::SymlinkDirectDependencies(SymlinkDirectDependenciesError::LinkBins(
                source,
            ))
        })?;
    }
    Ok(())
}

/// Clone the lockfile with every importer's excluded dep groups
/// cleared, so seeds for the hoist tree come only from the included
/// groups. Snapshots that thereby become unreachable are simply never
/// visited by the hoister, so the snapshot/package maps stay as-is.
fn exclude_importer_groups(lockfile: &Lockfile, included: IncludedDependencies) -> Lockfile {
    let mut filtered = lockfile.clone();
    for importer in filtered.importers.values_mut() {
        if !included.dependencies {
            importer.dependencies = None;
        }
        if !included.dev_dependencies {
            importer.dev_dependencies = None;
        }
        if !included.optional_dependencies {
            importer.optional_dependencies = None;
        }
    }
    filtered
}

/// Pre-computed hoist plan threaded across the install pipeline so
/// the dedupe pass in [`crate::SymlinkDirectDependencies`] (which
/// runs before the on-disk hoist phase in pacquet's ordering) can
/// fold publicly-hoisted aliases into root's target map. The on-disk
/// hoist phase later consumes the same [`crate::HoistResult`] instead of
/// re-running the BFS.
pub(crate) struct HoistPlan {
    pub(crate) graph: HashMap<PackageKey, crate::HoistGraphNode>,
    pub(crate) result: crate::HoistResult,
    pub(crate) skipped: HashSet<PackageKey>,
}

/// Compute the in-memory hoist plan. Returns `None` when nothing
/// should be hoisted today (no patterns, no lockfile graph, or the
/// install is going through the hoisted linker). Side-effect-free:
/// the on-disk symlinks happen later in the pipeline. Same input
/// gating as the legacy in-place block in [`InstallFrozenLockfile::run`].
/// `hoist-workspace-packages` input: every named non-root project's
/// `name → absolute project dir`, the shape v11 builds from
/// `allProjects` for its `hoistedWorkspacePackages` map. The root
/// project itself is excluded — its dir *is* where the hoisted
/// modules live.
pub(crate) fn workspace_packages_for_hoist(
    workspace_root: &Path,
    project_manifests: &[(PathBuf, &pacquet_package_manifest::PackageManifest)],
) -> std::collections::BTreeMap<String, PathBuf> {
    project_manifests
        .iter()
        .filter(|(project_dir, _)| project_dir != workspace_root)
        .filter_map(|(project_dir, manifest)| {
            let name = manifest.value().get("name")?.as_str()?;
            Some((name.to_string(), project_dir.clone()))
        })
        .collect()
}

#[expect(
    clippy::too_many_arguments,
    reason = "bundles every lockfile/config axis one hoist plan needs; both call sites pass the same shapes"
)]
pub(crate) fn compute_hoist_plan(
    config: &Config,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    importers: &HashMap<String, pacquet_lockfile::ProjectSnapshot>,
    dependency_groups: &[pacquet_package_manifest::DependencyGroup],
    skipped: &SkippedSnapshots,
    is_hoisted: bool,
    hoisted_workspace_packages: Option<&std::collections::BTreeMap<String, PathBuf>>,
) -> Option<HoistPlan> {
    if is_hoisted {
        return None;
    }
    // Independent of the empty patterns
    // [`Config::apply_virtual_store_only_derivation`] leaves behind, so a
    // caller that sets the flag without going through `Config::current`
    // still gets no hoisting.
    if config.virtual_store_only {
        return None;
    }
    if config.hoist_pattern.is_none() && config.public_hoist_pattern.is_none() {
        return None;
    }
    let (Some(snaps), Some(pkgs)) = (snapshots, packages) else { return None };
    let private_pattern = create_matcher(config.hoist_pattern.as_deref().unwrap_or(&[]));
    let public_pattern = create_matcher(config.public_hoist_pattern.as_deref().unwrap_or(&[]));
    // Static fast-path: when both compiled matchers come from empty
    // pattern lists (`Some([])`), there's no alias they could match,
    // so the BFS would visit every node only to drop every child.
    // Skip the graph-build + walk entirely.
    if private_pattern.is_empty() && public_pattern.is_empty() {
        return None;
    }
    let graph = build_hoist_graph(snaps, pkgs);
    // Walk every importer's direct deps so transitives unique to a
    // workspace project still get privately hoisted into the shared
    // `<vs>/node_modules` and contribute to `hoistedDependencies`.
    // The `link:` workspace-sibling entries `build_direct_deps_by_importer`
    // sees are skipped via [`pacquet_lockfile::ImporterDepVersion::as_regular`].
    let direct_deps = build_direct_deps_by_importer(importers, dependency_groups.iter().copied());
    // `HoistInputs` takes `&HashSet<PackageKey>`; build it once from
    // the outer `SkippedSnapshots` by cloning the small skip set
    // (typically 0-100 entries). Stored on [`HoistPlan`] so the
    // later on-disk pass can reuse the exact same set the BFS saw.
    let hoist_skipped: HashSet<PackageKey> = skipped.iter().cloned().collect();
    let result = get_hoisted_dependencies(&crate::HoistInputs {
        graph: &graph,
        direct_deps_by_importer: &direct_deps,
        skipped: &hoist_skipped,
        private_pattern,
        public_pattern,
        hoisted_workspace_packages,
    })?;
    Some(HoistPlan { graph, result, skipped: hoist_skipped })
}

/// Computes the hoist plan and the matching resolver-visible projection.
///
/// Keeping both results together guarantees that the context hash and the
/// on-disk hoist use the same alias selection.
#[expect(
    clippy::too_many_arguments,
    reason = "the context projection must use the exact lockfile/config inputs that produced the reusable hoist plan"
)]
pub(crate) fn compute_hoist_plan_and_context_projection(
    config: &Config,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    importers: &HashMap<String, pacquet_lockfile::ProjectSnapshot>,
    dependency_groups: &[pacquet_package_manifest::DependencyGroup],
    skipped: &SkippedSnapshots,
    is_hoisted: bool,
    hoisted_workspace_packages: Option<&std::collections::BTreeMap<String, PathBuf>>,
) -> (Option<HoistPlan>, crate::GlobalVirtualStoreContextProjection) {
    let plan = compute_hoist_plan(
        config,
        snapshots,
        packages,
        importers,
        dependency_groups,
        skipped,
        is_hoisted,
        hoisted_workspace_packages,
    );
    let projection = if config.enable_global_virtual_store && !is_hoisted {
        packages.map_or_else(crate::GlobalVirtualStoreContextProjection::new, |packages| {
            let direct_deps =
                build_direct_deps_by_importer(importers, dependency_groups.iter().copied());
            let skipped = plan
                .as_ref()
                .map_or_else(|| skipped.iter().cloned().collect(), |plan| plan.skipped.clone());
            crate::get_global_virtual_store_context_projection(
                plan.as_ref().map(|plan| &plan.result),
                &direct_deps,
                packages,
                &skipped,
            )
        })
    } else {
        crate::GlobalVirtualStoreContextProjection::new()
    };
    (plan, projection)
}

/// Build the `<alias → resolved-target-dir>` map for every publicly-
/// hoisted entry that will land in root's `node_modules/`. Pacquet
/// runs the dedupe pass before the on-disk hoist phase, so this map
/// lets the dedupe see the aliases it would otherwise miss — by the
/// time the linker reads `<root>/node_modules/`, the public-hoist
/// symlinks are already there because hoist ran first.
///
/// Skipped snapshots are dropped (their slot dir doesn't exist on
/// disk), missing-in-graph entries are dropped, and only `Public`
/// hoists contribute (private hoists land in the virtual store's
/// own `node_modules`, not root's). The target path uses the same
/// `<slot>/node_modules/<name>` shape that the on-disk hoist symlink
/// will point at, so [`PathBuf`] equality with
/// [`SymlinkDirectDependencies`]'s computed targets is exact.
pub(crate) fn collect_public_hoist_targets(
    result: &crate::HoistResult,
    graph: &HashMap<PackageKey, crate::HoistGraphNode>,
    layout: &crate::VirtualStoreLayout,
    hoist_skipped: &HashSet<PackageKey>,
) -> BTreeMap<String, PathBuf> {
    let mut targets = BTreeMap::new();
    // Publicly-hoisted workspace packages land in root's
    // `node_modules/` too; their dedupe target is the project dir
    // the hoist symlink points at.
    for (alias, kind, project_dir) in &result.hoisted_workspace_aliases {
        if matches!(kind, pacquet_modules_yaml::HoistKind::Public) {
            targets.entry(alias.clone()).or_insert_with(|| project_dir.clone());
        }
    }
    for (node_id, alias_map) in &result.hoisted_dependencies_by_node_id {
        if hoist_skipped.contains(node_id) {
            continue;
        }
        let Some(node) = graph.get(node_id) else { continue };
        let dep_dir = layout.slot_dir(node_id).join("node_modules").join(node.name.to_string());
        for (alias, kind) in alias_map {
            if !matches!(kind, pacquet_modules_yaml::HoistKind::Public) {
                continue;
            }
            // First-wins: the BFS already chose one source per alias
            // via its `hoisted_aliases` claim. Multiple entries with
            // the same alias would be a hoister bug; preserve the
            // first deterministically.
            targets.entry(alias.clone()).or_insert_with(|| dep_dir.clone());
        }
    }
    targets
}

/// Pull the leading major-version digits out of a semver string like
/// `"22.11.0"`. Returns `None` if the leading token isn't parseable
/// as `u32`. Used to derive the engine-name string the
/// side-effects cache lookup expects without re-spawning
/// `node --version`.
pub(crate) fn parse_major_from_version(version: &str) -> Option<u32> {
    let after_v = version.strip_prefix('v').unwrap_or(version);
    after_v.split('.').next()?.parse().ok()
}

/// Pull the `node@runtime:<version>` major out of a lockfile's
/// `snapshots:` map, if the project pinned a runtime Node.
///
/// The runtime resolver writes the pinned Node into the lockfile as a
/// snapshot with key `node@runtime:<version>`. The engine-name string
/// anchors the GVS hash and the side-effects-cache key prefix to that
/// pinned major instead of the host's own `node --version`. Scans the
/// snapshots with "first hit wins" semantics (the resolver rejects
/// workspaces with conflicting pins before they reach the lockfile).
///
/// Returns `None` when no importer pinned a runtime — callers should
/// then fall through to the host probe (`node --version` or the
/// cached `host_node`).
pub(crate) fn find_runtime_node_major(
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
) -> Option<u32> {
    let snapshots = snapshots?;
    for key in snapshots.keys() {
        if key.suffix.prefix() != Prefix::Runtime {
            continue;
        }
        // Only `node@runtime:` feeds the Node-shaped engine string —
        // `bun@runtime:` and `deno@runtime:` exist as separate runtime
        // kinds. Scan for `node@runtime:` exclusively.
        if key.name.scope.is_some() || key.name.bare != "node" {
            continue;
        }
        // `Version::major` is `u64`; the major is small (<=99 in
        // practice), so the cast is lossless. The downstream
        // `engine_name` argument is `u32`.
        let major = key.suffix.version_semver()?.major;
        return Some(major as u32);
    }
    None
}

/// Read one snapshot's own `engines.runtime` Node pin from its
/// `dependencies` map. The resolver desugars `engines.runtime`
/// declared on a dep's manifest into
/// `dependencies.node: 'runtime:<version>'`.
///
/// Returns the bare major when this snapshot pins its own Node, or
/// `None` when it doesn't — callers should then fall back to the
/// install-wide pin / host probe via [`find_runtime_node_major`].
///
/// Per-snapshot resolution matters because the bin linker routes
/// lifecycle-script spawns for a pinning package through that
/// package's own downloaded Node. Anchoring the snapshot's GVS engine
/// hash to an install-wide value would produce the wrong
/// side-effects-cache key for cross-pinning installs.
pub(crate) fn find_own_runtime_node_major(snapshot: &SnapshotEntry) -> Option<u32> {
    let deps = snapshot.dependencies.as_ref()?;
    for (alias, dep_ref) in deps {
        if alias.scope.is_some() || alias.bare != "node" {
            continue;
        }
        // `link:` deps have no version slot and can't carry a
        // `runtime:` pin — skip them.
        let Some(ver_peer) = dep_ref.ver_peer() else {
            continue;
        };
        if ver_peer.prefix() != Prefix::Runtime {
            continue;
        }
        // Same cast as `find_runtime_node_major` above; see the
        // comment there for why `u64 → u32` is lossless in practice.
        return Some(ver_peer.version_semver()?.major as u32);
    }
    None
}

/// Load custom fetchers from the pnpmfile at `lockfile_dir`, if any.
/// Returns `Ok(None)` when no pnpmfile exists or it exports no
/// fetchers, so the install path can skip the IPC overhead entirely.
/// A pnpmfile that fails to load or evaluate aborts the install, like
/// the custom-resolver load on the fresh-lockfile path.
async fn load_custom_fetcher_picker(
    lockfile_dir: &Path,
) -> Result<
    Option<Arc<pacquet_hooks::custom_fetcher_adapter::CustomFetcherPicker>>,
    InstallFrozenLockfileError,
> {
    let Some(hook) = pacquet_hooks::finder::load_pnpmfile(lockfile_dir) else {
        return Ok(None);
    };
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
    Ok(Some(Arc::new(pacquet_hooks::custom_fetcher_adapter::CustomFetcherPicker::new(fetchers))))
}

#[cfg(test)]
mod tests;
