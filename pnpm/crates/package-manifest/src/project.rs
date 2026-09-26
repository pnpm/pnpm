use crate::{PackageManifestError, serialization::parse_project_manifest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Supported project manifests in default precedence order.
pub const PROJECT_MANIFEST_BASENAMES: &[&str] = &["package.json", "package.json5", "package.yaml"];

/// A project manifest format, as named by the `preferredManifestFormat`
/// setting.
///
/// When several project manifests coexist in one directory, the preferred
/// format is read first and the others follow in default precedence order.
/// The default, [`ManifestFormat::Json`], yields the default order.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ManifestFormat {
    /// `package.json`.
    #[default]
    Json,
    /// `package.json5`.
    Json5,
    /// `package.yaml`.
    Yaml,
}

impl ManifestFormat {
    /// The manifest file name of this format.
    #[must_use]
    pub const fn basename(self) -> &'static str {
        match self {
            ManifestFormat::Json => "package.json",
            ManifestFormat::Json5 => "package.json5",
            ManifestFormat::Yaml => "package.yaml",
        }
    }

    /// Every project manifest basename, this format's first and the rest in
    /// default precedence order.
    pub fn precedence(self) -> impl Iterator<Item = &'static str> {
        let preferred = self.basename();
        std::iter::once(preferred)
            .chain(
                PROJECT_MANIFEST_BASENAMES
                    .iter()
                    .copied()
                    .filter(move |name| *name != preferred),
            )
    }
}

/// Select the existing project manifest in `preferred`'s precedence order,
/// or the JSON path for a new project.
#[must_use]
pub fn project_manifest_path(project_dir: &Path, preferred: ManifestFormat) -> PathBuf {
    preferred
        .precedence()
        .map(|basename| project_dir.join(basename))
        .find(|path| match fs::metadata(path) {
            Ok(_) => true,
            Err(error) => error.kind() != io::ErrorKind::NotFound,
        })
        .unwrap_or_else(|| project_dir.join("package.json"))
}

/// Read an unmodified project manifest in `preferred`'s precedence order.
/// Only missing files are skipped; an unreadable or invalid selected manifest
/// is an error.
pub fn safe_read_project_manifest_from_dir(
    dir: &Path,
    preferred: ManifestFormat,
) -> Result<Option<Value>, PackageManifestError> {
    for basename in preferred.precedence() {
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
///
/// This serves local dependencies, so each ancestor's manifest is picked in
/// the default order.
pub fn find_parent_publish_manifest(target: &Path) -> Result<Option<Value>, PackageManifestError> {
    let normalized_target = pnpm_fs::lexical_normalize(target);
    for parent in normalized_target.ancestors().skip(1) {
        let Some(manifest) =
            safe_read_project_manifest_from_dir(parent, ManifestFormat::default())?
        else {
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
