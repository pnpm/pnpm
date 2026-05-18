use crate::{SkippedSnapshots, SymlinkPackageError, VirtualStoreLayout, symlink_package};
use pacquet_lockfile::{PkgName, SnapshotDepRef};
use std::{collections::HashMap, path::Path};

/// Create symlink layout of dependencies for a package in a virtual dir.
///
/// Mirrors upstream's `linkAllModules` child-selection rules at
/// <https://github.com/pnpm/pnpm/blob/f2981a316/installing/deps-installer/src/install/link.ts#L521-L549>
/// and the underlying `dependencies` ∪ `optionalDependencies` merge in
/// `lockfileToDepGraph` at
/// <https://github.com/pnpm/pnpm/blob/f2981a316/deps/graph-builder/src/lockfileToDepGraph.ts#L150-L156>.
///
/// For each entry in `dependencies ∪ optional_dependencies`:
/// - Skip when the alias matches the slot's own package name
///   (`alias === depNode.name`), so a package that lists itself as
///   a dep doesn't get a circular self-link inside its own slot.
/// - Skip when the target snapshot is in `skipped` — its slot was
///   never materialized (platform-mismatched optional, `--no-optional`
///   excluded, or a swallowed optional fetch failure). Mirrors
///   upstream's `!pkg || (!pkg.installable && pkg.optional)` guard.
/// - Otherwise create a symlink at
///   `<virtual_node_modules_dir>/<alias>` pointing to the target's
///   slot under `<layout.slot_dir(target)>/node_modules/<target-name>`.
///
/// For npm-aliased dependencies (e.g.
/// `string-width-cjs: string-width@4.2.3`), the symlink filename
/// under `node_modules/` uses the entry key (the alias), while the
/// virtual-store lookup uses the aliased target.
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
    // it against. The single-caller policy upstream is to run this
    // stage single-threaded on a `spawn_blocking` worker (see
    // `CreateVirtualStore::run`), mirroring pnpm's `symlinkAllModules`
    // in `worker/src/start.ts`.
    let deps = dependencies.into_iter().flatten();
    let opt_deps = optional_dependencies.into_iter().flatten();
    deps.chain(opt_deps).try_for_each(|(alias_name, dep_ref)| {
        if alias_name == self_name {
            return Ok(());
        }
        let target = dep_ref.resolve(alias_name);
        if skipped.contains(&target) {
            return Ok(());
        }
        let target_name_str = target.name.to_string();
        let alias_name_str = alias_name.to_string();
        symlink_package(
            &layout.slot_dir(&target).join("node_modules").join(&target_name_str),
            &virtual_node_modules_dir.join(&alias_name_str),
        )
    })
}

#[cfg(test)]
mod tests;
