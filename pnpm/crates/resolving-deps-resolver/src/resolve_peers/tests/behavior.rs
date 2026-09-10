use super::{
    Arc, BTreeMap, DepPath, DirectDep, HashMap, HashSet, NodeId, PeerCycleShape,
    ResolvePeersOptions, ResolvedTree, graph_node, order_test_shape, package, peer_cycle_fixture,
    peer_cycle_graph_keys, resolve_peers, tree_node, walker_for_tests,
};

#[test]
fn same_package_child_does_not_shadow_inherited_parent_and_bubbles_by_name() {
    let x1 = NodeId::leaf("x@1.0.0");
    let x2 = NodeId::leaf("x@2.0.0");
    let p_root = NodeId::next();
    let p_child = NodeId::next();
    let plugin = NodeId::next();
    let mid = NodeId::next();

    let mut mid_children = BTreeMap::new();
    mid_children.insert("p".to_string(), p_child.clone());
    mid_children.insert("plugin".to_string(), plugin.clone());
    mid_children.insert("x".to_string(), x2.clone());

    let mut tree = ResolvedTree {
        direct: vec![
            DirectDep { alias: "x".to_string(), node_id: x1.clone(), id: "x@1.0.0".to_string() },
            DirectDep {
                alias: "p".to_string(),
                node_id: p_root.clone(),
                id: "p@1.0.0".to_string(),
            },
            DirectDep {
                alias: "mid".to_string(),
                node_id: mid.clone(),
                id: "mid@1.0.0".to_string(),
            },
        ],
        packages: HashMap::from_iter([
            ("x@1.0.0".into(), package("x", "1.0.0", &[], true)),
            ("x@2.0.0".into(), package("x", "2.0.0", &[], true)),
            ("p@1.0.0".into(), package("p", "1.0.0", &[("x", "*")], false)),
            ("plugin@1.0.0".into(), package("plugin", "1.0.0", &[("p", "*")], false)),
            ("mid@1.0.0".into(), package("mid", "1.0.0", &[], false)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (x1, tree_node("x@1.0.0", BTreeMap::new(), 0)),
            (x2, tree_node("x@2.0.0", BTreeMap::new(), 1)),
            (p_root, tree_node("p@1.0.0", BTreeMap::new(), 0)),
            (p_child, tree_node("p@1.0.0", BTreeMap::new(), 1)),
            (plugin, tree_node("plugin@1.0.0", BTreeMap::new(), 1)),
            (mid, tree_node("mid@1.0.0", mid_children, 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["p".to_string(), "x".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

    assert_eq!(result.direct_dependencies_by_alias.get("mid"), Some(&DepPath::from("mid@1.0.0")));
    assert!(
        result.graph.contains_key(&DepPath::from("plugin@1.0.0(p@1.0.0(x@1.0.0))")),
        "plugin should resolve p from the inherited root context: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
    assert!(
        !result.graph.contains_key(&DepPath::from("plugin@1.0.0(p@1.0.0(x@2.0.0))")),
        "same-package child p must not shadow inherited p: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
}

/// A hoisted peer provider whose tree position was never visited (nothing in
/// the walk enumerates its node) must still be resolved by the root-context
/// fallback so consumers that bound it get a depPath.
#[test]
fn pruned_hoisted_provider_falls_back_to_root_resolution() {
    let prov = NodeId::leaf("prov@1.0.0");
    let consumer = NodeId::next();

    let mut tree = ResolvedTree {
        direct: vec![
            DirectDep {
                alias: "consumer".to_string(),
                node_id: consumer.clone(),
                id: "consumer@1.0.0".to_string(),
            },
            DirectDep {
                alias: "prov".to_string(),
                node_id: prov.clone(),
                id: "prov@1.0.0".to_string(),
            },
        ],
        packages: HashMap::from_iter([
            ("prov@1.0.0".into(), package("prov", "1.0.0", &[], true)),
            ("consumer@1.0.0".into(), package("consumer", "1.0.0", &[("prov", "*")], false)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (prov.clone(), tree_node("prov@1.0.0", BTreeMap::new(), 1)),
            (consumer, tree_node("consumer@1.0.0", BTreeMap::new(), 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["prov".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers(
        &mut tree,
        ResolvePeersOptions {
            hoisted_peer_provider_node_ids: HashSet::from_iter([prov]),
            ..ResolvePeersOptions::default()
        },
    );

    assert_eq!(
        result.direct_dependencies_by_alias.get("prov"),
        Some(&DepPath::from("prov@1.0.0")),
        "the pruned provider must get a depPath from the fallback",
    );
    assert!(
        result.graph.contains_key(&DepPath::from("consumer@1.0.0(prov@1.0.0)")),
        "the consumer must bind the fallback-resolved provider: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
}

/// The other half of first-resolvable-wins: when the earlier triple
/// does resolve, it keeps the slot and the later one is ignored.
#[test]
fn the_first_resolvable_pending_edge_keeps_the_slot() {
    let mut tree = ResolvedTree::default();
    let mut walker = walker_for_tests(&mut tree);

    let parent = DepPath::from("parent@1.0.0");
    let first_child = NodeId::next();
    let second_child = NodeId::next();
    let mut graph_children = BTreeMap::new();

    for child in [&first_child, &second_child] {
        walker.add_graph_child_or_pending(
            &mut graph_children,
            &parent,
            "child".into(),
            child.clone(),
        );
    }

    let first_dep_path = DepPath::from("child@1.0.0");
    walker.node_dep_paths.insert(first_child, first_dep_path.clone());
    walker.node_dep_paths.insert(second_child, DepPath::from("child@2.0.0"));
    walker.graph.insert(parent.clone(), graph_node(&parent));
    walker.patch_pending_peer_edges();

    assert_eq!(
        walker.graph[&parent].children.get("child"),
        Some(&first_dep_path),
        "`or_insert` leaves an already-filled slot alone",
    );
}

/// Every revisit of an already-realized node hands back the same map
/// rather than a copy of it. The walk revisits nodes millions of times
/// on a cyclic peer graph, and the map owns a `String` per child alias,
/// so cloning here is what the shared `Arc` exists to avoid.
#[test]
fn realized_children_are_shared_across_visits() {
    let parent = NodeId::next();
    let mut children = BTreeMap::new();
    children.insert("child".to_string(), NodeId::leaf("child@1.0.0"));

    let mut tree = ResolvedTree::default();
    tree.dependencies_tree.insert(parent.clone(), tree_node("parent@1.0.0", children, 0));
    let mut walker = walker_for_tests(&mut tree);

    let (first, first_undo) = walker.realize_children_with(&parent, None);
    let (second, second_undo) = walker.realize_children_with(&parent, None);

    assert!(
        std::sync::Arc::ptr_eq(&first, &second),
        "a revisit reuses the realized map instead of cloning it",
    );
    assert!(
        first_undo.is_none() && second_undo.is_none(),
        "an already-realized node realizes nothing, so there is nothing to undo",
    );
}

/// The peer walk realizes one occurrence node per distinct
/// root-to-package path — millions of them on a large graph — and each
/// names its package. Realization must hand the child edge's `Arc` to
/// the node rather than copy the id into it, so this asserts pointer
/// equality through `realize_children_with`, the path that creates
/// those millions, rather than through the constructor alone.
#[test]
fn realizing_children_shares_the_edge_package_id() {
    let parent = NodeId::next();
    let edge_id: Arc<str> = "child@1.0.0".into();

    let mut tree = ResolvedTree::default();
    tree.children_by_id.insert(
        "parent@1.0.0".into(),
        Arc::new(vec![crate::resolved_tree::ChildEdge {
            alias: "child".to_string(),
            pkg_id: Arc::<str>::clone(&edge_id),
            optional: false,
        }]),
    );
    tree.dependencies_tree.insert(
        parent.clone(),
        crate::resolved_tree::DependenciesTreeNode::new(
            "parent@1.0.0".into(),
            crate::resolved_tree::TreeChildren::Lazy { parent_ids: Arc::new(Vec::new()).into() },
            0,
            true,
        ),
    );

    let mut walker = walker_for_tests(&mut tree);
    let (children, _) = walker.realize_children_with(&parent, None);
    let child_node_id = children.get("child").expect("the edge is realized into a child node");
    let realized = &walker.tree.dependencies_tree[child_node_id];

    assert!(
        Arc::ptr_eq(&edge_id, &realized.resolved_package_id),
        "the occurrence points at the edge's id instead of owning a copy of it",
    );
}

/// The same shape without `p`: members whose canonical subtree reaches
/// no peer consumer merge to bare depPaths — identically in either walk
/// order.
#[test]
fn a_backedge_cut_member_merges_to_a_bare_dep_path() {
    let first_order = peer_cycle_graph_keys(
        &[("entry00", 0, "1.0.0"), ("entry01", 2, "2.0.0")],
        &order_test_shape(false),
    );
    let second_order = peer_cycle_graph_keys(
        &[("entry01", 2, "2.0.0"), ("entry00", 0, "1.0.0")],
        &order_test_shape(false),
    );
    assert!(
        first_order.iter().any(|key| key == "ring02@1.0.0"),
        "ring members merge to bare depPaths; got {first_order:#?}",
    );
    assert_eq!(first_order, second_order, "the graph must not depend on the entries' walk order");
}

/// Nearest-wins is untouched at walked positions: an importer-level
/// provider does not shadow a nearer entry-level one. Only the shared
/// back-edge occurrence — which has no position — binds the importer's
/// provider.
#[test]
fn an_importer_provider_does_not_shadow_a_nearer_entry_provider() {
    let shape = || PeerCycleShape {
        wc_members: vec![1],
        rings_peer_on_p: true,
        wc_w_range: "<3.0.0",
        importer_w_version: Some("9.9.9"),
        ..Default::default()
    };
    let first_order =
        peer_cycle_graph_keys(&[("entry00", 0, "1.0.0"), ("entry01", 2, "2.0.0")], &shape());
    let second_order =
        peer_cycle_graph_keys(&[("entry01", 2, "2.0.0"), ("entry00", 0, "1.0.0")], &shape());
    assert!(
        first_order.iter().any(|key| key == "ring01@1.0.0(p@1.0.0)(w@1.0.0)"),
        "a walked position binds its entry's nearer w; got {first_order:#?}",
    );
    assert!(
        first_order.iter().any(|key| key == "ring01@1.0.0(p@1.0.0)(w@9.9.9)"),
        "the positionless back-edge occurrence binds the importer's w; got {first_order:#?}",
    );
    assert_eq!(first_order, second_order, "the graph must not depend on the entries' walk order");
}

/// The regression behind the canonical cut's record-only back-edges: a
/// cut edge is still a real dependency, so the cycle-closing member's
/// graph node keeps its edge to the back-edge target.
#[test]
fn a_backedge_dependency_stays_in_the_graph() {
    let mut tree = peer_cycle_fixture(
        &[("entry00", 0, "1.0.0")],
        &PeerCycleShape { wc_members: vec![1], ..Default::default() },
    );
    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

    let (_, ring03) = result
        .graph
        .iter()
        .find(|(path, _)| path.as_str().starts_with("ring03@1.0.0"))
        .expect("ring03 is walked");
    let next = ring03.children.get("next").expect("the cut ring03 → ring00 edge is recorded");
    assert!(
        next.as_str().starts_with("ring00@1.0.0"),
        "the back-edge references a ring00 occurrence, got {next:?}",
    );
}

/// A member whose canonical subtree ends at the back-edge is genuinely
/// pure: the cut is the same at every occurrence, so its cached state
/// never mentions the peer consumers behind the back-edge
/// (pnpm/pnpm#13865).
#[test]
fn a_backedge_cut_subtree_is_pure() {
    let mut tree = peer_cycle_fixture(
        &[("entry00", 0, "1.0.0")],
        &PeerCycleShape { wc_members: vec![1], wc_w_range: "<3.0.0", ..Default::default() },
    );
    let direct = tree.direct.clone();
    let mut walker = crate::resolve_peers::test_support::walker_for_tests(&mut tree);
    let importer_parents = Arc::new(walker.build_importer_parents_from(&direct));
    let importer_parent_dep_paths = walker.parent_dep_paths_from_refs(&importer_parents);
    for dep in &direct {
        walker.resolve_node(
            &dep.node_id,
            &crate::resolve_peers::walker::NodeWalkContext {
                parent_refs: &importer_parents,
                parent_dep_paths: &importer_parent_dep_paths,
                chain_names: &crate::resolve_peers::context::SharedChain::default(),
                parent_node_ids: &crate::resolve_peers::context::SharedChain::default(),
                parent_pkg_ids: &crate::resolve_peers::context::SharedChain::default(),
            },
        );
    }

    assert!(
        walker.pure_pkgs.contains_key("ring02@1.0.0"),
        "ring02's canonical subtree reaches no peer consumer, so it is pure",
    );
    let cached_mentions_w = walker.peers_cache.get("ring02@1.0.0").is_some_and(|items| {
        items.iter().any(|item| {
            item.resolved_peers.contains_key("w") || item.missing_peers.contains_key("w")
        })
    });
    assert!(!cached_mentions_w, "no cached ring02 verdict mentions the consumer behind the cut");
}
