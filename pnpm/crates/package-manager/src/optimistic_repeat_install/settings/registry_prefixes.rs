use super::Config;
use std::collections::BTreeMap;

/// The `registryPrefixes` an install that verified the lockfile's keys
/// records. See
/// [`WorkspaceStateSettings::registry_prefixes`](pnpm_workspace_state::WorkspaceStateSettings::registry_prefixes).
pub(crate) fn recorded_registry_prefixes(config: &Config) -> Option<BTreeMap<String, String>> {
    (!config.registries_by_prefix.is_empty()).then(|| {
        config.registries_by_prefix.clone()
    })
}

/// The scope routes an install resolves plain keys through, as
/// [`WorkspaceStateSettings::registry_routes`](pnpm_workspace_state::WorkspaceStateSettings::registry_routes)
/// records them.
pub(crate) fn recorded_registry_routes(config: &Config) -> BTreeMap<String, String> {
    let mut routes = config.resolved_registries();
    routes.remove("default");
    routes
}
