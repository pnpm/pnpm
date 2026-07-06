//! `resolveDependency` — resolve a wanted dependency to a concrete version.
//!
//! Replaces Bit's use of `@pnpm/installing.client`'s
//! `createResolver(...).resolve(...)` (via `generateResolverAndFetcher` /
//! `resolveRemoteVersion`). Backed by `pacquet_resolving_npm_resolver`.
//!
//! Only the **npm registry** protocol is handled today — the common case
//! (`name@version` / `name@range` / `name@tag`, including the `foo@npm:bar`
//! alias form). A specifier no npm resolver claims (git URL, tarball URL,
//! local path) yields a structured error rather than being silently dropped;
//! wiring the full default-resolver chain (git / tarball / local) is a
//! follow-up. See `pacquet/plans/NAPI.md`.

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use napi_derive::napi;
use pacquet_network::{NetworkSettings, RetryOpts, ThrottledClient};
use pacquet_resolving_npm_resolver::{
    NpmResolver, shared_in_memory_cache, shared_packument_fetch_locker,
    shared_picked_manifest_cache,
};
use pacquet_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};

use crate::{
    config::{ConfigOverlay, resolve_config},
    error::to_napi_error,
};

/// The `(alias, bareSpecifier)` a resolve is requested for. Mirrors
/// `WantedDependency` in `index.d.ts`.
#[napi(object)]
pub struct WantedDependencyInput {
    pub alias: Option<String>,
    pub bare_specifier: Option<String>,
}

/// Options for [`resolve_dependency`]. Mirrors `ResolveOptions` in
/// `index.d.ts`.
#[napi(object)]
pub struct ResolveDependencyOptions {
    pub dir: String,
    pub store_dir: Option<String>,
    pub cache_dir: Option<String>,
    pub registries: Option<HashMap<String, String>>,
    pub full_metadata: Option<bool>,
    pub offline: Option<bool>,
    pub prefer_offline: Option<bool>,
    /// Pre-computed `Authorization` headers keyed by nerf-darted registry URI
    /// (`""` for the default registry).
    pub auth_header_by_uri: Option<HashMap<String, String>>,
}

/// Result of [`resolve_dependency`]. Mirrors `ResolveResult` in `index.d.ts`.
#[napi(object)]
pub struct ResolveDependencyResult {
    pub id: String,
    pub manifest: Option<serde_json::Value>,
    pub resolved_via: String,
    pub normalized_bare_specifier: Option<String>,
    pub latest: Option<String>,
}

#[napi(js_name = "resolveDependency")]
pub async fn resolve_dependency(
    wanted: WantedDependencyInput,
    options: ResolveDependencyOptions,
) -> napi::Result<ResolveDependencyResult> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("pnpm-napi-resolve".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let _ = tx.send(run_resolve_blocking(wanted, &options));
        })
        .map_err(|error| {
            napi::Error::from_reason(format!("failed to spawn resolve thread: {error}"))
        })?;
    rx.await.map_err(|_| napi::Error::from_reason("resolve worker thread panicked"))?
}

fn run_resolve_blocking(
    wanted: WantedDependencyInput,
    options: &ResolveDependencyOptions,
) -> napi::Result<ResolveDependencyResult> {
    let dir = PathBuf::from(&options.dir);
    let overlay = ConfigOverlay {
        store_dir: options.store_dir.as_ref().map(PathBuf::from),
        cache_dir: options.cache_dir.as_ref().map(PathBuf::from),
        registries: options.registries.as_ref().map(|map| map.clone().into_iter().collect()),
        offline: options.offline,
        prefer_offline: options.prefer_offline,
        auth_header_by_uri: options.auth_header_by_uri.clone().map(|map| map.into_iter().collect()),
        ..ConfigOverlay::default()
    };
    let config = resolve_config(&dir, &overlay).map_err(|error| to_napi_error(&error))?;

    let http_client = Arc::new(
        ThrottledClient::for_installs(
            &config.proxy,
            &config.tls,
            &config.tls_by_uri,
            &NetworkSettings {
                network_concurrency: config.network_concurrency,
                fetch_timeout: std::time::Duration::from_millis(config.fetch_timeout),
                user_agent: config.user_agent.clone(),
            },
        )
        .map_err(|error| to_napi_error(&error))?,
    );

    let full_metadata = options.full_metadata.unwrap_or(false);
    let resolver = NpmResolver {
        // `resolved_registries` inserts the `default` route from `config.registry`;
        // `config.registries` alone omits it, which would leave the picker with a
        // host-less `/pkg` URL.
        registries: config.resolved_registries().into_iter().collect(),
        named_registries: config.named_registries.clone().into_iter().collect(),
        http_client: Arc::clone(&http_client),
        auth_headers: Arc::clone(&config.auth_headers),
        meta_cache: shared_in_memory_cache(),
        fetch_locker: shared_packument_fetch_locker(),
        picked_manifest_cache: shared_picked_manifest_cache(),
        cache_dir: Some(config.cache_dir.clone()),
        offline: config.offline,
        prefer_offline: config.prefer_offline,
        ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
        full_metadata,
        filter_metadata: full_metadata,
        retry_opts: RetryOpts {
            retries: config.fetch_retries,
            factor: config.fetch_retry_factor,
            min_timeout: std::time::Duration::from_millis(config.fetch_retry_mintimeout),
            max_timeout: std::time::Duration::from_millis(config.fetch_retry_maxtimeout),
        },
    };

    let wanted_dependency = WantedDependency {
        alias: wanted.alias,
        bare_specifier: wanted.bare_specifier,
        injected: None,
        prev_specifier: None,
        optional: None,
    };
    let resolve_options =
        ResolveOptions { project_dir: dir.clone(), lockfile_dir: dir, ..ResolveOptions::default() };

    let runtime =
        tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|error| {
            napi::Error::from_reason(format!("failed to build tokio runtime: {error}"))
        })?;

    let resolved = runtime
        .block_on(async { resolver.resolve(&wanted_dependency, &resolve_options).await })
        // `Resolver::resolve` erases its error to `ResolveError`
        // (`Box<dyn Error>`), so the underlying miette `Diagnostic` — and its
        // `ERR_PNPM_*` code / hint — is already gone by the time it reaches
        // here; only the message survives. Restoring the code on this path
        // requires the resolver trait to carry a typed diagnostic error, a
        // pacquet-core change tracked as a follow-up.
        .map_err(|error| napi::Error::from_reason(error.to_string()))?;

    let Some(resolved) = resolved else {
        return Err(napi::Error::from_reason(
            "the specifier was not claimed by the npm resolver; only npm registry specifiers \
             (name@version / range / tag) are supported today"
                .to_string(),
        ));
    };

    Ok(ResolveDependencyResult {
        id: resolved.id.to_string(),
        manifest: resolved.manifest.map(|manifest| (*manifest).clone()),
        resolved_via: resolved.resolved_via,
        normalized_bare_specifier: resolved.normalized_bare_specifier,
        latest: resolved.latest,
    })
}
