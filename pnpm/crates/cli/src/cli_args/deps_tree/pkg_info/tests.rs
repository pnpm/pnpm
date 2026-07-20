//! Ports of the upstream `getPkgInfo` tests
//! (deps/inspection/tree-builder/test/getPkgInfo.test.ts).

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use pacquet_lockfile::Lockfile;
use pretty_assertions::assert_eq;

use super::{EdgeContext, PkgInfoEnv, get_pkg_info};
use crate::cli_args::deps_tree::graph::GraphEdge;

// Port of upstream's 'getPkgInfo handles missing pkgSnapshot without crashing'
// (deps/inspection/tree-builder/test/getPkgInfo.test.ts). Upstream asserts
// `isMissing: true`; `DependencyNode` has no such field, so this port asserts
// the observable fallbacks instead: `name` falls back to the alias and
// `version` to the raw reference.
#[test]
fn get_pkg_info_handles_missing_pkg_snapshot_without_crashing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n\nimporters:\n  .: {}\n",
    )
    .unwrap();
    let lockfile = Lockfile::load_wanted_from_dir(dir.path()).unwrap().unwrap();

    let env = PkgInfoEnv {
        lockfile_dir: PathBuf::new(),
        modules_dir: PathBuf::new(),
        virtual_store_dir: PathBuf::from(".pnpm"),
        virtual_store_dir_max_length: 120,
        registries: HashMap::from([(
            "default".to_string(),
            "https://registry.npmjs.org/".to_string(),
        )]),
        skipped: HashSet::new(),
        store_dir: None,
        current_lockfile: &lockfile,
        wanted_lockfile: Some(&lockfile),
        dep_types: HashMap::new(),
    };
    let edge = GraphEdge {
        alias: "missing-pkg".to_string(),
        ref_display: "missing-pkg@1.0.0".to_string(),
        dep_path: Some("missing-pkg@1.0.0".parse().unwrap()),
        link_target: None,
        target: None,
    };
    let ctx = EdgeContext {
        peers: None,
        linked_path_base_dir: PathBuf::new(),
        rewrite_link_version_dir: None,
        parent_dir: None,
    };

    let (node, _manifest) = get_pkg_info(&env, &edge, &ctx);

    dbg!(&node);
    assert_eq!(node.alias, "missing-pkg");
    assert_eq!(node.name, "missing-pkg");
    assert_eq!(node.version, "missing-pkg@1.0.0");
    assert_eq!(
        node.path,
        Path::new(".pnpm")
            .join("missing-pkg@1.0.0")
            .join("node_modules")
            .join("missing-pkg")
            .to_string_lossy()
            .into_owned(),
    );
    assert_eq!(node.resolved, None);
    assert_eq!(node.dev, None);
    assert_eq!(node.peers_suffix_hash, None);
    assert!(!node.is_peer);
    assert!(!node.is_skipped);
    assert!(!node.optional);
}

// A lockfile-derived name with traversal segments is never joined into
// the package path (the same guard `pnpm licenses` applies to
// store paths built from lockfile keys).
#[test]
fn resolve_package_path_rejects_traversal_in_lockfile_derived_names() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n\nimporters:\n  .: {}\n",
    )
    .unwrap();
    let lockfile = Lockfile::load_wanted_from_dir(dir.path()).unwrap().unwrap();

    let virtual_store_dir = dir.path().join("node_modules").join(".pnpm");
    let env = PkgInfoEnv {
        lockfile_dir: dir.path().to_path_buf(),
        modules_dir: dir.path().join("node_modules"),
        virtual_store_dir: virtual_store_dir.clone(),
        virtual_store_dir_max_length: 120,
        registries: HashMap::from([(
            "default".to_string(),
            "https://registry.npmjs.org/".to_string(),
        )]),
        skipped: HashSet::new(),
        store_dir: None,
        current_lockfile: &lockfile,
        wanted_lockfile: Some(&lockfile),
        dep_types: HashMap::new(),
    };
    let ctx = EdgeContext {
        peers: None,
        linked_path_base_dir: PathBuf::new(),
        rewrite_link_version_dir: None,
        parent_dir: None,
    };

    let dep_path = "..@1.0.0".parse().unwrap();
    let path = super::resolve_package_path(&env, &dep_path, "../../../../escape", "alias", &ctx);

    assert_eq!(path, virtual_store_dir);
}
