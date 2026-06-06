//! Resolver wrapper that pipelines tarball downloads with resolution.
//!
//! Upstream pnpm's [`packageRequester.requestPackage`](https://github.com/pnpm/pnpm/blob/fecaee0b35/installing/package-requester/src/packageRequester.ts#L266)
//! returns a `pkgResponse` whose `fetching` field is a Promise that is
//! already running by the time the resolver returns. Resolution of
//! children continues in parallel with that download; the install pass
//! later `await`s each `fetching` promise (which is usually already
//! resolved by then).
//!
//! Pacquet's deps-resolver crate stays pure: it walks the manifest tree
//! and returns a [`ResolveResult`] without doing any tarball I/O. To
//! match pnpm's pipelined shape, the install orchestrator wraps the
//! resolver chain with [`PrefetchingResolver`]. After the inner
//! resolver claims a wanted dep, the wrapper inspects the result and,
//! for tarball-shaped resolutions, [`tokio::spawn`]s a
//! [`DownloadTarballToStore`] in the background.
//!
//! The download lands its result in the shared [`MemCache`]. Later, when
//! [`crate::InstallPackageFromRegistry`] calls
//! [`DownloadTarballToStore::run_with_mem_cache`] for the same URL, the
//! `MemCache` either returns `CacheValue::Available` immediately (the
//! prefetch is already done) or briefly blocks on the `Notify` (the
//! prefetch is still in flight). Errors are surfaced to the install
//! path as `TarballError::SiblingFetchFailed`.

use crate::{
    install_package_from_registry::{extract_tarball, manifest_unpacked_size},
    retry_config::retry_opts_from_config,
};
use dashmap::DashSet;
use pacquet_config::Config;
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_reporter::{Reporter, SilentReporter};
use pacquet_resolving_resolver_base::{
    LatestQuery, ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver,
    WantedDependency,
};
use pacquet_store_dir::{
    SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreDir, StoreIndexWriter,
};
use pacquet_tarball::{DownloadTarballToStore, MemCache, RetryOpts, SharedNetworkFetchedKeys};
use std::{marker::PhantomData, sync::Arc};

/// Borrowed-data bag handed to [`PrefetchingResolver::new`]. Everything
/// the wrapper needs to drive a background tarball download:
/// network/store handles, the shared mem cache, the
/// `verifiedFilesCache`, retry/offline knobs, and the install's
/// `requester` prefix for reporter events. The wrapper clones each
/// field into the form a `tokio::spawn`ed task can capture (`Arc` for
/// shared refs, `&'static` passes through, primitive copies).
pub struct PrefetchContext<'a> {
    pub http_client: &'a Arc<ThrottledClient>,
    pub mem_cache: &'a Arc<MemCache>,
    pub store_index: Option<&'a SharedReadonlyStoreIndex>,
    pub store_index_writer: Option<&'a Arc<StoreIndexWriter>>,
    pub verified_files_cache: &'a SharedVerifiedFilesCache,
    pub config: &'static Config,
    pub requester: &'a str,
    /// Install-scoped set the background download records its cache key
    /// into when its bytes come over the network. Lets the install
    /// pass's warm-batch reporter label a package downloaded this
    /// install as `fetched` rather than `found_in_store`, since the
    /// prefetch's own emits route through a [`SilentReporter`]. See
    /// [`SharedNetworkFetchedKeys`].
    pub network_fetched: &'a SharedNetworkFetchedKeys,
}

/// Owned, `'static`-friendly clones of [`PrefetchContext`] stored on
/// the wrapper. Every field is either an `Arc` clone or a `Copy`
/// scalar so each [`tokio::spawn`]ed download task can capture an
/// independent set without leaking lifetimes back into the resolver's
/// type.
struct OwnedFetchCtx {
    http_client: Arc<ThrottledClient>,
    mem_cache: Arc<MemCache>,
    store_dir: &'static StoreDir,
    store_index: Option<SharedReadonlyStoreIndex>,
    store_index_writer: Option<Arc<StoreIndexWriter>>,
    verified_files_cache: SharedVerifiedFilesCache,
    auth_headers: Arc<AuthHeaders>,
    retry_opts: RetryOpts,
    requester: Arc<str>,
    offline: bool,
    verify_store_integrity: bool,
    network_fetched: SharedNetworkFetchedKeys,
    /// Set of URLs that already had a prefetch task spawned, used as
    /// an atomic check-and-claim gate so concurrent resolves for the
    /// same tarball can't both pass a non-atomic `MemCache` lookup and
    /// race two spawns into the cache. [`DashSet::insert`] returns
    /// `true` only for the caller that wins the slot; later callers
    /// observe `false` and skip the spawn entirely. The
    /// [`MemCache`]-side dedup still backstops correctness (the loser
    /// would have parked on `Notify` instead of doing work), but
    /// without this gate the bench saw ~3-5k redundant spawns per
    /// install on the alotta-files fixture (one per dependent edge).
    spawned_urls: Arc<DashSet<String>>,
}

/// Wraps an inner [`Resolver`] and, after each successful resolve that
/// produces a tarball-shaped result, fires the tarball download into
/// the shared [`MemCache`] via [`tokio::spawn`]. The resolver returns
/// to the deps-resolver immediately; the download runs concurrently
/// with the rest of the tree walk.
///
/// Generic over `Reporter: self::Reporter` so
/// [`DownloadTarballToStore`]'s `pnpm:progress` emits route through
/// the same reporter the install pass uses. The wrapper itself
/// doesn't hold a `Reporter` value (`Reporter` is a static trait);
/// `PhantomData` carries the type through.
pub struct PrefetchingResolver<Reporter: self::Reporter> {
    inner: Box<dyn Resolver>,
    ctx: OwnedFetchCtx,
    _phantom: PhantomData<fn() -> Reporter>,
}

impl<Reporter: self::Reporter + 'static> PrefetchingResolver<Reporter> {
    /// Build a wrapper from the install orchestrator's borrowed
    /// references. Clones the necessary `Arc`s up front so the
    /// per-`resolve` spawn has all the data it needs without
    /// re-borrowing the install scope.
    pub fn new(inner: Box<dyn Resolver>, prefetch_ctx: PrefetchContext<'_>) -> Self {
        let PrefetchContext {
            http_client,
            mem_cache,
            store_index,
            store_index_writer,
            verified_files_cache,
            config,
            requester,
            network_fetched,
        } = prefetch_ctx;
        let ctx = OwnedFetchCtx {
            http_client: Arc::clone(http_client),
            mem_cache: Arc::clone(mem_cache),
            store_dir: &config.store_dir,
            store_index: store_index.cloned(),
            store_index_writer: store_index_writer.cloned(),
            verified_files_cache: SharedVerifiedFilesCache::clone(verified_files_cache),
            auth_headers: Arc::clone(&config.auth_headers),
            retry_opts: retry_opts_from_config(config),
            requester: Arc::<str>::from(requester),
            offline: config.offline,
            verify_store_integrity: config.verify_store_integrity,
            network_fetched: SharedNetworkFetchedKeys::clone(network_fetched),
            spawned_urls: Arc::new(DashSet::new()),
        };
        PrefetchingResolver { inner, ctx, _phantom: PhantomData }
    }

    /// Inspect a fresh `ResolveResult` and, if it carries a tarball
    /// URL + integrity, kick off the download as a detached
    /// [`tokio::spawn`] task.
    ///
    /// Non-tarball resolutions (git, directory, registry-shape,
    /// binary, variations) and resolutions missing a structured
    /// `name@version` fall through to a no-op — the install path's
    /// per-protocol code path handles them.
    ///
    /// The spawned task's result is dropped: the per-URL `MemCache`
    /// stores `CacheValue::Available` (on success) or
    /// `CacheValue::Failed` (on error) and any later
    /// `run_with_mem_cache` call observes the right value. Surfacing
    /// the error from inside the resolver would force the resolve
    /// pass to abort before the rest of the tree walk completes,
    /// which is the opposite of what we want for a prefetch.
    fn maybe_kickoff_download(&self, result: &ResolveResult) {
        // Only spawn for tarball-shaped resolutions with both URL and
        // integrity. Mirrors the gate in
        // `install_package_from_registry::extract_tarball`; other
        // resolution shapes are not fetched through
        // `DownloadTarballToStore` at all.
        let Ok((package_url, integrity)) = extract_tarball(&result.resolution) else {
            return;
        };
        // The npm picker's `dist.tarball` is the canonical URL the
        // install path will look up in `MemCache`. Tarball-resolver
        // and git-resolver paths can leave `name_ver` unset (they
        // learn the name from the manifest only after the fetch); in
        // those cases the install path's `InstallPackageFromRegistry`
        // also fails, so skipping here matches the install-side
        // behaviour without adding a divergence.
        let Some(name_ver) = result.name_ver.as_ref() else { return };

        // Per-occurrence atomic dedup: the deps resolver calls
        // `resolve()` once per (parent, child) edge. Concurrent calls
        // for the same tarball must collapse to a single spawn —
        // `MemCache` would dedup correctness-wise via its `InProgress`
        // slot, but two losers would still both `tokio::spawn` and
        // both `await` the `Notify`, contributing only scheduler /
        // lock churn. Use [`DashSet::insert`] as a check-and-claim
        // primitive: only the caller that flips the membership from
        // absent → present spawns; everyone else returns. The
        // `MemCache` is *not* atomic for this purpose — its
        // `contains_key` + `insert` is a TOCTOU pair under racing
        // resolvers (P3 from the code review).
        if !self.ctx.spawned_urls.insert(package_url.to_string()) {
            return;
        }

        let package_id = format!("{}@{}", name_ver.name, name_ver.suffix);
        let package_url = package_url.to_string();
        let package_unpacked_size = manifest_unpacked_size(result.manifest.as_deref());

        let http_client = Arc::clone(&self.ctx.http_client);
        let mem_cache = Arc::clone(&self.ctx.mem_cache);
        let store_dir = self.ctx.store_dir;
        let store_index = self.ctx.store_index.clone();
        let store_index_writer = self.ctx.store_index_writer.clone();
        let verified_files_cache = SharedVerifiedFilesCache::clone(&self.ctx.verified_files_cache);
        let auth_headers = Arc::clone(&self.ctx.auth_headers);
        let retry_opts = self.ctx.retry_opts;
        let requester = Arc::clone(&self.ctx.requester);
        let offline = self.ctx.offline;
        let verify_store_integrity = self.ctx.verify_store_integrity;
        let network_fetched = SharedNetworkFetchedKeys::clone(&self.ctx.network_fetched);

        tokio::spawn(async move {
            // Route the prefetch download through `SilentReporter`, not
            // the install's reporter. `DownloadTarballToStore` emits
            // `pnpm:progress fetched` / `found_in_store` internally;
            // emitting those from this background task would race ahead
            // of the install pass's own `resolved` event for the same
            // package, breaking the `resolved → fetched/found_in_store
            // → imported` order pnpm's reporter consumers (notably
            // `@pnpm/cli.default-reporter`) depend on. Mirrors pnpm's
            // `packageRequester.requestPackage` shape: the `fetching`
            // promise runs early but the per-event emit is driven by
            // the install pass.
            //
            // On the fresh phased path the install pass reports each
            // package through `CreateVirtualStore`'s warm batch (which
            // reads the freshly-populated store index), not through a
            // second `run_with_mem_cache` call. That warm batch can't
            // tell a tarball this task just downloaded from one that
            // was already in the store — both look like store hits by
            // the time it runs. So pass `network_fetched`: this task
            // records the cache key when its fetch hits the network,
            // and the warm batch consults the set to report `fetched`
            // rather than `found_in_store`. See [`SharedNetworkFetchedKeys`].
            //
            // Result is intentionally discarded — the `MemCache`
            // carries success / failure state to the install path.
            let _ = DownloadTarballToStore {
                http_client: &http_client,
                store_dir,
                store_index,
                store_index_writer,
                verify_store_integrity,
                verified_files_cache,
                package_integrity: &integrity,
                package_unpacked_size,
                package_url: &package_url,
                package_id: &package_id,
                requester: &requester,
                prefetched_cas_paths: None,
                retry_opts,
                auth_headers: &auth_headers,
                ignore_file_pattern: None,
                offline,
                network_fetched: Some(network_fetched),
            }
            .run_with_mem_cache::<SilentReporter>(&mem_cache)
            .await;
        });
    }
}

impl<Reporter: self::Reporter + 'static> Resolver for PrefetchingResolver<Reporter> {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        Box::pin(async move {
            let result = self.inner.resolve(wanted_dependency, opts).await?;
            if let Some(result_ref) = result.as_ref() {
                self.maybe_kickoff_download(result_ref);
            }
            Ok(result)
        })
    }

    fn resolve_latest<'a>(
        &'a self,
        query: &'a LatestQuery,
        opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        self.inner.resolve_latest(query, opts)
    }
}
