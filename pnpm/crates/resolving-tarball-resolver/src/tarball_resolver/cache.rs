use std::{path::Path, sync::Arc};

use pnpm_reporter::SilentReporter;
use pnpm_resolving_resolver_base::{ResolveError, ResolveResult, WantedDependency};
use pnpm_store_dir::store_index_key;
use pnpm_tarball::{PrefetchResult, ResolvedTarball, TarballResolutionFetch};

use super::TarballResolver;
use crate::http_cache::{self, Freshness, TarballResolutionRecord};

impl TarballResolver {
    /// Resolve from the persistent URL → integrity record when its archive
    /// is still in the store: directly while the record is fresh, and after
    /// an `If-None-Match` revalidation once it is stale. `None` leaves the
    /// caller to fetch the tarball.
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
        let freshness = record.freshness(now);
        if freshness == Freshness::Unusable
            || !self.may_cache(&record.tarball, normalized_bare_specifier)
        {
            http_cache::remove(cache_dir, normalized_bare_specifier);
            return Ok(None);
        }
        let Some(reused) =
            self.reuse_resolution(wanted_dependency, normalized_bare_specifier, &record).await
        else {
            http_cache::remove(cache_dir, normalized_bare_specifier);
            return Ok(None);
        };
        if freshness == Freshness::Fresh {
            return Ok(Some(reused));
        }
        self.revalidate_http_cache(
            wanted_dependency,
            normalized_bare_specifier,
            cache_dir,
            record,
            reused,
        )
        .await
    }

    async fn revalidate_http_cache(
        &self,
        wanted_dependency: &WantedDependency,
        normalized_bare_specifier: &str,
        cache_dir: &Path,
        record: TarballResolutionRecord,
        reused: ResolveResult,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        let Some(etag) = record.etag.as_deref() else {
            return Ok(None);
        };
        let Some(ctx) = self.fetch_context.as_ref() else {
            return Ok(None);
        };
        let fetched = self
            .tarball_fetch(ctx, normalized_bare_specifier, &record.tarball)
            .run_with_cache::<SilentReporter>(ctx.mem_cache.as_deref(), Some(etag))
            .await
            .map_err(|err| Box::new(err) as ResolveError)?;
        match fetched {
            TarballResolutionFetch::NotModified(response) => {
                if response.final_url != record.final_url
                    || http_cache::varies_on_everything(&response.cache_headers)
                {
                    http_cache::remove(cache_dir, normalized_bare_specifier);
                    return Ok(None);
                }
                let renewed = record.renewed(&response.cache_headers, http_cache::now_ms());
                http_cache::store(cache_dir, &renewed);
                Ok(Some(reused))
            }
            TarballResolutionFetch::Resolved(resolved) => {
                let tarball = cached_tarball_url(&record.tarball, &resolved);
                self.remember_http_cache(normalized_bare_specifier, &tarball, &resolved);
                Ok(Some(Self::head_only_result(
                    wanted_dependency,
                    normalized_bare_specifier.to_string(),
                    tarball,
                    Some(resolved.integrity),
                    resolved.manifest.map(Arc::new),
                )))
            }
        }
    }

    /// Record the response that produced `resolved`, fetched from
    /// `resolved_url` for `normalized_bare_specifier`.
    pub(super) fn remember_http_cache(
        &self,
        normalized_bare_specifier: &str,
        resolved_url: &str,
        resolved: &ResolvedTarball,
    ) {
        let Some(cache_dir) = self.http_cache_dir(normalized_bare_specifier) else {
            return;
        };
        let tarball = cached_tarball_url(resolved_url, resolved);
        if !self.may_cache(&tarball, normalized_bare_specifier)
            || http_cache::varies_on_everything(&resolved.cache_headers)
        {
            http_cache::remove(cache_dir, normalized_bare_specifier);
            return;
        }
        http_cache::store(
            cache_dir,
            &TarballResolutionRecord::from_response(
                normalized_bare_specifier.to_owned(),
                tarball,
                resolved.final_url.clone(),
                resolved.integrity.to_string(),
                &resolved.cache_headers,
                http_cache::now_ms(),
            ),
        );
    }

    /// The cache dir for records about `url`, or `None` when nothing may be
    /// cached for it.
    fn http_cache_dir(&self, url: &str) -> Option<&Path> {
        let ctx = self.fetch_context.as_ref()?;
        if !self.may_cache(url, url) {
            return None;
        }
        ctx.cache_dir.as_deref()
    }

    /// A response fetched with credentials is never reused without asking
    /// the origin, so revoking the credentials takes effect on the next
    /// install. This covers the requested URL and an immutable redirect
    /// target alike. The lookup takes the same package ID as the archive
    /// request, so both see the same credentials.
    fn may_cache(&self, url: &str, package_id: &str) -> bool {
        self.fetch_context
            .as_ref()
            .is_some_and(|ctx| {
                ctx.auth_headers
                    .for_url_with_package(url, Some(package_id))
                    .is_none()
            })
    }

    async fn reuse_resolution(
        &self,
        wanted_dependency: &WantedDependency,
        normalized_bare_specifier: &str,
        record: &TarballResolutionRecord,
    ) -> Option<ResolveResult> {
        let integrity = record.integrity.parse().ok()?;
        let ctx = self.fetch_context.as_ref()?;
        let cache_key = store_index_key(&record.integrity, normalized_bare_specifier);
        let PrefetchResult { cas_paths, manifests, .. } = ctx.prefetch(&cache_key).await;
        if !cas_paths.contains_key(&cache_key) {
            return None;
        }
        let manifest = manifests.get(&cache_key)?;
        Some(Self::head_only_result(
            wanted_dependency,
            normalized_bare_specifier.to_string(),
            record.tarball.clone(),
            Some(integrity),
            Some(Arc::clone(manifest)),
        ))
    }
}

/// The URL to record for a response fetched from `requested_url`: the
/// final URL after redirects when the response is `immutable`, as the
/// lockfile records it.
fn cached_tarball_url(requested_url: &str, resolved: &ResolvedTarball) -> String {
    if resolved.cache_headers.cache_control
        .as_deref()
        .is_some_and(|header| http_cache::CacheControl::parse(header).immutable)
    {
        resolved.final_url.clone()
    } else {
        requested_url.to_owned()
    }
}
