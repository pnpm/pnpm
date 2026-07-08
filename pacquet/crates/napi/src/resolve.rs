//! `resolveDependency` — resolve a wanted dependency to a concrete version.
//!
//! Replaces Bit's use of `@pnpm/installing.client`'s
//! `createResolver(...).resolve(...)` (via `generateResolverAndFetcher` /
//! `resolveRemoteVersion`).
//!
//! Mirrors the install path's [`DefaultResolver`] chain (see
//! `pacquet_package_manager::install_with_fresh_lockfile`) so a single
//! resolve claims every protocol the install claims: npm registry
//! (`name@version` / `range` / `tag`, incl. the `foo@npm:bar` alias
//! form), git URLs, `http(s)` tarball URLs, `file:` / `link:` /
//! `workspace:` and bare filesystem paths, the node / deno / bun
//! runtime specs, and `<alias>:` named-registry specs. A specifier no
//! resolver in the chain claims surfaces as
//! `SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`.
//!
//! The one deviation from the install chain: the tarball resolver runs
//! without a fetch context (no store to extract into on this
//! single-resolve path), so an `http(s)` tarball is claimed with its
//! normalized URL but no bundled manifest / integrity — those live in
//! the tarball's `package.json` and only the install pass extracts
//! them. Custom (pnpmfile) resolvers are also omitted: loading the
//! pnpmfile is an install-time concern. See `pacquet/plans/NAPI.md`.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

use napi_derive::napi;
use pacquet_engine_runtime_bun_resolver::BunResolver;
use pacquet_engine_runtime_deno_resolver::DenoResolver;
use pacquet_engine_runtime_node_resolver::NodeResolver;
use pacquet_network::{NetworkSettings, RetryOpts, ThrottledClient};
use pacquet_resolving_default_resolver::DefaultResolver;
use pacquet_resolving_git_resolver::{GitResolver, RealGitProbe, RealGitRunner};
use pacquet_resolving_local_resolver::{
    LocalPathResolver, LocalResolverContext, LocalSchemeResolver,
};
use pacquet_resolving_npm_resolver::{
    NamedRegistryResolver, NpmResolver, merge_named_registries, shared_in_memory_cache,
    shared_packument_fetch_locker, shared_picked_manifest_cache,
};
use pacquet_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};
use pacquet_resolving_tarball_resolver::TarballResolver;

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
    let retry_opts = RetryOpts {
        retries: config.fetch_retries,
        factor: config.fetch_retry_factor,
        min_timeout: std::time::Duration::from_millis(config.fetch_retry_mintimeout),
        max_timeout: std::time::Duration::from_millis(config.fetch_retry_maxtimeout),
    };

    // Shared behind an `Arc` so the deno / bun runtime resolvers (which
    // reuse the npm resolver for their own version picking) and the
    // chain slot below all point at the same instance and its metadata
    // cache.
    let npm_resolver: Arc<dyn Resolver> = Arc::new(NpmResolver {
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
        retry_opts,
    });

    let git_resolver = GitResolver::new(
        Arc::new(RealGitProbe::new(Arc::clone(&http_client))),
        Arc::new(RealGitRunner::new()),
    );

    // No fetch context on the single-resolve path: there's no install
    // store to extract into, so an `http(s)` tarball is claimed with its
    // normalized URL but no bundled manifest / integrity (see the module
    // docs). The install path wires the full [`TarballFetchContext`].
    let tarball_resolver =
        TarballResolver { http_client: Arc::clone(&http_client), fetch_context: None };

    // `preserveAbsolutePaths` isn't exposed by pacquet's `Config` yet, so
    // the local-resolver context defaults to `false` here — same as the
    // install path.
    let local_ctx = LocalResolverContext { preserve_absolute_paths: false };
    let local_scheme_resolver = LocalSchemeResolver::new(local_ctx);
    let local_path_resolver = LocalPathResolver::new(local_ctx);

    let mut node_resolver = NodeResolver::new(Arc::clone(&http_client));
    node_resolver.offline = config.offline;
    let mut deno_resolver = DenoResolver::new(Arc::clone(&http_client), Arc::clone(&npm_resolver));
    deno_resolver.offline = config.offline;
    let mut bun_resolver = BunResolver::new(Arc::clone(&http_client), Arc::clone(&npm_resolver));
    bun_resolver.offline = config.offline;

    // User-supplied named-registry aliases from
    // `pnpm-workspace.yaml#namedRegistries`, merged with pacquet's
    // built-ins (today: `gh:` → GitHub Packages). A malformed URL here
    // fails fast with `ERR_PNPM_INVALID_NAMED_REGISTRY_URL`, matching the
    // install path.
    let user_named_registries: HashMap<String, String> =
        config.named_registries.iter().map(|(name, url)| (name.clone(), url.clone())).collect();
    let merged_named_registries =
        merge_named_registries(&user_named_registries).map_err(|error| to_napi_error(&error))?;
    let named_registry_aliases: HashSet<String> = merged_named_registries.keys().cloned().collect();
    let named_registry_resolver = NamedRegistryResolver {
        named_registries: merged_named_registries,
        registry_names: named_registry_aliases,
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
        retry_opts,
    };

    // Chain order mirrors the install path
    // (`install_with_fresh_lockfile.rs`): npm → git → tarball →
    // localScheme → node → deno → bun → namedRegistry → localPath. The
    // local-resolver split (scheme before the runtimes, path last) lets a
    // `<alias>:@scope/pkg` named-registry specifier reach the
    // named-registry resolver instead of being claimed by the path-shape
    // detector on the strength of its embedded `/`.
    let chain: Vec<Box<dyn Resolver>> = vec![
        Box::new(Arc::clone(&npm_resolver)),
        Box::new(git_resolver),
        Box::new(tarball_resolver),
        Box::new(local_scheme_resolver),
        Box::new(node_resolver),
        Box::new(deno_resolver),
        Box::new(bun_resolver),
        Box::new(named_registry_resolver),
        Box::new(local_path_resolver),
    ];
    let resolver = DefaultResolver::new(chain);

    let wanted_dependency = WantedDependency {
        alias: wanted.alias,
        bare_specifier: wanted.bare_specifier,
        injected: None,
        prev_specifier: None,
        optional: None,
    };
    let resolve_options =
        ResolveOptions { project_dir: dir.clone(), lockfile_dir: dir, ..ResolveOptions::default() };

    // A single dependency resolve is one packument fetch — no task parallelism
    // to exploit — and this already runs on a dedicated worker thread. Use a
    // current-thread runtime so a `resolveDependency` call spawns no extra
    // worker-thread pool (a per-call multi-thread runtime would multiply threads
    // under concurrent resolves). The install path keeps a multi-thread runtime
    // because it fetches packages in parallel.
    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|error| {
            napi::Error::from_reason(format!("failed to build tokio runtime: {error}"))
        })?;

    // The inherent [`DefaultResolver::resolve`] (not the `Resolver`-trait
    // method) is chosen here: it raises `SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`
    // when no resolver in the chain claims the spec, rather than the
    // trait's `Ok(None)`.
    let resolved = runtime
        .block_on(async { resolver.resolve(&wanted_dependency, &resolve_options).await })
        // `Resolver::resolve` erases its error to `ResolveError`
        // (`Box<dyn Error>`), so the underlying miette `Diagnostic` — and its
        // `ERR_PNPM_*` code / hint — is already gone by the time it reaches
        // here; only the message survives. Restoring the code on this path
        // requires the resolver trait to carry a typed diagnostic error, a
        // pacquet-core change tracked as a follow-up.
        .map_err(|error| napi::Error::from_reason(error.to_string()))?;

    Ok(ResolveDependencyResult {
        id: resolved.id.to_string(),
        manifest: resolved.manifest.map(|manifest| (*manifest).clone()),
        resolved_via: resolved.resolved_via,
        normalized_bare_specifier: resolved.normalized_bare_specifier,
        latest: resolved.latest,
    })
}

#[cfg(test)]
mod tests;
