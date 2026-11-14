//! Placing and removing the links of agent skills in the agent skill
//! directories.

use super::{AgentSkillsError, discovery::MAX_SKILL_DIR_DEPTH, targets::STANDARD_AGENT_DIRS};
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
        .filter(|entry| recorded_parent_is_within_workspace(workspace_root, entry))
    {
        let path = workspace_root.join(entry);
        if links_to_a_package_skill(workspace_root, &path) {
            remove_link(&path).map_err(|source| AgentSkillsError::Io { path, source })?;
        }
    }
    Ok(())
}

pub(super) fn recorded_parent_is_within_workspace(workspace_root: &Path, entry: &str) -> bool {
    if !is_within_workspace(entry) {
        return false;
    }
    let Some(parent) = Path::new(entry).parent() else { return false };
    let Ok(root) = dunce::canonicalize(workspace_root) else { return false };
    let Ok(parent) = dunce::canonicalize(workspace_root.join(parent)) else { return false };
    pnpm_fs::is_subdir(&root, &parent)
}

/// `.modules.yaml` is not trusted to name only pnpm's entries, so pnpm
/// touches only a relative entry reached without `..`.
pub(super) fn is_within_workspace(entry: &str) -> bool {
    Path::new(entry)
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
}

/// Whether `path` is a link into the `skills` directory of an installed or
/// linked package, as every link pnpm creates is, even after `node_modules`
/// is removed.
fn links_to_a_package_skill(workspace_root: &Path, path: &Path) -> bool {
    let Ok(target) = pnpm_fs::read_symlink_dir(path) else { return false };
    let components: Vec<Component<'_>> = target.components().collect();
    if components
        .iter()
        .any(|component| component.as_os_str() == "node_modules")
    {
        return (0..components.len())
            .rev()
            .any(|idx| {
                components[idx].as_os_str() == "node_modules"
                    && is_modules_package_skill(&components[idx + 1..])
            });
    }
    is_local_package_skill(workspace_root, path, &target)
}

fn is_local_package_skill(workspace_root: &Path, path: &Path, target: &Path) -> bool {
    let Some(parent) = path.parent() else { return false };
    let root = canonical_or_lexical(workspace_root);
    let link_parent = canonical_or_lexical(parent);
    let resolved = pnpm_fs::lexical_normalize(&link_parent.join(target));
    let parts: Vec<Component<'_>> = resolved.components().collect();
    (0..parts.len()).any(|idx| {
        parts[idx].as_os_str() == "skills"
            && is_local_skills_boundary(&root, &link_parent, &parts, idx)
    })
}

fn is_local_skills_boundary(
    root: &Path,
    link_parent: &Path,
    parts: &[Component<'_>],
    skills_idx: usize,
) -> bool {
    if !is_valid_skill_subpath(&parts[skills_idx + 1..]) {
        return false;
    }
    let pkg_dir: PathBuf = parts[..skills_idx].iter().collect();
    if pkg_dir.as_os_str().is_empty() || (pkg_dir != root && pkg_dir.parent().is_none()) {
        return false;
    }
    let skills_dir = pkg_dir.join("skills");
    skills_dir != link_parent
        && !STANDARD_AGENT_DIRS
            .iter()
            .any(|dir| skills_dir == root.join(dir))
}

fn canonical_or_lexical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| pnpm_fs::lexical_normalize(path))
}

fn is_modules_package_skill(package: &[Component<'_>]) -> bool {
    let name_len = match package.first() {
        Some(Component::Normal(first)) if first.as_encoded_bytes().starts_with(b"@") => 2,
        Some(Component::Normal(_)) => 1,
        _ => return false,
    };
    package
        .get(..name_len)
        .is_some_and(|segments| {
            segments
                .iter()
                .copied()
                .all(is_valid_skill_segment)
        })
        && package
            .get(name_len)
            .is_some_and(|dir| dir.as_os_str() == "skills")
        && is_valid_skill_subpath(&package[name_len + 1..])
}

fn is_valid_skill_subpath(segments: &[Component<'_>]) -> bool {
    (1..=MAX_SKILL_DIR_DEPTH).contains(&segments.len())
        && segments
            .iter()
            .copied()
            .all(is_valid_skill_segment)
}

fn is_valid_skill_segment(component: Component<'_>) -> bool {
    let Component::Normal(segment) = component else { return false };
    segment
        .to_str()
        .is_some_and(|name| {
            !name.is_empty() && !name.starts_with('.') && !name.chars().any(char::is_control)
        })
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
