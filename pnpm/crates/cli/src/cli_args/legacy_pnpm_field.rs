//! The `pnpm` field of `package.json` is no longer read for install
//! settings — pnpm 10 moved them all to `pnpm-workspace.yaml`. A project
//! that still carries one of the migrated keys gets a warning naming it,
//! so the setting isn't silently dropped.

use super::{config_warnings::emit_config_warning, package_manager::read_manifest_json};
use serde_json::Value;
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
/// declares under `pnpm`. This is a config-load warning, so it goes to
/// stderr through [`emit_config_warning`] rather than the reporter. A
/// manifest that could not be read is not this function's problem — the
/// install path reports it with far more context — so `None` simply
/// produces no warning.
pub(crate) fn warn_ignored_pnpm_manifest_fields(manifest: Option<&Value>) {
    let ignored = ignored_pnpm_field_keys(manifest);
    if ignored.is_empty() {
        return;
    }
    let keys = ignored.iter().map(|key| format!(r#""pnpm.{key}""#)).collect::<Vec<_>>().join(", ");
    emit_config_warning(&format!(
        "The \"pnpm\" field in package.json is no longer read by pnpm. \
         The following keys were ignored: {keys}. \
         See https://pnpm.io/settings for the new home of each setting.",
    ));
}

/// [`warn_ignored_pnpm_manifest_fields`] for a caller that has not read
/// the root manifest yet.
pub(crate) fn warn_ignored_pnpm_manifest_fields_in(root_dir: &Path) {
    warn_ignored_pnpm_manifest_fields(root_manifest(root_dir).as_ref());
}

fn root_manifest(root_dir: &Path) -> Option<Value> {
    read_manifest_json(&root_dir.join("package.json")).ok().flatten()
}

fn ignored_pnpm_field_keys(manifest: Option<&Value>) -> Vec<String> {
    let Some(legacy_field) =
        manifest.and_then(|manifest| manifest.get("pnpm")).and_then(Value::as_object)
    else {
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
