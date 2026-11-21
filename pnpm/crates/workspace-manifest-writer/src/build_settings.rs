use pnpm_config::{
    AllowBuild,
    PackagePermissions,
    SkillsPermission,
};

use super::{
    Manifest,
    ManifestTarget,
    Path,
    UpdateWorkspaceManifestError,
    edit,
    has_control_char,
    read_manifest_at,
    read_owning_manifest,
    unsupported_inline_key,
    write_or_remove_manifest,
};

/// The value of one `permissions` entry: the capability the decision is
/// about is the variant, so no caller can pair a `skills` decision with a
/// `build` capability. Serialized untagged, matching the YAML shapes
/// `permissions` writes.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(untagged)]
pub enum PermissionValue {
    /// All-or-nothing: the package may (`true`) or may not (`false`) run
    /// build scripts.
    Build(bool),
    /// Whether pnpm links the agent skills the package ships: all, none
    /// (a denial, never offered again), or exactly the chosen ones.
    Skills(SkillsPermission),
}

/// Upsert `name → bool` entries into `dir`'s `pnpm-workspace.yaml`
/// `allowBuilds:` block (creating the file/block if absent), preserving the
/// rest of the document's formatting, and write the file back only when
/// something actually changed. Records which dependencies may (`true`) or
/// may not (`false`) run build scripts.
///
/// When the file has a `permissions` block, each decision lands there as
/// [`set_permissions`] places a [`PermissionValue::Build`]: a
/// `permissions.<name>.build` entry outranks `allowBuilds.<name>`, so an
/// `allowBuilds` write would not take effect.
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

/// Record `name → bool` build decisions as
/// `globalPackagesConfig.permissions.<name>.build` in the global
/// `config.yaml` at `path`, creating the file, its directory, and the
/// section if absent. Preserves the rest of the document's formatting, and
/// writes the file back only when something changed.
pub fn set_global_packages_build_permissions<'a, Entries>(
    path: &Path,
    entries: Entries,
) -> Result<(), UpdateWorkspaceManifestError>
where
    Entries: IntoIterator<Item = (&'a str, bool)>,
{
    let decisions = entries
        .into_iter()
        .map(|(name, allowed)| {
            (
                name.to_string(),
                PackagePermissions {
                    build: Some(AllowBuild::Decided(allowed)),
                    ..Default::default()
                },
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    set_global_packages_permissions(path, &decisions)
}

/// Record `globalPackagesConfig.permissions` decisions in the global
/// `config.yaml` at `path`, creating the file, its directory, and the
/// section if absent. Preserves the rest of the document's formatting, and
/// writes the file back only when something changed.
pub fn set_global_packages_permissions(
    path: &Path,
    permissions: &std::collections::BTreeMap<String, PackagePermissions>,
) -> Result<(), UpdateWorkspaceManifestError> {
    let target = ManifestTarget::root_manifest(path);
    let mut manifest = read_manifest_at(path)?;
    let mut blocks =
        vec![vec!["globalPackagesConfig"], vec!["globalPackagesConfig", "permissions"]];
    for (name, entry) in permissions {
        blocks.push(vec!["globalPackagesConfig", "permissions", name.as_str()]);
        if entry.build.is_some() {
            blocks.push(vec!["globalPackagesConfig", "permissions", name.as_str(), "build"]);
        }
        if entry.skills.is_some() {
            blocks.push(vec!["globalPackagesConfig", "permissions", name.as_str(), "skills"]);
        }
    }
    let blocks: Vec<&[&str]> = blocks
        .iter()
        .map(Vec::as_slice)
        .collect();
    if let Some(key) = unsupported_inline_key(manifest.document.text(), &blocks) {
        return Err(UpdateWorkspaceManifestError::UnsupportedInlineBlock {
            path: path.to_path_buf(),
            key,
        });
    }
    for (name, entry) in permissions {
        validate_global_permission_entry(path, name, entry)?;
    }
    let mut changed = false;
    for (name, entry) in permissions {
        if let Some(allowed) = entry.build.as_ref().and_then(AllowBuild::decided) {
            changed |= edit::set_global_packages_permission(
                &mut manifest,
                name,
                "build",
                &PermissionValue::Build(allowed),
            );
        }
        if let Some(skills) = entry.skills.as_ref() {
            changed |= edit::set_global_packages_permission(
                &mut manifest,
                name,
                "skills",
                &PermissionValue::Skills(skills.clone()),
            );
        }
    }
    if !changed {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|source| UpdateWorkspaceManifestError::Write {
                path: path.to_path_buf(),
                source,
            })?;
    }
    write_or_remove_manifest(&target, manifest)
}

fn validate_global_permission_entry(
    path: &Path,
    name: &str,
    entry: &PackagePermissions,
) -> Result<(), UpdateWorkspaceManifestError> {
    if has_control_char(name) {
        return Err(UpdateWorkspaceManifestError::InvalidControlCharacter {
            path: path.to_path_buf(),
            value: name.to_string(),
        });
    }
    if let Some(SkillsPermission::Chosen(chosen)) = entry.skills.as_ref()
        && let Some(skill) = chosen.iter().find(|skill| has_control_char(skill))
    {
        return Err(UpdateWorkspaceManifestError::InvalidControlCharacter {
            path: path.to_path_buf(),
            value: skill.clone(),
        });
    }
    Ok(())
}

/// Record `globalPackagesConfig.skills.dirs` in the global `config.yaml`.
pub fn set_global_packages_skills_dirs(
    path: &Path,
    dirs: &[String],
) -> Result<(), UpdateWorkspaceManifestError> {
    let target = ManifestTarget::root_manifest(path);
    let mut manifest = read_manifest_at(path)?;
    if let Some(key) = unsupported_inline_key(
        manifest.document.text(),
        &[
            &["globalPackagesConfig"],
            &["globalPackagesConfig", "skills"],
            &["globalPackagesConfig", "skills", "dirs"],
        ],
    ) {
        return Err(UpdateWorkspaceManifestError::UnsupportedInlineBlock {
            path: path.to_path_buf(),
            key,
        });
    }
    for dir in dirs {
        if has_control_char(dir) {
            return Err(UpdateWorkspaceManifestError::InvalidControlCharacter {
                path: path.to_path_buf(),
                value: dir.clone(),
            });
        }
    }
    if !edit::set_global_packages_skills_dirs(&mut manifest, dirs) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|source| UpdateWorkspaceManifestError::Write {
                path: path.to_path_buf(),
                source,
            })?;
    }
    write_or_remove_manifest(&target, manifest)
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
    let (target, mut manifest) = read_owning_manifest(dir, "allowBuilds")?;
    let path = target.path.clone();

    let entries: Vec<(&str, bool)> = entries.into_iter().collect();
    let blocks = permission_blocks(entries.iter().map(|(name, _)| *name));
    let blocks: Vec<&[&str]> = blocks
        .iter()
        .map(Vec::as_slice)
        .collect();
    validate_allow_build_entries(&manifest, &path, &entries, &blocks)?;

    let mut changed = record_permissions(
        &mut manifest,
        entries
            .into_iter()
            .map(|(name, allowed)| (name, PermissionValue::Build(allowed)))
            .collect(),
    );
    if clear_legacy_settings {
        for key in LEGACY_BUILD_SETTINGS {
            changed |= edit::remove_top_level_field(&mut manifest, key);
        }
    }
    if !changed {
        return Ok(());
    }

    write_or_remove_manifest(&target, manifest)
}

/// Refuse an `allowBuilds` write the line-based splices cannot make safely:
/// one into an inline block at `blocks`, or one whose name holds a control
/// character. The block-style splice writes `name: true` on one line, so a
/// control character in `name` (e.g. a newline from a crafted
/// `--allow-build`) would corrupt the document.
fn validate_allow_build_entries(
    manifest: &Manifest,
    path: &Path,
    entries: &[(&str, bool)],
    blocks: &[&[&str]],
) -> Result<(), UpdateWorkspaceManifestError> {
    if !entries.is_empty()
        && let Some(key) = unsupported_inline_key(manifest.document.text(), blocks)
    {
        return Err(UpdateWorkspaceManifestError::UnsupportedInlineBlock {
            path: path.to_path_buf(),
            key,
        });
    }
    if let Some((name, _)) = entries.iter().find(|(name, _)| has_control_char(name)) {
        return Err(UpdateWorkspaceManifestError::InvalidControlCharacter {
            path: path.to_path_buf(),
            value: (*name).to_string(),
        });
    }
    Ok(())
}

/// Record `permissions` decisions in `dir`'s `pnpm-workspace.yaml`,
/// deleting the [`LEGACY_BUILD_SETTINGS`] in the same write.
///
/// A [`PermissionValue::Build`] decision lands in `permissions` only when
/// the file already has a `permissions` block, and the package's
/// `allowBuilds` entry is then removed. Otherwise it lands in
/// `allowBuilds`, which pnpm versions without `permissions` read too. A
/// `PermissionValue::Skills` decision always lands in `permissions`; this
/// pnpm is the only reader of that capability.
pub fn set_permissions<'a, Entries>(
    dir: &Path,
    entries: Entries,
) -> Result<(), UpdateWorkspaceManifestError>
where
    Entries: IntoIterator<Item = (&'a str, PermissionValue)>,
{
    let (target, mut manifest) = read_owning_manifest(dir, "allowBuilds")?;
    let path = target.path.clone();
    let entries: Vec<(&str, PermissionValue)> = entries.into_iter().collect();
    validate_permission_entries(&manifest, &path, &entries)?;

    let mut changed = record_permissions(&mut manifest, entries);
    for key in LEGACY_BUILD_SETTINGS {
        changed |= edit::remove_top_level_field(&mut manifest, key);
    }
    if !changed {
        return Ok(());
    }
    write_or_remove_manifest(&target, manifest)
}

/// The blocks a build or permission decision for `names` may be spliced
/// into: `allowBuilds`, `permissions`, and each named package's
/// `permissions` entry.
fn permission_blocks<'a>(names: impl Iterator<Item = &'a str>) -> Vec<Vec<&'a str>> {
    let mut blocks = vec![vec!["allowBuilds"], vec!["permissions"]];
    blocks.extend(names.map(|name| vec!["permissions", name]));
    blocks
}

/// Refuse a write the line-based splices cannot make safely.
fn validate_permission_entries(
    manifest: &Manifest,
    path: &Path,
    entries: &[(&str, PermissionValue)],
) -> Result<(), UpdateWorkspaceManifestError> {
    let blocks = permission_blocks(entries.iter().map(|(name, _)| *name));
    let blocks: Vec<&[&str]> = blocks
        .iter()
        .map(Vec::as_slice)
        .collect();
    if !entries.is_empty()
        && let Some(key) = unsupported_inline_key(manifest.document.text(), &blocks)
    {
        return Err(UpdateWorkspaceManifestError::UnsupportedInlineBlock {
            path: path.to_path_buf(),
            key,
        });
    }
    // The block- and flow-style splices write every value on one line, so a
    // control character in a package key or a chosen skill name would
    // corrupt the document, refuse instead.
    for (name, value) in entries {
        if has_control_char(name) {
            return Err(UpdateWorkspaceManifestError::InvalidControlCharacter {
                path: path.to_path_buf(),
                value: (*name).to_string(),
            });
        }
        if let PermissionValue::Skills(SkillsPermission::Chosen(chosen)) = value
            && let Some(skill) = chosen.iter().find(|skill| has_control_char(skill))
        {
            return Err(UpdateWorkspaceManifestError::InvalidControlCharacter {
                path: path.to_path_buf(),
                value: skill.clone(),
            });
        }
    }
    Ok(())
}

/// Upsert `entries`, sending build decisions to `allowBuilds` unless the
/// document already has `permissions`. Returns whether anything changed.
fn record_permissions(manifest: &mut Manifest, entries: Vec<(&str, PermissionValue)>) -> bool {
    let builds_in_permissions = manifest.document.keys
        .iter()
        .any(|key| key == "permissions");
    let mut changed = false;
    for (name, value) in entries {
        match &value {
            PermissionValue::Build(allowed) => {
                if !builds_in_permissions {
                    changed |= edit::add_allow_build(manifest, name, *allowed);
                    continue;
                }
                changed |= edit::set_permission(manifest, name, "build", &value);
                changed |= edit::remove_allow_build(manifest, name);
            }
            PermissionValue::Skills(_) => {
                changed |= edit::set_permission(manifest, name, "skills", &value);
            }
        }
    }
    changed
}

/// Set `skills.dirs` in `dir`'s `pnpm-workspace.yaml` (creating the file and
/// blocks if absent), preserving the rest of the document's formatting, and
/// write the file back only when something actually changed.
///
/// The directories are workspace-root-relative. An empty `dirs` removes the
/// `skills.dirs` entry (and the `skills` block when it held nothing else)
/// recording the detected directories instead: pnpm only writes a decision
/// it has read, so a caller that learned nothing records nothing. A caller
/// can still write an empty `skills.dirs` list by hand to turn the feature
/// off.
pub fn set_skills_dirs(dir: &Path, dirs: &[String]) -> Result<(), UpdateWorkspaceManifestError> {
    let (target, mut manifest) = read_owning_manifest(dir, "skills")?;
    let path = target.path.clone();
    if let Some(key) =
        unsupported_inline_key(manifest.document.text(), &[&["skills"], &["skills", "dirs"]])
    {
        return Err(UpdateWorkspaceManifestError::UnsupportedInlineBlock { path, key });
    }
    // The upsert splices write every item on its own line but the entry
    // starts after `dirs: `, so a control character in a relative directory
    // would corrupt the document, refuse instead.
    if let Some(dir_value) = dirs.iter().find(|dir_value| has_control_char(dir_value)) {
        return Err(UpdateWorkspaceManifestError::InvalidControlCharacter {
            path,
            value: dir_value.clone(),
        });
    }
    if !edit::set_skills_dirs(&mut manifest, dirs) {
        return Ok(());
    }
    write_or_remove_manifest(&target, manifest)
}

/// The value an install writes for a package whose build it ignored. Not a
/// decision (pnpm's build policy only acts on `true` / `false`) so it is
/// purely a prompt to edit, next to the packages the user already decided.
pub const UNDECIDED_ALLOW_BUILD: &str = "set this to true or false";

/// Add an [`UNDECIDED_ALLOW_BUILD`] entry to `dir`'s `pnpm-workspace.yaml`
/// `allowBuilds:` block (creating the file/block if absent) for every name
/// in `names` that has no entry there yet, preserving the rest of the
/// document's formatting. Names that already have one (decided or not)
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
    let (target, mut manifest) = read_owning_manifest(dir, "allowBuilds")?;
    let path = target.path.clone();

    let names: Vec<&str> = names.into_iter().collect();
    if !names.is_empty()
        && let Some(key) = unsupported_inline_key(manifest.document.text(), &[&["allowBuilds"]])
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

    write_or_remove_manifest(&target, manifest)
}
