use super::{
    HashMap, IndexMap, LazyLock, Mutex, PacquetConfig, Path, PathBuf, ResolveRequest, StoreDir,
};

/// Hard cap on how many distinct client configurations the server will
/// intern. Each interned [`PacquetConfig`] is leaked (the install path
/// requires a `&'static Config`), so without a cap an authenticated
/// caller could exhaust memory by varying its registry/policy fields on
/// every request. `1024` is far above the handful of distinct setups a
/// real fleet produces (typically one), matching
/// [`cache::MAX_RESOLUTION_CACHE_ENTRIES`](super::cache::MAX_RESOLUTION_CACHE_ENTRIES).
pub(super) const MAX_INTERNED_CONFIGS: usize = 1024;

/// Returned (as a `503`) when [`MAX_INTERNED_CONFIGS`] is reached. The
/// limit resets on restart and a real client reuses one configuration, so
/// a legitimate caller never sees it.
pub(super) const TOO_MANY_CONFIGS_MESSAGE: &str = "too many distinct registry configurations";

/// Hard cap on the byte size of a single interned config's canonical key,
/// which carries its attacker-controlled `registry` / `namedRegistries` /
/// `overrides` content. [`MAX_INTERNED_CONFIGS`] bounds only the *count* of
/// leaked configs; without this a caller could pad each distinct config with
/// a giant overrides/package-extensions/registry map and still amplify the per-request
/// leak (the whole request body is allowed up to the publish-sized limit).
/// `128 KiB` is far above any real resolver configuration.
pub(super) const MAX_CONFIG_KEY_BYTES: usize = 128 * 1024;

/// The settings a request resolves under, and the only part of an input
/// lockfile's `settings` block the interning key carries. Keying on the whole
/// block would let a caller mint an unbounded number of distinct configs out
/// of the fields the config never reads (`peersSuffixMaxLength` alone is a
/// `u64`) and exhaust [`MAX_INTERNED_CONFIGS`], after which no caller gets a
/// config at all.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct EffectiveResolverSettings {
    pub(super) auto_install_peers: bool,
    pub(super) dedupe_peers: bool,
    pub(super) exclude_links_from_lockfile: bool,
}

impl EffectiveResolverSettings {
    /// The client's own values whenever it sends them. A client that sends
    /// none (one older than
    /// [pnpm/pnpm#13389](https://github.com/pnpm/pnpm/issues/13389)) falls
    /// back to the input lockfile on a frozen request — nothing is
    /// re-resolved there, the freshness gate compares these three against the
    /// config, and the server's defaults would call a lockfile that is valid
    /// for its owner stale. On an update-capable request it falls back to the
    /// server's defaults instead: the lockfile records what the *last* install
    /// used, which is stale exactly when the client has just changed one of
    /// these.
    pub(super) fn for_request(request: &ResolveRequest) -> Self {
        static DEFAULTS: LazyLock<PacquetConfig> = LazyLock::new(PacquetConfig::new);

        let lockfile_settings =
            request.frozen_lockfile.then(|| request.lockfile.as_ref()?.settings.as_ref()).flatten();

        EffectiveResolverSettings {
            auto_install_peers: request
                .auto_install_peers
                .or_else(|| lockfile_settings.map(|settings| settings.auto_install_peers))
                .unwrap_or(DEFAULTS.auto_install_peers),
            dedupe_peers: request
                .dedupe_peers
                .or_else(|| lockfile_settings.and_then(|settings| settings.dedupe_peers))
                .unwrap_or(DEFAULTS.dedupe_peers),
            exclude_links_from_lockfile: request
                .exclude_links_from_lockfile
                .or_else(|| lockfile_settings.map(|settings| settings.exclude_links_from_lockfile))
                .unwrap_or(DEFAULTS.exclude_links_from_lockfile),
        }
    }
}

/// Build + leak a `&'static Config` for a request's registry
/// configuration, interned by its canonical JSON so repeat requests reuse
/// it. Returns `None` when the config can't be safely interned:
///
/// * once `max_interned` distinct configurations have been interned — a
///   leaked config can never be reclaimed, so refusing to leak more is the
///   only real bound on the per-request leak (eviction would just let the
///   same key be re-leaked); or
/// * when a single config's canonical key exceeds `max_key_bytes`, which
///   bounds the *size* of each leaked config so a caller can't amplify the
///   leak with a giant `overrides`, `packageExtensions`, or registry map.
///
/// Both caps are generous enough that legitimate clients (which reuse one
/// small configuration) never hit them.
pub(super) fn intern_config(
    configs: &Mutex<HashMap<String, &'static PacquetConfig>>,
    store_dir: &StoreDir,
    cache_dir: &Path,
    request: &ResolveRequest,
    max_interned: usize,
    max_key_bytes: usize,
) -> Option<&'static PacquetConfig> {
    let registry =
        request.registry.clone().unwrap_or_else(|| "https://registry.npmjs.org/".to_string());
    let registry = if registry.ends_with('/') { registry } else { format!("{registry}/") };
    let overrides: Option<IndexMap<String, String>> =
        request.overrides.as_ref().and_then(|value| serde_json::from_value(value.clone()).ok());
    let resolver_settings = EffectiveResolverSettings::for_request(request);
    let key = config_cache_key(request, &registry, overrides.as_ref(), &resolver_settings);
    if key.len() > max_key_bytes {
        return None;
    }

    let mut configs = configs.lock().expect("config cache poisoned");
    if let Some(config) = configs.get(&key) {
        return Some(config);
    }
    if configs.len() >= max_interned {
        return None;
    }

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
    let config: &'static PacquetConfig = config.leak();
    configs.insert(key, config);
    Some(config)
}

/// Key on a sorted view of `overrides`: `serde_json` preserves insertion order
/// and `IndexMap` is insertion-ordered, so the same overrides sent with a
/// different key order would otherwise hash to distinct cache keys and intern
/// duplicate leaked configs — defeating dedup and burning the cap faster.
pub(super) fn config_cache_key(
    request: &ResolveRequest,
    registry: &str,
    overrides: Option<&IndexMap<String, String>>,
    resolver_settings: &EffectiveResolverSettings,
) -> String {
    let overrides_key: Option<std::collections::BTreeMap<&str, &str>> = overrides
        .map(|overrides| overrides.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect());

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
