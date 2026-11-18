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
        subtree_dep_paths: None,
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
    let current_ref = ParentRef {
        node_id: Some(current.clone()),
        version: "1.0.0".into(),
        alias: None,
        hoisted_provider: false,
    };

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

/// `strict` rejects `plugin@1.0.0` through an optional peer, which makes a
/// hoisted `plugin@1.0.0` a restricted provider.
fn tree_with_a_rejecting_optional_peer() -> ResolvedTree {
    use crate::resolve_peers::test_support::package_with_peer_dependencies;

    let (optional, is_leaf) = (true, true);
    let strict = package_with_peer_dependencies(
        "strict",
        "1.0.0",
        &[("plugin", "^2.0.0", optional)],
        is_leaf,
    );
    let plugin = package("plugin", "1.0.0", &[], is_leaf);
    let mut tree = ResolvedTree::default();
    tree.dependencies_tree.insert(
        NodeId::leaf(&plugin.id),
        tree_node(&plugin.id, BTreeMap::new(), 0),
    );
    for pkg in [strict, plugin] {
        tree.packages.insert(Arc::clone(&pkg.id), pkg);
    }
    tree
}

/// How an importer comes to provide `plugin@1.0.0`.
#[derive(Clone, Copy)]
enum PluginProvider {
    Hoisted,
    Declared,
}

/// What the cached walk did with its `plugin` peer.
#[derive(Clone, Copy)]
enum PluginPeer {
    Resolved,
    Missing,
}

fn plugin_ref(provider: PluginProvider) -> crate::resolve_peers::context::ParentRefs {
    use crate::resolve_peers::context::ParentRef;

    let hoisted_provider = match provider {
        PluginProvider::Hoisted => true,
        PluginProvider::Declared => false,
    };

    HashMap::from_iter([(
        "plugin".to_string(),
        ParentRef {
            node_id: Some(NodeId::leaf("plugin@1.0.0")),
            version: "1.0.0".into(),
            alias: None,
            hoisted_provider,
        },
    )])
}

fn cache_item_of_consumer(
    walker: &crate::resolve_peers::walker::Walker<'_>,
    recorded_under: &crate::resolve_peers::context::ParentRefs,
    plugin_peer: PluginPeer,
) -> super::PeersCacheItem {
    use crate::resolve_peers::walker::MissingPeerInfo;

    let plugin = "plugin".to_string();
    let resolved = HashMap::from_iter([(plugin.clone(), NodeId::leaf("plugin@1.0.0"))]);
    let missing = HashMap::from_iter([(
        plugin.clone(),
        MissingPeerInfo {
            range: "^2.0.0".to_string(),
            declaring_pkg_id: "strict@1.0.0".into(),
            optional: true,
        },
    )]);
    let (resolved, missing) = match plugin_peer {
        PluginPeer::Missing => (HashMap::default(), missing),
        PluginPeer::Resolved => (resolved, HashMap::default()),
    };
    super::PeersCacheItem {
        owner_node_id: NodeId::next(),
        dep_path: DepPath::from("consumer@1.0.0"),
        restricted_providers: walker.restricted_providers(recorded_under, std::iter::once(&plugin)),
        resolved_peers: Arc::new(resolved),
        missing_peers: Arc::new(missing),
        missing_peers_of_children: Arc::new(HashMap::default()),
        subtree_missing_by_pkg: None,
    }
}

#[test]
fn a_cached_verdict_is_reused_only_under_the_restriction_it_was_recorded_under() {
    use crate::resolve_peers::test_support::PeerCacheMode;

    let mut tree = tree_with_a_rejecting_optional_peer();
    let mut walker = walker_for_tests(&mut tree);
    walker.opts.cache_mode = PeerCacheMode::Enabled;
    let consumer: Arc<str> = "consumer@1.0.0".into();
    let hoisted = plugin_ref(PluginProvider::Hoisted);
    let declared = plugin_ref(PluginProvider::Declared);

    let bound_to_hoisted = cache_item_of_consumer(&walker, &hoisted, PluginPeer::Resolved);
    walker.caches.peers_cache.push(&consumer, bound_to_hoisted);
    assert!(walker.find_hit(&hoisted, &consumer).is_some());
    assert!(walker.find_hit(&declared, &consumer).is_none());

    let rejected_hoisted = cache_item_of_consumer(&walker, &hoisted, PluginPeer::Missing);
    let rejecter: Arc<str> = "rejecter@1.0.0".into();
    walker.caches.peers_cache.push(&rejecter, rejected_hoisted);
    assert!(walker.find_hit(&hoisted, &rejecter).is_some());
    assert!(walker.find_hit(&declared, &rejecter).is_none());

    let bound_to_declared = cache_item_of_consumer(&walker, &declared, PluginPeer::Resolved);
    let other: Arc<str> = "other@1.0.0".into();
    walker.caches.peers_cache.push(&other, bound_to_declared);
    assert!(walker.find_hit(&declared, &other).is_some());
    assert!(walker.find_hit(&hoisted, &other).is_none());
}

/// No optional peer range rejects `plugin@1.0.0` here, so a hoisted
/// provider resolves every subtree as a declared one does.
#[test]
fn an_unrestricted_hoisted_provider_reuses_a_verdict_recorded_under_a_declared_one() {
    use crate::resolve_peers::test_support::PeerCacheMode;

    let mut tree = tree_with_a_rejecting_optional_peer();
    tree.packages.remove("strict@1.0.0");
    let mut walker = walker_for_tests(&mut tree);
    walker.opts.cache_mode = PeerCacheMode::Enabled;
    let consumer: Arc<str> = "consumer@1.0.0".into();
    let hoisted = plugin_ref(PluginProvider::Hoisted);
    let declared = plugin_ref(PluginProvider::Declared);

    let bound_to_declared = cache_item_of_consumer(&walker, &declared, PluginPeer::Resolved);
    walker.caches.peers_cache.push(&consumer, bound_to_declared);
    assert!(walker.find_hit(&hoisted, &consumer).is_some());
}
