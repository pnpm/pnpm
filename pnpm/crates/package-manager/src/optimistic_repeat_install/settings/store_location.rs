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
    let configured = |key: &str| {
        config.explicit_settings
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|raw| !raw.is_empty())
    };
    let recorded = |dir: &Path| pnpm_fs::lexical_normalize(dir).to_string_lossy().into_owned();
    WorkspaceStateSettings {
        store_dir: configured("storeDir").map(|_| recorded(config.store_dir.root())),
        // Anchored like a configured `virtualStoreDir` without a global
        // virtual store: under one, `Config::virtual_store_dir` names the
        // global virtual store, which pnpm v11 does not record here.
        virtual_store_dir: configured("virtualStoreDir")
            .and_then(|raw| {
                config
                    .modules_dir_anchor()
                    .map(|anchor| recorded(&anchor.join(raw)))
            }),
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
