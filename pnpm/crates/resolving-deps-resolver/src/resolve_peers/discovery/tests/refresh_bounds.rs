use super::{
    Arc, BTreeMap, DepPath, DirectDep, HashSet, NodeId, PeerHoistDiscovery, ResolvePeersOptions,
    WorkspaceTreeCtx, add_lazy_direct_dep, child_edge, package, tree_node,
};
use crate::{
    ImporterPeerInput, PeerWalkAbort, ResolutionCancel, ResolveImporterError,
    WorkspacePeerSettings, resolve_dependency_tree::ResolveDependencyTreeError,
    resolve_peers_workspace,
};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[test]
fn discovery_snapshot_budget_converges_across_deep_acyclic_chain_and_diamond_ladder() {
    let workspace = WorkspaceTreeCtx::default();
    let mut engine = PeerHoistDiscovery::with_max_snapshots(Some(11));

    let view = &mut engine.tree;
    for idx in 0..=8 {
        view.all_peer_dep_names.insert(format!("b{idx}"));
    }
    view.all_peer_dep_names.insert("pkg-e".to_string());

    // `hub` peers on 5 chain nodes (`b0, b2, b4, b6, b8`), exceeding
    // `MAX_DIRECT_DEP_REFRESHES = 4`. Topological subgraph refresh must wait
    // for `b8 -> ... -> b0` to settle first and walk `hub` once.
    view.packages.insert(
        "hub@1.0.0".into(),
        package(
            "hub",
            "1.0.0",
            &[
                ("b0", "^1.0.0"),
                ("b2", "^1.0.0"),
                ("b4", "^1.0.0"),
                ("b6", "^1.0.0"),
                ("b8", "^1.0.0"),
                ("pkg-e", "^1.0.0"),
            ],
            true,
        ),
    );
    for idx in 0..8 {
        let pkg_name = format!("b{idx}");
        let next_peer = format!("b{}", idx + 1);
        view.packages.insert(
            format!("{pkg_name}@1.0.0").into(),
            package(&pkg_name, "1.0.0", &[(&next_peer, "^1.0.0"), ("pkg-e", "^1.0.0")], true),
        );
    }
    view.packages.insert("b8@1.0.0".into(), package("b8", "1.0.0", &[("pkg-e", "^1.0.0")], true));
    view.packages.insert("pkg-e@1.0.0".into(), package("pkg-e", "1.0.0", &[], true));

    let mut direct_round1 = Vec::new();
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut direct_round1, "hub", "hub@1.0.0");
    for idx in 0..=8 {
        let name = format!("b{idx}");
        let id = format!("{name}@1.0.0");
        add_lazy_direct_dep(&mut view.dependencies_tree, &mut direct_round1, &name, &id);
    }
    let mut direct_round2 = direct_round1.clone();
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut direct_round2, "pkg-e", "pkg-e@1.0.0");

    engine.discover(&workspace, &direct_round1, &direct_round1, ResolvePeersOptions::default());
    engine.check_snapshot_limit().expect("10 initial snapshots fit within max_snapshots = 11");

    workspace.tree.bump_revision();
    engine.discover(
        &workspace,
        &direct_round2,
        &direct_round2[10..],
        ResolvePeersOptions::default(),
    );
    engine
        .check_snapshot_limit()
        .expect("deep acyclic chain + diamond ladder must converge without exceeding refresh cap");
    assert_eq!(engine.snapshot_budget.live_paths.len(), 11);
    assert!(
        engine.snapshot_budget.live_paths.contains(&DepPath::from(
            "b7@1.0.0(b8@1.0.0(pkg-e@1.0.0))(pkg-e@1.0.0)",
        )),
        "expected converged path for b7, got: {:?}",
        engine.snapshot_budget.live_paths,
    );
}

#[test]
fn discovery_snapshot_budget_bounds_cycle_work_independently_of_unrelated_direct_deps() {
    let polls = Arc::new(AtomicUsize::new(0));
    let polls_probe = Arc::clone(&polls);
    let cancel = ResolutionCancel::new(Arc::new(move || {
        polls_probe.fetch_add(1, Ordering::Relaxed);
        false
    }));
    let workspace = WorkspaceTreeCtx::default();
    let mut engine = PeerHoistDiscovery::with_limits(Some(600), cancel);

    let view = &mut engine.tree;
    for peer in ["pkg-a", "pkg-b", "pkg-e", "react"] {
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
    view.packages.insert("react@18.0.0".into(), package("react", "18.0.0", &[], true));

    let mut direct_round1 = Vec::new();
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut direct_round1, "pkg-a", "pkg-a@1.0.0");
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut direct_round1, "pkg-b", "pkg-b@1.0.0");
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut direct_round1, "react", "react@18.0.0");
    for idx in 0..250 {
        let plain_name = format!("plain-{idx}");
        let plain_id = format!("{plain_name}@1.0.0");
        view.packages.insert(plain_id.clone().into(), package(&plain_name, "1.0.0", &[], true));
        add_lazy_direct_dep(
            &mut view.dependencies_tree,
            &mut direct_round1,
            &plain_name,
            &plain_id,
        );

        let widget_name = format!("widget-{idx}");
        let widget_id = format!("{widget_name}@1.0.0");
        view.packages.insert(
            widget_id.clone().into(),
            package(&widget_name, "1.0.0", &[("react", "^18.0.0")], true),
        );
        add_lazy_direct_dep(
            &mut view.dependencies_tree,
            &mut direct_round1,
            &widget_name,
            &widget_id,
        );
    }
    let round1_len = direct_round1.len();
    let mut direct_round2 = direct_round1.clone();
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut direct_round2, "pkg-e", "pkg-e@1.0.0");

    engine.discover(&workspace, &direct_round1, &direct_round1, ResolvePeersOptions::default());
    engine.check_snapshot_limit().expect("initial round fits within max_snapshots = 600");

    polls.store(0, Ordering::Relaxed);
    workspace.tree.bump_revision();
    engine.discover(
        &workspace,
        &direct_round2,
        &direct_round2[round1_len..],
        ResolvePeersOptions::default(),
    );
    engine
        .check_snapshot_limit()
        .expect("collapsed 2-node cycle plus 500 unrelated direct deps fits within budget");
    assert!(
        engine.snapshot_budget.live_paths.contains(&DepPath::from(
            "pkg-a@1.0.0(pkg-b@1.0.0)(pkg-e@1.0.0)",
        )),
        "expected converged cyclic path for pkg-a, got: {:?}",
        engine.snapshot_budget.live_paths,
    );
    let round2_polls = polls.load(Ordering::Relaxed);
    assert!(
        round2_polls <= 16,
        "2-node cycle refresh must stay O(cycle), independent of 500 unrelated direct deps (polls = {round2_polls})",
    );
}

#[test]
fn discovery_evicts_consumer_cache_when_provider_first_resolves_nested_peer_suffix() {
    let workspace = WorkspaceTreeCtx::default();
    let mut engine = PeerHoistDiscovery::with_max_snapshots(Some(8));

    let view = &mut engine.tree;
    for peer in ["beta-provider", "gamma-subpeer"] {
        view.all_peer_dep_names.insert(peer.to_string());
    }
    view.packages.insert(
        "alpha-consumer@1.0.0".into(),
        package("alpha-consumer", "1.0.0", &[("beta-provider", "^1.0.0")], true),
    );
    view.packages.insert(
        "beta-provider@1.0.0".into(),
        package("beta-provider", "1.0.0", &[("gamma-subpeer", "*")], true),
    );
    for major in 1..=3 {
        let id = format!("gamma-subpeer@{major}.0.0");
        view.packages.insert(
            id.into(),
            package("gamma-subpeer", &format!("{major}.0.0"), &[], true),
        );
    }

    let mut importer_directs = Vec::new();
    for major in 1..=3 {
        let mut direct = Vec::new();
        // Consumer is intentionally listed BEFORE its peer provider so `beta-provider`
        // transitions `None -> Some("beta-provider@1.0.0(gamma-subpeer@...)")` after
        // `alpha-consumer` has already pushed a provisional `PeersCache` item.
        add_lazy_direct_dep(
            &mut view.dependencies_tree,
            &mut direct,
            "alpha-consumer",
            "alpha-consumer@1.0.0",
        );
        add_lazy_direct_dep(
            &mut view.dependencies_tree,
            &mut direct,
            "beta-provider",
            "beta-provider@1.0.0",
        );
        add_lazy_direct_dep(
            &mut view.dependencies_tree,
            &mut direct,
            "gamma-subpeer",
            &format!("gamma-subpeer@{major}.0.0"),
        );
        importer_directs.push(direct);
    }

    for (idx, direct) in importer_directs.iter().enumerate() {
        let opts = ResolvePeersOptions {
            project_dir: Some(PathBuf::from(format!("/workspace/pkg-{idx}"))),
            ..ResolvePeersOptions::default()
        };
        engine.discover(&workspace, direct, direct, opts);
        if idx < 2 {
            let expected_major = idx + 1;
            engine
                .check_snapshot_limit()
                .expect("first two importers produce 6 distinct snapshots <= 8");
            assert!(
                engine.snapshot_budget.live_paths.contains(&DepPath::from(format!(
                    "alpha-consumer@1.0.0(beta-provider@1.0.0(gamma-subpeer@{expected_major}.0.0))",
                ))),
                "importer {idx} must refresh consumer onto nested provider suffix, got: {:?}",
                engine.snapshot_budget.live_paths,
            );
        } else {
            let err = engine
                .check_snapshot_limit()
                .expect_err("3rd importer reaches 9 snapshots and must exceed max_snapshots = 8");
            assert!(
                matches!(
                    err,
                    ResolveImporterError::Resolve(
                        ResolveDependencyTreeError::SnapshotLimitExceeded { max_snapshots: 8 },
                    ),
                ),
                "expected SnapshotLimitExceeded(8), got: {err:?}",
            );
        }
    }
}

#[test]
fn discovery_refreshes_cross_importer_cache_hit_when_second_importer_provider_gains_suffix() {
    let workspace = WorkspaceTreeCtx::default();
    let mut engine = PeerHoistDiscovery::with_max_snapshots(Some(10));

    let view = &mut engine.tree;
    for peer in ["beta-provider", "gamma-subpeer"] {
        view.all_peer_dep_names.insert(peer.to_string());
    }
    view.packages.insert(
        "alpha-consumer@1.0.0".into(),
        package("alpha-consumer", "1.0.0", &[("beta-provider", "^1.0.0")], true),
    );
    view.packages.insert(
        "beta-provider@1.0.0".into(),
        package("beta-provider", "1.0.0", &[("gamma-subpeer", "^1.0.0")], true),
    );
    view.packages.insert(
        "gamma-subpeer@1.0.0".into(),
        package("gamma-subpeer", "1.0.0", &[], true),
    );

    let mut imp0_direct = Vec::new();
    add_lazy_direct_dep(
        &mut view.dependencies_tree,
        &mut imp0_direct,
        "beta-provider",
        "beta-provider@1.0.0",
    );
    add_lazy_direct_dep(
        &mut view.dependencies_tree,
        &mut imp0_direct,
        "alpha-consumer",
        "alpha-consumer@1.0.0",
    );

    let mut imp1_round1 = Vec::new();
    add_lazy_direct_dep(
        &mut view.dependencies_tree,
        &mut imp1_round1,
        "beta-provider",
        "beta-provider@1.0.0",
    );
    add_lazy_direct_dep(
        &mut view.dependencies_tree,
        &mut imp1_round1,
        "alpha-consumer",
        "alpha-consumer@1.0.0",
    );
    let mut imp1_round2 = imp1_round1.clone();
    add_lazy_direct_dep(
        &mut view.dependencies_tree,
        &mut imp1_round2,
        "gamma-subpeer",
        "gamma-subpeer@1.0.0",
    );

    let imp0_opts = ResolvePeersOptions {
        project_dir: Some(PathBuf::from("/workspace/imp0")),
        ..ResolvePeersOptions::default()
    };
    let imp1_opts = ResolvePeersOptions {
        project_dir: Some(PathBuf::from("/workspace/imp1")),
        ..ResolvePeersOptions::default()
    };

    engine.discover(&workspace, &imp0_direct, &imp0_direct, imp0_opts);
    engine.discover(&workspace, &imp1_round1, &imp1_round1, imp1_opts.clone());
    engine.check_snapshot_limit().expect("initial shared snapshots fit within budget");

    // Only importer 1 hoists `gamma-subpeer`; importer 1's cached `alpha-consumer`
    // must still detect that importer 1's own `beta-provider` gained a suffix.
    workspace.tree.bump_revision();
    engine.discover(&workspace, &imp1_round2, &imp1_round2[2..], imp1_opts);
    engine.check_snapshot_limit().expect("5 snapshots fit within max_snapshots = 10");
    assert!(
        engine.snapshot_budget.live_paths.contains(&DepPath::from(
            "alpha-consumer@1.0.0(beta-provider@1.0.0(gamma-subpeer@1.0.0))",
        )),
        "importer 1 consumer must refresh when its own provider gains a suffix, got: {:?}",
        engine.snapshot_budget.live_paths,
    );
}

#[test]
fn discovery_and_workspace_peer_walks_honor_cancellation() {
    let armed = Arc::new(AtomicBool::new(false));
    let polls = Arc::new(AtomicUsize::new(0));
    let cancel = ResolutionCancel::new(Arc::new({
        let armed = Arc::clone(&armed);
        let polls = Arc::clone(&polls);
        move || armed.load(Ordering::Relaxed) && polls.fetch_add(1, Ordering::Relaxed) >= 1
    }));

    let workspace = WorkspaceTreeCtx::default();
    let mut engine = PeerHoistDiscovery::with_limits(Some(100), cancel);
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

    let base_tree = engine.tree.clone();
    engine.discover(&workspace, &direct_round1, &direct_round1, ResolvePeersOptions::default());
    engine.check_snapshot_limit().expect("round 1 completes while cancellation is unarmed");

    // Arm cancellation so Round 2's refresh loop stops mid-pass.
    armed.store(true, Ordering::Relaxed);
    workspace.tree.bump_revision();
    engine.discover(
        &workspace,
        &direct_round2,
        &direct_round2[2..],
        ResolvePeersOptions::default(),
    );
    let err = engine
        .check_snapshot_limit()
        .expect_err("refresh cancellation must surface ResolutionCancelled");
    assert!(
        matches!(
            err,
            ResolveImporterError::Resolve(ResolveDependencyTreeError::ResolutionCancelled),
        ),
        "expected ResolutionCancelled, got: {err:?}",
    );
    let total_polls = polls.load(Ordering::Relaxed);
    assert!(
        total_polls >= 2,
        "expected refresh loop to poll armed cancellation check, got {total_polls}",
    );

    // Final workspace peer walk also honors `WorkspacePeerSettings::cancel`.
    let mut final_tree = base_tree;
    let importer = ImporterPeerInput {
        id: ".".to_string(),
        direct: direct_round2,
        root_dir: Path::new("/workspace").to_path_buf(),
        modules_dir: None,
        rejected_optional_peers: HashSet::default(),
    };
    let ws_polls = Arc::new(AtomicUsize::new(0));
    let ws_result = resolve_peers_workspace(
        &mut final_tree,
        &[importer],
        Path::new("/workspace"),
        WorkspacePeerSettings {
            max_snapshots: Some(100),
            cancel: ResolutionCancel::new(Arc::new({
                let ws_polls = Arc::clone(&ws_polls);
                move || ws_polls.fetch_add(1, Ordering::Relaxed) >= 1
            })),
            ..WorkspacePeerSettings::default()
        },
        ResolvePeersOptions::default(),
    );
    assert_eq!(
        ws_result.abort,
        Some(PeerWalkAbort::Cancelled),
        "expected workspace peer walk to abort with Cancelled",
    );
}

#[test]
fn discovery_preserves_cyclic_peer_sccs_across_incremental_rounds_and_multiple_importers() {
    let workspace = WorkspaceTreeCtx::default();
    let mut engine = PeerHoistDiscovery::with_max_snapshots(Some(20));

    let view = &mut engine.tree;
    for idx in 0..6 {
        view.all_peer_dep_names.insert(format!("c{idx}"));
    }
    view.all_peer_dep_names.insert("pkg-e".to_string());
    for idx in 0..6 {
        let name = format!("c{idx}");
        let next = format!("c{}", (idx + 1) % 6);
        let pkg = if idx == 5 {
            package(&name, "1.0.0", &[(&next, "^1.0.0"), ("pkg-e", "^1.0.0")], true)
        } else {
            package(&name, "1.0.0", &[(&next, "^1.0.0")], true)
        };
        view.packages.insert(format!("{name}@1.0.0").into(), pkg);
    }
    view.packages.insert("pkg-e@1.0.0".into(), package("pkg-e", "1.0.0", &[], true));

    let mut imp0_round1 = Vec::new();
    let mut imp1_round1 = Vec::new();
    for idx in 0..6 {
        let name = format!("c{idx}");
        let id = format!("{name}@1.0.0");
        add_lazy_direct_dep(&mut view.dependencies_tree, &mut imp0_round1, &name, &id);
        add_lazy_direct_dep(&mut view.dependencies_tree, &mut imp1_round1, &name, &id);
    }
    let mut imp0_round2 = imp0_round1.clone();
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut imp0_round2, "pkg-e", "pkg-e@1.0.0");
    let mut imp1_round2 = imp1_round1.clone();
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut imp1_round2, "pkg-e", "pkg-e@1.0.0");

    let imp0_opts = ResolvePeersOptions {
        project_dir: Some(PathBuf::from("/workspace/imp0")),
        ..ResolvePeersOptions::default()
    };
    let imp1_opts = ResolvePeersOptions {
        project_dir: Some(PathBuf::from("/workspace/imp1")),
        ..ResolvePeersOptions::default()
    };

    engine.discover(&workspace, &imp0_round1, &imp0_round1, imp0_opts.clone());
    engine.discover(&workspace, &imp1_round1, &imp1_round1, imp1_opts.clone());
    engine.check_snapshot_limit().expect("initial 6-node cycle across two importers fits budget");

    // Only `c5` peers on `pkg-e`; incremental Round 2 on both importers must retain
    // `external_peers` across `discover` calls and remap cached `NodeId`s per importer
    // so `{c0..c5}` stays collapsed without tearing down `cyclic_peers_of`.
    workspace.tree.bump_revision();
    engine.discover(&workspace, &imp0_round2, &imp0_round2[6..], imp0_opts);
    engine.discover(&workspace, &imp1_round2, &imp1_round2[6..], imp1_opts);
    engine
        .check_snapshot_limit()
        .expect("6-node cycle with single-member hoist must stay collapsed across rounds");
    assert_eq!(
        engine.snapshot_budget.live_paths.len(),
        7,
        "expected 6 collapsed cycle paths + pkg-e = 7 live snapshots, got: {:?}",
        engine.snapshot_budget.live_paths,
    );
    assert!(
        engine.snapshot_budget.live_paths.contains(&DepPath::from(
            "c5@1.0.0(c0@1.0.0)(pkg-e@1.0.0)",
        )),
        "expected collapsed c5 path with pkg-e, got: {:?}",
        engine.snapshot_budget.live_paths,
    );
}

#[test]
fn discovery_cache_hit_after_preview_undo_retains_own_child_pure_provider() {
    let workspace = WorkspaceTreeCtx::default();
    let mut engine = PeerHoistDiscovery::with_max_snapshots(Some(20));

    let view = &mut engine.tree;
    for peer in ["pkg-x", "pkg-z"] {
        view.all_peer_dep_names.insert(peer.to_string());
    }
    view.packages.insert("leaf@1.0.0".into(), package("leaf", "1.0.0", &[], true));
    // Non-leaf pure provider (`is_leaf == false`) so `enter_package` inserts a
    // `NodeId::Counter(...)` preview node into `dependencies_tree` that `undo_realize`
    // later removes on a `finish_cache_hit`.
    view.packages.insert("pkg-x@1.0.0".into(), package("pkg-x", "1.0.0", &[], false));
    view.children_by_id.insert(
        "pkg-x@1.0.0".into(),
        Arc::new(vec![child_edge("leaf", "leaf@1.0.0")]),
    );
    view.packages.insert("pkg-x@2.0.0".into(), package("pkg-x", "2.0.0", &[], true));
    view.packages.insert("pkg-z@1.0.0".into(), package("pkg-z", "1.0.0", &[], true));
    view.packages.insert(
        "pkg-b@1.0.0".into(),
        package("pkg-b", "1.0.0", &[("pkg-x", "^1.0.0")], false),
    );
    view.children_by_id.insert(
        "pkg-b@1.0.0".into(),
        Arc::new(vec![child_edge("pkg-x", "pkg-x@1.0.0")]),
    );
    view.packages.insert(
        "pkg-d@1.0.0".into(),
        package("pkg-d", "1.0.0", &[("pkg-z", "^1.0.0")], false),
    );
    view.children_by_id.insert(
        "pkg-d@1.0.0".into(),
        Arc::new(vec![child_edge("pkg-b", "pkg-b@1.0.0")]),
    );

    let mut imp0_direct = Vec::new();
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut imp0_direct, "pkg-b", "pkg-b@1.0.0");

    let mut imp1_direct = Vec::new();
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut imp1_direct, "pkg-x", "pkg-x@2.0.0");
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut imp1_direct, "pkg-z", "pkg-z@1.0.0");
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut imp1_direct, "pkg-d", "pkg-d@1.0.0");

    engine.discover(
        &workspace,
        &imp0_direct,
        &imp0_direct,
        ResolvePeersOptions {
            project_dir: Some(PathBuf::from("/workspace/imp0")),
            ..ResolvePeersOptions::default()
        },
    );
    // In `imp1`, `pkg-x@2.0.0` is in `parent_refs` while `pkg-b` also has own child
    // `pkg-x@1.0.0`, forcing `FastProvider::Ambiguous` -> `enter_package` preview ->
    // `try_resolve_cached_node` -> `finish_cache_hit` (`undo_realize` + remap).
    engine.discover(
        &workspace,
        &imp1_direct,
        &imp1_direct,
        ResolvePeersOptions {
            project_dir: Some(PathBuf::from("/workspace/imp1")),
            ..ResolvePeersOptions::default()
        },
    );
    engine.check_snapshot_limit().expect("own-child pure provider cache hit fits budget");
    assert!(
        engine.snapshot_budget.live_paths.contains(&DepPath::from(
            "pkg-d@1.0.0(pkg-x@1.0.0)(pkg-z@1.0.0)",
        )),
        "expected pkg-d to retain pkg-b's shadowed pkg-x@1.0.0 provider, got: {:?}",
        engine.snapshot_budget.live_paths,
    );
}

#[test]
fn discovery_refresh_releases_stale_ancestors_before_rebuilding_tight_budget() {
    let workspace = WorkspaceTreeCtx::default();
    let mut engine = PeerHoistDiscovery::with_max_snapshots(Some(4));

    let view = &mut engine.tree;
    for peer in ["pkg-p", "pkg-e"] {
        view.all_peer_dep_names.insert(peer.to_string());
    }
    view.packages.insert("pkg-a@1.0.0".into(), package("pkg-a", "1.0.0", &[], false));
    view.children_by_id.insert(
        "pkg-a@1.0.0".into(),
        Arc::new(vec![child_edge("pkg-b", "pkg-b@1.0.0")]),
    );
    view.packages.insert(
        "pkg-b@1.0.0".into(),
        package("pkg-b", "1.0.0", &[("pkg-p", "^1.0.0")], true),
    );
    view.packages.insert(
        "pkg-p@1.0.0".into(),
        package("pkg-p", "1.0.0", &[("pkg-e", "^1.0.0")], true),
    );
    view.packages.insert("pkg-e@1.0.0".into(), package("pkg-e", "1.0.0", &[], true));

    let mut round1 = Vec::new();
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut round1, "pkg-a", "pkg-a@1.0.0");
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut round1, "pkg-b", "pkg-b@1.0.0");
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut round1, "pkg-p", "pkg-p@1.0.0");
    let mut round2 = round1.clone();
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut round2, "pkg-e", "pkg-e@1.0.0");

    engine.discover(&workspace, &round1, &round1, ResolvePeersOptions::default());
    engine.check_snapshot_limit().expect("round 1 has 3 snapshots <= 4");

    workspace.tree.bump_revision();
    engine.discover(&workspace, &round2, &round2[3..], ResolvePeersOptions::default());
    engine
        .check_snapshot_limit()
        .expect("releasing stale A and B before rebuilding keeps peak snapshots <= 4");
    assert_eq!(engine.snapshot_budget.live_paths.len(), 4);
    assert!(engine.snapshot_budget.live_paths.contains(&DepPath::from(
        "pkg-a@1.0.0(pkg-p@1.0.0(pkg-e@1.0.0))",
    )),);

    // Also verify single-round consumer-before-provider refresh under the exact 4-snapshot cap.
    let mut single_round_engine = PeerHoistDiscovery::with_max_snapshots(Some(4));
    single_round_engine.tree = engine.tree.clone();
    single_round_engine.discover(&workspace, &round2, &round2, ResolvePeersOptions::default());
    single_round_engine
        .check_snapshot_limit()
        .expect("dirty-subgraph refresh releases stale roots together within budget of 4");
    assert_eq!(single_round_engine.snapshot_budget.live_paths.len(), 4);
}

#[test]
fn discovery_evicts_deep_provider_chain_iteratively_without_stack_overflow() {
    let handle = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let chain_len = 2_500usize;
            let workspace = WorkspaceTreeCtx::default();
            let mut engine = PeerHoistDiscovery::with_max_snapshots(Some(chain_len + 10));
            let view = &mut engine.tree;
            for idx in 0..chain_len {
                view.all_peer_dep_names.insert(format!("d{idx}"));
            }
            view.all_peer_dep_names.insert("pkg-e".to_string());
            for idx in 0..(chain_len - 1) {
                let name = format!("d{idx}");
                let next = format!("d{}", idx + 1);
                view.packages.insert(
                    format!("{name}@1.0.0").into(),
                    package(&name, "1.0.0", &[(&next, "^1.0.0"), ("pkg-e", "^1.0.0")], true),
                );
            }
            let tail = format!("d{}", chain_len - 1);
            view.packages.insert(
                format!("{tail}@1.0.0").into(),
                package(&tail, "1.0.0", &[("pkg-e", "^1.0.0")], true),
            );
            view.packages.insert("pkg-e@1.0.0".into(), package("pkg-e", "1.0.0", &[], true));

            let mut round1 = Vec::with_capacity(chain_len);
            for idx in 0..chain_len {
                let name = format!("d{idx}");
                let id = format!("{name}@1.0.0");
                add_lazy_direct_dep(&mut view.dependencies_tree, &mut round1, &name, &id);
            }
            let mut round2 = round1.clone();
            add_lazy_direct_dep(&mut view.dependencies_tree, &mut round2, "pkg-e", "pkg-e@1.0.0");

            engine.discover(&workspace, &round1, &round1, ResolvePeersOptions::default());
            engine.check_snapshot_limit().expect("round 1 fits budget");

            workspace.tree.bump_revision();
            engine.discover(
                &workspace,
                &round2,
                &round2[chain_len..],
                ResolvePeersOptions::default(),
            );
            engine
                .check_snapshot_limit()
                .expect("iterative provider-first ordering fits 512 KiB stack");
            assert_eq!(engine.snapshot_budget.live_paths.len(), chain_len + 1);
        })
        .expect("spawn small-stack thread");
    handle.join().expect("deep provider chain must not overflow thread stack");
}

#[test]
fn discovery_recomputes_peer_cycle_when_provider_rebinds_with_unchanged_dep_path() {
    let workspace = WorkspaceTreeCtx::default();
    let mut engine = PeerHoistDiscovery::with_max_snapshots(Some(10));

    let view = &mut engine.tree;
    for peer in ["pkg-a", "pkg-b", "pkg-c"] {
        view.all_peer_dep_names.insert(peer.to_string());
    }
    view.packages.insert(
        "pkg-a@1.0.0".into(),
        package("pkg-a", "1.0.0", &[("pkg-b", "^1.0.0")], true),
    );
    view.packages.insert(
        "pkg-b@1.0.0".into(),
        package("pkg-b", "1.0.0", &[("pkg-c", "^1.0.0")], true),
    );
    view.packages.insert(
        "pkg-c@1.0.0".into(),
        package("pkg-c", "1.0.0", &[("pkg-a", "^1.0.0")], true),
    );

    let mut round1 = Vec::new();
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut round1, "pkg-a", "pkg-a@1.0.0");
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut round1, "pkg-b", "pkg-b@1.0.0");
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut round1, "pkg-c", "pkg-c@1.0.0");
    let node_a = round1[0].node_id.clone();

    let mut round2 = vec![round1[0].clone()];
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut round2, "pkg-b", "pkg-b@1.0.0");

    engine.discover(&workspace, &round1, &round1, ResolvePeersOptions::default());
    assert!(
        engine.snapshot_budget.cyclic_peers_of.contains_key(&node_a),
        "round 1 3-cycle must record node_a in cyclic_peers_of",
    );

    // Round 2 drops `pkg-c` and replaces `pkg-b` with a fresh `NodeId` whose `DepPath` is
    // bare `pkg-b@1.0.0`, keeping `node_a`'s `DepPath` (`pkg-a@1.0.0(pkg-b@1.0.0)`)
    // identical while breaking the cycle.
    workspace.tree.bump_revision();
    engine.discover(&workspace, &round2, &round2[1..], ResolvePeersOptions::default());
    engine.check_snapshot_limit().expect("broken cycle fits budget");
    assert!(
        !engine.snapshot_budget.cyclic_peers_of.contains_key(&node_a),
        "rebind to non-cyclic provider must clear stale cyclic_peers_of entry even when DepPath is unchanged",
    );
}

#[test]
fn discovery_refresh_preserves_in_tree_promoted_provider_context_and_budget() {
    let workspace = WorkspaceTreeCtx::default();
    // Exact final live-path count in rounds 2 and 3:
    // round 2: host@1.0.0, sib@1.0.0, plugin@1.0.0(sib@1.0.0),
    //          consumer@1.0.0(plugin@1.0.0(sib@1.0.0)) -> 4 snapshots.
    // round 3 (+ pkg-e): host@1.0.0(pkg-e@1.0.0), sib@1.0.0,
    //          plugin@1.0.0(pkg-e@1.0.0)(sib@1.0.0),
    //          consumer@1.0.0(pkg-e@1.0.0)(plugin@1.0.0(pkg-e@1.0.0)(sib@1.0.0)),
    //          pkg-e@1.0.0 -> 5 snapshots.
    // A phantom root-fallback snapshot for `plugin` would push round 2 to 5 or round 3 to 6.
    let mut engine = PeerHoistDiscovery::with_max_snapshots(Some(5));

    let view = &mut engine.tree;
    for peer in ["plugin", "sib", "pkg-e"] {
        view.all_peer_dep_names.insert(peer.to_string());
    }
    view.packages.insert(
        "host@1.0.0".into(),
        package("host", "1.0.0", &[("pkg-e", "^1.0.0")], false),
    );
    view.packages.insert("sib@1.0.0".into(), package("sib", "1.0.0", &[], true));
    view.packages.insert(
        "plugin@1.0.0".into(),
        package("plugin", "1.0.0", &[("sib", "^1.0.0"), ("pkg-e", "^1.0.0")], true),
    );
    view.packages.insert(
        "consumer@1.0.0".into(),
        package("consumer", "1.0.0", &[("plugin", "^1.0.0"), ("pkg-e", "^1.0.0")], true),
    );
    view.packages.insert("pkg-e@1.0.0".into(), package("pkg-e", "1.0.0", &[], true));

    // Allocate `plugin_id` BEFORE `host_id` so provider-first sorting places `plugin` ahead of
    // `host`, exercising the `own_pending` vs `provider_pending` partition directly.
    let plugin_id = NodeId::next();
    let sib_id = NodeId::next();
    let host_id = NodeId::next();
    view.dependencies_tree.insert(plugin_id.clone(), tree_node("plugin@1.0.0", BTreeMap::new(), 1));
    view.dependencies_tree.insert(sib_id.clone(), tree_node("sib@1.0.0", BTreeMap::new(), 1));
    view.dependencies_tree.insert(
        host_id.clone(),
        tree_node("host@1.0.0", BTreeMap::from([("sib".to_string(), sib_id.clone())]), 0),
    );

    let plugin_dep = DirectDep {
        alias: "plugin".to_string(),
        node_id: plugin_id.clone(),
        id: "plugin@1.0.0".into(),
    };
    let host_dep =
        DirectDep { alias: "host".to_string(), node_id: host_id, id: "host@1.0.0".into() };
    let mut consumer_vec = Vec::new();
    add_lazy_direct_dep(
        &mut view.dependencies_tree,
        &mut consumer_vec,
        "consumer",
        "consumer@1.0.0",
    );
    let consumer_dep = consumer_vec.pop().unwrap();

    let mut pkg_e_vec = Vec::new();
    add_lazy_direct_dep(&mut view.dependencies_tree, &mut pkg_e_vec, "pkg-e", "pkg-e@1.0.0");
    let pkg_e_dep = pkg_e_vec.pop().unwrap();

    let mut opts = ResolvePeersOptions::default();
    opts.scope.hoisted_peer_provider_node_ids.insert(plugin_id.clone());

    let mut pkg_f_vec = Vec::new();
    engine.tree.all_peer_dep_names.insert("pkg-f".to_string());
    engine.tree.packages.insert("pkg-f@1.0.0".into(), package("pkg-f", "1.0.0", &[], true));
    engine.tree.packages.insert(
        "host@1.0.0".into(),
        package("host", "1.0.0", &[("pkg-e", "^1.0.0"), ("pkg-f", "^1.0.0")], false),
    );
    engine.tree.packages.insert(
        "plugin@1.0.0".into(),
        package(
            "plugin",
            "1.0.0",
            &[("sib", "^1.0.0"), ("pkg-e", "^1.0.0"), ("pkg-f", "^1.0.0")],
            true,
        ),
    );
    engine.tree.packages.insert(
        "consumer@1.0.0".into(),
        package(
            "consumer",
            "1.0.0",
            &[("plugin", "^1.0.0"), ("pkg-e", "^1.0.0"), ("pkg-f", "^1.0.0")],
            true,
        ),
    );
    add_lazy_direct_dep(&mut engine.tree.dependencies_tree, &mut pkg_f_vec, "pkg-f", "pkg-f@1.0.0");
    let pkg_f_dep = pkg_f_vec.pop().unwrap();
    engine.snapshot_budget.max_snapshots = Some(6);

    // Round 1: `host` initially has only `sib` (`plugin` is pruned), so root-fallback `plugin`
    // MUST remain tracked in `scoped.by_direct_dep` and `live_paths`.
    let round1 = vec![consumer_dep.clone(), host_dep.clone(), plugin_dep.clone()];
    engine.discover(&workspace, &round1, &round1, opts.clone());
    engine.check_snapshot_limit().expect("round 1 fits budget");
    assert_eq!(engine.caches.node_dep_paths[&plugin_id], DepPath::from("plugin@1.0.0"));
    assert_eq!(engine.snapshot_budget.live_paths.len(), 4);
    assert!(engine.snapshot_budget.live_paths.contains(&DepPath::from("plugin@1.0.0")));

    // Round 2: `pkg-e` is hoisted while `plugin` is STILL pruned (`walk_direct = [pkg-e]`).
    // Because `plugin` was retained as a root-fallback budget entry, hoisting `pkg-e` refreshes
    // both `plugin` and `consumer`.
    let round2 =
        vec![consumer_dep.clone(), host_dep.clone(), plugin_dep.clone(), pkg_e_dep.clone()];
    workspace.tree.bump_revision();
    engine.discover(&workspace, &round2, std::slice::from_ref(&pkg_e_dep), opts.clone());
    engine.check_snapshot_limit().expect("round 2 fits budget");
    assert_eq!(
        engine.caches.node_dep_paths[&plugin_id],
        DepPath::from("plugin@1.0.0(pkg-e@1.0.0)"),
    );
    assert!(engine.snapshot_budget.live_paths.contains(&DepPath::from(
        "consumer@1.0.0(pkg-e@1.0.0)(plugin@1.0.0(pkg-e@1.0.0))",
    )),);
    assert_eq!(engine.snapshot_budget.live_paths.len(), 5);

    // Round 3: `host` now adopts `plugin` alongside `sib`, and `pkg-f` is hoisted (`walk_direct = [pkg-f]`).
    // `evict_stale_scope_budget` evicts `plugin`, `host`, and `consumer` together into
    // `walk_budget_only_direct_deps` with `plugin_id < host_id`. `own_pending` (`host`, `consumer`)
    // runs before `provider_pending` (`plugin`), `visited_in_tree` drops the obsolete root-fallback
    // entry, and `plugin` retains its in-tree `(sib@1.0.0)` context.
    engine.tree.dependencies_tree.insert(
        host_dep.node_id.clone(),
        tree_node(
            "host@1.0.0",
            BTreeMap::from([
                ("plugin".to_string(), plugin_id.clone()),
                ("sib".to_string(), sib_id),
            ]),
            0,
        ),
    );
    let round3 = vec![consumer_dep, host_dep, plugin_dep, pkg_e_dep, pkg_f_dep.clone()];
    workspace.tree.bump_revision();
    engine.discover(&workspace, &round3, std::slice::from_ref(&pkg_f_dep), opts);
    engine.check_snapshot_limit().expect("round 3 fits tight max_snapshots = 6");
    assert_eq!(
        engine.caches.node_dep_paths[&plugin_id],
        DepPath::from("plugin@1.0.0(pkg-e@1.0.0)(pkg-f@1.0.0)(sib@1.0.0)"),
        "promoted provider must keep its in-tree sibling peer context after budget refresh",
    );
    assert_eq!(engine.snapshot_budget.live_paths.len(), 6);
}
