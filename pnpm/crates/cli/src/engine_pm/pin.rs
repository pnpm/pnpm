//! Record which package manager a project uses.
//!
//! A package manager is not a dependency of the project it installs, so
//! naming one in `pnpm add` writes the field that declares it —
//! `devEngines.packageManager` — instead of a dependency entry. That is
//! the field pnpm's own package-manager check reads, and the one every
//! other tool reading a project's manager understands.

use serde_json::{Map, Value};

use crate::engine_pm::channel::PackageManager;

/// Write `pm` into the manifest's `devEngines.packageManager`, replacing
/// whatever package manager was declared there.
///
/// `version_spec` is recorded as the user wrote it: the field holds a
/// range, and a bare `pnpm add yarn` asks for Yarn without asking for a
/// version, so none is invented for it.
pub(crate) fn record_package_manager_pin(
    manifest: &mut Value,
    pm: PackageManager,
    version_spec: Option<&str>,
) {
    let mut entry = Map::new();
    entry.insert("name".to_string(), Value::String(pm.name().to_string()));
    if let Some(version_spec) = version_spec.map(str::trim).filter(|spec| !spec.is_empty()) {
        entry.insert("version".to_string(), Value::String(version_spec.to_string()));
    }

    let dev_engines = manifest
        .as_object_mut()
        .expect("a manifest is a JSON object")
        .entry("devEngines")
        .or_insert_with(|| Value::Object(Map::new()));
    if !dev_engines.is_object() {
        *dev_engines = Value::Object(Map::new());
    }
    let dev_engines = dev_engines.as_object_mut().expect("just made it an object");
    dev_engines.insert("packageManager".to_string(), Value::Object(entry));
}

/// How the recorded pin reads back, for the line `pnpm add` prints.
pub(crate) fn describe_pin(pm: PackageManager, version_spec: Option<&str>) -> String {
    match version_spec.map(str::trim).filter(|spec| !spec.is_empty()) {
        Some(version_spec) => format!("{}@{version_spec}", pm.name()),
        None => pm.name().to_string(),
    }
}

#[cfg(test)]
mod tests;
