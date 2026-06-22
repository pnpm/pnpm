//! Format-preserving writer for `pnpm-workspace.yaml`'s catalog blocks.
//!
//! Pacquet port of the catalog-relevant half of pnpm's
//! [`updateWorkspaceManifest`](https://github.com/pnpm/pnpm/blob/e7e99f04e4/workspace/workspace-manifest-writer/src/index.ts):
//! given a set of `updatedCatalogs`, merge them into the `catalog:` /
//! `catalogs:` blocks of an existing `pnpm-workspace.yaml` (or create the
//! file) while preserving the comments, blank lines, key order, and quote
//! styles of everything it does not touch.
//!
//! pnpm reaches that fidelity with eemeli/yaml's mutable Document AST plus
//! its own [`reorderRecursive`] / [`propagateBlankLinesToNewPairs`] passes.
//! Pacquet has no equivalent AST, so the format-preserving edits are
//! expressed as targeted text splices (for inserts) and
//! [`yamlpatch`] `Op::Replace` (for value updates) — which suffices because
//! `updatedCatalogs` only ever *inserts* new entries/blocks or *updates* a
//! single value, never reorders existing content.
//!
//! [`reorderRecursive`]: https://github.com/pnpm/pnpm/blob/e7e99f04e4/workspace/workspace-manifest-writer/src/index.ts#L290-L313
//! [`propagateBlankLinesToNewPairs`]: https://github.com/pnpm/pnpm/blob/e7e99f04e4/workspace/workspace-manifest-writer/src/index.ts#L347-L385

use std::{fs, io, path::Path};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_catalogs_types::Catalogs;

mod edit;
mod model;
mod render;

#[cfg(test)]
mod tests;

use model::Manifest;

/// Base name of pnpm's workspace manifest, matching pnpm's
/// [`WORKSPACE_MANIFEST_FILENAME`](https://github.com/pnpm/pnpm/blob/e7e99f04e4/packages/constants/src/index.ts).
pub const WORKSPACE_MANIFEST_FILENAME: &str = "pnpm-workspace.yaml";

/// Error raised while reading, editing, or writing `pnpm-workspace.yaml`.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum UpdateWorkspaceManifestError {
    #[display("Failed to read {path:?}: {source}")]
    #[diagnostic(code(pacquet_workspace_manifest_writer::read))]
    Read {
        path: std::path::PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to parse {path:?} as YAML: {source}")]
    #[diagnostic(code(pacquet_workspace_manifest_writer::parse))]
    Parse {
        path: std::path::PathBuf,
        #[error(source)]
        source: Box<serde_saphyr::Error>,
    },

    #[display("Failed to apply a YAML edit to {path:?}: {source}")]
    #[diagnostic(code(pacquet_workspace_manifest_writer::edit))]
    Edit {
        path: std::path::PathBuf,
        #[error(source)]
        source: Box<yamlpatch::Error>,
    },

    #[display("Failed to write {path:?}: {source}")]
    #[diagnostic(code(pacquet_workspace_manifest_writer::write))]
    Write {
        path: std::path::PathBuf,
        #[error(source)]
        source: io::Error,
    },
}

/// Merge `updated_catalogs` into `dir`'s `pnpm-workspace.yaml`, writing the
/// file back only when something actually changed.
pub fn update_workspace_manifest(
    dir: &Path,
    updated_catalogs: &Catalogs,
) -> Result<(), UpdateWorkspaceManifestError> {
    let path = dir.join(WORKSPACE_MANIFEST_FILENAME);

    let original = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(source) => return Err(UpdateWorkspaceManifestError::Read { path, source }),
    };

    let mut manifest = Manifest::parse(original.as_deref())
        .map_err(|source| UpdateWorkspaceManifestError::Parse { path: path.clone(), source })?;

    let changed = edit::add_catalogs(&mut manifest, updated_catalogs)
        .map_err(|source| UpdateWorkspaceManifestError::Edit { path: path.clone(), source })?;
    if !changed {
        return Ok(());
    }

    fs::write(&path, manifest.into_text())
        .map_err(|source| UpdateWorkspaceManifestError::Write { path, source })
}

/// Write a `name → specifier` entry into `dir`'s `pnpm-workspace.yaml`
/// `configDependencies:` block (creating the file/block if absent),
/// preserving the rest of the document's formatting. Used by
/// `pnpm add --config`; the resolved integrity is recorded separately in
/// the env lockfile, so only the clean specifier is written here.
pub fn set_config_dependency(
    dir: &Path,
    name: &str,
    specifier: &str,
) -> Result<(), UpdateWorkspaceManifestError> {
    let path = dir.join(WORKSPACE_MANIFEST_FILENAME);

    let original = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(source) => return Err(UpdateWorkspaceManifestError::Read { path, source }),
    };

    let mut manifest = Manifest::parse(original.as_deref())
        .map_err(|source| UpdateWorkspaceManifestError::Parse { path: path.clone(), source })?;

    let changed = edit::add_config_dependency(&mut manifest, name, specifier)
        .map_err(|source| UpdateWorkspaceManifestError::Edit { path: path.clone(), source })?;
    if !changed {
        return Ok(());
    }

    fs::write(&path, manifest.into_text())
        .map_err(|source| UpdateWorkspaceManifestError::Write { path, source })
}

/// Upsert `name → bool` entries into `dir`'s `pnpm-workspace.yaml`
/// `allowBuilds:` block (creating the file/block if absent), preserving the
/// rest of the document's formatting, and write the file back only when
/// something actually changed. Used by `pnpm approve-builds` to record
/// which dependencies may (`true`) or may not (`false`) run build scripts.
///
/// `entries` is iterated in its own order; pass an ordered map for a
/// deterministic result.
pub fn set_allow_builds<'a, I>(dir: &Path, entries: I) -> Result<(), UpdateWorkspaceManifestError>
where
    I: IntoIterator<Item = (&'a str, bool)>,
{
    let path = dir.join(WORKSPACE_MANIFEST_FILENAME);

    let original = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(source) => return Err(UpdateWorkspaceManifestError::Read { path, source }),
    };

    let mut manifest = Manifest::parse(original.as_deref())
        .map_err(|source| UpdateWorkspaceManifestError::Parse { path: path.clone(), source })?;

    let mut changed = false;
    for (name, value) in entries {
        changed |= edit::add_allow_build(&mut manifest, name, value)
            .map_err(|source| UpdateWorkspaceManifestError::Edit { path: path.clone(), source })?;
    }
    if !changed {
        return Ok(());
    }

    fs::write(&path, manifest.into_text())
        .map_err(|source| UpdateWorkspaceManifestError::Write { path, source })
}
