use super::{
    super::{LockfileToHoistedDepGraphOptions, lockfile_to_hoisted_dep_graph},
    dep_key, metadata_stub, pkg_name, resolved_dep, workspace_lockfile,
};
use pnpm_lockfile::{Lockfile, ResolvedDependencyMap, SnapshotEntry};
use pnpm_modules_yaml::DepPath;
use pretty_assertions::assert_eq;
use std::{collections::HashMap, path::PathBuf};

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
/// `hoist_workspace_packages: false` must NOT drop non-root
/// importers from the shared hoister tree — pnpm v11 attaches every
/// importer unconditionally and uses the knob only for root-level
/// name links of the workspace packages themselves. The walker
/// therefore still fans out into `packages/foo` and emits its deps.
#[test]
fn walker_hoist_workspace_packages_false_keeps_importer_deps() {
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
    // `b` is only used by `packages/foo`; with no conflict it hoists
    // to the root — the importer's subtree participates in the walk.
    assert!(result.graph.contains_key(&lockfile_dir.join("node_modules").join("b")));
    assert!(result.direct_dependencies_by_importer_id.contains_key(Lockfile::ROOT_IMPORTER_KEY));
    assert!(result.direct_dependencies_by_importer_id.contains_key("packages/foo"));
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
/// walk: in the workspace-hoisted case the root's `webpack@5.65.0`
/// lands at the root and `foo`'s `webpack@2.7.0` nests under `foo`.
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
