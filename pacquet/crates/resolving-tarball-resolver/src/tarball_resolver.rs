//! Pacquet port of pnpm's
//! [`resolveFromTarball`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/tarball-resolver/src/index.ts#L10-L38)
//! plus the `resolveLatestFromTarball` companion at the same file's
//! lines 43-47.

use std::sync::Arc;

use pacquet_lockfile::{LockfileResolution, TarballResolution};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_reporter::SilentReporter;
use pacquet_resolving_resolver_base::{
    LatestInfo, LatestQuery, PkgResolutionId, ResolveError, ResolveFuture, ResolveLatestFuture,
    ResolveOptions, ResolveResult, Resolver, WantedDependency,
};
use pacquet_store_dir::{StoreDir, StoreIndexWriter};
use pacquet_tarball::{FetchTarballForResolution, MemCache, RetryOpts};
use ssri::Integrity;

/// Store/network handles the [`TarballResolver`] needs to fetch a
/// remote tarball during resolution — download it, compute its sha512
/// integrity, extract it to the store, and read its bundled manifest.
///
/// The install orchestrator owns these and hands the resolver a clone;
/// `mem_cache` (when present) is warmed keyed by URL so the install
/// pass reuses the extraction without re-downloading. Absent only in
/// unit tests that exercise the HEAD/normalize/redirect logic in
/// isolation — see [`TarballResolver`].
pub struct TarballFetchContext {
    pub store_dir: &'static StoreDir,
    pub store_index_writer: Option<Arc<StoreIndexWriter>>,
    pub mem_cache: Option<Arc<MemCache>>,
    pub auth_headers: Arc<AuthHeaders>,
    pub retry_opts: RetryOpts,
}

/// Resolves `http://...` / `https://...` tarball URLs from a project's
/// manifest. One instance per install — the throttled HTTP client is
/// shared with the rest of the install pipeline so the HEAD
/// pre-flight respects the same concurrency cap as metadata fetches
/// and tarball downloads.
///
/// When `fetch_context` is `Some`, the resolver downloads the tarball
/// during resolution to fill `manifest` + `integrity` into its
/// [`ResolveResult`] — name/version/integrity for a remote tarball live
/// in the tarball's `package.json`, and pacquet builds the lockfile
/// before the install/fetch pass. `None` (unit tests only) keeps the
/// HEAD-only shape with no manifest/integrity.
pub struct TarballResolver {
    pub http_client: Arc<ThrottledClient>,
    pub fetch_context: Option<TarballFetchContext>,
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

        // No store context (unit tests): keep the HEAD-only shape. The
        // download below is what fills `manifest` + `integrity`; without
        // a store to extract into there's nothing to fetch, so leave
        // them unset.
        let Some(ctx) = self.fetch_context.as_ref() else {
            return Ok(Some(self.head_only_result(
                wanted_dependency,
                normalized_bare_specifier,
                resolved_url,
                None,
                None,
            )));
        };

        // Download the tarball, compute its sha512 integrity, extract it
        // to the store, and read its bundled manifest. Warms `mem_cache`
        // (keyed by `resolved_url`) so the install pass reuses the
        // extraction. Silent reporter: the install pass owns the
        // `resolved → found_in_store → imported` event ordering (see
        // `prefetching_resolver.rs`).
        let resolved = FetchTarballForResolution {
            http_client: &self.http_client,
            store_dir: ctx.store_dir,
            store_index_writer: ctx.store_index_writer.clone(),
            package_url: &resolved_url,
            auth_headers: &ctx.auth_headers,
            retry_opts: ctx.retry_opts,
        }
        .run::<SilentReporter>(ctx.mem_cache.as_deref())
        .await
        .map_err(|err| Box::new(err) as ResolveError)?;

        Ok(Some(self.head_only_result(
            wanted_dependency,
            normalized_bare_specifier,
            resolved_url,
            Some(resolved.integrity),
            resolved.manifest.map(Arc::new),
        )))
    }

    /// Build the `ResolveResult` for a claimed http(s) tarball.
    /// `name_ver` stays `None` (URL-id semantics: the depPath is
    /// `name@<url>`, derived downstream from the manifest name);
    /// `integrity` and `manifest` are filled once the tarball is
    /// fetched.
    fn head_only_result(
        &self,
        wanted_dependency: &WantedDependency,
        normalized_bare_specifier: String,
        resolved_url: String,
        integrity: Option<Integrity>,
        manifest: Option<Arc<serde_json::Value>>,
    ) -> ResolveResult {
        ResolveResult {
            id: PkgResolutionId::from(normalized_bare_specifier.clone()),
            name_ver: None,
            latest: None,
            published_at: None,
            manifest,
            resolution: LockfileResolution::Tarball(TarballResolution {
                tarball: resolved_url,
                integrity,
                git_hosted: None,
                path: None,
            }),
            resolved_via: "url".to_string(),
            normalized_bare_specifier: Some(normalized_bare_specifier),
            alias: wanted_dependency.alias.clone(),
            policy_violation: None,
        }
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
