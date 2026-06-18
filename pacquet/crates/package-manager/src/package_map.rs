use crate::LockfileToDepGraphResult;
use pacquet_config::{Config, NodePackageMapType};
use pacquet_fs::lexical_normalize;
use pacquet_lockfile::{Lockfile, PackageKey, ProjectSnapshot, SnapshotDepRef};
use pacquet_package_manifest::PackageManifest;
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write as _,
    path::{Path, PathBuf},
};

pub(crate) const PACKAGE_MAP_FILENAME: &str = ".package-map.json";

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct PackageMap {
    packages: BTreeMap<String, PackageMapPackage>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
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
    Write(#[error(source)] pacquet_fs::EnsureFileError),
}

pub(crate) struct PackageMapOptions<'a> {
    pub lockfile_dir: &'a Path,
    pub modules_dir: &'a Path,
    pub package_map_type: NodePackageMapType,
    /// Resolves each snapshot to its real on-disk slot, so the map stays
    /// correct under both the legacy flat layout and the content-hashed
    /// global virtual store.
    pub layout: &'a crate::VirtualStoreLayout,
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
    let mut contents = serde_json::to_vec(&lockfile_to_package_map(lockfile, opts))
        .map_err(WritePackageMapError::Serialize)?;
    contents.push(b'\n');
    // Hardened atomic write (temp file + rename): never follows a symlink an
    // attacker (or a crashed prior install) may have pre-seeded at the target,
    // and never leaves a torn file a concurrent reader could observe.
    pacquet_fs::ensure_file(&opts.modules_dir.join(PACKAGE_MAP_FILENAME), &contents, None)
        .map_err(WritePackageMapError::Write)
}

pub(crate) fn write_hoisted_package_map(
    lockfile: &Lockfile,
    graph: &LockfileToDepGraphResult,
    opts: &HoistedPackageMapOptions<'_>,
) -> Result<(), WritePackageMapError> {
    std::fs::create_dir_all(opts.modules_dir).map_err(WritePackageMapError::CreateDir)?;
    let mut contents =
        serde_json::to_vec(&dependencies_graph_to_package_map(lockfile, graph, opts))
            .map_err(WritePackageMapError::Serialize)?;
    contents.push(b'\n');
    // Hardened atomic write (temp file + rename): never follows a symlink an
    // attacker (or a crashed prior install) may have pre-seeded at the target,
    // and never leaves a torn file a concurrent reader could observe.
    pacquet_fs::ensure_file(&opts.modules_dir.join(PACKAGE_MAP_FILENAME), &contents, None)
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

    for (importer_id, importer) in &lockfile.importers {
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
            &importer_dir,
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
                &opts.layout.slot_dir(key).join("node_modules").join(key.name.to_string()),
                dependencies,
                opts.modules_dir,
            );
            if let Some(loose_index) = loose_index.as_mut() {
                let package_dir =
                    opts.layout.slot_dir(key).join("node_modules").join(key.name.to_string());
                if let Some(modules_dir) = get_node_modules_path(&package_dir) {
                    loose_index.add(&modules_dir, key.name.to_string(), key.to_string());
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
                        &opts.layout.slot_dir(key).join("node_modules").join(key.name.to_string()),
                    ),
                    dependencies,
                }
            });
            if let Some(package_dirs) = package_dirs.as_mut() {
                package_dirs.entry(id).or_insert_with(|| {
                    opts.layout.slot_dir(key).join("node_modules").join(key.name.to_string())
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
        package_ids_by_dep_path
            .entry(node.dep_path.as_str().to_string())
            .or_insert_with(|| id.clone());
        if let Some(loose_index) = loose_index.as_mut()
            && let Some(modules_dir) = get_node_modules_path(&node.dir)
        {
            loose_index.add(&modules_dir, node.name.clone(), id);
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
            &importer_dir,
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
            &node.dir,
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
        quote_path_if_needed(&package_map_path.to_string_lossy()),
    ));
    parts.join(" ")
}

pub fn package_map_path_for_execution(config: &Config, dir: &Path) -> Option<PathBuf> {
    if !config.node_experimental_package_map {
        return None;
    }
    // Installs write the map under the configured modules dir, so detect it
    // by that dir's basename rather than the hard-coded `node_modules`.
    let modules_dir_name =
        config.modules_dir.file_name().unwrap_or_else(|| std::ffi::OsStr::new("node_modules"));
    let workspace_path = config
        .workspace_dir
        .as_ref()
        .map(|dir| dir.join(modules_dir_name).join(PACKAGE_MAP_FILENAME));
    if let Some(path) = workspace_path
        && path.exists()
    {
        return Some(path);
    }
    let path = dir.join(modules_dir_name).join(PACKAGE_MAP_FILENAME);
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

fn add_hoisted_linked_dependencies<Reference>(
    packages: &mut BTreeMap<String, PackageMapPackage>,
    dependencies: &mut BTreeMap<String, String>,
    loose_index: &mut Option<PhysicalPackageIndex>,
    opts: &HoistedPackageMapOptions<'_>,
    deps: Option<&HashMap<pacquet_lockfile::PkgName, Reference>>,
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
    package_dir: &Path,
    dependencies: BTreeMap<String, String>,
    modules_dir: &Path,
) {
    if let Some(package_dirs) = package_dirs {
        package_dirs.insert(id.clone(), package_dir.to_path_buf());
    }
    packages.insert(
        id,
        PackageMapPackage { url: to_relative_url(modules_dir, package_dir), dependencies },
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
    fn add(&mut self, modules_dir: &Path, package_name: String, package_id: String) {
        self.by_modules_dir
            .entry(normalize_path(&lexical_normalize(modules_dir)))
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
    if relative == ".." || relative.is_empty() { ".".to_string() } else { relative }
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
    let mut escaped = false;
    for ch in node_options.chars() {
        // `\` escapes the next character anywhere, matching Node's
        // NODE_OPTIONS tokenizer, so an escaped quote does not end a token.
        // The literal text (backslash included) is preserved so retained
        // tokens round-trip verbatim.
        if escaped {
            token.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            token.push(ch);
            escaped = true;
            continue;
        }
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
    // Node's NODE_OPTIONS tokenizer treats whitespace as a separator, `'`/`"`
    // as quote delimiters, and `\` as an escape character (so a bare Windows
    // path would lose its separators). Wrap such paths in double quotes,
    // escaping only `\` and `"`. A full JSON encode is wrong here: Node does
    // not decode `\uXXXX`, so escaping non-ASCII bytes would corrupt the path.
    if path.chars().any(|ch| ch.is_whitespace() || matches!(ch, '"' | '\'' | '\\')) {
        let escaped = path.replace('\\', r"\\").replace('"', r#"\""#);
        format!("\"{escaped}\"")
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
                encoded.push(byte as char);
            }
            _ => write!(encoded, "%{byte:02X}").expect("writing to a string cannot fail"),
        }
    }
    encoded
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests;
