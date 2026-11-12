//! Unit tests for the peer-hoist discovery engine.

use super::{
    CanonicalCycleGate, PeerDiscoveryCaches, PeerDiscoveryResult, PeerHoistDiscovery,
    ViewGeneration, discovery_provider_sources, walk_discovery_direct,
};
use crate::{
    node_id::NodeId,
    resolve_dependency_tree::WorkspaceTreeCtx,
    resolve_peers::{
        ResolvePeersOptions,
        test_support::{add_lazy_direct_dep, child_edge, package, tree_node},
        walker::{PeerWalkMode, RootWalk, Walker},
    },
    resolved_tree::{DirectDep, ResolvedTree},
};
use pnpm_deps_path::DepPath;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{collections::BTreeMap, sync::Arc};

fn discover_peers(
    tree: &mut ResolvedTree,
    parents_direct: &[DirectDep],
    walk_direct: &[DirectDep],
    caches: PeerDiscoveryCaches,
    opts: ResolvePeersOptions,
) -> (PeerDiscoveryResult, PeerDiscoveryCaches) {
    let current_provider_sources = discovery_provider_sources(parents_direct, &opts);
    let mut walker = Walker::new(
        tree,
        opts,
        HashMap::default(),
        current_provider_sources,
        caches,
        PeerWalkMode::Discovery,
    );
    let root = RootWalk::of(&walker, parents_direct);
    let result = walk_discovery_direct(&mut walker, walk_direct, &root);
    (result, walker.into_discovery_state().0)
}

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

#[test]
fn discovery_snapshot_budget_evicts_stale_paths_and_isolates_view_rebuild_reset() {
    let workspace = WorkspaceTreeCtx::default();
    let mut engine = PeerHoistDiscovery::with_max_snapshots(Some(2));

    let view = &mut engine.tree;
    view.all_peer_dep_names.insert("peerpkg".to_string());
    view.packages.insert(
        "consumer@1.0.0".into(),
        package("consumer", "1.0.0", &[("peerpkg", "^1.0.0")], true),
    );
    view.packages.insert("peerpkg@1.0.0".into(), package("peerpkg", "1.0.0", &[], true));
    let mut direct_before = Vec::new();
    add_lazy_direct_dep(
        &mut view.dependencies_tree,
        &mut direct_before,
        "consumer",
        "consumer@1.0.0",
    );
    let mut direct_after = direct_before.clone();
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut direct_after, "peerpkg", "peerpkg@1.0.0");

    // Round 1 (before `peerpkg` is hoisted): records bare `consumer@1.0.0`.
    engine.discover(&workspace, &direct_before, &direct_before, ResolvePeersOptions::default());
    engine.check_snapshot_limit().expect("1 snapshot <= 2");

    // Round 2 incremental hoist (`walk_direct = &direct_after[1..]`): stale bare
    // `consumer@1.0.0` is evicted and refreshed to `consumer@1.0.0(peerpkg@1.0.0)`
    // alongside `peerpkg@1.0.0` (2 live snapshots <= 2).
    workspace.tree.bump_revision();
    engine.discover(&workspace, &direct_after, &direct_after[1..], ResolvePeersOptions::default());
    engine
        .check_snapshot_limit()
        .expect("peer-context change must not retain stale pre-hoist discovery paths");
    assert_eq!(engine.snapshot_budget.discovery_dep_paths.len(), 2);

    // A rewrite-free revision bump with unchanged `parents_direct` and empty `walk_direct`
    // preserves the live snapshot budget.
    workspace.tree.bump_revision();
    engine.discover(&workspace, &direct_after, &[], ResolvePeersOptions::default());
    assert_eq!(engine.snapshot_budget.discovery_dep_paths.len(), 2);

    // A children-ownership rewrite with the same `parents_direct` and empty `walk_direct`
    // triggers `reset_for_view()` before refreshing against the rebuilt (empty) workspace view.
    workspace.tree.record_children_rewrite();
    workspace.tree.bump_revision();
    engine.discover(&workspace, &direct_after, &[], ResolvePeersOptions::default());
    assert!(engine.snapshot_budget.discovery_dep_paths.is_empty());
}

#[test]
fn discovery_snapshot_budget_retains_unwalked_live_subtrees_across_incremental_rounds() {
    let workspace = WorkspaceTreeCtx::default();
    let mut engine = PeerHoistDiscovery::with_max_snapshots(Some(3));

    let view = &mut engine.tree;
    view.all_peer_dep_names.insert("peerpkg".to_string());
    view.packages.insert("settled@1.0.0".into(), package("settled", "1.0.0", &[], false));
    view.packages.insert("leaf@1.0.0".into(), package("leaf", "1.0.0", &[], true));
    view.children_by_id.insert(
        "settled@1.0.0".into(),
        Arc::new(vec![child_edge("leaf", "leaf@1.0.0")]),
    );
    view.packages.insert(
        "consumer@1.0.0".into(),
        package("consumer", "1.0.0", &[("peerpkg", "^1.0.0")], true),
    );
    view.packages.insert("peerpkg@1.0.0".into(), package("peerpkg", "1.0.0", &[], true));

    let mut direct_round1 = Vec::new();
    add_lazy_direct_dep(
        &mut view.dependencies_tree,
        &mut direct_round1,
        "settled",
        "settled@1.0.0",
    );
    add_lazy_direct_dep(
        &mut view.dependencies_tree,
        &mut direct_round1,
        "consumer",
        "consumer@1.0.0",
    );
    let mut direct_round2 = direct_round1.clone();
    add_lazy_direct_dep(
        &mut view.dependencies_tree,
        &mut direct_round2,
        "peerpkg",
        "peerpkg@1.0.0",
    );

    // Round 1 walks `settled -> leaf` (2 snapshots) and `consumer` (1 snapshot) = 3 <= 3.
    engine.discover(&workspace, &direct_round1, &direct_round1, ResolvePeersOptions::default());
    engine.check_snapshot_limit().expect("3 live snapshots fit within max_snapshots = 3");

    // Round 2 bumps workspace revision and walks ONLY the newly hoisted `peerpkg`
    // (`&direct_round2[2..]`). `settled -> leaf` (2) + `consumer(peerpkg)` (1) + `peerpkg` (1)
    // = 4 live snapshots > 3, so `check_snapshot_limit` must fail immediately.
    workspace.tree.bump_revision();
    engine.discover(
        &workspace,
        &direct_round2,
        &direct_round2[2..],
        ResolvePeersOptions::default(),
    );
    let err = engine
        .check_snapshot_limit()
        .expect_err("incremental hoist round must retain unwalked live snapshots in budget");
    assert!(
        matches!(
            err,
            crate::ResolveImporterError::Resolve(
                crate::resolve_dependency_tree::ResolveDependencyTreeError::SnapshotLimitExceeded {
                    max_snapshots: 3,
                },
            ),
        ),
        "unexpected error: {err:?}",
    );
}
