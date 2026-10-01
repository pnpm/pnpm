//! Unit tests for the peer-hoist discovery engine.

use super::{
    CanonicalCycleGate, PeerDiscoveryCaches, PeerHoistDiscovery, ViewGeneration, discover_peers,
};
use crate::{
    node_id::NodeId,
    resolve_dependency_tree::WorkspaceTreeCtx,
    resolve_peers::{
        ResolvePeersOptions,
        test_support::{add_lazy_direct_dep, child_edge, package, tree_node},
        walker::Walker,
    },
    resolved_tree::{DirectDep, ResolvedTree},
};
use pnpm_deps_path::DepPath;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{collections::BTreeMap, sync::Arc};

/// The children graph `c -> a <-> b`: a mutual dependency entered from a
/// package outside the cycle.
fn mutual_dependency_children() -> ResolvedTree {
    let mut tree = ResolvedTree::default();
    tree.children_by_id.insert("a@1.0.0".into(), Arc::new(vec![child_edge("b", "b@1.0.0")]));
    tree.children_by_id.insert("b@1.0.0".into(), Arc::new(vec![child_edge("a", "a@1.0.0")]));
    tree.children_by_id.insert("c@1.0.0".into(), Arc::new(vec![child_edge("a", "a@1.0.0")]));
    tree
}

#[test]
fn cycle_gate_cuts_one_edge_of_a_mutual_dependency() {
    let tree = mutual_dependency_children();
    let table = CanonicalCycleGate::default().table(&tree, ViewGeneration::default());

    assert_eq!(table["a@1.0.0"], table["b@1.0.0"], "mutually dependent packages form one cycle");
    assert_ne!(table["c@1.0.0"], table["a@1.0.0"], "a package that enters the cycle is outside it");
    assert!(
        Walker::cuts_cycle_edge(&table, "b@1.0.0", "a@1.0.0"),
        "the edge back to the cycle's canonically first member is cut",
    );
    assert!(
        !Walker::cuts_cycle_edge(&table, "a@1.0.0", "b@1.0.0"),
        "the cycle's forward edge is walked",
    );
    assert!(
        !Walker::cuts_cycle_edge(&table, "c@1.0.0", "a@1.0.0"),
        "the edge into the cycle is walked",
    );
}

#[test]
fn cycle_gate_rebuilds_its_table_for_a_newer_view_generation() {
    let mut tree = mutual_dependency_children();
    let gate = CanonicalCycleGate::default();
    let generation = ViewGeneration::default();
    let table = gate.table(&tree, generation);

    // The generation, not the tree, keys the table: the engine advances
    // it with every change of its view.
    tree.children_by_id.insert("b@1.0.0".into(), Arc::new(Vec::new()));
    assert!(
        Arc::ptr_eq(&table, &gate.table(&tree, generation)),
        "reads under one view generation share one table",
    );

    let mut newer = generation;
    newer.advance();
    let rebuilt = gate.table(&tree, newer);
    assert_ne!(
        rebuilt["a@1.0.0"], rebuilt["b@1.0.0"],
        "a newer generation rebuilds the table from the current view",
    );
    assert!(
        !Walker::cuts_cycle_edge(&rebuilt, "a@1.0.0", "b@1.0.0"),
        "without the cycle, the edge is walked",
    );
}

#[test]
fn discovery_engine_refreshes_the_cycle_gate_with_its_view() {
    let workspace = WorkspaceTreeCtx::default();
    let mut engine = PeerHoistDiscovery::new();
    // The view is filled directly: recording children is private to the
    // dependency walk, and this is the shape its sync produces.
    let view = &mut engine.tree;
    view.packages.insert("a@1.0.0".into(), package("a", "1.0.0", &[], false));
    view.packages.insert("b@1.0.0".into(), package("b", "1.0.0", &[], true));
    view.children_by_id.insert("a@1.0.0".into(), Arc::new(vec![child_edge("b", "b@1.0.0")]));
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut view.direct, "a", "a@1.0.0");
    let direct = engine.tree.direct.clone();
    engine.discover(&workspace, &direct, &direct, ResolvePeersOptions::default());
    let first_round = engine.caches.view_generation();
    let table = engine.caches.canonical_cycles.table(&engine.tree, first_round);
    assert!(table.get("p@1.0.0").is_none(), "the first round's view has no `p` yet");

    let view = &mut engine.tree;
    view.packages.insert("p@1.0.0".into(), package("p", "1.0.0", &[], false));
    view.packages.insert("q@1.0.0".into(), package("q", "1.0.0", &[], false));
    view.children_by_id.insert("p@1.0.0".into(), Arc::new(vec![child_edge("q", "q@1.0.0")]));
    view.children_by_id.insert("q@1.0.0".into(), Arc::new(vec![child_edge("p", "p@1.0.0")]));
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut view.direct, "p", "p@1.0.0");
    workspace.tree.bump_revision();
    let direct = engine.tree.direct.clone();
    engine.discover(&workspace, &direct, &[], ResolvePeersOptions::default());
    let table = engine.caches.canonical_cycles.table(&engine.tree, engine.caches.view_generation());
    let hoisted_cycle =
        table.get("p@1.0.0").expect("the refreshed gate indexes the hoisted package");
    assert_eq!(
        table.get("q@1.0.0"),
        Some(hoisted_cycle),
        "the refreshed gate knows the cycle the hoist round installed",
    );

    // Only a cut `q -> p` edge lets the walk over the cycle finish.
    let hoisted = direct[1..].to_vec();
    engine.discover(&workspace, &direct, &hoisted, ResolvePeersOptions::default());
    assert!(
        engine.caches.pure_pkgs.contains_key("p@1.0.0"),
        "the walk over the hoisted cycle completed and recorded its verdict",
    );
}

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
            DirectDep { alias: "mid".to_string(), node_id: mid.clone(), id: "mid@1.0.0".into() },
            DirectDep {
                alias: "peerx".to_string(),
                node_id: peerx.clone(),
                id: "peerx@1.0.0".into(),
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
    engine.caches.pure_pkgs.insert("marker@1.0.0".into(), DepPath::from("marker@1.0.0"));
    // Production rewrites happen inside `extend_tree`, which always
    // bumps the revision; mirror that pairing.
    workspace.tree.record_children_rewrite();
    workspace.tree.bump_revision();
    engine.discover(&workspace, &[], &[], ResolvePeersOptions::default());
    assert!(
        engine.caches.pure_pkgs.is_empty(),
        "an ownership rewrite must discard walk state derived before it",
    );

    engine.caches.pure_pkgs.insert("marker@1.0.0".into(), DepPath::from("marker@1.0.0"));
    workspace.tree.bump_revision();
    engine.discover(&workspace, &[], &[], ResolvePeersOptions::default());
    assert!(
        engine.caches.pure_pkgs.contains_key("marker@1.0.0"),
        "a rewrite-free revision bump merges instead of rebuilding",
    );
}
