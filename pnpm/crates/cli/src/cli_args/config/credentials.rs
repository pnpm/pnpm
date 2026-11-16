//! Which global settings carry credentials, so `pnpm config set --global`
//! writes them owner-only.

use serde_json::Value;

/// Whether setting `camel_key` to `value` writes cache signing credentials.
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
                            .any(|(setting, value)| carries_global_credentials(setting, value))
                    })))
}
