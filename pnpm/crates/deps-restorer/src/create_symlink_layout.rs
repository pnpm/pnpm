use crate::{
    SkippedSnapshots, SymlinkPackageError, VirtualStoreLayout,
    safe_join_modules_dir::safe_join_modules_dir, symlink_package,
};
use pnpm_lockfile::{PkgName, SnapshotDepRef};
use std::{collections::HashMap, path::Path};

/// Create symlink layout of dependencies for a package in a virtual dir.
///
/// Links the union of the package's `dependencies` and
/// `optionalDependencies` into the slot's `node_modules`, skipping the
/// package's own name and any target whose slot was not materialized.
///
/// Child target paths come from the install-scoped
/// [`VirtualStoreLayout`]: `layout.slot_dir(&target)` returns either
/// `<virtual_store_dir>/<flat-name>` (legacy) or
/// `<global_virtual_store_dir>/<scope>/<name>/<version>/<hash>` (GVS),
/// so the caller doesn't have to branch on which mode is in effect.
///
/// `virtual_node_modules_dir` does not have to exist; missing parent
/// directories are created as needed.
pub fn create_symlink_layout(
    dependencies: Option<&HashMap<PkgName, SnapshotDepRef>>,
    optional_dependencies: Option<&HashMap<PkgName, SnapshotDepRef>>,
    include_optional_dependencies: bool,
    self_name: &PkgName,
    skipped: &SkippedSnapshots,
    layout: &VirtualStoreLayout,
    virtual_node_modules_dir: &Path,
) -> Result<(), SymlinkPackageError> {
    // Serial iteration: the symlink work per snapshot is small (a
    // handful of entries), so fanning out to rayon here would just add
    // task-scheduling overhead without a wider work queue to amortise
    // it against. This stage runs single-threaded on a `spawn_blocking`
    // worker (see `CreateVirtualStore::run`).
    let deps = dependencies.into_iter().flatten();
    let opt_deps =
        optional_dependencies.filter(|_| include_optional_dependencies).into_iter().flatten();
    deps.chain(opt_deps).try_for_each(|(alias_name, dep_ref)| {
        if alias_name == self_name {
            return Ok(());
        }
        // A `link:` dep has no slot of its own: it points at a
        // directory outside the virtual store, named relative to the
        // lockfile. The importer's own copy is installed by the
        // symlink-direct-dependencies stage, but a snapshot that
        // depends on one still needs the link inside *its* slot —
        // without it the dependency is simply absent, and Node only
        // finds it when the slot happens to sit under the importer's
        // `node_modules` and the upward walk reaches the importer's
        // copy. Under the global virtual store the slot lives in the
        // shared store, that walk never reaches the project, and the
        // dependency goes missing at runtime.
        if let Some(link_target) = dep_ref.as_link_target() {
            let Some(lockfile_dir) = layout.lockfile_dir() else {
                return Ok(());
            };
            let symlink_path =
                safe_join_modules_dir(virtual_node_modules_dir, &alias_name.to_string())
                    .map_err(SymlinkPackageError::InvalidAlias)?;
            return symlink_package(&lockfile_dir.join(link_target), &symlink_path).map(drop);
        }
        let Some(target) = dep_ref.resolve(alias_name) else {
            return Ok(());
        };
        if skipped.contains(&target) {
            return Ok(());
        }
        // Both names are lockfile-derived and untrusted: `target.name`
        // is the resolved package's own name and `alias_name` is the
        // dependency key. A traversal-shaped name (`@x/../../...`) would
        // otherwise let the symlink target or the symlink itself escape
        // the slot's `node_modules`, so guard each join.
        let symlink_target = safe_join_modules_dir(
            &layout.slot_dir(&target).join("node_modules"),
            &target.name.to_string(),
        )
        .map_err(SymlinkPackageError::InvalidAlias)?;
        let symlink_path = safe_join_modules_dir(virtual_node_modules_dir, &alias_name.to_string())
            .map_err(SymlinkPackageError::InvalidAlias)?;
        symlink_package(&symlink_target, &symlink_path).map(drop)
    })
}

#[cfg(test)]
mod tests;
