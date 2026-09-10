use super::{
    Manifest, Path, UpdateWorkspaceManifestError, WORKSPACE_MANIFEST_FILENAME, edit, fs,
    has_control_char, io, unsupported_inline_key, write_or_remove_manifest,
};

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
    update_allow_builds(dir, entries, false)
}

/// Top-level `pnpm-workspace.yaml` settings that `allowBuilds:` replaced in
/// pnpm v11.
pub const LEGACY_BUILD_SETTINGS: &[&str] = &[
    "onlyBuiltDependencies",
    "onlyBuiltDependenciesFile",
    "neverBuiltDependencies",
    "ignoredBuiltDependencies",
];

/// Same as [`set_allow_builds`], but also deletes the
/// [`LEGACY_BUILD_SETTINGS`] in the same write. Used by `pnpm
/// approve-builds` so a workspace migrated from pnpm v10 is not left with
/// dead build settings next to the `allowBuilds:` it writes.
pub fn set_allow_builds_clearing_legacy<'a, Entries>(
    dir: &Path,
    entries: Entries,
) -> Result<(), UpdateWorkspaceManifestError>
where
    Entries: IntoIterator<Item = (&'a str, bool)>,
{
    update_allow_builds(dir, entries, true)
}

fn update_allow_builds<'a, Entries>(
    dir: &Path,
    entries: Entries,
    clear_legacy_settings: bool,
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

    let entries: Vec<(&str, bool)> = entries.into_iter().collect();
    if !entries.is_empty()
        && let Some(key) = unsupported_inline_key(manifest.text(), &[&["allowBuilds"]])
    {
        return Err(UpdateWorkspaceManifestError::UnsupportedInlineBlock { path, key });
    }

    // The block-style splice writes `- name: true` on one line, so a control
    // character in `name` (e.g. a newline from a crafted `--allow-build`)
    // would corrupt the document — refuse instead.
    if let Some((name, _)) = entries.iter().find(|(name, _)| has_control_char(name)) {
        return Err(UpdateWorkspaceManifestError::InvalidControlCharacter {
            path,
            value: (*name).to_string(),
        });
    }

    let mut changed = false;
    for (name, value) in entries {
        changed |= edit::add_allow_build(&mut manifest, name, value);
    }
    if clear_legacy_settings {
        for key in LEGACY_BUILD_SETTINGS {
            changed |= edit::remove_top_level_field(&mut manifest, key);
        }
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

    let names: Vec<&str> = names.into_iter().collect();
    if !names.is_empty()
        && let Some(key) = unsupported_inline_key(manifest.text(), &[&["allowBuilds"]])
    {
        return Err(UpdateWorkspaceManifestError::UnsupportedInlineBlock { path, key });
    }

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
