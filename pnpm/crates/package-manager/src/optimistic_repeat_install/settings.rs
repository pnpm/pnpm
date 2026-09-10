//! Comparing the settings a previous install recorded against the current ones.

use super::{
    Catalogs, Config, IncludedDependencies, LinkWorkspacePackages, NodeLinker, Path,
    SupportedArchitectures, TrustPolicy, WorkspaceState, WorkspaceStateNodeLinker,
    WorkspaceStateSettings, WorkspaceStateTrustPolicy, load_workspace_state,
};

/// Whether the `supportedArchitectures` recorded by the last install
/// matches `live` (today's CLI-merged value). Read from the workspace
/// state for the frozen path's lockfile-up-to-date early return, whose
/// other guards (`wanted == current`, `.modules.yaml` consistency) cannot
/// see an architecture change: the skip set it would produce differs, so
/// the shortcut must not fire. A missing state file or an unreadable one
/// reports a match only when `live` is also unset — the conservative
/// direction is a full install.
pub(crate) fn recorded_supported_architectures_match(
    workspace_root: &Path,
    live: Option<&SupportedArchitectures>,
) -> bool {
    let recorded = match load_workspace_state(workspace_root) {
        Ok(Some(state)) => state.settings.supported_architectures,
        _ => None,
    };
    recorded == live.and_then(|value| serde_json::to_value(value).ok())
}

pub(crate) fn settings_match(
    state: &WorkspaceState,
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
    supported_architectures: Option<&SupportedArchitectures>,
    ignored_workspace_state_settings: &[&str],
) -> bool {
    first_setting_drift(
        state,
        config,
        node_linker,
        included,
        supported_architectures,
        ignored_workspace_state_settings,
    )
    .is_none()
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
/// `undefined`. Every key pnpm resolves to a concrete default —
/// `excludeLinksFromLockfile` (`false`), `minimumReleaseAge` (`1440`),
/// `minimumReleaseAgeIgnoreMissingTime` (`true`) — must therefore be
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
    node_linker: NodeLinker,
    included: IncludedDependencies,
    supported_architectures: Option<&SupportedArchitectures>,
    ignored_workspace_state_settings: &[&str],
) -> Option<&'static str> {
    let current = current_settings(config, node_linker, included, supported_architectures);
    SettingsComparison {
        recorded: &state.settings,
        live: &current,
        ignored: ignored_workspace_state_settings,
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

struct SettingsComparison<'a> {
    recorded: &'a WorkspaceStateSettings,
    live: &'a WorkspaceStateSettings,
    ignored: &'a [&'a str],
}

impl SettingsComparison<'_> {
    fn first_drift(&self) -> Option<&'static str> {
        self.installation_drift()
            .or_else(|| self.linking_drift())
            .or_else(|| self.resolution_drift())
            .or_else(|| self.workspace_policy_drift())
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
        return_drift_if!(self, "dev", recorded.dev != live.dev);
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
            "minimumReleaseAge",
            recorded.minimum_release_age != live.minimum_release_age,
        );
        return_drift_if!(
            self,
            "minimumReleaseAgeExclude",
            recorded.minimum_release_age_exclude != live.minimum_release_age_exclude,
        );
        return_drift_if!(
            self,
            "minimumReleaseAgeIgnoreMissingTime",
            recorded.minimum_release_age_ignore_missing_time
                != live.minimum_release_age_ignore_missing_time,
        );
        return_drift_if!(
            self,
            "minimumReleaseAgeStrict",
            recorded.minimum_release_age_strict != live.minimum_release_age_strict,
        );
        return_drift_if!(self, "nodeLinker", recorded.node_linker != live.node_linker);
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
        return_drift_if!(
            self,
            "patchedDependencies",
            recorded.patched_dependencies != live.patched_dependencies,
        );
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
        return_drift_if!(self, "trustPolicy", recorded.trust_policy != live.trust_policy);
        return_drift_if!(
            self,
            "trustPolicyExclude",
            recorded.trust_policy_exclude != live.trust_policy_exclude,
        );
        return_drift_if!(
            self,
            "trustPolicyIgnoreAfter",
            recorded.trust_policy_ignore_after != live.trust_policy_ignore_after,
        );
        None
    }
}

/// `enableGlobalVirtualStore` has no `?? default` coercion on pnpm's
/// read side, but its `undefined` default and an explicit `false` both
/// mean "global virtual store off". pnpm omits the key for the former
/// and records `false` only when CI forces it; pacquet omits both.
/// Normalize the absent and `false` forms before comparing so a
/// pnpm-written file (omitted or `false`) matches a pacquet install
/// with the store off, while a real `true`/`false` flip still trips.
pub(crate) fn enable_global_virtual_store_match(
    state_value: Option<bool>,
    current_value: Option<bool>,
) -> bool {
    state_value.unwrap_or(false) == current_value.unwrap_or(false)
}

/// Pnpm writes `Some({})` for an empty `allowBuilds`; pacquet writes
/// `None` for the same effective value. Treat them as equivalent so
/// cross-package-manager state files don't trip the comparison.
pub(crate) fn allow_builds_match(
    state_value: Option<&std::collections::BTreeMap<String, serde_json::Value>>,
    current_value: Option<&std::collections::BTreeMap<String, serde_json::Value>>,
) -> bool {
    match (state_value, current_value) {
        (None, None) => true,
        (Some(map), None) | (None, Some(map)) => map.is_empty(),
        (Some(state_map), Some(current_map)) => state_map == current_map,
    }
}

/// `packageExtensions` are compared as opaque `serde_json::Value`
/// trees so the workspace-state file written by either implementation
/// round-trips through the other. Empty maps are equivalent to absent
/// — pacquet's [`pnpm_config::WorkspaceSettings::apply_to`] already collapses
/// `packageExtensions: {}` to `None`, but pnpm may write `Some({})`
/// directly, and the workspace-state file is shared across the two.
pub(crate) fn package_extensions_match(
    state_value: Option<&serde_json::Value>,
    current_value: Option<&serde_json::Value>,
) -> bool {
    fn is_empty(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(map) => map.is_empty(),
            serde_json::Value::Null => true,
            _ => false,
        }
    }
    match (state_value, current_value) {
        (None, None) => true,
        (Some(value), None) | (None, Some(value)) => is_empty(value),
        (Some(state_value), Some(current_value)) => state_value == current_value,
    }
}

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
        overrides: config
            .overrides
            .as_ref()
            .map(|map| map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        package_extensions: config
            .package_extensions
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
        supported_architectures: supported_architectures
            .and_then(|value| serde_json::to_value(value).ok()),
        ..current_policy_settings(config)
    }
}

fn recorded_allow_builds(
    config: &Config,
) -> Option<std::collections::BTreeMap<String, serde_json::Value>> {
    (!config.allow_builds.is_empty()).then(|| {
        config.allow_builds.iter().map(|(k, v)| (k.clone(), serde_json::Value::Bool(*v))).collect()
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
        minimum_release_age_strict: config
            .minimum_release_age_strict
            .or_else(|| config.resolved_minimum_release_age_strict().then_some(true)),
        // pnpm records the raw config value, which stays `undefined`
        // until the user configures the setting — `explicit_settings` is
        // how pacquet tells its resolved default apart from a real
        // `trustPolicy: off`.
        trust_policy: config
            .explicit_settings
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

pub(crate) fn catalogs_cache_matches(
    recorded: Option<&serde_json::Value>,
    current: &Catalogs,
) -> bool {
    let recorded = recorded.cloned().map_or_else(empty_json_object, filter_null_object_values);
    let current = filter_null_object_values(catalogs_to_json(current));
    recorded == current
}

pub(crate) fn catalogs_to_json(catalogs: &Catalogs) -> serde_json::Value {
    serde_json::to_value(catalogs).expect("Catalogs serialize to a JSON object")
}

pub(crate) fn empty_json_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

pub(crate) fn filter_null_object_values(value: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Object(mut map) = value else { return value };
    map.retain(|_, value| !value.is_null());
    serde_json::Value::Object(map)
}

pub(crate) fn link_workspace_packages_to_json(value: LinkWorkspacePackages) -> serde_json::Value {
    match value {
        LinkWorkspacePackages::Off => serde_json::Value::Bool(false),
        LinkWorkspacePackages::DirectOnly => serde_json::Value::Bool(true),
        LinkWorkspacePackages::Deep => serde_json::Value::String("deep".to_string()),
    }
}

pub(crate) fn map_node_linker(linker: NodeLinker) -> WorkspaceStateNodeLinker {
    match linker {
        NodeLinker::Isolated => WorkspaceStateNodeLinker::Isolated,
        NodeLinker::Hoisted => WorkspaceStateNodeLinker::Hoisted,
        NodeLinker::Pnp => WorkspaceStateNodeLinker::Pnp,
    }
}

fn map_trust_policy(policy: TrustPolicy) -> WorkspaceStateTrustPolicy {
    match policy {
        TrustPolicy::Off => WorkspaceStateTrustPolicy::Off,
        TrustPolicy::NoDowngrade => WorkspaceStateTrustPolicy::NoDowngrade,
    }
}
