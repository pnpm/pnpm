//! Construction of the fresh-install resolver chain and the store-side
//! handles it shares with the install pass.
//!
//! Everything here is in place before the first `resolve_workspace`
//! call: the resolver chain and the later install pass must share one
//! store index, writer, and verified-files cache, so the handles are
//! opened here and lent to both.

use super::InstallWithFreshLockfileError;
use crate::{PrefetchContext, PrefetchingResolver};
use pnpm_config::{Config, NeedsFullMetadataFor};
use pnpm_engine_pm_yarn_resolver::YarnResolver;
use pnpm_engine_runtime_bun_resolver::BunResolver;
use pnpm_engine_runtime_deno_resolver::DenoResolver;
use pnpm_engine_runtime_node_resolver::NodeResolver;
use pnpm_lockfile::{Lockfile, LockfileResolution};
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_resolving_default_resolver::DefaultResolver;
use pnpm_resolving_git_resolver::{GitFetchContext, GitResolver, RealGitProbe, RealGitRunner};
use pnpm_resolving_local_resolver::{LocalPathResolver, LocalResolverContext, LocalSchemeResolver};
use pnpm_resolving_npm_resolver::{
    InMemoryPackageMetaCache, NamedRegistryResolver, NpmResolver, merge_named_registries,
    shared_packument_fetch_locker, shared_picked_manifest_cache,
};
use pnpm_resolving_resolver_base::Resolver;
use pnpm_resolving_tarball_resolver::{PriorTarballEntry, TarballFetchContext, TarballResolver};
use pnpm_store_dir::{
    SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreDir, StoreIndex, StoreIndexWriter,
    store_index_key,
};
use pnpm_tarball::{MemCache, SharedReportedProgressKeys};
use std::{collections::HashMap, path::Path, sync::Arc};

/// The store index the resolver chain and the install pass share, plus
/// the batched writer both feed rows into.
pub(super) struct StoreIndexHandles {
    pub index: Option<SharedReadonlyStoreIndex>,
    pub writer: Arc<StoreIndexWriter>,
    pub writer_task: tokio::task::JoinHandle<Result<(), pnpm_store_dir::StoreIndexError>>,
}

/// Open the read-only index and spawn the batched writer *before* the
/// resolver chain is built: the [`TarballResolver`] (which fetches a
/// remote tarball direct dep during resolution to learn its
/// name/version/integrity) and the [`PrefetchingResolver`] both need
/// them at construction time, so the store index / writer / verify cache
/// they touch is the same one the install pass uses once resolution is
/// done.
///
/// Under `frozenStore` the store is opened read-only and the writer is
/// replaced with a drain-and-drop stub that never opens `index.db` (no
/// WAL / SHM sidecar under the read-only root).
pub(super) async fn open_store_index_handles(
    config: &Config,
    store_dir: &'static StoreDir,
) -> StoreIndexHandles {
    let index = StoreIndex::open_shared(store_dir, config.frozen_store).await;
    let (writer, writer_task) = StoreIndexWriter::spawn_for(store_dir, config.frozen_store);
    StoreIndexHandles { index, writer, writer_task }
}

pub(super) struct Registries {
    /// Scope → registry URL, as `.npmrc` resolves them.
    pub by_scope: HashMap<String, String>,
    /// `pnpm-workspace.yaml#namedRegistries` folded together with
    /// pacquet's built-in aliases — today `gh:` → GitHub Packages.
    pub named: HashMap<String, String>,
}

/// Resolve the registries every registry-touching resolver consults.
///
/// A malformed named-registry URL aborts the install with
/// `ERR_PNPM_INVALID_NAMED_REGISTRY_URL` rather than surfacing as a
/// downstream 404.
pub(super) fn resolve_registries(
    config: &Config,
) -> Result<Registries, InstallWithFreshLockfileError> {
    let user_registries_by_prefix: HashMap<String, String> =
        config.registries_by_prefix.iter().map(|(name, url)| (name.clone(), url.clone())).collect();
    Ok(Registries {
        by_scope: config.resolved_registries().into_iter().collect(),
        named: merge_named_registries(&user_registries_by_prefix)
            .map_err(InstallWithFreshLockfileError::InvalidNamedRegistry)?,
    })
}

/// Map every remote tarball the prior lockfile recorded (with an
/// integrity) to its `<integrity>\t<pkg_id>` store-index key, keyed
/// exactly as `snapshot_cache_key` / the install pass address the row,
/// so the [`TarballResolver`] can reuse a warm store entry instead of
/// re-downloading on re-resolution. Git-hosted tarballs are skipped
/// (they key by `gitHostedStoreIndexKey`, not the integrity) and just
/// re-fetch as before. Empty on a first install.
///
/// Keyed by `pkg_id` — the bare specifier — rather than
/// `resolution.tarball`, because that is what the resolver looks a
/// dependency up by. The two differ once an immutable response has
/// redirected: the lockfile then records the post-redirect URL while the
/// id stays the specifier the manifest asked for.
pub(super) fn prior_tarball_entries(
    wanted_lockfile: Option<&Lockfile>,
) -> HashMap<String, PriorTarballEntry> {
    wanted_lockfile
        .and_then(|lockfile| lockfile.packages.as_ref())
        .map(|packages| {
            packages
                .iter()
                .filter_map(|(key, metadata)| match &metadata.resolution {
                    LockfileResolution::Tarball(tarball) if tarball.git_hosted != Some(true) => {
                        let integrity = tarball.integrity.clone()?;
                        let pkg_id = key.pkg_id();
                        let store_index_key = store_index_key(&integrity.to_string(), &pkg_id);
                        Some((
                            pkg_id,
                            PriorTarballEntry {
                                integrity,
                                store_index_key,
                                tarball_url: tarball.tarball.clone(),
                            },
                        ))
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Everything [`build_resolver_chain`] needs. Borrowed for the duration
/// of the call: the wrapper clones each field into the owned form a
/// spawned download task can capture, so the returned chain carries no
/// lifetime.
pub(super) struct ResolverChainInputs<'a> {
    pub config: &'static Config,
    pub store_dir: &'static StoreDir,
    pub http_client_arc: &'a Arc<ThrottledClient>,
    pub tarball_mem_cache: &'a Arc<MemCache>,
    pub auth_headers: &'a Arc<AuthHeaders>,
    pub meta_cache: &'a Arc<InMemoryPackageMetaCache>,
    pub lockfile_dir: &'a Path,
    pub requester: &'a str,
    pub supported_architectures: Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    pub registries: &'a HashMap<String, String>,
    pub registries_by_prefix: &'a HashMap<String, String>,
    /// See `NpmResolver::full_metadata` — forced on when `time-based`
    /// resolution or the `no-downgrade` trust policy needs the
    /// per-version `time` field.
    pub full_metadata: bool,
    /// See `NpmResolver::needs_full_metadata_for` — the same question asked
    /// of one registry.
    pub needs_full_metadata_for: NeedsFullMetadataFor,
    pub wanted_lockfile: Option<&'a Lockfile>,
    pub store_index: Option<&'a SharedReadonlyStoreIndex>,
    pub store_index_writer: &'a Arc<StoreIndexWriter>,
    pub verified_files_cache: &'a SharedVerifiedFilesCache,
    pub progress_reported: &'a SharedReportedProgressKeys,
    /// Whether a resolved tarball is prefetched — `false` for a run
    /// whose install pass will never ask for those bytes.
    pub prefetch_downloads: bool,
    /// In-process hooks supplied by an embedder; `None` falls back to
    /// the on-disk `.pnpmfile.cjs` lookup.
    pub pnpmfile_hook_override: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub resolution_observer: Option<Arc<dyn crate::ResolutionObserver>>,
}

/// The assembled resolver chain plus the pieces later install phases
/// still need: the npm resolver (the stale-convergence and fast-override
/// passes re-query it), the caches the orchestrator drops before the
/// install pass, and the pnpmfile handles.
pub(super) struct ResolverChain {
    pub resolver: Box<dyn Resolver>,
    pub npm_resolver: Arc<dyn Resolver>,
    pub fetch_locker: pnpm_resolving_npm_resolver::PackumentFetchLocker,
    pub picked_manifest_cache: pnpm_resolving_npm_resolver::PickedManifestCache,
    pub custom_resolvers: Vec<Arc<dyn pnpm_hooks::CustomResolver>>,
    pub custom_fetcher_session: Option<Arc<pnpm_deps_restorer::CustomFetcherSession>>,
    pub pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
}

/// Build the fresh-install resolver chain.
///
/// Chain order: custom resolvers → npm → jsr (folded into npm) → git →
/// tarball → localScheme → node → deno → bun → namedRegistry →
/// localPath. Custom resolvers join only when they implement both
/// `canResolve` and `resolve`; the others are skipped. The local-resolver
/// split is required by named-registry: a `<alias>:@scope/pkg` specifier
/// carries an embedded `/`, which the path-shape detector
/// (`contains_path_sep` in `parse_bare_specifier.rs`) would otherwise
/// claim and prevent the named-registry resolver from running.
///
/// The chain is then wrapped twice: [`PrefetchingResolver`] so each
/// tarball-shaped result fires a background download while the tree walk
/// continues, and — for the pnpr server only — [`crate::ObservingResolver`]
/// so each resolution is reported to the client as it lands.
pub(super) async fn build_resolver_chain<Reporter: pnpm_reporter::Reporter + 'static>(
    inputs: ResolverChainInputs<'_>,
) -> Result<ResolverChain, InstallWithFreshLockfileError> {
    let ResolverChainInputs {
        config,
        store_dir,
        http_client_arc,
        tarball_mem_cache,
        auth_headers,
        meta_cache,
        lockfile_dir,
        requester,
        supported_architectures,
        registries,
        registries_by_prefix,
        full_metadata,
        needs_full_metadata_for,
        wanted_lockfile,
        store_index,
        store_index_writer,
        verified_files_cache,
        progress_reported,
        prefetch_downloads,
        pnpmfile_hook_override,
        resolution_observer,
    } = inputs;

    // One per-cache-key packument fetch serializer shared between the
    // npm and named-registry resolvers. Concurrent picks for the same
    // `(registry, name)` coalesce into a single network fetch instead of
    // firing N parallel HTTP GETs queued behind the `ThrottledClient`
    // semaphore.
    let fetch_locker = shared_packument_fetch_locker();
    // One per-`(name, version)` JSON manifest cache shared between the
    // same two resolvers, so duplicate picks of the same package version
    // reuse the already-serialised `Arc<Value>` instead of re-running
    // `serde_json::to_value` for every occurrence of a shared dep.
    let picked_manifest_cache = shared_picked_manifest_cache();

    let npm_resolver: Arc<dyn Resolver> = Arc::new(NpmResolver {
        registries: registries.clone(),
        registries_by_prefix: registries_by_prefix.clone(),
        http_client: Arc::clone(http_client_arc),
        auth_headers: Arc::clone(auth_headers),
        meta_cache: Arc::clone(meta_cache),
        fetch_locker: Arc::clone(&fetch_locker),
        picked_manifest_cache: Arc::clone(&picked_manifest_cache),
        cache_dir: Some(config.cache_dir.clone()),
        offline: config.offline,
        prefer_offline: config.prefer_offline,
        ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
        // Abbreviated metadata at resolve time unless `time-based`
        // resolution or the `no-downgrade` trust policy needs the
        // per-version `time` field (and the registry doesn't serve it in
        // abbreviated form). When `false`, [`pick_package`] still
        // upgrades per-call where `published_by` / `optional` demand it.
        full_metadata,
        needs_full_metadata_for: Some(Arc::clone(&needs_full_metadata_for)),
        filter_metadata: config.requires_filtered_full_metadata(),
        retry_opts: crate::retry_config::retry_opts_from_config(config),
    });
    // A git dep's specifier names a repo, not a package, so its name —
    // the `<name>@` half of every lockfile key it reaches — is only
    // readable from the package's own `package.json`, in the host's
    // archive or (for a repo with no archive endpoint) a checkout. Hand
    // the resolver the handles to read it, on the same rationale as the
    // remote-tarball fetch below.
    let git_resolver = GitResolver::new(
        Arc::new(RealGitProbe::new(Arc::clone(http_client_arc))),
        Arc::new(RealGitRunner::new()),
    )
    .with_fetch_context(GitFetchContext {
        http_client: Arc::clone(http_client_arc),
        store_dir,
        store_index_writer: Some(Arc::clone(store_index_writer)),
        auth_headers: Arc::clone(auth_headers),
        retry_opts: crate::retry_config::retry_opts_from_config(config),
        git_shallow_hosts: config.git_shallow_hosts.clone(),
    });
    // A remote (non-registry) tarball *direct* dependency carries no
    // name/version/integrity at resolve time — they live in the
    // tarball's `package.json`. The resolver downloads + extracts it here
    // (warming `tarball_mem_cache` keyed by URL) so the lockfile builder
    // gets the manifest + integrity and the install pass reuses the
    // extraction without a second download. Wired in both the
    // materializing and `--lockfile-only` paths: the lockfile needs the
    // integrity regardless of whether `node_modules` is built.
    let tarball_resolver = TarballResolver {
        http_client: Arc::clone(http_client_arc),
        fetch_context: Some(TarballFetchContext {
            store_dir,
            store_index_writer: Some(Arc::clone(store_index_writer)),
            mem_cache: Some(Arc::clone(tarball_mem_cache)),
            auth_headers: Arc::clone(auth_headers),
            retry_opts: crate::retry_config::retry_opts_from_config(config),
            store_index: store_index.cloned(),
            verify_store_integrity: config.verify_store_integrity,
            verified_files_cache: Arc::clone(verified_files_cache),
            prior_tarball_entries: Arc::new(prior_tarball_entries(wanted_lockfile)),
        }),
    };
    // `preserveAbsolutePaths` is wired through `Config`; thread the
    // current value into the local-resolver context so absolute `file:` /
    // `link:` specs round-trip the right shape under the
    // `--config.preserve-absolute-paths` setting. Pacquet doesn't expose
    // `preserveAbsolutePaths` yet, so the context defaults to `false`.
    let local_ctx = LocalResolverContext { preserve_absolute_paths: false };
    let local_scheme_resolver = LocalSchemeResolver::new(local_ctx);
    let local_path_resolver = LocalPathResolver::new(local_ctx);
    let mut node_resolver =
        NodeResolver::new_with_auth(Arc::clone(http_client_arc), Arc::clone(auth_headers));
    node_resolver.node_download_mirrors.clone_from(&config.node_download_mirrors);
    node_resolver.offline = config.offline;
    node_resolver.cache_dir = Some(config.cache_dir.clone());
    let deno_resolver = DenoResolver::new(Arc::clone(http_client_arc), Arc::clone(&npm_resolver));
    let bun_resolver = BunResolver::new(Arc::clone(http_client_arc), Arc::clone(&npm_resolver));
    let yarn_resolver = YarnResolver::new(Arc::clone(http_client_arc));
    let named_registry_resolver = NamedRegistryResolver {
        registries_by_prefix: registries_by_prefix.clone(),
        registry_names: registries_by_prefix.keys().cloned().collect(),
        http_client: Arc::clone(http_client_arc),
        auth_headers: Arc::clone(auth_headers),
        meta_cache: Arc::clone(meta_cache),
        fetch_locker: Arc::clone(&fetch_locker),
        picked_manifest_cache: Arc::clone(&picked_manifest_cache),
        cache_dir: Some(config.cache_dir.clone()),
        offline: config.offline,
        prefer_offline: config.prefer_offline,
        ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
        // Same rationale as `NpmResolver.full_metadata` above.
        full_metadata,
        needs_full_metadata_for: Some(Arc::clone(&needs_full_metadata_for)),
        filter_metadata: config.requires_filtered_full_metadata(),
        retry_opts: crate::retry_config::retry_opts_from_config(config),
    };

    let pnpmfile_hook = match pnpmfile_hook_override {
        Some(hook) => Some(hook),
        None if config.ignore_pnpmfile => None,
        None => pnpm_hooks::finder::load_pnpmfiles(lockfile_dir, crate::pnpmfile_selection(config))
            .map_err(InstallWithFreshLockfileError::MissingPnpmfile)?,
    };
    let custom_resolvers: Vec<Arc<dyn pnpm_hooks::CustomResolver>> =
        if let Some(ref hook) = pnpmfile_hook {
            hook.get_custom_resolvers().await.map_err(|err| {
                tracing::error!(
                    target: "pacquet::install",
                    "Failed to get custom resolvers from pnpmfile: {err}",
                );
                InstallWithFreshLockfileError::CustomResolverHook(err)
            })?
        } else {
            vec![]
        };
    // Loaded alongside the custom resolvers (same worker, same fatality
    // rule) and consumed by `CreateVirtualStore` — a custom resolver
    // typically writes the custom-typed resolutions its sibling fetcher
    // materializes.
    let custom_fetcher_session = if let Some(ref hook) = pnpmfile_hook {
        let fetchers = hook.get_custom_fetchers().await.map_err(|err| {
            tracing::error!(
                target: "pacquet::install",
                "Failed to get custom fetchers from pnpmfile: {err}",
            );
            InstallWithFreshLockfileError::CustomFetcherHook(err)
        })?;
        (!fetchers.is_empty())
            .then(|| Arc::new(pnpm_deps_restorer::CustomFetcherSession::new(fetchers)))
    } else {
        None
    };

    let mut chain: Vec<Box<dyn Resolver>> = Vec::with_capacity(custom_resolvers.len() + 9);
    chain.extend(
        custom_resolvers
            .iter()
            .filter(|custom| custom.has_can_resolve() && custom.has_resolve())
            .map(|custom| {
                Box::new(pnpm_hooks::custom_resolver_adapter::CustomResolverAdapter::new(
                    Arc::clone(custom),
                )) as Box<dyn Resolver>
            }),
    );
    chain.extend([
        Box::new(Arc::clone(&npm_resolver)) as Box<dyn Resolver>,
        Box::new(git_resolver),
        Box::new(tarball_resolver),
        Box::new(local_scheme_resolver),
        Box::new(node_resolver),
        Box::new(deno_resolver),
        Box::new(bun_resolver),
        Box::new(yarn_resolver),
        Box::new(named_registry_resolver),
        Box::new(local_path_resolver),
    ]);

    // The install pass later calls `DownloadTarballToStore::run_with_mem_cache`
    // for the same URLs and either picks up `CacheValue::Available`
    // immediately or briefly blocks on the per-URL `Notify`. See
    // `prefetching_resolver.rs` for the full design rationale.
    let resolver: Box<dyn Resolver> = Box::new(PrefetchingResolver::<Reporter>::new(
        Box::new(DefaultResolver::new(chain)),
        PrefetchContext {
            http_client: http_client_arc,
            mem_cache: tarball_mem_cache,
            store_index,
            store_index_writer: Some(store_index_writer),
            verified_files_cache,
            config,
            requester,
            supported_architectures,
            progress_reported,
            prefetch_downloads: prefetch_downloads && custom_fetcher_session.is_none(),
            custom_fetcher_session: custom_fetcher_session.as_ref(),
        },
    ));

    // Wrapped last so the observer sees each resolve as the prefetching
    // wrapper leaves it, integrity included. A no-op for every local
    // install (`resolution_observer` is `None`).
    let resolver: Box<dyn Resolver> = match resolution_observer {
        Some(observer) => Box::new(crate::ObservingResolver::new(resolver, observer)),
        None => resolver,
    };

    Ok(ResolverChain {
        resolver,
        npm_resolver,
        fetch_locker,
        picked_manifest_cache,
        custom_resolvers,
        custom_fetcher_session,
        pnpmfile_hook,
    })
}
