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

/// The `defaultRegistry` recorded with [`recorded_registry_prefixes`]. See
/// [`WorkspaceStateSettings::default_registry`](pnpm_workspace_state::WorkspaceStateSettings::default_registry).
pub(crate) fn recorded_default_registry(config: &Config) -> Option<String> {
    if config.registries_by_prefix.is_empty() {
        return None;
    }
    pnpm_lockfile::normalized_registry_key(&config.registry)
}

/// Whether two states record different registry-key verifications.
pub(crate) fn registry_key_settings_differ(
    recorded: &pnpm_workspace_state::WorkspaceStateSettings,
    live: &pnpm_workspace_state::WorkspaceStateSettings,
) -> bool {
    recorded.registry_prefixes != live.registry_prefixes
        || recorded.default_registry != live.default_registry
}

/// Whether `settings` records the registry-key verification `config` calls
/// for: its prefixes under its default registry.
pub(crate) fn records_registry_keys_of(
    settings: &pnpm_workspace_state::WorkspaceStateSettings,
    config: &Config,
) -> bool {
    settings.registry_prefixes == recorded_registry_prefixes(config)
        && settings.default_registry == recorded_default_registry(config)
}

/// The scope routes an install resolves plain keys through, as
/// [`WorkspaceStateSettings::registry_routes`](pnpm_workspace_state::WorkspaceStateSettings::registry_routes)
/// records them.
pub(crate) fn recorded_registry_routes(config: &Config) -> BTreeMap<String, String> {
    let mut routes = config.resolved_registries();
    routes.remove("default");
    routes
}

/// How much of the lockfile an install resolved, for
/// [`recorded_registry_routes_after_install`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolvedImporters {
    /// Every importer, so every plain key resolved through the current routes.
    All,
    /// The selected importers; the others keep their recorded keys.
    Selected,
    /// Nothing: a frozen install, or a tree found up to date.
    None,
}

/// The `registryRoutes` an install records (see
/// [`WorkspaceStateSettings::registry_routes`](pnpm_workspace_state::WorkspaceStateSettings::registry_routes)):
/// the routes it ran under, unless it kept part of `lockfile` as recorded
/// while a plain key still sits under a route `previous` records
/// elsewhere, when `previous` stands.
pub(crate) fn recorded_registry_routes_after_install(
    config: &Config,
    previous: Option<&BTreeMap<String, String>>,
    lockfile: Option<&pnpm_lockfile::Lockfile>,
    resolution: ResolvedImporters,
) -> BTreeMap<String, String> {
    let current = recorded_registry_routes(config);
    let keeps_recorded_keys = match resolution {
        ResolvedImporters::All => false,
        ResolvedImporters::Selected | ResolvedImporters::None => true,
    };
    let Some(previous) = previous.filter(|previous| **previous != current && keeps_recorded_keys)
    else {
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
