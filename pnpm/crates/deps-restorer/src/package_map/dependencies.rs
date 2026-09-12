use super::{
    PackageMapOptions, PackageMapPackage, add_external_link_package, has_package_entry,
    link_target_id, normalize_path,
};
use pnpm_fs::lexical_normalize;
use pnpm_lockfile::{Lockfile, ProjectSnapshot, SnapshotDepRef};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

pub(super) fn add_importer_dependencies(
    packages: &mut BTreeMap<String, PackageMapPackage>,
    dependencies: &mut BTreeMap<String, String>,
    lockfile: &Lockfile,
    opts: &PackageMapOptions<'_>,
    importer_id: &str,
    importer: &ProjectSnapshot,
) {
    for deps in [
        importer.dependencies.as_ref(),
        importer.optional_dependencies.as_ref(),
        importer.dev_dependencies.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        for (alias, spec) in deps {
            if let Some(target) = spec.version.as_link_target() {
                let target = resolve_link_target(opts.lockfile_dir, Some(importer_id), target);
                add_external_link_package(packages, &target, opts.modules_dir);
                dependencies.insert(alias.to_string(), target.id);
                continue;
            }
            if let Some(key) = spec.version.resolved_key(alias)
                && has_package_entry(lockfile, &key)
            {
                dependencies.insert(alias.to_string(), key.to_string());
            }
        }
    }
}
pub(super) fn add_physical_importer_dependencies(
    loose_index: &mut PhysicalPackageIndex,
    packages: &mut BTreeMap<String, PackageMapPackage>,
    lockfile: &Lockfile,
    opts: &PackageMapOptions<'_>,
    modules_dir: &Path,
    deps: Option<&pnpm_lockfile::ResolvedDependencyMap>,
    importer_id: Option<&str>,
) {
    let Some(deps) = deps else { return };
    for (alias, spec) in deps {
        if let Some(target) = spec.version.as_link_target() {
            let target = resolve_link_target(opts.lockfile_dir, importer_id, target);
            add_external_link_package(packages, &target, opts.modules_dir);
            loose_index.add(modules_dir, alias.to_string(), target.id);
            continue;
        }
        if let Some(key) = spec.version.resolved_key(alias)
            && has_package_entry(lockfile, &key)
        {
            loose_index.add(modules_dir, alias.to_string(), key.to_string());
        }
    }
}
pub(super) fn add_snapshot_dependencies(
    packages: &mut BTreeMap<String, PackageMapPackage>,
    dependencies: &mut BTreeMap<String, String>,
    lockfile: &Lockfile,
    opts: &PackageMapOptions<'_>,
    deps: Option<&HashMap<pnpm_lockfile::PkgName, SnapshotDepRef>>,
) {
    let Some(deps) = deps else { return };
    for (alias, reference) in deps {
        if let Some(target) = reference.as_link_target() {
            let target = resolve_link_target(opts.lockfile_dir, None, target);
            add_external_link_package(packages, &target, opts.modules_dir);
            dependencies.insert(alias.to_string(), target.id);
            continue;
        }
        if let Some(key) = reference.resolve(alias)
            && has_package_entry(lockfile, &key)
        {
            dependencies.insert(alias.to_string(), key.to_string());
        }
    }
}
pub(super) fn add_physical_snapshot_dependencies(
    loose_index: &mut PhysicalPackageIndex,
    packages: &mut BTreeMap<String, PackageMapPackage>,
    lockfile: &Lockfile,
    opts: &PackageMapOptions<'_>,
    modules_dir: &Path,
    deps: Option<&HashMap<pnpm_lockfile::PkgName, SnapshotDepRef>>,
) {
    let Some(deps) = deps else { return };
    for (alias, reference) in deps {
        if let Some(target) = reference.as_link_target() {
            let target = resolve_link_target(opts.lockfile_dir, None, target);
            add_external_link_package(packages, &target, opts.modules_dir);
            loose_index.add(modules_dir, alias.to_string(), target.id);
            continue;
        }
        if let Some(key) = reference.resolve(alias)
            && has_package_entry(lockfile, &key)
        {
            loose_index.add(modules_dir, alias.to_string(), key.to_string());
        }
    }
}
pub(super) trait LinkReference {
    fn as_link_target(&self) -> Option<&'_ str>;
}
impl LinkReference for pnpm_lockfile::ResolvedDependencySpec {
    fn as_link_target(&self) -> Option<&'_ str> {
        self.version.as_link_target()
    }
}
impl LinkReference for SnapshotDepRef {
    fn as_link_target(&self) -> Option<&'_ str> {
        SnapshotDepRef::as_link_target(self)
    }
}
#[derive(Debug, Default)]
pub(super) struct PhysicalPackageIndex {
    by_modules_dir: BTreeMap<String, BTreeMap<String, String>>,
}
impl PhysicalPackageIndex {
    pub(super) fn add(&mut self, modules_dir: &Path, package_name: String, package_id: String) {
        self.by_modules_dir
            .entry(normalize_path(&lexical_normalize(modules_dir)))
            .or_default()
            .insert(package_name, package_id);
    }
}
pub(super) fn add_loose_dependencies(
    packages: &mut BTreeMap<String, PackageMapPackage>,
    package_dirs: Option<&BTreeMap<String, PathBuf>>,
    loose_index: Option<&PhysicalPackageIndex>,
) {
    let (Some(package_dirs), Some(loose_index)) = (package_dirs, loose_index) else { return };
    for (id, package_dir) in package_dirs {
        let physical = physical_dependencies(package_dir, loose_index);
        if let Some(pkg) = packages.get_mut(id) {
            for (alias, dep_id) in physical {
                pkg.dependencies.insert(alias, dep_id);
            }
        }
    }
}
pub(super) fn physical_dependencies(
    package_dir: &Path,
    loose_index: &PhysicalPackageIndex,
) -> BTreeMap<String, String> {
    let mut dependencies = BTreeMap::new();
    let mut current = package_dir.to_path_buf();
    loop {
        let modules_dir = normalize_path(&current.join("node_modules"));
        for (name, id) in loose_index.by_modules_dir.get(&modules_dir).into_iter().flatten() {
            dependencies.entry(name.clone()).or_insert_with(|| id.clone());
        }
        if !current.pop() {
            break;
        }
    }
    dependencies
}
pub(super) fn get_node_modules_path(package_location: &Path) -> Option<PathBuf> {
    let mut result = PathBuf::new();
    let mut last_node_modules = None;
    for component in package_location.components() {
        result.push(component.as_os_str());
        if component.as_os_str() == "node_modules" {
            last_node_modules = Some(result.clone());
        }
    }
    last_node_modules
}
pub(super) struct LinkTarget {
    pub(super) id: String,
    pub(super) dir: PathBuf,
}
pub(super) fn resolve_link_target(
    lockfile_dir: &Path,
    importer_id: Option<&str>,
    target: &str,
) -> LinkTarget {
    let importer_dir =
        importer_id.map_or_else(|| lockfile_dir.to_path_buf(), |id| lockfile_dir.join(id));
    let dir = if Path::new(target).is_absolute() {
        PathBuf::from(target)
    } else {
        importer_dir.join(target)
    };
    let dir = lexical_normalize(&dir);
    let id = link_target_id(pathdiff::diff_paths(&dir, lockfile_dir), &dir);
    LinkTarget { id, dir }
}
