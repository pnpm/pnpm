use crate::{
    AllowBuildPolicy, GraphToLockfileOptions, HoistedDependencies, InstallPackageFromRegistry,
    InstallPackageFromRegistryError, LinkVirtualStoreBins, LinkVirtualStoreBinsError,
    VersionPolicyError, VirtualStoreLayout, dependencies_graph_to_lockfile,
    store_init::init_store_dir_best_effort,
};
use async_recursion::async_recursion;
use dashmap::{DashMap, mapref::entry::Entry};
use derive_more::{Display, Error};
use futures_util::future;
use miette::Diagnostic;
use pacquet_catalogs_types::Catalogs;
use pacquet_cmd_shim::{Host, LinkBinsError, link_bins};
use pacquet_config::Config;
use pacquet_engine_runtime_bun_resolver::BunResolver;
use pacquet_engine_runtime_deno_resolver::DenoResolver;
use pacquet_engine_runtime_node_resolver::NodeResolver;
use pacquet_lockfile::{Lockfile, PackageKey, SaveLockfileError};
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_reporter::{LogEvent, LogLevel, Reporter, Stage, StageLog};
use pacquet_resolving_default_resolver::DefaultResolver;
use pacquet_resolving_deps_resolver::{
    DepPath, DependenciesGraph, ResolveDependencyTreeError, ResolveImporterError,
    ResolveImporterOptions, resolve_importer,
};
use pacquet_resolving_git_resolver::{GitResolver, RealGitProbe, RealGitRunner};
use pacquet_resolving_local_resolver::{
    LocalPathResolver, LocalResolverContext, LocalSchemeResolver,
};
use pacquet_resolving_npm_resolver::{
    InMemoryPackageMetaCache, MergeNamedRegistriesError, NamedRegistryResolver, NpmResolver,
    merge_named_registries, shared_packument_fetch_locker,
};
use pacquet_resolving_resolver_base::{
    LatestQuery, ResolveFuture, ResolveLatestFuture, ResolveOptions, Resolver, WantedDependency,
};
use pacquet_resolving_tarball_resolver::TarballResolver;
use pacquet_store_dir::{SharedVerifiedFilesCache, StoreIndex, StoreIndexWriter};
use pacquet_tarball::MemCache;
use pipe_trait::Pipe;
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
/// writer's materialization is complete, save_path is on disk).
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
/// * Resolve the dependency through the [`NpmResolver`] chain
///   ([`resolve_importer`] builds the full tree and hoists peers first).
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
    /// Shared in-memory tarball cache. Held behind [`Arc`] for parity
    /// with the [`crate::Install`] surface; the install-side calls
    /// take `&MemCache` via deref.
    pub tarball_mem_cache: Arc<MemCache>,
    pub resolved_packages: &'a ResolvedPackages,
    pub http_client: &'a ThrottledClient,
    /// Same client behind an [`Arc`] for the [`NpmResolver`], whose
    /// stored `ThrottledClient` outlives any per-call borrow.
    pub http_client_arc: Arc<ThrottledClient>,
    pub config: &'static Config,
    pub manifest: &'a PackageManifest,
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
    /// Existing `pnpm-lock.yaml` to seed `getPreferredVersionsFromLockfileAndManifests`
    /// with already-pinned `(name, version)` pairs. `Some` on the
    /// stale-lockfile / `preferFrozenLockfile: false` rewrite path
    /// — the resolver biases toward the seeded versions when they
    /// still satisfy the spec so unrelated dependencies keep their
    /// pins. `None` on the no-lockfile path. Mirrors upstream's
    /// `update: false` resolver mode at
    /// <https://github.com/pnpm/pnpm/blob/097983fbca/lockfile/preferred-versions/src/index.ts#L13-L33>.
    pub wanted_lockfile: Option<&'a Lockfile>,
}

/// Error type of [`InstallWithFreshLockfile`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum InstallWithFreshLockfileError {
    #[diagnostic(transparent)]
    InstallPackageFromRegistry(#[error(source)] InstallPackageFromRegistryError),

    #[diagnostic(transparent)]
    LinkBins(#[error(source)] LinkBinsError),

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

    /// A user-defined `namedRegistries` entry mapped an alias to a
    /// non-http(s) URL. Surfaced at resolver construction so the
    /// install fails fast with a specific error code instead of a
    /// downstream 404. Mirrors upstream's
    /// `ERR_PNPM_INVALID_NAMED_REGISTRY_URL`.
    #[diagnostic(transparent)]
    InvalidNamedRegistry(#[error(source)] MergeNamedRegistriesError),

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
    /// `Some` when the install resolved a graph that was written to
    /// `pnpm-lock.yaml`; `None` when the write was skipped (today: only
    /// `config.lockfile=false`). The caller mirrors the same gate when
    /// deciding whether to persist the current-lockfile.
    pub wanted_lockfile: Option<Lockfile>,
}

impl<'a, DependencyGroupList> InstallWithFreshLockfile<'a, DependencyGroupList> {
    /// Execute the subroutine.
    ///
    /// The fresh-lockfile path's [`HoistedDependencies`] slot is always
    /// empty. Hoisting needs the resolved snapshot graph the lockfile
    /// carries; this path serializes the graph into `pnpm-lock.yaml`
    /// itself, but the hoist pass still runs only inside the
    /// frozen-lockfile install ([`crate::InstallFrozenLockfile::run`]).
    /// The signature symmetry keeps `Install::run` from branching on
    /// which sub-path produced the result.
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
            manifest,
            dependency_groups,
            resolved_packages,
            logged_methods,
            requester,
            catalogs,
            lockfile_dir,
            workspace_packages,
            wanted_lockfile,
        } = self;

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

        let meta_cache = Arc::new(InMemoryPackageMetaCache::default());

        // One per-cache-key packument fetch serializer shared between
        // the npm and named-registry resolvers. Ports upstream's
        // [`metafileOperationLimits`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L42-L44):
        // concurrent picks for the same `(registry, name)` coalesce
        // into a single network fetch instead of firing N parallel
        // HTTP GETs queued behind the `ThrottledClient` semaphore.
        let fetch_locker = shared_packument_fetch_locker();

        let npm_resolver: Arc<dyn Resolver> = Arc::new(NpmResolver {
            registries,
            named_registries: merged_named_registries.clone(),
            http_client: Arc::clone(&http_client_arc),
            auth_headers: Arc::clone(&config.auth_headers),
            meta_cache: Arc::clone(&meta_cache),
            fetch_locker: Arc::clone(&fetch_locker),
            cache_dir: Some(config.cache_dir.clone()),
            offline: config.offline,
            prefer_offline: config.prefer_offline,
            ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
            // Default to abbreviated metadata at resolve time and let
            // [`pick_package`] upgrade per-call when `published_by` or
            // `optional` demand it. Mirrors upstream's
            // [`ctx.fullMetadata`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L175)
            // default. Pacquet's `Config` doesn't surface a
            // `fullMetadata` knob; if one lands later, thread it
            // here.
            full_metadata: false,
        });
        let git_resolver = GitResolver::new(
            Arc::new(RealGitProbe::new(Arc::clone(&http_client_arc))),
            Arc::new(RealGitRunner::new()),
        );
        let tarball_resolver = TarballResolver { http_client: Arc::clone(&http_client_arc) };
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
            auth_headers: Arc::clone(&config.auth_headers),
            meta_cache: Arc::clone(&meta_cache),
            fetch_locker: Arc::clone(&fetch_locker),
            cache_dir: Some(config.cache_dir.clone()),
            offline: config.offline,
            prefer_offline: config.prefer_offline,
            ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
            // Same rationale as `NpmResolver.full_metadata` above.
            full_metadata: false,
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
        let resolver: Box<dyn Resolver> = Box::new(DefaultResolver::new(vec![
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
        let published_by = config.minimum_release_age.and_then(|minutes| {
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

        // Seed `allPreferredVersions` from the importer manifest +
        // the wanted lockfile's snapshots (when an existing one is
        // present and is being rewritten). Mirrors upstream's
        // `getPreferredVersionsFromLockfileAndManifests` shape: the
        // manifest contributes direct-dep specifiers, the lockfile
        // contributes concrete `(name, version)` pins that bump the
        // weight of an already-matching direct-dep entry. Without the
        // lockfile-side seed, every install on a stale lockfile would
        // resolve unrelated entries from scratch and lose their
        // recorded pins; see <https://pnpm.io/settings#preferfrozenlockfile>.
        let all_preferred_versions =
            pacquet_lockfile_preferred_versions::get_preferred_versions_from_lockfile_and_manifests(
                wanted_lockfile.and_then(|lockfile| lockfile.snapshots.as_ref()),
                &[manifest],
            );

        // Thread the manifest's directory and the lockfile root into
        // the resolver's `ResolveOptions` so `workspace:` and `link:`
        // resolutions can compute the right relative paths.
        let project_dir =
            manifest.path().parent().expect("manifest path always has a parent dir").to_path_buf();

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

        let importer_opts = ResolveImporterOptions {
            auto_install_peers: config.auto_install_peers,
            auto_install_peers_from_highest_match: config.auto_install_peers_from_highest_match,
            resolve_peers_from_workspace_root: config.resolve_peers_from_workspace_root,
            all_preferred_versions,
            patched_dependencies,
            base_opts: ResolveOptions {
                default_tag: Some("latest".to_string()),
                published_by,
                published_by_exclude,
                project_dir,
                lockfile_dir: lockfile_dir.to_path_buf(),
                workspace_packages,
                block_exotic_subdeps: config.block_exotic_subdeps,
                ..ResolveOptions::default()
            },
            catalogs,
        };

        let importer_result =
            resolve_importer(&*resolver, manifest, dependency_groups, importer_opts)
                .await
                .map_err(InstallWithFreshLockfileError::ResolveImporter)?;

        // Drop the resolver (and its packument cache) before the
        // install pass. Dropping `resolver` releases the strong
        // reference held by the `ArcResolver` wrapper; the standalone
        // `npm_resolver` binding holds a second strong reference
        // because the deno- and bun-resolvers were handed a clone of
        // the same `Arc` for their version-selection delegate. Drop
        // both so the `NpmResolver`'s meta cache is freed before the
        // install pass starts pulling tarballs into the CAFS.
        drop(resolver);
        drop(npm_resolver);

        // Open the read-only SQLite index, spawn the batched writer,
        // and allocate the install-scoped `verifiedFilesCache`. Same
        // shape `create_virtual_store::run` opens for the frozen-
        // lockfile install path — the warm-cache prefetch below shares
        // them with the per-package install routines.
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
        let store_index_writer_ref = Some(&store_index_writer);

        let verified_files_cache = SharedVerifiedFilesCache::default();

        // Warm-cache batched prefetch: collect every `(integrity,
        // pkg_id)` pair the resolver produced, run one batched SQL
        // `SELECT ... WHERE key IN (...)` against the store index,
        // then verify each row's files on rayon. Mirrors what
        // `create_virtual_store::run` already does for the frozen-
        // lockfile path. Without this, the per-package `run_with_mem_cache`
        // → `load_cached_cas_paths` flow fires N individual `spawn_blocking`
        // tasks that all serialize on `Arc<Mutex<StoreIndex>>` — at ~1k
        // resolved packages the Mutex contention dominates the resolve-
        // walk wall-clock under global virtual store, where the install
        // side is otherwise just symlinking.
        let cache_keys: Vec<String> = collect_prefetch_cache_keys(&importer_result.peers_result);
        let prefetch = pacquet_tarball::prefetch_cas_paths(
            store_index_ref.cloned(),
            store_dir,
            cache_keys,
            config.verify_store_integrity,
            SharedVerifiedFilesCache::clone(&verified_files_cache),
        )
        .await;
        let prefetched_cas_paths = prefetch.cas_paths;

        // Peer-resolution result (collected by `resolve_importer` after
        // the hoist loop converged). Peer issues collected here are not
        // fatal — they are reported (TODO: wire into the reporter once
        // the issue renderer is ported) and the install proceeds with
        // whichever candidate was reachable.
        let peers_result = &importer_result.peers_result;
        if !peers_result.peer_dependency_issues.missing.is_empty()
            || !peers_result.peer_dependency_issues.bad.is_empty()
        {
            tracing::warn!(
                target: "pacquet::install",
                missing = peers_result.peer_dependency_issues.missing.len(),
                bad = peers_result.peer_dependency_issues.bad.len(),
                "Peer dependency issues detected (issue renderer not ported yet)",
            );
        }

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
        let layout_lockfile = if config.enable_global_virtual_store {
            Some(build_fresh_lockfile(config, manifest, &importer_result))
        } else {
            None
        };
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
        let layout = VirtualStoreLayout::new(
            config,
            engine_name.as_deref(),
            layout_lockfile.as_ref().and_then(|lockfile| lockfile.snapshots.as_ref()),
            layout_lockfile.as_ref().and_then(|lockfile| lockfile.packages.as_ref()),
            Some(&allow_build_policy),
        );

        let install_ctx = InstallCtx {
            tarball_mem_cache: tarball_mem_cache.as_ref(),
            http_client,
            config,
            graph: &peers_result.graph,
            layout: &layout,
            store_index: store_index_ref,
            store_index_writer: store_index_writer_ref,
            verified_files_cache: &verified_files_cache,
            prefetched_cas_paths: &prefetched_cas_paths,
            logged_methods,
            resolved_packages,
            requester,
        };

        peers_result
            .direct_dependencies_by_alias
            .iter()
            .map(|(alias, dep_path)| {
                install_subtree::<Reporter>(&install_ctx, alias, dep_path, &config.modules_dir)
            })
            .pipe(future::try_join_all)
            .await?;

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

        // Link bins. Direct dependencies first (root project's
        // `node_modules/.bin`) and then per-slot children inside the
        // virtual store. Mirrors the same two-call shape as
        // `install_frozen_lockfile.rs`. We re-walk `<modules_dir>` instead
        // of replaying the manifest because the `dependency_groups`
        // iterator was already consumed by the install loop above; pnpm's
        // own `linkBins(modulesDir, binsDir)` overload uses the same
        // strategy.
        link_bins::<Host>(&config.modules_dir, &config.modules_dir.join(".bin"))
            .map_err(InstallWithFreshLockfileError::LinkBins)?;

        // No prefetched manifests are available — fall back to the
        // legacy readdir-driven path (slots discovered by walking
        // `<virtual_store_dir>` or `<store_dir>/links` per the active
        // layout, child manifests read from disk). The frozen-lockfile
        // path skips both via [`LinkVirtualStoreBins::snapshots`] /
        // `package_manifests`.
        //
        // The bin linker reuses the install-scoped `layout` above so
        // GVS installs walk the shared `<store_dir>/links/...`
        // directory instead of the project-local
        // `<virtual_store_dir>/.pnpm` one.
        let empty_manifests = std::collections::HashMap::new();
        let empty_skipped = crate::SkippedSnapshots::new();
        LinkVirtualStoreBins {
            layout: &layout,
            snapshots: None,
            packages: None,
            package_manifests: &empty_manifests,
            // The without-lockfile path has no installability check
            // (no `packages:` metadata to evaluate constraints
            // against), so the skip set is empty by definition.
            skipped: &empty_skipped,
        }
        .run()
        .map_err(InstallWithFreshLockfileError::LinkVirtualStoreBins)?;

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
        let wanted_lockfile = if config.lockfile {
            // GVS already built one above for the layout — reuse it
            // rather than walking the resolver graph again. When GVS
            // is off, `layout_lockfile` is `None` and we build here.
            let lockfile_to_save = layout_lockfile
                .unwrap_or_else(|| build_fresh_lockfile(config, manifest, &importer_result));
            let target = lockfile_dir.join(Lockfile::FILE_NAME);
            lockfile_to_save
                .save_to_path(&target)
                .map_err(InstallWithFreshLockfileError::SaveWantedLockfile)?;
            Some(lockfile_to_save)
        } else {
            None
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
            hoisted_dependencies: BTreeMap::new(),
            wanted_lockfile,
        })
    }
}

/// Walk the resolver-produced graph and emit the
/// `{integrity}\t{pkg_id}` cache keys
/// [`pacquet_tarball::prefetch_cas_paths`] uses for its batched
/// `SELECT ... WHERE key IN (...)` against the store index. Mirrors
/// the equivalent collection loop in
/// [`crate::CreateVirtualStore::run`] for the frozen-lockfile path —
/// same key shape, same dedup, so the fresh-lockfile path's warm
/// batch hits the same rows pnpm or pacquet wrote on the prior
/// install.
///
/// Skips nodes whose resolver result isn't a tarball with both
/// `integrity` and a structured `name@version`: git-hosted tarballs
/// and directory / git / binary resolutions use a different key
/// shape (`pkg_id`-only) and route through the cold path. Today's
/// `install_subtree` only handles tarball+integrity anyway, so the
/// skipped entries can't be served from the prefetch either way.
fn collect_prefetch_cache_keys(
    peers_result: &pacquet_resolving_deps_resolver::ResolvePeersResult,
) -> Vec<String> {
    let mut keys: Vec<String> = peers_result
        .graph
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
            let name_ver = node.resolve_result.name_ver.as_ref()?;
            let pkg_id = format!("{}@{}", name_ver.name, name_ver.suffix);
            Some(pacquet_store_dir::store_index_key(&integrity, &pkg_id))
        })
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// Build the [`Lockfile`] for `<lockfile_dir>/pnpm-lock.yaml` from the
/// resolver's output.
///
/// Mirrors upstream's
/// [`updateLockfile`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/updateLockfile.ts)
/// then the
/// [`writeLockfiles`](https://github.com/pnpm/pnpm/blob/094aa6e57b/lockfile/fs/src/write.ts#L133)
/// fan-out, with [`dependencies_graph_to_lockfile()`] doing the wire-shape lifting.
fn build_fresh_lockfile(
    config: &Config,
    manifest: &PackageManifest,
    importer_result: &pacquet_resolving_deps_resolver::ResolveImporterResult,
) -> Lockfile {
    dependencies_graph_to_lockfile(GraphToLockfileOptions {
        manifest,
        resolved: importer_result,
        auto_install_peers: config.auto_install_peers,
        // `excludeLinksFromLockfile` isn't ported to pacquet's `Config`
        // yet (pnpm/pacquet#431 brings workspace support, which is
        // when the knob starts mattering). Default to `false` —
        // matches upstream's default and round-trips cleanly through
        // `@pnpm/lockfile.settings-checker`.
        exclude_links_from_lockfile: false,
        overrides: config
            .overrides
            .as_ref()
            .map(|map| map.iter().map(|(key, value)| (key.clone(), value.clone())).collect()),
        ignored_optional_dependencies: config.ignored_optional_dependencies.clone(),
    })
}

/// Per-install state threaded into [`install_subtree`]. Holds every
/// shared handle the per-package installer needs plus a borrowed view
/// of the depPath-keyed graph the peer-resolution pass produced.
struct InstallCtx<'a> {
    tarball_mem_cache: &'a MemCache,
    http_client: &'a ThrottledClient,
    config: &'static Config,
    graph: &'a DependenciesGraph,
    /// Install-scoped virtual-store layout. Computes the per-snapshot
    /// `slot_dir` consumed by both `install_subtree` (for the child
    /// `node_modules/` dir) and [`InstallPackageFromRegistry`] (for
    /// the package's save path). Falls through to the legacy
    /// `<virtual_store_dir>/<flat-name>` shape when GVS is off; under
    /// GVS, returns `<store_dir>/links/<scope>/<name>/<version>/<hash>`.
    layout: &'a VirtualStoreLayout,
    store_index: Option<&'a pacquet_store_dir::SharedReadonlyStoreIndex>,
    store_index_writer: Option<&'a Arc<StoreIndexWriter>>,
    verified_files_cache: &'a SharedVerifiedFilesCache,
    /// Warm-cache lookup table built once at install start via
    /// [`pacquet_tarball::prefetch_cas_paths`]. Threaded through to
    /// [`InstallPackageFromRegistry`] so the per-package
    /// `run_with_mem_cache` short-circuits the per-snapshot SQLite
    /// round-trip + per-file `fs::metadata` work when the
    /// `(integrity, pkg_id)` is already in the CAFS.
    prefetched_cas_paths: &'a pacquet_tarball::PrefetchedCasPaths,
    logged_methods: &'a AtomicU8,
    resolved_packages: &'a ResolvedPackages,
    requester: &'a str,
}

/// Install the package referenced by `dep_path` plus its transitive
/// children. Recurses into each child's `<slot_dir>/node_modules/` so
/// transitive symlinks land in their parent's virtual-store slot.
///
/// `dep_path` is the depPath key produced by the peer-resolution stage
/// inside [`resolve_importer`] —
/// `pkgIdWithPatchHash` for pure packages, `pkgId(peer1@v)(peer2@v)`
/// when peer-suffix variation applies. The slot directory is computed
/// via [`crate::VirtualStoreLayout::slot_dir`] so the GVS-on path
/// routes through `<store_dir>/links/<scope>/<name>/<version>/<hash>`
/// and the legacy path routes through
/// `<virtual_store_dir>/<flat-name>`, both addressing the same
/// peer-context-aware snapshot.
#[async_recursion]
async fn install_subtree<'ctx, Reporter>(
    ctx: &InstallCtx<'ctx>,
    alias: &str,
    dep_path: &DepPath,
    node_modules_dir: &Path,
) -> Result<(), InstallWithFreshLockfileError>
where
    Reporter: self::Reporter,
{
    let node =
        ctx.graph.get(dep_path).expect("resolve_peers must populate every referenced depPath");

    // The dedup key is the depPath flattened to a filesystem-safe
    // form — kept as the `resolved_packages` map's key so the
    // `FirstWriterAborted` diagnostic still names a human-readable
    // slot. Equal to upstream's `depPathToFilename` output. The
    // slot's *path* is resolved separately via `ctx.layout` so GVS
    // and legacy installs share the same dedup gate but land on
    // different on-disk directories.
    let virtual_store_name = pacquet_deps_path::dep_path_to_filename(
        dep_path.as_str(),
        ctx.config.virtual_store_dir_max_length as usize,
    );

    // Resolve the slot path through the install-scoped layout. The
    // depPath should always parse as a `PackageKey`
    // (`<name>@<version>[(peer)...]`); the fallback to the legacy
    // `<virtual_store_dir>/<flat-name>` shape is defensive for the
    // rare exotic depPath the resolver could emit (e.g. legacy
    // pre-v9 forms) — under GVS that would silently fall out of the
    // shared store, but the install still completes rather than
    // panicking on a parse failure.
    let slot_dir = match dep_path.as_str().parse::<PackageKey>() {
        Ok(package_key) => ctx.layout.slot_dir(&package_key),
        Err(_) => ctx.config.virtual_store_dir.join(&virtual_store_name),
    };

    // Claim the `(name, version)` slot. `first_visit` is true iff this
    // task created the watch sender; later visitors get a receiver and
    // await the first writer's completion before continuing — without
    // that gate, a second visitor's `symlink_package` could land
    // before the first writer's `import_indexed_dir` has created the
    // target directory, which `force_symlink_dir`'s Windows junction
    // fallback rejects (junctions require an existing target).
    let (first_visit, completion_rx) = match ctx.resolved_packages.entry(virtual_store_name.clone())
    {
        Entry::Vacant(slot) => {
            let (tx, _initial_rx) = watch::channel(false);
            slot.insert(tx);
            (true, None)
        }
        Entry::Occupied(slot) => (false, Some(slot.get().subscribe())),
    };

    if let Some(mut rx) = completion_rx {
        loop {
            if *rx.borrow_and_update() {
                break;
            }
            if rx.changed().await.is_err() {
                return Err(InstallWithFreshLockfileError::FirstWriterAborted {
                    virtual_store_name,
                });
            }
        }
    }

    InstallPackageFromRegistry {
        tarball_mem_cache: ctx.tarball_mem_cache,
        http_client: ctx.http_client,
        config: ctx.config,
        store_index: ctx.store_index,
        store_index_writer: ctx.store_index_writer,
        verified_files_cache: ctx.verified_files_cache,
        prefetched_cas_paths: Some(ctx.prefetched_cas_paths),
        logged_methods: ctx.logged_methods,
        requester: ctx.requester,
        node_modules_dir,
        slot_dir: &slot_dir,
        alias,
        resolution: &node.resolve_result,
        first_visit,
    }
    .run::<Reporter>()
    .await
    .map_err(InstallWithFreshLockfileError::InstallPackageFromRegistry)?;

    if first_visit {
        // `send_replace` (not `send`) is critical here: `Sender::send`
        // returns `Err` when the channel has zero receivers, leaving
        // the sender's stored value unchanged. The initial receiver
        // from `watch::channel` is dropped at the Vacant arm above to
        // avoid keeping a live receiver in the map, so the channel
        // really *does* have zero receivers when the first writer
        // races ahead of every subscriber (common for cyclic and
        // diamond graphs where the second visitor only enters the map
        // after the first writer has already finished `IPFR::run`).
        // Under `send`, that race left the stored value at `false` and
        // any later subscriber's `borrow_and_update()` would see
        // `false` and `changed().await` would block forever — the
        // hang the cycle tests hit. `send_replace` always writes the
        // value and returns the old one regardless of receiver count.
        if let Some(slot) = ctx.resolved_packages.get(&virtual_store_name) {
            slot.send_replace(true);
        }
    } else {
        // Second visitor: the per-parent symlink is the only step
        // that needed to run; the first writer is already walking
        // this package's children.
        tracing::info!(target: "pacquet::install", package = %virtual_store_name, "Skip subset");
        return Ok(());
    }

    let child_node_modules = slot_dir.join("node_modules");

    let child_node_modules_ref = &child_node_modules;
    node.children
        .iter()
        .map(|(child_alias, child_dep_path)| async move {
            install_subtree::<Reporter>(ctx, child_alias, child_dep_path, child_node_modules_ref)
                .await
        })
        .pipe(future::try_join_all)
        .await?;

    Ok(())
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
