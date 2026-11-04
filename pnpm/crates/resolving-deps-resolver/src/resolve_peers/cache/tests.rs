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
    ),);
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
    let parent_pkg_ids = SharedChain::default().pushed("loop@1.0.0".into());
    let children =
        walker.previously_resolved_children(&parent_node_ids, &parent_pkg_ids, "loop@1.0.0");

    assert_eq!(children.get("shared"), Some(&close_child));
}

#[test]
fn equal_provisional_paths_require_matching_nested_providers() {
    use super::CacheMatch;
    use crate::resolve_peers::context::ParentRef;

    let current = NodeId::next();
    let cached = NodeId::next();
    let current_inner = NodeId::next();
    let cached_inner = NodeId::next();
    let mut tree = ResolvedTree::default();
    let mut walker = walker_for_tests(&mut tree);
    for node_id in [&current, &cached] {
        walker.caches.node_dep_paths.insert(
            node_id.clone(),
            DepPath::from("provider@1.0.0(inner@1.0.0)"),
        );
        walker.caches.parent_pkgs_of_node.insert(node_id.clone(), Arc::new(HashMap::default()));
    }
    walker.nodes.external_peers.insert(
        current.clone(),
        Arc::new(HashMap::from_iter([("inner".into(), current_inner.clone())])),
    );
    walker.nodes.external_peers.insert(
        cached.clone(),
        Arc::new(HashMap::from_iter([("inner".into(), cached_inner.clone())])),
    );
    let current_ref =
        ParentRef { node_id: Some(current.clone()), version: "1.0.0".into(), alias: None };

    assert_eq!(walker.provider_identity_match(&current, &cached), CacheMatch::Ambiguous);
    assert!(!walker.parent_ref_matches_cached(&current_ref, &cached));
    walker.caches.node_dep_paths.insert(
        current_inner.clone(),
        DepPath::from("inner@1.0.0(peer@2.0.0)"),
    );
    walker.caches.node_dep_paths.insert(
        cached_inner.clone(),
        DepPath::from("inner@1.0.0(peer@1.0.0)"),
    );
    assert_eq!(walker.provider_identity_match(&current, &cached), CacheMatch::NoMatch);
    assert!(!walker.parent_ref_matches_cached(&current_ref, &cached));
    walker.nodes.cache_owners.insert(current_inner, cached_inner);
    assert_eq!(walker.provider_identity_match(&current, &cached), CacheMatch::Match);
    assert!(walker.parent_ref_matches_cached(&current_ref, &cached));
}

#[test]
fn provider_identity_comparison_terminates_on_peer_cycles() {
    use super::CacheMatch;

    let current = NodeId::next();
    let cached = NodeId::next();
    let mut tree = ResolvedTree::default();
    let mut walker = walker_for_tests(&mut tree);
    for node_id in [&current, &cached] {
        walker.caches.node_dep_paths.insert(
            node_id.clone(),
            DepPath::from("provider@1.0.0(loop@1.0.0)"),
        );
        walker.nodes.external_peers.insert(
            node_id.clone(),
            Arc::new(HashMap::from_iter([("loop".into(), node_id.clone())])),
        );
    }
    assert_eq!(walker.provider_identity_match(&current, &cached), CacheMatch::Match);
}
