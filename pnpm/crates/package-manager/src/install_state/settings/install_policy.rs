use std::collections::BTreeMap;

use pnpm_config::Tool;

use super::{
    Config,
    SettingsComparison,
    WorkspaceState,
    WorkspaceStateSettings,
};

/// Whether the `extendNodePath` the last install recorded matches `live`.
/// The frozen path's up-to-date early return reads it: the lockfile and
/// `.modules.yaml` stay the same when it changes, and the bin shims it shapes
/// do not.
pub(crate) fn recorded_extend_node_path_matches(
    recorded: Option<&WorkspaceState>,
    live: bool,
) -> bool {
    recorded.and_then(|state| state.settings.extend_node_path).unwrap_or(true) == live
}

/// The settings that decide which builds, runtimes, bin shims, and lockfile
/// verification an install runs. An unrecorded value reads as the default.
/// `dangerouslyAllowAllBuilds` is always recorded, as pnpm v11 records it.
pub(super) fn install_policy_settings(
    config: &Config,
    rest: WorkspaceStateSettings,
) -> WorkspaceStateSettings {
    WorkspaceStateSettings {
        dangerously_allow_all_builds: Some(config.dangerously_allow_all_builds),
        extend_node_path: (!config.extend_node_path).then_some(false),
        node_download_mirrors: recorded_node_download_mirrors(config),
        runtime: config.skip_runtimes.then_some(false),
        runtime_on_fail: config.runtime_on_fail,
        tool_mirrors: recorded_tool_mirrors(config),
        trust_lockfile: config.trust_lockfile.then_some(true),
        ..rest
    }
}

fn recorded_node_download_mirrors(config: &Config) -> Option<BTreeMap<String, String>> {
    (!config.node_download_mirrors.is_empty()).then(|| {
        config.node_download_mirrors
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    })
}

fn recorded_tool_mirrors(config: &Config) -> Option<BTreeMap<String, String>> {
    let mut mirrors = BTreeMap::new();
    for (tool, name) in [(Tool::Node, "node"), (Tool::Bun, "bun")] {
        if let Some(mirror) = config.tool_mirror(tool) {
            mirrors.insert(format!("{name}.mirror"), mirror.to_string());
        }
        for (channel, mirror) in config.tool_channel_mirrors(tool) {
            mirrors.insert(format!("{name}.channels.{channel}"), mirror);
        }
    }
    (!mirrors.is_empty()).then_some(mirrors)
}

fn optional_string_maps_differ(
    recorded: Option<&BTreeMap<String, String>>,
    live: Option<&BTreeMap<String, String>>,
) -> bool {
    let empty = BTreeMap::new();
    recorded.unwrap_or(&empty) != live.unwrap_or(&empty)
}

impl SettingsComparison<'_> {
    pub(super) fn install_policy_drift(&self) -> Option<&'static str> {
        let (recorded, live) = (self.recorded, self.live);
        let differs = |recorded: Option<bool>, live: Option<bool>| {
            recorded.unwrap_or(false) != live.unwrap_or(false)
        };
        [
            (
                "dangerouslyAllowAllBuilds",
                differs(recorded.dangerously_allow_all_builds, live.dangerously_allow_all_builds),
            ),
            (
                "extendNodePath",
                recorded.extend_node_path.unwrap_or(true)
                    != live.extend_node_path.unwrap_or(true),
            ),
            (
                "nodeDownloadMirrors",
                optional_string_maps_differ(
                    recorded.node_download_mirrors.as_ref(),
                    live.node_download_mirrors.as_ref(),
                ),
            ),
            ("runtime", recorded.runtime.unwrap_or(true) != live.runtime.unwrap_or(true)),
            ("runtimeOnFail", recorded.runtime_on_fail != live.runtime_on_fail),
            (
                "toolMirrors",
                optional_string_maps_differ(
                    recorded.tool_mirrors.as_ref(),
                    live.tool_mirrors.as_ref(),
                ),
            ),
            ("trustLockfile", differs(recorded.trust_lockfile, live.trust_lockfile)),
        ]
        .into_iter()
        .find(|(key, differs)| *differs && !self.ignored.contains(key))
        .map(|(key, _)| key)
    }
}
