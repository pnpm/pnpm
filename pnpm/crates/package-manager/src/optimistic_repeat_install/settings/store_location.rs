use super::{Config, SettingsComparison, WorkspaceStateSettings};
use pnpm_config::GetHomeDir;
use std::path::MAIN_SEPARATOR_STR;

/// `storeDir` and `virtualStoreDir` as configured, the raw values pnpm v11
/// records, with a leading `~/` in `storeDir` expanded as pnpm v11 does.
pub(super) fn store_location_settings(
    config: &Config,
    rest: WorkspaceStateSettings,
) -> WorkspaceStateSettings {
    let configured =
        |key: &str| config.explicit_settings.get(key).and_then(serde_json::Value::as_str);
    WorkspaceStateSettings {
        store_dir: configured("storeDir").map(expand_home_prefix::<pnpm_config::Host>),
        virtual_store_dir: configured("virtualStoreDir").map(str::to_string),
        ..rest
    }
}

fn expand_home_prefix<Sys: GetHomeDir>(path: &str) -> String {
    let Some(relative) = path
        .strip_prefix("~/")
        .or_else(|| path.strip_prefix(r"~\"))
    else {
        return path.to_string();
    };
    let Some(home_dir) = Sys::home_dir() else { return path.to_string() };
    let mut joined = home_dir.into_os_string();
    joined.push(MAIN_SEPARATOR_STR);
    joined.push(relative);
    pnpm_fs::lexical_normalize(std::path::Path::new(&joined)).to_string_lossy().into_owned()
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

#[cfg(test)]
mod tests;
