//! Protected-settings censoring for `pnpm config list` / `pnpm config get`.
//!
//! Port of pnpm's
//! [`protectedSettings.ts`](https://github.com/pnpm/pnpm/blob/8eb1be4988/config/commands/src/protectedSettings.ts):
//! credential-bearing keys are replaced with `(protected)` so the command
//! never prints a token, password, or username.

use serde_json::{Map, Value};

const PROTECTED_SUFFIXES: &[&str] = &["_auth", "_authToken", "username", "_password"];

/// The placeholder written in place of a protected value.
pub const PROTECTED_PLACEHOLDER: &str = "(protected)";

/// Whether `key` names a protected (credential-bearing) setting. A
/// per-registry key (`//host/...`) is protected when it ends with one of the
/// credential suffixes; otherwise the key itself must be one of them.
///
/// Mirrors pnpm's `isSettingProtected`.
#[must_use]
pub fn is_setting_protected(key: &str) -> bool {
    if key.starts_with("//") {
        PROTECTED_SUFFIXES.iter().any(|suffix| key.ends_with(&format!(":{suffix}")))
    } else {
        PROTECTED_SUFFIXES.contains(&key)
    }
}

/// Replace every protected setting's value with [`PROTECTED_PLACEHOLDER`].
///
/// Mirrors pnpm's `censorProtectedSettings`.
pub fn censor_protected_settings(config: &mut Map<String, Value>) {
    for (key, value) in config.iter_mut() {
        if is_setting_protected(key) {
            *value = Value::String(PROTECTED_PLACEHOLDER.to_string());
        }
    }
}

#[cfg(test)]
mod tests;
