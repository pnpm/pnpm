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

pub use pnpm_config::version_policy::ResolvedPackageVersions;

mod edit;
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
        "Cannot edit {key:?} in {path:?}: it uses an inline (flow) YAML value. Reformat it to block style and try again."
    )]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_MANIFEST_WRITER_UNSUPPORTED_INLINE_BLOCK))]
    UnsupportedInlineBlock { path: std::path::PathBuf, key: String },

    #[display(
        "Cannot write {value:?} to {path:?}: it contains a control character that would corrupt the YAML."
    )]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_MANIFEST_WRITER_INVALID_CONTROL_CHARACTER))]
    InvalidControlCharacter { path: std::path::PathBuf, value: String },
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
    /// records. Present only under `minimumReleaseAgeExcludePrune`, and
    /// only when the lockfile covers every project
    /// `minimumReleaseAgeExclude` governs; `None` disables that pass,
    /// mirroring the [`Self::all_projects`] guard of
    /// `catalogPrune`.
    pub resolved_package_versions: Option<&'a ResolvedPackageVersions>,
}

/// Merge `opts.updated_catalogs` into `dir`'s `pnpm-workspace.yaml` and run
/// the `catalogPrune` pass when requested, writing the file back
/// only when something actually changed (and removing it when the edits
/// empty the document).
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

    let mut changed = match opts.updated_catalogs {
        Some(updated_catalogs) => {
            if let Some(bad) = first_control_char_value(updated_catalogs) {
                return Err(UpdateWorkspaceManifestError::InvalidControlCharacter {
                    path,
                    value: bad.to_string(),
                });
            }
            edit::add_catalogs(&mut manifest, updated_catalogs).map_err(|source| {
                UpdateWorkspaceManifestError::Edit { path: path.clone(), source }
            })?
        }
        None => false,
    };
    if opts.catalog_prune && !opts.all_projects.is_empty() {
        let references = collect_catalog_references(opts.all_projects, &manifest);
        changed |= edit::remove_unused_catalogs(&mut manifest, &references);
    }
    if let Some(resolved) = opts.resolved_package_versions {
        changed |= edit::prune_minimum_release_age_excludes(&mut manifest, resolved);
    }
    if !changed {
        return Ok(());
    }

    write_or_remove_manifest(&path, manifest)
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

/// Write `name → specifier` entries into `dir`'s `pnpm-workspace.yaml`
/// `configDependencies:` block (creating the file/block if absent),
/// preserving the rest of the document's formatting and reading, parsing,
/// and writing the file at most once. Used by `pnpm add --config`; the
/// resolved integrity is recorded separately in the env lockfile, so only
/// the clean specifier is written here.
pub fn set_config_dependencies<'a>(
    dir: &Path,
    entries: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<(), UpdateWorkspaceManifestError> {
    let path = dir.join(WORKSPACE_MANIFEST_FILENAME);

    let original = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(source) => return Err(UpdateWorkspaceManifestError::Read { path, source }),
    };

    let mut manifest = Manifest::parse(original.as_deref())
        .map_err(|source| UpdateWorkspaceManifestError::Parse { path: path.clone(), source })?;

    let mut changed = false;
    for (name, specifier) in entries {
        changed |= edit::add_config_dependency(&mut manifest, name, specifier)
            .map_err(|source| UpdateWorkspaceManifestError::Edit { path: path.clone(), source })?;
    }
    if !changed {
        return Ok(());
    }

    write_or_remove_manifest(&path, manifest)
}

/// Upsert `name → bool` entries into `dir`'s `pnpm-workspace.yaml`
/// `allowBuilds:` block (creating the file/block if absent), preserving the
/// rest of the document's formatting, and write the file back only when
/// something actually changed. Used by `pnpm approve-builds` to record
/// which dependencies may (`true`) or may not (`false`) run build scripts.
///
/// `entries` is iterated in its own order; pass an ordered map for a
/// deterministic result.
pub fn set_allow_builds<'a, Entries>(
    dir: &Path,
    entries: Entries,
) -> Result<(), UpdateWorkspaceManifestError>
where
    Entries: IntoIterator<Item = (&'a str, bool)>,
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
        // The block-style splice writes `- name: true` on one line, so a
        // control character in `name` (e.g. a newline from a crafted
        // `--allow-build`) would corrupt the document — refuse instead.
        if has_control_char(name) {
            return Err(UpdateWorkspaceManifestError::InvalidControlCharacter {
                path,
                value: name.to_string(),
            });
        }
        changed |= edit::add_allow_build(&mut manifest, name, value);
    }
    if !changed {
        return Ok(());
    }

    write_or_remove_manifest(&path, manifest)
}

/// The value an install writes for a package whose build it ignored. Not a
/// decision — pnpm's build policy only acts on `true` / `false` — so it is
/// purely a prompt to edit, next to the packages the user already decided.
pub const UNDECIDED_ALLOW_BUILD: &str = "set this to true or false";

/// Add an [`UNDECIDED_ALLOW_BUILD`] entry to `dir`'s `pnpm-workspace.yaml`
/// `allowBuilds:` block (creating the file/block if absent) for every name
/// in `names` that has no entry there yet, preserving the rest of the
/// document's formatting. Names that already have one — decided or not —
/// are left alone, so this never overwrites a user's answer.
///
/// `names` is iterated in its own order; pass an ordered collection for a
/// deterministic result.
pub fn scaffold_allow_builds<'a, Names>(
    dir: &Path,
    names: Names,
) -> Result<(), UpdateWorkspaceManifestError>
where
    Names: IntoIterator<Item = &'a str>,
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
    for name in names {
        // Same guard as `set_allow_builds`: the block-style splice writes
        // the entry on one line, so a control character in `name` would
        // corrupt the document.
        if has_control_char(name) {
            return Err(UpdateWorkspaceManifestError::InvalidControlCharacter {
                path,
                value: name.to_string(),
            });
        }
        changed |= edit::add_undecided_allow_build(&mut manifest, name, UNDECIDED_ALLOW_BUILD);
    }
    if !changed {
        return Ok(());
    }

    write_or_remove_manifest(&path, manifest)
}

/// Merge `patched_dependencies` into `dir`'s `pnpm-workspace.yaml`
/// `patchedDependencies:` block, preserving the rest of the document's
/// formatting.
pub fn set_patched_dependencies(
    dir: &Path,
    patched_dependencies: &IndexMap<String, String>,
) -> Result<(), UpdateWorkspaceManifestError> {
    let path = dir.join(WORKSPACE_MANIFEST_FILENAME);

    let original = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(source) => return Err(UpdateWorkspaceManifestError::Read { path, source }),
    };

    let mut manifest = Manifest::parse(original.as_deref())
        .map_err(|source| UpdateWorkspaceManifestError::Parse { path: path.clone(), source })?;

    let changed = edit::add_patched_dependencies(&mut manifest, patched_dependencies)
        .map_err(|source| UpdateWorkspaceManifestError::Edit { path: path.clone(), source })?;
    if !changed {
        return Ok(());
    }

    write_or_remove_manifest(&path, manifest)
}

/// Upsert `selector → specifier` entries into `dir`'s `pnpm-workspace.yaml`
/// `overrides:` block (creating the file/block if absent), preserving the
/// rest of the document's formatting, and write the file back only when
/// something actually changed. Used by `pacquet link` to record `link:`
/// overrides and by `pnpm audit --fix` to force non-vulnerable versions.
/// A hand-written non-string (parent-scoped object) value is refused rather
/// than clobbered.
///
/// `entries` is iterated in its own order; pass an ordered map for a
/// deterministic result.
pub fn set_overrides<'a, Entries>(
    dir: &Path,
    entries: Entries,
) -> Result<(), UpdateWorkspaceManifestError>
where
    Entries: IntoIterator<Item = (&'a str, &'a str)>,
{
    let path = dir.join(WORKSPACE_MANIFEST_FILENAME);

    let original = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(source) => return Err(UpdateWorkspaceManifestError::Read { path, source }),
    };

    let mut manifest = Manifest::parse(original.as_deref())
        .map_err(|source| UpdateWorkspaceManifestError::Parse { path: path.clone(), source })?;

    // The block-style splice can't safely edit an inline (flow) `overrides:`
    // mapping; refuse rather than corrupt it.
    if edit::top_level_has_inline_value(manifest.text(), "overrides") {
        return Err(UpdateWorkspaceManifestError::UnsupportedInlineBlock {
            path,
            key: "overrides".to_string(),
        });
    }

    let mut changed = false;
    for (selector, specifier) in entries {
        if has_control_char(selector) || has_control_char(specifier) {
            let value = if has_control_char(selector) { selector } else { specifier };
            return Err(UpdateWorkspaceManifestError::InvalidControlCharacter {
                path,
                value: value.to_string(),
            });
        }
        // Refuse to overwrite a hand-written non-string (parent-scoped
        // object) override value with a scalar — that would corrupt config.
        if manifest.non_scalar_overrides.contains(selector) {
            return Err(UpdateWorkspaceManifestError::OverrideConflict {
                key: selector.to_string(),
                path,
            });
        }
        changed |= edit::add_overrides(&mut manifest, selector, specifier)
            .map_err(|source| UpdateWorkspaceManifestError::Edit { path: path.clone(), source })?;
    }
    if !changed {
        return Ok(());
    }

    write_or_remove_manifest(&path, manifest)
}

/// Set `dir`'s `pnpm-workspace.yaml` `auditConfig.ignoreGhsas:` to `ghsas`
/// (the complete desired list), creating the file/block if absent and
/// removing the `auditConfig:` block when `ghsas` is empty. Preserves the
/// rest of the document's formatting and writes the file back only when
/// something actually changed. Used by `pnpm audit --ignore` /
/// `--ignore-unfixable` to persist suppressed advisories.
pub fn set_audit_ignore_ghsas(
    dir: &Path,
    ghsas: &[String],
) -> Result<(), UpdateWorkspaceManifestError> {
    let path = dir.join(WORKSPACE_MANIFEST_FILENAME);

    let original = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(source) => return Err(UpdateWorkspaceManifestError::Read { path, source }),
    };

    let mut manifest = Manifest::parse(original.as_deref())
        .map_err(|source| UpdateWorkspaceManifestError::Parse { path: path.clone(), source })?;

    if let Some(bad) = ghsas.iter().find(|ghsa| has_control_char(ghsa)) {
        return Err(UpdateWorkspaceManifestError::InvalidControlCharacter {
            path,
            value: bad.clone(),
        });
    }

    // The block-style splice can't safely edit an inline (flow) `auditConfig:`
    // mapping; refuse rather than corrupt it.
    if edit::top_level_has_inline_value(manifest.text(), "auditConfig") {
        return Err(UpdateWorkspaceManifestError::UnsupportedInlineBlock {
            path,
            key: "auditConfig".to_string(),
        });
    }

    let changed = edit::set_audit_ignore_ghsas(&mut manifest, ghsas)
        .map_err(|source| UpdateWorkspaceManifestError::Edit { path: path.clone(), source })?;
    if !changed {
        return Ok(());
    }

    write_or_remove_manifest(&path, manifest)
}

/// Set `dir`'s `pnpm-workspace.yaml` top-level `minimumReleaseAgeExclude:` to
/// `excludes` (the complete desired list), creating the file/block if absent
/// and removing the block when `excludes` is empty. The caller merges with any
/// existing entries (via `pnpm_config::version_policy::merge_package_version_specs`)
/// before calling. Used by `pnpm audit --fix` to let patched versions through
/// the `minimumReleaseAge` maturity cutoff.
pub fn set_minimum_release_age_excludes(
    dir: &Path,
    excludes: &[String],
) -> Result<(), UpdateWorkspaceManifestError> {
    let path = dir.join(WORKSPACE_MANIFEST_FILENAME);

    let original = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(source) => return Err(UpdateWorkspaceManifestError::Read { path, source }),
    };

    if let Some(bad) = excludes.iter().find(|exclude| has_control_char(exclude)) {
        return Err(UpdateWorkspaceManifestError::InvalidControlCharacter {
            path,
            value: bad.clone(),
        });
    }

    let mut manifest = Manifest::parse(original.as_deref())
        .map_err(|source| UpdateWorkspaceManifestError::Parse { path: path.clone(), source })?;

    if !edit::set_minimum_release_age_excludes(&mut manifest, excludes) {
        return Ok(());
    }

    write_or_remove_manifest(&path, manifest)
}

/// Delete `selectors` from `dir`'s `pnpm-workspace.yaml` `overrides:` block,
/// dropping the block (and the file, once it has no other top-level keys)
/// when nothing remains, and writing back only when something actually
/// changed. A missing file is a no-op. The inverse of [`set_overrides`];
/// used by `pacquet unlink` to drop link: overrides.
pub fn remove_overrides(
    dir: &Path,
    selectors: &[String],
) -> Result<(), UpdateWorkspaceManifestError> {
    let path = dir.join(WORKSPACE_MANIFEST_FILENAME);

    let original = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(UpdateWorkspaceManifestError::Read { path, source }),
    };

    let mut manifest = Manifest::parse(Some(&original))
        .map_err(|source| UpdateWorkspaceManifestError::Parse { path: path.clone(), source })?;

    if !edit::remove_overrides(&mut manifest, selectors) {
        return Ok(());
    }

    write_or_remove_manifest(&path, manifest)
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

    let mut manifest = Manifest::parse(original.as_deref()).map_err(|source| {
        UpdateWorkspaceManifestError::Parse { path: path.to_path_buf(), source }
    })?;

    let changed = if value.is_null() {
        edit::remove_top_level_field(&mut manifest, key)
    } else {
        edit::set_top_level_field(&mut manifest, key, value)
    };
    if !changed {
        return Ok(());
    }

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

    write_or_remove_manifest(path, manifest)
}

fn write_or_remove_manifest(
    path: &Path,
    manifest: Manifest,
) -> Result<(), UpdateWorkspaceManifestError> {
    if manifest.top_level_keys.is_empty() {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => {
                Err(UpdateWorkspaceManifestError::Remove { path: path.to_path_buf(), source })
            }
        }
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
