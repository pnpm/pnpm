use super::{
    HoistedPackageMapOptions, LinkReference, LinkTarget, PackageMap, PackageMapPackage,
    PhysicalPackageIndex, add_external_link_package, add_loose_dependencies, add_package,
    get_node_modules_path, graph_package_id, importer_names, resolve_link_target,
};
use crate::LockfileToDepGraphResult;
use pnpm_config::NodePackageMapType;
use pnpm_fs::lexical_normalize;
use pnpm_lockfile::{Lockfile, PackageKey};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

pub fn dependencies_graph_to_package_map(
    lockfile: &Lockfile,
    graph: &LockfileToDepGraphResult,
    opts: &HoistedPackageMapOptions<'_>,
) -> PackageMap {
    let mut builder = HoistedMapBuilder::new(graph, opts);
    let importer_names = importer_names(opts.lockfile_dir, opts.project_manifests);
    for (importer_id, importer) in &lockfile.importers {
        builder.add_importer(
            importer_id,
            importer,
            importer_names.get(importer_id).and_then(Option::as_deref),
        );
    }
    for (graph_key, node) in &graph.graph {
        builder.add_graph_node(lockfile, graph_key, node);
    }
    builder.finish()
}
/// The hoisted package map under construction: the packages so far, the
/// ids graph nodes and pkg ids resolve to, and the loose-mode physical
/// index and directories.
pub(super) struct HoistedMapBuilder<'b> {
    opts: &'b HoistedPackageMapOptions<'b>,
    is_loose: bool,
    packages: BTreeMap<String, PackageMapPackage>,
    package_ids_by_graph_key: BTreeMap<PathBuf, String>,
    package_ids_by_pkg_id: BTreeMap<String, String>,
    package_dirs: Option<BTreeMap<String, PathBuf>>,
    loose_index: Option<PhysicalPackageIndex>,
}
impl<'b> HoistedMapBuilder<'b> {
    fn new(graph: &LockfileToDepGraphResult, opts: &'b HoistedPackageMapOptions<'b>) -> Self {
        let is_loose = opts.package_map_type == NodePackageMapType::Loose;
        let mut builder = Self {
            opts,
            is_loose,
            packages: BTreeMap::new(),
            package_ids_by_graph_key: BTreeMap::new(),
            package_ids_by_pkg_id: BTreeMap::new(),
            package_dirs: is_loose.then(BTreeMap::new),
            loose_index: is_loose.then(PhysicalPackageIndex::default),
        };
        index_graph_nodes(
            graph,
            opts.modules_dir,
            &mut builder.loose_index,
            &mut builder.package_ids_by_graph_key,
            &mut builder.package_ids_by_pkg_id,
        );
        builder
    }

    fn add_importer(
        &mut self,
        importer_id: &str,
        importer: &pnpm_lockfile::ProjectSnapshot,
        name: Option<&str>,
    ) {
        let importer_dir = lexical_normalize(&self.opts.lockfile_dir.join(importer_id));
        let importer_id_for_map = graph_package_id(&importer_dir, self.opts.modules_dir);
        let mut dependencies = BTreeMap::new();
        if let Some(name) = name {
            dependencies.insert(name.to_string(), importer_id_for_map.clone());
        }
        for deps in [
            importer.dependencies.as_ref(),
            importer.optional_dependencies.as_ref(),
            importer.dev_dependencies.as_ref(),
        ] {
            add_hoisted_importer_dependencies(&mut dependencies, deps, &self.package_ids_by_pkg_id);
        }
        let importer_modules_dir = self.is_loose.then(|| importer_dir.join("node_modules"));
        for deps in [
            importer.dependencies.as_ref(),
            importer.optional_dependencies.as_ref(),
            importer.dev_dependencies.as_ref(),
        ] {
            add_hoisted_linked_dependencies(
                &mut self.packages,
                &mut dependencies,
                &mut self.loose_index,
                self.opts,
                deps,
                Some(importer_id),
                importer_modules_dir.as_deref(),
            );
        }
        add_package(
            &mut self.packages,
            importer_id_for_map,
            &mut self.package_dirs,
            &importer_dir,
            dependencies,
            self.opts.modules_dir,
        );
    }

    fn add_graph_node(
        &mut self,
        lockfile: &Lockfile,
        graph_key: &Path,
        node: &crate::DependenciesGraphNode,
    ) {
        let id = self.package_ids_by_graph_key[graph_key].clone();
        let mut dependencies = BTreeMap::from([(node.name.clone(), id.clone())]);
        add_hoisted_graph_dependencies(
            &mut dependencies,
            &node.children,
            &self.package_ids_by_graph_key,
        );

        if let Some(snapshot) = lockfile.snapshots.as_ref().and_then(|snapshots| {
            node.dep_path.as_str().parse::<PackageKey>().ok().and_then(|key| snapshots.get(&key))
        }) {
            let package_modules_dir = self.is_loose.then(|| node.dir.join("node_modules"));
            for deps in [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()] {
                add_hoisted_linked_dependencies(
                    &mut self.packages,
                    &mut dependencies,
                    &mut self.loose_index,
                    self.opts,
                    deps,
                    None,
                    package_modules_dir.as_deref(),
                );
            }
        }

        add_package(
            &mut self.packages,
            id,
            &mut self.package_dirs,
            &node.dir,
            dependencies,
            self.opts.modules_dir,
        );
    }

    fn finish(self) -> PackageMap {
        let mut packages = self.packages;
        add_loose_dependencies(
            &mut packages,
            self.package_dirs.as_ref(),
            self.loose_index.as_ref(),
        );
        PackageMap { packages }
    }
}
/// Assign a package-map id to every hoisted graph node, indexed both by
/// graph key and by [`pnpm_real_hoist::pkg_id`].
///
/// The hoister collapses every peer variant of one package version onto
/// a single node, so an importer that declared another variant still
/// has to find this one (see [`crate::hoisted_dep_graph`]'s
/// `pkg_locations_by_pkg_id`).
pub(super) fn index_graph_nodes(
    graph: &LockfileToDepGraphResult,
    modules_dir: &Path,
    loose_index: &mut Option<PhysicalPackageIndex>,
    package_ids_by_graph_key: &mut BTreeMap<PathBuf, String>,
    package_ids_by_pkg_id: &mut BTreeMap<String, String>,
) {
    for (graph_key, node) in &graph.graph {
        let id = graph_package_id(&node.dir, modules_dir);
        package_ids_by_graph_key.insert(graph_key.clone(), id.clone());
        if let Ok(key) = node.dep_path.as_str().parse::<PackageKey>() {
            package_ids_by_pkg_id
                .entry(pnpm_real_hoist::pkg_id(&key))
                .or_insert_with(|| id.clone());
        }
        if let Some(loose_index) = loose_index.as_mut()
            && let Some(modules_dir) = get_node_modules_path(&node.dir)
        {
            loose_index.add(&modules_dir, node.name.clone(), id);
        }
    }
}
pub(super) fn add_hoisted_importer_dependencies(
    dependencies: &mut BTreeMap<String, String>,
    deps: Option<&pnpm_lockfile::ResolvedDependencyMap>,
    package_ids_by_pkg_id: &BTreeMap<String, String>,
) {
    let Some(deps) = deps else { return };
    for (alias, spec) in deps {
        if spec.version.as_link_target().is_some() {
            continue;
        }
        if let Some(key) = spec.version.resolved_key(alias)
            && let Some(id) = package_ids_by_pkg_id.get(&pnpm_real_hoist::pkg_id(&key))
        {
            dependencies.insert(alias.to_string(), id.clone());
        }
    }
}
pub(super) fn add_hoisted_graph_dependencies(
    dependencies: &mut BTreeMap<String, String>,
    deps: &BTreeMap<String, PathBuf>,
    package_ids_by_graph_key: &BTreeMap<PathBuf, String>,
) {
    for (alias, graph_key) in deps {
        if let Some(id) = package_ids_by_graph_key.get(graph_key) {
            dependencies.insert(alias.clone(), id.clone());
        }
    }
}
pub(super) fn add_hoisted_linked_dependencies<Reference>(
    packages: &mut BTreeMap<String, PackageMapPackage>,
    dependencies: &mut BTreeMap<String, String>,
    loose_index: &mut Option<PhysicalPackageIndex>,
    opts: &HoistedPackageMapOptions<'_>,
    deps: Option<&HashMap<pnpm_lockfile::PkgName, Reference>>,
    importer_id: Option<&str>,
    modules_dir: Option<&Path>,
) where
    Reference: LinkReference,
{
    let Some(deps) = deps else { return };
    for (alias, reference) in deps {
        let Some(target_ref) = reference.as_link_target() else { continue };
        let target = resolve_link_target(opts.lockfile_dir, importer_id, target_ref);
        let id = graph_package_id(&target.dir, opts.modules_dir);
        add_external_link_package(
            packages,
            &LinkTarget { id: id.clone(), dir: target.dir.clone() },
            opts.modules_dir,
        );
        dependencies.insert(alias.to_string(), id.clone());
        if let (Some(loose_index), Some(modules_dir)) = (loose_index.as_mut(), modules_dir) {
            loose_index.add(modules_dir, alias.to_string(), id);
        }
    }
}
