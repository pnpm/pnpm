use super::{
    BTreeMap, Config, ConfigOverlay, HashMap, Host, Path, StoreDir, default_registry, nerf_dart,
    normalize_auth_key,
};

/// Store, home and cache directories.
pub(super) fn apply_store_dirs(config: &mut Config, overlay: &ConfigOverlay, dir: &Path) {
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
}

/// Registry endpoints and the transport settings used to reach them.
pub(super) fn apply_registries(config: &mut Config, overlay: &ConfigOverlay) {
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
}

/// How `node_modules` and the virtual store are laid out.
pub(super) fn apply_layout(config: &mut Config, overlay: &ConfigOverlay) {
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
}

/// The overlay's rewrites of what the manifests declare: extensions,
/// patches, hoisting patterns and overrides.
pub(super) fn apply_manifest_rewrites(config: &mut Config, overlay: &ConfigOverlay, dir: &Path) {
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
}

/// Flags that steer what an install resolves and writes.
pub(super) fn apply_install_flags(config: &mut Config, overlay: &ConfigOverlay) {
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
}

/// Deduplication and peer-resolution settings.
pub(super) fn apply_dedupe_settings(config: &mut Config, overlay: &ConfigOverlay) {
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
}

/// How many connections the fetcher may open.
pub(super) fn apply_network_limits(config: &mut Config, overlay: &ConfigOverlay) {
    if let Some(value) = overlay.network_concurrency {
        config.network_concurrency = value;
    }
    if let Some(value) = overlay.max_sockets {
        config.max_sockets = Some(value);
    }
}

/// Retry, timeout and identification settings for every fetch.
pub(super) fn apply_fetch_tuning(config: &mut Config, overlay: &ConfigOverlay) {
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
}

/// Which dependency build scripts may run, and under what engine.
pub(super) fn apply_build_policy(config: &mut Config, overlay: &ConfigOverlay) {
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
}

/// Release-age gating and the peer-dependency rules.
pub(super) fn apply_release_policy(config: &mut Config, overlay: &ConfigOverlay) {
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
}

/// Key the overlay's unkeyed (`""`) `Authorization` header — the host's
/// default-registry credential — at the registry that same overlay declared,
/// or at the npmjs default when it declared none. This mirrors the pinning
/// `.npmrc` credentials get in `NpmrcAuth::rescope_unscoped`: the credential
/// and the registry it is sent to both come from the host, so a `registry=`
/// in the repository's `.npmrc` cannot redirect it. A header the host already
/// keyed at that URI wins, and an unparsable default registry drops the
/// unkeyed header rather than sending it somewhere unintended.
pub(super) fn pin_unkeyed_header(
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

pub(super) fn overlay_default_registry(overlay: &ConfigOverlay) -> String {
    overlay
        .registries
        .as_ref()
        .and_then(|registries| registries.get("default"))
        .or(overlay.registry.as_ref())
        .cloned()
        .unwrap_or_else(default_registry)
}
