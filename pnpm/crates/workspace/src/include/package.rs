//! The config entry a config dependency declares in its `package.json`.

use super::{INCLUDE_KEY, IncludeError, SettingSlot, parse_mapping, selector::defined_slots};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};
use yaml_serde::Mapping;

/// Where config dependencies are installed, relative to the workspace root.
const CONFIG_DEPENDENCIES_DIR: [&str; 2] = ["node_modules", ".pnpm-config"];

/// Whether the config entry of an included config dependency was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageEntry {
    /// The entry file and its text.
    Loaded { path: PathBuf, text: String },
    /// The config dependency is not installed, so its settings are unknown.
    NotInstalled,
}

/// The installed directory of the config dependency `name`.
#[must_use]
pub fn config_dependency_dir(workspace_dir: &Path, name: &str) -> PathBuf {
    let [modules, config] = CONFIG_DEPENDENCIES_DIR;
    workspace_dir
        .join(modules)
        .join(config)
        .join(name)
}

/// Read the config entry `name`'s `package.json` names as `pnpm.config`.
pub(super) fn read_entry(workspace_dir: &Path, name: &str) -> Result<PackageEntry, IncludeError> {
    let package_dir = config_dependency_dir(workspace_dir, name);
    let manifest = match fs::read_to_string(package_dir.join("package.json")) {
        Ok(manifest) => manifest,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(PackageEntry::NotInstalled),
        Err(source) => return Err(IncludeError::ReadPackage { name: name.to_string(), source }),
    };
    let entry = declared_entry(name, &manifest)?;
    let path = resolve_entry(name, &package_dir, &entry)?;
    let text = fs::read_to_string(&path)
        .map_err(|source| IncludeError::ReadPackage { name: name.to_string(), source })?;
    Ok(PackageEntry::Loaded { path, text })
}

fn declared_entry(name: &str, manifest: &str) -> Result<String, IncludeError> {
    let manifest: serde_json::Value = serde_json::from_str(manifest)
        .map_err(|source| IncludeError::ParsePackageManifest { name: name.to_string(), source })?;
    manifest
        .pointer("/pnpm/config")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| IncludeError::MissingPackageEntry { name: name.to_string() })
}

/// The entry file, which must be a YAML file inside the installed package
/// after following symlinks.
fn resolve_entry(name: &str, package_dir: &Path, entry: &str) -> Result<PathBuf, IncludeError> {
    let invalid = |reason| IncludeError::InvalidPackageEntry {
        name: name.to_string(),
        entry: entry.to_string(),
        reason,
    };
    let relative = Path::new(entry);
    if relative.is_absolute() || entry.contains(':') {
        return Err(invalid("it must be a path relative to the package"));
    }
    let is_yaml = relative
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "yaml" | "yml"));
    if !is_yaml {
        return Err(invalid("it must be a .yaml or .yml file"));
    }
    let read_error = |source| IncludeError::ReadPackage { name: name.to_string(), source };
    let package_root = fs::canonicalize(package_dir).map_err(read_error)?;
    let path = fs::canonicalize(package_dir.join(relative)).map_err(read_error)?;
    if !path.starts_with(&package_root) {
        return Err(invalid("it resolves outside the package"));
    }
    if !path.is_file() {
        return Err(invalid("it is not a file"));
    }
    Ok(path)
}

/// The settings of a config dependency's entry, which may define only the
/// settings delegated to it.
pub(super) fn check_entry(
    name: &str,
    delegated: &[SettingSlot],
    text: &str,
) -> Result<Mapping, IncludeError> {
    let settings = parse_mapping(text)
        .map_err(|source| IncludeError::ParseFragment { spec: name.to_string(), source })?
        .ok_or_else(|| IncludeError::FragmentNotAMapping { spec: name.to_string() })?;
    if settings.contains_key(INCLUDE_KEY) {
        return Err(IncludeError::NestedInclude { spec: name.to_string() });
    }
    for (key, value) in &settings {
        let Some(key) = key.as_str() else {
            continue;
        };
        let undelegated = defined_slots(key, value)
            .into_iter()
            .find(|slot| {
                !delegated
                    .iter()
                    .any(|listed| listed.covers(slot))
            });
        if let Some(slot) = undelegated {
            return Err(IncludeError::SettingNotDelegated {
                setting: slot.to_string(),
                spec: name.to_string(),
            });
        }
    }
    Ok(settings)
}
