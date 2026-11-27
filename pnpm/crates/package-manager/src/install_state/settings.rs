//! Comparing the settings a previous install recorded against the current ones.

pub(crate) use current::{
    current_settings,
    current_settings_with_catalogs,
};
pub(crate) use install_policy::recorded_extend_node_path_matches;
pub(crate) use recorded_values::{
    catalogs_to_json,
    empty_json_object,
    filter_null_object_values,
    link_workspace_packages_to_json,
};
pub(crate) use registry_prefixes::{
    ResolvedImporters,
    recorded_default_registry,
    recorded_registry_prefixes,
    recorded_registry_routes,
    recorded_registry_routes_after_install,
    records_registry_keys_of,
    registry_key_settings_differ,
};

mod agent_skills;
mod current;
mod equivalence;
mod install_policy;
mod recorded_values;
mod registry_prefixes;
mod store_location;
use agent_skills::agent_skills_settings;
use equivalence::{
    allow_builds_match,
    enable_global_virtual_store_match,
    package_extensions_match,
    patched_dependencies_match,
    reference_time_moved_back,
};
use install_policy::install_policy_settings;
use recorded_values::map_trust_policy;

use pnpm_catalogs_types::Catalogs;
use pnpm_config::{
    Config,
    NodeLinker,
    version_policy::ResolvedPackageVersions,
};
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_package_is_installable::SupportedArchitectures;
use pnpm_workspace_state::{
    WorkspaceState,
    WorkspaceStateSettings,
};
use std::cell::LazyCell;

/// The workspace-state key of [`global_virtual_store_layout_version`]. It is
/// not a setting a user can name, so a message words it separately.
pub(crate) const GLOBAL_VIRTUAL_STORE_LAYOUT_VERSION_KEY: &str = "globalVirtualStoreLayoutVersion";

pub(crate) fn global_virtual_store_layout_version(config: &Config) -> Option<u8> {
    config.enable_global_virtual_store.then_some(
        pnpm_deps_restorer::GLOBAL_VIRTUAL_STORE_LAYOUT_VERSION,
    )
}

pub(crate) fn settings_json(settings: &WorkspaceStateSettings) -> String {
    serde_json::to_string(settings).expect("serialize workspace state settings")
}

/// Compare settings an install recorded against today's settings with the
/// same equivalence rules the freshness gate applies to workspace state.
pub(crate) fn settings_match<'a>(
    recorded: &'a str,
    current: &'a WorkspaceStateSettings,
    ignored: &'a [&'a str],
    workspace_dir: Option<&'a Path>,
) -> Result<bool, serde_json::Error> {
    let mut recorded: WorkspaceStateSettings = serde_json::from_str(recorded)?;
    if recorded.registry_routes.is_none()
        && let Some(installed) = recorded.installed_registry_routes.take()
    {
        recorded.registry_routes = Some(installed);
    }
    Ok(SettingsComparison { recorded: &recorded, live: current, ignored, workspace_dir }
        .first_drift()
        .is_none())
}

/// Whether the `supportedArchitectures` recorded by the last install
/// matches `live` (today's CLI-merged value). Read from the workspace
/// state for the frozen path's lockfile-up-to-date early return, whose
/// other guards (`wanted == current`, `.modules.yaml` consistency) cannot
/// see an architecture change: the skip set it would produce differs, so
/// the shortcut must not fire. A missing or unreadable state (`None`)
/// reports a match only when `live` is also unset, the conservative
/// direction is a full install.
pub(crate) fn recorded_supported_architectures_match(
    recorded: Option<&WorkspaceState>,
    live: Option<&SupportedArchitectures>,
) -> bool {
    let recorded =
        recorded.and_then(|state| state.settings.supported_architectures.as_ref());
    recorded
        == live
            .map(serde_json::to_value)
            .and_then(Result::ok)
            .as_ref()
}

/// Compare today's settings against what the previous install
/// recorded.
///
/// Only the fields pacquet populates via [`current_settings`]
/// participate in the comparison; the rest are listed at the end of
/// this function with the reason each is safe to skip.
///
/// pnpm iterates the full `WORKSPACE_STATE_SETTING_KEYS` list, reading a
/// key absent from the recorded state as `undefined`. So the reverse
/// scenario (pacquet wrote the state, pnpm reads it next) stays on the
/// fast path only for keys whose pnpm-resolved value is also
/// `undefined`. Every key pnpm resolves to a concrete default
/// (`excludeLinksFromLockfile` (`false`), `minimumReleaseAge` (`1440`),
/// `minimumReleaseAgeIgnoreMissingTime` (`true`)) must therefore be
/// written by [`current_settings`] and compared here, or pnpm would
/// report drift and re-run a (no-op) install on every command after a
/// pacquet install. `enableGlobalVirtualStore` is `undefined` by
/// default (concrete only under `--global`/CI), so pacquet's omit-when-
/// off encoding already matches. The `allowBuilds` coercion treats an
/// absent value as an empty map on the read side, matching pnpm's
/// tolerance of an absent `allowBuilds` key in the recorded state on
/// the write side.
/// The camelCase name (pnpm's workspace-state setting key) of the first
/// recorded setting that differs from today's config, or `None` when
/// they all match. `ignored_workspace_state_settings` lets callers skip
/// keys such as `dev` / `optional` / `production`: `pnpm run` / `pnpm
/// exec` always execute with the default dependency groups, so those
/// never match the state written by a `--prod` / `--no-optional`
/// install (pnpm's `ignoredWorkspaceStateSettings`).
pub(crate) fn first_setting_drift(
    state: &WorkspaceState,
    config: &Config,
    load_prune_versions: &dyn Fn() -> Option<ResolvedPackageVersions>,
    layout: crate::RepeatInstallLayout<'_>,
    ignored_workspace_state_settings: &[&str],
) -> Option<&'static str> {
    let current = current_settings(
        config,
        layout.node_linker,
        layout.included,
        layout.supported_architectures,
    );
    SettingsComparison {
        recorded: &state.settings,
        live: &current,
        ignored: ignored_workspace_state_settings,
        config,
        prune_versions: LazyCell::new(load_prune_versions),
    }
    .first_drift()
}

macro_rules! return_drift_if {
    ($comparison:expr, $key:literal, $differs:expr $(,)?) => {
        if !$comparison.ignored.contains(&$key) && $differs {
            return Some($key);
        }
    };
}

mod release_policy;

struct SettingsComparison<'a> {
    recorded: &'a WorkspaceStateSettings,
    live: &'a WorkspaceStateSettings,
    ignored: &'a [&'a str],
    config: &'a Config,
    prune_versions:
        LazyCell<Option<ResolvedPackageVersions>, &'a dyn Fn() -> Option<ResolvedPackageVersions>>,
}

impl SettingsComparison<'_> {
    fn first_drift(&self) -> Option<&'static str> {
        self.installation_drift()
            .or_else(|| self.lockfile_drift())
            .or_else(|| self.linking_drift())
            .or_else(|| self.resolution_drift())
            .or_else(|| self.workspace_policy_drift())
            .or_else(|| self.agent_skills_drift())
            .or_else(|| self.store_location_drift())
            .or_else(|| self.install_policy_drift())
    }
    fn installation_drift(&self) -> Option<&'static str> {
        let recorded = self.recorded;
        let live = self.live;

        let allow_builds_drift =
            !allow_builds_match(recorded.allow_builds.as_ref(), live.allow_builds.as_ref());
        return_drift_if!(self, "allowBuilds", allow_builds_drift);
        return_drift_if!(
            self,
            "autoInstallPeers",
            recorded.auto_install_peers != live.auto_install_peers
        );
        return_drift_if!(
            self,
            "dedupeDirectDeps",
            recorded.dedupe_direct_deps != live.dedupe_direct_deps
        );
        return_drift_if!(
            self,
            "dedupeInjectedDeps",
            recorded.dedupe_injected_deps != live.dedupe_injected_deps,
        );
        return_drift_if!(
            self,
            "dedupePeerDependents",
            recorded.dedupe_peer_dependents != live.dedupe_peer_dependents,
        );
        return_drift_if!(self, "dedupePeers", recorded.dedupe_peers != live.dedupe_peers);
        return_drift_if!(
            self,
            "addMissingPeerTypes",
            recorded.add_missing_peer_types != live.add_missing_peer_types,
        );
        return_drift_if!(
            self,
            "projectScopedOptionalPeers",
            recorded.project_scoped_optional_peers != live.project_scoped_optional_peers,
        );
        return_drift_if!(self, "autoDedupe", recorded.auto_dedupe != live.auto_dedupe);
        return_drift_if!(self, "dev", recorded.dev != live.dev);
        None
    }
    fn lockfile_drift(&self) -> Option<&'static str> {
        let recorded = self.recorded;
        let live = self.live;
        return_drift_if!(self, "lockfile.format", recorded.lockfile_format != live.lockfile_format);
        return_drift_if!(
            self,
            "lockfile.includeResolutionSettings",
            recorded.lockfile_include_resolution_settings
                != live.lockfile_include_resolution_settings,
        );
        return_drift_if!(
            self,
            "lockfile.integrityTarget",
            recorded.lockfile_integrity_target != live.lockfile_integrity_target,
        );
        let registry_keys_drift = registry_key_settings_differ(recorded, live);
        return_drift_if!(self, "registryPrefixes", registry_keys_drift);
        if self.registry_routes_differ() {
            return Some("registryRoutes");
        }
        let reference_time_drift = reference_time_moved_back(
            recorded.reference_time.as_deref(),
            live.reference_time.as_deref(),
            jiff::Timestamp::now(),
        );
        return_drift_if!(self, "referenceTime", reference_time_drift);
        None
    }
    fn linking_drift(&self) -> Option<&'static str> {
        let recorded = self.recorded;
        let live = self.live;
        let enable_global_virtual_store_drift = !enable_global_virtual_store_match(
            recorded.enable_global_virtual_store,
            live.enable_global_virtual_store,
        );
        return_drift_if!(self, "enableGlobalVirtualStore", enable_global_virtual_store_drift);
        return_drift_if!(
            self,
            "excludeLinksFromLockfile",
            recorded.exclude_links_from_lockfile != live.exclude_links_from_lockfile,
        );
        return_drift_if!(self, "hoistPattern", recorded.hoist_pattern != live.hoist_pattern);
        return_drift_if!(
            self,
            "sideEffectsCache.exclude",
            recorded.side_effects_cache_exclude != live.side_effects_cache_exclude,
        );
        return_drift_if!(
            self,
            "hoistWorkspacePackages",
            recorded.hoist_workspace_packages != live.hoist_workspace_packages,
        );
        return_drift_if!(
            self,
            "ignoredOptionalDependencies",
            recorded.ignored_optional_dependencies != live.ignored_optional_dependencies,
        );
        return_drift_if!(
            self,
            "injectWorkspacePackages",
            recorded.inject_workspace_packages != live.inject_workspace_packages,
        );
        return_drift_if!(
            self,
            "linkWorkspacePackages",
            recorded.link_workspace_packages != live.link_workspace_packages,
        );
        None
    }
    fn resolution_drift(&self) -> Option<&'static str> {
        let recorded = self.recorded;
        let live = self.live;
        return_drift_if!(
            self,
            "blockExoticSubdeps",
            recorded.block_exotic_subdeps != live.block_exotic_subdeps,
        );
        if let Some(drift) = self.minimum_release_age_drift() {
            return Some(drift);
        }
        return_drift_if!(
            self,
            "nodeLinker",
            recorded.node_linker != live.node_linker
                || recorded.node_linker_excluded != live.node_linker_excluded
        );
        return_drift_if!(self, "optional", recorded.optional != live.optional);
        return_drift_if!(self, "overrides", recorded.overrides != live.overrides);
        let package_extensions_drift = !package_extensions_match(
            recorded.package_extensions.as_ref(),
            live.package_extensions.as_ref(),
        );
        return_drift_if!(self, "packageExtensions", package_extensions_drift);
        None
    }
    fn workspace_policy_drift(&self) -> Option<&'static str> {
        let recorded = self.recorded;
        let live = self.live;
        let patched_dependencies_drift =
            !patched_dependencies_match(recorded, live, self.config.workspace_dir.as_deref());
        return_drift_if!(self, "patchedDependencies", patched_dependencies_drift);
        return_drift_if!(
            self,
            "peersSuffixMaxLength",
            recorded.peers_suffix_max_length != live.peers_suffix_max_length,
        );
        return_drift_if!(
            self,
            "preferWorkspacePackages",
            recorded.prefer_workspace_packages != live.prefer_workspace_packages,
        );
        return_drift_if!(self, "production", recorded.production != live.production);
        return_drift_if!(
            self,
            "publicHoistPattern",
            recorded.public_hoist_pattern != live.public_hoist_pattern,
        );
        return_drift_if!(
            self,
            "supportedArchitectures",
            recorded.supported_architectures != live.supported_architectures,
        );
        self.trust_policy_drift()
    }
}

pub(crate) fn catalogs_cache_matches(
    recorded: Option<&serde_json::Value>,
    current: &Catalogs,
) -> bool {
    let recorded = recorded.cloned().map_or_else(empty_json_object, filter_null_object_values);
    let current = filter_null_object_values(catalogs_to_json(current));
    recorded == current
}

#[cfg(test)]
mod tests;
