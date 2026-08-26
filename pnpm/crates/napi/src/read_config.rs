//! `readConfig` — expose the engine's resolved configuration to the embedder.
//!
//! Hosts like Bit historically parsed `.npmrc` themselves (via
//! `@pnpm/config.reader`) to learn registries, credentials, proxy, and TLS
//! settings — only to hand most of it straight back to the engine. This
//! binding inverts that: the engine resolves the exact configuration its own
//! installs use (see [`crate::config::resolve_config`]) and returns the
//! subset an embedder consumes, so the host needs no JavaScript config
//! reader at all.

use std::collections::HashMap;

use napi_derive::napi;
use pnpm_network::{DEFAULT_REGISTRY_SCOPE, NoProxySetting, nerf_dart};

use crate::{
    config::{ConfigOverlay, resolve_config},
    error::to_napi_error,
};

#[napi(object)]
pub struct ReadConfigOptions {
    /// Directory whose config cascade to resolve — its `.npmrc`, the
    /// enclosing workspace's `pnpm-workspace.yaml` and `.npmrc`, the user
    /// and global config files, and `npm_config_*` environment variables.
    pub dir: String,
}

/// One configured registry: the `default` entry plus one per `@scope`.
#[napi(object)]
pub struct ResolvedRegistry {
    /// `"default"` or the package scope (`"@teambit"`).
    pub name: String,
    pub url: String,
    /// Ready-to-send `Authorization` header for this registry, when the
    /// config carries a static credential for it. `tokenHelper`
    /// credentials are not executed here and yield no header.
    pub auth_header: Option<String>,
}

#[napi(object)]
pub struct ResolvedConfig {
    pub registries: Vec<ResolvedRegistry>,
    /// Static `Authorization` headers keyed by nerf-darted registry URI
    /// (`//host[:port]/path/`) — the shape [`fn@crate::install`]'s
    /// `authHeaderByUri` accepts.
    pub auth_header_by_uri: HashMap<String, String>,
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    /// `true` (bypass every proxy) or a comma-separated host list.
    pub no_proxy: Option<serde_json::Value>,
    /// PEM-encoded CA certificates (`ca` / `cafile` already merged).
    pub ca: Option<Vec<String>>,
    pub cert: Option<String>,
    pub key: Option<String>,
    pub strict_ssl: Option<bool>,
    pub store_dir: String,
    pub cache_dir: String,
    pub virtual_store_dir_max_length: u32,
    /// Whether the resolved configuration uses the global virtual store.
    pub enable_global_virtual_store: bool,
    /// Shared virtual-store root.
    pub global_virtual_store_dir: String,
    /// Project-local virtual-store directory.
    pub virtual_store_dir: String,
    /// Virtual-store directory used by this configuration and recorded in `.modules.yaml`.
    pub effective_virtual_store_dir: String,
    pub network_concurrency: u32,
    pub max_sockets: Option<u32>,
    pub fetch_retries: u32,
    pub fetch_retry_factor: u32,
    pub fetch_retry_mintimeout: u32,
    pub fetch_retry_maxtimeout: u32,
    pub fetch_timeout: u32,
    pub fetch_warn_timeout_ms: u32,
    pub fetch_min_speed_ki_bps: u32,
    /// The explicitly configured user agent, when the cascade set one.
    /// The engine's own computed default is omitted — an embedder that
    /// passes nothing back to `install` gets that same default.
    pub user_agent: Option<String>,
    pub engine_strict: bool,
    pub node_version: Option<String>,
    /// `"auto"` / `"hardlink"` / `"copy"` / `"clone"` / `"clone-or-copy"`.
    pub package_import_method: String,
    pub hoist_pattern: Option<Vec<String>>,
    pub public_hoist_pattern: Option<Vec<String>>,
    /// The legacy `shamefullyHoist` flag; `publicHoistPattern` already
    /// reflects it, exposed for embedders that branch on the flag itself.
    pub shamefully_hoist: bool,
    /// The engine's pnpm home directory (`PNPM_HOME` or the platform
    /// default) — not a per-project setting. `None` when no home
    /// directory is resolvable.
    pub pnpm_home_dir: Option<String>,
    /// The camelCase names of settings the cascade set explicitly
    /// (`pnpm-workspace.yaml`, the global config, `pnpm_config_*` env
    /// vars). Every other projected value is an engine default; an
    /// embedder that layers this config over its own must forward only
    /// the explicit ones.
    pub explicit_settings: Vec<String>,
}

#[napi(js_name = "readConfig")]
pub fn read_config(options: ReadConfigOptions) -> napi::Result<ResolvedConfig> {
    let dir = std::path::PathBuf::from(options.dir);
    let config =
        resolve_config(&dir, &ConfigOverlay::default()).map_err(|error| to_napi_error(&error))?;
    Ok(project_config(config))
}

fn project_config(config: &pnpm_config::Config) -> ResolvedConfig {
    // `to_by_scope` maps `nerf-darted uri -> scope -> header`, carrying
    // only static credentials (a `tokenHelper` has no header until run).
    let by_scope = config.auth_headers.to_by_scope();
    let auth_header_by_uri: HashMap<String, String> = by_scope
        .iter()
        .filter_map(|(uri, by_scope)| {
            by_scope.get(DEFAULT_REGISTRY_SCOPE).map(|header| (uri.clone(), header.clone()))
        })
        .collect();

    // The default registry lives in `config.registry`; the `registries`
    // map carries it only when something set a `default` key explicitly.
    let default_entry = (!config.registries_by_scope.contains_key("default"))
        .then(|| ("default".to_string(), config.registry.clone()));
    let registries = default_entry
        .iter()
        .map(|(name, url)| (name, url))
        .chain(config.registries_by_scope.iter())
        .map(|(name, url)| {
            let uri = nerf_dart(url);
            let scoped = by_scope.get(&uri);
            // A scope registry prefers its scope-keyed credential; both
            // registry kinds fall back to the registry-wide (`@`) one.
            let auth_header = scoped
                .and_then(|headers| if name == "default" { None } else { headers.get(name) })
                .or_else(|| scoped.and_then(|headers| headers.get(DEFAULT_REGISTRY_SCOPE)))
                .cloned();
            ResolvedRegistry { name: name.clone(), url: url.clone(), auth_header }
        })
        .collect();

    let no_proxy = config.proxy.no_proxy.as_ref().map(|setting| match setting {
        NoProxySetting::Bypass => serde_json::Value::Bool(true),
        NoProxySetting::List(hosts) => serde_json::Value::String(hosts.join(",")),
    });

    ResolvedConfig {
        registries,
        auth_header_by_uri,
        http_proxy: config.proxy.http_proxy.clone(),
        https_proxy: config.proxy.https_proxy.clone(),
        no_proxy,
        ca: (!config.tls.ca.is_empty()).then(|| config.tls.ca.clone()),
        cert: config.tls.cert.clone(),
        key: config.tls.key.clone(),
        strict_ssl: config.tls.strict_ssl,
        store_dir: std::path::PathBuf::from(config.store_dir.clone()).display().to_string(),
        cache_dir: config.cache_dir.display().to_string(),
        virtual_store_dir_max_length: u32::try_from(config.virtual_store_dir_max_length)
            .unwrap_or(u32::MAX),
        enable_global_virtual_store: config.enable_global_virtual_store,
        global_virtual_store_dir: config.global_virtual_store_dir.display().to_string(),
        virtual_store_dir: config.virtual_store_dir.display().to_string(),
        effective_virtual_store_dir: config.effective_virtual_store_dir().display().to_string(),
        network_concurrency: u32::try_from(config.network_concurrency).unwrap_or(u32::MAX),
        max_sockets: config.max_sockets.map(|value| u32::try_from(value).unwrap_or(u32::MAX)),
        fetch_retries: config.fetch_retries,
        fetch_retry_factor: config.fetch_retry_factor,
        fetch_retry_mintimeout: u32::try_from(config.fetch_retry_mintimeout).unwrap_or(u32::MAX),
        fetch_retry_maxtimeout: u32::try_from(config.fetch_retry_maxtimeout).unwrap_or(u32::MAX),
        fetch_timeout: u32::try_from(config.fetch_timeout).unwrap_or(u32::MAX),
        fetch_warn_timeout_ms: u32::try_from(config.fetch_warn_timeout_ms).unwrap_or(u32::MAX),
        fetch_min_speed_ki_bps: u32::try_from(config.fetch_min_speed_ki_bps).unwrap_or(u32::MAX),
        user_agent: config
            .explicit_settings
            .contains_key("userAgent")
            .then(|| config.user_agent.clone()),
        engine_strict: config.engine_strict,
        node_version: config.node_version.clone(),
        package_import_method: import_method_name(config.package_import_method).to_string(),
        hoist_pattern: config.hoist_pattern.clone(),
        public_hoist_pattern: config.public_hoist_pattern.clone(),
        shamefully_hoist: config.shamefully_hoist,
        pnpm_home_dir: pnpm_config::default_pnpm_home_dir::<pnpm_config::Host>()
            .map(|dir| dir.display().to_string()),
        explicit_settings: config.explicit_settings.keys().cloned().collect(),
    }
}

fn import_method_name(method: pnpm_config::PackageImportMethod) -> &'static str {
    match method {
        pnpm_config::PackageImportMethod::Auto => "auto",
        pnpm_config::PackageImportMethod::Hardlink => "hardlink",
        pnpm_config::PackageImportMethod::Copy => "copy",
        pnpm_config::PackageImportMethod::Clone => "clone",
        pnpm_config::PackageImportMethod::CloneOrCopy => "clone-or-copy",
    }
}

#[cfg(test)]
mod tests;
