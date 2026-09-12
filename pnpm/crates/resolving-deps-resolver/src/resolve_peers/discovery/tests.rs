//! Unit tests for the peer-hoist discovery engine.

use super::{PeerDiscoveryCaches, PeerHoistDiscovery, discover_peers};
use crate::{
    node_id::NodeId,
    resolve_dependency_tree::WorkspaceTreeCtx,
    resolve_peers::{
        ResolvePeersOptions,
        test_support::{package, tree_node},
    },
    resolved_tree::{DirectDep, ResolvedTree},
};
use pnpm_deps_path::DepPath;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{collections::BTreeMap, sync::Arc};

/// See [`PeersCacheItem`] for why a cache hit reports no providers.
#[test]
fn cached_subtree_reuse_reports_no_peer_providers() {
    let peerx = NodeId::leaf("peerx@1.0.0");
    let peerpkg = NodeId::leaf("peerpkg@2.0.0");
    let consumer = NodeId::next();
    let mid = NodeId::next();

    let mut mid_children = BTreeMap::new();
    mid_children.insert("peerpkg".to_string(), peerpkg.clone());
    mid_children.insert("consumer".to_string(), consumer.clone());

    let mut tree = ResolvedTree {
        direct: vec![
            DirectDep {
                alias: "mid".to_string(),
                node_id: mid.clone(),
                id: "mid@1.0.0".to_string(),
            },
            DirectDep {
                alias: "peerx".to_string(),
                node_id: peerx.clone(),
                id: "peerx@1.0.0".to_string(),
            },
        ],
        packages: HashMap::from_iter([
            ("peerx@1.0.0".into(), package("peerx", "1.0.0", &[], true)),
            ("peerpkg@2.0.0".into(), package("peerpkg", "2.0.0", &[], true)),
            (
                Arc::from("consumer@1.0.0".to_string()),
                package("consumer", "1.0.0", &[("peerpkg", "*"), ("peerx", "*")], false),
            ),
            ("mid@1.0.0".into(), package("mid", "1.0.0", &[], false)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (peerx, tree_node("peerx@1.0.0", BTreeMap::new(), 0)),
            (peerpkg, tree_node("peerpkg@2.0.0", BTreeMap::new(), 1)),
            (consumer, tree_node("consumer@1.0.0", BTreeMap::new(), 1)),
            (mid, tree_node("mid@1.0.0", mid_children, 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["peerpkg".to_string(), "peerx".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };
    let direct = tree.direct.clone();

    let (first, caches) = discover_peers(
        &mut tree,
        &direct,
        &direct,
        PeerDiscoveryCaches::default(),
        ResolvePeersOptions::default(),
    );
    assert!(
        first.resolved_peer_providers_by_alias.contains_key("peerpkg"),
        "the walk that resolves the subtree reports its providers",
    );

    let (second, _) =
        discover_peers(&mut tree, &direct, &direct, caches, ResolvePeersOptions::default());
    assert_eq!(
        second.resolved_peer_providers_by_alias.get("peerpkg"),
        None,
        "a cached-subtree reuse must not re-report the owner walk's providers",
    );
}

#[test]
fn discovery_engine_rebuilds_after_a_children_ownership_rewrite() {
    let workspace = WorkspaceTreeCtx::default();
    let mut engine = PeerHoistDiscovery::new();
    engine.discover(&workspace, &[], &[], ResolvePeersOptions::default());

    // A marker in the persistent caches makes reset-vs-merge observable.
    engine.caches.pure_pkgs.insert("marker@1.0.0".to_string(), DepPath::from("marker@1.0.0"));
    // Production rewrites happen inside `extend_tree`, which always
    // bumps the revision; mirror that pairing.
    workspace.record_children_rewrite();
    workspace.bump_revision();
    engine.discover(&workspace, &[], &[], ResolvePeersOptions::default());
    assert!(
        engine.caches.pure_pkgs.is_empty(),
        "an ownership rewrite must discard walk state derived before it",
    );

    engine.caches.pure_pkgs.insert("marker@1.0.0".to_string(), DepPath::from("marker@1.0.0"));
    workspace.bump_revision();
    engine.discover(&workspace, &[], &[], ResolvePeersOptions::default());
    assert!(
        engine.caches.pure_pkgs.contains_key("marker@1.0.0"),
        "a rewrite-free revision bump merges instead of rebuilding",
    );
}
