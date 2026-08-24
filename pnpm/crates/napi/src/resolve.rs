//! `resolveDependency` — resolve a wanted dependency to a concrete version.
//!
//! Replaces Bit's use of `@pnpm/installing.client`'s
//! `createResolver(...).resolve(...)` (via `generateResolverAndFetcher` /
//! `resolveRemoteVersion`).
//!
//! Mirrors the install path's
//! [`DefaultResolver`][pnpm_resolving_default_resolver::DefaultResolver] chain (see
//! `pnpm_package_manager::install_with_fresh_lockfile`) so a single
//! resolve claims every protocol the install claims: npm registry
//! (`name@version` / `range` / `tag`, incl. the `foo@npm:bar` alias
//! form), git URLs, `http(s)` tarball URLs, `file:` / `link:` /
//! `workspace:` and bare filesystem paths, the node / deno / bun
//! runtime specs — including the `yarn@runtime:` line that ships as
//! release archives rather than as an npm package — and `<alias>:`
//! named-registry specs. A specifier no
//! resolver in the chain claims surfaces as
//! `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`.
//!
//! The chain itself is
//! [`pnpm_resolving_default_resolver::standalone::build_standalone_chain`],
//! shared with the other single-resolve callers; its module documents the
//! two deviations from the install chain. See `pnpm/plans/NAPI.md`.

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use napi_derive::napi;
use pnpm_network::ThrottledClient;
use pnpm_resolving_default_resolver::standalone::{StandaloneChainOptions, build_standalone_chain};
use pnpm_resolving_resolver_base::{ResolveOptions, WantedDependency};

use crate::{
    config::{ConfigOverlay, resolve_config},
    error::to_napi_error,
    reporter_bridge::NodeBridgeReporter,
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
    /// (`""` for the default registry, pinned to the `registries` passed
    /// alongside it).
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
            &config.network_settings(),
        )
        .map_err(|error| to_napi_error(&error))?,
    );
    http_client.set_warning_handler(pnpm_reporter::emit_global_warning::<NodeBridgeReporter>);

    let resolver = build_standalone_chain(&StandaloneChainOptions {
        config,
        http_client: &http_client,
        full_metadata: options.full_metadata.unwrap_or(false),
        // `filter_metadata` stays off even under `full_metadata`, unlike
        // the install path: a caller asking this API for full metadata
        // wants the version object's non-abbreviated fields (Bit reads
        // `componentId`), and `clear_meta` would drop exactly those.
        // Matches pnpm's `createClient({ fullMetadata: true })` with no
        // `filterMetadata`, which is what this API replaces.
        filter_metadata: false,
    })
    .map_err(|error| to_napi_error(&error))?;

    let wanted_dependency = WantedDependency {
        alias: wanted.alias,
        // An empty bareSpecifier means "no range given" — pnpm v11's
        // resolver treated it like an absent pref (resolve the latest
        // matching version); passing it through verbatim would fall off
        // the resolver chain as ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER.
        bare_specifier: wanted.bare_specifier.filter(|spec| !spec.trim().is_empty()),
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
    // method) is chosen here: it raises `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`
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
