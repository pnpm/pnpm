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

/// The `registryRoutes` an install records (see
/// [`WorkspaceStateSettings::registry_routes`](pnpm_workspace_state::WorkspaceStateSettings::registry_routes)):
/// the routes it ran under, unless it kept part of `lockfile` as recorded
/// (`resolved_all` is false) while a plain key still sits under a route
/// `previous` records elsewhere, when `previous` stands.
pub(crate) fn recorded_registry_routes_after_install(
    config: &Config,
    previous: Option<&BTreeMap<String, String>>,
    lockfile: Option<&pnpm_lockfile::Lockfile>,
    resolved_all: bool,
) -> BTreeMap<String, String> {
    let current = recorded_registry_routes(config);
    let Some(previous) = previous.filter(|previous| **previous != current && !resolved_all) else {
        return current;
    };
    let registries = config
        .registry_context()
        .with_locked_registries(Some(
            previous
                .iter()
                .map(|(route, url)| (route.clone(), url.clone()))
                .collect(),
        ));
    match lockfile {
        Some(lockfile)
            if pnpm_lockfile::check_lockfile_registry_routes(lockfile, &registries).is_err() =>
        {
            previous.clone()
        }
        _ => current,
    }
}
