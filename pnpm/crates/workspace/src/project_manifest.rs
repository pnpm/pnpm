//! Read a project's package manifest.

pub use pnpm_package_manifest::{ManifestFormat, project_manifest_path};
pub(crate) use pnpm_package_manifest::{PROJECT_MANIFEST_BASENAMES, manifest_format_order};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_package_manifest::{PackageManifest, PackageManifestError};
use std::path::{Path, PathBuf};

/// Error type of [`read_exact_project_manifest`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ReadProjectManifestError {
    #[diagnostic(transparent)]
    Read(#[error(source)] PackageManifestError),

    #[display("Not supported manifest name {basename:?}")]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_UNSUPPORTED_PROJECT_MANIFEST))]
    UnsupportedName { basename: String },
}

/// Error type of [`read_project_manifest_only`] /
/// [`try_read_project_manifest`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ReadProjectManifestOnlyError {
    #[display("No package.json, package.json5, or package.yaml was found in {:?}", project_dir.display())]
    #[diagnostic(code(ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND))]
    NoImporterManifestFound { project_dir: PathBuf },

    #[diagnostic(transparent)]
    Read(#[error(source)] ReadProjectManifestError),
}

/// Read the manifest under `project_dir`, `preferred` format first.
///
/// Returns the manifest plus the basename that was loaded.
pub fn try_read_project_manifest(
    project_dir: &Path,
    preferred: Option<ManifestFormat>,
) -> Result<Option<(&'static str, PackageManifest)>, ReadProjectManifestOnlyError> {
    for format in manifest_format_order(preferred) {
        let basename = format.basename();
        let manifest_path = project_dir.join(basename);
        if manifest_path.is_file() {
            let manifest = read_exact_project_manifest(&manifest_path)
                .map_err(ReadProjectManifestOnlyError::Read)?;
            return Ok(Some((basename, manifest)));
        }
    }
    Ok(None)
}

/// Strict version: error when no manifest is found.
pub fn read_project_manifest_only(
    project_dir: &Path,
    preferred: Option<ManifestFormat>,
) -> Result<PackageManifest, ReadProjectManifestOnlyError> {
    match try_read_project_manifest(project_dir, preferred)? {
        Some((_, manifest)) => Ok(manifest),
        None => Err(ReadProjectManifestOnlyError::NoImporterManifestFound {
            project_dir: project_dir.to_path_buf(),
        }),
    }
}

/// Like [`read_project_manifest_only`] but returns `None` instead of
/// erroring when the manifest is missing.
pub fn safe_read_project_manifest_only(
    project_dir: &Path,
    preferred: Option<ManifestFormat>,
) -> Result<Option<PackageManifest>, ReadProjectManifestOnlyError> {
    Ok(try_read_project_manifest(project_dir, preferred)?.map(|(_, m)| m))
}

/// The `name` the project at `project_dir` declares, if any.
///
/// A missing, unreadable, or nameless manifest all answer `None`:
/// callers want a key to address the project by, not a reason the read
/// failed, and every one of them has a defined answer for a project
/// that has no name.
///
/// Default precedence, never `preferredManifestFormat`: one caller is config
/// layout derivation, which runs before settings are resolved, so honoring the
/// setting here would mean reading config to decide how to read config. A name
/// that disagrees between two coexisting manifests is malformed either way.
pub fn read_project_name(project_dir: &Path) -> Option<String> {
    safe_read_project_manifest_only(project_dir, None)
        .ok()??
        .value()
        .get("name")?
        .as_str()
        .map(str::to_string)
}

/// Read a manifest from an explicit path, probing the basename to pick
/// a parser.
pub fn read_exact_project_manifest(
    manifest_path: &Path,
) -> Result<PackageManifest, ReadProjectManifestError> {
    let basename = manifest_path
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match basename.as_str() {
        "package.json" | "package.json5" | "package.yaml" => {
            PackageManifest::from_path(manifest_path.to_path_buf())
                .map_err(ReadProjectManifestError::Read)
        }
        _ => Err(ReadProjectManifestError::UnsupportedName { basename }),
    }
}

#[cfg(test)]
mod tests;
