#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

//! Format-preserving writer for `pnpm-workspace.yaml`'s catalog blocks.
//!
//! Given a set of updated catalogs, merge them into the `catalog:` /
//! `catalogs:` blocks of an existing `pnpm-workspace.yaml` (or create the
//! file) while preserving the comments, blank lines, key order, and quote
//! styles of everything it does not touch.

pub use build_settings::{
    LEGACY_BUILD_SETTINGS, UNDECIDED_ALLOW_BUILD, scaffold_allow_builds, set_allow_builds,
    set_allow_builds_clearing_legacy, set_permissions,
};
pub use catalog_references::CatalogReferenceSources;
pub use pnpm_config::version_policy::ResolvedPackageVersions;
pub use version_policies::{
    remove_overrides, set_audit_ignore_ghsas, set_config_dependencies,
    set_minimum_release_age_excludes, set_overrides, set_patched_dependencies,
};

use std::{
    fs,
    io::{self, Write as _},
    path::Path,
};

use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::Diagnostic;
use pnpm_catalogs_types::Catalogs;

mod edit;
mod flow;
mod model;
mod render;

#[cfg(test)]
mod tests;

use model::Manifest;
use routing::{ManifestTarget, WorkspaceManifests, read_owning_manifest};

/// Base name of pnpm's workspace manifest.
pub const WORKSPACE_MANIFEST_FILENAME: &str = "pnpm-workspace.yaml";

/// Error raised while reading, editing, or writing `pnpm-workspace.yaml`.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum UpdateWorkspaceManifestError {
    #[display("Failed to read {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_MANIFEST_WRITER_READ))]
    Read {
        path: std::path::PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to parse {path:?} as YAML: {source}")]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_MANIFEST_WRITER_PARSE))]
    Parse {
        path: std::path::PathBuf,
        #[error(source)]
        source: Box<serde_saphyr::Error>,
    },

    #[display("Failed to apply a YAML edit to {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_MANIFEST_WRITER_EDIT))]
    Edit {
        path: std::path::PathBuf,
        #[error(source)]
        source: Box<yamlpatch::Error>,
    },

    #[display("Failed to write {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_MANIFEST_WRITER_WRITE))]
    Write {
        path: std::path::PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to remove {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_MANIFEST_WRITER_REMOVE))]
    Remove {
        path: std::path::PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display(
        "Cannot write the override for {key:?} in {path:?}: it already has a non-string value (a parent-scoped object). Resolve it manually."
    )]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_MANIFEST_WRITER_OVERRIDE_CONFLICT))]
    OverrideConflict { path: std::path::PathBuf, key: String },

    #[display(
        "Cannot edit {key:?} in {path:?}: it uses an inline YAML value that cannot be edited in place (a multi-line flow collection, an alias, or a scalar). Reformat it to block style and try again."
    )]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_MANIFEST_WRITER_UNSUPPORTED_INLINE_BLOCK))]
    UnsupportedInlineBlock { path: std::path::PathBuf, key: String },

    #[display(
        "Cannot write {value:?} to {path:?}: it contains a control character that would corrupt the YAML."
    )]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_MANIFEST_WRITER_INVALID_CONTROL_CHARACTER))]
    InvalidControlCharacter { path: std::path::PathBuf, value: String },

    #[diagnostic(transparent)]
    VersionPolicy(#[error(source)] pnpm_config::version_policy::VersionPolicyError),

    #[diagnostic(transparent)]
    Include(#[error(source)] pnpm_workspace::IncludeError),

    #[display(r#""include" cannot be changed by this command. Edit pnpm-workspace.yaml directly."#)]
    #[diagnostic(code(ERR_PNPM_INCLUDE_NOT_WRITABLE))]
    IncludeNotWritable,

    #[display(r#"The catalogs are owned by more than one file, so "catalogs" cannot be written as a whole. Edit the files that own the catalogs."#)]
    #[diagnostic(code(ERR_PNPM_CATALOGS_SPLIT_ACROSS_FILES))]
    CatalogsSplitAcrossFiles,

    #[display(r#""{setting}" is delegated to {name}. Change it in that package and update the config dependency."#)]
    #[diagnostic(code(ERR_PNPM_SETTING_OWNED_BY_CONFIG_DEPENDENCY))]
    OwnedByConfigDependency { setting: String, name: String },
}

/// Whether `value` holds a character YAML treats as a line break: a
/// control character (newline, carriage return, ...) or one of the Unicode
/// line/paragraph separators, which are not in the control category.
///
/// The block-style writers splice `value` into a single `key: value` /
/// `- item` line. A control character forces a multi-line scalar and
/// corrupts the document outright; a separator is subtler — the emitter
/// folds the scalar and the parser reads back the folding indentation as
/// part of the value, so the write silently succeeds with a mangled
/// value. The values these writers handle (GHSA ids, version-policy
/// specs, override selectors/specifiers, catalog names) never
/// legitimately contain either.
fn has_control_char(value: &str) -> bool {
    value
        .chars()
        .any(|character| character.is_control() || matches!(character, '\u{2028}' | '\u{2029}'))
}

/// The first of `paths` whose value is written as an inline shape none of
/// the writers can edit, named for the error message. A single-line flow
/// collection is editable and never reported here; a multi-line one, an
/// alias, or a scalar standing where a collection belongs is.
fn unsupported_inline_key(text: &str, paths: &[&[&str]]) -> Option<String> {
    paths
        .iter()
        .find(|path| edit::has_unsupported_inline_value(text, path))
        .map(|path| path.join("."))
}

/// Inputs of [`update_workspace_manifest`].
#[derive(Default, Clone, Copy)]
pub struct UpdateWorkspaceManifestOptions<'a> {
    /// Catalog entries to merge into the `catalog:` / `catalogs:` blocks.
    pub updated_catalogs: Option<&'a Catalogs>,
    /// Run the `catalogPrune` pass after the merge: drop catalog
    /// entries nothing in [`Self::catalog_references`] references.
    pub catalog_prune: bool,
    pub catalog_references: CatalogReferenceSources<'a>,
    /// Package name → the versions the freshly resolved lockfile
    /// records. Present only when the lockfile covers every project the
    /// exclude lists govern; `None` disables the passes below that consult
    /// it, mirroring the [`CatalogReferenceSources::all_projects`] guard of
    /// `catalogPrune`.
    pub resolved_package_versions: Option<&'a ResolvedPackageVersions>,
    pub prune_minimum_release_age_excludes: bool,
    pub prune_trust_policy_excludes: bool,
    pub prune_allow_builds: bool,
    /// Entries to merge into the project-local `minimumReleaseAgeExclude`
    /// list after its cleanup pass.
    pub added_minimum_release_age_excludes: &'a [String],
}

/// Apply the requested merges and cleanup passes to `dir`'s
/// `pnpm-workspace.yaml` and the files it includes, writing each file back
/// only when something actually changed (and removing the root manifest when
/// the edits empty it).
pub fn update_workspace_manifest(
    dir: &Path,
    opts: &UpdateWorkspaceManifestOptions<'_>,
) -> Result<(), UpdateWorkspaceManifestError> {
    let manifests = WorkspaceManifests::read(dir)?;
    if !manifests.has_includes() {
        let target = manifests.root_target();
        let edited = edit_manifest_file(&target, opts, None)?;
        return edited.map_or(Ok(()), |manifest| write_or_remove_manifest(&target, manifest));
    }
    routing::update_included_manifests(&manifests, opts)
}

/// The edits of [`update_workspace_manifest`] applied to one file, or `None`
/// when they change nothing. `merged` is the whole workspace's settings when
/// they span several files, for the passes that read settings the file may
/// not hold.
fn edit_manifest_file(
    target: &ManifestTarget,
    opts: &UpdateWorkspaceManifestOptions<'_>,
    merged: Option<&Manifest>,
) -> Result<Option<Manifest>, UpdateWorkspaceManifestError> {
    let path = &target.path;
    let mut manifest = read_manifest_at(path)?;

    if let Some(key) = unsupported_edit_target(&manifest, opts) {
        return Err(UpdateWorkspaceManifestError::UnsupportedInlineBlock {
            path: path.clone(),
            key,
        });
    }

    let mut changed = add_updated_catalogs(&mut manifest, opts, path)?;
    if opts.catalog_prune && !opts.catalog_references.all_projects.is_empty() {
        let references = opts.catalog_references.collect(merged.unwrap_or(&manifest));
        changed |= edit::remove_unused_catalogs(&mut manifest, &references);
    }
    if let Some(resolved) = opts.resolved_package_versions {
        changed |= prune_version_policies(&mut manifest, opts, resolved);
    }
    if !opts.added_minimum_release_age_excludes.is_empty() {
        changed |= add_minimum_release_age_excludes(&mut manifest, opts, path)?;
    }
    Ok(changed.then_some(manifest))
}

/// The text of `path`, or `None` when there is no file.
fn read_if_present(path: &Path) -> Result<Option<String>, UpdateWorkspaceManifestError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(UpdateWorkspaceManifestError::Read { path: path.to_path_buf(), source }),
    }
}

/// `path` parsed, or an empty document when there is no file.
fn read_manifest_at(path: &Path) -> Result<Manifest, UpdateWorkspaceManifestError> {
    let original = read_if_present(path)?;
    Manifest::parse(original.as_deref())
        .map_err(|source| UpdateWorkspaceManifestError::Parse { path: path.to_path_buf(), source })
}

/// The first key an edit would have to rewrite that is written in a flow
/// (inline) style this writer cannot edit in place.
fn unsupported_edit_target(
    manifest: &Manifest,
    opts: &UpdateWorkspaceManifestOptions<'_>,
) -> Option<String> {
    if let Some(updated_catalogs) = opts.updated_catalogs.filter(|catalogs| {
        catalogs
            .values()
            .any(|entries| !entries.is_empty())
    }) {
        let named: Vec<Vec<&str>> = updated_catalogs
            .keys()
            .map(|name| vec!["catalogs", name.as_str()])
            .collect();
        let mut paths: Vec<&[&str]> = vec![&["catalog"], &["catalogs"]];
        paths.extend(named.iter().map(Vec::as_slice));
        if let Some(key) = unsupported_inline_key(manifest.document.text(), &paths) {
            return Some(key);
        }
    }
    if opts.added_minimum_release_age_excludes.is_empty() {
        return None;
    }
    unsupported_inline_key(manifest.document.text(), &[&["minimumReleaseAgeExclude"]])
}

fn add_updated_catalogs(
    manifest: &mut Manifest,
    opts: &UpdateWorkspaceManifestOptions<'_>,
    path: &Path,
) -> Result<bool, UpdateWorkspaceManifestError> {
    let Some(updated_catalogs) = opts.updated_catalogs else {
        return Ok(false);
    };
    if let Some(bad) = first_control_char_value(updated_catalogs) {
        return Err(UpdateWorkspaceManifestError::InvalidControlCharacter {
            path: path.to_path_buf(),
            value: bad.to_string(),
        });
    }
    edit::add_catalogs(manifest, updated_catalogs)
        .map_err(|source| UpdateWorkspaceManifestError::Edit { path: path.to_path_buf(), source })
}

/// Drop the version-policy entries whose package the workspace no longer
/// resolves.
fn prune_version_policies(
    manifest: &mut Manifest,
    opts: &UpdateWorkspaceManifestOptions<'_>,
    resolved: &pnpm_config::version_policy::ResolvedPackageVersions,
) -> bool {
    let mut changed = false;
    if opts.prune_minimum_release_age_excludes {
        changed |= edit::prune_minimum_release_age_excludes(manifest, resolved);
    }
    if opts.prune_trust_policy_excludes {
        changed |= edit::prune_trust_policy_excludes(manifest, resolved);
    }
    if opts.prune_allow_builds {
        changed |= edit::prune_allow_builds(manifest, resolved);
    }
    changed
}

fn add_minimum_release_age_excludes(
    manifest: &mut Manifest,
    opts: &UpdateWorkspaceManifestOptions<'_>,
    path: &Path,
) -> Result<bool, UpdateWorkspaceManifestError> {
    let merged = pnpm_config::version_policy::merge_package_version_specs(
        manifest.exceptions.release_age
            .iter()
            .flatten()
            .chain(opts.added_minimum_release_age_excludes),
    )
    .map_err(UpdateWorkspaceManifestError::VersionPolicy)?;
    if let Some(bad) = merged.iter().find(|exclude| has_control_char(exclude)) {
        return Err(UpdateWorkspaceManifestError::InvalidControlCharacter {
            path: path.to_path_buf(),
            value: bad.clone(),
        });
    }
    Ok(edit::set_minimum_release_age_excludes(manifest, &merged))
}

/// The first catalog name, dependency name, or specifier in `catalogs` that
/// holds a control character, if any. `saveCatalogName` reaches this writer
/// from `pnpm-workspace.yaml`, `PNPM_CONFIG_SAVE_CATALOG_NAME`, and
/// `--save-catalog-name`, none of which constrain the value.
fn first_control_char_value(catalogs: &Catalogs) -> Option<&str> {
    catalogs
        .iter()
        .flat_map(|(catalog_name, entries)| {
            std::iter::once(catalog_name).chain(entries.keys()).chain(entries.values())
        })
        .find(|value| has_control_char(value))
        .map(String::as_str)
}

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
    update_target_field(&target, key, value)
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
    update_target_field(&target, key, &target.setting_from(dir, key, value))
}

fn update_target_field(
    target: &ManifestTarget,
    key: &str,
    value: &serde_json::Value,
) -> Result<(), UpdateWorkspaceManifestError> {
    let path = target.path.as_path();
    let original = read_if_present(path)?;

    let edit = edit_manifest_field(original.as_deref(), key, value)
        .map_err(|error| match error {
            EditManifestFieldError::Edit(source) => {
                UpdateWorkspaceManifestError::Edit { path: path.to_path_buf(), source }
            }
            EditManifestFieldError::Parse { source } => {
                UpdateWorkspaceManifestError::Parse { path: path.to_path_buf(), source }
            }
            EditManifestFieldError::UnsupportedInlineBlock { key } => {
                UpdateWorkspaceManifestError::UnsupportedInlineBlock {
                    path: path.to_path_buf(),
                    key,
                }
            }
        })?;

    let text = match edit {
        ManifestEdit::Unchanged => return Ok(()),
        ManifestEdit::Remove if !target.included => return remove_manifest(path),
        ManifestEdit::Remove => String::new(),
        ManifestEdit::Write(text) => text,
    };

    // A `set` may target a config directory that does not exist yet
    // (`pnpm config set --global`). Create the directory recursively before
    // the write; a `delete` never needs it (the file, hence its parent,
    // already exists).
    if !value.is_null()
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

    write_atomic(path, &text)
        .map_err(|source| UpdateWorkspaceManifestError::Write { path: path.to_path_buf(), source })
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

fn remove_manifest(path: &Path) -> Result<(), UpdateWorkspaceManifestError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => {
            Err(UpdateWorkspaceManifestError::Remove { path: path.to_path_buf(), source })
        }
    }
}

/// Write `manifest` back to `target`. A root manifest the edits emptied is
/// removed; an included file is kept, since the root manifest still names it.
fn write_or_remove_manifest(
    target: &ManifestTarget,
    manifest: Manifest,
) -> Result<(), UpdateWorkspaceManifestError> {
    let path = &target.path;
    if manifest.document.keys.is_empty() && !target.included {
        return remove_manifest(path);
    }
    let text = manifest.document
        .into_text()
        .map_err(|source| UpdateWorkspaceManifestError::Edit { path: path.clone(), source })?;
    write_atomic(path, &text)
        .map_err(|source| UpdateWorkspaceManifestError::Write { path: path.clone(), source })
}

/// Write `contents` to `path` atomically: a sibling temp file in the same
/// directory is written, flushed to disk, and renamed over `path`. The
/// rename replaces the destination's directory entry, so a
/// `pnpm-workspace.yaml` that is a symlink is overwritten rather than
/// followed, and a crash mid-write cannot leave a torn manifest.
fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut tmp = pnpm_fs::private_named_tempfile_in(dir)?;
    tmp.write_all(contents.as_bytes())?;
    pnpm_fs::sync_file(tmp.as_file())?;
    tmp.persist(path).map_err(|err| err.error)?;
    Ok(())
}

mod build_settings;
mod catalog_references;
mod routing;

mod version_policies;
