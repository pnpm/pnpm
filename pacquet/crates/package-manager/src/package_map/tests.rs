use super::{
    HoistedPackageMapOptions, PackageMapOptions, absolute_package_url,
    dependencies_graph_to_package_map, link_target_id, lockfile_to_package_map,
    make_node_package_map_option, to_relative_url,
};
use crate::{DependenciesGraphNode, LockfileToDepGraphResult, VirtualStoreLayout};
use pacquet_lockfile::{
    ComVer, Lockfile, LockfileResolution, LockfileVersion, PackageKey, PkgIdWithPatchHash, PkgName,
    ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
    TarballResolution,
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
    let layout = VirtualStoreLayout::legacy(cwd.join("node_modules/.pnpm"), 120);
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
            layout: &layout,
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
        }),
    );
}

#[test]
fn lockfile_package_map_uses_global_virtual_store_layout() {
    let cwd = std::env::current_dir().expect("current dir");
    let mut config = pacquet_config::Config::new();
    config.enable_global_virtual_store = true;
    config.global_virtual_store_dir = cwd.join("store/links");
    config.virtual_store_dir = cwd.join("node_modules/.pnpm");

    let snapshots =
        HashMap::from([("dep1@1.0.0".parse::<PackageKey>().unwrap(), SnapshotEntry::default())]);
    // GVS precomputes a `<name>/<version>/<hash>` slot per snapshot; the
    // package map must read those locations rather than the flat depPath name.
    let layout = VirtualStoreLayout::new(&config, None, Some(&snapshots), None, None);

    let root_manifest = manifest("root");
    let project_manifests = vec![(cwd.clone(), &root_manifest)];
    let package_map = lockfile_to_package_map(
        &Lockfile {
            importers: HashMap::from([(
                ".".to_string(),
                ProjectSnapshot {
                    dependencies: Some(deps(&[("dep1", "1.0.0")])),
                    ..ProjectSnapshot::default()
                },
            )]),
            snapshots: Some(snapshots),
            ..empty_lockfile()
        },
        &PackageMapOptions {
            lockfile_dir: &cwd,
            modules_dir: &cwd.join("node_modules"),
            package_map_type: pacquet_config::NodePackageMapType::Standard,
            layout: &layout,
            project_manifests: &project_manifests,
        },
    );

    let url = &package_map.packages["dep1@1.0.0"].url;
    assert!(
        url.contains("store/links/") && url.contains("/dep1/1.0.0/"),
        "expected a nested GVS slot url, got {url:?}",
    );
    assert!(
        !url.contains("dep1@1.0.0/node_modules"),
        "must not fall back to the flat local layout, got {url:?}",
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
    let layout = VirtualStoreLayout::legacy(cwd.join("node_modules/.pnpm"), 120);
    let root_manifest = manifest("root");
    let project_manifests = vec![(cwd.clone(), &root_manifest)];
    let lockfile = Lockfile {
        importers: HashMap::from([(
            ".".to_string(),
            ProjectSnapshot {
                dependencies: Some(deps(&[("dep1", "1.0.0"), ("linked", "link:packages/linked")])),
                ..ProjectSnapshot::default()
            },
        )]),
        snapshots: Some(HashMap::from([("dep1@1.0.0".parse().unwrap(), SnapshotEntry::default())])),
        ..empty_lockfile()
    };
    let standard_package_map = lockfile_to_package_map(
        &lockfile,
        &PackageMapOptions {
            lockfile_dir: &cwd,
            modules_dir: &cwd.join("node_modules"),
            package_map_type: pacquet_config::NodePackageMapType::Standard,
            layout: &layout,
            project_manifests: &project_manifests,
        },
    );
    let loose_package_map = lockfile_to_package_map(
        &lockfile,
        &PackageMapOptions {
            lockfile_dir: &cwd,
            modules_dir: &cwd.join("node_modules"),
            package_map_type: pacquet_config::NodePackageMapType::Loose,
            layout: &layout,
            project_manifests: &project_manifests,
        },
    );

    assert_eq!(
        standard_package_map.packages["dep1@1.0.0"].dependencies,
        BTreeMap::from([("dep1".to_string(), "dep1@1.0.0".to_string())]),
    );
    assert_eq!(
        loose_package_map.packages["dep1@1.0.0"].dependencies,
        BTreeMap::from([
            ("dep1".to_string(), "dep1@1.0.0".to_string()),
            ("linked".to_string(), "packages/linked".to_string()),
        ]),
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
                dependencies: Some(deps(&[("dep1", "1.0.0"), ("linked", "link:packages/linked")])),
                ..ProjectSnapshot::default()
            },
        )]),
        snapshots: Some(HashMap::from([("dep1@1.0.0".parse().unwrap(), SnapshotEntry::default())])),
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
        ]),
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
        ]),
    );
    assert_eq!(
        loose_package_map.packages["."].dependencies,
        BTreeMap::from([
            ("dep1".to_string(), "dep1".to_string()),
            ("dep2".to_string(), "dep2".to_string()),
            ("root".to_string(), ".".to_string()),
        ]),
    );
}

#[test]
fn package_map_node_options_replaces_existing_package_map_option() {
    assert_eq!(
        make_node_package_map_option(
            Path::new("/repo/node_modules/.package-map.json"),
            Some("--require ./hook.cjs --experimental-package-map=old.json --trace-warnings"),
        ),
        "--require ./hook.cjs --trace-warnings --experimental-package-map=/repo/node_modules/.package-map.json",
    );
    assert_eq!(
        make_node_package_map_option(
            Path::new("/repo with spaces/node_modules/.package-map.json"),
            Some("--experimental-package-map old.json"),
        ),
        r#"--experimental-package-map="/repo with spaces/node_modules/.package-map.json""#,
    );
    // A backslash path (e.g. Windows) must be quoted and the separators escaped
    // so Node's NODE_OPTIONS parser does not consume them as escapes.
    assert_eq!(
        make_node_package_map_option(
            Path::new(r"C:\repo\node_modules\.package-map.json"),
            Some(""),
        ),
        r#"--experimental-package-map="C:\\repo\\node_modules\\.package-map.json""#,
    );
    // An existing flag whose quoted path contains an escaped quote must be
    // stripped in full, so the rebuilt NODE_OPTIONS is not corrupted.
    assert_eq!(
        make_node_package_map_option(
            Path::new("/new/.package-map.json"),
            Some(r#"--experimental-package-map="/quo\"te/old.json" --inspect"#),
        ),
        "--inspect --experimental-package-map=/new/.package-map.json",
    );
}

#[test]
fn link_target_id_uses_link_prefix_for_paths_above_the_lockfile_dir() {
    let dir = PathBuf::from("/outside/pkg");
    assert_eq!(link_target_id(Some(PathBuf::from("../outside/pkg")), &dir), "link:/outside/pkg");
}

#[test]
fn relative_url_uses_a_file_url_when_relative_path_cannot_be_computed() {
    assert_eq!(
        absolute_package_url(Path::new("/outside/pkg with space")),
        "file:///outside/pkg%20with%20space",
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
        lockfile_version: LockfileVersion::<9>::try_from(ComVer { major: 9, minor: 0 }).unwrap(),
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
