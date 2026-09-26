//! The per-pick route state ([`PickState`]) and the layer methods every
//! candidate path of a pick shares.

use std::{path::PathBuf, sync::Arc};

use pnpm_network::MetadataCacheScope;
use pnpm_registry::Package;

use crate::{
    FetchFullMetadataCachedOptions, FetchMetadataError, fetch_full_metadata_cached,
    mirror::{
        ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR, FULL_META_DIR, get_pkg_mirror_path,
        load_meta_async, scoped_meta_dir,
    },
    pick_package_from_meta::RegistryPackageSpec,
    registry_url::to_registry_url,
};

use super::{
    PackageMetaCache, PickPackageContext, PickPackageError, PickPackageOptions, PickPackageResult,
    PickerOpts, UpgradeOutcome, handle_cache_hit, maybe_upgrade_abbreviated_meta_for_release_age,
    metadata_cache_key, persist_upgraded_to_mirror, pick_from_meta,
};

/// The route classification and cache keys every layer of one pick shares.
pub(super) struct PickState<'a> {
    pub(super) picker_opts: PickerOpts<'a>,
    pub(super) scope: MetadataCacheScope,
    pub(super) full_metadata: bool,
    pub(super) use_filtered_full_metadata: bool,
    pub(super) pkg_mirror: Option<PathBuf>,
    /// The unscoped metadata directory this pick's shape selects, before a
    /// `Private` route relocates it.
    pub(super) base_meta_dir: &'static str,
    pub(super) cache_key: String,
    /// `updateChecksums` must reach the conditional registry request, so it
    /// can't be served from the in-memory cache — which may hold a
    /// disk-promoted entry rather than a fresh network fetch (see the
    /// `update_checksums` doc).
    pub(super) use_mem_cache: bool,
}

impl<'a> PickState<'a> {
    pub(super) fn new<Cache: PackageMetaCache>(
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'a>,
    ) -> Self {
        // Every layer below — the in-memory cache, the offline / version-spec
        // / publishedBy disk fast paths, and the network fetch — answers this
        // pick for the same `(registry, package)` route. The fast paths return
        // straight from cache without ever reaching the auth-selection point,
        // so a server route hook would never see this package and its private
        // footprint would under-report the data the resolve depended on.
        // Record the route up front, classified exactly as the network fetch
        // would classify it, so the footprint is complete regardless of which
        // layer serves the metadata. A no-op for the CLI (no hook installed);
        // idempotent on the hook, so the network path re-recording it is fine.
        let url = to_registry_url(opts.registry, &spec.name);
        ctx.metadata.http.auth_headers.record_route(&url, Some(&spec.name));

        // Classify the metadata cache scope once. Every layer below — the
        // in-memory cache, the disk fast paths, and the network fetch — must
        // agree on the mirror namespace and cache keys this route resolves to,
        // or a private packument could leak into (or read from) the global
        // mirror. `Public` for the CLI, leaving the global mirror unchanged.
        let scope = ctx.metadata.http.auth_headers.metadata_scope(&url, Some(&spec.name));

        let (full_metadata, use_filtered_full_metadata, base_meta_dir) =
            Self::metadata_shape(ctx, opts);

        // A `Private` route relocates the mirror under its descriptor
        // namespace so it can never be read by a caller who doesn't reproduce
        // the same descriptor; a `Public` route keeps the global mirror.
        let pkg_mirror = ctx.metadata.cache_dir.and_then(|dir| {
            let meta_dir = scoped_meta_dir(&scope, base_meta_dir);
            get_pkg_mirror_path(dir, &meta_dir, opts.registry, &spec.name).ok()
        });

        PickState {
            picker_opts: PickerOpts {
                preferred_version_selectors: opts.preferred_version_selectors,
                published_by: opts.policy.published_by,
                published_by_exclude: opts.policy.published_by_exclude,
                pick_lowest_version: opts.pick_lowest_version,
                include_latest_tag: opts.include_latest_tag,
                ignore_missing_time_field: ctx.cache_policy.ignore_missing_time_field,
            },
            cache_key: metadata_cache_key(
                &scope,
                opts.registry,
                &spec.name,
                full_metadata,
                use_filtered_full_metadata,
            ),
            scope,
            full_metadata,
            use_filtered_full_metadata,
            pkg_mirror,
            base_meta_dir,
            use_mem_cache: !opts.request.update_checksums,
        }
    }

    /// Whether this pick wants full metadata, whether that full metadata is
    /// filtered, and the mirror directory that selection reads/writes. The
    /// per-registry answer is authoritative when the caller can give one: it
    /// already folds in the reasons that hold for every registry, so a
    /// registry that carries `time` is free to stay on abbreviated metadata
    /// while the others do not.
    fn metadata_shape<Cache: PackageMetaCache>(
        ctx: &PickPackageContext<'_, Cache>,
        opts: &PickPackageOptions<'_>,
    ) -> (bool, bool, &'static str) {
        let policy_wants_full_metadata =
            ctx.needs_full_metadata_for.map_or(ctx.full_metadata, |needs_full_metadata| {
                needs_full_metadata(opts.registry)
            });
        let full_metadata = opts.request.optional || policy_wants_full_metadata;
        let use_filtered_full_metadata = full_metadata && ctx.filter_metadata;
        let base_meta_dir = if full_metadata {
            if use_filtered_full_metadata { FULL_FILTERED_META_DIR } else { FULL_META_DIR }
        } else {
            ABBREVIATED_META_DIR
        };
        (full_metadata, use_filtered_full_metadata, base_meta_dir)
    }

    pub(super) async fn cached_pick<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
    ) -> Result<Option<PickPackageResult>, PickPackageError> {
        if !self.use_mem_cache {
            return Ok(None);
        }
        let Some(cached) = ctx.metadata.meta_cache.get(&self.cache_key) else {
            return Ok(None);
        };
        handle_cache_hit(
            ctx,
            spec,
            opts,
            &self.picker_opts,
            self.full_metadata,
            self.use_filtered_full_metadata,
            &self.cache_key,
            self.pkg_mirror.as_deref(),
            cached,
        )
        .await
    }

    /// Run the release-age upgrade check over a packument, persisting and
    /// caching the upgraded document when one was fetched.
    pub(super) async fn upgraded_meta<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
        meta: Arc<Package>,
    ) -> Result<Arc<Package>, PickPackageError> {
        let upgrade = maybe_upgrade_abbreviated_meta_for_release_age(
            ctx,
            spec,
            opts,
            self.full_metadata,
            &self.cache_key,
            meta,
        )
        .await?;
        let mut meta = upgrade.meta;
        if !upgrade.upgraded {
            // A cache hit re-runs the release-age upgrade check, so serving
            // this meta from memory can't bypass the upgrade.
            self.promote_unverified(ctx, opts, &meta);
            return Ok(meta);
        }
        if !opts.request.dry_run {
            if let Some(reloaded) = self.pkg_mirror
                .as_deref()
                .and_then(|path| {
                    persist_upgraded_to_mirror(
                        path,
                        &meta,
                        self.use_filtered_full_metadata,
                        upgrade.uncacheable,
                    )
                })
            {
                meta = Arc::new(reloaded);
            }
            // The upgrade fetched a registry-validated document; don't
            // downgrade it to an unverified marking.
            ctx.metadata.meta_cache.set(self.cache_key.clone(), Arc::clone(&meta));
        }
        Ok(meta)
    }

    /// The network fetch via the cached fetcher (step 5). The cached
    /// fetcher handles conditional headers + 200 cache write internally; on
    /// a 304 it re-reads the mirror body. On the error path, a fetch failure
    /// with a disk fallback uses it; otherwise the error propagates.
    pub(super) async fn fetch_and_pick<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        spec: &RegistryPackageSpec,
        opts: &PickPackageOptions<'_>,
        disk_meta: Option<Arc<Package>>,
    ) -> Result<PickPackageResult, PickPackageError> {
        let fetch_opts = self.cached_fetch_options(ctx, opts.registry);

        let meta = match fetch_full_metadata_cached(&spec.name, &fetch_opts).await {
            Ok(meta) => Arc::new(meta),
            Err(error) => {
                let Some(disk) = self.disk_fallback(&error, disk_meta).await else {
                    return Err(error.into());
                };
                tracing::debug!(
                    target: "pnpm_resolving_npm_resolver::pick_package",
                    ?error,
                    pkg_name = %spec.name,
                    "metadata fetch failed; falling back to on-disk mirror",
                );
                let (meta, picked) =
                    pick_from_meta(&self.picker_opts, spec, disk, opts.blocked_versions)?;
                return Ok(PickPackageResult { meta, picked_package: picked });
            }
        };

        let upgrade = maybe_upgrade_abbreviated_meta_for_release_age(
            ctx,
            spec,
            opts,
            self.full_metadata,
            &self.cache_key,
            meta,
        )
        .await?;
        let meta = self.persist_release_age_upgrade(ctx, opts, upgrade);

        // Worth flagging: a dry-run is meant to gate the on-disk save, but
        // `fetch_full_metadata_cached` already wrote the response body to
        // the mirror by the time it returned, so `opts.dry_run` only
        // suppresses the in-memory cache write. A future refactor that
        // threads `dry_run` into the fetcher can restore a fully
        // no-disk-side-effect dry-run.
        if !opts.request.dry_run {
            ctx.metadata.meta_cache.set(self.cache_key.clone(), Arc::clone(&meta));
        }
        let (meta, picked) = pick_from_meta(&self.picker_opts, spec, meta, opts.blocked_versions)?;
        Ok(PickPackageResult { meta, picked_package: picked })
    }

    pub(super) fn persist_release_age_upgrade<Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'_, Cache>,
        opts: &PickPackageOptions<'_>,
        upgrade: UpgradeOutcome,
    ) -> Arc<Package> {
        let mut meta = upgrade.meta;
        if upgrade.upgraded {
            if !opts.request.dry_run
                && let Some(reloaded) = self.pkg_mirror
                    .as_deref()
                    .and_then(|path| {
                        persist_upgraded_to_mirror(
                            path,
                            &meta,
                            self.use_filtered_full_metadata,
                            upgrade.uncacheable,
                        )
                    })
            {
                meta = Arc::new(reloaded);
            }
            ctx.metadata.fetch_locker.mark_release_age_upgrade_checked(&self.cache_key, &meta);
        }

        meta
    }

    pub(super) fn cached_fetch_options<'ctx, Cache: PackageMetaCache>(
        &self,
        ctx: &PickPackageContext<'ctx, Cache>,
        registry: &'ctx str,
    ) -> FetchFullMetadataCachedOptions<'ctx> {
        FetchFullMetadataCachedOptions {
            registry,
            cache_dir: ctx.metadata.cache_dir,
            full_metadata: self.full_metadata,
            filter_metadata: self.use_filtered_full_metadata,
            offline: ctx.cache_policy.offline,
            priority: pnpm_network::UNPRIORITIZED,
            http: ctx.metadata.http,
        }
    }

    /// The mirror a failed fetch may fall back to.
    ///
    /// The fetcher already saved a 200 to disk before it returned (when it
    /// returned Ok). If it returned Err, an existing mirror is good enough
    /// to pick from, even if the latest sync failed.
    ///
    /// A private route must fail closed on a `401`/`403`/private-`404`: a
    /// revoked credential or a hidden private package must not keep serving
    /// the last cached packument, even from its own (same-namespace) mirror.
    /// Only a transport failure (`5xx`/timeout/network) falls back, and only
    /// within the scoped mirror `pkg_mirror` already points at. A public
    /// route (the CLI / public registries) keeps the original
    /// fall-back-on-any-error behavior.
    pub(super) async fn disk_fallback(
        &self,
        error: &FetchMetadataError,
        disk_meta: Option<Arc<Package>>,
    ) -> Option<Arc<Package>> {
        let allow_fallback =
            matches!(self.scope, MetadataCacheScope::Public) || !error.is_access_denied();
        if !allow_fallback {
            return None;
        }
        match disk_meta {
            Some(meta) => Some(meta),
            None => load_meta_async(self.pkg_mirror.as_deref()).await.map(Arc::new),
        }
    }
}
