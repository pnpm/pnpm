use super::{
    super::{DependenciesGraph, DependenciesGraphNode},
    ACCEPTS_DEP_PATH, sample_resolution,
};
use pnpm_lockfile::PkgIdWithPatchHash;
use pnpm_modules_yaml::DepPath;
use pretty_assertions::assert_eq;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

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
        present: false,
    };

    let mut graph = DependenciesGraph::new();
    graph.insert(dir.clone(), node.clone());
    assert_eq!(graph.get(&dir), Some(&node));
}
