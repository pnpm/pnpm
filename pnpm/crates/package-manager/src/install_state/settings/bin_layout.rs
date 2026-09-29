use super::{Config, SettingsComparison, WorkspaceStateSettings};

/// The settings that decide how an install links bins. `preserveBinName` is
/// recorded as its effective value, which is off on Windows.
pub(super) fn bin_layout_settings(
    config: &Config,
    rest: WorkspaceStateSettings,
) -> WorkspaceStateSettings {
    WorkspaceStateSettings {
        preserve_bin_name: Some(cfg!(unix) && config.preserve_bin_name),
        ..rest
    }
}

impl SettingsComparison<'_> {
    pub(super) fn bin_layout_drift(&self) -> Option<&'static str> {
        let key = "preserveBinName";
        let differs = self.recorded.preserve_bin_name.unwrap_or(false)
            != self.live.preserve_bin_name.unwrap_or(false);
        (differs && !self.ignored.contains(&key)).then_some(key)
    }
}
