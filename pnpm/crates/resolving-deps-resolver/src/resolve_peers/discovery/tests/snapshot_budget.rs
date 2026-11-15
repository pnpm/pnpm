use super::{
    Arc, DepPath, PeerHoistDiscovery, ResolvePeersOptions, WorkspaceTreeCtx, add_lazy_direct_dep,
    child_edge, package,
};

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

    engine.discover(&workspace, &direct_before, &direct_before, ResolvePeersOptions::default());
    engine.check_snapshot_limit().expect("1 snapshot <= 2");

    workspace.tree.bump_revision();
    engine.discover(&workspace, &direct_after, &direct_after[1..], ResolvePeersOptions::default());
    engine
        .check_snapshot_limit()
        .expect("peer-context change must not retain stale pre-hoist discovery paths");
    assert_eq!(engine.snapshot_budget.live_paths.len(), 2);

    workspace.tree.bump_revision();
    engine.discover(&workspace, &direct_after, &[], ResolvePeersOptions::default());
    assert_eq!(engine.snapshot_budget.live_paths.len(), 2);

    workspace.tree.record_children_rewrite();
    workspace.tree.bump_revision();
    engine.discover(&workspace, &direct_after, &[], ResolvePeersOptions::default());
    assert!(engine.snapshot_budget.live_paths.is_empty());
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

    engine.discover(&workspace, &direct_round1, &direct_round1, ResolvePeersOptions::default());
    engine.check_snapshot_limit().expect("3 live snapshots fit within max_snapshots = 3");

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

#[test]
fn discovery_snapshot_budget_handles_aliased_child_peer_providers_without_looping() {
    let workspace = WorkspaceTreeCtx::default();
    let mut engine = PeerHoistDiscovery::with_max_snapshots(Some(4));

    let view = &mut engine.tree;
    view.all_peer_dep_names.insert("react".to_string());
    view.packages.insert("mid@1.0.0".into(), package("mid", "1.0.0", &[], false));
    view.packages.insert(
        "consumer@1.0.0".into(),
        package("consumer", "1.0.0", &[("react", "^18.0.0")], true),
    );
    view.packages.insert("react@18.0.0".into(), package("react", "18.0.0", &[], true));
    view.packages.insert("extra@1.0.0".into(), package("extra", "1.0.0", &[], true));
    view.children_by_id.insert(
        "mid@1.0.0".into(),
        Arc::new(vec![
            child_edge("aliased-react", "react@18.0.0"),
            child_edge("consumer", "consumer@1.0.0"),
        ]),
    );

    let mut direct_round1 = Vec::new();
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut direct_round1, "mid", "mid@1.0.0");
    let mut direct_round2 = direct_round1.clone();
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut direct_round2, "extra", "extra@1.0.0");

    engine.discover(&workspace, &direct_round1, &direct_round1, ResolvePeersOptions::default());
    engine.check_snapshot_limit().expect("3 live snapshots fit within max_snapshots = 4");
    assert_eq!(engine.snapshot_budget.live_paths.len(), 3);

    workspace.tree.bump_revision();
    engine.discover(
        &workspace,
        &direct_round2,
        &direct_round2[1..],
        ResolvePeersOptions::default(),
    );
    engine.check_snapshot_limit().expect("4 live snapshots fit within max_snapshots = 4");
    assert_eq!(engine.snapshot_budget.live_paths.len(), 4);
}

#[test]
fn discovery_snapshot_budget_converges_across_three_hop_direct_peer_chain() {
    let workspace = WorkspaceTreeCtx::default();
    let mut engine = PeerHoistDiscovery::with_max_snapshots(Some(4));

    let view = &mut engine.tree;
    for peer in ["pkg-b", "pkg-c", "pkg-e"] {
        view.all_peer_dep_names.insert(peer.to_string());
    }
    view.packages.insert(
        "pkg-a@1.0.0".into(),
        package("pkg-a", "1.0.0", &[("pkg-b", "^1.0.0"), ("pkg-e", "^1.0.0")], true),
    );
    view.packages.insert(
        "pkg-b@1.0.0".into(),
        package("pkg-b", "1.0.0", &[("pkg-c", "^1.0.0"), ("pkg-e", "^1.0.0")], true),
    );
    view.packages.insert(
        "pkg-c@1.0.0".into(),
        package("pkg-c", "1.0.0", &[("pkg-e", "^1.0.0")], true),
    );
    view.packages.insert("pkg-e@1.0.0".into(), package("pkg-e", "1.0.0", &[], true));

    // Order `[pkg-a, pkg-b, pkg-c]` ensures refresh walks `pkg-a` before `pkg-b` and
    // `pkg-b` before `pkg-c`, requiring three refresh passes for `pkg-a` to converge
    // once `pkg-e` is hoisted incrementally.
    let mut direct_round1 = Vec::new();
    for (alias, pkg_id) in
        [("pkg-a", "pkg-a@1.0.0"), ("pkg-b", "pkg-b@1.0.0"), ("pkg-c", "pkg-c@1.0.0")]
    {
        add_lazy_direct_dep(&mut view.dependencies_tree, &mut direct_round1, alias, pkg_id);
    }
    let mut direct_round2 = direct_round1.clone();
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut direct_round2, "pkg-e", "pkg-e@1.0.0");

    engine.discover(&workspace, &direct_round1, &direct_round1, ResolvePeersOptions::default());
    engine.check_snapshot_limit().expect("3 live snapshots fit within max_snapshots = 4");

    workspace.tree.bump_revision();
    engine.discover(
        &workspace,
        &direct_round2,
        &direct_round2[3..],
        ResolvePeersOptions::default(),
    );
    engine.check_snapshot_limit().expect("4 converged live snapshots fit within max_snapshots = 4");
    assert_eq!(engine.snapshot_budget.live_paths.len(), 4);
    assert!(
        engine.snapshot_budget.live_paths.contains(&DepPath::from(
            "pkg-a@1.0.0(pkg-b@1.0.0(pkg-c@1.0.0(pkg-e@1.0.0))(pkg-e@1.0.0))(pkg-e@1.0.0)",
        )),
        "expected converged 3-hop path for pkg-a, got: {:?}",
        engine.snapshot_budget.live_paths,
    );
}

#[test]
fn discovery_snapshot_budget_collapses_cyclic_peers_and_converges_across_hoist_rounds() {
    let workspace = WorkspaceTreeCtx::default();
    let mut engine = PeerHoistDiscovery::with_max_snapshots(Some(3));

    let view = &mut engine.tree;
    for peer in ["pkg-a", "pkg-b", "pkg-e"] {
        view.all_peer_dep_names.insert(peer.to_string());
    }
    view.packages.insert(
        "pkg-a@1.0.0".into(),
        package("pkg-a", "1.0.0", &[("pkg-b", "^1.0.0"), ("pkg-e", "^1.0.0")], true),
    );
    view.packages.insert(
        "pkg-b@1.0.0".into(),
        package("pkg-b", "1.0.0", &[("pkg-a", "^1.0.0"), ("pkg-e", "^1.0.0")], true),
    );
    view.packages.insert("pkg-e@1.0.0".into(), package("pkg-e", "1.0.0", &[], true));

    let mut direct_round1 = Vec::new();
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut direct_round1, "pkg-a", "pkg-a@1.0.0");
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut direct_round1, "pkg-b", "pkg-b@1.0.0");
    let mut direct_round2 = direct_round1.clone();
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut direct_round2, "pkg-e", "pkg-e@1.0.0");

    engine.discover(&workspace, &direct_round1, &direct_round1, ResolvePeersOptions::default());
    engine
        .check_snapshot_limit()
        .expect("collapsed 2-node peer cycle fits within max_snapshots = 3");
    assert_eq!(engine.snapshot_budget.live_paths.len(), 2);
    assert!(
        engine.snapshot_budget.live_paths.contains(&DepPath::from("pkg-a@1.0.0(pkg-b@1.0.0)")),
        "expected collapsed cyclic path for pkg-a in round 1, got: {:?}",
        engine.snapshot_budget.live_paths,
    );
    assert!(
        engine.snapshot_budget.live_paths.contains(&DepPath::from("pkg-b@1.0.0(pkg-a@1.0.0)")),
        "expected collapsed cyclic path for pkg-b in round 1, got: {:?}",
        engine.snapshot_budget.live_paths,
    );

    workspace.tree.bump_revision();
    engine.discover(
        &workspace,
        &direct_round2,
        &direct_round2[2..],
        ResolvePeersOptions::default(),
    );
    engine
        .check_snapshot_limit()
        .expect("collapsed cyclic peers plus hoisted pkg-e fit within max_snapshots = 3");
    assert_eq!(engine.snapshot_budget.live_paths.len(), 3);
    assert!(
        engine.snapshot_budget.live_paths.contains(&DepPath::from(
            "pkg-a@1.0.0(pkg-b@1.0.0)(pkg-e@1.0.0)",
        )),
        "expected converged path for pkg-a in round 2, got: {:?}",
        engine.snapshot_budget.live_paths,
    );
    assert!(
        engine.snapshot_budget.live_paths.contains(&DepPath::from(
            "pkg-b@1.0.0(pkg-a@1.0.0)(pkg-e@1.0.0)",
        )),
        "expected converged path for pkg-b in round 2, got: {:?}",
        engine.snapshot_budget.live_paths,
    );
}
