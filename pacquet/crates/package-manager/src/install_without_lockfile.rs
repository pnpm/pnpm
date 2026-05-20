use crate::{
    HoistedDependencies, InstallPackageFromRegistry, InstallPackageFromRegistryError,
    LinkVirtualStoreBins, LinkVirtualStoreBinsError, store_init::init_store_dir_best_effort,
};
use async_recursion::async_recursion;
use dashmap::{DashMap, mapref::entry::Entry};
use derive_more::{Display, Error};
use futures_util::future;
use miette::Diagnostic;
use pacquet_cmd_shim::{Host, LinkBinsError, link_bins};
use pacquet_config::Config;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_reporter::{LogEvent, LogLevel, Reporter, Stage, StageLog};
use pacquet_resolving_deps_resolver::{
    DepPath, DependenciesGraph, ResolveDependencyTreeError, ResolveDependencyTreeOptions,
    ResolvePeersOptions, resolve_dependency_tree, resolve_peers,
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

    /// `minimumReleaseAgeExclude` patterns rejected at compile time.
    /// Mirrors upstream's `ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE`.
    #[display("Invalid value in minimumReleaseAgeExclude: {_0}")]
    #[diagnostic(code(ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE))]
    MinimumReleaseAgeExclude(#[error(source)] pacquet_config::version_policy::VersionPolicyError),

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
            .map_err(InstallWithoutLockfileError::MinimumReleaseAgeExclude)?;

        let tree_opts = ResolveDependencyTreeOptions {
            auto_install_peers: config.auto_install_peers,
            base_opts: ResolveOptions {
                default_tag: Some("latest".to_string()),
                published_by,
                published_by_exclude,
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

        // Peer-resolution pass. Walks the per-occurrence tree built
        // above, matches each visited package's `peerDependencies`
        // against its parent chain, and emits a depPath-keyed graph
        // the install pass consumes. Peer issues collected here are
        // not fatal — they are reported (TODO: wire into the reporter
        // once the issue renderer is ported) and the install proceeds
        // with whichever candidate was reachable.
        let peers_result =
            resolve_peers(&tree, ResolvePeersOptions { peers_suffix_max_length: 1000 });
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

        let install_ctx = InstallCtx {
            tarball_mem_cache,
            http_client,
            config,
            graph: &peers_result.graph,
            store_index: store_index_ref,
            store_index_writer: store_index_writer_ref,
            verified_files_cache: &verified_files_cache,
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
        let layout = crate::VirtualStoreLayout::legacy(
            config.virtual_store_dir.clone(),
            config.virtual_store_dir_max_length as usize,
        );
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
/// of the depPath-keyed graph the peer-resolution pass produced.
struct InstallCtx<'a> {
    tarball_mem_cache: &'a MemCache,
    http_client: &'a ThrottledClient,
    config: &'static Config,
    graph: &'a DependenciesGraph,
    store_index: Option<&'a pacquet_store_dir::SharedReadonlyStoreIndex>,
    store_index_writer: Option<&'a Arc<StoreIndexWriter>>,
    verified_files_cache: &'a SharedVerifiedFilesCache,
    logged_methods: &'a AtomicU8,
    resolved_packages: &'a ResolvedPackages,
    requester: &'a str,
}

/// Install the package referenced by `dep_path` plus its transitive
/// children. Recurses into each child's `node_modules/.pacquet/<vsn>/
/// node_modules/` so transitive symlinks land in their parent's slot.
///
/// `dep_path` is the depPath key produced by [`resolve_peers`] —
/// `pkgIdWithPatchHash` for pure packages, `pkgId(peer1@v)(peer2@v)`
/// when peer-suffix variation applies. The virtual-store slot name is
/// derived via [`pacquet_deps_path::dep_path_to_filename`] so it stays
/// stable across peer variants of the same package.
#[async_recursion]
async fn install_subtree<'ctx, Reporter>(
    ctx: &InstallCtx<'ctx>,
    alias: &str,
    dep_path: &DepPath,
    node_modules_dir: &Path,
) -> Result<(), InstallWithoutLockfileError>
where
    Reporter: self::Reporter,
{
    let node =
        ctx.graph.get(dep_path).expect("resolve_peers must populate every referenced depPath");

    // Slot name = the depPath flattened to a filesystem-safe form.
    // Mirrors upstream's `depPathToFilename(depPath, virtualStoreDirMaxLength)`.
    // `dep_path_to_filename` already applies the same trailing length /
    // case shortening that `pacquet_crypto_hash::shorten_virtual_store_name`
    // exposes for the flat-name call sites; consume the configured cap
    // from `ctx.config.virtual_store_dir_max_length` so users can override
    // via `pnpm-workspace.yaml` / env.
    let virtual_store_name = pacquet_deps_path::dep_path_to_filename(
        dep_path.as_str(),
        ctx.config.virtual_store_dir_max_length as usize,
    );

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
                return Err(InstallWithoutLockfileError::FirstWriterAborted { virtual_store_name });
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
        logged_methods: ctx.logged_methods,
        requester: ctx.requester,
        node_modules_dir,
        alias,
        resolution: &node.resolve_result,
        first_visit,
    }
    .run::<Reporter>()
    .await
    .map_err(InstallWithoutLockfileError::InstallPackageFromRegistry)?;

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

    let child_node_modules =
        ctx.config.virtual_store_dir.join(&virtual_store_name).join("node_modules");

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
