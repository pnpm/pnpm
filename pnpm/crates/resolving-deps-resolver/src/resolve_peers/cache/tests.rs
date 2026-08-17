//! Unit tests for the walk's cache and child-realization decisions.

use super::should_retain_materialized_node;
use crate::{
    node_id::NodeId,
    resolve_peers::{
        context::SharedChain,
        test_support::{package, tree_node, walker_for_tests},
        walker::NodeOutput,
    },
    resolved_tree::ResolvedTree,
};
use pnpm_deps_path::DepPath;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{collections::BTreeMap, sync::Arc};

#[test]
fn materialized_nodes_referenced_by_peer_outputs_are_retained() {
    let referenced = NodeId::next();
    let unreferenced = NodeId::next();
    let output = NodeOutput {
        dep_path: DepPath::from("consumer@1.0.0"),
        external_resolved_peers: Arc::new(HashMap::from_iter([(
            "peer".into(),
            referenced.clone(),
        )])),
        auto_install_resolved_peers: HashMap::default(),
        missing_peers: Arc::new(HashMap::default()),
        subtree_missing_by_pkg: None,
    };

    assert!(should_retain_materialized_node(&HashSet::default(), Some(&output), &referenced));
    assert!(!should_retain_materialized_node(&HashSet::default(), Some(&output), &unreferenced,));
    assert!(should_retain_materialized_node(
        &HashSet::from_iter([unreferenced.clone()]),
        None,
        &unreferenced,
    ));
}

#[test]
fn previously_resolved_children_prefers_closest_same_package_ancestor() {
    let far_parent = NodeId::next();
    let close_parent = NodeId::next();
    let far_child = NodeId::leaf("shared@1.0.0");
    let close_child = NodeId::leaf("shared@2.0.0");

    let mut far_children = BTreeMap::new();
    far_children.insert("shared".to_string(), far_child);
    let mut close_children = BTreeMap::new();
    close_children.insert("shared".to_string(), close_child.clone());

    let mut tree = ResolvedTree {
        direct: Vec::new(),
        packages: HashMap::from_iter([("loop@1.0.0".into(), package("loop", "1.0.0", &[], false))]),
        dependencies_tree: HashMap::from_iter([
            (far_parent.clone(), tree_node("loop@1.0.0", far_children, 0)),
            (close_parent.clone(), tree_node("loop@1.0.0", close_children, 2)),
        ]),
        all_peer_dep_names: HashSet::default(),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };
    let mut walker = walker_for_tests(&mut tree);

    let parent_node_ids = SharedChain::default().pushed(far_parent).pushed(close_parent);
    let parent_pkg_ids = SharedChain::default().pushed("loop@1.0.0".to_string());
    let children =
        walker.previously_resolved_children(&parent_node_ids, &parent_pkg_ids, "loop@1.0.0");

    assert_eq!(children.get("shared"), Some(&close_child));
}
