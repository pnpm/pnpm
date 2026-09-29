//! Read `pnpm-workspace.yaml` into a [`WorkspaceManifest`].
//!
//! Pacquet already has `pnpm_config::WorkspaceSettings` parsing
//! the file for *settings* (`storeDir`, `registry`, ...). That stays the
//! authoritative settings parser; this module is concerned only with
//! the workspace-shape fields (`packages:`, catalogs) that drive
//! project enumeration. Keeping the typed shape separate from settings
//! lets each reader focus on the fields its callers actually need.
//! (`pnpm_config` is not a dependency of this crate, so it is not
//! linked here.)

mod extends;

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_catalogs_types::{Catalog, Catalogs, DEFAULT_CATALOG_NAME};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

/// Basename of the workspace manifest.
pub const WORKSPACE_MANIFEST_FILENAME: &str = "pnpm-workspace.yaml";

/// Subset of `pnpm-workspace.yaml` consumed by project enumeration.
///
/// The settings half (`storeDir`, `registry`, lifecycle policies, ...)
/// is read separately by `pnpm_config::WorkspaceSettings`.
/// Keeping the two readers apart keeps each focused on the shape its
/// callers actually need and avoids a monolithic struct that has to
/// grow with every new pnpm setting.
#[derive(Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceManifest {
    /// Glob patterns identifying the workspace's projects, relative to
    /// the workspace dir.
    ///
    /// `Option` rather than `Vec` so callers can distinguish three
    /// states: `None` (the `packages` key is absent), `Some(vec![])`
    /// (explicit empty array), and `Some(...)` (the user's patterns).
    /// Callers that enumerate a real workspace should pass this through
    /// [`workspace_package_patterns`], while lower-level callers can
    /// still choose the recursive default directly. Collapsing the first
    /// two states would silently lose the difference between omitted and
    /// explicitly-empty `packages`.
    #[serde(default)]
    pub packages: Option<Vec<String>>,

    /// Top-level shorthand for the default catalog. Mutually exclusive
    /// with `catalogs.default` — `pnpm_catalogs_config` enforces
    /// that.
    #[serde(default)]
    pub catalog: Option<Catalog>,

    /// Named catalogs. Includes a `default` key when the user opted for
    /// the explicit form over the top-level [`Self::catalog`] field.
    #[serde(default)]
    pub catalogs: Option<Catalogs>,

    /// Other workspace manifests whose catalogs this one inherits: a
    /// directory holding a `pnpm-workspace.yaml`, the path of one, or a
    /// glob matching several. Relative paths start at this manifest's
    /// directory.
    #[serde(default)]
    pub extends: Option<WorkspaceExtends>,

    /// The catalogs of the manifests [`Self::extends`] names, which
    /// [`read_workspace_manifest`] resolves. The catalogs this manifest
    /// declares itself win over them.
    #[serde(skip)]
    pub inherited_catalogs: Catalogs,
}

/// The `extends` field of `pnpm-workspace.yaml`: one reference or a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WorkspaceExtends {
    One(String),
    Many(Vec<String>),
}

impl WorkspaceExtends {
    /// The references, in the order they are listed.
    #[must_use]
    pub fn entries(&self) -> &[String] {
        match self {
            WorkspaceExtends::One(entry) => std::slice::from_ref(entry),
            WorkspaceExtends::Many(entries) => entries,
        }
    }
}

impl WorkspaceManifest {
    /// The catalogs this manifest declares itself, through `catalog` and
    /// `catalogs`, without the ones it inherits. `catalogs.default` wins
    /// over `catalog`; `pnpm_catalogs_config` rejects a manifest that
    /// declares both.
    #[must_use]
    pub fn declared_catalogs(&self) -> Catalogs {
        let mut catalogs = Catalogs::new();
        if let Some(default) = &self.catalog {
            catalogs.insert(DEFAULT_CATALOG_NAME.to_string(), default.clone());
        }
        if let Some(named) = &self.catalogs {
            catalogs.extend(named.clone());
        }
        catalogs
    }
}

/// Raised when `pnpm-workspace.yaml` parses as YAML but fails a shape
/// check that serde itself can't enforce. Carries pnpm's
/// `invalid_workspace_configuration` error code.
///
/// Note: the "packages field is not an array" case is covered by
/// [`ReadWorkspaceManifestError::ParseYaml`] in pacquet —
/// `serde_saphyr` rejects a non-array shape before this layer runs.
/// Only the empty-string-entry check needs a dedicated variant.
#[derive(Debug, Display, Error, Diagnostic)]
#[diagnostic(code(ERR_PNPM_INVALID_WORKSPACE_CONFIGURATION))]
#[non_exhaustive]
pub enum InvalidWorkspaceManifestError {
    #[display("Missing or empty package")]
    EmptyPackageEntry,
    #[display(r#"The "extends" field lists an empty path"#)]
    EmptyExtendsEntry,
    #[display(r#"Invalid pattern "{pattern}" in the "extends" field: {message}"#)]
    InvalidExtendsPattern { pattern: String, message: String },
}

/// Error type of [`read_workspace_manifest`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ReadWorkspaceManifestError {
    #[display("Failed to read pnpm-workspace.yaml at {}: {source}", path.display())]
    ReadFile {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
    #[display("Failed to parse pnpm-workspace.yaml at {}: {source}", path.display())]
    ParseYaml {
        path: PathBuf,
        #[error(source)]
        source: Box<serde_saphyr::Error>,
    },
    #[diagnostic(transparent)]
    Invalid(#[error(source)] InvalidWorkspaceManifestError),
    #[display(
        r#"Cannot find a pnpm-workspace.yaml file in "{}", which is referenced by the "extends" field of the workspace at "{}""#,
        dir.display(),
        referenced_by.display()
    )]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_EXTENDS_NOT_FOUND))]
    ExtendsNotFound { dir: PathBuf, referenced_by: PathBuf },
    #[display(
        r#"Circular workspace "extends" reference detected. The workspace at "{}" eventually extends itself"#,
        dir.display()
    )]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_EXTENDS_CYCLE))]
    ExtendsCycle { dir: PathBuf },
    #[display(
        "The 'default' catalog was defined multiple times in {}. Use the 'catalog' field or 'catalogs.default', but not both.",
        path.display()
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_CATALOGS_CONFIGURATION))]
    ExtendedDefaultCatalogDefinedTwice { path: PathBuf },
    #[display("Failed to look for workspace manifests matching {pattern}: {message}")]
    WalkExtendsPattern { pattern: String, message: String },
}

/// Resolve `pnpm-workspace.yaml` `packages:` into the workspace package
/// pattern default, falling back to `["."]` when `packages:` is absent.
#[must_use]
pub fn workspace_package_patterns(manifest: &WorkspaceManifest) -> Vec<String> {
    manifest.packages
        .clone()
        .unwrap_or_else(|| vec![".".to_string()])
}

/// Read and validate the `pnpm-workspace.yaml` under `dir`, resolving the
/// catalogs it inherits through `extends` into
/// [`WorkspaceManifest::inherited_catalogs`].
///
/// Returns `Ok(None)` when the file does not exist (`ENOENT` means "no
/// manifest", not an error). Every other read or parse failure
/// propagates.
pub fn read_workspace_manifest(
    dir: &Path,
) -> Result<Option<WorkspaceManifest>, ReadWorkspaceManifestError> {
    let Some(mut manifest) = read_declared_workspace_manifest(dir)? else { return Ok(None) };
    manifest.inherited_catalogs = extends::inherited_catalogs(dir, &manifest)?;
    Ok(Some(manifest))
}

/// [`read_workspace_manifest`] without resolving `extends`.
fn read_declared_workspace_manifest(
    dir: &Path,
) -> Result<Option<WorkspaceManifest>, ReadWorkspaceManifestError> {
    let path = dir.join(WORKSPACE_MANIFEST_FILENAME);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(ReadWorkspaceManifestError::ReadFile { path, source }),
    };
    parse_workspace_manifest(&path, &text).map(Some)
}

/// Parse and validate `text` as a `pnpm-workspace.yaml`. `path` only labels
/// errors.
pub fn parse_workspace_manifest(
    path: &Path,
    text: &str,
) -> Result<WorkspaceManifest, ReadWorkspaceManifestError> {
    // An empty workspace manifest is valid and means "no settings, no
    // packages" — same as `{}`. `serde_saphyr` would otherwise reject
    // an empty document; short-circuit to the default value.
    if text.trim().is_empty() {
        return Ok(WorkspaceManifest::default());
    }

    let manifest: WorkspaceManifest = serde_saphyr::from_str(text)
        .map_err(|source| ReadWorkspaceManifestError::ParseYaml {
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;

    // serde_saphyr already enforces the array shape and string type
    // for `packages:` at deserialization. The remaining invariant —
    // entries cannot be empty strings — needs a manual pass since serde
    // doesn't know about that constraint.
    if let Some(packages) = &manifest.packages {
        for entry in packages {
            if entry.is_empty() {
                return Err(ReadWorkspaceManifestError::Invalid(
                    InvalidWorkspaceManifestError::EmptyPackageEntry,
                ));
            }
        }
    }
    if manifest.extends
        .as_ref()
        .is_some_and(|extends| {
            extends
                .entries()
                .iter()
                .any(String::is_empty)
        })
    {
        return Err(ReadWorkspaceManifestError::Invalid(
            InvalidWorkspaceManifestError::EmptyExtendsEntry,
        ));
    }

    Ok(manifest)
}

#[cfg(test)]
mod tests;
