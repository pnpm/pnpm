//! Protected-settings censoring for `pnpm config list` / `pnpm config get`.
//!
//! Credential-bearing keys are replaced with `(protected)` so the command
//! never prints a token, password, or username.

use serde_json::{Map, Value};

const PROTECTED_SUFFIXES: &[&str] = &["_auth", "_authToken", "username", "_password"];

/// The placeholder written in place of a protected value.
pub const PROTECTED_PLACEHOLDER: &str = "(protected)";

/// Whether `key` names a protected (credential-bearing) setting. A
/// per-registry key (`//host/...`) is protected when it ends with one of the
/// credential suffixes; otherwise the key itself must be one of them.
#[must_use]
pub fn is_setting_protected(key: &str) -> bool {
    if key.starts_with("//") {
        PROTECTED_SUFFIXES
            .iter()
            .any(|suffix| key.ends_with(&format!(":{suffix}")))
    } else {
        PROTECTED_SUFFIXES.contains(&key)
    }
}

/// Replace every protected setting's value (and nested cache signing keys)
/// with [`PROTECTED_PLACEHOLDER`].
pub fn censor_protected_settings(config: &mut Map<String, Value>) {
    for (key, value) in config.iter_mut() {
        censor_protected_setting_value(key, value);
    }
}

/// Mask `value` in place when `key` is a protected setting or carries nested
/// cache signing credentials (`privateKey`).
pub fn censor_protected_setting_value(key: &str, value: &mut Value) {
    if is_setting_protected(key) {
        *value = Value::String(PROTECTED_PLACEHOLDER.to_string());
        return;
    }
    match key {
        "remoteCache" | "remoteSideEffectsCache" => censor_private_key_field(value),
        "sideEffectsCache" => {
            if let Some(remote) = value
                .as_object_mut()
                .and_then(|map| map.get_mut("remote"))
            {
                censor_private_key_field(remote);
            }
        }
        _ => {}
    }
}

fn censor_private_key_field(section: &mut Value) {
    if let Some(private_key) = section
        .as_object_mut()
        .and_then(|map| map.get_mut("privateKey"))
        .filter(|field| !field.is_null())
    {
        *private_key = Value::String(PROTECTED_PLACEHOLDER.to_string());
    }
}

#[cfg(test)]
mod tests;
