use std::{
    collections::HashMap,
    sync::OnceLock,
};

static ENV_FILE_VALUES: OnceLock<HashMap<String, String>> = OnceLock::new();

pub(crate) fn record_env_file(names: impl IntoIterator<Item = String>) {
    let values = names
        .into_iter()
        .filter_map(|name| {
            std::env::var(&name)
                .ok()
                .map(|value| (name, value))
        })
        .collect();
    let _ = ENV_FILE_VALUES.set(values);
}

pub(crate) fn env_file_values() -> HashMap<String, String> {
    ENV_FILE_VALUES
        .get()
        .cloned()
        .unwrap_or_default()
}
