use super::{HoistedLinkerError, Lockfile, PackageKey, SymlinkPackageError};
use crate::{DependenciesGraphNode, safe_join_modules_dir::safe_join_modules_dir, symlink_package};
use std::{collections::HashSet, path::PathBuf};

/// Link each `link:<root>/...` dependency of a hoisted package from that
/// package's own `node_modules` to the directory inside the package. The
/// hoister keeps such a dependency next to the package that declares it, and
/// importing the package keeps its nested `node_modules`, so the link
/// survives later imports. Optional dependencies are linked only when
/// `include_optional` is set.
pub(crate) fn link_hoisted_package_root_links<'g>(
    lockfile: &Lockfile,
    nodes: impl IntoIterator<Item = &'g DependenciesGraphNode>,
    include_optional: bool,
) -> Result<HashSet<PathBuf>, HoistedLinkerError> {
    let mut changed_dirs = HashSet::new();
    for node in nodes {
        let Ok(key) = node.package.dep_path.as_str().parse::<PackageKey>() else {
            continue;
        };
        let Some(snapshot) = lockfile.snapshots
            .as_ref()
            .and_then(|snapshots| snapshots.get(&key))
        else {
            continue;
        };
        let deps = snapshot.dependencies
            .iter()
            .flatten()
            .chain(
                snapshot.optional_dependencies
                    .iter()
                    .filter(|_| include_optional)
                    .flatten(),
            );
        for (alias, dep_ref) in deps {
            if let Some(target) = dep_ref.package_root_link_target() {
                link_package_root_target(node, &alias.to_string(), target)?;
                changed_dirs.insert(node.dir.join("node_modules"));
            }
        }
    }
    Ok(changed_dirs)
}

fn link_package_root_target(
    node: &DependenciesGraphNode,
    alias: &str,
    target: &str,
) -> Result<(), HoistedLinkerError> {
    let symlink_path = safe_join_modules_dir(&node.dir.join("node_modules"), alias)
        .map_err(SymlinkPackageError::InvalidAlias)
        .map_err(HoistedLinkerError::HoistSymlink)?;
    let target_dir = pnpm_lockfile::join_package_root_link(&node.dir, target);
    symlink_package(&target_dir, &symlink_path).map(drop).map_err(HoistedLinkerError::HoistSymlink)
}
