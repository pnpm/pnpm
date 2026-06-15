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
    pub virtual_store_dir: &'a Path,
    pub virtual_store_dir_max_length: usize,
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

pub(crate) fn lockfile_to_package_map(
    lockfile: &Lockfile,
    opts: &PackageMapOptions<'_>,
) -> PackageMap {
    let mut packages = BTreeMap::new();
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
        add_package(
            &mut packages,
            importer_id.clone(),
            opts.lockfile_dir.join(importer_id),
            dependencies,
            opts.modules_dir,
        );
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
                package_dir(key, opts.virtual_store_dir, opts.virtual_store_dir_max_length),
                dependencies,
                opts.modules_dir,
            );
        }
    }

    if let Some(metadata) = lockfile.packages.as_ref() {
        for key in metadata.keys() {
            let id = key.to_string();
            packages.entry(id.clone()).or_insert_with(|| {
                let mut dependencies = BTreeMap::new();
                dependencies.insert(key.name.to_string(), id);
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
        }
    }

    PackageMap { packages }
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

fn has_package_entry(lockfile: &Lockfile, key: &PackageKey) -> bool {
    lockfile.snapshots.as_ref().is_some_and(|snapshots| snapshots.contains_key(key))
        || lockfile.packages.as_ref().is_some_and(|packages| packages.contains_key(key))
}

fn add_package(
    packages: &mut BTreeMap<String, PackageMapPackage>,
    id: String,
    package_dir: PathBuf,
    dependencies: BTreeMap<String, String>,
    modules_dir: &Path,
) {
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
        PackageMapOptions, absolute_package_url, link_target_id, lockfile_to_package_map,
        to_relative_url,
    };
    use pacquet_lockfile::{
        ComVer, Lockfile, LockfileVersion, PkgName, ProjectSnapshot, ResolvedDependencyMap,
        ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
    };
    use pacquet_package_manifest::PackageManifest;
    use std::{
        collections::HashMap,
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
}
