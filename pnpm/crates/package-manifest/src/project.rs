use crate::{PackageManifestError, serialization::parse_project_manifest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Supported project manifests in precedence order.
pub const PROJECT_MANIFEST_BASENAMES: &[&str] = &["package.json", "package.json5", "package.yaml"];

/// A project manifest's file format.
///
/// The variants are ordered as [`PROJECT_MANIFEST_BASENAMES`] is, so
/// `ManifestFormat::ALL` and that list stay in step.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ManifestFormat {
    #[default]
    Json,
    Json5,
    Yaml,
}

impl ManifestFormat {
    /// Every format, in default precedence order.
    pub const ALL: &'static [ManifestFormat] =
        &[ManifestFormat::Json, ManifestFormat::Json5, ManifestFormat::Yaml];

    /// The manifest file name this format is read from and written to.
    #[must_use]
    pub fn basename(self) -> &'static str {
        match self {
            ManifestFormat::Json => "package.json",
            ManifestFormat::Json5 => "package.json5",
            ManifestFormat::Yaml => "package.yaml",
        }
    }

    /// The format `basename` names, or `None` when it names no manifest.
    #[must_use]
    pub fn from_basename(basename: &str) -> Option<Self> {
        ManifestFormat::ALL
            .iter()
            .copied()
            .find(|format| format.basename() == basename)
    }
}

/// The order project manifests are tried in, `preferred` first.
///
/// A preference only reorders: a directory that does not hold the preferred
/// format still resolves through the remaining formats in default order,
/// so setting one never makes a project stop being found.
#[must_use]
pub fn manifest_format_order(preferred: Option<ManifestFormat>) -> Vec<ManifestFormat> {
    let Some(preferred) = preferred else {
        return ManifestFormat::ALL.to_vec();
    };
    std::iter::once(preferred)
        .chain(
            ManifestFormat::ALL
                .iter()
                .copied()
                .filter(|format| *format != preferred),
        )
        .collect()
}

/// The basenames project manifests are tried in, `preferred` first.
#[must_use]
pub fn project_manifest_basenames(preferred: Option<ManifestFormat>) -> Vec<&'static str> {
    manifest_format_order(preferred)
        .into_iter()
        .map(ManifestFormat::basename)
        .collect()
}

/// Select the existing project manifest, or the path for a new project.
///
/// A project that has no manifest yet is given the preferred format, so the
/// setting governs the files pnpm creates as well as the ones it picks
/// between.
#[must_use]
pub fn project_manifest_path(project_dir: &Path, preferred: Option<ManifestFormat>) -> PathBuf {
    let order = manifest_format_order(preferred);
    order
        .iter()
        .map(|format| project_dir.join(format.basename()))
        .find(|path| match fs::metadata(path) {
            Ok(_) => true,
            Err(error) => error.kind() != io::ErrorKind::NotFound,
        })
        .unwrap_or_else(|| project_dir.join(order[0].basename()))
}

/// Read an unmodified project manifest in precedence order. Only missing files
/// are skipped; an unreadable or invalid preferred manifest is an error.
pub fn safe_read_project_manifest_from_dir(
    dir: &Path,
    preferred: Option<ManifestFormat>,
) -> Result<Option<Value>, PackageManifestError> {
    for basename in project_manifest_basenames(preferred) {
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
