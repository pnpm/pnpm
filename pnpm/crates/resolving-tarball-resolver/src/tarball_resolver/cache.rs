use std::{path::Path, sync::Arc};

use pnpm_reporter::SilentReporter;
use pnpm_resolving_resolver_base::{ResolveError, ResolveResult, WantedDependency};
use pnpm_store_dir::store_index_key;
use pnpm_tarball::{PrefetchResult, TarballResolutionFetch};

use super::{PriorTarballEntry, TarballResolver};
use crate::http_cache::{self, Freshness, TarballResolutionRecord};

impl TarballResolver {
    /// Attempts to reuse a resolution from the persistent HTTP cache.
    pub(super) async fn reuse_from_http_cache(
        &self,
        wanted_dependency: &WantedDependency,
        normalized_bare_specifier: &str,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        let Some(cache_dir) = self.http_cache_dir(normalized_bare_specifier) else {
            return Ok(None);
        };
        let Some(record) = http_cache::load(cache_dir, normalized_bare_specifier) else {
            return Ok(None);
        };
        let now = http_cache::now_ms();
        match record.freshness(now) {
            Freshness::Unusable => {
                http_cache::remove(cache_dir, normalized_bare_specifier);
                Ok(None)
            }
            Freshness::Fresh => {
                Ok(self.reuse_resolution(wanted_dependency, normalized_bare_specifier, &record)
                    .await)
            }
            Freshness::Revalidate => {
                if self
                    .reuse_resolution(wanted_dependency, normalized_bare_specifier, &record)
                    .await
                    .is_none()
                {
                    http_cache::remove(cache_dir, normalized_bare_specifier);
                    return Ok(None);
                }
                self.revalidate_http_cache(
                    wanted_dependency,
                    normalized_bare_specifier,
                    cache_dir,
                    record,
                    now,
                )
                .await
            }
        }
    }

    async fn revalidate_http_cache(
        &self,
        wanted_dependency: &WantedDependency,
        normalized_bare_specifier: &str,
        cache_dir: &Path,
        record: TarballResolutionRecord,
        now: u64,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        let Some(etag) = record.etag.clone() else {
            return Ok(None);
        };
        let Some(ctx) = self.fetch_context.as_ref() else {
            return Ok(None);
        };
        let fetched = self
            .tarball_fetch(ctx, normalized_bare_specifier, &record.tarball)
            .run_with_cache::<SilentReporter>(ctx.mem_cache.as_deref(), Some(&etag))
            .await
            .map_err(|err| Box::new(err) as ResolveError)?;
        match fetched {
            TarballResolutionFetch::NotModified { etag, cache_control } => {
                let renewed = record.renewed(cache_control, etag, now);
                http_cache::store(cache_dir, &renewed);
                Ok(self.reuse_resolution(wanted_dependency, normalized_bare_specifier, &renewed)
                    .await)
            }
            TarballResolutionFetch::Resolved(resolved) => Ok(Some(self.handle_resolved_fetch(
                wanted_dependency,
                normalized_bare_specifier,
                &record.tarball,
                resolved,
            ))),
        }
    }

    fn handle_resolved_fetch(
        &self,
        wanted_dependency: &WantedDependency,
        normalized_bare_specifier: &str,
        record_tarball: &str,
        resolved: pnpm_tarball::ResolvedTarball,
    ) -> ResolveResult {
        let tarball = cached_tarball_url(record_tarball, &resolved);
        self.remember_http_cache(normalized_bare_specifier, &tarball, &resolved);
        Self::head_only_result(
            wanted_dependency,
            normalized_bare_specifier.to_string(),
            tarball,
            Some(resolved.integrity),
            resolved.manifest.map(Arc::new),
        )
    }

    pub(super) fn remember_http_cache(
        &self,
        normalized_bare_specifier: &str,
        resolved_url: &str,
        resolved: &pnpm_tarball::ResolvedTarball,
    ) {
        let Some(cache_dir) = self.http_cache_dir(normalized_bare_specifier) else {
            return;
        };
        let tarball = cached_tarball_url(resolved_url, resolved);
        http_cache::store(
            cache_dir,
            &TarballResolutionRecord {
                url: normalized_bare_specifier.to_owned(),
                tarball,
                integrity: resolved.integrity.to_string(),
                etag: resolved.etag.clone(),
                cache_control: resolved.cache_control.clone(),
                fetched_at: http_cache::now_ms(),
            },
        );
    }

    fn http_cache_dir(&self, url: &str) -> Option<&Path> {
        let ctx = self.fetch_context.as_ref()?;
        if ctx.auth_headers.for_url(url).is_some() {
            return None;
        }
        ctx.cache_dir.as_deref()
    }

    async fn reuse_resolution(
        &self,
        wanted_dependency: &WantedDependency,
        normalized_bare_specifier: &str,
        record: &TarballResolutionRecord,
    ) -> Option<ResolveResult> {
        let integrity = record.integrity.parse().ok()?;
        let prior = PriorTarballEntry {
            integrity,
            store_index_key: store_index_key(&record.integrity, normalized_bare_specifier),
            tarball_url: record.tarball.clone(),
        };
        let ctx = self.fetch_context.as_ref()?;
        let cache_key = &prior.store_index_key;
        let PrefetchResult { cas_paths, manifests, .. } = ctx.prefetch(cache_key).await;
        if !cas_paths.contains_key(cache_key) {
            return None;
        }
        let manifest = manifests.get(cache_key)?;
        Some(Self::head_only_result(
            wanted_dependency,
            normalized_bare_specifier.to_string(),
            prior.tarball_url,
            Some(prior.integrity),
            Some(Arc::clone(manifest)),
        ))
    }
}

pub(super) fn cached_tarball_url(
    requested_url: &str,
    resolved: &pnpm_tarball::ResolvedTarball,
) -> String {
    if resolved.cache_control
        .as_deref()
        .is_some_and(|header| http_cache::CacheControl::parse(header).immutable)
    {
        resolved.final_url.clone()
    } else {
        requested_url.to_owned()
    }
}
