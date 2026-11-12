use super::{
    Arc, DepPath, HashSet, PeerHoistDiscovery, ResolvePeersOptions, WorkspaceTreeCtx,
    add_lazy_direct_dep, child_edge, package,
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
