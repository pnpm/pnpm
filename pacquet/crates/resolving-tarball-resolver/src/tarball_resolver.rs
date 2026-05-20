//! Pacquet port of pnpm's
//! [`resolveFromTarball`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/tarball-resolver/src/index.ts#L10-L38)
//! plus the `resolveLatestFromTarball` companion at the same file's
//! lines 43-47.

use std::sync::Arc;

use pacquet_lockfile::{LockfileResolution, TarballResolution};
use pacquet_network::ThrottledClient;
use pacquet_resolving_resolver_base::{
    LatestInfo, LatestQuery, PkgResolutionId, ResolveError, ResolveFuture, ResolveLatestFuture,
    ResolveOptions, ResolveResult, Resolver, WantedDependency,
};

/// Resolves `http://...` / `https://...` tarball URLs from a project's
/// manifest. One instance per install — the throttled HTTP client is
/// shared with the rest of the install pipeline so the HEAD
/// pre-flight respects the same concurrency cap as metadata fetches
/// and tarball downloads.
pub struct TarballResolver {
    pub http_client: Arc<ThrottledClient>,
}

impl Resolver for TarballResolver {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        Box::pin(self.resolve_impl(wanted_dependency))
    }

    fn resolve_latest<'a>(
        &'a self,
        query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async move { Ok(resolve_latest(query)) })
    }
}

impl TarballResolver {
    async fn resolve_impl(
        &self,
        wanted_dependency: &WantedDependency,
    ) -> Result<Option<ResolveResult>, ResolveError> {
        let Some(bare) = wanted_dependency.bare_specifier.as_deref() else {
            return Ok(None);
        };
        if !is_http_url(bare) {
            return Ok(None);
        }

        // Round-trip through `Url::parse` to drop a redundant default
        // port (`registry.npmjs.org:443` → `registry.npmjs.org`),
        // matching upstream's `new URL(spec).toString()`.
        let normalized_bare_specifier =
            reqwest::Url::parse(bare).map_err(|err| Box::new(err) as ResolveError)?.to_string();

        let client = self.http_client.acquire_for_url(&normalized_bare_specifier).await;
        let response = client
            .head(&normalized_bare_specifier)
            .send()
            .await
            .map_err(|err| Box::new(err) as ResolveError)?;

        // If the upstream marks the response immutable, store the
        // *post-redirect* URL so subsequent installs hit the
        // canonical location directly. Mutable responses (and
        // missing headers) keep the normalized request URL so the
        // moving target gets revalidated next run.
        let resolved_url = if response
            .headers()
            .get(reqwest::header::CACHE_CONTROL)
            .and_then(|header| header.to_str().ok())
            .is_some_and(|header| header.contains("immutable"))
        {
            response.url().to_string()
        } else {
            normalized_bare_specifier.clone()
        };

        Ok(Some(ResolveResult {
            id: PkgResolutionId::from(normalized_bare_specifier.clone()),
            // Tarball URLs carry no `name@version` at resolve time —
            // the canonical name/version come from the manifest after
            // the tarball is fetched. Downstream consumers fall back
            // to reading the manifest when `name_ver` is `None`.
            name_ver: None,
            latest: None,
            published_at: None,
            manifest: None,
            resolution: LockfileResolution::Tarball(TarballResolution {
                tarball: resolved_url,
                integrity: None,
                git_hosted: None,
                path: None,
            }),
            resolved_via: "url".to_string(),
            normalized_bare_specifier: Some(normalized_bare_specifier),
            alias: wanted_dependency.alias.clone(),
            policy_violation: None,
        }))
    }
}

/// URL tarballs lock to the exact URL — no concept of "latest". When
/// the wanted dep names an http(s) URL, claim it so the dispatcher
/// stops; the caller still surfaces a ref-mismatch report if the
/// lockfile points at a different URL than before. Mirrors upstream's
/// [`resolveLatestFromTarball`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/tarball-resolver/src/index.ts#L43-L47).
fn resolve_latest(query: &LatestQuery) -> Option<LatestInfo> {
    let bare = query.wanted_dependency.bare_specifier.as_deref()?;
    if !is_http_url(bare) {
        return None;
    }
    Some(LatestInfo { latest_manifest: None })
}

fn is_http_url(bare: &str) -> bool {
    bare.starts_with("http:") || bare.starts_with("https:")
}

#[cfg(test)]
mod tests;
