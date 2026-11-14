use std::{fs, path::Path};

use derive_more::{Display, Error};
use miette::Diagnostic;

use crate::{
    Manifest, ManifestTarget, UpdateWorkspaceManifestError, WorkspaceManifests, edit,
    read_if_present, remove_manifest, write_atomic,
};

/// Set or delete an arbitrary top-level field in the YAML manifest at `path`
/// (a `pnpm-workspace.yaml` or a global `config.yaml`), preserving the rest of
/// the document's formatting and writing back only when something changed.
///
/// A `null` `value` deletes the key; any other value sets it. When the
/// edit empties the document, the file is removed. Used by `pnpm config set` /
/// `pnpm config delete` for the keys routed to a YAML config file.
pub fn update_manifest_field(
    path: &Path,
    key: &str,
    value: &serde_json::Value,
) -> Result<(), UpdateWorkspaceManifestError> {
    let target = ManifestTarget::root_manifest(path);
    update_target_field(&target, key, value, WritePrivacy::InheritOrUmask)
}

/// [`update_manifest_field`] with owner-only (`0600`) permissions, tightening
/// even an existing readable file. Used when `pnpm config set --global` writes
/// sections that may carry signing keys.
pub fn update_private_manifest_field(
    path: &Path,
    key: &str,
    value: &serde_json::Value,
) -> Result<(), UpdateWorkspaceManifestError> {
    let target = ManifestTarget::root_manifest(path);
    update_target_field(&target, key, value, WritePrivacy::OwnerOnly)
}

/// [`update_manifest_field`] for a workspace setting of `dir`, written to the
/// file of the workspace that owns `key`: the root `pnpm-workspace.yaml`, or a
/// file it includes. A file an edit empties is removed only when it is the
/// root manifest.
pub fn update_workspace_setting(
    dir: &Path,
    key: &str,
    value: &serde_json::Value,
) -> Result<(), UpdateWorkspaceManifestError> {
    let target = WorkspaceManifests::read(dir)?.target_for_setting(key)?;
    update_target_field(
        &target,
        key,
        &target.setting_from(dir, key, value),
        WritePrivacy::InheritOrUmask,
    )
}

#[derive(Clone, Copy)]
enum WritePrivacy {
    InheritOrUmask,
    OwnerOnly,
}

fn update_target_field(
    target: &ManifestTarget,
    key: &str,
    value: &serde_json::Value,
    privacy: WritePrivacy,
) -> Result<(), UpdateWorkspaceManifestError> {
    let path = target.path.as_path();
    let original = read_if_present(path)?;
    let edit = edit_manifest_field(original.as_deref(), key, value)
        .map_err(|error| map_edit_field_error(path, error))?;

    let text = match edit {
        ManifestEdit::Unchanged => {
            if let (WritePrivacy::OwnerOnly, Some(existing)) = (privacy, original.as_deref()) {
                pnpm_fs::write_atomic_private(path, existing.as_bytes())
                    .map_err(|source| UpdateWorkspaceManifestError::Write {
                        path: path.to_path_buf(),
                        source,
                    })?;
            }
            return Ok(());
        }
        ManifestEdit::Remove if !target.included => return remove_manifest(path),
        ManifestEdit::Remove => String::new(),
        ManifestEdit::Write(text) => text,
    };

    write_target_text(path, !value.is_null(), &text, privacy)
}

fn map_edit_field_error(
    path: &Path,
    error: EditManifestFieldError,
) -> UpdateWorkspaceManifestError {
    match error {
        EditManifestFieldError::Edit(source) => {
            UpdateWorkspaceManifestError::Edit { path: path.to_path_buf(), source }
        }
        EditManifestFieldError::Parse { source } => {
            UpdateWorkspaceManifestError::Parse { path: path.to_path_buf(), source }
        }
        EditManifestFieldError::UnsupportedInlineBlock { key } => {
            UpdateWorkspaceManifestError::UnsupportedInlineBlock { path: path.to_path_buf(), key }
        }
    }
}

fn write_target_text(
    path: &Path,
    ensure_parent_dir: bool,
    text: &str,
    privacy: WritePrivacy,
) -> Result<(), UpdateWorkspaceManifestError> {
    // A `set` may target a config directory that does not exist yet
    // (`pnpm config set --global`). Create the directory recursively before
    // the write; a `delete` never needs it (the file, hence its parent,
    // already exists).
    if ensure_parent_dir
        && let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|source| UpdateWorkspaceManifestError::Write {
                path: path.to_path_buf(),
                source,
            })?;
    }

    let write_result = match privacy {
        WritePrivacy::InheritOrUmask => write_atomic(path, text),
        WritePrivacy::OwnerOnly => pnpm_fs::write_atomic_private(path, text.as_bytes()),
    };
    write_result.map_err(|source| UpdateWorkspaceManifestError::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// What [`edit_manifest_field`] leaves the caller to do with the file the
/// `original` text came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestEdit {
    /// The document already says what the edit wanted it to say.
    Unchanged,
    /// Replace the file's contents with this text.
    Write(String),
    /// The edit left no keys behind, so the file should go with them.
    Remove,
}

/// Failure to apply [`edit_manifest_field`], for the caller to pair with the
/// path it read.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum EditManifestFieldError {
    #[display("Failed to edit the document as YAML: {_0}")]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_MANIFEST_WRITER_EDIT))]
    Edit(#[error(source)] Box<yamlpatch::Error>),

    #[display("Failed to parse the document as YAML: {source}")]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_MANIFEST_WRITER_PARSE))]
    Parse {
        #[error(source)]
        source: Box<serde_saphyr::Error>,
    },

    #[display(
        "Cannot edit {key:?}: the document uses an inline YAML value that cannot be edited in place (a multi-line flow collection, an alias, or a scalar). Reformat it to block style and try again."
    )]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_MANIFEST_WRITER_UNSUPPORTED_INLINE_BLOCK))]
    UnsupportedInlineBlock { key: String },
}

/// Set or delete a top-level field of the YAML document `original`, returning
/// the text to write back rather than touching the filesystem, so a caller that
/// owns its own I/O — `pnpm login` writing through its capability seam — can
/// reuse the format-preserving edit. [`update_manifest_field`] is this function
/// plus the read and the atomic write.
///
/// A `null` `value` deletes the key; any other value sets it. `None` `original`
/// is a document that does not exist yet.
pub fn edit_manifest_field(
    original: Option<&str>,
    key: &str,
    value: &serde_json::Value,
) -> Result<ManifestEdit, EditManifestFieldError> {
    let mut manifest =
        Manifest::parse(original).map_err(|source| EditManifestFieldError::Parse { source })?;

    if edit::document_root_is_inline(manifest.document.text()) {
        return Err(EditManifestFieldError::UnsupportedInlineBlock { key: key.to_string() });
    }

    let changed = if value.is_null() {
        edit::remove_top_level_field(&mut manifest, key)
    } else {
        edit::set_top_level_field(&mut manifest, key, value)
    };
    if !changed {
        return Ok(ManifestEdit::Unchanged);
    }
    if manifest.document.keys.is_empty() {
        return Ok(ManifestEdit::Remove);
    }
    Ok(ManifestEdit::Write(manifest.document.into_text().map_err(EditManifestFieldError::Edit)?))
}
