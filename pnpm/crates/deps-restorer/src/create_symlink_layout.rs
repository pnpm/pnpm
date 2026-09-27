use crate::{
    DirectDepsByImporter, HoistGraphNode, SkippedSnapshots, SymlinkPackageError,
    VirtualStoreLayout, safe_join_modules_dir::safe_join_modules_dir, symlink_package,
};
use indexmap::IndexMap;
use pnpm_config::matcher::Matcher;
use pnpm_lockfile::{PackageKey, PkgName, SnapshotDepRef, SnapshotEntry};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

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
    let opt_deps = optional_dependencies
        .filter(|_| include_optional_dependencies)
        .into_iter()
        .flatten();
    deps.chain(opt_deps)
        .try_for_each(|(alias_name, dep_ref)| {
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
            let symlink_path =
                safe_join_modules_dir(virtual_node_modules_dir, &alias_name.to_string())
                    .map_err(SymlinkPackageError::InvalidAlias)?;
            symlink_package(&symlink_target, &symlink_path).map(drop)
        })
}

type SymlinkWork = (Arc<PathBuf>, Arc<PathBuf>);

fn snapshot_direct_deps(snapshot: &SnapshotEntry) -> IndexMap<String, PackageKey> {
    let mut direct_deps = IndexMap::new();
    for (alias, dep_ref) in snapshot.dependencies
        .iter()
        .flatten()
        .chain(snapshot.optional_dependencies.iter().flatten())
    {
        if let Some(key) = dep_ref.resolve(alias) {
            direct_deps.entry(alias.to_string()).or_insert(key);
        }
    }
    direct_deps
}

fn push_alias_symlinks(
    pkg_name: &str,
    direct_deps: &IndexMap<String, PackageKey>,
    virtual_node_modules_dir: &Path,
    target: &Arc<PathBuf>,
    aliases: &IndexMap<String, pnpm_modules_yaml::HoistKind>,
    work: &mut Vec<SymlinkWork>,
) {
    for alias in aliases.keys() {
        if alias == pkg_name || direct_deps.contains_key(alias) {
            continue;
        }
        if let Ok(dest) = safe_join_modules_dir(virtual_node_modules_dir, alias) {
            work.push((Arc::clone(target), Arc::new(dest)));
        }
    }
}

fn collect_dep_hoist_symlinks(
    pkg_key: &PackageKey,
    direct_deps: &IndexMap<String, PackageKey>,
    virtual_node_modules_dir: &Path,
    layout: &VirtualStoreLayout,
    hoist_graph: &HashMap<PackageKey, HoistGraphNode>,
    hoisted_dependencies: &crate::HoistedDependencies,
) -> Vec<SymlinkWork> {
    let mut work = Vec::new();
    let pkg_name = pkg_key.name.to_string();
    for (dep_path_str, aliases) in hoisted_dependencies {
        let Ok(dep_key) = dep_path_str.parse::<PackageKey>() else {
            continue;
        };
        let Some(pkg) = hoist_graph.get(&dep_key) else {
            continue;
        };
        let target_slot = layout.slot_dir(&dep_key);
        let Ok(target) =
            safe_join_modules_dir(&target_slot.join("node_modules"), &pkg.name.to_string())
        else {
            continue;
        };
        let target_arc = Arc::new(target);
        push_alias_symlinks(
            &pkg_name,
            direct_deps,
            virtual_node_modules_dir,
            &target_arc,
            aliases,
            &mut work,
        );
    }
    work
}

fn slot_hoisted_symlinks(
    pkg_key: &PackageKey,
    snapshot: &SnapshotEntry,
    layout: &VirtualStoreLayout,
    hoist_graph: &HashMap<PackageKey, HoistGraphNode>,
    patterns: (&Matcher, &Matcher),
    hoist_skipped: &HashSet<PackageKey>,
) -> Vec<SymlinkWork> {
    let direct_deps = snapshot_direct_deps(snapshot);
    if direct_deps.is_empty() {
        return Vec::new();
    }
    let mut direct_deps_by_importer = DirectDepsByImporter::new();
    direct_deps_by_importer.insert(".".to_string(), direct_deps.clone());

    let (private_pattern, public_pattern) = patterns;
    let Some(result) = crate::hoist::get_hoisted_dependencies(&crate::hoist::HoistInputs {
        graph: hoist_graph,
        direct_deps_by_importer: &direct_deps_by_importer,
        skipped: hoist_skipped,
        private_pattern: private_pattern.clone(),
        public_pattern: public_pattern.clone(),
        hoisted_workspace_packages: None,
    }) else {
        return Vec::new();
    };

    let slot_dir = layout.slot_dir(pkg_key);
    collect_dep_hoist_symlinks(
        pkg_key,
        &direct_deps,
        &slot_dir.join("node_modules"),
        layout,
        hoist_graph,
        &result.hoisted_dependencies,
    )
}

fn ensure_parent_directories(pairs: &[SymlinkWork]) {
    let mut dir_set = HashSet::new();
    for (target, dest) in pairs {
        if let Some(parent) = target.parent() {
            dir_set.insert(parent);
        }
        if let Some(parent) = dest.parent() {
            dir_set.insert(parent);
        }
    }
    for dir in dir_set {
        let _ = std::fs::create_dir_all(dir);
    }
}

/// Symlinks hoisted transitive dependencies inside each GVS slot's `node_modules`.
pub fn create_gvs_hoisted_children_symlinks(
    hoist_graph: &HashMap<PackageKey, HoistGraphNode>,
    private_pattern: &Matcher,
    public_pattern: &Matcher,
    layout: &VirtualStoreLayout,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    hoist_skipped: &HashSet<PackageKey>,
) -> Result<(), SymlinkPackageError> {
    use rayon::prelude::*;

    if !layout.enable_global_virtual_store() {
        return Ok(());
    }

    let pairs: Vec<SymlinkWork> = snapshots
        .par_iter()
        .flat_map(|(pkg_key, snapshot)| {
            slot_hoisted_symlinks(
                pkg_key,
                snapshot,
                layout,
                hoist_graph,
                (private_pattern, public_pattern),
                hoist_skipped,
            )
        })
        .collect();

    ensure_parent_directories(&pairs);

    pairs
        .par_iter()
        .try_for_each(|(target, dest)| symlink_package(target.as_ref(), dest.as_ref()).map(drop))
}

#[cfg(test)]
mod tests;
