//! Placing and removing the links of agent skills in the agent skill
//! directories.

use super::AgentSkillsError;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Component, Path, PathBuf},
};

pub(super) fn prune(
    workspace_root: &Path,
    linked: &[String],
    keep: &BTreeSet<String>,
) -> Result<(), AgentSkillsError> {
    for entry in linked
        .iter()
        .filter(|entry| !keep.contains(*entry))
        .filter(|entry| is_within_workspace(entry))
    {
        let path = workspace_root.join(entry);
        if links_to_a_package_skill(&path) {
            remove_link(&path).map_err(|source| AgentSkillsError::Io { path, source })?;
        }
    }
    Ok(())
}

/// `.modules.yaml` is not trusted to name only pnpm's entries, so pnpm
/// touches only a relative entry reached without `..`.
pub(super) fn is_within_workspace(entry: &str) -> bool {
    Path::new(entry)
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
}

/// Whether `path` is a link into the `skills` directory of a package in
/// `node_modules`, as every link pnpm creates is, even after `node_modules`
/// is removed.
fn links_to_a_package_skill(path: &Path) -> bool {
    let Ok(target) = fs::read_link(path) else { return false };
    let components: Vec<&std::ffi::OsStr> = target
        .components()
        .map(Component::as_os_str)
        .collect();
    let Some(modules) = components
        .iter()
        .rposition(|component| *component == "node_modules")
    else {
        return false;
    };
    let package = &components[modules + 1..];
    let name_len = if package
        .first()
        .is_some_and(|name| name.as_encoded_bytes().starts_with(b"@"))
    {
        2
    } else {
        1
    };
    package
        .get(name_len)
        .is_some_and(|dir| *dir == "skills")
}

/// Remove a link or junction without following it. Anything else at the path
/// was put there by someone other than pnpm and is left alone.
fn remove_link(path: &Path) -> io::Result<()> {
    match pnpm_fs::is_symlink_or_junction(path) {
        Ok(true) => pnpm_fs::remove_symlink_dir(path),
        Ok(false) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Fail before anything is pruned or linked when an entry to link is held
/// by something pnpm must not replace, so a failed sync leaves the existing
/// links in place.
pub(super) fn ensure_unoccupied(
    workspace_root: &Path,
    linked: &[String],
    desired: &BTreeMap<String, PathBuf>,
    virtual_store_dir: &Path,
) -> Result<(), AgentSkillsError> {
    for (entry, target) in desired {
        let path = workspace_root.join(entry);
        if is_occupied(&path, target, linked.contains(entry), virtual_store_dir) {
            return Err(AgentSkillsError::Occupied { path });
        }
    }
    Ok(())
}

pub(super) fn materialize(
    workspace_root: &Path,
    desired: &BTreeMap<String, PathBuf>,
) -> Result<(), AgentSkillsError> {
    for (entry, target) in desired {
        let path = workspace_root.join(entry);
        create_link(target, &path).map_err(|source| AgentSkillsError::Io { path, source })?;
    }
    Ok(())
}

/// Whether `path` holds something pnpm must not replace. pnpm replaces only
/// a link: one it recorded, one that already resolves to `target`, one that
/// resolves to nothing, as a link into a removed `node_modules` does, or one
/// that resolves to the same skill of another copy of the package in
/// `virtual_store_dir`, such as a peer variant that has left the lockfile.
pub(super) fn is_occupied(
    path: &Path,
    target: &Path,
    recorded: bool,
    virtual_store_dir: &Path,
) -> bool {
    if fs::symlink_metadata(path).is_err() {
        return false;
    }
    let replaceable = pnpm_fs::is_symlink_or_junction(path).unwrap_or(false)
        && (recorded
            || links_to(path, target)
            || fs::metadata(path).is_err()
            || links_to_same_skill_in(path, target, virtual_store_dir));
    !replaceable
}

/// The link target is relative and `target` is canonical, so the link is
/// placed through the canonical form of its directory. Otherwise a symlink
/// in the workspace path, such as `/var` on macOS, leaves it dangling.
fn create_link(target: &Path, path: &Path) -> io::Result<()> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    };
    pnpm_fs::force_symlink_dir(target, &fs::canonicalize(dir)?.join(name)).map(drop)
}

/// Whether `path` resolves inside `virtual_store_dir` to a path ending in
/// the `node_modules/<name>/skills/<skill>` of `target`.
fn links_to_same_skill_in(path: &Path, target: &Path, virtual_store_dir: &Path) -> bool {
    let Some(skill_path) = skill_path_from_modules_dir(target) else { return false };
    matches!(
        (fs::canonicalize(path), fs::canonicalize(virtual_store_dir)),
        (Ok(resolved), Ok(store)) if resolved.starts_with(&store) && resolved.ends_with(skill_path),
    )
}

/// `node_modules/<name>/skills/<skill>` of `<dir>/node_modules/<name>/skills/<skill>`,
/// nested categories included: the canonical form of every copy of the skill
/// in the virtual store ends with it.
fn skill_path_from_modules_dir(target: &Path) -> Option<&Path> {
    let modules_dir = target
        .ancestors()
        .find(|dir| {
            dir.file_name()
                .is_some_and(|name| name == "node_modules")
        })?;
    target
        .strip_prefix(modules_dir.parent()?)
        .ok()
}

fn links_to(path: &Path, target: &Path) -> bool {
    matches!(
        (fs::canonicalize(path), fs::canonicalize(target)),
        (Ok(resolved), Ok(target)) if resolved == target,
    )
}

#[cfg(test)]
mod tests;
