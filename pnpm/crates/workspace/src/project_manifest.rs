//! Read a project's package manifest.
//!
//! pnpm also supports `package.json5` and returns a writer closure that
//! preserves formatting. Pacquet does not consume JSON5 yet, and the
//! install pipeline never writes the manifest back through this reader,
//! so this handles `package.json` plus read-only `package.yaml`.

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_package_manifest::{PackageManifest, PackageManifestError};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Error type of [`read_exact_project_manifest`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ReadProjectManifestError {
    #[diagnostic(transparent)]
    Read(#[error(source)] PackageManifestError),

    #[display("Failed to read {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_READ_PROJECT_MANIFEST))]
    ReadFile {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to parse {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_PARSE_PROJECT_MANIFEST))]
    ParseYaml {
        path: PathBuf,
        #[error(source)]
        source: Box<serde_saphyr::Error>,
    },

    #[display("Not supported manifest name {basename:?}")]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_UNSUPPORTED_PROJECT_MANIFEST))]
    UnsupportedName { basename: String },
}

/// Error type of [`read_project_manifest_only`] /
/// [`try_read_project_manifest`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ReadProjectManifestOnlyError {
    #[display("No package.json or package.yaml was found in {:?}", project_dir.display())]
    #[diagnostic(code(ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND))]
    NoImporterManifestFound { project_dir: PathBuf },

    #[diagnostic(transparent)]
    Read(#[error(source)] ReadProjectManifestError),
}

/// Read the manifest under `project_dir`.
///
/// Returns the manifest plus the basename that was loaded.
pub fn try_read_project_manifest(
    project_dir: &Path,
) -> Result<Option<(&'static str, PackageManifest)>, ReadProjectManifestOnlyError> {
    for basename in ["package.json", "package.yaml"] {
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
) -> Result<PackageManifest, ReadProjectManifestOnlyError> {
    match try_read_project_manifest(project_dir)? {
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
) -> Result<Option<PackageManifest>, ReadProjectManifestOnlyError> {
    Ok(try_read_project_manifest(project_dir)?.map(|(_, m)| m))
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
        "package.json" => PackageManifest::from_path(manifest_path.to_path_buf())
            .map_err(ReadProjectManifestError::Read),
        "package.yaml" => read_package_yaml(manifest_path),
        _ => Err(ReadProjectManifestError::UnsupportedName { basename }),
    }
}

fn read_package_yaml(path: &Path) -> Result<PackageManifest, ReadProjectManifestError> {
    let text = fs::read_to_string(path).map_err(|source| ReadProjectManifestError::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;
    let value = serde_saphyr::from_str(&text).map_err(|source| {
        ReadProjectManifestError::ParseYaml { path: path.to_path_buf(), source: Box::new(source) }
    })?;
    Ok(PackageManifest::from_value(path.to_path_buf(), value))
}

#[cfg(test)]
mod tests;
