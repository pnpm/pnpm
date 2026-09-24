use crate::{PackageManifestError, serialization::parse_project_manifest};
use serde_json::Value;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Supported project manifests in precedence order.
pub const PROJECT_MANIFEST_BASENAMES: &[&str] =
    &["package.json", "package.json5", "package.yaml", "package.yml"];

/// Select the existing project manifest, or the JSON path for a new project.
#[must_use]
pub fn project_manifest_path(project_dir: &Path) -> PathBuf {
    PROJECT_MANIFEST_BASENAMES
        .iter()
        .map(|basename| project_dir.join(basename))
        .find(|path| match fs::metadata(path) {
            Ok(_) => true,
            Err(error) => error.kind() != io::ErrorKind::NotFound,
        })
        .unwrap_or_else(|| project_dir.join("package.json"))
}

/// Read an unmodified project manifest in precedence order. Only missing files
/// are skipped; an unreadable or invalid preferred manifest is an error.
pub fn safe_read_project_manifest_from_dir(
    dir: &Path,
) -> Result<Option<Value>, PackageManifestError> {
    for basename in PROJECT_MANIFEST_BASENAMES {
        let path = dir.join(basename);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => return Err(PackageManifestError::Read { path, source }),
        };
        return parse_project_manifest(&path, &text).map(Some);
    }
    Ok(None)
}
