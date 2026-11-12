use super::{
    HashMap, IndexMap, LazyLock, Mutex, PacquetConfig, Path, PathBuf, ResolveRequest, StoreDir,
};

use std::{sync::Arc, time::Instant};

pub(super) const MAX_CACHED_CONFIGS: usize = 1024;
pub(super) const CONFIG_KEY_TOO_LARGE_MESSAGE: &str = "registry configuration is too large";

/// Bound the memory retained for each cached request configuration.
pub(super) const MAX_CONFIG_KEY_BYTES: usize = 128 * 1024;

pub(super) struct CachedConfig {
    config: Arc<PacquetConfig>,
    last_used: Instant,
}

/// The settings a request resolves under, and the only part of an input
/// lockfile's `settings` block the interning key carries. Keying on the whole
/// block would let a caller mint an unbounded number of distinct configs out
/// of the fields the config never reads (`peersSuffixMaxLength` alone is a
/// `u64`).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct EffectiveResolverSettings {
    pub(super) auto_install_peers: bool,
    pub(super) dedupe_peers: bool,
    pub(super) exclude_links_from_lockfile: bool,
    pub(super) add_missing_peer_types: bool,
    pub(super) project_scoped_optional_peers: bool,
}

impl EffectiveResolverSettings {
    /// The client's own values whenever it sends them. A client that sends
    /// none (one older than
    /// [pnpm/pnpm#13389](https://github.com/pnpm/pnpm/issues/13389)) falls
    /// back to the input lockfile on a frozen request — nothing is
    /// re-resolved there, the freshness gate compares these against the
    /// config, and the server's defaults would call a lockfile that is valid
    /// for its owner stale. On an update-capable request it falls back to the
    /// server's defaults instead: the lockfile records what the *last* install
    /// used, which is stale exactly when the client has just changed one of
    /// these.
    pub(super) fn for_request(request: &ResolveRequest) -> Self {
        static DEFAULTS: LazyLock<PacquetConfig> = LazyLock::new(PacquetConfig::new);

        let lockfile_settings = request.frozen_lockfile
            .then(|| request.lockfile.as_ref()?.settings.as_ref())
            .flatten();

        EffectiveResolverSettings {
            auto_install_peers: request.auto_install_peers
                .or_else(|| lockfile_settings.map(|settings| settings.auto_install_peers))
                .unwrap_or(DEFAULTS.auto_install_peers),
            dedupe_peers: request.dedupe_peers
                .or_else(|| lockfile_settings.and_then(|settings| settings.dedupe_peers))
                .unwrap_or(DEFAULTS.dedupe_peers),
            exclude_links_from_lockfile: request.exclude_links_from_lockfile
                .or_else(|| lockfile_settings.map(|settings| settings.exclude_links_from_lockfile))
                .unwrap_or(DEFAULTS.exclude_links_from_lockfile),
            add_missing_peer_types: request.add_missing_peer_types
                .or_else(|| lockfile_settings.and_then(|settings| settings.add_missing_peer_types))
                .unwrap_or(DEFAULTS.add_missing_peer_types),
            project_scoped_optional_peers: request.project_scoped_optional_peers
                .or_else(|| {
                    lockfile_settings.and_then(|settings| settings.project_scoped_optional_peers)
                })
                .unwrap_or(DEFAULTS.project_scoped_optional_peers),
        }
    }
}

/// Reuse a request configuration, evicting the least recently used entry at capacity.
/// Active requests retain their own reference across eviction.
pub(super) fn intern_config(
    configs: &Mutex<HashMap<String, CachedConfig>>,
    store_dir: &StoreDir,
    cache_dir: &Path,
    request: &ResolveRequest,
    max_cached: usize,
    max_key_bytes: usize,
) -> Option<Arc<PacquetConfig>> {
    let registry =
        request.registry.clone().unwrap_or_else(|| "https://registry.npmjs.org/".to_string());
    let registry = if registry.ends_with('/') { registry } else { format!("{registry}/") };
    let overrides: Option<IndexMap<String, String>> = request.overrides
        .as_ref()
        .and_then(|value| serde_json::from_value(value.clone()).ok());
    let resolver_settings = EffectiveResolverSettings::for_request(request);
    let key = config_cache_key(request, &registry, overrides.as_ref(), &resolver_settings);
    if key.len() > max_key_bytes {
        return None;
    }

    let mut configs = configs.lock().expect("config cache poisoned");
    if let Some(config) = configs.get_mut(&key) {
        config.last_used = Instant::now();
        return Some(Arc::clone(&config.config));
    }
    if configs.len() >= max_cached
        && let Some(oldest) = configs
            .iter()
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(key, _)| key.clone())
    {
        configs.remove(&oldest);
    }

    let config =
        build_config(store_dir, cache_dir, request, registry, overrides, &resolver_settings);
    let config = Arc::new(config);
    if max_cached > 0 {
        configs.insert(
            key,
            CachedConfig { config: Arc::clone(&config), last_used: Instant::now() },
        );
    }
    Some(config)
}

fn build_config(
    store_dir: &StoreDir,
    cache_dir: &Path,
    request: &ResolveRequest,
    registry: String,
    overrides: Option<IndexMap<String, String>>,
    resolver_settings: &EffectiveResolverSettings,
) -> PacquetConfig {
    let mut config = PacquetConfig::new();
    config.store_dir = store_dir.clone();
    config.cache_dir = cache_dir.to_path_buf();
    config.registry = registry;
    apply_registry_declarations(&mut config, request);
    config.overrides = overrides;
    config.patched_dependency_hashes_override.clone_from(&request.patched_dependencies);
    config.package_extensions.clone_from(&request.package_extensions);
    config.allow_unused_patches = request.allow_unused_patches;
    config.modules_dir = PathBuf::from("node_modules");
    config.lockfile = true;
    config.verify_store_integrity = true;
    apply_request_policy(&mut config, request);
    config.auto_install_peers = resolver_settings.auto_install_peers;
    config.dedupe_peers = resolver_settings.dedupe_peers;
    config.exclude_links_from_lockfile = resolver_settings.exclude_links_from_lockfile;
    config.add_missing_peer_types = resolver_settings.add_missing_peer_types;
    config.project_scoped_optional_peers = resolver_settings.project_scoped_optional_peers;
    config
}

/// Key on a sorted view of `overrides`: `serde_json` preserves insertion order
/// and `IndexMap` is insertion-ordered, so the same overrides sent with a
/// different key order would otherwise hash to distinct cache keys and intern
/// duplicate configurations.
pub(super) fn config_cache_key(
    request: &ResolveRequest,
    registry: &str,
    overrides: Option<&IndexMap<String, String>>,
    resolver_settings: &EffectiveResolverSettings,
) -> String {
    let overrides_key: Option<std::collections::BTreeMap<&str, &str>> =
        overrides.map(|overrides| {
            overrides
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect()
        });

    serde_json::json!({
        "registry": registry,
        "resolverSettings": resolver_settings,
        "registries": &request.registries,
        "overrides": overrides_key,
        "patchedDependencies": &request.patched_dependencies,
        "packageExtensions": &request.package_extensions,
        "allowUnusedPatches": request.allow_unused_patches,
        "resolutionMode": request.resolution_mode,
        "minimumReleaseAge": request.minimum_release_age,
        "minimumReleaseAgeExclude": request.minimum_release_age_exclude,
        "minimumReleaseAgeIgnoreMissingTime": request.minimum_release_age_ignore_missing_time,
        "trustPolicy": request.trust_policy,
        "trustPolicyExclude": request.trust_policy_exclude,
        "trustPolicyIgnoreAfter": request.trust_policy_ignore_after,
    })
    .to_string()
}

/// Apply the same declaration inversion as the client config reader.
pub(super) fn apply_registry_declarations(config: &mut PacquetConfig, request: &ResolveRequest) {
    let lookups = pnpm_config::registries::declarations_into_lookups(request.registries.clone());
    if request.registry.is_none()
        && let Some(default_registry) = lookups.default_registry
    {
        config.registry = default_registry;
    }
    config.registries_by_scope = lookups.registries_by_scope;
    config.registries_by_prefix = lookups.registries_by_prefix;
    config.registry_options_by_url = lookups.registry_options_by_url;
}

/// Apply the client's policy to both reused-lockfile verification and pick-time checks.
pub(super) fn apply_request_policy(config: &mut PacquetConfig, request: &ResolveRequest) {
    config.resolution_mode = request.resolution_mode;
    config.minimum_release_age = request.minimum_release_age;
    config.minimum_release_age_exclude.clone_from(&request.minimum_release_age_exclude);
    if let Some(ignore_missing_time) = request.minimum_release_age_ignore_missing_time {
        config.minimum_release_age_ignore_missing_time = ignore_missing_time;
    }
    config.trust_policy = request.trust_policy;
    config.trust_policy_exclude.clone_from(&request.trust_policy_exclude);
    config.trust_policy_ignore_after = request.trust_policy_ignore_after;
}
