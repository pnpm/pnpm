use super::{
    Arc, DirectDep, HashMap, HashSet, ImporterPeerInput, NodeId, PeerCycleShape,
    ResolvePeersOptions, ResolvedTree, order_test_shape, package, peer_cycle_fixture,
    peer_cycle_graph_keys, resolve_peers, resolve_peers_workspace,
};

/// End-to-end shape of a cycle package under canonical cycle-breaking:
/// the ring's one back-edge (`ring03 → ring00`) is cut identically at
/// every occurrence, so `ring00` has exactly two deterministic
/// variants — the position under the entry that walks the ring from
/// its canonical root (whose subtree reaches the `w` consumers), and
/// the shared back-edge occurrence resolved at importer context, where
/// no `w` is provided.
#[test]
fn a_cycle_package_resolves_identically_at_every_occurrence() {
    let mut tree = peer_cycle_fixture(
        &[("entry00", 0, "1.0.0"), ("entry01", 2, "2.0.0")],
        &PeerCycleShape { wc_members: vec![1, 3], rings_peer_on_p: true, ..Default::default() },
    );
    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

    assert!(
        result.peer_dependency_issues.missing.is_empty(),
        "unexpected missing peers: {:#?}",
        result.peer_dependency_issues.missing,
    );
    let mut ring00_variants: Vec<&str> = result
        .graph
        .keys()
        .map(pnpm_deps_path::DepPath::as_str)
        .filter(|path| path.starts_with("ring00@1.0.0"))
        .collect();
    ring00_variants.sort_unstable();
    assert_eq!(
        ring00_variants,
        ["ring00@1.0.0(p@1.0.0)", "ring00@1.0.0(p@1.0.0)(w@1.0.0)"],
        "one positional variant under the canonical-root entry, one importer-context          back-edge occurrence",
    );
}

/// The integrity bound behind pnpm/pnpm#13681: repeated entries into a
/// dense cyclic region must collapse onto shared occurrence subtrees
/// instead of materializing one per entry. Deterministic, no wall
/// clocks.
#[test]
fn cycle_re_walks_collapse_instead_of_multiplying_occurrences() {
    // Each entry provides its own `w`, so nothing untruncated transfers
    // across entries and only occurrence sharing can collapse the
    // repeated laps.
    let names: Vec<(String, String)> =
        (0..12).map(|index| (format!("entry{index:02}"), format!("{index}.0.0"))).collect();
    let entries: Vec<(&str, usize, &str)> =
        names.iter().map(|(alias, version)| (alias.as_str(), 0, version.as_str())).collect();
    let mut tree = peer_cycle_fixture(
        &entries,
        &PeerCycleShape {
            ring_len: 10,
            with_skips: true,
            wc_members: vec![1, 3, 5, 7, 9],
            rings_peer_on_p: true,
            ..Default::default()
        },
    );
    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

    assert!(
        result.peer_dependency_issues.missing.is_empty(),
        "the bound only means something for a healthy resolution",
    );
    let occurrences = tree.dependencies_tree.len();
    // The canonical walk realizes ~2,275 occurrences here; realizing
    // one subtree per entry would roughly double that. The bound sits
    // between, with headroom for fixture-neutral resolver changes.
    let bound = 3000;
    assert!(
        occurrences < bound,
        "peer walk realized {occurrences} occurrence nodes (bound {bound});          occurrence sharing has stopped collapsing re-walks",
    );
}

/// Importers are walked in id order, so reordering them cannot change
/// which context first realizes a shared back-edge occurrence — even
/// when their overlays provide conflicting versions (pnpm/pnpm#13846).
#[test]
fn backedge_bindings_do_not_depend_on_importer_order() {
    let graph_for_order = |first: &str, second: &str| {
        let edge = |alias: &str, pkg_id: &str| crate::resolved_tree::ChildEdge {
            alias: alias.to_string(),
            pkg_id: Arc::from(pkg_id),
            optional: false,
        };
        let mut packages = HashMap::default();
        packages.insert(Arc::from("p@1.0.0"), package("p", "1.0.0", &[], true));
        packages.insert(Arc::from("p@2.0.0"), package("p", "2.0.0", &[], true));
        packages
            .insert(Arc::from("ring00@1.0.0"), package("ring00", "1.0.0", &[("p", "*")], false));
        packages.insert(Arc::from("ring01@1.0.0"), package("ring01", "1.0.0", &[], false));
        packages.insert(Arc::from("enter-a@1.0.0"), package("enter-a", "1.0.0", &[], false));
        packages.insert(Arc::from("enter-b@1.0.0"), package("enter-b", "1.0.0", &[], false));
        let children_by_id: HashMap<Arc<str>, Arc<Vec<crate::resolved_tree::ChildEdge>>> =
            HashMap::from_iter([
                (Arc::from("ring00@1.0.0"), Arc::new(vec![edge("next", "ring01@1.0.0")])),
                (Arc::from("ring01@1.0.0"), Arc::new(vec![edge("back", "ring00@1.0.0")])),
                (Arc::from("enter-a@1.0.0"), Arc::new(vec![edge("ring", "ring00@1.0.0")])),
                (Arc::from("enter-b@1.0.0"), Arc::new(vec![edge("ring", "ring01@1.0.0")])),
            ]);

        let mut dependencies_tree = HashMap::default();
        let mut direct_dep = |pkg_id: &str, alias: &str| {
            let node_id = NodeId::next();
            dependencies_tree.insert(
                node_id.clone(),
                crate::resolved_tree::DependenciesTreeNode::new(
                    Arc::from(pkg_id),
                    crate::resolved_tree::TreeChildren::Lazy {
                        parent_ids: Arc::new(Vec::new()).into(),
                    },
                    0,
                    true,
                ),
            );
            DirectDep { alias: alias.to_string(), node_id, id: pkg_id.to_string() }
        };
        let importer = |id: &str, direct: Vec<DirectDep>| ImporterPeerInput {
            id: id.to_string(),
            direct,
            root_dir: std::path::PathBuf::from(format!("/repo/{id}")),
            modules_dir: None,
        };
        let root = importer(".", vec![direct_dep("p@1.0.0", "p")]);
        let importer_a =
            importer("a", vec![direct_dep("enter-a@1.0.0", "enter-a"), direct_dep("p@2.0.0", "p")]);
        let importer_b = importer("b", vec![direct_dep("enter-b@1.0.0", "enter-b")]);
        let importers: Vec<ImporterPeerInput> = [first, second]
            .iter()
            .map(|id| match *id {
                "a" => importer_a.clone(),
                _ => importer_b.clone(),
            })
            .collect();
        let importers = [vec![root], importers].concat();

        let mut tree = ResolvedTree {
            direct: Vec::new(),
            packages,
            dependencies_tree,
            all_peer_dep_names: HashSet::from_iter(["p".to_string()]),
            policy_violations: Vec::new(),
            applied_patches: HashSet::default(),
            children_by_id,
        };
        let result = resolve_peers_workspace(
            &mut tree,
            &importers,
            std::path::Path::new("/repo"),
            false,
            false,
            true,
            ResolvePeersOptions::default(),
        );
        let mut keys: Vec<String> =
            result.graph.keys().map(|path| path.as_str().to_string()).collect();
        keys.sort_unstable();
        keys
    };

    let a_first = graph_for_order("a", "b");
    let b_first = graph_for_order("b", "a");
    assert!(
        a_first.iter().any(|key| key == "ring00@1.0.0(p@2.0.0)"),
        "the back-edge occurrence binds the id-ordered first realizer's context;          got {a_first:#?}",
    );
    assert_eq!(a_first, b_first, "the graph must not depend on the importers' order");
}

/// The canonical cut is a property of the graph, not the walk: entering
/// the ring at `ring00` or at `ring02` first cannot make peer variants
/// appear or disappear (pnpm/pnpm#13865, pnpm/pnpm#13846).
#[test]
fn walk_order_cannot_change_the_graph() {
    let first_order = peer_cycle_graph_keys(
        &[("entry00", 0, "1.0.0"), ("entry01", 2, "2.0.0")],
        &order_test_shape(true),
    );
    let second_order = peer_cycle_graph_keys(
        &[("entry01", 2, "2.0.0"), ("entry00", 0, "1.0.0")],
        &order_test_shape(true),
    );
    assert!(
        first_order.iter().any(|key| key == "ring02@1.0.0(p@1.0.0)"),
        "ring members resolve their importer-provided p; got {first_order:#?}",
    );
    assert!(
        !first_order.iter().any(|key| key.starts_with("ring02") && key.contains("(w@")),
        "ring02's canonical subtree ends at the back-edge and reaches no w consumer;          got {first_order:#?}",
    );
    assert_eq!(first_order, second_order, "the graph must not depend on the entries' walk order");
}
