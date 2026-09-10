pub use hoisted::dependencies_graph_to_package_map;
pub use node_options::{
    make_node_package_map_option, make_node_require_option, package_map_path_for_execution,
};

mod dependencies;
use dependencies::{
    LinkReference, LinkTarget, PhysicalPackageIndex, add_importer_dependencies,
    add_loose_dependencies, add_physical_importer_dependencies, add_physical_snapshot_dependencies,
    add_snapshot_dependencies, get_node_modules_path, resolve_link_target,
};

mod hoisted;

mod node_options;

use crate::LockfileToDepGraphResult;
use pnpm_config::NodePackageMapType;
use pnpm_fs::lexical_normalize;
use pnpm_lockfile::{Lockfile, PackageKey};
use pnpm_package_manifest::PackageManifest;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
};

pub const PACKAGE_MAP_FILENAME: &str = ".package-map.json";

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct PackageMap {
    packages: BTreeMap<String, PackageMapPackage>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct PackageMapPackage {
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
    Write(#[error(source)] pnpm_fs::EnsureFileError),
}

pub struct PackageMapOptions<'a> {
    pub lockfile_dir: &'a Path,
    pub modules_dir: &'a Path,
    pub package_map_type: NodePackageMapType,
    /// Resolves each snapshot to its real on-disk slot, so the map stays
    /// correct under both the legacy flat layout and the content-hashed
    /// global virtual store.
    pub layout: &'a crate::VirtualStoreLayout,
    pub project_manifests: &'a [(PathBuf, &'a PackageManifest)],
}

pub struct HoistedPackageMapOptions<'a> {
    pub lockfile_dir: &'a Path,
    pub modules_dir: &'a Path,
    pub package_map_type: NodePackageMapType,
    pub project_manifests: &'a [(PathBuf, &'a PackageManifest)],
}

/// Delete a `.package-map.json` an earlier install wrote.
///
/// An install that does not write the map must not leave the previous
/// one behind: [`package_map_path_for_execution`] finds the file by
/// existence, so a map left over from a run with
/// `nodeExperimentalPackageMap` on would be handed to Node the moment
/// the setting came back on, describing a dependency set that has since
/// changed.
///
/// Best-effort and infallible: an absent map is the wanted state, and
/// any other failure is logged and swallowed rather than failing an
/// install over a file nothing is going to read. `removePackageMap` in
/// `@pnpm/lockfile.to-pnp` makes the same promise, so both stacks leave
/// an install in the same state when the removal cannot happen.
pub fn remove_package_map(modules_dir: &std::path::Path) {
    let path = modules_dir.join(PACKAGE_MAP_FILENAME);
    if let Err(error) = std::fs::remove_file(&path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::debug!(
            target: "pacquet::install",
            ?path,
            ?error,
            "could not remove a stale package map",
        );
    }
}

pub fn write_package_map(
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
    pnpm_fs::ensure_file(&opts.modules_dir.join(PACKAGE_MAP_FILENAME), &contents, None)
        .map_err(WritePackageMapError::Write)
}

pub fn write_hoisted_package_map(
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
    pnpm_fs::ensure_file(&opts.modules_dir.join(PACKAGE_MAP_FILENAME), &contents, None)
        .map_err(WritePackageMapError::Write)
}

pub fn lockfile_to_package_map(lockfile: &Lockfile, opts: &PackageMapOptions<'_>) -> PackageMap {
    let is_loose = opts.package_map_type == NodePackageMapType::Loose;
    let mut accum = PackageMapAccum {
        packages: BTreeMap::new(),
        package_dirs: is_loose.then(BTreeMap::new),
        loose_index: is_loose.then(PhysicalPackageIndex::default),
    };
    let importer_names = importer_names(opts.lockfile_dir, opts.project_manifests);

    for (importer_id, importer) in &lockfile.importers {
        add_importer_package(
            &mut accum,
            lockfile,
            opts,
            importer_id,
            importer,
            importer_names.get(importer_id).and_then(Option::as_ref),
        );
    }

    for (key, snapshot) in lockfile.snapshots.iter().flatten() {
        add_snapshot_package(&mut accum, lockfile, opts, key, snapshot);
    }

    // A package with metadata but no snapshot still needs a map entry:
    // it resolves to its own slot and to nothing else.
    for key in lockfile.packages.iter().flatten().map(|(key, _)| key) {
        add_metadata_only_package(&mut accum, opts, key);
    }

    add_loose_dependencies(
        &mut accum.packages,
        accum.package_dirs.as_ref(),
        accum.loose_index.as_ref(),
    );

    PackageMap { packages: accum.packages }
}

/// The maps a package-map build accumulates. `package_dirs` and
/// `loose_index` are `Some` only under
/// [`NodePackageMapType::Loose`], which resolves through physical
/// directories on top of the lockfile's edges.
struct PackageMapAccum {
    packages: BTreeMap<String, PackageMapPackage>,
    package_dirs: Option<BTreeMap<String, PathBuf>>,
    loose_index: Option<PhysicalPackageIndex>,
}

fn add_importer_package(
    accum: &mut PackageMapAccum,
    lockfile: &Lockfile,
    opts: &PackageMapOptions<'_>,
    importer_id: &String,
    importer: &pnpm_lockfile::ProjectSnapshot,
    importer_name: Option<&String>,
) {
    let PackageMapAccum { packages, package_dirs, loose_index } = accum;
    let mut dependencies = BTreeMap::new();
    if let Some(name) = importer_name {
        dependencies.insert(name.clone(), importer_id.clone());
    }
    add_importer_dependencies(packages, &mut dependencies, lockfile, opts, importer_id, importer);
    let importer_dir = lexical_normalize(&opts.lockfile_dir.join(importer_id));
    add_package(
        packages,
        importer_id.clone(),
        package_dirs,
        &importer_dir,
        dependencies,
        opts.modules_dir,
    );
    let Some(loose_index) = loose_index.as_mut() else { return };
    let importer_modules_dir =
        lexical_normalize(&opts.lockfile_dir.join(importer_id).join("node_modules"));
    for group in [
        importer.dependencies.as_ref(),
        importer.optional_dependencies.as_ref(),
        importer.dev_dependencies.as_ref(),
    ] {
        add_physical_importer_dependencies(
            loose_index,
            packages,
            lockfile,
            opts,
            &importer_modules_dir,
            group,
            Some(importer_id),
        );
    }
}

fn add_snapshot_package(
    accum: &mut PackageMapAccum,
    lockfile: &Lockfile,
    opts: &PackageMapOptions<'_>,
    key: &PackageKey,
    snapshot: &pnpm_lockfile::SnapshotEntry,
) {
    let PackageMapAccum { packages, package_dirs, loose_index } = accum;
    let id = key.to_string();
    let mut dependencies = BTreeMap::new();
    dependencies.insert(key.name.to_string(), id.clone());
    for group in [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()] {
        add_snapshot_dependencies(packages, &mut dependencies, lockfile, opts, group);
    }
    let package_dir = opts.layout.slot_dir(key).join("node_modules").join(key.name.to_string());
    add_package(packages, id, package_dirs, &package_dir, dependencies, opts.modules_dir);
    let Some(loose_index) = loose_index.as_mut() else { return };
    if let Some(modules_dir) = get_node_modules_path(&package_dir) {
        loose_index.add(&modules_dir, key.name.to_string(), key.to_string());
    }
    let package_modules_dir = package_dir.join("node_modules");
    for group in [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()] {
        add_physical_snapshot_dependencies(
            loose_index,
            packages,
            lockfile,
            opts,
            &package_modules_dir,
            group,
        );
    }
}

fn add_metadata_only_package(
    accum: &mut PackageMapAccum,
    opts: &PackageMapOptions<'_>,
    key: &PackageKey,
) {
    let id = key.to_string();
    let package_dir = || opts.layout.slot_dir(key).join("node_modules").join(key.name.to_string());
    accum.packages.entry(id.clone()).or_insert_with(|| {
        let mut dependencies = BTreeMap::new();
        dependencies.insert(key.name.to_string(), id.clone());
        PackageMapPackage { url: to_relative_url(opts.modules_dir, &package_dir()), dependencies }
    });
    if let Some(package_dirs) = accum.package_dirs.as_mut() {
        package_dirs.entry(id).or_insert_with(package_dir);
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
