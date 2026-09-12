use super::{
    Arc, BTreeMap, DepPath, DirectDep, HashMap, HashSet, ImporterPeerInput, NodeId, PeerId,
    PkgResolutionId, ResolvePeersOptions, ResolvedTree, graph_node, linked_package, package,
    peer_id_pair, resolve_peers, resolve_peers_workspace, resolve_result, tree_node,
    walker_for_tests,
};

#[test]
fn workspace_importers_get_distinct_instances_for_different_peer_versions() {
    let peer_v1 = NodeId::leaf("peer@1.0.0");
    let peer_v2 = NodeId::leaf("peer@2.0.0");
    let consumer_v1 = NodeId::next();
    let consumer_v2 = NodeId::next();
    let importers = [
        ImporterPeerInput {
            id: "project-a".to_string(),
            direct: vec![
                DirectDep {
                    alias: "consumer".to_string(),
                    node_id: consumer_v1.clone(),
                    id: "consumer@1.0.0".to_string(),
                },
                DirectDep {
                    alias: "peer".to_string(),
                    node_id: peer_v1.clone(),
                    id: "peer@1.0.0".to_string(),
                },
            ],
            root_dir: std::path::PathBuf::from("/repo/project-a"),
            modules_dir: None,
        },
        ImporterPeerInput {
            id: "project-b".to_string(),
            direct: vec![
                DirectDep {
                    alias: "consumer".to_string(),
                    node_id: consumer_v2.clone(),
                    id: "consumer@1.0.0".to_string(),
                },
                DirectDep {
                    alias: "peer".to_string(),
                    node_id: peer_v2.clone(),
                    id: "peer@2.0.0".to_string(),
                },
            ],
            root_dir: std::path::PathBuf::from("/repo/project-b"),
            modules_dir: None,
        },
    ];
    let mut tree = ResolvedTree {
        direct: Vec::new(),
        packages: HashMap::from_iter([
            ("peer@1.0.0".into(), package("peer", "1.0.0", &[], true)),
            ("peer@2.0.0".into(), package("peer", "2.0.0", &[], true)),
            ("consumer@1.0.0".into(), package("consumer", "1.0.0", &[("peer", "*")], false)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (peer_v1, tree_node("peer@1.0.0", BTreeMap::new(), 0)),
            (peer_v2, tree_node("peer@2.0.0", BTreeMap::new(), 0)),
            (consumer_v1, tree_node("consumer@1.0.0", BTreeMap::new(), 0)),
            (consumer_v2, tree_node("consumer@1.0.0", BTreeMap::new(), 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["peer".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers_workspace(
        &mut tree,
        &importers,
        std::path::Path::new("/repo"),
        false,
        false,
        false,
        ResolvePeersOptions::default(),
    );

    assert_eq!(
        result.direct_dependencies_by_importer["project-a"]["consumer"],
        DepPath::from("consumer@1.0.0(peer@1.0.0)"),
    );
    assert_eq!(
        result.direct_dependencies_by_importer["project-b"]["consumer"],
        DepPath::from("consumer@1.0.0(peer@2.0.0)"),
    );
}

#[test]
fn a_shared_consumer_keeps_the_first_importers_peer_provider_variant() {
    let plugin_v1 = NodeId::next();
    let plugin_v2 = NodeId::next();
    let utils_root = NodeId::next();
    let utils_app = NodeId::next();
    let resolver_root = NodeId::next();
    let resolver_app = NodeId::next();
    let parser = NodeId::leaf("parser@1.0.0");

    let importers = [
        ImporterPeerInput {
            id: ".".to_string(),
            direct: vec![
                DirectDep {
                    alias: "plugin".to_string(),
                    node_id: plugin_v1.clone(),
                    id: "plugin@1.0.0".to_string(),
                },
                DirectDep {
                    alias: "parser".to_string(),
                    node_id: parser.clone(),
                    id: "parser@1.0.0".to_string(),
                },
                DirectDep {
                    alias: "resolver".to_string(),
                    node_id: resolver_root.clone(),
                    id: "resolver@1.0.0".to_string(),
                },
            ],
            root_dir: std::path::PathBuf::from("/repo"),
            modules_dir: None,
        },
        ImporterPeerInput {
            id: "app".to_string(),
            direct: vec![
                DirectDep {
                    alias: "plugin".to_string(),
                    node_id: plugin_v2.clone(),
                    id: "plugin@2.0.0".to_string(),
                },
                DirectDep {
                    alias: "resolver".to_string(),
                    node_id: resolver_app.clone(),
                    id: "resolver@1.0.0".to_string(),
                },
            ],
            root_dir: std::path::PathBuf::from("/repo/app"),
            modules_dir: None,
        },
    ];

    let mut tree = ResolvedTree {
        direct: Vec::new(),
        packages: HashMap::from_iter([
            ("plugin@1.0.0".into(), package("plugin", "1.0.0", &[("parser", "*")], false)),
            ("plugin@2.0.0".into(), package("plugin", "2.0.0", &[("parser", "*")], false)),
            (
                Arc::from("utils@1.0.0".to_string()),
                package("utils", "1.0.0", &[("resolver", "*"), ("parser", "*")], false),
            ),
            ("resolver@1.0.0".into(), package("resolver", "1.0.0", &[("plugin", "*")], false)),
            ("parser@1.0.0".into(), package("parser", "1.0.0", &[], true)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (
                plugin_v1,
                tree_node(
                    "plugin@1.0.0",
                    BTreeMap::from([("utils".to_string(), utils_root.clone())]),
                    0,
                ),
            ),
            (
                plugin_v2,
                tree_node(
                    "plugin@2.0.0",
                    BTreeMap::from([("utils".to_string(), utils_app.clone())]),
                    0,
                ),
            ),
            (utils_root, tree_node("utils@1.0.0", BTreeMap::new(), 1)),
            (utils_app, tree_node("utils@1.0.0", BTreeMap::new(), 1)),
            (resolver_root, tree_node("resolver@1.0.0", BTreeMap::new(), 0)),
            (resolver_app, tree_node("resolver@1.0.0", BTreeMap::new(), 0)),
            (parser, tree_node("parser@1.0.0", BTreeMap::new(), 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter([
            "plugin".into(),
            "resolver".into(),
            "parser".into(),
        ]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
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

    // Both `utils` occurrences collapse onto one depPath because the
    // `resolver` peer id collapses on the plugin/resolver peer cycle, so
    // exactly one of them supplies the graph node's edges.
    let utils = result.graph.keys().filter(|dep_path| dep_path.as_str().starts_with("utils@"));
    assert_eq!(utils.count(), 1, "one utils entry: {:?}", result.graph.keys().collect::<Vec<_>>());
    let utils_dep_path = result
        .graph
        .keys()
        .find(|dep_path| dep_path.as_str().starts_with("utils@"))
        .expect("utils entry")
        .clone();
    assert_eq!(
        result.graph[&utils_dep_path].children.get("resolver"),
        Some(&DepPath::from("resolver@1.0.0(plugin@1.0.0)")),
    );
    // Trimming a peer segment off the edge would key it to a variant no
    // importer reaches, leaving an orphan entry in the lockfile —
    // <https://github.com/pnpm/pnpm/issues/13320>.
    let mut reachable: HashSet<DepPath> = HashSet::default();
    let mut queue: Vec<DepPath> = result
        .direct_dependencies_by_importer
        .values()
        .flat_map(|direct| direct.values().cloned())
        .collect();
    while let Some(dep_path) = queue.pop() {
        if !reachable.insert(dep_path.clone()) {
            continue;
        }
        queue.extend(result.graph[&dep_path].children.values().cloned());
    }
    let orphans: Vec<_> =
        result.graph.keys().filter(|dep_path| !reachable.contains(*dep_path)).collect();
    assert!(orphans.is_empty(), "every graph entry is reachable from an importer: {orphans:?}");
}

#[test]
fn linked_peer_provider_uses_root_relative_snapshot_ref_in_workspace_fallback() {
    let peer = NodeId::leaf("link:packages/peer");
    let consumer = NodeId::next();
    let importer = ImporterPeerInput {
        id: "apps/nested/app".to_string(),
        direct: vec![
            DirectDep {
                alias: "consumer".to_string(),
                node_id: consumer.clone(),
                id: "consumer@1.0.0".to_string(),
            },
            DirectDep {
                alias: "peer".to_string(),
                node_id: peer.clone(),
                id: "link:packages/peer".to_string(),
            },
        ],
        root_dir: std::path::PathBuf::from("/repo/apps/nested/app"),
        modules_dir: None,
    };
    let mut tree = ResolvedTree {
        direct: Vec::new(),
        packages: HashMap::from_iter([
            (
                "link:packages/peer".into(),
                linked_package("peer", "link:packages/peer", "packages/peer"),
            ),
            ("consumer@1.0.0".into(), package("consumer", "1.0.0", &[("peer", "*")], false)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (peer.clone(), tree_node("link:packages/peer", BTreeMap::new(), -1)),
            (consumer, tree_node("consumer@1.0.0", BTreeMap::new(), 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["peer".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers_workspace(
        &mut tree,
        &[importer],
        std::path::Path::new("/repo"),
        false,
        false,
        false,
        ResolvePeersOptions {
            lockfile_dir: Some(std::path::PathBuf::from("/repo")),
            hoisted_peer_provider_node_ids: HashSet::from_iter([peer]),
            ..ResolvePeersOptions::default()
        },
    );

    assert_eq!(
        result.direct_dependencies_by_importer["apps/nested/app"]["peer"].as_str(),
        "link:../../../packages/peer",
    );
    let consumer = result
        .graph
        .values()
        .find(|node| node.resolved_package_id == "consumer@1.0.0")
        .expect("consumer graph node");
    assert_eq!(consumer.children.get("peer"), Some(&DepPath::from("link:packages/peer")));
}

/// `excludeLinksFromLockfile` only remaps links that point outside the
/// workspace. Each importer's own root has to reach the remap for that
/// to hold in the multi-importer walk, since a workspace link's target
/// is recorded relative to the importer that declares it.
#[test]
fn workspace_internal_link_peer_keeps_its_node_id_when_exclude_links_on() {
    let peer = NodeId::leaf("link:packages/peer");
    let consumer = NodeId::next();
    let importer = ImporterPeerInput {
        id: "apps/app".to_string(),
        direct: vec![
            DirectDep {
                alias: "consumer".to_string(),
                node_id: consumer.clone(),
                id: "consumer@1.0.0".to_string(),
            },
            DirectDep {
                alias: "peer".to_string(),
                node_id: peer.clone(),
                id: "link:packages/peer".to_string(),
            },
        ],
        root_dir: std::path::PathBuf::from("/repo/apps/app"),
        modules_dir: Some(std::path::PathBuf::from("/repo/apps/app/node_modules")),
    };
    let mut tree = ResolvedTree {
        direct: Vec::new(),
        packages: HashMap::from_iter([
            (
                "link:packages/peer".into(),
                linked_package("peer", "link:packages/peer", "../../packages/peer"),
            ),
            ("consumer@1.0.0".into(), package("consumer", "1.0.0", &[("peer", "*")], false)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (peer, tree_node("link:packages/peer", BTreeMap::new(), -1)),
            (consumer, tree_node("consumer@1.0.0", BTreeMap::new(), 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["peer".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers_workspace(
        &mut tree,
        &[importer],
        std::path::Path::new("/repo"),
        false,
        false,
        false,
        ResolvePeersOptions {
            exclude_links_from_lockfile: true,
            lockfile_dir: Some(std::path::PathBuf::from("/repo")),
            ..ResolvePeersOptions::default()
        },
    );

    let consumer_dep_path = &result.direct_dependencies_by_importer["apps/app"]["consumer"];
    assert_eq!(consumer_dep_path.as_str(), "consumer@1.0.0(peer@packages+peer)");
    assert_eq!(
        result.graph[consumer_dep_path].children.get("peer"),
        Some(&DepPath::from("link:packages/peer")),
    );
}

/// Mirror of the TS test "pruned hoisted peer providers that peer-depend on
/// each other are resolved together" (`deps-resolver/test/resolvePeers.ts`):
/// two pruned providers form a peer cycle, so each one's suffix depends on
/// the other's. Both must come out of the fallback with the cycle collapsed
/// to `name@version`, matching the in-place cycle handling.
#[test]
fn pruned_hoisted_providers_with_mutual_peers_resolve() {
    let lib_a = NodeId::leaf("lib-a@1.0.0");
    let lib_b = NodeId::leaf("lib-b@1.0.0");
    let consumer = NodeId::next();

    let mut tree = ResolvedTree {
        direct: vec![
            DirectDep {
                alias: "consumer".to_string(),
                node_id: consumer.clone(),
                id: "consumer@1.0.0".to_string(),
            },
            DirectDep {
                alias: "lib-a".to_string(),
                node_id: lib_a.clone(),
                id: "lib-a@1.0.0".to_string(),
            },
            DirectDep {
                alias: "lib-b".to_string(),
                node_id: lib_b.clone(),
                id: "lib-b@1.0.0".to_string(),
            },
        ],
        packages: HashMap::from_iter([
            ("lib-a@1.0.0".into(), package("lib-a", "1.0.0", &[("lib-b", "^1.0.0")], true)),
            ("lib-b@1.0.0".into(), package("lib-b", "1.0.0", &[("lib-a", "^1.0.0")], true)),
            (
                Arc::from("consumer@1.0.0".to_string()),
                package("consumer", "1.0.0", &[("lib-a", "^1.0.0"), ("lib-b", "^1.0.0")], false),
            ),
        ]),
        dependencies_tree: HashMap::from_iter([
            (lib_a.clone(), tree_node("lib-a@1.0.0", BTreeMap::new(), 1)),
            (lib_b.clone(), tree_node("lib-b@1.0.0", BTreeMap::new(), 1)),
            (consumer, tree_node("consumer@1.0.0", BTreeMap::new(), 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["lib-a".to_string(), "lib-b".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers(
        &mut tree,
        ResolvePeersOptions {
            hoisted_peer_provider_node_ids: HashSet::from_iter([lib_a, lib_b]),
            ..ResolvePeersOptions::default()
        },
    );

    assert_eq!(
        result.direct_dependencies_by_alias.get("lib-a"),
        Some(&DepPath::from("lib-a@1.0.0(lib-b@1.0.0)")),
        "graph keys: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
    assert_eq!(
        result.direct_dependencies_by_alias.get("lib-b"),
        Some(&DepPath::from("lib-b@1.0.0(lib-a@1.0.0)")),
        "graph keys: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
    assert!(
        result.graph.contains_key(&DepPath::from("consumer@1.0.0(lib-a@1.0.0)(lib-b@1.0.0)")),
        "the consumer must bind both fallback-resolved providers: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
}

/// Mirror of the TS test "an own direct dependency and a pruned hoisted peer
/// provider that peer-depend on each other are resolved together"
/// (`deps-resolver/test/resolvePeers.ts`) — the shape behind
/// <https://github.com/pnpm/pnpm/issues/12921>, where the peer cycle spans an
/// own direct dependency and a pruned provider. Both sides of the cycle must
/// collapse to `name@version` suffixes.
#[test]
fn own_direct_dep_and_pruned_provider_with_mutual_peers_resolve() {
    let plugin = NodeId::leaf("plugin@1.0.0");
    let main = NodeId::next();

    let mut tree = ResolvedTree {
        direct: vec![
            DirectDep {
                alias: "main".to_string(),
                node_id: main.clone(),
                id: "main@1.0.0".to_string(),
            },
            DirectDep {
                alias: "plugin".to_string(),
                node_id: plugin.clone(),
                id: "plugin@1.0.0".to_string(),
            },
        ],
        packages: HashMap::from_iter([
            ("main@1.0.0".into(), package("main", "1.0.0", &[("plugin", "^1.0.0")], false)),
            ("plugin@1.0.0".into(), package("plugin", "1.0.0", &[("main", "^1.0.0")], true)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (main, tree_node("main@1.0.0", BTreeMap::new(), 0)),
            (plugin.clone(), tree_node("plugin@1.0.0", BTreeMap::new(), 1)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["main".to_string(), "plugin".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers(
        &mut tree,
        ResolvePeersOptions {
            hoisted_peer_provider_node_ids: HashSet::from_iter([plugin]),
            ..ResolvePeersOptions::default()
        },
    );

    assert_eq!(
        result.direct_dependencies_by_alias.get("main"),
        Some(&DepPath::from("main@1.0.0(plugin@1.0.0)")),
        "graph keys: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
    assert_eq!(
        result.direct_dependencies_by_alias.get("plugin"),
        Some(&DepPath::from("plugin@1.0.0(main@1.0.0)")),
        "graph keys: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
}

/// Mirror of the TS test "a peer cycle between an own direct dependency and a
/// hoisted peer provider resolved at its tree position does not deadlock":
/// the provider is walked at its true position inside host's subtree, so the
/// peer cycle spans two traversal levels instead of two root-level passes.
#[test]
fn peer_cycle_between_own_dep_and_provider_at_tree_position_resolves() {
    let host = NodeId::next();
    let main = NodeId::next();
    let plugin = NodeId::next();

    let mut tree = ResolvedTree {
        direct: vec![
            DirectDep {
                alias: "host".to_string(),
                node_id: host.clone(),
                id: "host@1.0.0".to_string(),
            },
            DirectDep {
                alias: "main".to_string(),
                node_id: main.clone(),
                id: "main@1.0.0".to_string(),
            },
            DirectDep {
                alias: "plugin".to_string(),
                node_id: plugin.clone(),
                id: "plugin@1.0.0".to_string(),
            },
        ],
        packages: HashMap::from_iter([
            ("host@1.0.0".into(), package("host", "1.0.0", &[], false)),
            ("main@1.0.0".into(), package("main", "1.0.0", &[("plugin", "^1.0.0")], false)),
            ("plugin@1.0.0".into(), package("plugin", "1.0.0", &[("main", "^1.0.0")], false)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (
                host,
                tree_node(
                    "host@1.0.0",
                    BTreeMap::from([("plugin".to_string(), plugin.clone())]),
                    0,
                ),
            ),
            (main, tree_node("main@1.0.0", BTreeMap::new(), 0)),
            (plugin.clone(), tree_node("plugin@1.0.0", BTreeMap::new(), 1)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["main".to_string(), "plugin".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers(
        &mut tree,
        ResolvePeersOptions {
            hoisted_peer_provider_node_ids: HashSet::from_iter([plugin]),
            ..ResolvePeersOptions::default()
        },
    );

    assert_eq!(
        result.direct_dependencies_by_alias.get("host"),
        Some(&DepPath::from("host@1.0.0(main@1.0.0)")),
        "graph keys: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
    assert_eq!(
        result.direct_dependencies_by_alias.get("main"),
        Some(&DepPath::from("main@1.0.0(plugin@1.0.0)")),
        "graph keys: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
    assert_eq!(
        result.direct_dependencies_by_alias.get("plugin"),
        Some(&DepPath::from("plugin@1.0.0(main@1.0.0)")),
        "graph keys: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
}

#[test]
fn peer_id_pair_keeps_the_named_registry() {
    let mut result = resolve_result("foo", "1.0.0");
    result.id = PkgResolutionId::from("foo@work:1.0.0".to_string());
    result.resolved_via = "named-registry".to_string();

    let PeerId::Pair { name, version } = peer_id_pair(&result) else {
        panic!("expected a name/version pair");
    };
    assert_eq!(name, "foo");
    assert_eq!(version, "work:1.0.0");
}

#[test]
fn peer_id_pair_leaves_an_ordinary_registry_package_bare() {
    let PeerId::Pair { name, version } = peer_id_pair(&resolve_result("foo", "1.0.0")) else {
        panic!("expected a name/version pair");
    };
    assert_eq!(name, "foo");
    assert_eq!(version, "1.0.0");
}

/// A parent reached through many occurrences enqueues the identical
/// `(parent_dep_path, alias, child_node_id)` triple over and over —
/// millions of times on a cyclic peer graph. Replaying a repeat is a
/// no-op, because it resolves the same `DepPath` and `or_insert`s a
/// slot that is already filled, so the buffer keeps only the first.
/// Distinct triples must still all be kept: dedup is by whole triple,
/// never by `(parent, alias)` slot, or the first-resolvable-wins
/// behaviour of `patch_pending_peer_edges` would change.
#[test]
fn repeated_pending_peer_edges_are_buffered_once() {
    let mut tree = ResolvedTree::default();
    let mut walker = walker_for_tests(&mut tree);

    let parent = DepPath::from("parent@1.0.0");
    let first_child = NodeId::next();
    let second_child = NodeId::next();
    let mut graph_children = BTreeMap::new();

    for _ in 0..3 {
        walker.add_graph_child_or_pending(
            &mut graph_children,
            &parent,
            "child".into(),
            first_child.clone(),
        );
    }

    assert!(graph_children.is_empty(), "the child has no depPath yet, so nothing is a graph edge");
    assert_eq!(walker.pending_peer_edges.len(), 1, "the same triple is buffered once");

    // Same slot, different child: a distinct triple, so it is kept.
    walker.add_graph_child_or_pending(
        &mut graph_children,
        &parent,
        "child".into(),
        second_child.clone(),
    );
    assert_eq!(walker.pending_peer_edges.len(), 2, "dedup is by triple, not by (parent, alias)");

    // What the buffer holds only matters through the graph it patches.
    // Leave the first child unresolved: `patch_pending_peer_edges` is
    // first-*resolvable*-wins, so the edge must come from the second
    // triple. Deduplicating by `(parent, alias)` instead of by whole
    // triple would have dropped that triple and left no edge at all.
    let second_dep_path = DepPath::from("child@2.0.0");
    walker.node_dep_paths.insert(second_child, second_dep_path.clone());
    walker.graph.insert(parent.clone(), graph_node(&parent));
    walker.patch_pending_peer_edges();

    assert_eq!(
        walker.graph[&parent].children.get("child"),
        Some(&second_dep_path),
        "an unresolvable first triple yields to the next one for the same slot",
    );
}

/// The membership guard lives and dies with the buffer: once the edges
/// are drained into the graph, a triple enqueued again describes a
/// graph that has moved on and must be replayed.
#[test]
fn pending_peer_edges_replay_after_a_drain() {
    let mut tree = ResolvedTree::default();
    let mut walker = walker_for_tests(&mut tree);

    let parent = DepPath::from("parent@1.0.0");
    let child = NodeId::next();
    let mut graph_children = BTreeMap::new();

    walker.add_graph_child_or_pending(&mut graph_children, &parent, "child".into(), child.clone());
    walker.patch_pending_peer_edges();
    assert!(walker.pending_peer_edges.is_empty(), "the drain empties the buffer");

    walker.add_graph_child_or_pending(&mut graph_children, &parent, "child".to_string(), child);
    assert_eq!(walker.pending_peer_edges.len(), 1, "the guard cleared with the buffer");
}
