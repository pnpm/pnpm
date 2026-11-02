use super::{Config, SettingsComparison, WorkspaceStateSettings};

/// The settings that decide which builds, runtimes, and lockfile verification
/// an install runs. An unrecorded value reads as the default.
/// `dangerouslyAllowAllBuilds` is always recorded, as pnpm v11 records it.
pub(super) fn install_policy_settings(
    config: &Config,
    rest: WorkspaceStateSettings,
) -> WorkspaceStateSettings {
    WorkspaceStateSettings {
        dangerously_allow_all_builds: Some(config.dangerously_allow_all_builds),
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
            ("runtime", recorded.runtime.unwrap_or(true) != live.runtime.unwrap_or(true)),
            ("trustLockfile", differs(recorded.trust_lockfile, live.trust_lockfile)),
        ]
        .into_iter()
        .find(|(key, differs)| *differs && !self.ignored.contains(key))
        .map(|(key, _)| key)
    }
}
