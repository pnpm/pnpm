//! Which global settings carry credentials, so `pnpm config set --global`
//! writes them owner-only.

use pnpm_config::protected_settings::is_setting_protected;
use serde_json::Value;

/// Whether setting `camel_key` to `value` writes credentials: cache signing
/// keys, or a protected setting inside a section of settings.
pub(super) fn carries_global_credentials(camel_key: &str, value: &Value) -> bool {
    !value.is_null()
        && (matches!(camel_key, "remoteCache" | "remoteSideEffectsCache")
            || (camel_key == "sideEffectsCache"
                && value
                    .as_object()
                    .is_some_and(|object| object.contains_key("remote")))
            || (camel_key == "globalPackagesConfig"
                && value
                    .as_object()
                    .is_some_and(|section| {
                        section
                            .iter()
                            .any(|(setting, value)| {
                                is_setting_protected(setting)
                                    || carries_global_credentials(setting, value)
                            })
                    })))
}
