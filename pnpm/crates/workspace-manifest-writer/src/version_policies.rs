use super::{
    IndexMap, Manifest, Path, UpdateWorkspaceManifestError, WORKSPACE_MANIFEST_FILENAME, edit, fs,
    has_control_char, io, unsupported_inline_key, write_or_remove_manifest,
};

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

    let entries: Vec<(&str, &str)> = entries.into_iter().collect();
    if !entries.is_empty()
        && let Some(key) = unsupported_inline_key(manifest.text(), &[&["configDependencies"]])
    {
        return Err(UpdateWorkspaceManifestError::UnsupportedInlineBlock { path, key });
    }

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

    if !patched_dependencies.is_empty()
        && let Some(key) = unsupported_inline_key(manifest.text(), &[&["patchedDependencies"]])
    {
        return Err(UpdateWorkspaceManifestError::UnsupportedInlineBlock { path, key });
    }

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

    let entries: Vec<(&str, &str)> = entries.into_iter().collect();
    if !entries.is_empty()
        && let Some(key) = unsupported_inline_key(manifest.text(), &[&["overrides"]])
    {
        return Err(UpdateWorkspaceManifestError::UnsupportedInlineBlock { path, key });
    }

    if let Some(value) = first_control_char_override(&entries) {
        return Err(UpdateWorkspaceManifestError::InvalidControlCharacter {
            path,
            value: value.to_string(),
        });
    }
    // Refuse to overwrite a hand-written non-string (parent-scoped object)
    // override value with a scalar — that would corrupt config.
    if let Some((selector, _)) =
        entries.iter().find(|(selector, _)| manifest.non_scalar_overrides.contains(*selector))
    {
        return Err(UpdateWorkspaceManifestError::OverrideConflict {
            key: (*selector).to_string(),
            path,
        });
    }

    let mut changed = false;
    for (selector, specifier) in entries {
        changed |= edit::add_overrides(&mut manifest, selector, specifier)
            .map_err(|source| UpdateWorkspaceManifestError::Edit { path: path.clone(), source })?;
    }
    if !changed {
        return Ok(());
    }

    write_or_remove_manifest(&path, manifest)
}

/// The first selector or specifier holding a control character, if any.
fn first_control_char_override<'a>(entries: &[(&'a str, &'a str)]) -> Option<&'a str> {
    entries
        .iter()
        .flat_map(|(selector, specifier)| [*selector, *specifier])
        .find(|value| has_control_char(value))
}

/// Set `dir`'s `pnpm-workspace.yaml` audit ignore list to `ghsas` (the
/// complete desired list), targeting whichever spelling the manifest uses —
/// the canonical `audit.ignore` wins over the deprecated
/// `auditConfig.ignoreGhsas`, matching the reader's precedence, and the
/// shadowed deprecated list is removed when both are present — creating the
/// file plus an `auditConfig:` block when neither is present.
/// Preserves the rest of the document's formatting and writes the file back
/// only when something actually changed. Used by `pnpm audit --ignore` /
/// `--ignore-unfixable` and the `audit.ignorePrune` cleanup to persist
/// suppressed advisories.
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

    if let Some(key) = unsupported_inline_key(
        manifest.text(),
        &[&["auditConfig"], &["auditConfig", "ignoreGhsas"], &["audit"], &["audit", "ignore"]],
    ) {
        return Err(UpdateWorkspaceManifestError::UnsupportedInlineBlock { path, key });
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

    if let Some(key) = unsupported_inline_key(manifest.text(), &[&["minimumReleaseAgeExclude"]]) {
        return Err(UpdateWorkspaceManifestError::UnsupportedInlineBlock { path, key });
    }

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
