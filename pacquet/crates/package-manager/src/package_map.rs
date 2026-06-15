use crate::LockfileToDepGraphResult;
use pacquet_config::{Config, NodePackageMapType};
use pacquet_fs::lexical_normalize;
use pacquet_lockfile::{Lockfile, PackageKey, ProjectSnapshot, SnapshotDepRef};
use pacquet_package_manifest::PackageManifest;
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

pub(crate) const PACKAGE_MAP_FILENAME: &str = ".package-map.json";

#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct PackageMap {
    packages: BTreeMap<String, PackageMapPackage>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct PackageMapPackage {
    url: String,
    dependencies: BTreeMap<String, String>,
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub enum WritePackageMapError {
    #[display("failed to create package-map directory: {_0}")]
    CreateDir(#[error(source)] std::io::Error),
    #[display("failed to serialize package map: {_0}")]
    Serialize(#[error(source)] serde_json::Error),
    #[display("failed to write package map: {_0}")]
    Write(#[error(source)] std::io::Error),
}

pub(crate) struct PackageMapOptions<'a> {
    pub lockfile_dir: &'a Path,
    pub modules_dir: &'a Path,
    pub package_map_type: NodePackageMapType,
    pub virtual_store_dir: &'a Path,
    pub virtual_store_dir_max_length: usize,
    pub project_manifests: &'a [(PathBuf, &'a PackageManifest)],
}

pub(crate) struct HoistedPackageMapOptions<'a> {
    pub lockfile_dir: &'a Path,
    pub modules_dir: &'a Path,
    pub package_map_type: NodePackageMapType,
    pub project_manifests: &'a [(PathBuf, &'a PackageManifest)],
}

pub(crate) fn write_package_map(
    lockfile: &Lockfile,
    opts: &PackageMapOptions<'_>,
) -> Result<(), WritePackageMapError> {
    std::fs::create_dir_all(opts.modules_dir).map_err(WritePackageMapError::CreateDir)?;
    let mut contents = serde_json::to_vec_pretty(&lockfile_to_package_map(lockfile, opts))
        .map_err(WritePackageMapError::Serialize)?;
    contents.push(b'\n');
    std::fs::write(opts.modules_dir.join(PACKAGE_MAP_FILENAME), contents)
        .map_err(WritePackageMapError::Write)
}

pub(crate) fn write_hoisted_package_map(
    lockfile: &Lockfile,
    graph: &LockfileToDepGraphResult,
    opts: &HoistedPackageMapOptions<'_>,
) -> Result<(), WritePackageMapError> {
    std::fs::create_dir_all(opts.modules_dir).map_err(WritePackageMapError::CreateDir)?;
    let mut contents =
        serde_json::to_vec_pretty(&dependencies_graph_to_package_map(lockfile, graph, opts))
            .map_err(WritePackageMapError::Serialize)?;
    contents.push(b'\n');
    std::fs::write(opts.modules_dir.join(PACKAGE_MAP_FILENAME), contents)
        .map_err(WritePackageMapError::Write)
}

pub(crate) fn lockfile_to_package_map(
    lockfile: &Lockfile,
    opts: &PackageMapOptions<'_>,
) -> PackageMap {
    let is_loose = opts.package_map_type == NodePackageMapType::Loose;
    let mut packages = BTreeMap::new();
    let mut loose_index = is_loose.then(PhysicalPackageIndex::default);
    let mut package_dirs = is_loose.then(BTreeMap::new);
    let importer_names = importer_names(opts.lockfile_dir, opts.project_manifests);

    for (importer_id, importer) in lockfile.importers.iter() {
        let mut dependencies = BTreeMap::new();
        if let Some(Some(name)) = importer_names.get(importer_id) {
            dependencies.insert(name.clone(), importer_id.clone());
        }
        add_importer_dependencies(
            &mut packages,
            &mut dependencies,
            lockfile,
            opts,
            importer_id,
            importer,
        );
        let importer_dir = lexical_normalize(&opts.lockfile_dir.join(importer_id));
        add_package(
            &mut packages,
            importer_id.clone(),
            &mut package_dirs,
            importer_dir,
            dependencies,
            opts.modules_dir,
        );
        if let Some(loose_index) = loose_index.as_mut() {
            let importer_modules_dir =
                lexical_normalize(&opts.lockfile_dir.join(importer_id).join("node_modules"));
            add_physical_importer_dependencies(
                loose_index,
                &mut packages,
                lockfile,
                opts,
                &importer_modules_dir,
                importer.dependencies.as_ref(),
                Some(importer_id),
            );
            add_physical_importer_dependencies(
                loose_index,
                &mut packages,
                lockfile,
                opts,
                &importer_modules_dir,
                importer.optional_dependencies.as_ref(),
                Some(importer_id),
            );
            add_physical_importer_dependencies(
                loose_index,
                &mut packages,
                lockfile,
                opts,
                &importer_modules_dir,
                importer.dev_dependencies.as_ref(),
                Some(importer_id),
            );
        }
    }

    if let Some(snapshots) = lockfile.snapshots.as_ref() {
        for (key, snapshot) in snapshots {
            let id = key.to_string();
            let mut dependencies = BTreeMap::new();
            dependencies.insert(key.name.to_string(), id.clone());
            add_snapshot_dependencies(
                &mut packages,
                &mut dependencies,
                lockfile,
                opts,
                snapshot.dependencies.as_ref(),
            );
            add_snapshot_dependencies(
                &mut packages,
                &mut dependencies,
                lockfile,
                opts,
                snapshot.optional_dependencies.as_ref(),
            );
            add_package(
                &mut packages,
                id,
                &mut package_dirs,
                package_dir(key, opts.virtual_store_dir, opts.virtual_store_dir_max_length),
                dependencies,
                opts.modules_dir,
            );
            if let Some(loose_index) = loose_index.as_mut() {
                let package_dir =
                    package_dir(key, opts.virtual_store_dir, opts.virtual_store_dir_max_length);
                if let Some(modules_dir) = get_node_modules_path(&package_dir) {
                    loose_index.add(modules_dir, key.name.to_string(), key.to_string());
                }
                let package_modules_dir = package_dir.join("node_modules");
                add_physical_snapshot_dependencies(
                    loose_index,
                    &mut packages,
                    lockfile,
                    opts,
                    &package_modules_dir,
                    snapshot.dependencies.as_ref(),
                );
                add_physical_snapshot_dependencies(
                    loose_index,
                    &mut packages,
                    lockfile,
                    opts,
                    &package_modules_dir,
                    snapshot.optional_dependencies.as_ref(),
                );
            }
        }
    }

    if let Some(metadata) = lockfile.packages.as_ref() {
        for key in metadata.keys() {
            let id = key.to_string();
            packages.entry(id.clone()).or_insert_with(|| {
                let mut dependencies = BTreeMap::new();
                dependencies.insert(key.name.to_string(), id.clone());
                PackageMapPackage {
                    url: to_relative_url(
                        opts.modules_dir,
                        &package_dir(
                            key,
                            opts.virtual_store_dir,
                            opts.virtual_store_dir_max_length,
                        ),
                    ),
                    dependencies,
                }
            });
            if let Some(package_dirs) = package_dirs.as_mut() {
                package_dirs.entry(id).or_insert_with(|| {
                    package_dir(key, opts.virtual_store_dir, opts.virtual_store_dir_max_length)
                });
            }
        }
    }

    add_loose_dependencies(&mut packages, package_dirs.as_ref(), loose_index.as_ref());

    PackageMap { packages }
}

pub(crate) fn dependencies_graph_to_package_map(
    lockfile: &Lockfile,
    graph: &LockfileToDepGraphResult,
    opts: &HoistedPackageMapOptions<'_>,
) -> PackageMap {
    let is_loose = opts.package_map_type == NodePackageMapType::Loose;
    let mut packages = BTreeMap::new();
    let mut package_ids_by_graph_key = BTreeMap::new();
    let mut package_ids_by_dep_path = BTreeMap::new();
    let mut package_dirs = is_loose.then(BTreeMap::new);
    let mut loose_index = is_loose.then(PhysicalPackageIndex::default);
    let importer_names = importer_names(opts.lockfile_dir, opts.project_manifests);

    for (graph_key, node) in &graph.graph {
        let id = graph_package_id(&node.dir, opts.modules_dir);
        package_ids_by_graph_key.insert(graph_key.clone(), id.clone());
        package_ids_by_dep_path.entry(node.dep_path.as_str().to_string()).or_insert(id.clone());
        if let Some(loose_index) = loose_index.as_mut()
            && let Some(modules_dir) = get_node_modules_path(&node.dir)
        {
            loose_index.add(modules_dir, node.name.clone(), id);
        }
    }

    for (importer_id, importer) in &lockfile.importers {
        let importer_dir = lexical_normalize(&opts.lockfile_dir.join(importer_id));
        let importer_id_for_map = graph_package_id(&importer_dir, opts.modules_dir);
        let mut dependencies = BTreeMap::new();
        if let Some(Some(name)) = importer_names.get(importer_id) {
            dependencies.insert(name.clone(), importer_id_for_map.clone());
        }
        add_hoisted_importer_dependencies(
            &mut dependencies,
            importer.dependencies.as_ref(),
            &package_ids_by_dep_path,
        );
        add_hoisted_importer_dependencies(
            &mut dependencies,
            importer.optional_dependencies.as_ref(),
            &package_ids_by_dep_path,
        );
        add_hoisted_importer_dependencies(
            &mut dependencies,
            importer.dev_dependencies.as_ref(),
            &package_ids_by_dep_path,
        );
        let importer_modules_dir = is_loose.then(|| importer_dir.join("node_modules"));
        add_hoisted_linked_dependencies(
            &mut packages,
            &mut dependencies,
            &mut loose_index,
            opts,
            importer.dependencies.as_ref(),
            Some(importer_id),
            importer_modules_dir.as_deref(),
        );
        add_hoisted_linked_dependencies(
            &mut packages,
            &mut dependencies,
            &mut loose_index,
            opts,
            importer.optional_dependencies.as_ref(),
            Some(importer_id),
            importer_modules_dir.as_deref(),
        );
        add_hoisted_linked_dependencies(
            &mut packages,
            &mut dependencies,
            &mut loose_index,
            opts,
            importer.dev_dependencies.as_ref(),
            Some(importer_id),
            importer_modules_dir.as_deref(),
        );
        add_package(
            &mut packages,
            importer_id_for_map,
            &mut package_dirs,
            importer_dir,
            dependencies,
            opts.modules_dir,
        );
    }

    for (graph_key, node) in &graph.graph {
        let id = package_ids_by_graph_key[graph_key].clone();
        let mut dependencies = BTreeMap::from([(node.name.clone(), id.clone())]);
        add_hoisted_graph_dependencies(
            &mut dependencies,
            &node.children,
            &package_ids_by_graph_key,
        );

        if let Some(snapshot) = lockfile.snapshots.as_ref().and_then(|snapshots| {
            node.dep_path.as_str().parse::<PackageKey>().ok().and_then(|key| snapshots.get(&key))
        }) {
            let package_modules_dir = is_loose.then(|| node.dir.join("node_modules"));
            add_hoisted_linked_dependencies(
                &mut packages,
                &mut dependencies,
                &mut loose_index,
                opts,
                snapshot.dependencies.as_ref(),
                None,
                package_modules_dir.as_deref(),
            );
            add_hoisted_linked_dependencies(
                &mut packages,
                &mut dependencies,
                &mut loose_index,
                opts,
                snapshot.optional_dependencies.as_ref(),
                None,
                package_modules_dir.as_deref(),
            );
        }

        add_package(
            &mut packages,
            id,
            &mut package_dirs,
            node.dir.clone(),
            dependencies,
            opts.modules_dir,
        );
    }

    add_loose_dependencies(&mut packages, package_dirs.as_ref(), loose_index.as_ref());

    PackageMap { packages }
}

pub fn make_node_package_map_option(package_map_path: &Path, node_options: Option<&str>) -> String {
    let node_options =
        node_options.map(str::to_string).or_else(|| std::env::var("NODE_OPTIONS").ok());
    let mut parts = remove_node_package_map_option(node_options.as_deref().unwrap_or_default());
    parts.push(format!(
        "--experimental-package-map={}",
        quote_path_if_needed(&package_map_path.to_string_lossy())
    ));
    parts.join(" ")
}

pub fn package_map_path_for_execution(config: &Config, dir: &Path) -> Option<PathBuf> {
    if !config.node_experimental_package_map {
        return None;
    }
    let workspace_path = config
        .workspace_dir
        .as_ref()
        .map(|dir| dir.join("node_modules").join(PACKAGE_MAP_FILENAME));
    if let Some(path) = workspace_path
        && path.exists()
    {
        return Some(path);
    }
    let path = dir.join("node_modules").join(PACKAGE_MAP_FILENAME);
    path.exists().then_some(path)
}

fn add_importer_dependencies(
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

fn add_physical_importer_dependencies(
    loose_index: &mut PhysicalPackageIndex,
    packages: &mut BTreeMap<String, PackageMapPackage>,
    lockfile: &Lockfile,
    opts: &PackageMapOptions<'_>,
    modules_dir: &Path,
    deps: Option<&pacquet_lockfile::ResolvedDependencyMap>,
    importer_id: Option<&str>,
) {
    let Some(deps) = deps else { return };
    for (alias, spec) in deps {
        if let Some(target) = spec.version.as_link_target() {
            let target = resolve_link_target(opts.lockfile_dir, importer_id, target);
            add_external_link_package(packages, &target, opts.modules_dir);
            loose_index.add(modules_dir.to_path_buf(), alias.to_string(), target.id);
            continue;
        }
        if let Some(key) = spec.version.resolved_key(alias)
            && has_package_entry(lockfile, &key)
        {
            loose_index.add(modules_dir.to_path_buf(), alias.to_string(), key.to_string());
        }
    }
}

fn add_snapshot_dependencies(
    packages: &mut BTreeMap<String, PackageMapPackage>,
    dependencies: &mut BTreeMap<String, String>,
    lockfile: &Lockfile,
    opts: &PackageMapOptions<'_>,
    deps: Option<&HashMap<pacquet_lockfile::PkgName, SnapshotDepRef>>,
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

fn add_physical_snapshot_dependencies(
    loose_index: &mut PhysicalPackageIndex,
    packages: &mut BTreeMap<String, PackageMapPackage>,
    lockfile: &Lockfile,
    opts: &PackageMapOptions<'_>,
    modules_dir: &Path,
    deps: Option<&HashMap<pacquet_lockfile::PkgName, SnapshotDepRef>>,
) {
    let Some(deps) = deps else { return };
    for (alias, reference) in deps {
        if let Some(target) = reference.as_link_target() {
            let target = resolve_link_target(opts.lockfile_dir, None, target);
            add_external_link_package(packages, &target, opts.modules_dir);
            loose_index.add(modules_dir.to_path_buf(), alias.to_string(), target.id);
            continue;
        }
        if let Some(key) = reference.resolve(alias)
            && has_package_entry(lockfile, &key)
        {
            loose_index.add(modules_dir.to_path_buf(), alias.to_string(), key.to_string());
        }
    }
}

fn add_hoisted_importer_dependencies(
    dependencies: &mut BTreeMap<String, String>,
    deps: Option<&pacquet_lockfile::ResolvedDependencyMap>,
    package_ids_by_dep_path: &BTreeMap<String, String>,
) {
    let Some(deps) = deps else { return };
    for (alias, spec) in deps {
        if spec.version.as_link_target().is_some() {
            continue;
        }
        if let Some(key) = spec.version.resolved_key(alias)
            && let Some(id) = package_ids_by_dep_path.get(&key.to_string())
        {
            dependencies.insert(alias.to_string(), id.clone());
        }
    }
}

fn add_hoisted_graph_dependencies(
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

fn add_hoisted_linked_dependencies<T>(
    packages: &mut BTreeMap<String, PackageMapPackage>,
    dependencies: &mut BTreeMap<String, String>,
    loose_index: &mut Option<PhysicalPackageIndex>,
    opts: &HoistedPackageMapOptions<'_>,
    deps: Option<&HashMap<pacquet_lockfile::PkgName, T>>,
    importer_id: Option<&str>,
    modules_dir: Option<&Path>,
) where
    T: LinkReference,
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
            loose_index.add(modules_dir.to_path_buf(), alias.to_string(), id);
        }
    }
}

trait LinkReference {
    fn as_link_target(&self) -> Option<&'_ str>;
}

impl LinkReference for pacquet_lockfile::ResolvedDependencySpec {
    fn as_link_target(&self) -> Option<&'_ str> {
        self.version.as_link_target()
    }
}

impl LinkReference for SnapshotDepRef {
    fn as_link_target(&self) -> Option<&'_ str> {
        SnapshotDepRef::as_link_target(self)
    }
}

fn has_package_entry(lockfile: &Lockfile, key: &PackageKey) -> bool {
    lockfile.snapshots.as_ref().is_some_and(|snapshots| snapshots.contains_key(key))
        || lockfile.packages.as_ref().is_some_and(|packages| packages.contains_key(key))
}

fn add_package(
    packages: &mut BTreeMap<String, PackageMapPackage>,
    id: String,
    package_dirs: &mut Option<BTreeMap<String, PathBuf>>,
    package_dir: PathBuf,
    dependencies: BTreeMap<String, String>,
    modules_dir: &Path,
) {
    if let Some(package_dirs) = package_dirs {
        package_dirs.insert(id.clone(), package_dir.clone());
    }
    packages.insert(
        id,
        PackageMapPackage { url: to_relative_url(modules_dir, &package_dir), dependencies },
    );
}

fn add_external_link_package(
    packages: &mut BTreeMap<String, PackageMapPackage>,
    target: &LinkTarget,
    modules_dir: &Path,
) {
    packages.entry(target.id.clone()).or_insert_with(|| PackageMapPackage {
        url: to_relative_url(modules_dir, &target.dir),
        dependencies: BTreeMap::new(),
    });
}

#[derive(Debug, Default)]
struct PhysicalPackageIndex {
    by_modules_dir: BTreeMap<String, BTreeMap<String, String>>,
}

impl PhysicalPackageIndex {
    fn add(&mut self, modules_dir: PathBuf, package_name: String, package_id: String) {
        self.by_modules_dir
            .entry(normalize_path(&lexical_normalize(&modules_dir)))
            .or_default()
            .insert(package_name, package_id);
    }
}

fn add_loose_dependencies(
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

fn physical_dependencies(
    package_dir: &Path,
    loose_index: &PhysicalPackageIndex,
) -> BTreeMap<String, String> {
    let mut dependencies = BTreeMap::new();
    let mut current = package_dir.to_path_buf();
    loop {
        let modules_dir = normalize_path(&current.join("node_modules"));
        if let Some(locations) = loose_index.by_modules_dir.get(&modules_dir) {
            for (name, id) in locations {
                dependencies.entry(name.clone()).or_insert_with(|| id.clone());
            }
        }
        if !current.pop() {
            break;
        }
    }
    dependencies
}

fn get_node_modules_path(package_location: &Path) -> Option<PathBuf> {
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

struct LinkTarget {
    id: String,
    dir: PathBuf,
}

fn resolve_link_target(lockfile_dir: &Path, importer_id: Option<&str>, target: &str) -> LinkTarget {
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

fn package_dir(
    key: &PackageKey,
    virtual_store_dir: &Path,
    virtual_store_dir_max_length: usize,
) -> PathBuf {
    virtual_store_dir
        .join(key.to_virtual_store_name(virtual_store_dir_max_length))
        .join("node_modules")
        .join(key.name.to_string())
}

fn importer_names(
    lockfile_dir: &Path,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> BTreeMap<String, Option<String>> {
    project_manifests
        .iter()
        .map(|(project_dir, manifest)| {
            let relative = pathdiff::diff_paths(project_dir, lockfile_dir)
                .unwrap_or_else(|| project_dir.clone());
            let id = normalize_path(&relative);
            let id = if id.is_empty() { ".".to_string() } else { id };
            (id, manifest_string_field(manifest, "name"))
        })
        .collect()
}

fn manifest_string_field(manifest: &PackageManifest, key: &str) -> Option<String> {
    manifest.value().get(key).and_then(|v| v.as_str()).map(ToString::to_string)
}

fn to_relative_url(from: &Path, to: &Path) -> String {
    let Some(relative) = pathdiff::diff_paths(to, from) else {
        return absolute_package_url(to);
    };
    let relative = normalize_path(&relative);
    let relative = if relative.is_empty() { ".".to_string() } else { relative };
    if relative == "."
        || relative == ".."
        || relative.starts_with("./")
        || relative.starts_with("../")
    {
        relative
    } else {
        format!("./{relative}")
    }
}

fn link_target_id(relative: Option<PathBuf>, dir: &Path) -> String {
    let Some(relative) = relative else {
        return format!("link:{}", normalize_path(dir));
    };
    let relative_id = normalize_path(&relative);
    if relative_id == ".." || relative_id.starts_with("../") {
        format!("link:{}", normalize_path(dir))
    } else if relative_id.is_empty() {
        ".".to_string()
    } else {
        relative_id
    }
}

fn graph_package_id(package_dir: &Path, modules_dir: &Path) -> String {
    let package_dir = lexical_normalize(package_dir);
    let Some(relative) = pathdiff::diff_paths(&package_dir, modules_dir) else {
        return format!("link:{}", normalize_path(&package_dir));
    };
    let relative = normalize_path(&relative);
    if relative == ".." {
        ".".to_string()
    } else if relative.is_empty() {
        ".".to_string()
    } else {
        relative
    }
}

fn remove_node_package_map_option(node_options: &str) -> Vec<String> {
    let tokens = split_node_options(node_options);
    let mut retained = Vec::new();
    let mut skip_next = false;
    for token in tokens {
        if skip_next {
            skip_next = false;
            continue;
        }
        if token == "--experimental-package-map" {
            skip_next = true;
            continue;
        }
        if token.starts_with("--experimental-package-map=") {
            continue;
        }
        retained.push(token);
    }
    retained
}

fn split_node_options(node_options: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quote: Option<char> = None;
    for ch in node_options.chars() {
        if let Some(q) = quote {
            token.push(ch);
            if ch == q {
                quote = None;
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            token.push(ch);
        } else if ch.is_whitespace() {
            if !token.is_empty() {
                tokens.push(std::mem::take(&mut token));
            }
        } else {
            token.push(ch);
        }
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    tokens
}

fn quote_path_if_needed(path: &str) -> String {
    if path.chars().any(char::is_whitespace) {
        serde_json::to_string(path).expect("serializing a string cannot fail")
    } else {
        path.to_string()
    }
}

fn absolute_package_url(path: &Path) -> String {
    let normalized = normalize_path(path);
    if cfg!(windows) && normalized.starts_with("//") {
        format!("file:{}", encode_url_path(&normalized))
    } else if cfg!(windows) && !normalized.starts_with('/') {
        format!("file:///{}", encode_url_path(&normalized))
    } else {
        format!("file://{}", encode_url_path(&normalized))
    }
}

fn encode_url_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::{
        HoistedPackageMapOptions, PackageMapOptions, absolute_package_url,
        dependencies_graph_to_package_map, link_target_id, lockfile_to_package_map,
        make_node_package_map_option, to_relative_url,
    };
    use crate::{DependenciesGraphNode, LockfileToDepGraphResult};
    use pacquet_lockfile::{
        ComVer, Lockfile, LockfileResolution, LockfileVersion, PackageKey, PkgIdWithPatchHash,
        PkgName, ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef,
        SnapshotEntry, TarballResolution,
    };
    use pacquet_modules_yaml::DepPath;
    use pacquet_package_manifest::PackageManifest;
    use std::{
        collections::{BTreeMap, BTreeSet, HashMap},
        path::{Path, PathBuf},
    };

    #[test]
    fn builds_package_map_from_lockfile() {
        let cwd = std::env::current_dir().expect("current dir");
        let root_manifest = manifest("root");
        let app_manifest = manifest("app");
        let linked_manifest = manifest("linked");
        let project_manifests = vec![
            (cwd.clone(), &root_manifest),
            (cwd.join("packages/app"), &app_manifest),
            (cwd.join("packages/linked"), &linked_manifest),
        ];
        let package_map = lockfile_to_package_map(
            &Lockfile {
                importers: HashMap::from([
                    (
                        ".".to_string(),
                        ProjectSnapshot {
                            dependencies: Some(deps(&[
                                ("dep1", "1.0.0"),
                                ("dep2-alias", "foo@2.0.0"),
                                ("linked", "link:packages/linked"),
                            ])),
                            ..ProjectSnapshot::default()
                        },
                    ),
                    (
                        "packages/app".to_string(),
                        ProjectSnapshot {
                            dependencies: Some(deps(&[
                                ("dep1", "1.0.0"),
                                ("linked", "link:../linked"),
                            ])),
                            dev_dependencies: Some(deps(&[("dep2-alias", "foo@2.0.0")])),
                            ..ProjectSnapshot::default()
                        },
                    ),
                    (
                        "packages/linked".to_string(),
                        ProjectSnapshot {
                            dependencies: Some(deps(&[("qar", "3.0.0")])),
                            ..ProjectSnapshot::default()
                        },
                    ),
                ]),
                snapshots: Some(HashMap::from([
                    ("dep1@1.0.0".parse().unwrap(), snapshot_deps(&[("dep2-alias", "foo@2.0.0")])),
                    ("foo@2.0.0".parse().unwrap(), snapshot_optional_deps(&[("qar", "3.0.0")])),
                    ("qar@3.0.0".parse().unwrap(), SnapshotEntry::default()),
                ])),
                ..empty_lockfile()
            },
            &PackageMapOptions {
                lockfile_dir: &cwd,
                modules_dir: &cwd.join("node_modules"),
                package_map_type: pacquet_config::NodePackageMapType::Standard,
                virtual_store_dir: &cwd.join("node_modules/.pnpm"),
                virtual_store_dir_max_length: 120,
                project_manifests: &project_manifests,
            },
        );

        assert_eq!(
            serde_json::to_value(&package_map).expect("serialize package map"),
            serde_json::json!({
                "packages": {
                    ".": {
                        "url": "..",
                        "dependencies": {
                            "dep1": "dep1@1.0.0",
                            "dep2-alias": "foo@2.0.0",
                            "linked": "packages/linked",
                            "root": "."
                        }
                    },
                    "dep1@1.0.0": {
                        "url": "./.pnpm/dep1@1.0.0/node_modules/dep1",
                        "dependencies": {
                            "dep1": "dep1@1.0.0",
                            "dep2-alias": "foo@2.0.0"
                        }
                    },
                    "foo@2.0.0": {
                        "url": "./.pnpm/foo@2.0.0/node_modules/foo",
                        "dependencies": {
                            "foo": "foo@2.0.0",
                            "qar": "qar@3.0.0"
                        }
                    },
                    "packages/app": {
                        "url": "../packages/app",
                        "dependencies": {
                            "app": "packages/app",
                            "dep1": "dep1@1.0.0",
                            "dep2-alias": "foo@2.0.0",
                            "linked": "packages/linked"
                        }
                    },
                    "packages/linked": {
                        "url": "../packages/linked",
                        "dependencies": {
                            "linked": "packages/linked",
                            "qar": "qar@3.0.0"
                        }
                    },
                    "qar@3.0.0": {
                        "url": "./.pnpm/qar@3.0.0/node_modules/qar",
                        "dependencies": {
                            "qar": "qar@3.0.0"
                        }
                    }
                }
            })
        );
    }

    #[test]
    fn link_target_id_uses_link_prefix_when_relative_path_cannot_be_computed() {
        let dir = PathBuf::from("/outside/store/pkg");
        assert_eq!(link_target_id(None, &dir), "link:/outside/store/pkg");
    }

    #[test]
    fn lockfile_package_map_loose_mode_includes_physical_ancestor_dependencies() {
        let cwd = std::env::current_dir().expect("current dir");
        let root_manifest = manifest("root");
        let project_manifests = vec![(cwd.clone(), &root_manifest)];
        let lockfile = Lockfile {
            importers: HashMap::from([(
                ".".to_string(),
                ProjectSnapshot {
                    dependencies: Some(deps(&[
                        ("dep1", "1.0.0"),
                        ("linked", "link:packages/linked"),
                    ])),
                    ..ProjectSnapshot::default()
                },
            )]),
            snapshots: Some(HashMap::from([(
                "dep1@1.0.0".parse().unwrap(),
                SnapshotEntry::default(),
            )])),
            ..empty_lockfile()
        };
        let standard_package_map = lockfile_to_package_map(
            &lockfile,
            &PackageMapOptions {
                lockfile_dir: &cwd,
                modules_dir: &cwd.join("node_modules"),
                package_map_type: pacquet_config::NodePackageMapType::Standard,
                virtual_store_dir: &cwd.join("node_modules/.pnpm"),
                virtual_store_dir_max_length: 120,
                project_manifests: &project_manifests,
            },
        );
        let loose_package_map = lockfile_to_package_map(
            &lockfile,
            &PackageMapOptions {
                lockfile_dir: &cwd,
                modules_dir: &cwd.join("node_modules"),
                package_map_type: pacquet_config::NodePackageMapType::Loose,
                virtual_store_dir: &cwd.join("node_modules/.pnpm"),
                virtual_store_dir_max_length: 120,
                project_manifests: &project_manifests,
            },
        );

        assert_eq!(
            standard_package_map.packages["dep1@1.0.0"].dependencies,
            BTreeMap::from([("dep1".to_string(), "dep1@1.0.0".to_string())])
        );
        assert_eq!(
            loose_package_map.packages["dep1@1.0.0"].dependencies,
            BTreeMap::from([
                ("dep1".to_string(), "dep1@1.0.0".to_string()),
                ("linked".to_string(), "packages/linked".to_string()),
            ])
        );
    }

    #[test]
    fn hoisted_package_map_loose_mode_includes_physical_ancestor_dependencies() {
        let cwd = std::env::current_dir().expect("current dir");
        let root_modules_dir = cwd.join("node_modules");
        let dep1_dir = root_modules_dir.join("dep1");
        let root_manifest = manifest("root");
        let project_manifests = vec![(cwd.clone(), &root_manifest)];
        let mut graph = LockfileToDepGraphResult::default();
        graph
            .direct_dependencies_by_importer_id
            .insert(".".to_string(), BTreeMap::from([("dep1".to_string(), dep1_dir.clone())]));
        graph.graph.insert(dep1_dir.clone(), graph_node("dep1", "1.0.0", &dep1_dir));
        let lockfile = Lockfile {
            importers: HashMap::from([(
                ".".to_string(),
                ProjectSnapshot {
                    dependencies: Some(deps(&[
                        ("dep1", "1.0.0"),
                        ("linked", "link:packages/linked"),
                    ])),
                    ..ProjectSnapshot::default()
                },
            )]),
            snapshots: Some(HashMap::from([(
                "dep1@1.0.0".parse().unwrap(),
                SnapshotEntry::default(),
            )])),
            ..empty_lockfile()
        };

        let package_map = dependencies_graph_to_package_map(
            &lockfile,
            &graph,
            &HoistedPackageMapOptions {
                lockfile_dir: &cwd,
                modules_dir: &root_modules_dir,
                package_map_type: pacquet_config::NodePackageMapType::Loose,
                project_manifests: &project_manifests,
            },
        );

        assert_eq!(
            package_map.packages["dep1"].dependencies,
            BTreeMap::from([
                ("dep1".to_string(), "dep1".to_string()),
                ("linked".to_string(), "../packages/linked".to_string()),
            ])
        );
    }

    #[test]
    fn hoisted_package_map_standard_mode_uses_declared_importer_dependencies_only() {
        let cwd = std::env::current_dir().expect("current dir");
        let root_modules_dir = cwd.join("node_modules");
        let dep1_dir = root_modules_dir.join("dep1");
        let dep2_dir = root_modules_dir.join("dep2");
        let root_manifest = manifest("root");
        let project_manifests = vec![(cwd.clone(), &root_manifest)];
        let mut graph = LockfileToDepGraphResult::default();
        graph.graph.insert(dep1_dir.clone(), graph_node("dep1", "1.0.0", &dep1_dir));
        graph.graph.insert(dep2_dir.clone(), graph_node("dep2", "1.0.0", &dep2_dir));
        let lockfile = Lockfile {
            importers: HashMap::from([(
                ".".to_string(),
                ProjectSnapshot {
                    dependencies: Some(deps(&[("dep1", "1.0.0")])),
                    ..ProjectSnapshot::default()
                },
            )]),
            snapshots: Some(HashMap::from([
                ("dep1@1.0.0".parse().unwrap(), SnapshotEntry::default()),
                ("dep2@1.0.0".parse().unwrap(), SnapshotEntry::default()),
            ])),
            ..empty_lockfile()
        };

        let standard_package_map = dependencies_graph_to_package_map(
            &lockfile,
            &graph,
            &HoistedPackageMapOptions {
                lockfile_dir: &cwd,
                modules_dir: &root_modules_dir,
                package_map_type: pacquet_config::NodePackageMapType::Standard,
                project_manifests: &project_manifests,
            },
        );
        let loose_package_map = dependencies_graph_to_package_map(
            &lockfile,
            &graph,
            &HoistedPackageMapOptions {
                lockfile_dir: &cwd,
                modules_dir: &root_modules_dir,
                package_map_type: pacquet_config::NodePackageMapType::Loose,
                project_manifests: &project_manifests,
            },
        );

        assert_eq!(
            standard_package_map.packages["."].dependencies,
            BTreeMap::from([
                ("dep1".to_string(), "dep1".to_string()),
                ("root".to_string(), ".".to_string()),
            ])
        );
        assert_eq!(
            loose_package_map.packages["."].dependencies,
            BTreeMap::from([
                ("dep1".to_string(), "dep1".to_string()),
                ("dep2".to_string(), "dep2".to_string()),
                ("root".to_string(), ".".to_string()),
            ])
        );
    }

    #[test]
    fn package_map_node_options_replaces_existing_package_map_option() {
        assert_eq!(
            make_node_package_map_option(
                Path::new("/repo/node_modules/.package-map.json"),
                Some("--require ./hook.cjs --experimental-package-map=old.json --trace-warnings"),
            ),
            "--require ./hook.cjs --trace-warnings --experimental-package-map=/repo/node_modules/.package-map.json"
        );
        assert_eq!(
            make_node_package_map_option(
                Path::new("/repo with spaces/node_modules/.package-map.json"),
                Some("--experimental-package-map old.json"),
            ),
            "--experimental-package-map=\"/repo with spaces/node_modules/.package-map.json\""
        );
    }

    #[test]
    fn link_target_id_uses_link_prefix_for_paths_above_the_lockfile_dir() {
        let dir = PathBuf::from("/outside/pkg");
        assert_eq!(
            link_target_id(Some(PathBuf::from("../outside/pkg")), &dir),
            "link:/outside/pkg"
        );
    }

    #[test]
    fn relative_url_uses_a_file_url_when_relative_path_cannot_be_computed() {
        assert_eq!(
            absolute_package_url(Path::new("/outside/pkg with space")),
            "file:///outside/pkg%20with%20space"
        );
    }

    #[test]
    fn relative_url_keeps_same_volume_paths_relative() {
        assert_eq!(
            to_relative_url(
                Path::new("/workspace/node_modules"),
                Path::new("/workspace/node_modules/.pnpm/foo")
            ),
            "./.pnpm/foo",
        );
    }

    fn manifest(name: &str) -> PackageManifest {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut manifest = PackageManifest::create_if_needed(dir.path().join("package.json"))
            .expect("create package manifest");
        manifest.value_mut()["name"] = serde_json::json!(name);
        manifest
    }

    fn deps(entries: &[(&str, &str)]) -> ResolvedDependencyMap {
        entries
            .iter()
            .map(|(alias, version)| {
                (
                    pkg(alias),
                    ResolvedDependencySpec {
                        specifier: (*version).to_string(),
                        version: (*version).parse().unwrap(),
                    },
                )
            })
            .collect()
    }

    fn snapshot_deps(entries: &[(&str, &str)]) -> SnapshotEntry {
        SnapshotEntry { dependencies: Some(snapshot_dep_map(entries)), ..SnapshotEntry::default() }
    }

    fn snapshot_optional_deps(entries: &[(&str, &str)]) -> SnapshotEntry {
        SnapshotEntry {
            optional_dependencies: Some(snapshot_dep_map(entries)),
            ..SnapshotEntry::default()
        }
    }

    fn snapshot_dep_map(entries: &[(&str, &str)]) -> HashMap<PkgName, SnapshotDepRef> {
        entries.iter().map(|(alias, version)| (pkg(alias), version.parse().unwrap())).collect()
    }

    fn pkg(name: &str) -> PkgName {
        name.parse().unwrap()
    }

    fn empty_lockfile() -> Lockfile {
        Lockfile {
            lockfile_version: LockfileVersion::<9>::try_from(ComVer { major: 9, minor: 0 })
                .unwrap(),
            settings: None,
            catalogs: None,
            overrides: None,
            package_extensions_checksum: None,
            pnpmfile_checksum: None,
            ignored_optional_dependencies: None,
            patched_dependencies: None,
            importers: HashMap::new(),
            packages: None,
            snapshots: None,
        }
    }

    fn graph_node(name: &str, version: &str, dir: &Path) -> DependenciesGraphNode {
        let key: PackageKey = format!("{name}@{version}").parse().unwrap();
        DependenciesGraphNode {
            alias: Some(name.to_string()),
            dep_path: DepPath::from(key.to_string()),
            pkg_id_with_patch_hash: PkgIdWithPatchHash::from(key.to_string()),
            dir: dir.to_path_buf(),
            modules: dir.parent().expect("package dir has parent").to_path_buf(),
            children: BTreeMap::new(),
            name: name.to_string(),
            version: version.to_string(),
            optional: false,
            optional_dependencies: BTreeSet::new(),
            has_bin: false,
            has_bundled_dependencies: false,
            patch: None,
            resolution: LockfileResolution::Tarball(TarballResolution {
                tarball: String::new(),
                integrity: None,
                git_hosted: None,
                path: None,
            }),
        }
    }
}
