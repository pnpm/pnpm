//! Resolver wrapper that pipelines tarball downloads with resolution.
//!
//! Package resolution returns a response whose tarball download is
//! already running by the time the resolver returns. Resolution of
//! children continues in parallel with that download; the install pass
//! later awaits each download (which is usually already finished by
//! then).
//!
//! Pacquet's deps-resolver crate stays pure: it walks the manifest tree
//! and returns a [`ResolveResult`] without doing any tarball I/O. To
//! match pnpm's pipelined shape, the install orchestrator wraps the
//! resolver chain with [`PrefetchingResolver`]. After the inner
//! resolver claims a wanted dep, the wrapper inspects the result and,
//! for tarball-shaped resolutions, [`tokio::spawn`]s a
//! [`IngestTarballToStore`] in the background.
//!
//! The download lands its result in the shared [`MemCache`]. Later, when
//! [`crate::InstallPackageFromRegistry`] calls
//! [`IngestTarballToStore::run_with_mem_cache`] for the same archive, the
//! `MemCache` either returns `CacheValue::Available` immediately (the
//! prefetch is already done) or briefly blocks on the `Notify` (the
//! prefetch is still in flight). Errors are surfaced to the install
//! path as `TarballError::SiblingFetchFailed`.
//!
//! That prefetch is speculative, and a run may switch it off
//! ([`PrefetchPolicy::downloads`]). Reading an archive whose resolution
//! describes it only partly is not speculative — the lockfile records
//! the hash the bytes yield, and the dependency walk reads the
//! package's children out of the manifest inside. Such a read publishes
//! its extraction too, so the install pass never downloads the archive a
//! second time. When a prefetch for the same archive is already in
//! flight, the read parks on that extraction and takes the bundled
//! manifest from the settled cache slot. A custom fetcher must decline
//! an unpinned tarball before that read can download it.

use crate::install_package_from_registry::{
    extract_tarball, manifest_file_count, manifest_unpacked_size,
};
use dashmap::{DashMap, DashSet};
use pnpm_config::Config;
use pnpm_deps_restorer::{CustomFetcherSession, ResolvedTarballMetadata};
use pnpm_lockfile::{LockfileResolution, is_git_hosted_tarball_url};
use pnpm_network::ThrottledClient;
use pnpm_package_is_installable::{
    SupportedArchitectures, WantedPlatformRef, platform_is_supported,
};
use pnpm_reporter::Reporter;
use pnpm_resolving_resolver_base::{
    LatestQuery, ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver,
    WantedDependency,
};
use pnpm_store_dir::{SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreIndexWriter};
use pnpm_tarball::{
    ArchiveStoreProjection, IngestTarballToStore, MemCache, SharedReportedProgressKeys,
    package_mem_cache_key,
};
use std::{marker::PhantomData, sync::Arc};
use tokio::sync::OnceCell;

/// Borrowed-data bag handed to [`PrefetchingResolver::new`]. Everything
/// the wrapper needs to drive a background tarball download:
/// network/store handles, the shared mem cache, the
/// `verifiedFilesCache`, retry/offline knobs, and the install's
/// `requester` prefix for reporter events. The wrapper clones each
/// field into the form a `tokio::spawn`ed task can capture (`Arc` for
/// shared refs, `&'static` passes through, primitive copies).
#[derive(Clone, Copy)]
pub struct PrefetchContext<'a> {
    pub http_client: &'a Arc<ThrottledClient>,
    pub mem_cache: &'a Arc<MemCache>,
    pub config: &'static Config,
    pub requester: &'a str,
    pub supported_architectures: Option<&'a SupportedArchitectures>,
    /// Install-scoped set the background download records when it emits
    /// a package-status progress event. The later warm/cold install pass
    /// consults the set so prefetch progress is visible immediately
    /// without being counted again.
    pub progress_reported: &'a SharedReportedProgressKeys,
    pub store: PrefetchStoreRefs<'a>,
    pub policy: PrefetchPolicy<'a>,
}

#[derive(Clone, Copy)]
pub struct PrefetchStoreRefs<'a> {
    pub index: Option<&'a SharedReadonlyStoreIndex>,
    pub index_writer: Option<&'a Arc<StoreIndexWriter>>,
    pub verified_files_cache: &'a SharedVerifiedFilesCache,
}

#[derive(Clone, Copy)]
pub struct PrefetchPolicy<'a> {
    /// Whether native speculative tarball downloads are allowed.
    pub downloads: bool,
    /// Consulted before downloading a tarball to learn its missing hash.
    pub custom_session: Option<&'a Arc<CustomFetcherSession>>,
}

/// Owned, `'static`-friendly clones of [`PrefetchContext`] stored on
/// the wrapper. Every field is either an `Arc` clone or a `Copy`
/// scalar so each [`tokio::spawn`]ed download task can capture an
/// independent set without leaking lifetimes back into the resolver's
/// type.
struct OwnedFetchCtx {
    mem_cache: Arc<MemCache>,
    requester: Arc<str>,
    progress_reported: SharedReportedProgressKeys,
    store: pnpm_tarball::ArchiveStoreContext<'static>,
    fetching: crate::tarball_prefetch::PrefetchHttpClient,
    platform: PrefetchPlatform,
    policy: PrefetchRunPolicy,
}

struct PrefetchPlatform {
    supported_architectures: Option<SupportedArchitectures>,
    os: &'static str,
    cpu: &'static str,
    libc: &'static str,
}

struct PrefetchRunPolicy {
    downloads: bool,
    custom_session: Option<Arc<CustomFetcherSession>>,
    ignore_scripts: bool,
}

/// Wraps an inner [`Resolver`] and, after each successful resolve that
/// produces a tarball-shaped result, fires the tarball download into
/// the shared [`MemCache`] via [`tokio::spawn`]. The resolver returns
/// to the deps-resolver immediately; the download runs concurrently
/// with the rest of the tree walk.
///
/// Generic over `Reporter: self::Reporter` so
/// [`IngestTarballToStore`]'s `pnpm:progress` emits route through
/// the same reporter the install pass uses. The wrapper itself
/// doesn't hold a `Reporter` value (`Reporter` is a static trait);
/// `PhantomData` carries the type through.
pub struct PrefetchingResolver<Reporter: self::Reporter> {
    inner: Box<dyn Resolver>,
    /// Cache identities that already had a download claimed, used as an
    /// atomic check-and-claim gate so concurrent resolves for the same
    /// archive can't both pass a non-atomic `MemCache` lookup and race
    /// two spawns into the cache. [`DashSet::insert`] returns `true`
    /// only for the caller that wins the slot; later callers observe
    /// `false` and skip the spawn entirely. The [`MemCache`]-side dedup
    /// still backstops correctness (the loser would have parked on
    /// `Notify` instead of doing work), but without this gate the bench
    /// saw ~3-5k redundant spawns per install on the alotta-files
    /// fixture (one per dependent edge).
    ///
    /// Entries are [`package_mem_cache_key`]s, the same identity the
    /// download publishes under, so a resolve-time read that already
    /// warmed the cache claims exactly the slot the prefetch would have
    /// filled.
    spawned_downloads: DashSet<String>,
    /// Shares completed archive reads, including custom resolution rewrites.
    tarball_metadata_cache: DashMap<String, Arc<OnceCell<ResolvedTarballMetadata>>>,
    /// Shared with every prefetch task the resolver spawns.
    ctx: Arc<OwnedFetchCtx>,
    _phantom: PhantomData<fn() -> Reporter>,
}

impl<Reporter: self::Reporter + 'static> PrefetchingResolver<Reporter> {
    /// Build a wrapper from the install orchestrator's borrowed
    /// references. Clones the necessary `Arc`s up front so the
    /// per-`resolve` spawn has all the data it needs without
    /// re-borrowing the install scope.
    #[must_use]
    pub fn new(inner: Box<dyn Resolver>, prefetch_ctx: PrefetchContext<'_>) -> Self {
        let ctx = owned_fetch_context(&prefetch_ctx);
        PrefetchingResolver {
            inner,
            spawned_downloads: DashSet::new(),
            tarball_metadata_cache: DashMap::new(),
            ctx: Arc::new(ctx),
            _phantom: PhantomData,
        }
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
    /// The spawned task's result is dropped: the `MemCache` slot
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
        // `IngestTarballToStore` at all.
        let Ok((package_url, integrity)) = extract_tarball(&result.resolution) else {
            return;
        };
        let revision_addressed = matches!(
            &result.resolution,
            LockfileResolution::Tarball(tarball) if tarball.revision.is_some(),
        );
        // The npm picker's `dist.tarball` is the canonical URL the
        // install path will look up in `MemCache`. Tarball-resolver
        // and git-resolver paths can leave `name_ver` unset (they
        // learn the name from the manifest only after the fetch); in
        // those cases the install path's `InstallPackageFromRegistry`
        // also fails, so skipping here matches the install-side
        // behaviour without adding a divergence.
        let Some(name_ver) = result.package.name_ver.as_ref() else { return };

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
        // resolvers.
        if !self.claim_download(package_url, &integrity, revision_addressed) {
            return;
        }

        let package_id = format!("{}@{}", name_ver.name, name_ver.suffix);
        let package_url = package_url.to_string();
        let package_unpacked_size = manifest_unpacked_size(result.package.manifest.as_deref());
        let package_file_count = manifest_file_count(result.package.manifest.as_deref());
        let ctx = Arc::clone(&self.ctx);

        tokio::spawn(async move {
            // Report prefetch progress through the install reporter as
            // soon as the fetch/cache-hit outcome is known. pnpm can
            // likewise start package fetching before the dependency
            // resolver emits `resolved`; the default reporter counts
            // progress events independently. The shared
            // `progress_reported` set lets the later warm/cold install
            // pass skip a duplicate package-status event for this cache
            // key while still emitting `resolved`.
            //
            // Result is intentionally discarded — the `MemCache`
            // carries success / failure state to the install path.
            let download = ctx.tarball_download(
                &package_url,
                &package_id,
                Some(&integrity),
                package_unpacked_size,
                package_file_count,
            );
            let _ = if revision_addressed {
                download.run_revision_addressed_with_mem_cache::<Reporter>(&ctx.mem_cache).await
            } else {
                download.run_with_mem_cache::<Reporter>(&ctx.mem_cache).await
            };
        });
    }

    /// Take the single download of this archive, returning `false` when
    /// another resolve or a resolve-time read already holds it.
    fn claim_download(
        &self,
        package_url: &str,
        integrity: &ssri::Integrity,
        revision_addressed: bool,
    ) -> bool {
        self.spawned_downloads.insert(package_mem_cache_key(
            package_url,
            Some(integrity),
            revision_addressed,
        ))
    }

    fn should_skip_prefetch(
        &self,
        wanted_dependency: &WantedDependency,
        result: &ResolveResult,
    ) -> bool {
        if wanted_dependency.optional != Some(true) || !is_remote_tarball(&result.resolution) {
            return false;
        }
        let manifest = crate::platform_manifest_from_resolve_result(
            result,
            wanted_dependency.alias.as_deref(),
        );
        let manifest = crate::manifest_with_inferred_platform(&manifest);
        !platform_is_supported(
            WantedPlatformRef {
                os: manifest.os.as_deref(),
                cpu: manifest.cpu.as_deref(),
                libc: manifest.libc.as_deref(),
            },
            self.ctx.platform.supported_architectures.as_ref(),
            self.ctx.platform.os,
            self.ctx.platform.cpu,
            self.ctx.platform.libc,
        )
    }
}

impl OwnedFetchCtx {
    fn tarball_download<'a>(
        &'a self,
        package_url: &'a str,
        package_id: &'a str,
        package_integrity: Option<&'a ssri::Integrity>,
        package_unpacked_size: Option<usize>,
        package_file_count: Option<usize>,
    ) -> IngestTarballToStore<'a> {
        IngestTarballToStore {
            fetching: self.fetching.options(),
            package: pnpm_tarball::TarballPackage {
                integrity: package_integrity,
                unpacked_size: package_unpacked_size,
                file_count: package_file_count,
                url: package_url,
                id: package_id,
            },
            store: self.store.clone(),

            requester: &self.requester,

            ignore_file_pattern: None,

            progress_reported: Some(Arc::clone(&self.progress_reported)),
            store_projection: ArchiveStoreProjection::Package { append_manifest: None },
        }
    }
}

fn owned_fetch_context(prefetch_ctx: &PrefetchContext<'_>) -> OwnedFetchCtx {
    let PrefetchContext {
        http_client,
        mem_cache,
        config,
        requester,
        supported_architectures,
        progress_reported,
        store,
        policy:
            crate::PrefetchPolicy {
                downloads: prefetch_downloads,
                custom_session: custom_fetcher_session,
            },
    } = prefetch_ctx;
    OwnedFetchCtx {
        mem_cache: Arc::clone(mem_cache),
        requester: Arc::<str>::from(*requester),
        progress_reported: SharedReportedProgressKeys::clone(progress_reported),
        store: store.archive_context(config),
        fetching: crate::tarball_prefetch::PrefetchHttpClient::new(config, http_client, None),
        platform: PrefetchPlatform {
            supported_architectures: supported_architectures.cloned(),
            os: pnpm_graph_hasher::host_platform(),
            cpu: pnpm_graph_hasher::host_arch(),
            libc: pnpm_graph_hasher::host_libc(),
        },
        policy: PrefetchRunPolicy {
            downloads: *prefetch_downloads,
            custom_session: custom_fetcher_session.cloned(),
            ignore_scripts: config.ignore_scripts,
        },
    }
}

impl<Reporter: self::Reporter + 'static> Resolver for PrefetchingResolver<Reporter> {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        Box::pin(async move {
            let mut result = self.inner.resolve(wanted_dependency, opts).await?;
            if let Some(result_mut) = result.as_mut() {
                self.populate_missing_tarball_metadata(result_mut, &opts.project.lockfile_dir)
                    .await?;
                if self.ctx.policy.downloads
                    && !self.should_skip_prefetch(wanted_dependency, result_mut)
                {
                    self.maybe_kickoff_download(result_mut);
                }
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

fn is_remote_tarball(resolution: &LockfileResolution) -> bool {
    let LockfileResolution::Tarball(tarball) = resolution else { return false };
    !tarball.tarball.starts_with("file:") && !is_git_hosted_tarball_url(&tarball.tarball)
}

mod archive_read;

#[cfg(test)]
mod tests;

impl PrefetchStoreRefs<'_> {
    fn archive_context(
        &self,
        config: &'static Config,
    ) -> pnpm_tarball::ArchiveStoreContext<'static> {
        pnpm_tarball::ArchiveStoreContext {
            dir: &config.store_dir,
            index: self.index.cloned(),
            index_writer: self.index_writer.cloned(),
            verified_files_cache: SharedVerifiedFilesCache::clone(self.verified_files_cache),
            verify_integrity: config.verify_store_integrity,
            strict_pkg_content_check: config.strict_store_pkg_content_check,
            prefetched_cas_paths: None,
        }
    }
}
