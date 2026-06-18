use super::{
    DepHierarchy, DependenciesGraph, DependenciesGraphNode, LockfileToDepGraphResult,
    LockfileToHoistedDepGraphOptions,
};
use pacquet_lockfile::{DirectoryResolution, LockfileResolution, PkgIdWithPatchHash};
use pacquet_modules_yaml::DepPath;
use pretty_assertions::assert_eq;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

fn sample_resolution() -> LockfileResolution {
    DirectoryResolution { directory: "../local-pkg".to_string() }.into()
}

/// Sample v9 depPath. v9 lockfiles use `name@version[(peers)]`
/// (see `PkgNameVerPeer` in `pacquet-lockfile`); the v5-era
/// `/name/version` shape is only kept for legacy
/// `hoistedAliases` read-side compatibility.
const ACCEPTS_DEP_PATH: &str = "accepts@1.3.7";

#[test]
fn default_result_is_empty() {
    let actual = LockfileToDepGraphResult::default();
    assert_eq!(actual.graph, DependenciesGraph::new());
    assert!(actual.direct_dependencies_by_importer_id.is_empty());
    assert!(actual.hierarchy.is_empty());
    assert!(actual.hoisted_locations.is_empty());
    assert!(actual.symlinked_direct_dependencies_by_importer_id.is_empty());
    assert!(actual.prev_graph.is_none());
    assert!(actual.injection_targets_by_dep_path.is_empty());
    assert!(actual.skipped.is_empty());
}

#[test]
fn graph_node_inserts_by_dir() {
    let dir = PathBuf::from("/repo/node_modules/accepts");
    let modules = PathBuf::from("/repo/node_modules");
    let node = DependenciesGraphNode {
        alias: Some("accepts".to_string()),
        dep_path: DepPath::from(ACCEPTS_DEP_PATH.to_string()),
        pkg_id_with_patch_hash: PkgIdWithPatchHash::from(ACCEPTS_DEP_PATH),
        dir: dir.clone(),
        modules,
        children: BTreeMap::new(),
        name: "accepts".to_string(),
        version: "1.3.7".to_string(),
        optional: false,
        optional_dependencies: BTreeSet::new(),
        has_bin: false,
        has_bundled_dependencies: false,
        patch: None,
        resolution: sample_resolution(),
    };

    let mut graph = DependenciesGraph::new();
    graph.insert(dir.clone(), node.clone());
    assert_eq!(graph.get(&dir), Some(&node));
}

/// The newtype wrapper exists because Rust doesn't allow recursive type
/// aliases.
#[test]
fn hierarchy_nests_recursively() {
    let mut inner_children = BTreeMap::new();
    inner_children.insert(
        PathBuf::from("/repo/node_modules/accepts/node_modules/mime-types"),
        DepHierarchy::default(),
    );
    let inner = DepHierarchy(inner_children);

    let mut root_children = BTreeMap::new();
    root_children.insert(PathBuf::from("/repo/node_modules/accepts"), inner.clone());
    let root = DepHierarchy(root_children);

    let accepts = root.0.get(&PathBuf::from("/repo/node_modules/accepts")).expect("accepts entry");
    assert_eq!(accepts, &inner);
    assert_eq!(accepts.0.len(), 1);
}

#[test]
fn options_default_is_empty() {
    let opts = LockfileToHoistedDepGraphOptions::default();
    assert_eq!(opts.lockfile_dir, PathBuf::new());
    assert!(!opts.auto_install_peers);
    assert!(opts.skipped.is_empty());
    assert!(!opts.force);
    assert!(!opts.engine_strict);
    assert!(opts.current_node_version.is_empty());
    assert!(opts.supported_architectures.is_none());
}

// --- Walker tests ----------------------------------------------------

use super::{HoistedDepGraphError, InstallabilityError, lockfile_to_hoisted_dep_graph};
use pacquet_lockfile::{
    ComVer, Lockfile, LockfileSettings, LockfileVersion, PackageKey, PackageMetadata, PkgName,
    PkgNameVerPeer, PkgVerPeer, ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec,
    SnapshotDepRef, SnapshotEntry,
};
use std::collections::HashMap;

fn lockfile_version() -> LockfileVersion<9> {
    LockfileVersion::<9>::try_from(ComVer::new(9, 0)).expect("lockfileVersion 9.0 is compatible")
}

fn pkg_name(text: &str) -> PkgName {
    PkgName::parse(text).expect("parse PkgName")
}

fn ver_peer(text: &str) -> PkgVerPeer {
    text.parse::<PkgVerPeer>().expect("parse PkgVerPeer")
}

fn dep_key(name: &str, version: &str) -> PkgNameVerPeer {
    PkgNameVerPeer::new(pkg_name(name), ver_peer(version))
}

fn resolved_dep(version: &str) -> ResolvedDependencySpec {
    ResolvedDependencySpec { specifier: version.to_string(), version: ver_peer(version).into() }
}

fn directory_resolution(directory: &str) -> LockfileResolution {
    DirectoryResolution { directory: directory.to_string() }.into()
}

/// Uses a synthetic `directory:` resolution: walker tests don't exercise
/// resolution semantics — they only need *some* resolution so the graph node
/// has a non-default value to inspect.
fn metadata_stub() -> PackageMetadata {
    PackageMetadata {
        resolution: directory_resolution("/dev/null/stub"),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

fn lockfile_with(
    importer_deps: ResolvedDependencyMap,
    packages: HashMap<PackageKey, PackageMetadata>,
    snapshots: HashMap<PackageKey, SnapshotEntry>,
) -> Lockfile {
    let mut importers = HashMap::new();
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(importer_deps), ..ProjectSnapshot::default() },
    );
    Lockfile {
        lockfile_version: lockfile_version(),
        settings: Some(LockfileSettings::default()),
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers,
        packages: Some(packages),
        snapshots: Some(snapshots),
    }
}

/// Mirrors the `empty_lockfile_yields_empty_root` case from the hoister.
#[test]
fn walker_empty_lockfile_produces_empty_result() {
    let lockfile = Lockfile {
        lockfile_version: lockfile_version(),
        settings: Some(LockfileSettings::default()),
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: HashMap::new(),
        packages: None,
        snapshots: None,
    };
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: PathBuf::from("/repo"),
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result =
        lockfile_to_hoisted_dep_graph(&lockfile, None, &opts).expect("empty lockfile walks");

    assert!(result.graph.is_empty(), "graph should be empty");
    assert!(result.hoisted_locations.is_empty(), "no locations recorded");
    assert_eq!(result.direct_dependencies_by_importer_id.len(), 1);
    assert!(result.direct_dependencies_by_importer_id[Lockfile::ROOT_IMPORTER_KEY].is_empty());
}

#[test]
fn walker_single_root_dep_emits_one_node() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));

    let mut packages = HashMap::new();
    packages.insert(dep_key("a", "1.0.0"), metadata_stub());

    let mut snapshots = HashMap::new();
    snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());

    let lockfile = lockfile_with(root_deps, packages, snapshots);
    let lockfile_dir = PathBuf::from("/repo");
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: lockfile_dir.clone(),
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &opts).expect("walker succeeds");

    let expected_dir = lockfile_dir.join("node_modules").join("a");
    assert_eq!(
        result.graph.len(),
        1,
        "one node emitted: {:?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
    let node = result.graph.get(&expected_dir).expect("node keyed by dir");
    assert_eq!(node.alias.as_deref(), Some("a"));
    assert_eq!(node.dep_path, DepPath::from("a@1.0.0".to_string()));
    assert_eq!(node.name, "a");
    assert_eq!(node.version, "1.0.0");

    assert_eq!(result.hoisted_locations["a@1.0.0"], vec!["node_modules/a".to_string()]);
    assert_eq!(
        result.direct_dependencies_by_importer_id[Lockfile::ROOT_IMPORTER_KEY]["a"],
        expected_dir,
    );
}

#[test]
fn walker_transitive_dep_flattens_under_root() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));

    let mut packages = HashMap::new();
    packages.insert(dep_key("a", "1.0.0"), metadata_stub());
    packages.insert(dep_key("b", "1.0.0"), metadata_stub());

    let mut snapshots = HashMap::new();
    let mut a_deps = HashMap::new();
    a_deps.insert(pkg_name("b"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    snapshots.insert(
        dep_key("a", "1.0.0"),
        SnapshotEntry { dependencies: Some(a_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("b", "1.0.0"), SnapshotEntry::default());

    let lockfile = lockfile_with(root_deps, packages, snapshots);
    let lockfile_dir = PathBuf::from("/repo");
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: lockfile_dir.clone(),
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &opts).expect("walker succeeds");

    let modules = lockfile_dir.join("node_modules");
    assert_eq!(
        result.graph.keys().cloned().collect::<Vec<_>>(),
        vec![modules.join("a"), modules.join("b")],
        "both nodes hoisted to root, sorted by dir",
    );
    let a_node = result.graph.get(&modules.join("a")).expect("a in graph");
    assert_eq!(
        a_node.children.get("b"),
        Some(&modules.join("b")),
        "a's `children[\"b\"]` points at the hoisted (root-level) dir",
    );

    assert_eq!(result.hoisted_locations["a@1.0.0"], vec!["node_modules/a".to_string()]);
    assert_eq!(result.hoisted_locations["b@1.0.0"], vec!["node_modules/b".to_string()]);
}

#[test]
fn walker_version_conflict_keeps_loser_nested() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("c"), resolved_dep("1.0.0"));

    let mut packages = HashMap::new();
    packages.insert(dep_key("a", "1.0.0"), metadata_stub());
    packages.insert(dep_key("a", "2.0.0"), metadata_stub());
    packages.insert(dep_key("c", "1.0.0"), metadata_stub());

    let mut snapshots = HashMap::new();
    snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("a", "2.0.0"), SnapshotEntry::default());
    let mut c_deps = HashMap::new();
    c_deps.insert(pkg_name("a"), SnapshotDepRef::Plain(ver_peer("2.0.0")));
    snapshots.insert(
        dep_key("c", "1.0.0"),
        SnapshotEntry { dependencies: Some(c_deps), ..SnapshotEntry::default() },
    );

    let lockfile = lockfile_with(root_deps, packages, snapshots);
    let lockfile_dir = PathBuf::from("/repo");
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: lockfile_dir.clone(),
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &opts).expect("walker succeeds");

    let modules = lockfile_dir.join("node_modules");
    let a1_dir = modules.join("a");
    let c_dir = modules.join("c");
    let a2_dir = c_dir.join("node_modules").join("a");

    assert!(result.graph.contains_key(&a1_dir), "a@1 at root");
    assert!(result.graph.contains_key(&c_dir), "c at root");
    assert!(result.graph.contains_key(&a2_dir), "a@2 nested under c");

    assert_eq!(result.graph[&a1_dir].dep_path, DepPath::from("a@1.0.0".to_string()));
    assert_eq!(result.graph[&a2_dir].dep_path, DepPath::from("a@2.0.0".to_string()));

    assert_eq!(result.hoisted_locations["a@1.0.0"], vec!["node_modules/a".to_string()]);
    assert_eq!(
        result.hoisted_locations["a@2.0.0"],
        vec!["node_modules/c/node_modules/a".to_string()],
    );

    assert_eq!(result.graph[&c_dir].children.get("a"), Some(&a2_dir));
}

#[test]
fn walker_honors_pre_skipped_dep_path() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));

    let mut packages = HashMap::new();
    packages.insert(dep_key("a", "1.0.0"), metadata_stub());

    let mut snapshots = HashMap::new();
    snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());

    let lockfile = lockfile_with(root_deps, packages, snapshots);
    let mut skipped = BTreeSet::new();
    skipped.insert("a@1.0.0".to_string());
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: PathBuf::from("/repo"),
        skipped,
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &opts).expect("walker succeeds");

    assert!(result.graph.is_empty(), "skipped dep not emitted");
    assert!(result.hoisted_locations.is_empty());
    assert!(
        result.skipped.contains("a@1.0.0"),
        "pre-skipped dep is still in the output skipped set",
    );
}

/// A `directory:` resolution gets recorded in
/// `injection_targets_by_dep_path` so the post-install
/// re-mirror step (a later sub-slice) can find it.
#[test]
fn walker_records_directory_resolution_as_injection_target() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));

    let mut packages = HashMap::new();
    packages.insert(
        dep_key("a", "1.0.0"),
        PackageMetadata { resolution: directory_resolution("../local-a"), ..metadata_stub() },
    );

    let mut snapshots = HashMap::new();
    snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());

    let lockfile = lockfile_with(root_deps, packages, snapshots);
    let lockfile_dir = PathBuf::from("/repo");
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: lockfile_dir.clone(),
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &opts).expect("walker succeeds");

    assert_eq!(
        result.injection_targets_by_dep_path["a@1.0.0"],
        vec![lockfile_dir.join("node_modules").join("a")],
    );
}

// --- Installability tests --------------------------------------------

fn host_aware_opts() -> LockfileToHoistedDepGraphOptions {
    // Concrete platform values so the installability check has
    // something to compare against. The specific host doesn't
    // matter — tests assert relative behavior (compatible vs
    // incompatible) by setting metadata that targets *this*
    // value or its opposite.
    LockfileToHoistedDepGraphOptions {
        lockfile_dir: PathBuf::from("/repo"),
        current_node_version: "20.0.0".to_string(),
        current_os: "linux".to_string(),
        current_cpu: "x64".to_string(),
        current_libc: "glibc".to_string(),
        ..LockfileToHoistedDepGraphOptions::default()
    }
}

fn metadata_with_os(os: &str) -> PackageMetadata {
    PackageMetadata { os: Some(vec![os.to_string()]), ..metadata_stub() }
}

/// Mirrors upstream's
/// `if (!opts.force && packageIsInstallable(...) === false) {
/// opts.skipped.add(depPath); return; }`.
#[test]
fn walker_skips_optional_dep_on_unsupported_platform() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));

    let mut packages = HashMap::new();
    // Linux host, package targets darwin only → unsupported.
    packages.insert(dep_key("a", "1.0.0"), metadata_with_os("darwin"));

    let mut snapshots = HashMap::new();
    snapshots.insert(
        dep_key("a", "1.0.0"),
        SnapshotEntry { optional: true, ..SnapshotEntry::default() },
    );

    let lockfile = lockfile_with(root_deps, packages, snapshots);
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &host_aware_opts())
        .expect("walker succeeds");

    assert!(result.graph.is_empty(), "optional incompatible dep not emitted");
    assert!(
        result.skipped.contains("a@1.0.0"),
        "incompatible optional dep added to skipped: {:?}",
        result.skipped,
    );
    assert!(result.hoisted_locations.is_empty(), "no location recorded for skipped dep");
}

/// Required (non-optional) package on an unsupported platform
/// proceeds with a warning rather than erroring — mirrors
/// upstream `package_is_installable`'s `true` return for the
/// required-incompatible case (only `engineStrict + engine
/// mismatch` and `InvalidNodeVersion` actually throw). The
/// warning log emit is out of scope here; the walker proceeds
/// silently for now.
#[test]
fn walker_emits_required_dep_with_unsupported_platform_as_warning() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));

    let mut packages = HashMap::new();
    packages.insert(dep_key("a", "1.0.0"), metadata_with_os("darwin"));

    let mut snapshots = HashMap::new();
    // optional: false — upstream's `packageIsInstallable`
    // returns `true` (warn but proceed) rather than `false`.
    snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());

    let lockfile = lockfile_with(root_deps, packages, snapshots);
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &host_aware_opts())
        .expect("walker proceeds");

    assert_eq!(result.graph.len(), 1, "required incompatible dep emitted as warning");
    assert!(result.skipped.is_empty(), "required dep not added to skipped");
}

/// `engineStrict = true` + engine mismatch surfaces as
/// `HoistedDepGraphError::Installability`. Mirrors upstream's
/// `throw warn` path in `packageIsInstallable` when
/// `engineStrict && warn instanceof UnsupportedEngineError`.
#[test]
fn walker_errors_on_engine_strict_mismatch() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));

    let mut engines = HashMap::new();
    engines.insert("node".to_string(), ">=99.0.0".to_string());
    let mut packages = HashMap::new();
    packages.insert(
        dep_key("a", "1.0.0"),
        PackageMetadata { engines: Some(engines), ..metadata_stub() },
    );

    let mut snapshots = HashMap::new();
    snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());

    let lockfile = lockfile_with(root_deps, packages, snapshots);
    let opts = LockfileToHoistedDepGraphOptions { engine_strict: true, ..host_aware_opts() };
    let err = lockfile_to_hoisted_dep_graph(&lockfile, None, &opts)
        .expect_err("engine_strict + engine mismatch should error");
    match err {
        HoistedDepGraphError::Installability(inner) => match *inner {
            InstallabilityError::Engine(engine_err) => {
                assert_eq!(engine_err.package_id, "a@1.0.0");
            }
            other => panic!("expected Engine variant, got {other:?}"),
        },
        other => panic!("expected Installability error, got {other:?}"),
    }
}

/// `opts.force = true` bypasses the installability check
/// entirely — even a required dep on an unsupported platform
/// passes through. Used by the `prev_graph` walk so the diff
/// against the previous lockfile catches packages that
/// previously installed but would now be filtered.
#[test]
fn walker_force_bypasses_installability_check() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));

    let mut packages = HashMap::new();
    packages.insert(dep_key("a", "1.0.0"), metadata_with_os("darwin"));

    let mut snapshots = HashMap::new();
    snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());

    let lockfile = lockfile_with(root_deps, packages, snapshots);
    let opts = LockfileToHoistedDepGraphOptions { force: true, ..host_aware_opts() };
    let result =
        lockfile_to_hoisted_dep_graph(&lockfile, None, &opts).expect("force bypasses check");

    assert_eq!(result.graph.len(), 1, "force=true emits the dep regardless of platform");
    assert!(result.skipped.is_empty(), "force=true doesn't add to skipped");
}

/// Sanity check that the installability path doesn't drop packages it
/// shouldn't.
#[test]
fn walker_emits_compatible_dep() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));

    let mut packages = HashMap::new();
    // Linux host, package targets linux → compatible.
    packages.insert(dep_key("a", "1.0.0"), metadata_with_os("linux"));

    let mut snapshots = HashMap::new();
    snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());

    let lockfile = lockfile_with(root_deps, packages, snapshots);
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &host_aware_opts())
        .expect("walker succeeds");

    assert_eq!(result.graph.len(), 1);
    assert!(result.skipped.is_empty());
}

// --- prev_graph tests ------------------------------------------------

/// Mirrors upstream's `prevGraph = {}` fallback when no current
/// lockfile is supplied — pacquet uses `None` instead of an
/// empty map, but the linker treats the two the same way (no
/// orphans to remove on a fresh install).
#[test]
fn prev_graph_none_when_current_lockfile_absent() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));

    let mut packages = HashMap::new();
    packages.insert(dep_key("a", "1.0.0"), metadata_stub());

    let mut snapshots = HashMap::new();
    snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());

    let lockfile = lockfile_with(root_deps, packages, snapshots);
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: PathBuf::from("/repo"),
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &opts).expect("walker succeeds");

    assert!(result.prev_graph.is_none(), "no current_lockfile → no prev_graph");
    assert_eq!(result.graph.len(), 1, "wanted lockfile still produces the graph");
}

/// Mirrors upstream's `currentLockfile?.packages != null` guard.
#[test]
fn prev_graph_none_when_current_lockfile_has_no_packages() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));

    let mut packages = HashMap::new();
    packages.insert(dep_key("a", "1.0.0"), metadata_stub());

    let mut snapshots = HashMap::new();
    snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());

    let wanted = lockfile_with(root_deps, packages, snapshots);
    let current = Lockfile {
        lockfile_version: lockfile_version(),
        settings: Some(LockfileSettings::default()),
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: HashMap::new(),
        packages: None,
        snapshots: None,
    };
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: PathBuf::from("/repo"),
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result =
        lockfile_to_hoisted_dep_graph(&wanted, Some(&current), &opts).expect("walker succeeds");

    assert!(result.prev_graph.is_none(), "current lockfile without packages → no prev_graph");
}

/// Pacquet collapses null and empty into the same "no orphans"
/// representation, since walking an empty `packages:` would just produce an
/// empty graph anyway.
#[test]
fn prev_graph_none_when_current_lockfile_has_empty_packages() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));

    let mut packages = HashMap::new();
    packages.insert(dep_key("a", "1.0.0"), metadata_stub());

    let mut snapshots = HashMap::new();
    snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());

    let wanted = lockfile_with(root_deps, packages, snapshots);
    let current = Lockfile {
        lockfile_version: lockfile_version(),
        settings: Some(LockfileSettings::default()),
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: HashMap::new(),
        packages: Some(HashMap::new()),
        snapshots: Some(HashMap::new()),
    };
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: PathBuf::from("/repo"),
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result =
        lockfile_to_hoisted_dep_graph(&wanted, Some(&current), &opts).expect("walker succeeds");

    assert!(result.prev_graph.is_none(), "current lockfile with empty packages → no prev_graph");
}

/// The linker subtracts `graph` from `prev_graph` to find orphan
/// directories and `rimraf` them.
#[test]
fn prev_graph_contains_orphan_from_current_only_lockfile() {
    // Current install: root → {a, orphan}
    let mut current_root_deps = ResolvedDependencyMap::new();
    current_root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));
    current_root_deps.insert(pkg_name("orphan"), resolved_dep("1.0.0"));
    let mut current_packages = HashMap::new();
    current_packages.insert(dep_key("a", "1.0.0"), metadata_stub());
    current_packages.insert(dep_key("orphan", "1.0.0"), metadata_stub());
    let mut current_snapshots = HashMap::new();
    current_snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());
    current_snapshots.insert(dep_key("orphan", "1.0.0"), SnapshotEntry::default());
    let current_lockfile = lockfile_with(current_root_deps, current_packages, current_snapshots);

    // Wanted install: root → {a} (orphan removed)
    let mut wanted_root_deps = ResolvedDependencyMap::new();
    wanted_root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));
    let mut wanted_packages = HashMap::new();
    wanted_packages.insert(dep_key("a", "1.0.0"), metadata_stub());
    let mut wanted_snapshots = HashMap::new();
    wanted_snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());
    let wanted_lockfile = lockfile_with(wanted_root_deps, wanted_packages, wanted_snapshots);

    let lockfile_dir = PathBuf::from("/repo");
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: lockfile_dir.clone(),
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result = lockfile_to_hoisted_dep_graph(&wanted_lockfile, Some(&current_lockfile), &opts)
        .expect("walker succeeds");

    let orphan_dir = lockfile_dir.join("node_modules").join("orphan");
    let a_dir = lockfile_dir.join("node_modules").join("a");

    let prev = result.prev_graph.expect("prev_graph populated");
    assert!(prev.contains_key(&orphan_dir), "orphan present in prev_graph");
    assert!(prev.contains_key(&a_dir), "carried-over dep also in prev_graph");
    assert!(result.graph.contains_key(&a_dir), "wanted graph carries a");
    assert!(!result.graph.contains_key(&orphan_dir), "wanted graph omits orphan");
}

/// The prev-graph walk uses `force: true, skipped: empty` so
/// the *current* layout is preserved even for packages that
/// would now fail installability. Mirrors upstream's
/// `{ ...opts, force: true, skipped: new Set() }` override at
/// [lockfileToHoistedDepGraph.ts:72-76](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/lockfileToHoistedDepGraph.ts#L72-L76).
/// Without this, an orphan that targets an unsupported platform
/// wouldn't appear in `prev_graph` and the linker would leave
/// the stale directory in place.
#[test]
fn prev_graph_includes_orphan_even_when_now_incompatible() {
    // Current install had a darwin-targeting orphan dep that
    // landed on a host where the wanted install runs on linux.
    let mut current_root_deps = ResolvedDependencyMap::new();
    current_root_deps.insert(pkg_name("orphan"), resolved_dep("1.0.0"));
    let mut current_packages = HashMap::new();
    current_packages.insert(dep_key("orphan", "1.0.0"), metadata_with_os("darwin"));
    let mut current_snapshots = HashMap::new();
    current_snapshots.insert(dep_key("orphan", "1.0.0"), SnapshotEntry::default());
    let current_lockfile = lockfile_with(current_root_deps, current_packages, current_snapshots);

    // Wanted install: empty root.
    let wanted_lockfile =
        lockfile_with(ResolvedDependencyMap::new(), HashMap::new(), HashMap::new());

    let lockfile_dir = PathBuf::from("/repo");
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: lockfile_dir.clone(),
        current_node_version: "20.0.0".to_string(),
        current_os: "linux".to_string(),
        current_cpu: "x64".to_string(),
        current_libc: "glibc".to_string(),
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result = lockfile_to_hoisted_dep_graph(&wanted_lockfile, Some(&current_lockfile), &opts)
        .expect("walker succeeds");

    let orphan_dir = lockfile_dir.join("node_modules").join("orphan");
    let prev = result.prev_graph.expect("prev_graph populated");
    assert!(
        prev.contains_key(&orphan_dir),
        "force: true emits the orphan even though it would now fail installability",
    );
    assert!(result.graph.is_empty(), "wanted graph stays empty");
    assert!(result.skipped.is_empty(), "skipped from wanted walk only, not prev walk");
}

// --- Multi-importer (workspace) walker tests --------------------------

/// Build a multi-importer workspace fixture lockfile. Each
/// importer in `importer_deps` becomes a `ProjectSnapshot`
/// with the supplied direct deps. Root importer (`.`) takes
/// the first entry in `importer_deps`; remaining entries
/// become non-root workspace importers under their lockfile
/// keys.
fn workspace_lockfile(
    importer_deps: Vec<(&str, ResolvedDependencyMap)>,
    packages: HashMap<PackageKey, PackageMetadata>,
    snapshots: HashMap<PackageKey, SnapshotEntry>,
) -> Lockfile {
    let mut importers = HashMap::new();
    for (id, deps) in importer_deps {
        importers.insert(
            id.to_string(),
            ProjectSnapshot { dependencies: Some(deps), ..ProjectSnapshot::default() },
        );
    }
    Lockfile {
        lockfile_version: lockfile_version(),
        settings: Some(LockfileSettings::default()),
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers,
        packages: Some(packages),
        snapshots: Some(snapshots),
    }
}

#[test]
fn walker_multi_importer_emits_per_importer_direct_deps() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));

    let mut foo_deps = ResolvedDependencyMap::new();
    foo_deps.insert(pkg_name("b"), resolved_dep("1.0.0"));

    let mut packages = HashMap::new();
    packages.insert(dep_key("a", "1.0.0"), metadata_stub());
    packages.insert(dep_key("b", "1.0.0"), metadata_stub());

    let mut snapshots = HashMap::new();
    snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("b", "1.0.0"), SnapshotEntry::default());

    let lockfile = workspace_lockfile(
        vec![(Lockfile::ROOT_IMPORTER_KEY, root_deps), ("packages/foo", foo_deps)],
        packages,
        snapshots,
    );
    let lockfile_dir = PathBuf::from("/repo");
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: lockfile_dir.clone(),
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &opts).expect("walker succeeds");

    let modules = lockfile_dir.join("node_modules");
    assert!(result.graph.contains_key(&modules.join("a")));
    assert!(result.graph.contains_key(&modules.join("b")));
    assert_eq!(
        result.direct_dependencies_by_importer_id[Lockfile::ROOT_IMPORTER_KEY]["a"],
        modules.join("a"),
    );
    assert_eq!(result.direct_dependencies_by_importer_id["packages/foo"]["b"], modules.join("b"));
    assert!(!result.graph.values().any(|node| node.alias.as_deref() == Some("packages%2Ffoo")));
}

/// The linker drives its per-importer parallel fan-out off the hierarchy
/// map, so an importer missing a hierarchy entry would be silently
/// un-linked.
#[test]
fn walker_multi_importer_emits_per_importer_hierarchy() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));

    let mut foo_deps = ResolvedDependencyMap::new();
    foo_deps.insert(pkg_name("b"), resolved_dep("1.0.0"));

    let mut packages = HashMap::new();
    packages.insert(dep_key("a", "1.0.0"), metadata_stub());
    packages.insert(dep_key("b", "1.0.0"), metadata_stub());

    let mut snapshots = HashMap::new();
    snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("b", "1.0.0"), SnapshotEntry::default());

    let lockfile = workspace_lockfile(
        vec![(Lockfile::ROOT_IMPORTER_KEY, root_deps), ("packages/foo", foo_deps)],
        packages,
        snapshots,
    );
    let lockfile_dir = PathBuf::from("/repo");
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: lockfile_dir.clone(),
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &opts).expect("walker succeeds");

    let importer_root = lockfile_dir.join("packages/foo");
    assert!(result.hierarchy.contains_key(&lockfile_dir), "root importer hierarchy missing");
    assert!(
        result.hierarchy.contains_key(&importer_root),
        "packages/foo hierarchy missing: {:?}",
        result.hierarchy.keys().collect::<Vec<_>>(),
    );
}

/// `hoist_workspace_packages: false` opts non-root importers
/// out of the shared hoister tree. The walker then sees no
/// `Workspace`-kind children to fan out into, so only the
/// root importer's direct deps + hierarchy are emitted.
/// Non-root importers stay absent from
/// `direct_dependencies_by_importer_id`. Mirrors upstream's
/// per-project independent hoist mode.
#[test]
fn walker_hoist_workspace_packages_false_emits_root_only() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));

    let mut foo_deps = ResolvedDependencyMap::new();
    foo_deps.insert(pkg_name("b"), resolved_dep("1.0.0"));

    let mut packages = HashMap::new();
    packages.insert(dep_key("a", "1.0.0"), metadata_stub());
    packages.insert(dep_key("b", "1.0.0"), metadata_stub());

    let mut snapshots = HashMap::new();
    snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("b", "1.0.0"), SnapshotEntry::default());

    let lockfile = workspace_lockfile(
        vec![(Lockfile::ROOT_IMPORTER_KEY, root_deps), ("packages/foo", foo_deps)],
        packages,
        snapshots,
    );
    let lockfile_dir = PathBuf::from("/repo");
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: lockfile_dir.clone(),
        hoist_workspace_packages: false,
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &opts).expect("walker succeeds");

    assert!(result.graph.contains_key(&lockfile_dir.join("node_modules").join("a")));
    assert!(!result.graph.contains_key(&lockfile_dir.join("node_modules").join("b")));
    assert_eq!(result.direct_dependencies_by_importer_id.len(), 1);
    assert!(result.direct_dependencies_by_importer_id.contains_key(Lockfile::ROOT_IMPORTER_KEY));
    assert!(!result.direct_dependencies_by_importer_id.contains_key("packages/foo"));
    assert_eq!(result.hierarchy.len(), 1);
    assert!(result.hierarchy.contains_key(&lockfile_dir));
}

#[test]
fn walker_multi_importer_version_conflict_nests_loser() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));

    let mut foo_deps = ResolvedDependencyMap::new();
    foo_deps.insert(pkg_name("a"), resolved_dep("2.0.0"));

    let mut packages = HashMap::new();
    packages.insert(dep_key("a", "1.0.0"), metadata_stub());
    packages.insert(dep_key("a", "2.0.0"), metadata_stub());

    let mut snapshots = HashMap::new();
    snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("a", "2.0.0"), SnapshotEntry::default());

    let lockfile = workspace_lockfile(
        vec![(Lockfile::ROOT_IMPORTER_KEY, root_deps), ("packages/foo", foo_deps)],
        packages,
        snapshots,
    );
    let lockfile_dir = PathBuf::from("/repo");
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir,
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &opts).expect("walker succeeds");

    assert_eq!(result.graph.len(), 2);
    // Root sees a@1 (or a@2 — depends on hoister's tie-break)
    // and packages/foo sees the other version. Whichever wins
    // the root slot, the *other* lives under that importer's
    // own `node_modules/a`.
    let root_a = &result.direct_dependencies_by_importer_id[Lockfile::ROOT_IMPORTER_KEY]["a"];
    let foo_a = &result.direct_dependencies_by_importer_id["packages/foo"]["a"];
    assert_ne!(root_a, foo_a, "conflict resolves to two distinct dirs");
}

/// The cross-importer workspace invariant: when the root importer and
/// a workspace project pin conflicting versions of the same name, the
/// root's version wins the top-level `node_modules` slot and the
/// project's version nests under the project. Locks in the popularity
/// preference (root deps rank first) together with the per-importer
/// walk. Mirrors upstream's `installing/deps-restorer/test/index.ts`
/// workspace-hoisted case where the root's `webpack@5.65.0` lands at
/// the root and `foo`'s `webpack@2.7.0` nests under `foo`.
#[test]
fn walker_workspace_root_version_wins_root_slot() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("webby"), resolved_dep("5.0.0"));

    let mut app_deps = ResolvedDependencyMap::new();
    app_deps.insert(pkg_name("webby"), resolved_dep("2.0.0"));

    let mut packages = HashMap::new();
    packages.insert(dep_key("webby", "5.0.0"), metadata_stub());
    packages.insert(dep_key("webby", "2.0.0"), metadata_stub());

    let mut snapshots = HashMap::new();
    snapshots.insert(dep_key("webby", "5.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("webby", "2.0.0"), SnapshotEntry::default());

    let lockfile = workspace_lockfile(
        vec![(Lockfile::ROOT_IMPORTER_KEY, root_deps), ("packages/app", app_deps)],
        packages,
        snapshots,
    );
    let lockfile_dir = PathBuf::from("/repo");
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: lockfile_dir.clone(),
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &opts).expect("walker succeeds");

    let root_webby = lockfile_dir.join("node_modules").join("webby");
    let nested_webby = lockfile_dir.join("packages/app").join("node_modules").join("webby");

    assert_eq!(
        result.graph[&root_webby].dep_path,
        DepPath::from("webby@5.0.0".to_string()),
        "the root importer's version wins the top-level slot",
    );
    assert_eq!(
        result.graph[&nested_webby].dep_path,
        DepPath::from("webby@2.0.0".to_string()),
        "the workspace project's conflicting version nests under the project",
    );
    assert_eq!(
        result.direct_dependencies_by_importer_id[Lockfile::ROOT_IMPORTER_KEY]["webby"],
        root_webby,
    );
    assert_eq!(result.direct_dependencies_by_importer_id["packages/app"]["webby"], nested_webby);
}

#[test]
fn walker_forwards_external_dependencies_to_hoister() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));

    let mut packages = HashMap::new();
    packages.insert(dep_key("a", "1.0.0"), metadata_stub());

    let mut snapshots = HashMap::new();
    snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());

    let lockfile = lockfile_with(root_deps, packages, snapshots);
    let lockfile_dir = PathBuf::from("/repo");
    let mut externals = BTreeSet::new();
    externals.insert("a".to_string());
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir,
        external_dependencies: externals,
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &opts).expect("walker succeeds");

    assert!(result.graph.is_empty(), "external strips the alias from the hoist result");
    assert!(
        result.direct_dependencies_by_importer_id[Lockfile::ROOT_IMPORTER_KEY].is_empty(),
        "root direct deps drop the externalised alias",
    );
}

/// A crafted lockfile whose dependency alias is a path-traversal
/// (`../../../escape`) or a reserved name (`.bin`, `.pnpm`,
/// `node_modules`) is rejected at the hoisted graph sink before the
/// node is inserted or the walker recurses. `PkgName::parse` is
/// permissive enough to carry such an alias straight out of a
/// deserialized lockfile, so this is the boundary that stops it.
/// Mirrors pnpm's `ERR_PNPM_INVALID_DEPENDENCY_NAME`; `force: true`
/// skips installability so the walk reaches the alias sink directly.
#[test]
fn walker_rejects_invalid_hoisted_alias() {
    for alias in ["../../../escape", "@scope/../../escape", ".bin", ".pnpm", "node_modules"] {
        let mut root_deps = ResolvedDependencyMap::new();
        root_deps.insert(pkg_name(alias), resolved_dep("1.0.0"));

        let mut packages = HashMap::new();
        packages.insert(dep_key(alias, "1.0.0"), metadata_stub());

        let mut snapshots = HashMap::new();
        snapshots.insert(dep_key(alias, "1.0.0"), SnapshotEntry::default());

        let lockfile = lockfile_with(root_deps, packages, snapshots);
        let opts = LockfileToHoistedDepGraphOptions { force: true, ..host_aware_opts() };
        let err = lockfile_to_hoisted_dep_graph(&lockfile, None, &opts)
            .expect_err("invalid alias must be rejected");
        match err {
            HoistedDepGraphError::InvalidDependencyAlias(inner) => {
                assert_eq!(inner.alias, alias);
            }
            other => panic!("expected InvalidDependencyAlias error for {alias:?}, got {other:?}"),
        }
    }
}
