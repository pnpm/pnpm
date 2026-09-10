use super::{
    super::{LockfileToHoistedDepGraphOptions, lockfile_to_hoisted_dep_graph},
    dep_key, host_aware_opts, lockfile_with, metadata_stub, metadata_with_os, pkg_name,
    resolved_dep,
};
use pnpm_lockfile::{Lockfile, ResolvedDependencyMap, SnapshotEntry};
use pnpm_modules_yaml::DepPath;
use pretty_assertions::assert_eq;
use std::{collections::HashMap, path::PathBuf};

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
/// Required (non-optional) package on an unsupported platform
/// proceeds with a warning rather than erroring — for the
/// required-incompatible case `package_is_installable` returns
/// `true` (only `engineStrict + engine mismatch` and
/// `InvalidNodeVersion` actually throw). The
/// warning log emit is out of scope here; the walker proceeds
/// silently for now.
#[test]
fn walker_emits_required_dep_with_unsupported_platform_as_warning() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));

    let mut packages = HashMap::new();
    packages.insert(dep_key("a", "1.0.0"), metadata_with_os("darwin"));

    let mut snapshots = HashMap::new();
    // optional: false — `package_is_installable`
    // returns `true` (warn but proceed) rather than `false`.
    snapshots.insert(dep_key("a", "1.0.0"), SnapshotEntry::default());

    let lockfile = lockfile_with(root_deps, packages, snapshots);
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &host_aware_opts())
        .expect("walker proceeds");

    assert_eq!(result.graph.len(), 1, "required incompatible dep emitted as warning");
    assert!(result.skipped.is_empty(), "required dep not added to skipped");
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
