use crate::{
    SkippedSnapshots, SymlinkPackageError, VirtualStoreLayout,
    safe_join_modules_dir::safe_join_modules_dir, symlink_package,
};
use pacquet_lockfile::{PkgName, SnapshotDepRef};
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
/// `virtual_node_modules_dir` does not have to exist —
/// `symlink_package` calls `fs::create_dir_all` on the symlink path's
/// parent before each link. Callers that already know the directory
/// exists (e.g. `CreateVirtualStore::run`, which `mkdir`s it just
/// before calling this function) just pay redundant stat syscalls,
/// which is cheap and matches pnpm's own redundant-mkdir shape.
pub fn create_symlink_layout(
    dependencies: Option<&HashMap<PkgName, SnapshotDepRef>>,
    optional_dependencies: Option<&HashMap<PkgName, SnapshotDepRef>>,
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
    let opt_deps = optional_dependencies.into_iter().flatten();
    deps.chain(opt_deps).try_for_each(|(alias_name, dep_ref)| {
        if alias_name == self_name {
            return Ok(());
        }
        // `link:` deps point at a workspace sibling outside the
        // virtual store; the symlink-direct-dependencies stage
        // installs those for the importer, not here.
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
