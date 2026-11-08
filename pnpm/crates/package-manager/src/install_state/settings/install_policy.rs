use super::{Config, SettingsComparison, WorkspaceState, WorkspaceStateSettings};

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
        runtime: config.skip_runtimes.then_some(false),
        trust_lockfile: config.trust_lockfile.then_some(true),
        ..rest
    }
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
            ("runtime", recorded.runtime.unwrap_or(true) != live.runtime.unwrap_or(true)),
            ("trustLockfile", differs(recorded.trust_lockfile, live.trust_lockfile)),
        ]
        .into_iter()
        .find(|(key, differs)| *differs && !self.ignored.contains(key))
        .map(|(key, _)| key)
    }
}
