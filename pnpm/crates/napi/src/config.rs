//! Build and intern a `&'static Config` from a base directory plus a caller
//! overlay.
//!
//! pacquet's install pipeline holds `&'static Config` (obtained via
//! [`Config::leak`], a one-way conversion). A long-lived Node process that
//! installs repeatedly would leak a `Config` per call, so resolved configs are
//! interned in a process-global map keyed by a hash of `(dir, overlay,
//! config sources)`: the same inputs return the same leaked reference instead
//! of allocating a new one, but changed `.npmrc` / `pnpm-workspace.yaml` /
//! environment policy builds a fresh config.
//!
//! Each *distinct* input still leaks once and is never evicted (the map exists
//! to stop *repeated identical* calls from leaking, not to bound total memory —
//! leaked memory cannot be reclaimed). Retained configs therefore grow with the
//! number of unique `(dir, overlay, config sources)` combinations the process
//! observes, which is bounded in practice for the trusted embedder this binding
//! targets. Removing the leak entirely requires the engine to accept a borrowed
//! or `Arc` config instead of `&'static Config`; that is a pacquet-core change
//! tracked as a follow-up.
//!
//! The base is [`Config::current`] over `dir` — it reads the `.npmrc`
//! auth/registry/network subset and `pnpm-workspace.yaml` exactly as the CLI
//! does — then the explicit overlay fields the host passed (store/cache dirs,
//! registries, linker, hoist patterns, overrides, peer/dedupe policy, ...) win.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, hash_map::DefaultHasher},
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use dashmap::DashMap;
use indexmap::IndexMap;
use pnpm_config::{
    Config, GetHomeDir, Host, LinkWorkspacePackages, LoadWorkspaceYamlError, NodeLinker,
    PackageExtension, PackageImportMethod, default_registry,
};
use pnpm_network::{AuthHeaders, ProxyConfig, TlsConfig, nerf_dart, normalize_auth_key};
use pnpm_store_dir::StoreDir;

/// Host-supplied config values. Every field is optional: `None` keeps the
/// value [`Config::current`] resolved from `.npmrc` / `pnpm-workspace.yaml` /
/// defaults.
#[derive(Debug, Default)]
pub struct ConfigOverlay {
    pub store_dir: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
    pub registry: Option<String>,
    /// `{ "default": url, "@scope": url, ... }` — merged over the resolved map.
    pub registries: Option<BTreeMap<String, String>>,
    pub proxy: Option<ProxyConfig>,
    pub tls: Option<TlsConfig>,
    /// `pnpmHomeDir` — the home directory the default store location is
    /// resolved under when no config source sets `storeDir` (mirrors the
    /// `pnpmHomeDir` input of pnpm's `getStorePath`). An explicit
    /// [`Self::store_dir`] or a cascade-configured `storeDir` wins.
    pub pnpm_home_dir: Option<PathBuf>,
    pub node_linker: Option<NodeLinker>,
    /// `linkWorkspacePackages` — whether a bare-semver dependency may resolve
    /// to a workspace package by name. `Off` (the default) matches only
    /// `workspace:`-prefixed ranges.
    pub link_workspace_packages: Option<LinkWorkspacePackages>,
    /// `virtualStoreOnly` — populate the virtual store but perform no
    /// post-import linking (importer symlinks, `.bin` entries, hoisting,
    /// project lifecycle scripts). The binding sets it for
    /// `ignorePackageManifest` installs — pnpm `fetch` semantics.
    pub virtual_store_only: Option<bool>,
    /// `enableModulesDir` — pnpm's setting for suppressing the
    /// `node_modules` directory. The binding forces it on for
    /// `ignorePackageManifest` installs, which need `node_modules/.pnpm`
    /// even when an ambient config source disables the modules dir.
    pub enable_modules_dir: Option<bool>,
    pub package_import_method: Option<PackageImportMethod>,
    pub virtual_store_dir_max_length: Option<u64>,
    pub enable_global_virtual_store: Option<bool>,
    pub global_virtual_store_dir: Option<PathBuf>,
    pub package_extensions: Option<IndexMap<String, PackageExtension>>,
    pub patched_dependencies: Option<IndexMap<String, String>>,
    /// `allowUnusedPatches` — when `true`, a configured patch that matches no
    /// installed package warns instead of failing with
    /// `ERR_PNPM_UNUSED_PATCH`.
    pub allow_unused_patches: Option<bool>,
    pub hoist_pattern: Option<Vec<String>>,
    pub public_hoist_pattern: Option<Vec<String>>,
    pub external_dependencies: Option<BTreeSet<String>>,
    pub overrides: Option<IndexMap<String, String>>,
    pub auto_install_peers: Option<bool>,
    pub exclude_links_from_lockfile: Option<bool>,
    pub hoist_workspace_packages: Option<bool>,
    pub inject_workspace_packages: Option<bool>,
    pub prefer_offline: Option<bool>,
    pub offline: Option<bool>,
    pub lockfile: Option<bool>,
    pub prefer_frozen_lockfile: Option<bool>,
    pub dedupe_peer_dependents: Option<bool>,
    pub dedupe_peers: Option<bool>,
    pub dedupe_direct_deps: Option<bool>,
    pub dedupe_injected_deps: Option<bool>,
    pub resolve_peers_from_workspace_root: Option<bool>,
    pub peers_suffix_max_length: Option<u64>,
    pub network_concurrency: Option<usize>,
    /// `maxSockets` — per-origin concurrent-connection cap. Threaded onto the
    /// install client via `ThrottledClient::with_max_sockets_per_host`.
    pub max_sockets: Option<usize>,
    pub fetch_retries: Option<u32>,
    pub fetch_retry_factor: Option<u32>,
    pub fetch_retry_mintimeout: Option<u64>,
    pub fetch_retry_maxtimeout: Option<u64>,
    pub fetch_timeout: Option<u64>,
    /// Slow metadata-request threshold in milliseconds. [`None`] keeps the
    /// value resolved by [`Config::current`].
    pub fetch_warn_timeout_ms: Option<u64>,
    /// Minimum average tarball speed in KiB/s. [`None`] keeps the value
    /// resolved by [`Config::current`].
    pub fetch_min_speed_ki_bps: Option<u64>,
    pub user_agent: Option<String>,
    /// When `false` (the embedder default), an install that blocks dependency
    /// build scripts reports them via `depsRequiringBuild` instead of failing
    /// with `ERR_PNPM_IGNORED_BUILDS`.
    pub strict_dep_builds: Option<bool>,
    /// Per-package build-script allow-list: `name -> allowed`. A package must
    /// be `true` here (or covered by `dangerously_allow_all_builds`) for its
    /// lifecycle scripts to run. `BTreeMap` (not `HashMap`) so the overlay's
    /// `Debug` output — which feeds the config intern cache key — is stable.
    pub allow_builds: Option<BTreeMap<String, bool>>,
    /// Allow every dependency's build scripts to run.
    pub dangerously_allow_all_builds: Option<bool>,
    /// When `true`, skip all dependency and project lifecycle scripts.
    pub ignore_scripts: Option<bool>,
    /// When `true`, trust lockfile resolutions without verifying them against
    /// current registry metadata.
    pub trust_lockfile: Option<bool>,
    /// `engineStrict` — fail the install when a dependency's `engines` /
    /// platform constraint the host does not satisfy is required.
    pub engine_strict: Option<bool>,
    /// `nodeVersion` — overrides the Node.js version the installability check
    /// uses as the `engines.node` target. `None` auto-detects from `node`.
    pub node_version: Option<String>,
    pub minimum_release_age: Option<u64>,
    pub minimum_release_age_exclude: Option<Vec<String>>,
    /// `peerDependencyRules` — customizations for how peer-dependency
    /// mismatches are treated during resolution.
    pub peer_dependency_rules: Option<PeerDependencyRulesOverlay>,
    /// Pre-computed `Authorization` header values keyed by nerf-darted registry
    /// URI (`//host[:port]/path/`), plus the empty string `""` for the default
    /// registry. When present, replaces the `.npmrc`-derived `auth_headers` —
    /// the host (which owns the raw `.npmrc`/config credentials) resolves the
    /// `Bearer ...` / `Basic ...` values and passes them in, so the binding never
    /// reparses npmrc auth. `BTreeMap` (not `HashMap`) so the overlay's `Debug`
    /// output — which feeds the config intern cache key — is stable.
    pub auth_header_by_uri: Option<BTreeMap<String, String>>,
}

/// Host-supplied `peerDependencyRules`. Mirrors pnpm's shape and pacquet's
/// [`pnpm_config::Config::peer_dependency_rules`] fields.
#[derive(Debug, Default)]
pub struct PeerDependencyRulesOverlay {
    pub ignore_missing: Option<Vec<String>>,
    pub allow_any: Option<Vec<String>>,
    pub allowed_versions: Option<BTreeMap<String, String>>,
}

/// Process-global intern table of leaked configs, keyed by the hash of
/// `(dir, overlay, config source contents)`.
fn config_cache() -> &'static DashMap<u64, &'static Config> {
    static CACHE: OnceLock<DashMap<u64, &'static Config>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn cache_key(dir: &Path, overlay: &ConfigOverlay) -> u64 {
    let mut hasher = DefaultHasher::new();
    dir.hash(&mut hasher);
    // The overlay's `Debug` string covers every field. This is only stable
    // because the map-typed fields are `BTreeMap` (ordered) rather than
    // `HashMap` (per-instance random iteration order) — otherwise logically
    // identical overlays would hash differently, miss the cache, and leak a
    // fresh `Config` on every call.
    format!("{overlay:?}").hash(&mut hasher);
    hash_config_sources(dir, &mut hasher);
    hasher.finish()
}

fn hash_config_sources(dir: &Path, hasher: &mut DefaultHasher) {
    hash_file(&dir.join(".npmrc"), hasher);

    let workspace_dir = std::env::var_os("NPM_CONFIG_WORKSPACE_DIR")
        .or_else(|| std::env::var_os("npm_config_workspace_dir"))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| pnpm_workspace::find_workspace_dir(dir).ok().flatten());
    if let Some(workspace_dir) = workspace_dir {
        hash_file(&workspace_dir.join(pnpm_config::WORKSPACE_MANIFEST_FILENAME), hasher);
        hash_file(&workspace_dir.join(".npmrc"), hasher);
    }

    if let Some(config_dir) = pnpm_config::default_config_dir::<Host>() {
        hash_file(&config_dir.join(pnpm_config::GLOBAL_CONFIG_YAML_FILENAME), hasher);
        hash_file(&config_dir.join("auth.ini"), hasher);
    }
    if let Some(home_dir) = Host::home_dir() {
        hash_file(&home_dir.join(".npmrc"), hasher);
    }
    for name in [
        "PNPM_CONFIG_NPMRC_AUTH_FILE",
        "pnpm_config_npmrc_auth_file",
        "PNPM_CONFIG_USERCONFIG",
        "pnpm_config_userconfig",
        "NPM_CONFIG_USERCONFIG",
        "npm_config_userconfig",
    ] {
        if let Some(path) = std::env::var_os(name).filter(|value| !value.is_empty()) {
            hash_file(&PathBuf::from(path), hasher);
        }
    }

    let mut env_vars: Vec<(String, String)> = std::env::vars_os()
        .filter_map(|(name, value)| {
            let name = name.into_string().ok()?;
            is_config_env_name(&name).then(|| (name, value.into_string().unwrap_or_default()))
        })
        .collect();
    env_vars.sort();
    env_vars.hash(hasher);
}

fn hash_file(path: &Path, hasher: &mut DefaultHasher) {
    path.hash(hasher);
    match fs::read(path) {
        Ok(contents) => {
            true.hash(hasher);
            contents.hash(hasher);
        }
        Err(error) => {
            false.hash(hasher);
            format!("{:?}", error.kind()).hash(hasher);
        }
    }
}

fn is_config_env_name(name: &str) -> bool {
    name.starts_with("PNPM_CONFIG_")
        || name.starts_with("pnpm_config_")
        || name.starts_with("NPM_CONFIG_")
        || name.starts_with("npm_config_")
        || matches!(
            name,
            "HTTPS_PROXY"
                | "https_proxy"
                | "HTTP_PROXY"
                | "http_proxy"
                | "NO_PROXY"
                | "no_proxy"
                | "NODE_EXTRA_CA_CERTS",
        )
}

/// Resolve `(dir, overlay)` into an interned `&'static Config`.
pub fn resolve_config(
    dir: &Path,
    overlay: &ConfigOverlay,
) -> Result<&'static Config, LoadWorkspaceYamlError> {
    let key = cache_key(dir, overlay);
    if let Some(config) = config_cache().get(&key) {
        return Ok(*config);
    }
    let config = build_config(dir, overlay)?;
    let leaked: &'static Config = config.leak();
    config_cache().insert(key, leaked);
    Ok(leaked)
}

fn build_config(dir: &Path, overlay: &ConfigOverlay) -> Result<Config, LoadWorkspaceYamlError> {
    let mut config = Config::default().current::<Host>(dir)?;
    if let Some(store_dir) = &overlay.store_dir {
        config.store_dir = StoreDir::new(store_dir.clone());
    } else if let Some(pnpm_home_dir) = &overlay.pnpm_home_dir
        && !config.explicit_settings.contains_key("storeDir")
    {
        config.resolve_store_dir_from_home::<Host>(pnpm_home_dir, dir);
    }
    if let Some(cache_dir) = &overlay.cache_dir {
        config.cache_dir.clone_from(cache_dir);
    }
    if let Some(registry) = &overlay.registry {
        config.registry.clone_from(registry);
        config.registries_by_scope.insert("default".to_string(), registry.clone());
    }
    if let Some(registries) = &overlay.registries {
        for (scope, url) in registries {
            config.registries_by_scope.insert(scope.clone(), url.clone());
            if scope == "default" {
                config.registry.clone_from(url);
            }
        }
    }
    if let Some(proxy) = &overlay.proxy {
        config.proxy.clone_from(proxy);
    }
    if let Some(tls) = &overlay.tls {
        config.tls.clone_from(tls);
    }
    if let Some(node_linker) = overlay.node_linker {
        config.node_linker = node_linker;
    }
    if let Some(link_workspace_packages) = overlay.link_workspace_packages {
        config.link_workspace_packages = link_workspace_packages;
    }
    if let Some(value) = overlay.virtual_store_only {
        config.virtual_store_only = value;
    }
    if let Some(value) = overlay.enable_modules_dir {
        config.enable_modules_dir = value;
    }
    if let Some(method) = overlay.package_import_method {
        config.package_import_method = method;
    }
    if let Some(max_length) = overlay.virtual_store_dir_max_length {
        config.virtual_store_dir_max_length = max_length;
    }
    if let Some(value) = overlay.enable_global_virtual_store {
        config.enable_global_virtual_store = value;
    }
    if let Some(package_extensions) = &overlay.package_extensions {
        config.package_extensions = Some(package_extensions.clone());
    }
    if let Some(patched_dependencies) = &overlay.patched_dependencies {
        // Embedded installs resolve relative patch paths from `dir`, even without a workspace file.
        config.patched_dependencies = Some(
            patched_dependencies
                .iter()
                .map(|(key, path)| (key.clone(), dir.join(path).display().to_string()))
                .collect(),
        );
        if config.workspace_dir.is_none() {
            config.workspace_dir = Some(dir.to_path_buf());
        }
    }
    if let Some(value) = overlay.allow_unused_patches {
        config.allow_unused_patches = value;
    }
    if let Some(hoist_pattern) = &overlay.hoist_pattern {
        config.hoist_pattern = Some(hoist_pattern.clone());
    }
    if let Some(public_hoist_pattern) = &overlay.public_hoist_pattern {
        config.public_hoist_pattern = Some(public_hoist_pattern.clone());
    }
    if let Some(external_dependencies) = &overlay.external_dependencies {
        config.external_dependencies.clone_from(external_dependencies);
    }
    if let Some(overrides) = &overlay.overrides {
        config.overrides = Some(overrides.clone());
    }
    if let Some(value) = overlay.auto_install_peers {
        config.auto_install_peers = value;
    }
    if let Some(value) = overlay.exclude_links_from_lockfile {
        config.exclude_links_from_lockfile = value;
    }
    if let Some(value) = overlay.hoist_workspace_packages {
        config.hoist_workspace_packages = value;
    }
    if let Some(value) = overlay.inject_workspace_packages {
        config.inject_workspace_packages = value;
    }
    if let Some(value) = overlay.prefer_offline {
        config.prefer_offline = value;
    }
    if let Some(value) = overlay.offline {
        config.offline = value;
    }
    if let Some(value) = overlay.lockfile {
        config.lockfile = value;
    }
    if let Some(value) = overlay.prefer_frozen_lockfile {
        config.prefer_frozen_lockfile = value;
    }
    if let Some(value) = overlay.dedupe_peer_dependents {
        config.dedupe_peer_dependents = value;
    }
    if let Some(value) = overlay.dedupe_peers {
        config.dedupe_peers = value;
    }
    if let Some(value) = overlay.dedupe_direct_deps {
        config.dedupe_direct_deps = value;
    }
    if let Some(value) = overlay.dedupe_injected_deps {
        config.dedupe_injected_deps = value;
    }
    if let Some(value) = overlay.resolve_peers_from_workspace_root {
        config.resolve_peers_from_workspace_root = value;
    }
    if let Some(value) = overlay.peers_suffix_max_length {
        config.peers_suffix_max_length = value;
    }
    if let Some(value) = overlay.network_concurrency {
        config.network_concurrency = value;
    }
    if let Some(value) = overlay.max_sockets {
        config.max_sockets = Some(value);
    }
    if let Some(value) = overlay.fetch_retries {
        config.fetch_retries = value;
    }
    if let Some(value) = overlay.fetch_retry_factor {
        config.fetch_retry_factor = value;
    }
    if let Some(value) = overlay.fetch_retry_mintimeout {
        config.fetch_retry_mintimeout = value;
    }
    if let Some(value) = overlay.fetch_retry_maxtimeout {
        config.fetch_retry_maxtimeout = value;
    }
    if let Some(value) = overlay.fetch_timeout {
        config.fetch_timeout = value;
    }
    if let Some(value) = overlay.fetch_warn_timeout_ms {
        config.fetch_warn_timeout_ms = value;
    }
    if let Some(value) = overlay.fetch_min_speed_ki_bps {
        config.fetch_min_speed_ki_bps = value;
    }
    if let Some(user_agent) = &overlay.user_agent {
        config.user_agent.clone_from(user_agent);
    }
    if let Some(value) = overlay.strict_dep_builds {
        config.strict_dep_builds = value;
    }
    if let Some(allow_builds) = &overlay.allow_builds {
        config.allow_builds =
            allow_builds.iter().map(|(name, allowed)| (name.clone(), *allowed)).collect();
    }
    if let Some(value) = overlay.dangerously_allow_all_builds {
        config.dangerously_allow_all_builds = value;
    }
    if let Some(value) = overlay.ignore_scripts {
        config.ignore_scripts = value;
    }
    if let Some(value) = overlay.trust_lockfile {
        config.trust_lockfile = value;
    }
    if let Some(value) = overlay.engine_strict {
        config.engine_strict = value;
    }
    if let Some(node_version) = &overlay.node_version {
        config.node_version = Some(node_version.clone());
    }
    if let Some(value) = overlay.minimum_release_age {
        config.minimum_release_age = Some(value);
    }
    if let Some(value) = &overlay.minimum_release_age_exclude {
        config.minimum_release_age_exclude = Some(value.clone());
    }
    if let Some(rules) = &overlay.peer_dependency_rules {
        if let Some(ignore_missing) = &rules.ignore_missing {
            config.peer_dependency_rules.ignore_missing = Some(ignore_missing.clone());
        }
        if let Some(allow_any) = &rules.allow_any {
            config.peer_dependency_rules.allow_any = Some(allow_any.clone());
        }
        if let Some(allowed_versions) = &rules.allowed_versions {
            config.peer_dependency_rules.allowed_versions = Some(allowed_versions.clone());
        }
    }
    if let Some(headers) = &overlay.auth_header_by_uri {
        config.auth_headers = std::sync::Arc::new(AuthHeaders::from_map(pin_unkeyed_header(
            headers,
            &overlay_default_registry(overlay),
        )));
    }
    // An overlay hoist pattern must not undo the empty-pattern derivation a
    // `virtualStoreOnly` install records in `.modules.yaml`, so re-derive
    // after every pattern-touching field above has been applied.
    config.apply_virtual_store_only_derivation();
    // Overlay fields may invalidate the path derived by `Config::current`.
    if let Some(global_virtual_store_dir) = &overlay.global_virtual_store_dir {
        config.global_virtual_store_dir.clone_from(global_virtual_store_dir);
    } else if overlay.enable_global_virtual_store.is_some()
        || overlay.store_dir.is_some()
        || overlay.pnpm_home_dir.is_some()
    {
        let virtual_store_dir_explicit = config.explicit_settings.contains_key("virtualStoreDir");
        let global_virtual_store_dir_explicit =
            config.explicit_settings.contains_key("globalVirtualStoreDir");
        config.apply_global_virtual_store_derivation(
            virtual_store_dir_explicit,
            global_virtual_store_dir_explicit,
        );
    }
    Ok(config)
}

/// Key the overlay's unkeyed (`""`) `Authorization` header — the host's
/// default-registry credential — at the registry that same overlay declared,
/// or at the npmjs default when it declared none. This mirrors the pinning
/// `.npmrc` credentials get in `NpmrcAuth::rescope_unscoped`: the credential
/// and the registry it is sent to both come from the host, so a `registry=`
/// in the repository's `.npmrc` cannot redirect it. A header the host already
/// keyed at that URI wins, and an unparsable default registry drops the
/// unkeyed header rather than sending it somewhere unintended.
fn pin_unkeyed_header(
    headers: &BTreeMap<String, String>,
    default_registry: &str,
) -> HashMap<String, String> {
    let mut by_uri: HashMap<String, String> = HashMap::new();
    let mut unkeyed = None;
    for (uri, header) in headers {
        if uri.is_empty() {
            unkeyed = Some(header);
        } else {
            // Normalized on the way in, so a host key spelled without the
            // trailing slash still counts as "already keyed at that URI"
            // below instead of colliding with the pinned entry later.
            by_uri.insert(normalize_auth_key(uri.clone()), header.clone());
        }
    }
    let default_uri = nerf_dart(default_registry);
    if let Some(header) = unkeyed
        && !default_uri.is_empty()
    {
        by_uri.entry(default_uri).or_insert_with(|| header.clone());
    }
    by_uri
}

fn overlay_default_registry(overlay: &ConfigOverlay) -> String {
    overlay
        .registries
        .as_ref()
        .and_then(|registries| registries.get("default"))
        .or(overlay.registry.as_ref())
        .cloned()
        .unwrap_or_else(default_registry)
}

#[cfg(test)]
mod tests;
