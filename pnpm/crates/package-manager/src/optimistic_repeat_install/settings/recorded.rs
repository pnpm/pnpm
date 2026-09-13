use super::{
    Catalogs, Config, IncludedDependencies, NodeLinker, SupportedArchitectures, TrustPolicy,
    WorkspaceStateSettings, WorkspaceStateTrustPolicy, catalogs_to_json,
    link_workspace_packages_to_json, map_node_linker,
};

/// Build the [`WorkspaceStateSettings`] that today's install would
/// write. Shared with `install::build_workspace_state` so the
/// freshness check sees the same byte shape the writer produced —
/// when one side grows a field, the other automatically does too.
pub(crate) fn current_settings(
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
    supported_architectures: Option<&SupportedArchitectures>,
) -> WorkspaceStateSettings {
    WorkspaceStateSettings {
        allow_builds: recorded_allow_builds(config),
        auto_install_peers: Some(config.auto_install_peers),
        dedupe_direct_deps: Some(config.dedupe_direct_deps),
        dedupe_injected_deps: Some(config.dedupe_injected_deps),
        dedupe_peer_dependents: Some(config.dedupe_peer_dependents),
        dedupe_peers: Some(config.dedupe_peers),
        dev: Some(included.dev_dependencies),
        // Mirror pnpm's writer, which omits the key for its `undefined`
        // default and records a concrete value only when forced. pacquet
        // has no `--global` flow, so the only "on" value it ever writes
        // is `true`; an off store maps back to the omitted `None`.
        enable_global_virtual_store: config.enable_global_virtual_store.then_some(true),
        exclude_links_from_lockfile: Some(config.exclude_links_from_lockfile),
        hoist_pattern: config.hoist_pattern.clone(),
        hoist_workspace_packages: Some(config.hoist_workspace_packages),
        ignored_optional_dependencies: config.ignored_optional_dependencies.clone(),
        inject_workspace_packages: Some(config.inject_workspace_packages),
        link_workspace_packages: Some(link_workspace_packages_to_json(
            config.link_workspace_packages,
        )),
        node_linker: Some(map_node_linker(node_linker)),
        optional: Some(included.optional_dependencies),
        overrides: recorded_overrides(config),
        package_extensions: config.package_extensions
            .as_ref()
            .and_then(|map| serde_json::to_value(map).ok()),
        patched_dependencies: config.patched_dependencies.clone(),
        peers_suffix_max_length: Some(
            u32::try_from(config.peers_suffix_max_length).unwrap_or(u32::MAX),
        ),
        prefer_workspace_packages: Some(config.prefer_workspace_packages),
        production: Some(included.dependencies),
        public_hoist_pattern: config.public_hoist_pattern.clone(),
        // The CLI-merged effective value (yaml plus `--cpu` / `--os` /
        // `--libc`), like `included` above: a change through either
        // channel re-evaluates the skipped optionals on the next run.
        supported_architectures: supported_architectures.and_then(|value| {
            serde_json::to_value(value).ok()
        }),
        ..current_policy_settings(config)
    }
}

fn recorded_allow_builds(
    config: &Config,
) -> Option<std::collections::BTreeMap<String, serde_json::Value>> {
    (!config.allow_builds.is_empty()).then(|| {
        config.allow_builds
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::Bool(*v)))
            .collect()
    })
}

fn current_policy_settings(config: &Config) -> WorkspaceStateSettings {
    WorkspaceStateSettings {
        minimum_release_age: config.minimum_release_age,
        minimum_release_age_exclude: config.minimum_release_age_exclude.clone(),
        minimum_release_age_ignore_missing_time: Some(
            config.minimum_release_age_ignore_missing_time,
        ),
        // The resolved form pnpm records — see
        // `WorkspaceStateSettings::minimum_release_age_strict`.
        minimum_release_age_strict: config.minimum_release_age_strict.or_else(|| {
            config.resolved_minimum_release_age_strict().then_some(true)
        }),
        // pnpm records the raw config value, which stays `undefined`
        // until the user configures the setting — `explicit_settings` is
        // how pacquet tells its resolved default apart from a real
        // `trustPolicy: off`.
        trust_policy: config.explicit_settings
            .contains_key("trustPolicy")
            .then(|| map_trust_policy(config.trust_policy)),
        trust_policy_exclude: config.trust_policy_exclude.clone(),
        trust_policy_ignore_after: config.trust_policy_ignore_after,
        ..Default::default()
    }
}

pub(crate) fn current_settings_with_catalogs(
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
    supported_architectures: Option<&SupportedArchitectures>,
    catalogs: &Catalogs,
) -> WorkspaceStateSettings {
    let mut settings = current_settings(config, node_linker, included, supported_architectures);
    settings.catalogs = Some(catalogs_to_json(catalogs));
    settings
}

fn map_trust_policy(policy: TrustPolicy) -> WorkspaceStateTrustPolicy {
    match policy {
        TrustPolicy::Off => WorkspaceStateTrustPolicy::Off,
        TrustPolicy::NoDowngrade => WorkspaceStateTrustPolicy::NoDowngrade,
    }
}

fn recorded_overrides(config: &Config) -> Option<std::collections::BTreeMap<String, String>> {
    config.overrides
        .as_ref()
        .map(|map| {
            map
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
}
