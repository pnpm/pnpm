use super::{
    PackageManifestError,
    serialization::{
        ManifestFormatting, invalid_root_error, map_not_found_to_missing_manifest,
        read_raw_project_manifest,
    },
};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Unnormalized `package.json`, `package.json5`, or `package.yaml` manifest
/// and its source formatting.
///
/// Unlike [`crate::PackageManifest`], [`RawPackageManifest`] does not fold
/// `engines.runtime` or `devEngines.runtime` into dependency maps and does
/// not sort or prune empty dependency objects on [`Self::save`].
#[derive(Clone)]
pub struct RawPackageManifest {
    path: PathBuf,
    value: Value,
    formatting: ManifestFormatting,
    on_disk: Value,
}

impl RawPackageManifest {
    fn read_from_file(path: PathBuf) -> Result<Self, PackageManifestError> {
        let (value, formatting) = read_raw_project_manifest(&path)?;
        let on_disk = value.clone();
        Ok(Self { path, value, formatting, on_disk })
    }

    pub fn from_path(path: PathBuf) -> Result<Self, PackageManifestError> {
        let rendered_path = path.display().to_string();
        map_not_found_to_missing_manifest(rendered_path, Self::read_from_file(path))
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn value_mut(&mut self) -> &mut Value {
        &mut self.value
    }

    /// Persist the raw manifest in its source file's format and style.
    ///
    /// Skips writing when neither values nor object key orders changed.
    pub fn save(&mut self) -> Result<(), PackageManifestError> {
        if !self.value.is_object() {
            return Err(invalid_root_error(&self.path));
        }
        if values_identical(&self.on_disk, &self.value) {
            return Ok(());
        }
        self.formatting.write(&self.path, &self.value)?;
        self.on_disk = self.value.clone();
        Ok(())
    }
}

/// Compare two JSON values while treating object key order as significant.
///
/// `serde_json::Map` delegates `PartialEq` to `IndexMap`, which ignores entry
/// order, so plain `==` would treat a key-reordering edit as a no-op.
fn values_identical(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Array(left_items), Value::Array(right_items)) => {
            left_items.len() == right_items.len()
                && left_items
                    .iter()
                    .zip(right_items.iter())
                    .all(|(left_item, right_item)| values_identical(left_item, right_item))
        }
        (Value::Object(left_entries), Value::Object(right_entries)) => {
            left_entries.len() == right_entries.len()
                && left_entries
                    .iter()
                    .zip(right_entries.iter())
                    .all(|((left_key, left_val), (right_key, right_val))| {
                        left_key == right_key && values_identical(left_val, right_val)
                    })
        }
        _ => left == right,
    }
}
