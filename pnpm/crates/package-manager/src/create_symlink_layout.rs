use crate::{
    DirectDepsByImporter, HoistGraphNode, SkippedSnapshots, SymlinkPackageError,
    VirtualStoreLayout, safe_join_modules_dir::safe_join_modules_dir, symlink_package,
};
use pacquet_config::matcher::Matcher;
use pacquet_lockfile::{PackageKey, PkgName, SnapshotDepRef, SnapshotEntry};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

pub fn create_symlink_layout(
    dependencies: Option<&HashMap<PkgName, SnapshotDepRef>>,
    optional_dependencies: Option<&HashMap<PkgName, SnapshotDepRef>>,
    self_name: &PkgName,
    skipped: &SkippedSnapshots,
    layout: &VirtualStoreLayout,
    virtual_node_modules_dir: &Path,
) -> Result<(), SymlinkPackageError> {
    let deps = dependencies.into_iter().flatten();
    let opt_deps = optional_dependencies.into_iter().flatten();
    deps.chain(opt_deps).try_for_each(|(alias_name, dep_ref)| {
        if alias_name == self_name {
            return Ok(());
        }
        let Some(target) = dep_ref.resolve(alias_name) else {
            return Ok(());
        };
        if skipped.contains(&target) {
            return Ok(());
        }
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

pub fn create_gvs_hoisted_children_symlinks(
    hoist_graph: &HashMap<PackageKey, HoistGraphNode>,
    private_pattern: &Matcher,
    public_pattern: &Matcher,
    layout: &VirtualStoreLayout,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    hoist_skipped: &HashSet<PackageKey>,
) -> Result<(), SymlinkPackageError> {
    use crate::hoist::{HoistInputs, get_hoisted_dependencies};
    use rayon::prelude::*;

    if !layout.enable_global_virtual_store() {
        return Ok(());
    }

    type SymlinkWork = (Arc<PathBuf>, Arc<PathBuf>);

    let pairs: Vec<SymlinkWork> = snapshots
        .par_iter()
        .flat_map(|(pkg_key, snapshot)| {
            let slot_dir = layout.slot_dir(pkg_key);
            let virtual_node_modules_dir = slot_dir.join("node_modules");

            let mut direct_deps: HashMap<String, PackageKey> = HashMap::new();
            for (alias, dep_ref) in snapshot
                .dependencies
                .iter()
                .flatten()
                .chain(snapshot.optional_dependencies.iter().flatten())
            {
                if let Some(key) = dep_ref.resolve(alias) {
                    direct_deps.entry(alias.to_string()).or_insert(key);
                }
            }
            if direct_deps.is_empty() {
                return Vec::new();
            }

            let mut direct_deps_by_importer = DirectDepsByImporter::new();
            direct_deps_by_importer.insert(".".to_string(), direct_deps.clone());

            let Some(result) = get_hoisted_dependencies(&HoistInputs {
                graph: hoist_graph,
                direct_deps_by_importer: &direct_deps_by_importer,
                skipped: hoist_skipped,
                private_pattern: private_pattern.clone(),
                public_pattern: public_pattern.clone(),
                hoisted_workspace_packages: None,
            }) else {
                return Vec::new();
            };

            let mut work = Vec::new();
            for (dep_path_str, aliases) in &result.hoisted_dependencies {
                let dep_key: PackageKey = match dep_path_str.parse() {
                    Ok(k) => k,
                    Err(_) => continue,
                };
                let Some(pkg) = hoist_graph.get(&dep_key) else {
                    continue;
                };
                let target_slot = layout.slot_dir(&dep_key);
                for alias in aliases.keys() {
                    if alias == &pkg_key.name.to_string() || direct_deps.contains_key(alias) {
                        continue;
                    }
                    let Ok(target) = safe_join_modules_dir(
                        &target_slot.join("node_modules"),
                        &pkg.name.to_string(),
                    ) else {
                        continue;
                    };
                    let Ok(dest) = safe_join_modules_dir(&virtual_node_modules_dir, alias) else {
                        continue;
                    };
                    work.push((Arc::new(target), Arc::new(dest)));
                }
            }
            work
        })
        .collect();

    let mut dir_set: HashSet<&Path> = HashSet::new();
    for (target, dest) in &pairs {
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

    pairs
        .par_iter()
        .try_for_each(|(target, dest)| symlink_package(target.as_ref(), dest.as_ref()).map(drop))
}

#[cfg(test)]
mod tests;
