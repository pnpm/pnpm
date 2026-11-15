use super::{
    HashMap, IndexMap, LazyLock, Mutex, PacquetConfig, Path, PathBuf, ResolveRequest, StoreDir,
};

use std::{sync::Arc, time::Instant};

pub(super) const MAX_ACTIVE_CONFIGS: usize = 1024;

pub(super) struct ConfigCache {
    pub(super) entries: Mutex<HashMap<String, CachedConfig>>,
    pub(super) admission: Arc<tokio::sync::Semaphore>,
}

impl Default for ConfigCache {
    fn default() -> Self {
        Self {
            entries: Mutex::default(),
            admission: Arc::new(tokio::sync::Semaphore::new(MAX_ACTIVE_CONFIGS)),
        }
    }
}

/// Retains admission until verification or the detached resolution finishes.
pub(super) struct ActiveConfig {
    pub(super) config: Arc<PacquetConfig>,
    pub(super) _permit: tokio::sync::OwnedSemaphorePermit,
}

impl std::ops::Deref for ActiveConfig {
    type Target = PacquetConfig;

    fn deref(&self) -> &Self::Target {
        &self.config
    }
}

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
    let inputs = ConfigKeyInputs::for_request(request);
    let key = key_value(CONFIG_KEY, request, &inputs).to_string();
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

    let config = build_config(store_dir, cache_dir, request, inputs);
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
    inputs: ConfigKeyInputs,
) -> PacquetConfig {
    let ConfigKeyInputs {
        registry,
        overrides,
        resolver_settings,
    } = inputs;
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

/// The registry, overrides, and resolver settings a request resolves under,
/// normalized the way [`intern_config`] keys them.
pub(super) struct ConfigKeyInputs {
    pub(super) registry: String,
    pub(super) overrides: Option<IndexMap<String, String>>,
    pub(super) resolver_settings: EffectiveResolverSettings,
}

impl ConfigKeyInputs {
    pub(super) fn for_request(request: &ResolveRequest) -> Self {
        let registry =
            request.registry.clone().unwrap_or_else(|| "https://registry.npmjs.org/".to_string());
        let registry = if registry.ends_with('/') { registry } else { format!("{registry}/") };
        let overrides = request.overrides
            .as_ref()
            .and_then(|value| serde_json::from_value(value.clone()).ok());
        ConfigKeyInputs {
            registry,
            overrides,
            resolver_settings: EffectiveResolverSettings::for_request(request),
        }
    }
}

/// One part of a cache key: the request fields it covers, by their wire
/// names, and the value it contributes.
pub(super) struct KeyPart {
    pub(super) fields: &'static [&'static str],
    pub(super) value: fn(&ResolveRequest, &ConfigKeyInputs) -> serde_json::Value,
}

/// The parts of [`intern_config`]'s key. The resolution cache key carries
/// them too. A test checks that every request field is in a key or listed as
/// one neither key needs.
pub(super) const CONFIG_KEY: &[KeyPart] = &[
    KeyPart { fields: &["registry"], value: |_, inputs| serde_json::json!(inputs.registry) },
    KeyPart { fields: &["overrides"], value: |_, inputs| sorted_overrides(inputs) },
    KeyPart {
        fields: &[
            "autoInstallPeers",
            "dedupePeers",
            "excludeLinksFromLockfile",
            "addMissingPeerTypes",
            "projectScopedOptionalPeers",
        ],
        value: |_, inputs| serde_json::json!(inputs.resolver_settings),
    },
    KeyPart { fields: &["registries"], value: |request, _| serde_json::json!(request.registries) },
    KeyPart {
        fields: &["patchedDependencies"],
        value: |request, _| serde_json::json!(request.patched_dependencies),
    },
    KeyPart {
        fields: &["packageExtensions"],
        value: |request, _| serde_json::json!(request.package_extensions),
    },
    KeyPart {
        fields: &["allowUnusedPatches"],
        value: |request, _| serde_json::json!(request.allow_unused_patches),
    },
    KeyPart {
        fields: &["resolutionMode"],
        value: |request, _| serde_json::json!(request.resolution_mode),
    },
    KeyPart {
        fields: &["minimumReleaseAge"],
        value: |request, _| serde_json::json!(request.minimum_release_age),
    },
    KeyPart {
        fields: &["minimumReleaseAgeExclude"],
        value: |request, _| serde_json::json!(request.minimum_release_age_exclude),
    },
    KeyPart {
        fields: &["minimumReleaseAgeIgnoreMissingTime"],
        value: |request, _| serde_json::json!(request.minimum_release_age_ignore_missing_time),
    },
    KeyPart {
        fields: &["trustPolicy"],
        value: |request, _| serde_json::json!(request.trust_policy),
    },
    KeyPart {
        fields: &["trustPolicyExclude"],
        value: |request, _| serde_json::json!(request.trust_policy_exclude),
    },
    KeyPart {
        fields: &["trustPolicyIgnoreAfter"],
        value: |request, _| serde_json::json!(request.trust_policy_ignore_after),
    },
];

/// The values of `parts` for `request`, keyed by the fields each covers.
pub(super) fn key_value(
    parts: &[KeyPart],
    request: &ResolveRequest,
    inputs: &ConfigKeyInputs,
) -> serde_json::Value {
    parts
        .iter()
        .map(|part| (part.fields.join(","), (part.value)(request, inputs)))
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// Key on a sorted view of `overrides`: `serde_json` preserves insertion order
/// and `IndexMap` is insertion-ordered, so the same overrides sent with a
/// different key order would otherwise intern duplicate configurations.
fn sorted_overrides(inputs: &ConfigKeyInputs) -> serde_json::Value {
    let overrides: Option<std::collections::BTreeMap<&str, &str>> = inputs
        .overrides
        .as_ref()
        .map(|overrides| {
            overrides
                .iter()
                .map(|(selector, spec)| (selector.as_str(), spec.as_str()))
                .collect()
        });
    serde_json::json!(overrides)
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
