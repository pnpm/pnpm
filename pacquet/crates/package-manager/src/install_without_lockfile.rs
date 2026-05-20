use crate::{
    HoistedDependencies, InstallPackageFromRegistry, InstallPackageFromRegistryError,
    LinkVirtualStoreBins, LinkVirtualStoreBinsError, store_init::init_store_dir_best_effort,
};
use async_recursion::async_recursion;
use dashmap::DashSet;
use derive_more::{Display, Error};
use futures_util::future;
use miette::Diagnostic;
use pacquet_cmd_shim::{Host, LinkBinsError, link_bins};
use pacquet_config::Config;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_reporter::{LogEvent, LogLevel, Reporter, Stage, StageLog};
use pacquet_resolving_deps_resolver::{
    DirectDep, ResolveDependencyTreeError, ResolveDependencyTreeOptions, ResolvedTree,
    resolve_dependency_tree,
};
use pacquet_resolving_npm_resolver::{InMemoryPackageMetaCache, NpmResolver};
use pacquet_resolving_resolver_base::ResolveOptions;
use pacquet_store_dir::{SharedVerifiedFilesCache, StoreIndex, StoreIndexWriter};
use pacquet_tarball::MemCache;
use pipe_trait::Pipe;
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::{Arc, atomic::AtomicU8},
};

/// In-memory dedupe set for packages already materialized this
/// install. Keyed by virtual-store name (`{name-with-slashes-replaced}@{version}`).
pub type ResolvedPackages = DashSet<String>;

/// This subroutine install packages from a `package.json` without reading or writing a lockfile.
///
/// **Brief overview for each package:**
/// * Resolve the dependency through the [`NpmResolver`] chain
///   ([`resolve_dependency_tree`] builds the full tree first).
/// * Fetch a tarball of each resolved package and extract it into the
///   store directory.
/// * Import (by reflink, hardlink, or copy) the files from the store
///   dir to `node_modules/.pacquet/{name}@{version}/node_modules/{name}/`.
/// * Create dependency symbolic links in
///   `node_modules/.pacquet/{name}@{version}/node_modules/`.
/// * Create a symbolic link at `node_modules/{name}`.
#[must_use]
pub struct InstallWithoutLockfile<'a, DependencyGroupList> {
    pub tarball_mem_cache: &'a MemCache,
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
}

/// Error type of [`InstallWithoutLockfile`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum InstallWithoutLockfileError {
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
}

impl<'a, DependencyGroupList> InstallWithoutLockfile<'a, DependencyGroupList> {
    /// Execute the subroutine.
    ///
    /// The without-lockfile path always returns an empty
    /// [`HoistedDependencies`] map. Hoisting needs the resolved
    /// snapshot graph the lockfile carries; without it, pacquet has
    /// nothing to walk. Frozen-lockfile installs (the production
    /// pacquet path) get the full hoist treatment via
    /// [`crate::InstallFrozenLockfile::run`]. The signature symmetry
    /// keeps `Install::run` from branching on which sub-path produced
    /// the result.
    pub async fn run<Reporter: self::Reporter>(
        self,
    ) -> Result<HoistedDependencies, InstallWithoutLockfileError>
    where
        DependencyGroupList: IntoIterator<Item = DependencyGroup>,
    {
        let InstallWithoutLockfile {
            tarball_mem_cache,
            http_client,
            http_client_arc,
            config,
            manifest,
            dependency_groups,
            resolved_packages,
            logged_methods,
            requester,
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
        let npm_resolver = NpmResolver {
            registries,
            named_registries: HashMap::new(),
            http_client: Arc::clone(&http_client_arc),
            auth_headers: Arc::clone(&config.auth_headers),
            meta_cache: Arc::new(InMemoryPackageMetaCache::default()),
            cache_dir: Some(config.cache_dir.clone()),
            offline: config.offline,
            prefer_offline: config.prefer_offline,
            ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
        };

        let tree_opts = ResolveDependencyTreeOptions {
            auto_install_peers: config.auto_install_peers,
            base_opts: ResolveOptions {
                default_tag: Some("latest".to_string()),
                ..ResolveOptions::default()
            },
        };

        let tree = resolve_dependency_tree(&npm_resolver, manifest, dependency_groups, tree_opts)
            .await
            .map_err(InstallWithoutLockfileError::ResolveDependencyTree)?;

        // Drop the resolver (and its meta cache) before the install
        // pass: the tree captures every `ResolveResult` we need.
        drop(npm_resolver);

        // Open the read-only SQLite index once per install, shared across
        // every `DownloadTarballToStore`. See the matching comment in
        // `create_virtual_store.rs` for the full rationale, including the
        // `JoinError`-to-cache-miss degradation (with a `warn!` so it
        // stays diagnosable).
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

        // Batched store-index writer. See `create_virtual_store.rs` for
        // the full rationale — we spawn once, every tarball just queues a
        // row, and one writer task flushes them in batched transactions.
        let (store_index_writer, writer_task) = StoreIndexWriter::spawn(store_dir);
        let store_index_writer_ref = Some(&store_index_writer);

        // Install-scoped `verifiedFilesCache`. See the matching block
        // in `create_virtual_store.rs` for the full rationale — pnpm
        // threads one `Set<string>` through every package's verify
        // pass so a CAFS path stat'd for one package skips the stat
        // for any later package referencing the same blob.
        let verified_files_cache = SharedVerifiedFilesCache::default();

        let install_ctx = InstallCtx {
            tarball_mem_cache,
            http_client,
            config,
            tree: &tree,
            store_index: store_index_ref,
            store_index_writer: store_index_writer_ref,
            verified_files_cache: &verified_files_cache,
            logged_methods,
            resolved_packages,
            requester,
        };

        tree.direct
            .iter()
            .map(|dep| install_subtree::<Reporter>(&install_ctx, dep, &config.modules_dir))
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
            .map_err(InstallWithoutLockfileError::LinkBins)?;

        // No lockfile here, so no prefetched manifests are available —
        // fall back to the legacy readdir-driven path (slots discovered
        // by walking `<virtual_store_dir>`, child manifests read from
        // disk). The frozen-lockfile path skips both via
        // [`LinkVirtualStoreBins::snapshots`] / `package_manifests`.
        //
        // The bin linker also doesn't need GVS-aware slot lookups
        // here: without snapshots there are no GVS slot directories to
        // compute. Construct a legacy layout so the readdir path
        // enumerates `config.virtual_store_dir` exactly as before. GVS
        // is scoped to frozen-lockfile installs (pnpm/pacquet#432); the
        // without-lockfile fallback stays project-local.
        let layout = crate::VirtualStoreLayout::legacy(config.virtual_store_dir.clone());
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
        .map_err(InstallWithoutLockfileError::LinkVirtualStoreBins)?;

        // Mirrors upstream `link.ts:167-170`: `importing_done` fires once
        // extraction and symlink linking are complete. The without-lockfile
        // path does not run lifecycle scripts today, so emitting here also
        // marks end-of-install for reporters.
        // <https://github.com/pnpm/pnpm/blob/80037699fb/installing/deps-installer/src/install/link.ts#L167>
        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: requester.to_string(),
            stage: Stage::ImportingDone,
        }));

        Ok(BTreeMap::new())
    }
}

/// Per-install state threaded into [`install_subtree`]. Holds every
/// shared handle the per-package installer needs plus a borrowed view
/// of the resolved tree the resolve pass produced.
struct InstallCtx<'a> {
    tarball_mem_cache: &'a MemCache,
    http_client: &'a ThrottledClient,
    config: &'static Config,
    tree: &'a ResolvedTree,
    store_index: Option<&'a pacquet_store_dir::SharedReadonlyStoreIndex>,
    store_index_writer: Option<&'a Arc<StoreIndexWriter>>,
    verified_files_cache: &'a SharedVerifiedFilesCache,
    logged_methods: &'a AtomicU8,
    resolved_packages: &'a ResolvedPackages,
    requester: &'a str,
}

/// Install the package referenced by `dep` plus its transitive
/// children. Recurses into each child's `node_modules/.pacquet/<vsn>/
/// node_modules/` so transitive symlinks land in their parent's slot.
#[async_recursion]
async fn install_subtree<'ctx, Reporter>(
    ctx: &InstallCtx<'ctx>,
    dep: &DirectDep,
    node_modules_dir: &Path,
) -> Result<(), InstallWithoutLockfileError>
where
    Reporter: self::Reporter,
{
    let package = ctx
        .tree
        .packages
        .get(&dep.id)
        .expect("resolve_dependency_tree must populate every referenced id");

    let virtual_store_name = format!(
        "{}@{}",
        package.result.id.name.to_string().replace('/', "+"),
        package.result.id.suffix,
    );

    // `first_visit` is the `(name, version)`-level signal: gates the
    // tarball download, the virtual-store import, and the
    // `pnpm:progress resolved` / `pnpm:progress imported` emits so
    // they fire once per package (matching pnpm's reporter contract).
    // The per-parent symlink runs on every edge regardless, so a
    // dependency reached from two different parents still gets a
    // working `node_modules/<alias>` entry under each parent.
    let first_visit = ctx.resolved_packages.insert(virtual_store_name.clone());

    InstallPackageFromRegistry {
        tarball_mem_cache: ctx.tarball_mem_cache,
        http_client: ctx.http_client,
        config: ctx.config,
        store_index: ctx.store_index,
        store_index_writer: ctx.store_index_writer,
        verified_files_cache: ctx.verified_files_cache,
        logged_methods: ctx.logged_methods,
        requester: ctx.requester,
        node_modules_dir,
        alias: &dep.alias,
        resolution: &package.result,
        first_visit,
    }
    .run::<Reporter>()
    .await
    .map_err(InstallWithoutLockfileError::InstallPackageFromRegistry)?;

    // Dedup the recursion only. The first visitor walks this
    // package's children into its virtual-store node_modules slot;
    // subsequent visitors share that slot and don't need to repeat
    // the walk.
    if !first_visit {
        tracing::info!(target: "pacquet::install", package = %virtual_store_name, "Skip subset");
        return Ok(());
    }

    let child_node_modules =
        ctx.config.virtual_store_dir.join(&virtual_store_name).join("node_modules");

    package
        .children
        .iter()
        .map(|child| install_subtree::<Reporter>(ctx, child, &child_node_modules))
        .pipe(future::try_join_all)
        .await?;

    Ok(())
}
