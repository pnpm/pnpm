//! The `pnpm` field of `package.json` is no longer read for install
//! settings — pnpm 10 moved them all to `pnpm-workspace.yaml`. A project
//! that still carries one of the migrated keys gets a warning naming it,
//! so the setting isn't silently dropped.

use pacquet_reporter::{GlobalLog, LogEvent, LogLevel};
use std::path::Path;

/// Keys pnpm reads from `pnpm-workspace.yaml` and never from the `pnpm`
/// field of `package.json` — either because they moved there in v11, or
/// because (like `update`) they were introduced later and only ever
/// lived there. Keys outside this set (`app`, or anything third-party
/// tooling piggybacks on the `pnpm` namespace for) are left alone so the
/// warning can't fire on something pnpm never owned.
const MIGRATED_PNPM_FIELD_KEYS: &[&str] = &[
    "allowBuilds",
    "allowedDeprecatedVersions",
    "allowUnusedPatches",
    "audit",
    "auditConfig",
    "configDependencies",
    "executionEnv",
    "ignoredOptionalDependencies",
    "neverBuiltDependencies",
    "onlyBuiltDependencies",
    "onlyBuiltDependenciesFile",
    "overrides",
    "packageExtensions",
    "patchedDependencies",
    "peerDependencyRules",
    "requiredScripts",
    "supportedArchitectures",
    "update",
    "updateConfig",
];

/// Warn about every migrated key the root project manifest still
/// declares under `pnpm`. An unreadable or malformed manifest is not
/// this function's problem — the install path reports it with far more
/// context — so it simply produces no warning.
pub(crate) fn warn_ignored_pnpm_manifest_fields(root_dir: &Path, emit: fn(&LogEvent)) {
    let ignored = ignored_pnpm_field_keys(root_dir);
    if ignored.is_empty() {
        return;
    }
    let keys = ignored.iter().map(|key| format!(r#""pnpm.{key}""#)).collect::<Vec<_>>().join(", ");
    emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Warn,
        message: format!(
            "The \"pnpm\" field in package.json is no longer read by pnpm. \
             The following keys were ignored: {keys}. \
             See https://pnpm.io/settings for the new home of each setting.",
        ),
    }));
}

fn ignored_pnpm_field_keys(root_dir: &Path) -> Vec<String> {
    let Ok(bytes) = std::fs::read(root_dir.join("package.json")) else {
        return Vec::new();
    };
    let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Vec::new();
    };
    let Some(legacy_field) = manifest.get("pnpm").and_then(serde_json::Value::as_object) else {
        return Vec::new();
    };
    legacy_field
        .keys()
        .filter(|key| MIGRATED_PNPM_FIELD_KEYS.contains(&key.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests;
