use crate::{PackageManifestError, serialization::parse_project_manifest};
use serde_json::Value;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Supported project manifests in precedence order.
pub const PROJECT_MANIFEST_BASENAMES: &[&str] = &["package.json", "package.json5", "package.yaml"];

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

/// The manifest of the project enclosing `target` whose `publishConfig.directory`
/// resolves to `target`, or `None` when no ancestor publishes from it.
pub fn find_parent_publish_manifest(target: &Path) -> Result<Option<Value>, PackageManifestError> {
    let normalized_target = pnpm_fs::lexical_normalize(target);
    for parent in normalized_target.ancestors().skip(1) {
        let Some(manifest) = safe_read_project_manifest_from_dir(parent)? else {
            continue;
        };
        let is_publish_dir = manifest
            .get("publishConfig")
            .and_then(|config| config.get("directory"))
            .and_then(Value::as_str)
            .is_some_and(|dir| pnpm_fs::lexical_normalize(&parent.join(dir)) == normalized_target);
        if is_publish_dir {
            return Ok(Some(manifest));
        }
    }
    Ok(None)
}
