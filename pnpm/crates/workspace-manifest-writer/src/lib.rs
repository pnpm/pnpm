//! Format-preserving writer for `pnpm-workspace.yaml`'s catalog blocks.
//!
//! Given a set of updated catalogs, merge them into the `catalog:` /
//! `catalogs:` blocks of an existing `pnpm-workspace.yaml` (or create the
//! file) while preserving the comments, blank lines, key order, and quote
//! styles of everything it does not touch.
//!
//! The format-preserving edits are expressed as targeted text splices (for
//! inserts) and [`yamlpatch`] `Op::Replace` (for value updates) — which
//! suffices because the merge only ever *inserts* new entries/blocks or
//! *updates* a single value, never reorders existing content.

pub use build_settings::{
    LEGACY_BUILD_SETTINGS, UNDECIDED_ALLOW_BUILD, scaffold_allow_builds, set_allow_builds,
    set_allow_builds_clearing_legacy,
};
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
use pnpm_config_parse_overrides::parse_pkg_and_parent_selector;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};

mod edit;
mod flow;
mod model;
mod render;

#[cfg(test)]
mod tests;

use model::Manifest;

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
#[derive(Default)]
pub struct UpdateWorkspaceManifestOptions<'a> {
    /// Catalog entries to merge into the `catalog:` / `catalogs:` blocks.
    pub updated_catalogs: Option<&'a Catalogs>,
    /// Run the `catalogPrune` pass after the merge: drop catalog
    /// entries no manifest in [`Self::all_projects`] references.
    pub catalog_prune: bool,
    /// Every workspace project manifest (with in-memory dependency edits
    /// applied), consulted by the cleanup pass to decide which catalog
    /// entries are still referenced. An empty list disables the cleanup
    /// pass, mirroring upstream's `allProjects ?? []` guard.
    pub all_projects: &'a [&'a PackageManifest],
    /// Package name → the versions the freshly resolved lockfile
    /// records. Present only when the lockfile covers every project the
    /// exclude lists govern; `None` disables the passes below that consult
    /// it, mirroring the [`Self::all_projects`] guard of `catalogPrune`.
    pub resolved_package_versions: Option<&'a ResolvedPackageVersions>,
    pub prune_minimum_release_age_excludes: bool,
    pub prune_trust_policy_excludes: bool,
    pub prune_allow_builds: bool,
    /// Entries to merge into the project-local `minimumReleaseAgeExclude`
    /// list after its cleanup pass.
    pub added_minimum_release_age_excludes: &'a [String],
}

/// Apply the requested merges and cleanup passes to `dir`'s
/// `pnpm-workspace.yaml`, writing the file back only when something actually
/// changed (and removing it when the edits empty the document).
pub fn update_workspace_manifest(
    dir: &Path,
    opts: &UpdateWorkspaceManifestOptions<'_>,
) -> Result<(), UpdateWorkspaceManifestError> {
    let path = dir.join(WORKSPACE_MANIFEST_FILENAME);

    let original = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(source) => return Err(UpdateWorkspaceManifestError::Read { path, source }),
    };

    let mut manifest = Manifest::parse(original.as_deref())
        .map_err(|source| UpdateWorkspaceManifestError::Parse { path: path.clone(), source })?;

    if let Some(key) = unsupported_edit_target(&manifest, opts) {
        return Err(UpdateWorkspaceManifestError::UnsupportedInlineBlock { path, key });
    }

    let mut changed = add_updated_catalogs(&mut manifest, opts, &path)?;
    if opts.catalog_prune && !opts.all_projects.is_empty() {
        let references = collect_catalog_references(opts.all_projects, &manifest);
        changed |= edit::remove_unused_catalogs(&mut manifest, &references);
    }
    if let Some(resolved) = opts.resolved_package_versions {
        changed |= prune_version_policies(&mut manifest, opts, resolved);
    }
    if !opts.added_minimum_release_age_excludes.is_empty() {
        changed |= add_minimum_release_age_excludes(&mut manifest, opts, &path)?;
    }
    if !changed {
        return Ok(());
    }

    write_or_remove_manifest(&path, manifest)
}

/// The first key an edit would have to rewrite that is written in a flow
/// (inline) style this writer cannot edit in place.
fn unsupported_edit_target(
    manifest: &Manifest,
    opts: &UpdateWorkspaceManifestOptions<'_>,
) -> Option<String> {
    if let Some(updated_catalogs) = opts
        .updated_catalogs
        .filter(|catalogs| catalogs.values().any(|entries| !entries.is_empty()))
    {
        let named: Vec<Vec<&str>> =
            updated_catalogs.keys().map(|name| vec!["catalogs", name.as_str()]).collect();
        let mut paths: Vec<&[&str]> = vec![&["catalog"], &["catalogs"]];
        paths.extend(named.iter().map(Vec::as_slice));
        if let Some(key) = unsupported_inline_key(manifest.text(), &paths) {
            return Some(key);
        }
    }
    if opts.added_minimum_release_age_excludes.is_empty() {
        return None;
    }
    unsupported_inline_key(manifest.text(), &[&["minimumReleaseAgeExclude"]])
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
        manifest
            .minimum_release_age_exclude
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

/// The upstream `packageReferences` map: every raw dependency specifier per
/// package name across `dependencies`, `devDependencies`,
/// `optionalDependencies`, and `peerDependencies` of every project, plus the
/// workspace manifest's own `catalog:`-valued `overrides:` (whose selector
/// names the referenced package). Selectors that fail to parse are skipped,
/// matching upstream.
fn collect_catalog_references(
    all_projects: &[&PackageManifest],
    manifest: &Manifest,
) -> edit::CatalogReferences {
    const GROUPS: [DependencyGroup; 4] = [
        DependencyGroup::Prod,
        DependencyGroup::Dev,
        DependencyGroup::Optional,
        DependencyGroup::Peer,
    ];
    let mut references = edit::CatalogReferences::new();
    for project in all_projects {
        for (name, specifier) in project.dependencies(GROUPS) {
            references.entry(name.to_string()).or_default().insert(specifier.to_string());
        }
    }
    for (selector, specifier) in manifest.overrides.iter().flatten() {
        if !specifier.starts_with("catalog:") {
            continue;
        }
        let Ok((_, target_pkg)) = parse_pkg_and_parent_selector(selector) else {
            continue;
        };
        references.entry(target_pkg.name).or_default().insert(specifier.clone());
    }
    references
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
    let original = match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(UpdateWorkspaceManifestError::Read { path: path.to_path_buf(), source });
        }
    };

    let edit =
        edit_manifest_field(original.as_deref(), key, value).map_err(|error| match error {
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
        ManifestEdit::Remove => return remove_manifest(path),
        ManifestEdit::Write(text) => text,
    };

    // A `set` may target a config directory that does not exist yet
    // (`pnpm config set --global`). Create the directory recursively before
    // the write; a `delete` never needs it (the file, hence its parent,
    // already exists).
    if !value.is_null()
        && let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| UpdateWorkspaceManifestError::Write {
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

    if edit::document_root_is_inline(manifest.text()) {
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
    if manifest.top_level_keys.is_empty() {
        return Ok(ManifestEdit::Remove);
    }
    Ok(ManifestEdit::Write(manifest.into_text()))
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

fn write_or_remove_manifest(
    path: &Path,
    manifest: Manifest,
) -> Result<(), UpdateWorkspaceManifestError> {
    if manifest.top_level_keys.is_empty() {
        remove_manifest(path)
    } else {
        write_atomic(path, &manifest.into_text()).map_err(|source| {
            UpdateWorkspaceManifestError::Write { path: path.to_path_buf(), source }
        })
    }
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
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(contents.as_bytes())?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|err| err.error)?;
    Ok(())
}

mod build_settings;

mod version_policies;
