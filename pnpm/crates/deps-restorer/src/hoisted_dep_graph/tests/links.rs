use super::{
    super::{
        HoistedDepGraphError, LockfileToHoistedDepGraphOptions, lockfile_to_hoisted_dep_graph,
    },
    dep_key, host_aware_opts, lockfile_with, metadata_stub, pkg_name, resolved_dep,
};
use pnpm_lockfile::{Lockfile, ResolvedDependencyMap, SnapshotEntry};
use pretty_assertions::assert_eq;
use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
};

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
/// Surfaces `ERR_PNPM_INVALID_DEPENDENCY_NAME`; `force: true`
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
