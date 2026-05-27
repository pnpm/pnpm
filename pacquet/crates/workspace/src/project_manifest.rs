//! Read a project's `package.json`.
//!
//! Port of upstream's
//! [`readProjectManifest` / `tryReadProjectManifest` / `readExactProjectManifest`](https://github.com/pnpm/pnpm/blob/94240bc046/workspace/project-manifest-reader/src/index.ts).
//!
//! Upstream also supports `package.json5` and `package.yaml` and
//! returns a writer closure that preserves formatting. Pacquet doesn't
//! consume either alternative format yet, and the install pipeline
//! never writes the manifest back — so this port handles `package.json`
//! only and drops the writer closure. Adding the other formats is a
//! follow-up if real workspaces in the wild use them.

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_package_manifest::{PackageManifest, PackageManifestError};
use std::path::{Path, PathBuf};

/// Error type of [`read_exact_project_manifest`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ReadProjectManifestError {
    #[diagnostic(transparent)]
    Read(#[error(source)] PackageManifestError),

    #[display("Not supported manifest name {basename:?}")]
    #[diagnostic(code(pacquet_workspace::unsupported_project_manifest))]
    UnsupportedName { basename: String },
}

/// Error type of [`read_project_manifest_only`] /
/// [`try_read_project_manifest`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ReadProjectManifestOnlyError {
    // Upstream's variant of this message lists `package.yaml` and
    // `package.json5` alongside `package.json`. Pacquet only probes
    // `package.json` today, so the diagnostic mentions just that one —
    // bring back the alternatives when the readers do.
    #[display("No package.json was found in {:?}", project_dir.display())]
    #[diagnostic(code(pacquet_workspace::no_importer_manifest_found))]
    NoImporterManifestFound { project_dir: PathBuf },

    #[diagnostic(transparent)]
    Read(#[error(source)] PackageManifestError),
}

/// Read the manifest under `project_dir`.
///
/// Returns the manifest plus the basename that was loaded. Today the
/// only supported basename is `package.json`; the function exists in
/// its current shape to mirror upstream's tri-format probing so callers
/// can stay structurally the same when `package.json5` / `package.yaml`
/// support lands.
pub fn try_read_project_manifest(
    project_dir: &Path,
) -> Result<Option<(&'static str, PackageManifest)>, ReadProjectManifestOnlyError> {
    let json_path = project_dir.join("package.json");
    if !json_path.is_file() {
        return Ok(None);
    }
    let manifest =
        PackageManifest::from_path(json_path).map_err(ReadProjectManifestOnlyError::Read)?;
    Ok(Some(("package.json", manifest)))
}

/// Strict version: error when no manifest is found. Mirrors upstream's
/// `readProjectManifest`.
pub fn read_project_manifest_only(
    project_dir: &Path,
) -> Result<PackageManifest, ReadProjectManifestOnlyError> {
    match try_read_project_manifest(project_dir)? {
        Some((_, manifest)) => Ok(manifest),
        None => Err(ReadProjectManifestOnlyError::NoImporterManifestFound {
            project_dir: project_dir.to_path_buf(),
        }),
    }
}

/// Like [`read_project_manifest_only`] but returns `None` instead of
/// erroring when the manifest is missing. Mirrors upstream's
/// `safeReadProjectManifestOnly`.
pub fn safe_read_project_manifest_only(
    project_dir: &Path,
) -> Result<Option<PackageManifest>, ReadProjectManifestOnlyError> {
    Ok(try_read_project_manifest(project_dir)?.map(|(_, m)| m))
}

/// Read a manifest from an explicit path. Mirrors upstream's
/// `readExactProjectManifest`, which probes the basename to pick a
/// parser. Pacquet only supports `package.json`; other basenames are
/// rejected with the same wording upstream uses.
pub fn read_exact_project_manifest(
    manifest_path: &Path,
) -> Result<PackageManifest, ReadProjectManifestError> {
    let basename = manifest_path
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match basename.as_str() {
        "package.json" => PackageManifest::from_path(manifest_path.to_path_buf())
            .map_err(ReadProjectManifestError::Read),
        _ => Err(ReadProjectManifestError::UnsupportedName { basename }),
    }
}

#[cfg(test)]
mod tests;
