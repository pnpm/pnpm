use super::{
    super::{
        DepHierarchy, DependenciesGraph, HoistedDepGraphError, InstallabilityError,
        LockfileToDepGraphResult, LockfileToHoistedDepGraphOptions, lockfile_to_hoisted_dep_graph,
    },
    dep_key, directory_resolution, host_aware_opts, lockfile_with, metadata_stub, metadata_with_os,
    pkg_name, resolved_dep, ver_peer,
};
use pnpm_lockfile::{PackageMetadata, ResolvedDependencyMap, SnapshotDepRef, SnapshotEntry};
use pnpm_modules_yaml::DepPath;
use pretty_assertions::assert_eq;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::PathBuf,
};

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
        r#"a's `children["b"]` points at the hoisted (root-level) dir"#,
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
/// When `force` is off and the package is not installable, the
/// dep path is added to the skipped set and the walk skips it.
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
/// `engineStrict = true` + engine mismatch surfaces as
/// `HoistedDepGraphError::Installability`: the `engineStrict + engine
/// mismatch` case throws.
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
/// The prev-graph walk uses `force: true, skipped: empty` so
/// the *current* layout is preserved even for packages that
/// would now fail installability.
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
