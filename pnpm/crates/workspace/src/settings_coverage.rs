use crate::include::{
    IncludeError,
    SCHEMA_DIRECTIVE_KEY,
    parse_mapping,
    read_workspace_manifest_document,
};
use std::path::Path;
use yaml_serde::{
    Mapping,
    Value,
};

#[cfg(test)]
mod tests;

/// Top-level keys of `pnpm-workspace.yaml` that are not settings.
const NON_SETTING_KEYS: &[&str] = &["packages", SCHEMA_DIRECTIVE_KEY];

/// The settings of a workspace root's `pnpm-workspace.yaml`, with its local
/// includes merged in, for checking which nested manifests they cover.
#[derive(Debug, Default)]
pub struct RootSettings(Mapping);

impl RootSettings {
    /// Read the manifest under `root_dir`. A missing manifest has no settings.
    pub fn read(root_dir: &Path) -> Result<Self, IncludeError> {
        match read_settings(root_dir)? {
            ManifestSettings::Known(settings) => Ok(Self(settings)),
            ManifestSettings::Absent | ManifestSettings::Unknown => Ok(Self::default()),
        }
    }

    /// Whether these settings include every setting of the `pnpm-workspace.yaml`
    /// under `nested_dir`, so an install of the outer workspace loses none of
    /// them.
    ///
    /// The nested manifest is read with its local includes merged in.
    /// `packages` and `$schema` are left out. A mapping covers another when it has
    /// each of its keys with a covering value, a list when it holds each of its
    /// items, and any other value when it is equal. A nested manifest that is
    /// not a mapping, or that includes a config dependency that is not
    /// installed, is not covered.
    ///
    /// Returns `true` when `nested_dir` has no manifest.
    pub fn cover(&self, nested_dir: &Path) -> Result<bool, IncludeError> {
        let nested = match read_settings(nested_dir)? {
            ManifestSettings::Absent => return Ok(true),
            ManifestSettings::Unknown => return Ok(false),
            ManifestSettings::Known(settings) => settings,
        };
        Ok(nested
            .iter()
            .filter(|(key, _)| {
                !key.as_str()
                    .is_some_and(|key| NON_SETTING_KEYS.contains(&key))
            })
            .all(|(key, value)| {
                self.0.get(key).is_some_and(|root_value| covers(root_value, value))
            }))
    }
}

enum ManifestSettings {
    Absent,
    /// Not a mapping, or includes a config dependency that is not installed.
    Unknown,
    Known(Mapping),
}

fn read_settings(dir: &Path) -> Result<ManifestSettings, IncludeError> {
    let Some(document) = read_workspace_manifest_document(dir)? else {
        return Ok(ManifestSettings::Absent);
    };
    let includes_unavailable_package = document.includes
        .as_ref()
        .is_some_and(|includes| {
            includes
                .unavailable_packages()
                .next()
                .is_some()
        });
    if includes_unavailable_package {
        return Ok(ManifestSettings::Unknown);
    }
    if document.text.trim().is_empty() {
        return Ok(ManifestSettings::Known(Mapping::new()));
    }
    match parse_mapping(&document.text) {
        Ok(Some(settings)) => Ok(ManifestSettings::Known(settings)),
        Ok(None) => Ok(ManifestSettings::Unknown),
        Err(source) => Err(IncludeError::ParseRoot { path: document.path, source }),
    }
}

fn covers(root: &Value, nested: &Value) -> bool {
    match (root, nested) {
        (Value::Mapping(root), Value::Mapping(nested)) => nested
            .iter()
            .all(|(key, value)| root.get(key).is_some_and(|root_value| covers(root_value, value))),
        (Value::Sequence(root), Value::Sequence(nested)) => nested
            .iter()
            .all(|item| root.contains(item)),
        _ => root == nested,
    }
}
