//! `resolveDependency`: resolve a wanted dependency to a concrete version.
//!
//! Replaces Bit's use of `@pnpm/installing.client`'s
//! `createResolver(...).resolve(...)` (via `generateResolverAndFetcher` /
//! `resolveRemoteVersion`).
//!
//! Mirrors the install path's
//! [`DefaultResolver`][pnpm_resolving_default_resolver::DefaultResolver] chain (see
//! `pnpm_package_manager::install_with_fresh_lockfile`) so a single
//! resolve claims every protocol the install claims. A specifier no
//! resolver in the chain claims surfaces as
//! `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`.
//!
//! The chain itself is
//! [`pnpm_resolving_default_resolver::standalone::build_standalone_chain`],
//! shared with the other single-resolve callers; its module documents the
//! two deviations from the install chain. See `pnpm/plans/NAPI.md`.

use std::{
    collections::HashMap,
    path::PathBuf,
};

use napi_derive::napi;
use pnpm_resolving_default_resolver::{
    SpecNotSupportedByAnyResolverError,
    standalone::{
        LocalTarballResolution,
        StandaloneChainOptions,
        build_standalone_chain,
    },
};
use pnpm_resolving_deps_resolver::ResolveDependencyTreeError;
use pnpm_resolving_local_resolver::ResolveLocalError;
use pnpm_resolving_resolver_base::{
    ResolveError,
    ResolveOptions,
    WantedDependency,
};

use crate::{
    config::{
        ConfigOverlay,
        resolve_config,
    },
    error::to_napi_error,
    install::{
        NetworkConfigInput,
        ProxyConfigInput,
        shared_network_overlay,
    },
};

/// The `(alias, bareSpecifier)` a resolve is requested for.
#[napi(object)]
pub struct WantedDependencyInput {
    pub alias: Option<String>,
    pub bare_specifier: Option<String>,
}

/// Options for [`resolve_dependency`].
#[napi(object)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror the public JavaScript object exposed by the NAPI addon."
    )
)]
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
    pub proxy_config: Option<ProxyConfigInput>,
    pub network_config: Option<NetworkConfigInput>,
}

/// Result of [`resolve_dependency`].
#[napi(object)]
pub struct ResolveDependencyResult {
    pub id: String,
    pub manifest: Option<serde_json::Value>,
    pub resolved_via: String,
    pub normalized_bare_specifier: Option<String>,
    pub latest: Option<String>,
    /// The lockfile `resolution` object of the resolved package.
    pub resolution: serde_json::Value,
}

#[napi(js_name = "resolveDependency")]
pub async fn resolve_dependency(
    wanted: WantedDependencyInput,
    options: ResolveDependencyOptions,
) -> napi::Result<ResolveDependencyResult> {
    pnpm_package_manager::configure_rayon_pool();
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

fn normalize_wanted_dependency(wanted: WantedDependencyInput) -> WantedDependency {
    WantedDependency {
        alias: wanted.alias,
        // An empty bareSpecifier means "no range given", pnpm v11's
        // resolver treated it like an absent pref (resolve the latest
        // matching version); passing it through verbatim would fall off
        // the resolver chain as ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER.
        bare_specifier: wanted.bare_specifier.filter(|spec| !spec.trim().is_empty()),
        injected: None,
        prev_specifier: None,
        optional: None,
    }
}

fn run_resolve_blocking(
    wanted: WantedDependencyInput,
    options: &ResolveDependencyOptions,
) -> napi::Result<ResolveDependencyResult> {
    let dir = PathBuf::from(&options.dir);
    let config =
        resolve_config(&dir, &resolve_overlay(options)?).map_err(|error| to_napi_error(&error))?;

    let http_client = crate::install::install_http_client(config)?;

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
        local_tarball_resolution: LocalTarballResolution::ReadMetadata,
    })
    .map_err(|error| to_napi_error(&error))?;

    let wanted_dependency = normalize_wanted_dependency(wanted);
    let resolve_options = ResolveOptions {
        project: pnpm_resolving_resolver_base::ResolverProjectOptions {
            project_dir: dir.clone(),
            lockfile_dir: dir,
            ..Default::default()
        },
        ..ResolveOptions::default()
    };

    let runtime = current_thread_runtime()?;
    // The inherent [`DefaultResolver::resolve`] (not the `Resolver`-trait
    // method) is chosen here: it raises `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`
    // when no resolver in the chain claims the spec, rather than the
    // trait's `Ok(None)`.
    let resolved = runtime
        .block_on(async { resolver.resolve(&wanted_dependency, &resolve_options).await })
        .map_err(resolve_error_to_napi)?;
    let resolution = serde_json::to_value(&resolved.resolution)
        .map_err(|error| {
            napi::Error::from_reason(format!("serializing the resolution: {error}"))
        })?;

    Ok(ResolveDependencyResult {
        id: resolved.id.to_string(),
        manifest: resolved.package.manifest.map(|manifest| (*manifest).clone()),
        resolved_via: resolved.resolved_via,
        normalized_bare_specifier: resolved.normalized_bare_specifier,
        latest: resolved.package.latest,
        resolution,
    })
}

/// A single dependency resolve is one packument fetch (no task parallelism to
/// exploit) and already runs on a dedicated worker thread. A current-thread
/// runtime spawns no extra worker-thread pool, which a per-call multi-thread
/// runtime would multiply under concurrent resolves. The install path keeps a
/// multi-thread runtime because it fetches packages in parallel.
fn current_thread_runtime() -> napi::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            napi::Error::from_reason(format!("failed to build tokio runtime: {error}"))
        })
}

/// Recover the `ERR_PNPM_*` code that [`ResolveError`]'s type erasure hides.
/// An error that carries none keeps its plain message.
fn resolve_error_to_napi(error: ResolveError) -> napi::Error {
    let error = match error.downcast::<SpecNotSupportedByAnyResolverError>() {
        Ok(not_supported) => return to_napi_error(&*not_supported),
        Err(error) => error,
    };
    let error = match error.downcast::<ResolveLocalError>() {
        Ok(local) => return to_napi_error(&*local),
        Err(error) => error,
    };
    match ResolveDependencyTreeError::from_resolve_error(error) {
        ResolveDependencyTreeError::Resolve(error) => napi::Error::from_reason(error.to_string()),
        coded => to_napi_error(&coded),
    }
}

#[cfg(test)]
mod tests;

fn resolve_overlay(options: &ResolveDependencyOptions) -> napi::Result<ConfigOverlay> {
    let overlay = shared_network_overlay(
        "resolveDependency",
        options.proxy_config.as_ref(),
        options.network_config.as_ref(),
    )?;
    Ok(ConfigOverlay {
        store_dir: options.store_dir.as_ref().map(PathBuf::from),
        cache_dir: options.cache_dir.as_ref().map(PathBuf::from),
        registries: options.registries
            .clone()
            .map(std::iter::IntoIterator::into_iter)
            .map(std::iter::Iterator::collect),
        offline: options.offline,
        prefer_offline: options.prefer_offline,
        auth_header_by_uri: options.auth_header_by_uri
            .clone()
            .map(std::iter::IntoIterator::into_iter)
            .map(std::iter::Iterator::collect),
        ..overlay
    })
}
