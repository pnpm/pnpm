use crate::{
    AllowBuildPolicy, BuildModules, BuildModulesError, CreateVirtualStore, CreateVirtualStoreError,
    CreateVirtualStoreOutput, HoistedDepGraphError, HoistedDependencies, InstallabilityHost,
    LinkHoistedModulesError, LinkHoistedModulesOpts, LinkVirtualStoreBins,
    LinkVirtualStoreBinsError, LockfileToHoistedDepGraphOptions, SkippedSnapshots,
    SymlinkDirectDependencies, SymlinkDirectDependenciesError, SymlinkPackageError,
    VersionPolicyError, VirtualStoreLayout, any_installability_constraint,
    build_direct_deps_by_importer, build_hoist_graph, compute_skipped_snapshots,
    direct_dep_names_for_importer, get_hoisted_dependencies, link_direct_dep_bins,
    link_hoisted_modules, link_top_level_bins, lockfile_to_hoisted_dep_graph,
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
use pacquet_modules_yaml::{Host, read_modules_manifest};
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::DependencyGroup;
use pacquet_patching::{
    ExtendedPatchInfo, PatchKeyConflictError, ResolvePatchedDependenciesError, get_patch_info,
};
use pacquet_reporter::{IgnoredScriptsLog, LogEvent, LogLevel, Reporter, Stage, StageLog};
use pacquet_store_dir::StoreIndexWriter;
use pacquet_tarball::{MemCache, SharedReportedProgressKeys};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicU8},
};

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
    /// The previous install's persisted current lockfile, threaded
    /// through to the hoisted walker for `prev_graph` (orphan
    /// diff). Mirrors upstream's
    /// [`currentLockfile`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/lockfileToHoistedDepGraph.ts#L70-L79)
    /// argument. `None` on a first install.
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
    /// chain. Mirrors upstream's
    /// [`nodeLinker === 'hoisted'`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/index.ts#L369-L425)
    /// branch in `headlessInstall`.
    ///
    /// Pacquet's [`NodeLinker::Pnp`] is a config / serde
    /// placeholder today; an install request with `Pnp` reaches
    /// the isolated linker in this branch (no `PnP` code path
    /// exists yet). Upstream's `nodeLinker: 'pnp'` is also
    /// out-of-scope for [#438](https://github.com/pnpm/pacquet/issues/438); tracked separately.
    pub node_linker: NodeLinker,

    /// Install-scoped shared in-flight tarball cache, threaded down to
    /// [`crate::CreateVirtualStore`]'s cold-batch downloads. `Some` on
    /// the pnpr client path so the materialization reuses the
    /// [`crate::TarballPrefetcher`]'s background downloads instead of
    /// re-fetching every tarball; `None` for installs without a shared
    /// prefetch in flight.
    pub tarball_mem_cache: Option<&'a Arc<MemCache>>,
}

/// Error type of [`InstallFrozenLockfile`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum InstallFrozenLockfileError {
    #[diagnostic(transparent)]
    CreateVirtualStore(#[error(source)] CreateVirtualStoreError),

    #[diagnostic(transparent)]
    SymlinkDirectDependencies(#[error(source)] SymlinkDirectDependenciesError),

    #[diagnostic(transparent)]
    LinkVirtualStoreBins(#[error(source)] LinkVirtualStoreBinsError),

    #[diagnostic(transparent)]
    BuildModules(#[error(source)] BuildModulesError),

    #[diagnostic(transparent)]
    ResolvePatchedDependencies(#[error(source)] ResolvePatchedDependenciesError),

    /// Surfaces a failure to create one of the hoist symlinks
    /// (`<private_hoisted_modules_dir>/<alias>` or
    /// `<public_hoisted_modules_dir>/<alias>`). EEXIST is
    /// already swallowed by [`crate::symlink_package()`]; this variant
    /// only fires on genuine IO failures.
    #[diagnostic(transparent)]
    HoistSymlink(#[error(source)] SymlinkPackageError),

    /// Surfaces a failure to link bins of privately-hoisted
    /// dependencies. Mirrors upstream's `linkAllBins` for the
    /// `privateHoistedModulesDir` (the public-side bins go through
    /// the existing direct-deps bin-link pass at the root).
    #[diagnostic(transparent)]
    HoistLinkBins(#[error(source)] LinkBinsError),

    /// Surfaces a failure from the post-`BuildModules` per-importer
    /// top-level bin link. This pass mixes direct + publicly-hoisted
    /// candidates so `pacquet_cmd_shim::pick_winner` (private)'s
    /// [`pacquet_cmd_shim::BinOrigin::Direct`] tier resolves
    /// conflicts in a single call (pnpm/pacquet#342). Distinct from
    /// [`Self::HoistLinkBins`] because the failure surface is the
    /// project-tree top-level `<importer>/node_modules/.bin` rather
    /// than the virtual store's private-hoisted dir.
    #[diagnostic(transparent)]
    TopLevelBinLink(#[error(source)] LinkBinsError),

    /// Surfaces upstream's `ERR_PNPM_PATCH_KEY_CONFLICT` when more
    /// than one configured version range matches a snapshot. Mirrors
    /// pnpm's behavior of refusing to silently pick one — the user
    /// must add an exact-version entry to disambiguate. See
    /// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/config/src/getPatchInfo.ts#L5-L19>.
    #[diagnostic(transparent)]
    PatchKeyConflict(#[error(source)] PatchKeyConflictError),

    /// Surfaces upstream's `ERR_PNPM_INVALID_VERSION_UNION` /
    /// `ERR_PNPM_NAME_PATTERN_IN_VERSION_UNION` when an
    /// `allowBuilds` key in `pnpm-workspace.yaml` can't be parsed.
    /// See <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/version-policy/src/index.ts#L60-L80>.
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
    ///   (slice 2) will surface user-supplied bad values here,
    ///   mirroring upstream's `ERR_PNPM_INVALID_NODE_VERSION` throw
    ///   at <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/src/checkEngine.ts#L25-L27>.
    /// - `InstallabilityError::Engine` / `InstallabilityError::Platform`
    ///   from a non-optional incompatible snapshot with
    ///   `engine_strict = true`. Pacquet's default has
    ///   `engine_strict = false`, so this path is currently
    ///   unreachable from production either — wired through so the
    ///   slice that lands the config setting doesn't churn the
    ///   error enum again. Mirrors upstream's `throw warn` at
    ///   <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/src/index.ts#L63>.
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
            current_lockfile,
            current_snapshots,
            current_packages,
            dependency_groups,
            logged_methods,
            workspace_root,
            requester,
            supported_architectures,
            skip_runtimes,
            node_linker,
            tarball_mem_cache,
        } = self;
        let is_hoisted = matches!(node_linker, NodeLinker::Hoisted);
        // Cloned so the iterator can be reused below for hoist's
        // direct-deps map. `Vec<DependencyGroup>` is tiny (≤4 enum
        // variants) so the clone is essentially free.
        let dependency_groups: Vec<DependencyGroup> = dependency_groups.into_iter().collect();

        // TODO: check if the lockfile is out-of-date

        // Build the allow-builds policy up front so it can flow into
        // the cold-batch git fetcher in `CreateVirtualStore` as well as
        // the postinstall phase in `BuildModules`. Mirrors pnpm where
        // `createAllowBuildFunction` is a per-install constant.
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
        let (store_index_writer, writer_task) = StoreIndexWriter::spawn(&config.store_dir);

        // Caller-side fast-path for the installability check. The
        // common case (no lockfile metadata row declares an
        // `engines` / `cpu` / `os` / `libc` constraint) lets us skip
        // both [`InstallabilityHost::detect`] and
        // [`compute_skipped_snapshots`] entirely. Spawning
        // `node --version` here would otherwise serialize the
        // node-binary startup with `CreateVirtualStore::run` (the
        // dominant cost of a cold install), giving up the overlap
        // pacquet had before — see the previous benchmark regression
        // on this PR.
        //
        // When constraints DO exist, the host is needed before
        // extraction (so `CreateVirtualStore` can suppress slots for
        // skipped snapshots), and the spawn cost is unavoidable.
        let needs_installability_check = match (snapshots, packages) {
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
        // a known-skipped package. Mirrors upstream's seed-from-
        // `modules.skipped` at
        // <https://github.com/pnpm/pnpm/blob/94240bc046/installing/read-projects-context/src/index.ts#L79>
        // and the early-return at
        // <https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts#L194>.
        //
        // A read error (corrupt yaml, permissions) is degraded to
        // an empty seed — `.modules.yaml` is a cache artifact, not
        // an authoritative source. Missing file → empty seed.
        let seed = match read_modules_manifest::<Host>(&config.modules_dir) {
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
        };

        // Build the per-install [`SkippedSnapshots`] set. For every
        // lockfile snapshot, run the installability check against
        // the host triple; optional+incompatible entries land in
        // the set and fire `pnpm:skipped-optional-dependency`.
        // Mirrors pnpm's headless re-check at
        // <https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts#L206-L215>.
        //
        // `host` is built only when needed. The detection path runs
        // `node --version` on the blocking pool so it doesn't stall
        // the reactor thread.
        let (mut skipped, host_node) = if needs_installability_check {
            let mut host = tokio::task::spawn_blocking(InstallabilityHost::detect)
                .await
                .unwrap_or_else(|_| InstallabilityHost {
                    node_version: "99999.0.0".to_string(),
                    node_detected: false,
                    os: pacquet_graph_hasher::host_platform(),
                    cpu: pacquet_graph_hasher::host_arch(),
                    libc: pacquet_graph_hasher::host_libc(),
                    supported_architectures: None,
                    engine_strict: false,
                });
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

        // `--no-optional` enforcement (umbrella slice 5). Mirrors
        // upstream's depNode filter at
        // <https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/src/install/link.ts#L109-L111>:
        // when `include.optionalDependencies` is false, every
        // snapshot whose `optional` flag is true gets dropped from
        // the install graph. The lockfile's
        // [`SnapshotEntry::optional`] is set by the resolver when
        // the snapshot is reachable **only** through optional
        // edges; a snapshot reachable through any non-optional
        // edge carries `optional: false` and survives the filter
        // (covers
        // <https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/optionalDependencies.ts#L712>
        // — `dependency that is both optional and non-optional is
        // installed`). The exclusions land in the transient
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

        // `--no-runtime` (or `config.skip_runtimes`): exclude
        // every project-direct runtime dependency. Mirrors
        // pnpm's `skipRuntimes` filter at
        // [`installing/deps-installer/src/install/index.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/src/install/index.ts#L1374-L1387)
        // exactly — iterate each importer's direct deps and add
        // the runtime ones to the skip set; transitive runtime
        // entries (which would be unusual but possible) stay in
        // the install. Upstream's discriminator is the
        // `depPath.includes('@runtime:')` substring check on the
        // resolved depPath; pacquet's lockfile preserves the
        // `@runtime:` substring in the snapshot key, so the same
        // string-test works here.
        //
        // Re-using `add_optional_excluded` keeps the bucket count
        // (and `.modules.yaml.skipped` semantics) unchanged: like
        // `--no-optional`, this is a transient user-driven
        // exclusion that should *not* be persisted into
        // `.modules.yaml.skipped` — a future install without the
        // flag must bring the runtime back.
        if skip_runtimes && let Some(pkgs) = packages {
            for importer in importers.values() {
                for dep_map in [
                    importer.dependencies.as_ref(),
                    importer.dev_dependencies.as_ref(),
                    importer.optional_dependencies.as_ref(),
                ] {
                    let Some(dep_map) = dep_map else { continue };
                    for (alias, spec) in dep_map {
                        // Build the candidate snapshot key. For
                        // non-aliased deps this is `(alias, version)`;
                        // for aliased deps it's the alias's own
                        // (name, suffix). `link:` deps are skipped.
                        // Matches pnpm's lookup by resolved depPath.
                        let Some(key) = spec.version.resolved_key(alias) else { continue };
                        if !key.to_string().contains("@runtime:") {
                            continue;
                        }
                        if let Some(meta) = pkgs.get(&key)
                            && matches!(
                                &meta.resolution,
                                pacquet_lockfile::LockfileResolution::Binary(_)
                                    | pacquet_lockfile::LockfileResolution::Variations(_),
                            )
                        {
                            skipped.add_optional_excluded(key);
                        }
                    }
                }
            }
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
        // one reached the lockfile): pnpm's runtime resolver writes
        // the chosen Node as a `node@runtime:<version>` snapshot
        // (see
        // [`engine/runtime/node-resolver`](https://github.com/pnpm/pnpm/blob/29a42efc3b/engine/runtime/node-resolver/src/index.ts)),
        // and pnpm's `engineName` helper anchors the GVS hash and the
        // side-effects-cache key prefix to that pinned Node. Mirror
        // it here — otherwise pacquet hashes under whatever
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
        );

        // The frozen path runs no resolve-time prefetcher, so the warm
        // batch owns package-status progress for store hits. An empty set
        // leaves every warm package reported as `found_in_store`.
        let progress_reported = SharedReportedProgressKeys::default();

        let phase_start = std::time::Instant::now();
        let CreateVirtualStoreOutput {
            package_manifests,
            side_effects_maps_by_snapshot,
            fetch_failed,
            cas_paths_by_pkg_id,
        } = CreateVirtualStore {
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
            workspace_root,
            node_linker,
            progress_reported: &progress_reported,
            tarball_mem_cache,
            #[cfg(test)]
            link_concurrency_probe: None,
        }
        .run::<Reporter>()
        .await
        .map_err(InstallFrozenLockfileError::CreateVirtualStore)?;
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
        // so a subsequent install retries the fetch — matches
        // upstream's behavior of not updating `opts.skipped` at the
        // catch site.
        for key in fetch_failed {
            skipped.add_fetch_failed(key);
        }

        // Pre-compute the hoist plan so the dedupe pass inside
        // `SymlinkDirectDependencies` can fold publicly-hoisted aliases
        // into root's target map — pacquet runs hoist *after*
        // `SymlinkDirectDependencies`, so without this the dedupe map
        // only sees root's direct deps and a non-root importer's
        // direct dep that would land at root via public-hoist stays
        // un-deduped. The full `HoistResult` is also threaded to the
        // on-disk hoist pass below so the BFS isn't run twice.
        let pre_hoist = compute_hoist_plan(
            config,
            snapshots,
            packages,
            importers,
            &dependency_groups,
            &skipped,
            is_hoisted,
        );
        let public_hoist_targets: Option<BTreeMap<String, PathBuf>> =
            pre_hoist.as_ref().map(|plan| {
                collect_public_hoist_targets(&plan.result, &plan.graph, &layout, &plan.skipped)
            });

        if !is_hoisted {
            SymlinkDirectDependencies {
                config,
                layout: &layout,
                importers,
                dependency_groups: dependency_groups.iter().copied(),
                workspace_root,
                skipped: &skipped,
                link_only: false,
                public_hoist_targets: public_hoist_targets.as_ref(),
            }
            .run::<Reporter>()
            .map_err(InstallFrozenLockfileError::SymlinkDirectDependencies)?;

            // Link the bins of each virtual-store slot's children into the
            // slot's own `node_modules/.bin`. Pnpm runs this from
            // `linkBinsOfDependencies` during the headless install. See
            // <https://github.com/pnpm/pnpm/blob/4750fd370c/building/during-install/src/index.ts#L258-L309>.
            // Done before `importing_done` so reporters see the import phase
            // close only after every link (including per-slot bins) is in
            // place. The manifest map threaded from `CreateVirtualStore`
            // lets the linker hit `pkgFilesIndex.manifest` directly
            // (matching pnpm's `bundledManifest`-from-CAFS path) instead
            // of re-reading every child's `package.json` from disk.
            //
            // Both passes are gated by `!is_hoisted`: under
            // `nodeLinker: hoisted` there is no virtual store
            // (`CreateVirtualStore` skipped slot writes), and the
            // bin links go into `<parent>/node_modules/.bin` for
            // every hoist location instead. The hoisted linker
            // ([`crate::link_hoisted_modules()`], called below) does
            // its own per-`node_modules` bin pass while walking the
            // hierarchy. Mirrors upstream's `nodeLinker === 'hoisted'`
            // branch at
            // <https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/index.ts#L411-L425>
            // which routes both link phases through `linkHoistedModules`.
            LinkVirtualStoreBins {
                layout: &layout,
                snapshots,
                packages,
                package_manifests: &package_manifests,
                skipped: &skipped,
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
        // Mirrors upstream's hoisted branch at
        // [`installing/deps-restorer/src/index.ts:369-427`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/index.ts#L369-L427).
        // `pkg_root_by_key` is a per-snapshot override for
        // `BuildModules`'s `pkgRoot` lookup. Populated from the
        // walker's [`crate::DependenciesGraphNode::dir`] values so
        // the build phase can `cd` into the on-disk hoisted
        // directory instead of computing a virtual-store slot path
        // that doesn't exist under hoisted. The first recorded
        // location wins for snapshots the walker emitted multiple
        // times (a single physical package nested under siblings),
        // matching upstream's
        // [`pkgRoots[0]`](https://github.com/pnpm/pnpm/blob/94240bc046/building/after-install/src/index.ts#L348)
        // pick. `None` (and an empty `hoisted_locations`) for the
        // isolated linker.
        let HoistedLinkerOutput { hoisted_locations, hoisted_pkg_root_by_key } = if is_hoisted {
            run_hoisted_linker::<Reporter>(
                HoistedLinkerInputs {
                    config,
                    lockfile,
                    current_lockfile,
                    layout: &layout,
                    importers,
                    dependency_groups: &dependency_groups,
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
        // Mirrors upstream's
        // [`hoist(...)`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/hoist/src/index.ts#L36)
        // call site at
        // [`deps-restorer:471-486`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/index.ts#L471).
        //
        // Guard mirrors upstream's `hoistPattern != null || publicHoistPattern != null`
        // — `Some(empty)` is a valid disabled state for one side but
        // not the other, so the guard checks `is_some()` on the field
        // (not `Vec` length). With pacquet's defaults both sides are
        // `Some(non-empty)`, so the pass runs by default.
        // Stashed across the hoist pass for the post-`BuildModules`
        // top-level bin link. Isolated-linker public-hoist promotes
        // a transitive dep alias to `<root>/node_modules/<alias>`
        // where it competes for the same `<root>/node_modules/.bin`
        // slot as the root importer's direct deps. Per
        // pnpm/pacquet#342 / upstream's
        // [`preferDirectCmds`](https://github.com/pnpm/pnpm/blob/4750fd370c/bins/linker/src/index.ts#L92)
        // the direct dep's bin must win. The post-build pass below
        // takes both direct + hoisted candidate lists so
        // `pacquet_cmd_shim::pick_winner` (private)'s [`BinOrigin`] tier
        // resolves the conflict in one call. Empty means there's
        // no public-hoist (no patterns set, hoisted linker, or
        // `Some(empty)`-vs-`None` short-circuit).
        let mut publicly_hoisted_for_post_build: Vec<String> = Vec::new();
        // Isolated-linker hoist pass: shamefully-hoist + private
        // hoist into the virtual store. Skipped under hoisted —
        // the hoisted linker materialized the project tree above
        // and there's no virtual store to point hoist symlinks at.
        // Mirrors upstream's behavior of leaving
        // `newHoistedDependencies = opts.hoistedDependencies` (no
        // new isolated-hoist results) under the hoisted linker
        // when no `hoistPattern` / `publicHoistPattern` is
        // configured: see
        // [`installing/deps-restorer/src/index.ts:471-486`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/index.ts#L471-L486).
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
            // resolves there even with GVS enabled
            // (upstream's `virtualStoreDir` field is
            // mutated under GVS, but pacquet keeps
            // `virtual_store_dir` project-local and
            // routes the GVS-shared root through
            // `global_virtual_store_dir` instead — see
            // [`Config::apply_global_virtual_store_derivation`]).
            // The symlink *target* (under the slot dir)
            // does need to be GVS-aware, which the
            // `VirtualStoreLayout` handle below provides.
            let private_dir = config.virtual_store_dir.join("node_modules");
            let public_dir = config.modules_dir.clone();
            symlink_hoisted_dependencies(
                &result.hoisted_dependencies_by_node_id,
                &graph,
                &layout,
                &private_dir,
                &public_dir,
                &hoist_skipped,
            )
            .map_err(InstallFrozenLockfileError::HoistSymlink)?;
            // Private-side bins → `<vs>/node_modules/.bin`.
            // Reuses the rayon-parallel `link_direct_dep_bins`
            // (same shape — read each location's
            // `package.json`, fan out to
            // `link_bins_of_packages`).
            link_direct_dep_bins(&private_dir, &result.hoisted_aliases_with_bins)
                .map_err(InstallFrozenLockfileError::HoistLinkBins)?;
            // Stash the public-hoist alias list for the
            // post-`BuildModules` top-level bin link. The
            // previous in-place `link_direct_dep_bins(&public_dir,
            // ...)` pass would have written shims with no
            // knowledge of direct-dep candidates, so a
            // hoisted bin could shadow a direct one when the
            // hoisted package's name was lexically smaller.
            // The post-build pass re-links with the
            // [`BinOrigin`] tier so direct wins outright.
            // Mirrors upstream's
            // [`linkBinsOfImporter`](https://github.com/pnpm/pnpm/blob/4750fd370c/installing/deps-installer/src/install/index.ts#L1539)
            // which runs after `buildModules`.
            publicly_hoisted_for_post_build = result.publicly_hoisted_aliases_with_bins;
            result.hoisted_dependencies
        } else {
            BTreeMap::new()
        };

        // Mirrors upstream `link.ts:167-170`: `importing_done` fires once
        // extraction and symlink linking are complete, before any build
        // phase. Reporters use it to close the import progress display so
        // subsequent `pnpm:lifecycle` events render in their own section.
        // <https://github.com/pnpm/pnpm/blob/80037699fb/installing/deps-installer/src/install/link.ts#L167>
        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: requester.to_string(),
            stage: Stage::ImportingDone,
        }));

        // `manifest_dir` (= upstream's `lockfileDir`) is the workspace
        // root threaded through `BuildModules`. Use the real `Path`
        // here rather than reconstructing it from the lossy
        // `requester` string so non-UTF-8 filenames survive intact.
        // `allow_build_policy` was already constructed up-front
        // (before `CreateVirtualStore`) on `main` so the git fetcher
        // can consult it — no second construction needed here.
        let manifest_dir: &Path = workspace_root;

        // Resolve `pnpm-workspace.yaml`'s `patchedDependencies` once
        // per install. Yields `None` when nothing is configured (no
        // yaml, no key, or empty map). Mirrors upstream's single
        // `calcPatchHashes` + `groupPatchedDependencies` call at
        // <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/installing/deps-installer/src/install/index.ts#L468-L488>.
        let patch_groups = config
            .resolved_patched_dependencies()
            .map_err(InstallFrozenLockfileError::ResolvePatchedDependencies)?;

        // Look every snapshot up against the resolved record and
        // build a per-snapshot map keyed by the peer-stripped
        // `PackageKey` (patches are configured at name+version
        // granularity, not per peer-resolution variant). `None` when
        // no patches are configured at all; an empty map when patches
        // exist but match nothing in the current install.
        //
        // Mirrors upstream's per-node lookup at
        // <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/pkg-manager/resolve-dependencies/src/resolveDependencies.ts#L1482>,
        // adapted for pacquet's lockfile-driven flow: pnpm computes
        // `node.patch` during resolution, pacquet computes it after
        // lockfile load.
        let patches: Option<HashMap<PackageKey, ExtendedPatchInfo>> =
            match (patch_groups.as_ref(), snapshots) {
                (Some(groups), Some(snaps)) => {
                    let mut map = HashMap::new();
                    for key in snaps.keys() {
                        let metadata_key = key.without_peer();
                        let metadata_key_str = metadata_key.to_string();
                        let (name, version) =
                            crate::build_modules::parse_name_version_from_key(&metadata_key_str);
                        // Propagate `ERR_PNPM_PATCH_KEY_CONFLICT` rather
                        // than silently skipping the snapshot. Upstream
                        // fails the install here so the user adds an
                        // exact-version entry to disambiguate — silently
                        // dropping the patch would leave the package
                        // unpatched (and the cache key unchanged) without
                        // any signal.
                        if let Some(info) = get_patch_info(Some(groups), &name, &version)
                            .map_err(InstallFrozenLockfileError::PatchKeyConflict)?
                        {
                            map.insert(metadata_key, info.clone());
                        }
                    }
                    Some(map)
                }
                _ => None,
            };

        // Convert `pacquet-config`'s mirror enum to the executor's
        // canonical type. Config's enum carries the yaml-deserialize
        // impl; the executor's stays free of serde wiring. See the
        // doc on [`pacquet_config::ScriptsPrependNodePath`] for the
        // rationale.
        let scripts_prepend_node_path = match config.scripts_prepend_node_path {
            pacquet_config::ScriptsPrependNodePath::Always => ExecScriptsPrependNodePath::Always,
            pacquet_config::ScriptsPrependNodePath::Never => ExecScriptsPrependNodePath::Never,
            pacquet_config::ScriptsPrependNodePath::WarnOnly => {
                ExecScriptsPrependNodePath::WarnOnly
            }
        };

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

        // BuildModules walks per-snapshot package directories and
        // runs `preinstall` / `install` / `postinstall` lifecycle
        // scripts. Under isolated, the directories live under the
        // virtual-store slot layout; under hoisted, they live at
        // the project-tree paths the slice 4 walker assigned —
        // threaded in via `pkg_root_by_key`. `gather_ancestor_bin_paths`
        // additionally reroutes `extra_bin_paths` through
        // `bin_dirs_in_all_parent_dirs` for the hoisted case so
        // lifecycle scripts can resolve binaries from every
        // ancestor `node_modules/.bin` up to `lockfile_dir` —
        // mirrors upstream's
        // [`after-install:357`](https://github.com/pnpm/pnpm/blob/94240bc046/building/after-install/src/index.ts#L357)
        // call into `binDirsInAllParentDirs`.
        let ignored_builds = BuildModules {
            layout: &layout,
            modules_dir: &config.modules_dir,
            lockfile_dir: manifest_dir,
            snapshots,
            packages,
            importers,
            allow_build_policy: &allow_build_policy,
            side_effects_maps_by_snapshot: Some(&side_effects_maps_by_snapshot),
            engine_name: engine_name.as_deref(),
            side_effects_cache: config.side_effects_cache_read(),
            side_effects_cache_write: config.side_effects_cache_write(),
            store_dir: Some(&config.store_dir),
            store_index_writer: Some(&store_index_writer),
            patches: patches.as_ref(),
            scripts_prepend_node_path,
            unsafe_perm: config.unsafe_perm,
            child_concurrency: config.child_concurrency,
            skipped: &skipped,
            pkg_root_by_key: hoisted_pkg_root_by_key.as_ref(),
            gather_ancestor_bin_paths: is_hoisted,
        }
        .run::<Reporter>()
        .map_err(InstallFrozenLockfileError::BuildModules)?;

        // Mirrors upstream's single emit at the end of the build phase:
        // <https://github.com/pnpm/pnpm/blob/80037699fb/installing/deps-installer/src/install/index.ts#L414>.
        // Always emitted (with an empty list when nothing was ignored), so
        // the reporter can display a consistent "no ignored scripts" state.
        Reporter::emit(&LogEvent::IgnoredScripts(IgnoredScriptsLog {
            level: LogLevel::Debug,
            package_names: ignored_builds,
        }));

        // Post-`BuildModules` per-importer top-level bin link
        // (pnpm/pacquet#342). Two behaviors:
        //
        // 1. **Direct over Hoisted precedence.** The earlier
        //    [`SymlinkDirectDependencies`] + isolated public-hoist
        //    bin passes wrote shims separately, so a publicly-hoisted
        //    bin could shadow a direct dep's bin with the same name
        //    when the hoisted package's name was lexically smaller.
        //    This pass collects both candidate lists into one
        //    [`link_top_level_bins`] call so
        //    `pacquet_cmd_shim::pick_winner` (private)'s
        //    [`pacquet_cmd_shim::BinOrigin::Direct`] tier resolves
        //    the conflict the way upstream's
        //    [`preferDirectCmds`](https://github.com/pnpm/pnpm/blob/4750fd370c/bins/linker/src/index.ts#L92)
        //    does.
        // 2. **Lifecycle-script-created bins.** A package's
        //    `postinstall` may write a binary file that didn't
        //    exist at extract time (the `@pnpm.e2e/generated-bins`
        //    fixture upstream uses to test this). Re-running the
        //    bin link after [`BuildModules`] re-reads each
        //    direct dep's `package.json` and shims any newly-found
        //    bin entries that point at now-existing files. Mirrors
        //    upstream's [`linkBinsOfImporter`](https://github.com/pnpm/pnpm/blob/4750fd370c/installing/deps-installer/src/install/index.ts#L1539)
        //    pass that runs after `buildModules`.
        //
        // Idempotent for unchanged shims (the
        // `is_shim_pointing_at` marker check skips writes when the
        // existing shim already targets the same bin), so the
        // double-pass overhead is bounded by the already-modest
        // per-package manifest read cost.
        let modules_dir_basename: &OsStr =
            config.modules_dir.file_name().unwrap_or_else(|| OsStr::new("node_modules"));
        for (importer_id, importer_snapshot) in importers {
            let project_dir = importer_root_dir(workspace_root, importer_id);
            let modules_dir = project_dir.join(modules_dir_basename);
            // Same filter the symlink phase used so the post-build
            // pass sees the same candidate set (skipping
            // installability-skipped deps avoids dangling shims at
            // a slot that was never extracted).
            let direct_names = direct_dep_names_for_importer(
                importer_snapshot,
                dependency_groups.iter().copied(),
                &skipped,
                false,
            );
            // Public-hoist promotes transitives into the workspace
            // root's `<root>/node_modules/<alias>`, so only the
            // root importer's `<root>/node_modules/.bin` sees
            // `BinOrigin::Hoisted` candidates. Non-root importers
            // get `<importer>/node_modules/.bin` populated only
            // from their own direct deps.
            let hoisted_names: &[String] =
                if importer_id == pacquet_lockfile::Lockfile::ROOT_IMPORTER_KEY {
                    &publicly_hoisted_for_post_build
                } else {
                    &[]
                };
            link_top_level_bins(&modules_dir, &direct_names, hoisted_names)
                .map_err(InstallFrozenLockfileError::TopLevelBinLink)?;
        }

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

        Ok(InstallFrozenLockfileOutput { hoisted_dependencies, hoisted_locations, skipped })
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
    /// Install-time skip set produced by `compute_skipped_snapshots`,
    /// seeded from the previous install's `.modules.yaml.skipped`
    /// and augmented with snapshots that newly failed the
    /// installability check.
    pub skipped: SkippedSnapshots,
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
    /// Per-snapshot `pkgRoot` override for the build phase —
    /// snapshot key → its first recorded directory in the hoisted
    /// graph. `None` for the isolated linker (the layout-based
    /// lookup in `BuildModules` is used instead).
    pub(crate) hoisted_pkg_root_by_key: Option<HashMap<PackageKey, std::path::PathBuf>>,
}

/// Inputs to [`run_hoisted_linker`]. Bundled so the two install
/// paths (`InstallFrozenLockfile` and `InstallWithFreshLockfile`)
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
/// accounting, and `pkg_root_by_key` derivation stay identical.
/// Mirrors upstream's hoisted branch at
/// [`installing/deps-restorer/src/index.ts:369-440`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/index.ts#L369-L440).
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
        walker_lockfile_dir,
        symlink_workspace_root,
        host_node,
        supported_architectures,
        cas_paths_by_pkg_id,
        logged_methods,
        requester,
    } = inputs;

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
        force: false,
        // Pacquet's [`Config`] does not yet expose `engineStrict`
        // (tracked separately); default to `false` so the walker
        // matches `compute_skipped_snapshots` upthread, which uses
        // [`crate::InstallabilityHost::detect`]'s `false` default.
        // Promotes engine mismatches to skip-optional rather than
        // hard errors, in line with pacquet's production posture.
        engine_strict: false,
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
    };
    link_hoisted_modules::<Reporter>(&link_opts).map_err(HoistedLinkerError::LinkHoistedModules)?;
    // Workspace `link:` deps still need symlinks under each importer's
    // `node_modules/<alias>` even though the regular deps now live as
    // real directories. The hoisted dep-graph walker skips
    // `workspace:`-prefixed references entirely (they're not in the
    // hoist tree), so without this pass workspace siblings would be
    // missing from each project's `node_modules/`. `link_only: true`
    // filters every other dep out so the call doesn't try to re-create
    // symlinks for packages that the hoisted linker already wrote as
    // real dirs. Mirrors upstream's hoisted branch at
    // [`installing/deps-restorer/src/index.ts:411-440`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/index.ts#L411-L440).
    SymlinkDirectDependencies {
        config,
        layout,
        importers,
        dependency_groups: dependency_groups.iter().copied(),
        workspace_root: symlink_workspace_root,
        skipped: &*skipped,
        link_only: true,
        // Hoisted-linker path has no public-hoist virtual store to
        // dedupe against; the real-directory tree is the hoist layout.
        public_hoist_targets: None,
    }
    .run::<Reporter>()
    .map_err(HoistedLinkerError::SymlinkDirectDependencies)?;
    // Map snapshot key → first recorded directory. The walker can emit
    // multiple [`crate::DependenciesGraphNode`]s with the same
    // `dep_path` when the package nests under a sibling (version
    // conflict). Postinstall scripts and the side-effects-cache key
    // both depend only on the package contents (identical across
    // locations), so running once at the first dir matches upstream's
    // `pkgRoots[0]` pick at
    // [`after-install:348`](https://github.com/pnpm/pnpm/blob/94240bc046/building/after-install/src/index.ts#L348).
    let mut pkg_root_by_key: HashMap<PackageKey, std::path::PathBuf> = HashMap::new();
    for node in walker_result.graph.values() {
        if let Ok(key) = node.dep_path.as_str().parse::<PackageKey>() {
            pkg_root_by_key.entry(key).or_insert_with(|| node.dir.clone());
        }
    }
    Ok(HoistedLinkerOutput {
        hoisted_locations: walker_result.hoisted_locations,
        hoisted_pkg_root_by_key: Some(pkg_root_by_key),
    })
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
pub(crate) fn compute_hoist_plan(
    config: &Config,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    importers: &HashMap<String, pacquet_lockfile::ProjectSnapshot>,
    dependency_groups: &[pacquet_package_manifest::DependencyGroup],
    skipped: &SkippedSnapshots,
    is_hoisted: bool,
) -> Option<HoistPlan> {
    if is_hoisted {
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
    })?;
    Some(HoistPlan { graph, result, skipped: hoist_skipped })
}

/// Build the `<alias → resolved-target-dir>` map for every publicly-
/// hoisted entry that will land in root's `node_modules/`. Pacquet
/// runs the dedupe pass before the on-disk hoist phase, so this map
/// lets the dedupe see the aliases it would otherwise miss. Mirrors
/// pnpm's `linkDirectDepsAndDedupe` semantics — when the upstream
/// linker reads `<root>/node_modules/`, the public-hoist symlinks
/// are already there because hoist ran first.
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
/// as `u32`. Used to derive the engine-name string upstream's
/// side-effects cache lookup expects without re-spawning
/// `node --version`.
fn parse_major_from_version(version: &str) -> Option<u32> {
    let after_v = version.strip_prefix('v').unwrap_or(version);
    after_v.split('.').next()?.parse().ok()
}

/// Pull the `node@runtime:<version>` major out of a lockfile's
/// `snapshots:` map, if the project pinned a runtime Node.
///
/// Pnpm v11's runtime resolver writes the pinned Node into the
/// lockfile as a snapshot with key `node@runtime:<version>` (see
/// [`engine/runtime/node-resolver`](https://github.com/pnpm/pnpm/blob/29a42efc3b/engine/runtime/node-resolver/src/index.ts#L67)).
/// Pnpm's
/// [`engineName(nodeVersion)`](https://github.com/pnpm/pnpm/blob/HEAD/engine/runtime/system-node-version/src/index.ts)
/// anchors the GVS hash and the side-effects-cache key prefix to
/// that pinned major instead of pnpm's own `process.version`. The
/// helper here is pacquet's mirror — same snapshot-scan, same
/// "first hit wins" semantics (the resolver rejects workspaces with
/// conflicting pins before they reach the lockfile).
///
/// Returns `None` when no importer pinned a runtime — callers should
/// then fall through to the host probe (`node --version` or the
/// cached `host_node`).
fn find_runtime_node_major(snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>) -> Option<u32> {
    let snapshots = snapshots?;
    for key in snapshots.keys() {
        if key.suffix.prefix() != Prefix::Runtime {
            continue;
        }
        // Pnpm currently emits `node@runtime:` only — `bun@runtime:`
        // and `deno@runtime:` exist as separate runtime kinds but
        // don't feed the Node-shaped engine string. Match the
        // upstream helper which scans for `node@runtime:` exclusively.
        if key.name.scope.is_some() || key.name.bare != "node" {
            continue;
        }
        // `Version::major` is `u64`; pnpm's major is small (<=99 in
        // practice), so the cast is lossless. The downstream
        // `engine_name` argument is `u32`, matching upstream's
        // `process.version.split('.')[0].substring(1)`-derived
        // integer.
        let major = key.suffix.version_semver()?.major;
        return Some(major as u32);
    }
    None
}

/// Read one snapshot's own `engines.runtime` Node pin from its
/// `dependencies` map. Mirrors upstream's
/// [`readSnapshotRuntimePin`](https://github.com/pnpm/pnpm/blob/HEAD/engine/runtime/system-node-version/src/index.ts):
/// the resolver desugars `engines.runtime` declared on a dep's
/// manifest into `dependencies.node: 'runtime:<version>'` (see
/// [`installing/deps-resolver/src/resolveDependencies.ts:1477-1479`](https://github.com/pnpm/pnpm/blob/29a42efc3b/installing/deps-resolver/src/resolveDependencies.ts#L1477-L1479)).
///
/// Returns the bare major when this snapshot pins its own Node, or
/// `None` when it doesn't — callers should then fall back to the
/// install-wide pin / host probe via [`find_runtime_node_major`].
///
/// Per-snapshot resolution matters because pnpm's bin linker routes
/// lifecycle-script spawns for a pinning package through that
/// package's own downloaded Node (see
/// [`bins/linker/src/index.ts:229-237`](https://github.com/pnpm/pnpm/blob/29a42efc3b/bins/linker/src/index.ts#L229-L237)).
/// Anchoring the snapshot's GVS engine hash to an install-wide value
/// would produce the wrong side-effects-cache key for cross-pinning
/// installs.
pub(crate) fn find_own_runtime_node_major(snapshot: &SnapshotEntry) -> Option<u32> {
    let deps = snapshot.dependencies.as_ref()?;
    for (alias, dep_ref) in deps {
        // Match upstream's per-snapshot extraction rule — only the
        // unscoped `node` alias counts, and only when the resolved
        // ref-value's prefix is `runtime:` (bun/deno runtimes don't
        // contribute to the Node-shaped engine string).
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

#[cfg(test)]
mod tests;
