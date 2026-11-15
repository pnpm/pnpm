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

use crate::include::{IncludeError, read_workspace_manifest_document};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_catalogs_types::{Catalog, Catalogs};
use pnpm_env_replace::SentinelPlaceholders;
use serde::Deserialize;
use std::{
    io,
    path::{Path, PathBuf},
};

/// Basename of the workspace manifest.
pub const WORKSPACE_MANIFEST_FILENAME: &str = "pnpm-workspace.yaml";

/// Subset of `pnpm-workspace.yaml` consumed by project enumeration. The
/// settings half (`storeDir`, `registry`, lifecycle policies, ...) is read
/// separately by `pnpm_config::WorkspaceSettings`.
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
    #[diagnostic(transparent)]
    Include(#[error(source)] IncludeError),
}

/// Resolve `pnpm-workspace.yaml` `packages:` into the workspace package
/// pattern default, falling back to `["."]` when `packages:` is absent.
#[must_use]
pub fn workspace_package_patterns(manifest: &WorkspaceManifest) -> Vec<String> {
    manifest.packages
        .clone()
        .unwrap_or_else(|| vec![".".to_string()])
}

/// Read and validate the `pnpm-workspace.yaml` under `dir`, with the files it
/// includes merged in.
///
/// Returns `Ok(None)` when the file does not exist (`ENOENT` means "no
/// manifest", not an error). Every other read or parse failure
/// propagates.
pub fn read_workspace_manifest(
    dir: &Path,
) -> Result<Option<WorkspaceManifest>, ReadWorkspaceManifestError> {
    let document = match read_workspace_manifest_document(dir) {
        Ok(Some(document)) => document,
        Ok(None) => return Ok(None),
        Err(IncludeError::ReadRoot { path, source }) => {
            return Err(ReadWorkspaceManifestError::ReadFile { path, source });
        }
        Err(error) => return Err(ReadWorkspaceManifestError::Include(error)),
    };
    parse_workspace_manifest(&document.path, &document.text).map(Some)
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

    let manifest: WorkspaceManifest =
        match serde_saphyr::from_str::<Option<WorkspaceManifest>>(text) {
            Ok(manifest) => manifest.unwrap_or_default(),
            Err(raw_error) => parse_manifest_preserving_placeholders(text)
                .ok_or_else(|| ReadWorkspaceManifestError::ParseYaml {
                    path: path.to_path_buf(),
                    source: Box::new(raw_error),
                })?,
        };

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

    Ok(manifest)
}

fn parse_manifest_preserving_placeholders(text: &str) -> Option<WorkspaceManifest> {
    let sentinels = SentinelPlaceholders::new(text)?;
    let mut manifest =
        serde_saphyr::from_str::<Option<WorkspaceManifest>>(&sentinels.replace_all(text))
            .ok()?
            .unwrap_or_default();
    restore_manifest_placeholders(&sentinels, text, &mut manifest).then_some(manifest)
}

fn restore_manifest_placeholders(
    sentinels: &SentinelPlaceholders,
    original_text: &str,
    manifest: &mut WorkspaceManifest,
) -> bool {
    manifest.packages
        .iter_mut()
        .flatten()
        .all(|entry| sentinels.try_restore_in_place(original_text, entry))
        && manifest.catalog
            .as_mut()
            .is_none_or(|catalog| restore_catalog_placeholders(sentinels, original_text, catalog))
        && manifest.catalogs
            .as_mut()
            .is_none_or(|catalogs| {
                restore_catalogs_placeholders(sentinels, original_text, catalogs)
            })
}

fn restore_catalogs_placeholders(
    sentinels: &SentinelPlaceholders,
    original_text: &str,
    catalogs: &mut Catalogs,
) -> bool {
    let mut restored = Catalogs::new();
    for (mut name, mut catalog) in std::mem::take(catalogs) {
        if !sentinels.try_restore_in_place(original_text, &mut name)
            || !restore_catalog_placeholders(sentinels, original_text, &mut catalog)
            || restored.insert(name, catalog).is_some()
        {
            return false;
        }
    }
    *catalogs = restored;
    true
}

fn restore_catalog_placeholders(
    sentinels: &SentinelPlaceholders,
    original_text: &str,
    catalog: &mut Catalog,
) -> bool {
    let mut restored = Catalog::new();
    for (mut name, mut specifier) in std::mem::take(catalog) {
        if !sentinels.try_restore_in_place(original_text, &mut name)
            || !sentinels.try_restore_in_place(original_text, &mut specifier)
            || restored.insert(name, specifier).is_some()
        {
            return false;
        }
    }
    *catalog = restored;
    true
}

#[cfg(test)]
mod tests;
