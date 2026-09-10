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

use dashmap::{DashMap, mapref::entry::Entry};
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
    Ok(intern_config(key, config))
}

fn intern_config(key: u64, config: Config) -> &'static Config {
    match config_cache().entry(key) {
        Entry::Occupied(entry) => entry.get(),
        Entry::Vacant(entry) => *entry.insert(config.leak()),
    }
}

fn build_config(dir: &Path, overlay: &ConfigOverlay) -> Result<Config, LoadWorkspaceYamlError> {
    let mut config = Config::default().current::<Host>(dir)?;
    apply_store_dirs(&mut config, overlay, dir);
    apply_registries(&mut config, overlay);
    apply_layout(&mut config, overlay);
    apply_manifest_rewrites(&mut config, overlay, dir);
    apply_install_flags(&mut config, overlay);
    apply_dedupe_settings(&mut config, overlay);
    apply_network_limits(&mut config, overlay);
    apply_fetch_tuning(&mut config, overlay);
    apply_build_policy(&mut config, overlay);
    apply_release_policy(&mut config, overlay);
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

#[cfg(test)]
mod tests;

mod overlay;
use overlay::{
    apply_build_policy, apply_dedupe_settings, apply_fetch_tuning, apply_install_flags,
    apply_layout, apply_manifest_rewrites, apply_network_limits, apply_registries,
    apply_release_policy, apply_store_dirs, overlay_default_registry, pin_unkeyed_header,
};
