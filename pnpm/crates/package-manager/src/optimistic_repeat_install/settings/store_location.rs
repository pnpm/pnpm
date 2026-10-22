use super::{Config, SettingsComparison, WorkspaceStateSettings};
use std::path::Path;

/// The configured `storeDir` and `virtualStoreDir` as the directories they
/// resolve to, the form pnpm v11 records, so a relative and an absolute
/// spelling of one directory compare equal. A setting that is not configured
/// stays unset.
pub(super) fn store_location_settings(
    config: &Config,
    rest: WorkspaceStateSettings,
) -> WorkspaceStateSettings {
    let configured = |key: &str, dir: &Path| {
        config.explicit_settings
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|raw| !raw.is_empty())
            .map(|_| pnpm_fs::lexical_normalize(dir).to_string_lossy().into_owned())
    };
    WorkspaceStateSettings {
        store_dir: configured("storeDir", config.store_dir.root()),
        virtual_store_dir: configured("virtualStoreDir", config.virtual_store_dir()),
        ..rest
    }
}

impl SettingsComparison<'_> {
    pub(super) fn store_location_drift(&self) -> Option<&'static str> {
        let (recorded, live) = (self.recorded, self.live);
        [
            ("storeDir", recorded.store_dir != live.store_dir),
            ("virtualStoreDir", recorded.virtual_store_dir != live.virtual_store_dir),
        ]
        .into_iter()
        .find(|(key, differs)| *differs && !self.ignored.contains(key))
        .map(|(key, _)| key)
    }
}
