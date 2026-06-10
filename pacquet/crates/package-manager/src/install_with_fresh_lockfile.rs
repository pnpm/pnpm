use crate::{
    AllowBuildPolicy, CreateVirtualStore, CreateVirtualStoreError, CreateVirtualStoreOutput,
    GraphToLockfileOptions, HoistedDependencies, ImporterLockfileInput,
    InstallPackageFromRegistryError, LinkVirtualStoreBins, LinkVirtualStoreBinsError,
    PrefetchContext, PrefetchingResolver, SkippedSnapshots, SymlinkDirectDependencies,
    SymlinkDirectDependenciesError, VersionPolicyError, VersionsOverrider, VirtualStoreLayout,
    dependencies_graph_to_lockfile, store_init::init_store_dir_best_effort,
};
use dashmap::DashMap;
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::Diagnostic;
use pacquet_catalogs_types::Catalogs;
use pacquet_cmd_shim::{Host, LinkBinsError, link_bins};
use pacquet_config::{Config, LinkWorkspacePackages, NodeLinker, ResolutionMode, TrustPolicy};
use pacquet_engine_runtime_bun_resolver::BunResolver;
use pacquet_engine_runtime_deno_resolver::DenoResolver;
use pacquet_engine_runtime_node_resolver::NodeResolver;
use pacquet_hooks::finder;
use pacquet_lockfile::{Lockfile, LockfileResolution, SaveLockfileError};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_reporter::{HookLog, LogEvent, LogLevel, Reporter, Stage, StageLog};
use pacquet_resolving_default_resolver::DefaultResolver;
use pacquet_resolving_deps_resolver::{
    ManifestHook, ResolveDependencyTreeError, ResolveImporterError, ResolveImporterOptions,
};
use pacquet_resolving_git_resolver::{GitResolver, RealGitProbe, RealGitRunner};
use pacquet_resolving_local_resolver::{
    LocalPathResolver, LocalResolverContext, LocalSchemeResolver,
};
use pacquet_resolving_npm_resolver::{
    InMemoryPackageMetaCache, MergeNamedRegistriesError, NamedRegistryResolver, NpmResolver,
    merge_named_registries, shared_packument_fetch_locker, shared_picked_manifest_cache,
};
use pacquet_resolving_resolver_base::{
    LatestQuery, ResolveFuture, ResolveLatestFuture, ResolveOptions, Resolver, WantedDependency,
};
use pacquet_resolving_tarball_resolver::{TarballFetchContext, TarballResolver};
use pacquet_store_dir::{SharedVerifiedFilesCache, StoreIndex, StoreIndexWriter, store_index_key};
use pacquet_tarball::{MemCache, SharedReportedProgressKeys};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::{Arc, atomic::AtomicU8},
};
use tokio::sync::watch;

/// In-memory dedup gate for packages materialized during this install.
/// Keyed by virtual-store name (`{name-with-slashes-replaced}@{version}`).
///
/// The value is a [`watch::Sender<bool>`] whose state transitions from
/// `false` (slot reserved, first writer running) to `true` (the first
/// writer's materialization is complete, `save_path` is on disk).
/// Second visitors subscribe to the sender before issuing their
/// per-parent symlink so they don't race ahead of the first writer's
/// `import_indexed_dir` — critical on Windows where `symlink_package`
/// may fall back to a junction, which requires the target directory
/// to exist at creation time. Mirrors the implicit "wait until the
/// shared slot is on disk" sequencing pnpm gets from running one
/// resolveDependencyTree pass before the install pass.
pub type ResolvedPackages = DashMap<String, watch::Sender<bool>>;

/// Fresh-install path: resolve the project from the registry, fetch +
/// materialize `node_modules`, and emit a brand-new `pnpm-lock.yaml`
/// reflecting the resolved graph. Caller (see [`crate::Install::run`])
/// drives this path whenever no `--frozen-lockfile` was requested.
///
/// **Brief overview for each package:**
/// * Resolve every importer's dependency through the [`NpmResolver`] chain
///   (`resolve_workspace` builds the per-importer trees, runs the
///   cross-importer peer pass, and applies `dedupeInjectedDeps`).
/// * Fetch a tarball of each resolved package and extract it into the
///   store directory.
/// * Import (by reflink, hardlink, or copy) the files from the store
///   dir to `node_modules/.pacquet/{name}@{version}/node_modules/{name}/`.
/// * Create dependency symbolic links in
///   `node_modules/.pacquet/{name}@{version}/node_modules/`.
/// * Create a symbolic link at `node_modules/{name}`.
/// * Run the resolved graph through
///   [`crate::dependencies_graph_to_lockfile()`] to produce a v9
///   `pnpm-lock.yaml`; the caller writes it to `<lockfile_dir>/pnpm-lock.yaml`.
#[must_use]
pub struct InstallWithFreshLockfile<'a, DependencyGroupList> {
    /// Shared in-memory tarball cache. Held behind [`Arc`] so the
    /// resolve-time prefetcher ([`PrefetchingResolver`]) can capture
    /// an owned clone into the background download task spawned for
    /// each fresh resolution while the install-side per-package call
    /// in `install_subtree` still takes `&MemCache` via deref.
    pub tarball_mem_cache: Arc<MemCache>,
    pub resolved_packages: &'a ResolvedPackages,
    pub http_client: &'a ThrottledClient,
    /// Same client behind an [`Arc`] for the [`NpmResolver`], whose
    /// stored `ThrottledClient` outlives any per-call borrow.
    pub http_client_arc: Arc<ThrottledClient>,
    pub config: &'static Config,
    /// One entry per importer to resolve, keyed by the lockfile
    /// importer id (`"."` for the workspace root, POSIX-relative path
    /// for sibling projects — see
    /// [`pacquet_workspace::importer_id_from_root_dir`]). Mirrors
    /// upstream's `importers: ImporterToResolve[]` shape on
    /// `resolveDependencies`. For a non-workspace install this carries
    /// a single `"."` entry pointing at the only project.
    pub importer_manifests: BTreeMap<String, &'a PackageManifest>,
    pub dependency_groups: DependencyGroupList,
    /// Install-scoped dedupe state for `pnpm:package-import-method`.
    /// See `link_file::log_method_once`.
    pub logged_methods: &'a AtomicU8,
    /// Install root, threaded into reporter `requester` fields.
    pub requester: &'a str,
    /// Catalogs parsed from `pnpm-workspace.yaml`. Empty for projects
    /// without a workspace manifest.
    pub catalogs: Catalogs,
    /// Lockfile root for the install, used by the resolver chain to
    /// compute `link:` / `file:` relative paths and to anchor
    /// workspace-package resolution. Mirrors upstream's `lockfileDir`
    /// argument on `resolveDependencies`. Equal to the manifest's
    /// parent directory under single-project installs and to the
    /// `pnpm-workspace.yaml` root under monorepos.
    pub lockfile_dir: &'a Path,
    /// Workspace-sibling lookup the [`NpmResolver`] consults when it
    /// sees a `workspace:` spec. `None` when this install isn't inside
    /// a `pnpm-workspace.yaml` workspace; the resolver then errors out
    /// on any `workspace:` spec via
    /// `ResolveFromWorkspaceError::WorkspacePackagesNotLoaded` —
    /// matching pnpm's
    /// [`Cannot resolve package from workspace because opts.workspacePackages is not defined`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L828-L830)
    /// behavior.
    pub workspace_packages: Option<pacquet_resolving_resolver_base::WorkspacePackages>,
    /// Refresh locked integrity values from the registry. Threaded
    /// into [`ResolveOptions::update_checksums`] so the picker bypasses
    /// its in-memory and on-disk metadata caches and always goes to
    /// the registry with conditional headers.
    pub update_checksums: bool,
    /// Existing `pnpm-lock.yaml` to seed `getPreferredVersionsFromLockfileAndManifests`
    /// with already-pinned `(name, version)` pairs. `Some` on the
    /// stale-lockfile / `preferFrozenLockfile: false` rewrite path
    /// — the resolver biases toward the seeded versions when they
    /// still satisfy the spec so unrelated dependencies keep their
    /// pins. `None` on the no-lockfile path. Mirrors upstream's
    /// `update: false` resolver mode at
    /// <https://github.com/pnpm/pnpm/blob/097983fbca/lockfile/preferred-versions/src/index.ts#L13-L33>.
    pub wanted_lockfile: Option<&'a Lockfile>,
    /// Per-install packument cache shared with the lockfile-verifier
    /// constructed in [`Install::run`](crate::Install::run). The
    /// resolver writes to it during `pick_package`; the verifier reads
    /// from it to skip duplicate fetches when both touch the same
    /// `(registry, name)`.
    pub meta_cache: Arc<InMemoryPackageMetaCache>,
    /// Resolved [`pacquet_config::Config::node_linker`]. Selects the
    /// materialization shape after the virtual store is populated:
    /// under [`NodeLinker::Hoisted`] the freshly-built lockfile is
    /// routed through [`crate::lockfile_to_hoisted_dep_graph`] +
    /// [`crate::link_hoisted_modules()`] instead of the isolated
    /// symlink layout.
    pub node_linker: NodeLinker,
    /// CLI-merged `supportedArchitectures` (`pnpm-workspace.yaml` +
    /// `--cpu`/`--os`/`--libc`). Threaded into the hoisted-linker
    /// walker so its installability filter honors user-supplied
    /// accept lists. `None` when no architectures are configured.
    pub supported_architectures: Option<&'a pacquet_package_is_installable::SupportedArchitectures>,
    /// When `true`, resolve the graph and write `pnpm-lock.yaml`, then
    /// return — skipping the tarball prefetch, virtual-store
    /// materialization, symlinks, hoisting, and bin linking. The store
    /// stays untouched (no tarball is fetched), matching pnpm's
    /// `dryRun: opts.lockfileOnly` resolve pass. See
    /// [`crate::Install::lockfile_only`].
    pub lockfile_only: bool,
    /// Which lockfile pins to withhold from the preferred-versions seed
    /// so the affected names re-resolve to the highest version
    /// satisfying their manifest range. Drives `pacquet update`'s
    /// compatible bump; see [`UpdateSeedPolicy`].
    pub update_seed_policy: UpdateSeedPolicy,
    /// Per-invocation `Authorization`-header override; `None` uses
    /// `config.auth_headers`. See [`crate::Install::auth_override`].
    pub auth_override: Option<Arc<AuthHeaders>>,
    /// Sink notified for each resolved tarball package as the tree walk
    /// yields it. `None` for every local install; the pnpr server sets
    /// one. See [`crate::Install::resolution_observer`].
    pub resolution_observer: Option<Arc<dyn crate::ResolutionObserver>>,
}

/// Which lockfile-pinned `(name, version)` pairs to *withhold* from the
/// preferred-versions tie-break seed [`InstallWithFreshLockfile`] builds
/// via `get_preferred_versions_from_lockfile_and_manifests`.
///
/// A name whose pin is withheld no longer carries its previously-locked
/// version at the existing-version weight, so the resolver falls back to
/// picking the highest version satisfying the manifest range — the
/// compatible re-resolution `pacquet update` performs. Mirrors pnpm's
/// `update: 'compatible'` resolver mode, which ignores the lockfile
/// version for the dependency being updated
/// (<https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L823-L827>).
///
/// `KeepAll` is the install/add default (every pin seeds the table, so
/// unrelated entries keep their resolutions on a rewrite).
#[derive(Debug, Default, Clone)]
pub enum UpdateSeedPolicy {
    /// Seed every lockfile pin. `pacquet install` / `pacquet add`.
    #[default]
    KeepAll,
    /// Withhold every lockfile pin. `pacquet update` with no package
    /// selectors — the whole graph re-resolves to highest-in-range.
    DropAll,
    /// Withhold only the named packages' pins. `pacquet update <pattern>`
    /// — matched names (at any depth) re-resolve while everything else
    /// keeps its pin. Keyed by package name (scope included).
    DropOnly(std::collections::HashSet<String>),
}

/// Error type of [`InstallWithFreshLockfile`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum InstallWithFreshLockfileError {
    #[diagnostic(transparent)]
    InstallPackageFromRegistry(#[error(source)] InstallPackageFromRegistryError),

    #[diagnostic(transparent)]
    CreateVirtualStore(#[error(source)] CreateVirtualStoreError),

    #[diagnostic(transparent)]
    SymlinkDirectDependencies(#[error(source)] SymlinkDirectDependenciesError),

    /// Surfaces failures from [`crate::lockfile_to_hoisted_dep_graph`]
    /// when a fresh install runs under `nodeLinker: hoisted`. Same
    /// shape the frozen-lockfile path surfaces — see
    /// `InstallFrozenLockfileError::HoistedDepGraph`.
    #[diagnostic(transparent)]
    HoistedDepGraph(#[error(source)] crate::HoistedDepGraphError),

    /// Surfaces failures from [`crate::link_hoisted_modules()`] while
    /// materializing the on-disk hoisted tree on the fresh path. Same
    /// shape the frozen-lockfile path surfaces — see
    /// `InstallFrozenLockfileError::LinkHoistedModules`.
    #[diagnostic(transparent)]
    LinkHoistedModules(#[error(source)] crate::LinkHoistedModulesError),

    #[diagnostic(transparent)]
    LinkBins(#[error(source)] LinkBinsError),

    /// Surfaces a failure to create one of the hoist symlinks
    /// (`<private_hoisted_modules_dir>/<alias>` or
    /// `<public_hoisted_modules_dir>/<alias>`). EEXIST is
    /// already swallowed by the hoist helper, so this only fires
    /// on real I/O failures.
    #[diagnostic(transparent)]
    HoistSymlink(#[error(source)] crate::SymlinkPackageError),

    /// Surfaces a failure to link bins of privately-hoisted aliases
    /// into the virtual-store-local `<vs>/node_modules/.bin`.
    #[diagnostic(transparent)]
    HoistLinkBins(#[error(source)] LinkBinsError),

    #[diagnostic(transparent)]
    LinkVirtualStoreBins(#[error(source)] LinkVirtualStoreBinsError),

    /// The resolver chain failed for at least one dependency. Mirrors
    /// upstream's per-dep resolver error surface — the inner message
    /// carries the boxed error's `Display`.
    #[display("Failed to resolve dependency tree: {_0}")]
    #[diagnostic(code(pacquet_package_manager::resolve_dependency_tree))]
    ResolveDependencyTree(#[error(not(source))] ResolveDependencyTreeError),

    /// The hoist-loop orchestrator failed. Wraps the tree-walk error
    /// (the only failure source today) plus any future orchestrator-
    /// specific failures.
    #[display("Failed to resolve importer: {_0}")]
    #[diagnostic(code(pacquet_package_manager::resolve_importer))]
    ResolveImporter(#[error(not(source))] ResolveImporterError),

    /// `minimumReleaseAgeExclude` patterns rejected at compile time.
    /// Mirrors upstream's `ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE`.
    #[display("Invalid value in minimumReleaseAgeExclude: {_0}")]
    #[diagnostic(code(ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE))]
    MinimumReleaseAgeExclude(#[error(source)] pacquet_config::version_policy::VersionPolicyError),

    /// `trustPolicyExclude` patterns rejected at compile time.
    /// Mirrors upstream's `ERR_PNPM_INVALID_TRUST_POLICY_EXCLUDE`.
    #[display("Invalid value in trustPolicyExclude: {_0}")]
    #[diagnostic(code(ERR_PNPM_INVALID_TRUST_POLICY_EXCLUDE))]
    TrustPolicyExclude(#[error(source)] pacquet_config::version_policy::VersionPolicyError),

    /// `allowBuilds` patterns in `pnpm-workspace.yaml` couldn't be
    /// parsed. Same `VersionPolicyError` shape the frozen-lockfile
    /// path surfaces — see `InstallFrozenLockfileError::VersionPolicy`
    /// for the upstream reference.
    #[diagnostic(transparent)]
    AllowBuildsPolicy(#[error(source)] VersionPolicyError),

    /// Failed to resolve and hash `patchedDependencies` against the
    /// workspace directory.
    #[diagnostic(transparent)]
    ResolvePatchedDependencies(#[error(source)] pacquet_patching::ResolvePatchedDependenciesError),

    /// Failed to read or hash a patch file when computing the
    /// lockfile's top-level `patchedDependencies` block.
    #[diagnostic(transparent)]
    CalcPatchHashes(#[error(source)] pacquet_patching::CalcPatchHashError),

    /// A user-defined `namedRegistries` entry mapped an alias to a
    /// non-http(s) URL. Surfaced at resolver construction so the
    /// install fails fast with a specific error code instead of a
    /// downstream 404. Mirrors upstream's
    /// `ERR_PNPM_INVALID_NAMED_REGISTRY_URL`.
    #[diagnostic(transparent)]
    InvalidNamedRegistry(#[error(source)] MergeNamedRegistriesError),

    /// A `packageExtensions` selector's `@<range>` half failed to
    /// parse as a `node-semver` range. Mirrors upstream's behavior —
    /// pnpm hands the raw range to `semver.satisfies`, which throws
    /// a `TypeError` on a malformed range; pacquet surfaces the same
    /// failure mode earlier (install start, not first per-manifest
    /// match) so the user sees the bad selector before any tarballs
    /// are fetched.
    #[diagnostic(transparent)]
    InvalidPackageExtensionSelector(
        #[error(source)] crate::package_extender::InvalidPackageExtensionSelector,
    ),

    /// A value in `pnpm.overrides` couldn't be parsed before the
    /// fresh resolver's read-package hook was built.
    #[diagnostic(transparent)]
    InvalidOverrides(#[error(source)] pacquet_config_parse_overrides::ParseOverridesError),

    /// The first writer of a shared `(name, version)` slot dropped its
    /// completion signal without sending `true`. In practice this only
    /// fires when the first writer's task panicked / was cancelled
    /// mid-import; a second visitor that was waiting on the slot can't
    /// safely create its per-parent symlink (the virtual-store target
    /// directory may not exist), so the install fails closed.
    #[display(
        "First writer for virtual-store slot {virtual_store_name} dropped before signalling completion"
    )]
    #[diagnostic(code(pacquet_package_manager::first_writer_aborted))]
    FirstWriterAborted {
        #[error(not(source))]
        virtual_store_name: String,
    },

    /// Persisting the freshly-resolved `pnpm-lock.yaml` failed. Surfaced
    /// rather than swallowed because a missing wanted lockfile would
    /// force the next install to re-resolve every dep and would break
    /// the `pnpm install --frozen-lockfile` headless path.
    #[diagnostic(transparent)]
    SaveWantedLockfile(#[error(source)] SaveLockfileError),

    /// The `afterAllResolved` pnpmfile hook threw or otherwise failed.
    /// Mirrors pnpm, where a throwing `afterAllResolved` aborts the install.
    #[display("{_0}")]
    #[diagnostic(code(PNPMFILE_FAIL))]
    AfterAllResolvedHook(#[error(not(source))] pacquet_hooks::HookError),

    /// The freshly-built lockfile could not be serialized to JSON to pass to
    /// the `afterAllResolved` pnpmfile hook.
    #[display("Failed to serialize lockfile for the afterAllResolved hook: {_0}")]
    #[diagnostic(code(pacquet_package_manager::after_all_resolved_serialize))]
    AfterAllResolvedSerialize(#[error(source)] serde_json::Error),
}

impl From<crate::install_frozen_lockfile::HoistedLinkerError> for InstallWithFreshLockfileError {
    fn from(error: crate::install_frozen_lockfile::HoistedLinkerError) -> Self {
        use crate::install_frozen_lockfile::HoistedLinkerError;
        match error {
            HoistedLinkerError::HoistedDepGraph(error) => {
                InstallWithFreshLockfileError::HoistedDepGraph(error)
            }
            HoistedLinkerError::LinkHoistedModules(error) => {
                InstallWithFreshLockfileError::LinkHoistedModules(error)
            }
            HoistedLinkerError::SymlinkDirectDependencies(error) => {
                InstallWithFreshLockfileError::SymlinkDirectDependencies(error)
            }
        }
    }
}

/// Output of [`InstallWithFreshLockfile::run`].
///
/// Returns the hoist-graph slot the dispatch already consumed plus the
/// freshly-built [`Lockfile`] (when the writer ran), so the caller can
/// save it as `<virtual_store_dir>/lock.yaml` after `.modules.yaml`
/// succeeds — the same ordering the frozen-lockfile path uses to
/// guarantee a manifest failure can't leave a current-lockfile
/// pointing at incomplete install state.
#[must_use]
pub struct InstallWithFreshLockfileResult {
    pub hoisted_dependencies: HoistedDependencies,
    /// Per-depPath list of lockfile-relative directory paths the
    /// hoisted linker placed each package at. Empty under the
    /// isolated linker (the field is hoisted-only on disk). The
    /// caller persists it into
    /// [`pacquet_modules_yaml::Modules::hoisted_locations`] so a
    /// follow-up install or rebuild can locate every package without
    /// re-running the walker.
    pub hoisted_locations: BTreeMap<String, Vec<String>>,
    /// `Some` when the install resolved a graph that was written to
    /// `pnpm-lock.yaml`; `None` when the write was skipped (today: only
    /// `config.lockfile=false`). The caller mirrors the same gate when
    /// deciding whether to persist the current-lockfile.
    pub wanted_lockfile: Option<Lockfile>,
    /// `true` when the wanted lockfile written to disk is the same
    /// typed lockfile returned in [`Self::wanted_lockfile`]. A
    /// non-null `afterAllResolved` hook result can mutate fields the
    /// typed model tracks, so the caller must not record a verification
    /// cache entry for that case.
    pub can_record_lockfile_verification: bool,
}

impl<DependencyGroupList> InstallWithFreshLockfile<'_, DependencyGroupList> {
    /// Execute the subroutine.
    ///
    /// Under the isolated linker the [`HoistedDependencies`] result
    /// carries the publicly/privately-hoisted alias map; under
    /// `nodeLinker: hoisted` it is empty (the hoisted linker writes the
    /// on-disk tree directly and reports its placements through
    /// [`InstallWithFreshLockfileResult::hoisted_locations`] instead).
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
    ) -> Result<InstallWithFreshLockfileResult, InstallWithFreshLockfileError>
    where
        DependencyGroupList: IntoIterator<Item = DependencyGroup>,
    {
        let InstallWithFreshLockfile {
            tarball_mem_cache,
            http_client,
            http_client_arc,
            config,
            importer_manifests,
            dependency_groups,
            // The recursive `install_subtree` path used this `DashMap`
            // as a per-snapshot watch-channel dedup gate so duplicate
            // visitors of the same slot could await the first writer's
            // materialisation. The refactored pipeline routes through
            // `CreateVirtualStore`, whose warm/cold-batch shape dedups
            // by snapshot key inside the rayon pass instead, so this
            // map is no longer consulted. Kept on the struct so
            // `Install::run` can pass `&Default::default()` without
            // breaking the call sites that already supply it — a
            // follow-up can prune it once the per-test setup is
            // simplified.
            resolved_packages: _,
            logged_methods,
            requester,
            catalogs,
            lockfile_dir,
            workspace_packages,
            update_checksums,
            wanted_lockfile,
            meta_cache,
            node_linker,
            supported_architectures,
            lockfile_only,
            update_seed_policy,
            auth_override,
            resolution_observer,
        } = self;

        // The pnpr override when supplied, else the config's npmrc headers;
        // shared by every registry-touching resolver below.
        let auth_headers = auth_override.unwrap_or_else(|| Arc::clone(&config.auth_headers));
        let is_hoisted = matches!(node_linker, NodeLinker::Hoisted);
        // Materialise the caller's iterator into a `Vec` so the same
        // group set can be replayed into both the resolver (consumes
        // the iterator) and `SymlinkDirectDependencies` (needs to walk
        // each importer's per-group dep list again). Mirrors the
        // `dependency_groups.into_iter().collect()` shape
        // `install_frozen_lockfile.rs` uses for the same reason.
        // `Vec<DependencyGroup>` is at most a few enum variants so the
        // clone cost is negligible.
        let dependency_groups: Vec<DependencyGroup> = dependency_groups.into_iter().collect();

        let store_dir: &'static _ = &config.store_dir;

        // Eagerly create `files/00..ff` under the v11 store root so per-
        // tarball CAFS writes never pay a `create_dir_all` syscall on the
        // hot path. Ports pnpm's `initStore` in `worker/src/start.ts`.
        // See [`init_store_dir_best_effort`] for the error-degradation
        // policy shared with `create_virtual_store.rs`.
        init_store_dir_best_effort(store_dir).await;

        // Resolve pass: walk the manifest's dependencies through the
        // npm resolver chain and produce a flat tree keyed by
        // `name@version`. The meta cache is owned for the duration of
        // this call so every per-package resolve reuses a single
        // packument per `(registry, name)` pair, then dropped before
        // the install pass begins.
        let mut registries = HashMap::new();
        registries.insert("default".to_string(), config.registry.clone());

        // User-supplied named-registry aliases from
        // `pnpm-workspace.yaml#namedRegistries`. `merge_named_registries`
        // validates each URL up front and folds in pacquet's built-in
        // aliases (today: `gh:` → GitHub Packages); a malformed URL
        // here aborts the install with `ERR_PNPM_INVALID_NAMED_REGISTRY_URL`,
        // matching upstream's
        // [`mergeNamedRegistries`](https://github.com/pnpm/pnpm/blob/b61e268d57/resolving/npm-resolver/src/index.ts#L642-L656).
        let user_named_registries: HashMap<String, String> =
            config.named_registries.iter().map(|(name, url)| (name.clone(), url.clone())).collect();
        let merged_named_registries = merge_named_registries(&user_named_registries)
            .map_err(InstallWithFreshLockfileError::InvalidNamedRegistry)?;
        let named_registry_aliases: std::collections::HashSet<String> =
            merged_named_registries.keys().cloned().collect();

        // `resolutionMode` derivations. `time_based` and
        // `pick_lowest_direct` steer the deps-resolver's per-depth
        // version pick; `full_metadata` forces the npm resolver to
        // fetch per-version `time` fields so the time-based cutoff (and
        // the no-downgrade trust check) have publication dates to work
        // with. Mirrors pnpm's
        // [`fullMetadata`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/store/connection-manager/src/createNewStoreController.ts#L69-L74)
        // expression: `(time-based || no-downgrade) && !registrySupportsTimeField`.
        let time_based = config.resolution_mode == ResolutionMode::TimeBased;
        let pick_lowest_direct = config.resolution_mode.picks_lowest_direct();
        let full_metadata = (time_based || config.trust_policy == TrustPolicy::NoDowngrade)
            && !config.registry_supports_time_field;

        // One per-cache-key packument fetch serializer shared between
        // the npm and named-registry resolvers. Ports upstream's
        // [`metafileOperationLimits`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L42-L44):
        // concurrent picks for the same `(registry, name)` coalesce
        // into a single network fetch instead of firing N parallel
        // HTTP GETs queued behind the `ThrottledClient` semaphore.
        let fetch_locker = shared_packument_fetch_locker();

        // One per-`(name, version)` JSON manifest cache shared between
        // the npm and named-registry resolvers, so duplicate picks of
        // the same package version reuse the already-serialised
        // `Arc<Value>` instead of re-running `serde_json::to_value` for
        // every occurrence of a shared dep in the tree.
        let picked_manifest_cache = shared_picked_manifest_cache();

        // Open the read-only SQLite index, spawn the batched writer, and
        // allocate the install-scoped `verifiedFilesCache` *before* the
        // resolver chain is built. Both the `TarballResolver` (which
        // fetches a remote tarball direct dep during resolution to learn
        // its name/version/integrity) and the [`PrefetchingResolver`]
        // need these at construction time so the store-index / writer /
        // verify cache they touch is the same one the install pass uses
        // once resolution is done. Mirrors pnpm's `packageRequester`
        // shape: the fetch begins as soon as the resolver returns,
        // before any further tree walk.
        let store_index =
            match tokio::task::spawn_blocking(move || StoreIndex::shared_readonly_in(store_dir))
                .await
            {
                Ok(store_index) => store_index,
                Err(error) => {
                    tracing::warn!(
                        target: "pacquet::install",
                        ?error,
                        "store-index open task failed; continuing without a shared cache index",
                    );
                    None
                }
            };
        let store_index_ref = store_index.as_ref();

        let (store_index_writer, writer_task) = StoreIndexWriter::spawn(store_dir);

        let verified_files_cache = SharedVerifiedFilesCache::default();

        // Records package-status progress emitted by resolve-time
        // prefetches. `CreateVirtualStore` still emits `resolved` later,
        // but skips duplicate `fetched` / `found_in_store` statuses for
        // keys already reported here.
        let progress_reported = SharedReportedProgressKeys::default();

        let npm_resolver: Arc<dyn Resolver> = Arc::new(NpmResolver {
            registries,
            named_registries: merged_named_registries.clone(),
            http_client: Arc::clone(&http_client_arc),
            auth_headers: Arc::clone(&auth_headers),
            meta_cache: Arc::clone(&meta_cache),
            fetch_locker: Arc::clone(&fetch_locker),
            picked_manifest_cache: Arc::clone(&picked_manifest_cache),
            cache_dir: Some(config.cache_dir.clone()),
            offline: config.offline,
            prefer_offline: config.prefer_offline,
            ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
            // Abbreviated metadata at resolve time unless `time-based`
            // resolution or the `no-downgrade` trust policy needs the
            // per-version `time` field (and the registry doesn't serve
            // it in abbreviated form). When `false`, [`pick_package`]
            // still upgrades per-call where `published_by` / `optional`
            // demand it. Mirrors upstream's
            // [`fullMetadata`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/store/connection-manager/src/createNewStoreController.ts#L69-L74).
            full_metadata,
            retry_opts: crate::retry_config::retry_opts_from_config(config),
        });
        let git_resolver = GitResolver::new(
            Arc::new(RealGitProbe::new(Arc::clone(&http_client_arc))),
            Arc::new(RealGitRunner::new()),
        );
        // A remote (non-registry) tarball *direct* dependency carries no
        // name/version/integrity at resolve time — they live in the
        // tarball's `package.json`. The resolver downloads + extracts it
        // here (warming `tarball_mem_cache` keyed by URL) so the lockfile
        // builder gets the manifest + integrity and the install pass
        // reuses the extraction without a second download. Wired in both
        // the materializing and `--lockfile-only` paths: the lockfile
        // needs the integrity regardless of whether `node_modules` is
        // built.
        // Map every remote-tarball URL the prior lockfile recorded (with an
        // integrity) to its `<integrity>\t<pkg_id>` store-index key, keyed
        // exactly as `snapshot_cache_key` / the install pass address the row.
        // The `TarballResolver` uses it to reuse a warm store entry instead
        // of re-downloading on re-resolution. Git-hosted tarballs are skipped
        // (they key by `gitHostedStoreIndexKey`, not the integrity) and just
        // re-fetch as before. Empty on a first install.
        let prior_tarball_entries: HashMap<String, (ssri::Integrity, String)> = wanted_lockfile
            .and_then(|lockfile| lockfile.packages.as_ref())
            .map(|packages| {
                packages
                    .iter()
                    .filter_map(|(key, metadata)| match &metadata.resolution {
                        LockfileResolution::Tarball(t) if t.git_hosted != Some(true) => {
                            let integrity = t.integrity.clone()?;
                            let cache_key =
                                store_index_key(&integrity.to_string(), &key.to_string());
                            Some((t.tarball.clone(), (integrity, cache_key)))
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let tarball_resolver = TarballResolver {
            http_client: Arc::clone(&http_client_arc),
            fetch_context: Some(TarballFetchContext {
                store_dir,
                store_index_writer: Some(Arc::clone(&store_index_writer)),
                mem_cache: Some(Arc::clone(&tarball_mem_cache)),
                auth_headers: Arc::clone(&auth_headers),
                retry_opts: crate::retry_config::retry_opts_from_config(config),
                store_index: store_index.clone(),
                verify_store_integrity: config.verify_store_integrity,
                verified_files_cache: Arc::clone(&verified_files_cache),
                prior_tarball_entries: Arc::new(prior_tarball_entries),
            }),
        };
        // `preserveAbsolutePaths` is wired through `Config`; thread the
        // current value into the local-resolver context so absolute
        // `file:` / `link:` specs round-trip the same shape upstream
        // produces under the matching `--config.preserve-absolute-paths`
        // setting. Pacquet doesn't expose `preserveAbsolutePaths` yet,
        // so the context defaults to `false`.
        let local_ctx = LocalResolverContext { preserve_absolute_paths: false };
        let local_scheme_resolver = LocalSchemeResolver::new(local_ctx);
        let local_path_resolver = LocalPathResolver::new(local_ctx);
        let mut node_resolver = NodeResolver::new(Arc::clone(&http_client_arc));
        node_resolver.offline = config.offline;
        let deno_resolver =
            DenoResolver::new(Arc::clone(&http_client_arc), Arc::clone(&npm_resolver));
        let bun_resolver =
            BunResolver::new(Arc::clone(&http_client_arc), Arc::clone(&npm_resolver));
        let named_registry_resolver = NamedRegistryResolver {
            named_registries: merged_named_registries,
            registry_names: named_registry_aliases,
            http_client: Arc::clone(&http_client_arc),
            auth_headers: Arc::clone(&auth_headers),
            meta_cache: Arc::clone(&meta_cache),
            fetch_locker: Arc::clone(&fetch_locker),
            picked_manifest_cache: Arc::clone(&picked_manifest_cache),
            cache_dir: Some(config.cache_dir.clone()),
            offline: config.offline,
            prefer_offline: config.prefer_offline,
            ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
            // Same rationale as `NpmResolver.full_metadata` above.
            full_metadata,
            retry_opts: crate::retry_config::retry_opts_from_config(config),
        };
        // Order mirrors upstream's chain at
        // <https://github.com/pnpm/pnpm/blob/1627943d2a/resolving/default-resolver/src/index.ts#L128-L147>:
        // npm → jsr (folded into npm) → git → tarball → localScheme →
        // node → deno → bun → namedRegistry → localPath. The
        // local-resolver split is required by named-registry: a
        // `<alias>:@scope/pkg` specifier carries an embedded `/`,
        // which the path-shape detector
        // (`contains_path_sep` in `parse_bare_specifier.rs`) would
        // otherwise claim and prevent the named-registry resolver
        // from running.
        let inner_resolver: Box<dyn Resolver> = Box::new(DefaultResolver::new(vec![
            Box::new(ArcResolver(Arc::clone(&npm_resolver))),
            Box::new(git_resolver),
            Box::new(tarball_resolver),
            Box::new(local_scheme_resolver),
            Box::new(node_resolver),
            Box::new(deno_resolver),
            Box::new(bun_resolver),
            Box::new(named_registry_resolver),
            Box::new(local_path_resolver),
        ]));

        // Wrap the resolver chain so each tarball-shaped result fires a
        // background download into `tarball_mem_cache` while the
        // deps-resolver continues to walk the tree. The install pass
        // later calls `DownloadTarballToStore::run_with_mem_cache` for
        // the same URLs and either picks up `CacheValue::Available`
        // immediately or briefly blocks on the per-URL `Notify`. The
        // wrapper is generic over `R: Reporter` so the spawned
        // download's `pnpm:progress` emits route through the same
        // reporter the install pass uses. See
        // `prefetching_resolver.rs` for the full design rationale.
        //
        // Skipped entirely under `--lockfile-only`: that path writes only
        // `pnpm-lock.yaml` and must not touch the store, so resolution
        // runs through the bare chain with no background download.
        // Mirrors pnpm's `dryRun: opts.lockfileOnly` at
        // <https://github.com/pnpm/pnpm/blob/a33c4bfcb0/installing/deps-installer/src/install/index.ts#L1432>.
        let resolver: Box<dyn Resolver> = if lockfile_only {
            inner_resolver
        } else {
            Box::new(PrefetchingResolver::<Reporter>::new(
                inner_resolver,
                PrefetchContext {
                    http_client: &http_client_arc,
                    mem_cache: &tarball_mem_cache,
                    store_index: store_index_ref,
                    store_index_writer: Some(&store_index_writer),
                    verified_files_cache: &verified_files_cache,
                    config,
                    requester,
                    progress_reported: &progress_reported,
                },
            ))
        };

        // The pnpr server resolves `--lockfile-only` and reports each
        // resolved tarball to the client as it lands, so the client can
        // fetch in parallel with the server's resolution. Wrap the chain
        // last so the observer sees every resolve regardless of whether
        // the prefetcher above is in play (it isn't under
        // `--lockfile-only`, which is the pnpr resolve path). A no-op for
        // every local install (`resolution_observer` is `None`).
        let resolver: Box<dyn Resolver> = match resolution_observer {
            Some(observer) => Box::new(crate::ObservingResolver::new(resolver, observer)),
            None => resolver,
        };

        // Compile `minimumReleaseAge` (and its exclude pattern set)
        // for the resolve pass. Mirrors the verifier wiring in
        // `build_resolution_verifiers` so the resolver-time pick and
        // the lockfile-verification check enforce the same policy.
        //
        // Every step uses checked arithmetic so an absurd configured
        // value (e.g. `u64::MAX`) can't wrap on the `u64 → i64` cast,
        // overflow inside `chrono::Duration`, or underflow the
        // wall-clock subtraction. On overflow we leave the policy
        // inactive for this install — better than silently producing
        // a cutoff in the wrong direction.
        let published_by = config.resolved_minimum_release_age().and_then(|minutes| {
            let duration = chrono::Duration::try_minutes(i64::try_from(minutes).ok()?)?;
            chrono::Utc::now().checked_sub_signed(duration)
        });
        let published_by_exclude = config
            .minimum_release_age_exclude
            .as_deref()
            .filter(|patterns| !patterns.is_empty())
            .map(pacquet_config::version_policy::create_package_version_policy)
            .transpose()
            .map_err(InstallWithFreshLockfileError::MinimumReleaseAgeExclude)?;

        // `trustPolicy='no-downgrade'` config, threaded into every
        // resolve so the npm resolver re-applies the downgrade gate to
        // freshly picked versions. `full_metadata` above is already
        // forced on under this policy, so the picker hands the resolver
        // the per-version `time` + trust evidence the check reads.
        let trust_policy = match config.trust_policy {
            TrustPolicy::Off => None,
            TrustPolicy::NoDowngrade => Some(TrustPolicy::NoDowngrade),
        };
        let trust_policy_exclude = config
            .trust_policy_exclude
            .as_deref()
            .filter(|patterns| !patterns.is_empty())
            .map(pacquet_config::version_policy::create_package_version_policy)
            .transpose()
            .map_err(InstallWithFreshLockfileError::TrustPolicyExclude)?;

        let parsed_overrides = parse_config_overrides(config, &catalogs)?;
        let resolved_overrides = parsed_overrides.as_deref().map(resolved_overrides_map);

        // Build pnpm's built-in read-package hook chain for manifests
        // that fresh resolution consumes. The order matches
        // `createReadPackageHook`: packageExtensions first, overrides
        // after that. The pnpmfile readPackage hook is still threaded
        // separately and runs after this built-in hook in the resolver.
        let package_extender = match config.package_extensions.as_ref() {
            Some(extensions) => {
                let extender = crate::PackageExtender::new(extensions)
                    .map_err(InstallWithFreshLockfileError::InvalidPackageExtensionSelector)?;
                (!extender.is_empty()).then(|| Arc::new(extender))
            }
            None => None,
        };
        let versions_overrider = parsed_overrides
            .as_ref()
            .map(|parsed| Arc::new(VersionsOverrider::new(parsed, lockfile_dir)));

        let mut effective_importer_manifests_holder = BTreeMap::new();
        if package_extender.is_some()
            || versions_overrider.as_ref().is_some_and(|overrider| !overrider.is_empty())
        {
            for (id, manifest) in &importer_manifests {
                let mut cloned = (*manifest).clone();
                if let Some(extender) = package_extender.as_ref() {
                    extender.apply(cloned.value_mut());
                }
                if let Some(overrider) = versions_overrider.as_ref() {
                    let manifest_dir = cloned.path().parent().map(Path::to_path_buf);
                    overrider.apply(&mut cloned, manifest_dir.as_deref());
                }
                effective_importer_manifests_holder.insert(id.clone(), cloned);
            }
        }
        let importer_manifests: BTreeMap<String, &PackageManifest> =
            if effective_importer_manifests_holder.is_empty() {
                importer_manifests
            } else {
                effective_importer_manifests_holder
                    .iter()
                    .map(|(id, manifest)| (id.clone(), manifest))
                    .collect()
            };

        let package_extensions_hook: Option<ManifestHook> =
            package_extender.as_ref().map(|extender| {
                let extender = Arc::clone(extender);
                Arc::new(move |manifest| extender.apply_to_arc(manifest)) as ManifestHook
            });
        let overrides_hook: Option<ManifestHook> =
            versions_overrider.as_ref().and_then(|overrider| {
                if overrider.is_empty() {
                    None
                } else {
                    let overrider = Arc::clone(overrider);
                    Some(Arc::new(move |manifest| overrider.apply_to_arc(manifest, None))
                        as ManifestHook)
                }
            });
        let manifest_hook = compose_manifest_hooks(package_extensions_hook, overrides_hook);

        // Seed `allPreferredVersions` from every importer's manifest +
        // the wanted lockfile's snapshots (when an existing one is
        // present and is being rewritten). Mirrors upstream's
        // `getPreferredVersionsFromLockfileAndManifests` shape: the
        // manifests contribute direct-dep specifiers, the lockfile
        // contributes concrete `(name, version)` pins that bump the
        // weight of an already-matching direct-dep entry. Without the
        // lockfile-side seed, every install on a stale lockfile would
        // resolve unrelated entries from scratch and lose their
        // recorded pins; see <https://pnpm.io/settings#preferfrozenlockfile>.
        let manifests_for_preferred: Vec<&PackageManifest> =
            importer_manifests.values().copied().collect();
        // `pacquet update` withholds the lockfile pins for the names it
        // is bumping so they re-resolve to highest-in-range; everything
        // else keeps its pin. `DropOnly` builds a filtered snapshot map
        // (owned, so it outlives the seed build) excluding the matched
        // names; `DropAll` passes `None` so no pin seeds the table.
        let lockfile_snapshots = wanted_lockfile.and_then(|lockfile| lockfile.snapshots.as_ref());
        let filtered_snapshots;
        let seed_snapshots = match &update_seed_policy {
            UpdateSeedPolicy::KeepAll => lockfile_snapshots,
            UpdateSeedPolicy::DropAll => None,
            UpdateSeedPolicy::DropOnly(names) => match lockfile_snapshots {
                None => None,
                Some(snapshots) => {
                    // The update-target set is small (CLI selectors / direct
                    // deps); the snapshot map is large. Parse the targets to
                    // `PkgName` once so the per-snapshot filter compares
                    // against `key.name` directly instead of allocating a
                    // `String` per key.
                    let drop: std::collections::HashSet<pacquet_lockfile::PkgName> = names
                        .iter()
                        .filter_map(|name| pacquet_lockfile::PkgName::parse(name.as_str()).ok())
                        .collect();
                    filtered_snapshots = snapshots
                        .iter()
                        .filter(|(key, _)| !drop.contains(&key.name))
                        .map(|(key, entry)| (key.clone(), entry.clone()))
                        .collect::<HashMap<_, _>>();
                    Some(&filtered_snapshots)
                }
            },
        };
        let all_preferred_versions =
            pacquet_lockfile_preferred_versions::get_preferred_versions_from_lockfile_and_manifests(
                seed_snapshots,
                manifests_for_preferred.as_slice(),
            );
        // The picker biases toward this seed so pins that still satisfy
        // their range survive the re-resolve. Move the map into the `Arc`
        // (no extra clone) so each per-importer `ResolveOptions` shares it
        // with a refcount bump rather than deep-cloning the map.
        let preferred_versions_seed = Arc::new(all_preferred_versions);

        // Resolve `pnpm-workspace.yaml`'s `patchedDependencies` once
        // per install. The resolver consults the grouped record at
        // every per-node lookup to attach `(patch_hash=<hash>)` to the
        // matched package's `pkgIdWithPatchHash`. Mirrors upstream's
        // single `calcPatchHashes` + `groupPatchedDependencies` call at
        // <https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-installer/src/install/index.ts#L468-L488>.
        let patched_dependencies = config
            .resolved_patched_dependencies()
            .map_err(InstallWithFreshLockfileError::ResolvePatchedDependencies)?
            .map(Arc::new);
        // The verbatim `patchedDependencies` key → patch-file-hash map
        // recorded in the lockfile's top-level `patchedDependencies`
        // block. Computed separately from the grouped record above
        // (which buckets by package name) so the user's exact keys
        // survive into the lockfile, mirroring pnpm's
        // `calcPatchHashes(opts.patchedDependencies)`.
        let patched_dependency_hashes = config
            .patched_dependency_hashes()
            .map_err(InstallWithFreshLockfileError::CalcPatchHashes)?;

        // Loop per workspace project. Each importer gets its own
        // resolve_importer call with its own `project_dir` so
        // `workspace:` / `link:` resolutions compute paths relative
        // to the consuming project; the shared `meta_cache`,
        // `fetch_locker`, and `picked_manifest_cache` keep the
        // packument and version-pick work amortized across importers.
        // Mirrors upstream's
        // [`resolveRootDependencies`](https://github.com/pnpm/pnpm/blob/3422cecfd3/installing/deps-resolver/src/resolveDependencies.ts#L327-L437)
        // iteration: one shared resolution context, per-importer
        // direct-deps slices.
        let phase_start = std::time::Instant::now();
        let workspace_importers: Vec<pacquet_resolving_deps_resolver::WorkspaceImporter<'_>> =
            importer_manifests
                .iter()
                .map(|(id, manifest)| pacquet_resolving_deps_resolver::WorkspaceImporter {
                    id: id.clone(),
                    manifest,
                })
                .collect();
        let peers_suffix_max_length =
            usize::try_from(config.peers_suffix_max_length).unwrap_or(usize::MAX);
        let pnpmfile_hook = finder::load_pnpmfile(lockfile_dir);
        // Kept past the resolver hand-off (which consumes `pnpmfile_hook`) so
        // the `afterAllResolved` hook can transform the lockfile before it is
        // written.
        let after_all_resolved_hook = pnpmfile_hook.clone();
        // Pre-bind the reporter, project prefix, and pnpmfile path into the
        // `context.log(...)` sinks so the resolver and lockfile writer stay
        // reporter-agnostic. Mirrors pnpm's `createReadPackageHookContext`,
        // which forwards each hook's `context.log` to the `pnpm:hook` channel.
        let pnpmfile_path =
            pnpmfile_hook.as_ref().and_then(|hook| hook.source_path()).map(Path::to_path_buf);
        let read_package_log = pnpmfile_path
            .as_ref()
            .map(|from| hook_log_fn::<Reporter>(lockfile_dir, from, "readPackage"));
        let after_all_resolved_log = pnpmfile_path
            .as_ref()
            .map(|from| hook_log_fn::<Reporter>(lockfile_dir, from, "afterAllResolved"));

        // Call preResolution hook before resolution starts (mirrors pnpm's behavior in install/index.ts)
        if let Some(ref hook) = pnpmfile_hook {
            let wanted_lockfile_json = wanted_lockfile.map_or_else(
                || serde_json::json!({}),
                |lf| serde_json::to_value(lf).unwrap_or_else(|_| serde_json::json!({})),
            );
            let current_lockfile =
                Lockfile::load_current_from_virtual_store_dir(&config.virtual_store_dir)
                    .ok()
                    .flatten();
            let exists_current_lockfile = current_lockfile.is_some();
            let current_lockfile_json = current_lockfile.map_or_else(
                || serde_json::json!({}),
                |lf| serde_json::to_value(lf).unwrap_or_else(|_| serde_json::json!({})),
            );
            let registries = serde_json::json!({ "default": config.registry });
            let ctx = pacquet_hooks::PreResolutionHookContext {
                wanted_lockfile: wanted_lockfile_json,
                current_lockfile: current_lockfile_json,
                exists_current_lockfile,
                exists_non_empty_wanted_lockfile: wanted_lockfile.as_ref().is_some_and(|lf| {
                    !lf.snapshots.as_ref().is_none_or(std::collections::HashMap::is_empty)
                }),
                lockfile_dir: lockfile_dir.to_string_lossy().to_string(),
                store_dir: config.store_dir.display().to_string(),
                registries,
            };
            hook.pre_resolution(
                ctx,
                pacquet_hooks::PreResolutionHookLogger {
                    info: Arc::new(|_| {}),
                    warn: Arc::new(|_| {}),
                },
            )
            .await;
        }

        let workspace_opts = pacquet_resolving_deps_resolver::WorkspaceResolveOptions {
            dedupe_peers: config.dedupe_peers,
            dedupe_injected_deps: config.dedupe_injected_deps,
            dedupe_peer_dependents: config.dedupe_peer_dependents,
            exclude_links_from_lockfile: config.exclude_links_from_lockfile,
            lockfile_dir: lockfile_dir.to_path_buf(),
            peers_suffix_max_length,
            manifest_hook: manifest_hook.clone(),
            pnpmfile_hook,
            read_package_log,
            pick_lowest_direct,
            time_based,
            // Hand the resolver the prior lockfile so it can reuse
            // already-resolved subtrees instead of re-resolving from the
            // registry (see pacquet/plans/LOCKFILE_RESOLUTION_REUSE.md).
            // Withhold it when packageExtensions or overrides drifted:
            // both settings rewrite package dependency sets, so the
            // recorded subtree is stale. pnpm likewise invalidates the
            // lockfile on these settings changes.
            wanted_lockfile: wanted_lockfile
                .filter(|lockfile| {
                    lockfile.package_extensions_checksum
                        == compute_package_extensions_checksum(config)
                        && overrides_match(lockfile.overrides.as_ref(), resolved_overrides.as_ref())
                })
                .cloned()
                .map(Arc::new),
            // `pacquet update` must re-resolve its targets to highest-
            // in-range, so suppress reuse for them (and their subtrees).
            update_reuse_scope: match &update_seed_policy {
                UpdateSeedPolicy::KeepAll => pacquet_resolving_deps_resolver::UpdateReuseScope::All,
                UpdateSeedPolicy::DropAll => {
                    pacquet_resolving_deps_resolver::UpdateReuseScope::None
                }
                UpdateSeedPolicy::DropOnly(names) => {
                    pacquet_resolving_deps_resolver::UpdateReuseScope::Except(names.clone())
                }
            },
        };
        let modules_basename = config.modules_dir.file_name().map_or_else(
            || std::ffi::OsString::from("node_modules"),
            std::ffi::OsStr::to_os_string,
        );
        let workspace_result = pacquet_resolving_deps_resolver::resolve_workspace(
            &*resolver,
            &workspace_importers,
            &dependency_groups,
            workspace_opts,
            |importer| {
                let project_dir = importer
                    .manifest
                    .path()
                    .parent()
                    .expect("manifest path always has a parent dir")
                    .to_path_buf();
                let importer_modules_dir = project_dir.join(&modules_basename);
                ResolveImporterOptions {
                    auto_install_peers: config.auto_install_peers,
                    auto_install_peers_from_highest_match: config
                        .auto_install_peers_from_highest_match,
                    resolve_peers_from_workspace_root: config.resolve_peers_from_workspace_root,
                    dedupe_peers: config.dedupe_peers,
                    // The per-importer hoist loop mutates its own copy, so
                    // clone the shared seed's map here (deref past the `Arc`).
                    all_preferred_versions: (*preferred_versions_seed).clone(),
                    patched_dependencies: patched_dependencies.clone(),
                    // `pick_lowest_direct` / `subdep_published_by` are
                    // authoritative from `resolve_workspace` (it computes
                    // the workspace-wide time-based cutoff and overrides
                    // both per importer); the values here just satisfy
                    // the struct. `subdep_published_by` defaults to the
                    // `minimumReleaseAge` cutoff so non-time-based modes
                    // leave subdep resolution unchanged.
                    pick_lowest_direct,
                    subdep_published_by: published_by,
                    base_opts: ResolveOptions {
                        preferred_versions: Arc::clone(&preferred_versions_seed),
                        default_tag: Some("latest".to_string()),
                        published_by,
                        published_by_exclude: published_by_exclude.clone(),
                        trust_policy,
                        trust_policy_exclude: trust_policy_exclude.clone(),
                        trust_policy_ignore_after: config.trust_policy_ignore_after,
                        project_dir,
                        lockfile_dir: lockfile_dir.to_path_buf(),
                        workspace_packages: workspace_packages.clone(),
                        block_exotic_subdeps: config.block_exotic_subdeps,
                        always_try_workspace_packages: config.link_workspace_packages
                            != LinkWorkspacePackages::Off,
                        inject_workspace_packages: config.inject_workspace_packages,
                        prefer_workspace_packages: config.prefer_workspace_packages,
                        update_checksums,
                        ..ResolveOptions::default()
                    },
                    catalogs: catalogs.clone(),
                    exclude_links_from_lockfile: config.exclude_links_from_lockfile,
                    lockfile_dir: Some(lockfile_dir.to_path_buf()),
                    modules_dir: Some(importer_modules_dir),
                    peers_suffix_max_length,
                    catalog_server: false,
                    manifest_hook: manifest_hook.clone(),
                    pnpmfile_hook: None,
                }
            },
        )
        .await
        .map_err(InstallWithFreshLockfileError::ResolveImporter)?;
        let total_nodes = workspace_result.peers.graph.len();
        for (importer_id, issues) in &workspace_result.peers.peer_dependency_issues_by_importer {
            tracing::warn!(
                target: "pacquet::install",
                importer_id = %importer_id,
                missing = issues.missing.len(),
                bad = issues.bad.len(),
                "Peer dependency issues detected (issue renderer not ported yet)",
            );
        }
        let merged_graph = workspace_result.peers.graph;
        let direct_by_importer = workspace_result.peers.direct_dependencies_by_importer;
        tracing::info!(
            target: "pacquet::install::phase",
            phase = "resolve_workspace",
            elapsed_ms = phase_start.elapsed().as_millis() as u64,
            importers = importer_manifests.len(),
            nodes = total_nodes,
            "phase complete",
        );

        // Drop the resolver (and its packument cache) before the
        // install pass. Dropping `resolver` releases the
        // [`PrefetchingResolver`]'s wrapped inner chain, which in
        // turn releases the `ArcResolver`'s strong reference to
        // `npm_resolver`; the standalone `npm_resolver` binding
        // holds a second strong reference because the deno- and
        // bun-resolvers were handed a clone of the same `Arc` for
        // their version-selection delegate. Releasing every
        // reference takes an explicit drop on each binding —
        // letting the packument cache, fetch locker, and
        // picked-manifest cache shrink before the install pass
        // pulls more tarballs into the CAFS. The
        // `store_index` / `store_index_writer` /
        // `verified_files_cache` are owned above the resolver
        // chain so the prefetching wrapper can share them with the
        // install pass.
        drop(resolver);
        drop(npm_resolver);
        drop(meta_cache);
        drop(fetch_locker);
        drop(picked_manifest_cache);

        // Compute the `pnpmfileChecksum` once for both lockfile-build
        // paths below. Mirrors pnpm's `calculatePnpmfileChecksum`: the
        // hash of the project's `.pnpmfile.{cjs,mjs}` when it exports
        // hooks, `None` otherwise. Resolution has already spawned the
        // pnpmfile worker (every `readPackage` runs through it), so the
        // gate query is cheap here.
        let pnpmfile_checksum: Option<String> = match after_all_resolved_hook.as_ref() {
            Some(hook) => hook.calculate_pnpmfile_checksum().await,
            None => None,
        };

        // `--lockfile-only`: the graph is resolved, so build and write
        // `pnpm-lock.yaml` and return before any materialization. No
        // tarball was prefetched (the resolver ran without the
        // `PrefetchingResolver` wrapper), so the store is untouched and
        // there is no `node_modules`, `.modules.yaml`, or current
        // lockfile — matching pnpm's lockfileOnly resolve pass.
        if lockfile_only {
            let built_lockfile = build_fresh_lockfile(FreshLockfileBuildOptions {
                config,
                importer_manifests: &importer_manifests,
                graph: &merged_graph,
                direct_by_importer: &direct_by_importer,
                resolved_overrides: resolved_overrides.clone(),
                catalogs: &catalogs,
                pnpmfile_checksum: pnpmfile_checksum.as_deref(),
                patched_dependency_hashes: patched_dependency_hashes.as_ref(),
            });
            let (wanted_lockfile, can_record_lockfile_verification) = if config.lockfile {
                let can_record_lockfile_verification = save_wanted_lockfile(
                    &built_lockfile,
                    &lockfile_dir.join(Lockfile::FILE_NAME),
                    after_all_resolved_hook.as_ref(),
                    after_all_resolved_log.clone(),
                )
                .await?;
                (Some(built_lockfile), can_record_lockfile_verification)
            } else {
                (None, false)
            };

            // Close the store-index writer cleanly even though no rows
            // were written, mirroring the drain at the tail of the
            // materializing path.
            drop(store_index_writer);
            match writer_task.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::warn!(
                    target: "pacquet::install",
                    ?error,
                    "store-index writer task returned an error during a lockfile-only install",
                ),
                Err(error) => tracing::warn!(
                    target: "pacquet::install",
                    ?error,
                    "store-index writer task panicked during a lockfile-only install",
                ),
            }

            Reporter::emit(&LogEvent::Stage(StageLog {
                level: LogLevel::Debug,
                prefix: requester.to_string(),
                stage: Stage::ImportingDone,
            }));
            return Ok(InstallWithFreshLockfileResult {
                hoisted_dependencies: HoistedDependencies::new(),
                hoisted_locations: BTreeMap::new(),
                wanted_lockfile,
                can_record_lockfile_verification,
            });
        }

        // Warm-cache batched prefetch: collect every `(integrity,
        // pkg_id)` pair the resolver produced, run one batched SQL
        // `SELECT ... WHERE key IN (...)` against the store index,
        // then verify each row's files on rayon. Mirrors what
        // `create_virtual_store::run` already does for the frozen-
        // lockfile path. The store_index, store_index_writer, and
        // verified_files_cache are opened earlier (above the resolver
        // chain) so the [`PrefetchingResolver`] can share them; this
        // batched prefetch reuses the same handles to fold the
        // per-package SQL lookups the install pass would otherwise
        // serialize on `Arc<Mutex<StoreIndex>>` for warm packages
        // that weren't reached by the resolve-time prefetch (e.g.
        // resolutions without a structured `name@version`).
        let cache_keys: Vec<String> = collect_prefetch_cache_keys_from_graph(&merged_graph);
        let cache_keys_len = cache_keys.len();
        let phase_start = std::time::Instant::now();
        let prefetch = pacquet_tarball::prefetch_cas_paths(
            store_index_ref.cloned(),
            store_dir,
            cache_keys,
            config.verify_store_integrity,
            SharedVerifiedFilesCache::clone(&verified_files_cache),
        )
        .await;
        // `side_effects_maps` is intentionally dropped: the fresh-
        // lockfile path skips the build phase today (see the
        // `importing_done` emit at the tail of this function), so
        // there is no `is_built` gate to feed. Keep the binding name
        // explicit so a future port that wires builds in does not
        // miss the source.
        let pacquet_tarball::PrefetchResult {
            cas_paths: prefetched_cas_paths,
            manifests: prefetched_manifests,
            side_effects_maps: _,
        } = prefetch;
        tracing::info!(
            target: "pacquet::install::phase",
            phase = "prefetch_cas_paths",
            elapsed_ms = phase_start.elapsed().as_millis() as u64,
            cache_keys = cache_keys_len,
            hits = prefetched_cas_paths.len(),
            manifest_hits = prefetched_manifests.len(),
            "phase complete",
        );

        // Build the install-scoped virtual-store layout. When
        // `enable_global_virtual_store` is on, this precomputes each
        // snapshot's `<scope>/<name>/<version>/<hash>` suffix under
        // `<store_dir>/links`; otherwise it falls through to the legacy
        // `<virtual_store_dir>/<flat-name>` shape. Either way every
        // downstream slot-path lookup routes through
        // `VirtualStoreLayout::slot_dir`. Mirrors the frozen-lockfile
        // path's `enableGlobalVirtualStore: true → allowBuilds ??= {}`
        // shape at
        // <https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/index.ts#L342-L344>.
        //
        // The lockfile-shaped `snapshots:` / `packages:` maps the
        // layout reads are produced via the writable-lockfile adapter
        // ([`dependencies_graph_to_lockfile`]), but only when GVS is
        // on — the legacy layout doesn't consult them and the build
        // is cheap to skip otherwise. `engine_name` only matters when
        // GVS is on, so the `node --version` probe is gated the same
        // way.
        let allow_build_policy = AllowBuildPolicy::from_config(config)
            .map_err(InstallWithFreshLockfileError::AllowBuildsPolicy)?;
        // Build the freshly-resolved lockfile structure
        // unconditionally: GVS needs `snapshots:` / `packages:` to
        // compute the layout, and the bin-link pass below uses the
        // same maps to drive the lockfile-driven
        // `LinkVirtualStoreBins` path (skipping the per-slot
        // `read_dir` enumeration and the per-child `package.json`
        // read when the prefetched manifest is available). The build
        // is cheap — ~3 ms on the alotta-files fixture — and it is
        // what we end up saving below anyway.
        //
        // Named `built_lockfile` to keep it distinct from
        // [`Self::wanted_lockfile`], which is the *previous* run's
        // lockfile threaded in for preferred-versions seeding.
        let phase_start = std::time::Instant::now();
        let built_lockfile = build_fresh_lockfile(FreshLockfileBuildOptions {
            config,
            importer_manifests: &importer_manifests,
            graph: &merged_graph,
            direct_by_importer: &direct_by_importer,
            resolved_overrides: resolved_overrides.clone(),
            catalogs: &catalogs,
            pnpmfile_checksum: pnpmfile_checksum.as_deref(),
            patched_dependency_hashes: patched_dependency_hashes.as_ref(),
        });
        tracing::info!(
            target: "pacquet::install::phase",
            phase = "build_fresh_lockfile",
            elapsed_ms = phase_start.elapsed().as_millis() as u64,
            "phase complete",
        );
        let engine_name: Option<String> = if config.enable_global_virtual_store {
            tokio::task::spawn_blocking(|| {
                pacquet_graph_hasher::detect_node_major()
                    .map(|major| pacquet_graph_hasher::engine_name(major, None, None))
            })
            .await
            .ok()
            .flatten()
        } else {
            None
        };
        let phase_start = std::time::Instant::now();
        let layout = VirtualStoreLayout::new(
            config,
            engine_name.as_deref(),
            built_lockfile.snapshots.as_ref(),
            built_lockfile.packages.as_ref(),
            Some(&allow_build_policy),
        );
        if config.enable_global_virtual_store {
            tracing::info!(
                target: "pacquet::install::phase",
                phase = "virtual_store_layout_new",
                elapsed_ms = phase_start.elapsed().as_millis() as u64,
                "phase complete",
            );
        }

        // Materialise the virtual store via the same phased
        // warm/cold-batch pipeline the frozen-lockfile path uses. The
        // fresh-lockfile path used to dispatch through `install_subtree`,
        // a recursive per-package async tree walk that blocked one
        // tokio worker per in-flight package on its own rayon
        // `par_iter` for the per-package link step. The phased pipeline
        // in `CreateVirtualStore` runs a single `par_iter` over every
        // warm snapshot at once, which closes the ~94% wall-time gap
        // to pnpm on the full-resolution-warm scenario without
        // regressing the cold-cache or frozen-lockfile paths. See
        // <https://github.com/pnpm/pnpm/issues/11866> for the architectural diagnosis and the bench data.
        //
        // The fresh-lockfile path has no installability check yet
        // (the resolver's `PackageVersion` deserializer doesn't carry
        // engine / cpu / os / libc constraints to gate on), so the
        // skip set starts empty. A future port of
        // `compute_skipped_snapshots` for fresh-lockfile would route
        // through here too. Under `nodeLinker: hoisted` the
        // hoisted-linker walker may fold its own installability skips
        // into this set, so it is `mut`.
        let mut skipped = SkippedSnapshots::new();
        let phase_start = std::time::Instant::now();
        let CreateVirtualStoreOutput {
            package_manifests,
            side_effects_maps_by_snapshot: _,
            fetch_failed: _,
            // Populated only under `node_linker == Hoisted`; consumed by
            // the hoisted-linker pass below to materialize the on-disk
            // tree. `None` for the isolated linker.
            cas_paths_by_pkg_id,
        } = CreateVirtualStore {
            http_client,
            config,
            packages: built_lockfile.packages.as_ref(),
            snapshots: built_lockfile.snapshots.as_ref(),
            current_snapshots: None,
            current_packages: None,
            layout: &layout,
            logged_methods,
            requester,
            store_index_writer: &store_index_writer,
            allow_build_policy: &allow_build_policy,
            skipped: &skipped,
            workspace_root: lockfile_dir,
            node_linker,
            progress_reported: &progress_reported,
            // Share the resolve-time prefetcher's in-flight downloads with
            // the cold batch. The `PrefetchingResolver` streams each
            // tarball into `tarball_mem_cache` keyed by URL; the cold
            // batch's only on-disk dedup is the store-index row, which the
            // prefetcher's writer commits asynchronously. Without the
            // shared cache a snapshot whose prefetch hasn't committed its
            // row yet is classified cold and re-downloaded — the race in
            // <https://github.com/pnpm/pnpm/issues/12241>. Routing the cold
            // batch through the mem cache makes it reuse the in-flight
            // download instead.
            tarball_mem_cache: Some(&tarball_mem_cache),
            #[cfg(test)]
            link_concurrency_probe: None,
        }
        .run::<Reporter>()
        .await
        .map_err(InstallWithFreshLockfileError::CreateVirtualStore)?;
        tracing::info!(
            target: "pacquet::install::phase",
            phase = "create_virtual_store",
            elapsed_ms = phase_start.elapsed().as_millis() as u64,
            "phase complete",
        );

        // Drop the orchestration's writer handle so the channel closes,
        // then wait for the final batch flush. See `create_virtual_store.rs`
        // for why errors here are downgraded to `warn!`.
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

        // Create `<modules_dir>/<alias>` symlinks for each direct dep.
        // Replaces the per-edge `symlink_package` calls the old
        // `install_subtree` recursion did. Mirrors how
        // `install_frozen_lockfile` calls this after `CreateVirtualStore::run`.
        //
        // **Anchor on `config.modules_dir.parent()`, not `lockfile_dir`.**
        // `SymlinkDirectDependencies` resolves each importer's modules
        // dir as `<workspace_root>/<importer_id>/<modules_basename>`,
        // which for the root importer (`.`) collapses to
        // `<workspace_root>/<modules_basename>`. The fresh-lockfile
        // path's tests parameterise `config.modules_dir` at a path that
        // doesn't always live under the manifest's directory (the
        // historical `install_subtree` code took `&config.modules_dir`
        // verbatim as the parent of every direct-dep symlink), so
        // anchoring on the lockfile-dir-derived `workspace_root` would
        // land symlinks at the wrong path on those configurations.
        // Using `config.modules_dir.parent()` recovers the old
        // behaviour: for the common case where
        // `config.modules_dir == <lockfile_dir>/node_modules` they
        // coincide; for an explicitly-relocated `modules_dir` the
        // symlinks land where the rest of pacquet's install code
        // (`.modules.yaml`, `LinkVirtualStoreBins`, etc.) already
        // writes.
        let symlink_root: &Path = config.modules_dir.parent().unwrap_or(lockfile_dir);

        // Under `nodeLinker: hoisted` the regular deps live as real
        // directories materialized by the hoisted linker, not as
        // symlinks into the virtual store. Route through the same
        // walker + linker + `link_only` symlink pass the frozen path
        // uses (shared via `run_hoisted_linker`), then skip the
        // isolated-linker public/private hoist and `LinkVirtualStoreBins`
        // passes entirely — the hoisted linker writes per-`node_modules`
        // bins while walking the hierarchy. `hoisted_dependencies` stays
        // empty (the hoisted linker has no isolated-mode alias→kind
        // adapter shape); `hoisted_locations` carries the walker's
        // placements so `.modules.yaml` round-trips them.
        let (hoisted_dependencies, hoisted_locations) = if is_hoisted {
            // The hoisted walker runs the installability check, which
            // consults `engines.node`. Detect the host node version (as the
            // frozen path does) whenever a package carries an installability
            // constraint, so the engine check resolves against a real version
            // instead of erroring on an empty one. Skip the `node --version`
            // probe entirely when nothing constrains it.
            let host_node = if built_lockfile.packages.as_ref().is_some_and(|packages| {
                built_lockfile.snapshots.as_ref().is_some_and(|snapshots| {
                    crate::any_installability_constraint(snapshots, packages)
                })
            }) {
                tokio::task::spawn_blocking(crate::InstallabilityHost::detect)
                    .await
                    .ok()
                    .map(|host| (host.node_detected, host.node_version))
            } else {
                None
            };
            let output = crate::install_frozen_lockfile::run_hoisted_linker::<Reporter>(
                crate::install_frozen_lockfile::HoistedLinkerInputs {
                    config,
                    lockfile: &built_lockfile,
                    // No previous-install `<virtual_store_dir>/lock.yaml`
                    // is threaded into the fresh path yet (<https://github.com/pnpm/pnpm/issues/11871>), so the
                    // walker runs without an orphan diff.
                    current_lockfile: None,
                    layout: &layout,
                    importers: &built_lockfile.importers,
                    dependency_groups: &dependency_groups,
                    walker_lockfile_dir: lockfile_dir,
                    symlink_workspace_root: symlink_root,
                    host_node: host_node.as_ref(),
                    supported_architectures,
                    cas_paths_by_pkg_id,
                    logged_methods,
                    requester,
                },
                &mut skipped,
            )
            .map_err(InstallWithFreshLockfileError::from)?;
            (HoistedDependencies::new(), output.hoisted_locations)
        } else {
            // Pre-compute the hoist plan so the dedupe pass in
            // `SymlinkDirectDependencies` can fold publicly-hoisted
            // aliases into root's target map — same shape as the
            // frozen-lockfile path. The `HoistResult` is reused for
            // the on-disk hoist phase below, so the BFS runs once.
            let pre_hoist = crate::install_frozen_lockfile::compute_hoist_plan(
                config,
                built_lockfile.snapshots.as_ref(),
                built_lockfile.packages.as_ref(),
                &built_lockfile.importers,
                &dependency_groups,
                &skipped,
                false,
            );
            let public_hoist_targets: Option<
                std::collections::BTreeMap<String, std::path::PathBuf>,
            > = pre_hoist.as_ref().map(|plan| {
                crate::install_frozen_lockfile::collect_public_hoist_targets(
                    &plan.result,
                    &plan.graph,
                    &layout,
                    &plan.skipped,
                )
            });

            SymlinkDirectDependencies {
                config,
                layout: &layout,
                importers: &built_lockfile.importers,
                dependency_groups: dependency_groups.iter().copied(),
                workspace_root: symlink_root,
                skipped: &skipped,
                link_only: false,
                public_hoist_targets: public_hoist_targets.as_ref(),
            }
            .run::<Reporter>()
            .map_err(InstallWithFreshLockfileError::SymlinkDirectDependencies)?;

            // On-disk hoist phase. Mirrors the frozen-install block at
            // `install_frozen_lockfile.rs`: symlink the publicly +
            // privately hoisted aliases into their target dirs, then
            // link private-side bins into `<vs>/node_modules/.bin`.
            // Public-side bin precedence is handled implicitly by the
            // per-importer `link_bins` pass below, which now walks both
            // direct-dep and public-hoist symlinks in root's
            // `node_modules/`.
            let hoisted_dependencies = if let Some(plan) = pre_hoist {
                let crate::install_frozen_lockfile::HoistPlan {
                    graph,
                    result,
                    skipped: hoist_skipped,
                    ..
                } = plan;
                let private_dir = config.virtual_store_dir.join("node_modules");
                let public_dir = config.modules_dir.clone();
                crate::symlink_hoisted_dependencies(
                    &result.hoisted_dependencies_by_node_id,
                    &graph,
                    &layout,
                    &private_dir,
                    &public_dir,
                    &hoist_skipped,
                )
                .map_err(InstallWithFreshLockfileError::HoistSymlink)?;
                crate::link_direct_dep_bins(&private_dir, &result.hoisted_aliases_with_bins)
                    .map_err(InstallWithFreshLockfileError::HoistLinkBins)?;
                result.hoisted_dependencies
            } else {
                HoistedDependencies::new()
            };

            // Link bins. Direct dependencies first (each importer's
            // `node_modules/.bin`) and then per-slot children inside the
            // virtual store. Mirrors the same two-call shape as
            // `install_frozen_lockfile.rs`. We re-walk `<modules_dir>`
            // instead of replaying the manifest because the
            // `dependency_groups` iterator was already consumed above;
            // pnpm's own `linkBins(modulesDir, binsDir)` overload uses
            // the same strategy. One pass per importer so sibling
            // workspace projects get their own `.bin/` populated,
            // mirroring upstream's per-importer `linkBinsOfImporter` at
            // <https://github.com/pnpm/pnpm/blob/3422cecfd3/installing/deps-installer/src/install/link.ts>.
            let modules_basename = config.modules_dir.file_name().map_or_else(
                || std::ffi::OsString::from("node_modules"),
                std::ffi::OsStr::to_os_string,
            );
            for importer_id in importer_manifests.keys() {
                let project_dir = crate::symlink_direct_dependencies::importer_root_dir(
                    symlink_root,
                    importer_id,
                );
                let modules_dir = project_dir.join(&modules_basename);
                let bins_dir = modules_dir.join(".bin");
                link_bins::<Host>(&modules_dir, &bins_dir)
                    .map_err(InstallWithFreshLockfileError::LinkBins)?;
            }

            // Drive the lockfile-driven `LinkVirtualStoreBins` path. The
            // bin linker iterates `snapshots:` (no per-slot `read_dir`)
            // and reads each child's manifest from `package_manifests`
            // (no per-child `package.json` disk read on warm hits).
            // `package_manifests` is now produced by `CreateVirtualStore`
            // directly — its prefetch + cold-batch passes both feed into
            // the same map.
            //
            // `packages: None` on purpose: the freshly-built lockfile's
            // `packages:` rows carry an incomplete `has_bin` because the
            // resolver's `PackageVersion` deserializer does not include
            // the `bin` field. Trusting the empty-by-omission
            // `has_bin_set` here would filter out every child and skip
            // bin linking entirely. With `packages: None` the bin linker
            // falls through to "process every child" and lets each
            // child's actual manifest (`bin` present or not) decide.
            // Threading `bin` through `PackageVersion` is the proper
            // fix; once that lands, pass
            // `built_lockfile.packages.as_ref()` here to recover the
            // ~95% slot short-circuit the frozen path enjoys.
            LinkVirtualStoreBins {
                layout: &layout,
                snapshots: built_lockfile.snapshots.as_ref(),
                packages: None,
                package_manifests: &package_manifests,
                skipped: &skipped,
            }
            .run()
            .map_err(InstallWithFreshLockfileError::LinkVirtualStoreBins)?;

            (hoisted_dependencies, BTreeMap::new())
        };

        // Write `pnpm-lock.yaml` from the resolved graph. Mirrors
        // upstream's
        // [`writeLockfiles`](https://github.com/pnpm/pnpm/blob/094aa6e57b/lockfile/fs/src/write.ts#L133)
        // call at the tail of `deps-installer/src/install/index.ts`:
        // every non-frozen install lands a wanted lockfile so the next
        // pnpm / pacquet invocation can either go headless or diff
        // against it. The save runs after materialization succeeds so
        // a partial install can't leave a lockfile pointing at slots
        // that never landed on disk. `config.lockfile=false` skips the
        // write, matching pnpm's documented opt-out behavior even
        // though that knob is rarely exercised today.
        //
        // The built lockfile is returned to the caller so it can also
        // persist `<virtual_store_dir>/lock.yaml` after `.modules.yaml`
        // succeeds, matching the frozen-lockfile path's ordering. We
        // don't write the current-lockfile inline here because the
        // safety property — a manifest-write failure must not leave a
        // current-lockfile pointing at an incomplete install — needs
        // `.modules.yaml` to land first.
        let (wanted_lockfile, can_record_lockfile_verification) = if config.lockfile {
            let target = lockfile_dir.join(Lockfile::FILE_NAME);
            let can_record_lockfile_verification = save_wanted_lockfile(
                &built_lockfile,
                &target,
                after_all_resolved_hook.as_ref(),
                after_all_resolved_log.clone(),
            )
            .await?;
            (Some(built_lockfile), can_record_lockfile_verification)
        } else {
            (None, false)
        };

        // Mirrors upstream `link.ts:167-170`: `importing_done` fires once
        // extraction and symlink linking are complete. The fresh-lockfile
        // path does not run lifecycle scripts today, so emitting here also
        // marks end-of-install for reporters.
        // <https://github.com/pnpm/pnpm/blob/80037699fb/installing/deps-installer/src/install/link.ts#L167>
        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: requester.to_string(),
            stage: Stage::ImportingDone,
        }));

        Ok(InstallWithFreshLockfileResult {
            hoisted_dependencies,
            hoisted_locations,
            wanted_lockfile,
            can_record_lockfile_verification,
        })
    }
}

/// Walk the merged resolver graph and emit the `{integrity}\t{pkg_id}`
/// cache keys [`pacquet_tarball::prefetch_cas_paths`] uses for its
/// batched `SELECT ... WHERE key IN (...)` against the store index.
/// Mirrors the equivalent collection loop in
/// [`crate::CreateVirtualStore::run`] for the frozen-lockfile path —
/// same key shape, same dedup, so the fresh-lockfile path's warm
/// batch hits the same rows pnpm or pacquet wrote on the prior
/// install.
///
/// Skips nodes whose resolver result isn't a non-git-hosted tarball
/// with an `integrity` and a resolvable `name@version` (from `name_ver`
/// or, for remote-tarball direct deps, the fetched manifest):
/// git-hosted tarballs and directory / git / binary resolutions use a
/// different key shape (`pkg_id`-only) and route through the cold path.
/// Today's `install_subtree` only handles tarball+integrity anyway, so
/// the skipped entries can't be served from the prefetch either way.
fn collect_prefetch_cache_keys_from_graph(
    graph: &pacquet_resolving_deps_resolver::DependenciesGraph,
) -> Vec<String> {
    let mut keys: Vec<String> = graph
        .values()
        .filter_map(|node| {
            let pacquet_lockfile::LockfileResolution::Tarball(tarball) =
                &node.resolve_result.resolution
            else {
                return None;
            };
            if tarball.git_hosted == Some(true) {
                return None;
            }
            let integrity = tarball.integrity.as_ref()?.to_string();
            // `name_ver` when the resolver produced one (npm registry);
            // otherwise the manifest's `name@version` — remote-tarball
            // direct deps learn both from `package.json`, and the
            // resolve-time fetch keyed the store-index row the same way.
            let pkg_id = if let Some(name_ver) = node.resolve_result.name_ver.as_ref() {
                format!("{}@{}", name_ver.name, name_ver.suffix)
            } else {
                let manifest = node.resolve_result.manifest.as_deref()?;
                let name = manifest.get("name")?.as_str()?;
                let version = manifest.get("version")?.as_str()?;
                format!("{name}@{version}")
            };
            Some(pacquet_store_dir::store_index_key(&integrity, &pkg_id))
        })
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// Build the `context.log(...)` sink a pnpmfile hook forwards to: each
/// `context.log(message)` call emits a `pnpm:hook` event through the
/// install's reporter, carrying the project `prefix`, the pnpmfile path
/// (`from`), and the hook name. Mirrors pnpm's `createReadPackageHookContext`,
/// which routes `context.log` to `hookLogger.debug({ from, hook, message, prefix })`.
fn hook_log_fn<Reporter: self::Reporter>(
    prefix: &Path,
    from: &Path,
    hook: &'static str,
) -> pacquet_hooks::LogFn {
    let prefix = prefix.to_string_lossy().into_owned();
    let from = from.to_string_lossy().into_owned();
    Arc::new(move |message: String| {
        Reporter::emit(&LogEvent::Hook(HookLog {
            level: LogLevel::Debug,
            from: from.clone(),
            hook: hook.to_string(),
            message,
            prefix: prefix.clone(),
        }));
    })
}

/// Write the freshly-built wanted lockfile to `target`, first running the
/// `afterAllResolved` pnpmfile hook when one is configured.
///
/// Mirrors pnpm: `afterAllResolved` receives the resolved lockfile object and
/// returns the (possibly mutated) lockfile that gets written. The round-trip
/// goes through `serde_json::Value` so hook-added keys the typed [`Lockfile`]
/// cannot represent survive to disk; `serde_json`'s `preserve_order` feature
/// keeps the output byte-identical to the typed write when the hook makes no
/// changes. A throwing hook aborts the install.
async fn save_wanted_lockfile(
    built_lockfile: &Lockfile,
    target: &Path,
    hook: Option<&Arc<dyn pacquet_hooks::PnpmfileHooks>>,
    log: Option<pacquet_hooks::LogFn>,
) -> Result<bool, InstallWithFreshLockfileError> {
    let Some(hook) = hook else {
        built_lockfile
            .save_to_path(target)
            .map_err(InstallWithFreshLockfileError::SaveWantedLockfile)?;
        return Ok(true);
    };

    let value = serde_json::to_value(built_lockfile)
        .map_err(InstallWithFreshLockfileError::AfterAllResolvedSerialize)?;
    let ctx = pacquet_hooks::HookContext { log: log.unwrap_or_else(|| Arc::new(|_| {})) };
    let result = hook
        .after_all_resolved(value, ctx)
        .await
        .map_err(InstallWithFreshLockfileError::AfterAllResolvedHook)?;

    // `Null` means the pnpmfile has no `afterAllResolved` hook, so write the
    // typed lockfile unchanged.
    if result.is_null() {
        built_lockfile.save_to_path(target)
    } else {
        pacquet_lockfile::save_value_to_path(&result, target)
    }
    .map_err(InstallWithFreshLockfileError::SaveWantedLockfile)?;
    Ok(result.is_null())
}

fn parse_config_overrides(
    config: &Config,
    catalogs: &Catalogs,
) -> Result<
    Option<Vec<pacquet_config_parse_overrides::VersionOverride>>,
    InstallWithFreshLockfileError,
> {
    match config.overrides.as_ref() {
        Some(map) if !map.is_empty() => {
            pacquet_config_parse_overrides::parse_overrides_iter(map.iter(), catalogs)
                .map(Some)
                .map_err(InstallWithFreshLockfileError::InvalidOverrides)
        }
        _ => Ok(None),
    }
}

fn resolved_overrides_map(
    parsed: &[pacquet_config_parse_overrides::VersionOverride],
) -> IndexMap<String, String> {
    parsed.iter().map(|entry| (entry.selector.clone(), entry.new_bare_specifier.clone())).collect()
}

fn overrides_match(
    lockfile: Option<&IndexMap<String, String>>,
    config: Option<&IndexMap<String, String>>,
) -> bool {
    let lockfile = lockfile.filter(|map| !map.is_empty());
    let config = config.filter(|map| !map.is_empty());
    match (lockfile, config) {
        (None, None) => true,
        (Some(lockfile), Some(config)) => {
            lockfile.len() == config.len()
                && lockfile.iter().all(|(key, value)| {
                    config.get(key).is_some_and(|config_value| config_value == value)
                })
        }
        _ => false,
    }
}

fn compose_manifest_hooks(
    first: Option<ManifestHook>,
    second: Option<ManifestHook>,
) -> Option<ManifestHook> {
    match (first, second) {
        (None, None) => None,
        (Some(hook), None) | (None, Some(hook)) => Some(hook),
        (Some(first), Some(second)) => {
            Some(Arc::new(move |manifest| second(first(manifest))) as ManifestHook)
        }
    }
}

/// Build the [`Lockfile`] for `<lockfile_dir>/pnpm-lock.yaml` from the
/// merged resolver graph + per-importer direct-deps maps.
///
/// Mirrors upstream's
/// [`updateLockfile`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/updateLockfile.ts)
/// then the
/// [`writeLockfiles`](https://github.com/pnpm/pnpm/blob/094aa6e57b/lockfile/fs/src/write.ts#L133)
/// fan-out, with [`dependencies_graph_to_lockfile()`] doing the wire-shape lifting.
struct FreshLockfileBuildOptions<'a> {
    config: &'a Config,
    importer_manifests: &'a BTreeMap<String, &'a PackageManifest>,
    graph: &'a pacquet_resolving_deps_resolver::DependenciesGraph,
    direct_by_importer:
        &'a BTreeMap<String, BTreeMap<String, pacquet_resolving_deps_resolver::DepPath>>,
    resolved_overrides: Option<IndexMap<String, String>>,
    catalogs: &'a pacquet_catalogs_types::Catalogs,
    pnpmfile_checksum: Option<&'a str>,
    patched_dependency_hashes: Option<&'a BTreeMap<String, String>>,
}

fn build_fresh_lockfile(opts: FreshLockfileBuildOptions<'_>) -> Lockfile {
    let FreshLockfileBuildOptions {
        config,
        importer_manifests,
        graph,
        direct_by_importer,
        resolved_overrides,
        catalogs,
        pnpmfile_checksum,
        patched_dependency_hashes,
    } = opts;
    let mut importers = BTreeMap::new();
    for (id, manifest) in importer_manifests {
        let direct = direct_by_importer.get(id).cloned().unwrap_or_default();
        importers.insert(
            id.clone(),
            ImporterLockfileInput { manifest, direct_dependencies_by_alias: direct },
        );
    }
    dependencies_graph_to_lockfile(GraphToLockfileOptions {
        importers,
        graph,
        auto_install_peers: config.auto_install_peers,
        dedupe_peers: config.dedupe_peers,
        exclude_links_from_lockfile: config.exclude_links_from_lockfile,
        inject_workspace_packages: config.inject_workspace_packages,
        peers_suffix_max_length: (config.peers_suffix_max_length
            != pacquet_config::default_peers_suffix_max_length())
        .then_some(config.peers_suffix_max_length),
        overrides: resolved_overrides,
        ignored_optional_dependencies: config.ignored_optional_dependencies.clone(),
        patched_dependencies: patched_dependency_hashes.cloned(),
        package_extensions_checksum: compute_package_extensions_checksum(config),
        pnpmfile_checksum: pnpmfile_checksum.map(str::to_string),
        catalogs,
        registry: &config.registry,
        lockfile_include_tarball_url: config.lockfile_include_tarball_url,
    })
}

/// Hash `Config::package_extensions` into the prefixed sha256 string
/// pnpm writes to `pnpm-lock.yaml#packageExtensionsChecksum`.
/// Mirrors upstream's
/// [`hashObjectNullableWithPrefix(opts.packageExtensions)`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/src/install/index.ts#L545)
/// call at install time. Returns `None` when no extensions are
/// configured so the field is omitted from the on-disk lockfile, the
/// same way pnpm omits `packageExtensionsChecksum` when the input is
/// `undefined` or `{}`.
fn compute_package_extensions_checksum(config: &Config) -> Option<String> {
    let extensions =
        config.package_extensions.as_ref().filter(|extensions| !extensions.is_empty())?;
    let value = serde_json::to_value(extensions).ok()?;
    pacquet_graph_hasher::hash_object_nullable_with_prefix(&value)
}

/// [`Resolver`] adapter that delegates to a shared `Arc<dyn Resolver>`.
///
/// [`DefaultResolver::new`] takes `Vec<Box<dyn Resolver>>` — one owner
/// per chain slot. The npm resolver, however, is also handed to the
/// runtime resolvers (`Node` / `Deno` / `Bun` reuse it for version
/// picking) via `Arc<dyn Resolver>`, so the same instance owns its
/// metadata cache across both call paths. This wrapper bridges
/// the two by implementing [`Resolver`] on a `Box<ArcResolver>`,
/// forwarding every call to the shared backing resolver.
struct ArcResolver(Arc<dyn Resolver>);

impl Resolver for ArcResolver {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        self.0.resolve(wanted_dependency, opts)
    }

    fn resolve_latest<'a>(
        &'a self,
        query: &'a LatestQuery,
        opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        self.0.resolve_latest(query, opts)
    }
}

#[cfg(test)]
mod tests;
