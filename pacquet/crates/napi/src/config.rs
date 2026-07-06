//! Build and intern a `&'static Config` from a base directory plus a caller
//! overlay.
//!
//! pacquet's install pipeline holds `&'static Config` (obtained via
//! [`Config::leak`], a one-way conversion). A long-lived Node process that
//! installs repeatedly would leak a `Config` per call, so resolved configs are
//! interned in a process-global map keyed by a hash of `(dir, overlay)`: the
//! same inputs return the same leaked reference instead of allocating a new one.
//!
//! The base is [`Config::current`] over `dir` — it reads the `.npmrc`
//! auth/registry/network subset and `pnpm-workspace.yaml` exactly as the CLI
//! does — then the explicit overlay fields the host passed (store/cache dirs,
//! registries, linker, hoist patterns, overrides, peer/dedupe policy, ...) win.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use dashmap::DashMap;
use indexmap::IndexMap;
use pacquet_config::{Config, Host, LoadWorkspaceYamlError, NodeLinker, PackageImportMethod};
use pacquet_network::AuthHeaders;
use pacquet_store_dir::StoreDir;
use serde::Serialize;

/// Host-supplied config values. Every field is optional: `None` keeps the
/// value [`Config::current`] resolved from `.npmrc` / `pnpm-workspace.yaml` /
/// defaults. Serialized to form the interning key, so field order and contents
/// fully determine cache identity.
#[derive(Default, Serialize)]
pub struct ConfigOverlay {
    pub store_dir: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
    pub registry: Option<String>,
    /// `{ "default": url, "@scope": url, ... }` — merged over the resolved map.
    pub registries: Option<std::collections::BTreeMap<String, String>>,
    pub node_linker: Option<NodeLinker>,
    pub package_import_method: Option<PackageImportMethod>,
    pub virtual_store_dir_max_length: Option<u64>,
    pub hoist_pattern: Option<Vec<String>>,
    pub public_hoist_pattern: Option<Vec<String>>,
    pub overrides: Option<IndexMap<String, String>>,
    pub auto_install_peers: Option<bool>,
    pub prefer_offline: Option<bool>,
    pub offline: Option<bool>,
    pub lockfile: Option<bool>,
    pub prefer_frozen_lockfile: Option<bool>,
    pub dedupe_peer_dependents: Option<bool>,
    pub dedupe_direct_deps: Option<bool>,
    pub dedupe_injected_deps: Option<bool>,
    pub resolve_peers_from_workspace_root: Option<bool>,
    pub peers_suffix_max_length: Option<u64>,
    pub network_concurrency: Option<usize>,
    pub fetch_timeout: Option<u64>,
    pub user_agent: Option<String>,
    /// When `false` (the embedder default), an install that blocks dependency
    /// build scripts reports them via `depsRequiringBuild` instead of failing
    /// with `ERR_PNPM_IGNORED_BUILDS`.
    pub strict_dep_builds: Option<bool>,
    /// Per-package build-script allow-list: `name -> allowed`. A package must
    /// be `true` here (or covered by `dangerously_allow_all_builds`) for its
    /// lifecycle scripts to run.
    pub allow_builds: Option<std::collections::HashMap<String, bool>>,
    /// Allow every dependency's build scripts to run.
    pub dangerously_allow_all_builds: Option<bool>,
    /// `peerDependencyRules` — customizations for how peer-dependency
    /// mismatches are treated during resolution.
    pub peer_dependency_rules: Option<PeerDependencyRulesOverlay>,
    /// Pre-computed `Authorization` header values keyed by nerf-darted registry
    /// URI (`//host[:port]/path/`), plus the empty string `""` for the default
    /// registry. When present, replaces the `.npmrc`-derived `auth_headers` —
    /// the host (which owns the raw `.npmrc`/config credentials) resolves the
    /// `Bearer ...` / `Basic ...` values and passes them in, so the binding never
    /// reparses npmrc auth.
    pub auth_header_by_uri: Option<std::collections::HashMap<String, String>>,
}

/// Host-supplied `peerDependencyRules`. Mirrors pnpm's shape and pacquet's
/// [`pacquet_config::Config::peer_dependency_rules`] fields.
#[derive(Default, Serialize)]
pub struct PeerDependencyRulesOverlay {
    pub ignore_missing: Option<Vec<String>>,
    pub allow_any: Option<Vec<String>>,
    pub allowed_versions: Option<std::collections::BTreeMap<String, String>>,
}

/// Process-global intern table of leaked configs, keyed by the hash of
/// `(dir, overlay)`.
fn config_cache() -> &'static DashMap<u64, &'static Config> {
    static CACHE: OnceLock<DashMap<u64, &'static Config>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn cache_key(dir: &Path, overlay: &ConfigOverlay) -> u64 {
    let mut hasher = DefaultHasher::new();
    dir.hash(&mut hasher);
    // Serialization failure is impossible for this plain-data struct; fall back
    // to a constant so the key stays deterministic rather than panicking.
    serde_json::to_string(overlay).unwrap_or_default().hash(&mut hasher);
    hasher.finish()
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
    }
    if let Some(cache_dir) = &overlay.cache_dir {
        config.cache_dir.clone_from(cache_dir);
    }
    if let Some(registry) = &overlay.registry {
        config.registry.clone_from(registry);
        config.registries.insert("default".to_string(), registry.clone());
    }
    if let Some(registries) = &overlay.registries {
        for (scope, url) in registries {
            config.registries.insert(scope.clone(), url.clone());
            if scope == "default" {
                config.registry.clone_from(url);
            }
        }
    }
    if let Some(node_linker) = overlay.node_linker {
        config.node_linker = node_linker;
    }
    if let Some(method) = overlay.package_import_method {
        config.package_import_method = method;
    }
    if let Some(max_length) = overlay.virtual_store_dir_max_length {
        config.virtual_store_dir_max_length = max_length;
    }
    if let Some(hoist_pattern) = &overlay.hoist_pattern {
        config.hoist_pattern = Some(hoist_pattern.clone());
    }
    if let Some(public_hoist_pattern) = &overlay.public_hoist_pattern {
        config.public_hoist_pattern = Some(public_hoist_pattern.clone());
    }
    if let Some(overrides) = &overlay.overrides {
        config.overrides = Some(overrides.clone());
    }
    if let Some(value) = overlay.auto_install_peers {
        config.auto_install_peers = value;
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
    if let Some(value) = overlay.fetch_timeout {
        config.fetch_timeout = value;
    }
    if let Some(user_agent) = &overlay.user_agent {
        config.user_agent.clone_from(user_agent);
    }
    if let Some(value) = overlay.strict_dep_builds {
        config.strict_dep_builds = value;
    }
    if let Some(allow_builds) = &overlay.allow_builds {
        config.allow_builds.clone_from(allow_builds);
    }
    if let Some(value) = overlay.dangerously_allow_all_builds {
        config.dangerously_allow_all_builds = value;
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
        let auth_headers = AuthHeaders::from_creds_map(
            headers.iter().map(|(uri, header)| (uri.clone(), header.clone())),
            Some(config.registry.as_str()),
        );
        config.auth_headers = std::sync::Arc::new(auth_headers);
    }
    Ok(config)
}
