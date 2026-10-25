use super::Config;

/// The `registryPrefixes` an install that verified the lockfile's keys
/// records. See
/// [`WorkspaceStateSettings::registry_prefixes`](pnpm_workspace_state::WorkspaceStateSettings::registry_prefixes).
pub(crate) fn recorded_registry_prefixes(
    config: &Config,
) -> Option<std::collections::BTreeMap<String, String>> {
    if config.registries_by_prefix.is_empty() {
        return None;
    }
    let declared = config.declared_registry_prefixes();
    let mut recorded = config.registries_by_prefix.clone();
    recorded.extend(
        config
            .resolved_registries()
            .into_iter()
            .filter_map(|(route, registry)| {
                let prefix = declared.prefix_for(&registry)?;
                Some((format!("route:{route}"), prefix.to_string()))
            }),
    );
    Some(recorded)
}
