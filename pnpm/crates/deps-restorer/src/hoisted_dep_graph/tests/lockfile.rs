use super::{
    super::{LockfileToHoistedDepGraphOptions, lockfile_to_hoisted_dep_graph},
    dep_key, lockfile_version, lockfile_with, metadata_stub, pkg_name, resolved_dep,
};
use pnpm_lockfile::{Lockfile, LockfileSettings, ResolvedDependencyMap, SnapshotEntry};
use pretty_assertions::assert_eq;
use std::{collections::HashMap, path::PathBuf};

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
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
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
// --- prev_graph tests ------------------------------------------------

/// When no current lockfile is supplied, `prev_graph` is `None`
/// (the empty-graph fallback) — the linker treats it the same as an
/// empty map (no orphans to remove on a fresh install).
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
/// A current lockfile with no `packages` map yields no `prev_graph`.
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
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
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
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
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
