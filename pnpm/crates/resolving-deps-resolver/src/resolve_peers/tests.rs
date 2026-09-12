mod resolution_order;

mod workspace_links;

mod overrides;

mod workspace_peer_contexts;

mod peer_dependencies_own_peer_is_resolved;

mod behavior;

use super::{
    ImporterPeerInput, ResolvePeersOptions, ResolvePeersResult,
    context::peer_id_pair,
    resolve_peers, resolve_peers_workspace,
    test_support::{
        linked_package, package, package_with_peer_dependencies, resolve_result, tree_node,
        walker_for_tests,
    },
};
use crate::{
    node_id::NodeId,
    resolved_tree::{DirectDep, ResolvedTree},
};
use pnpm_deps_path::{DepPath, PeerId};
use pnpm_resolving_resolver_base::PkgResolutionId;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{collections::BTreeMap, sync::Arc};

/// A tree with a `consumer` whose peer on `types@1.0.0` is declared with the
/// given named-registry specifier. Returns the tree and the expected dep path.
fn named_registry_peer_tree(peer_spec: &str) -> (ResolvedTree, DepPath) {
    let types = NodeId::leaf("types@1.0.0");
    let consumer = NodeId::next();

    let mut consumer_children = BTreeMap::new();
    consumer_children.insert("types".to_string(), types.clone());

    let tree = ResolvedTree {
        direct: vec![DirectDep {
            alias: "consumer".to_string(),
            node_id: consumer.clone(),
            id: "consumer@1.0.0".to_string(),
        }],
        packages: HashMap::from_iter([
            ("types@1.0.0".into(), package("types", "1.0.0", &[], true)),
            (
                Arc::from("consumer@1.0.0".to_string()),
                package_with_peer_dependencies(
                    "consumer",
                    "1.0.0",
                    &[("types", peer_spec, false)],
                    false,
                ),
            ),
        ]),
        dependencies_tree: HashMap::from_iter([
            (types, tree_node("types@1.0.0", BTreeMap::new(), 1)),
            (consumer, tree_node("consumer@1.0.0", consumer_children, 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["types".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    (tree, DepPath::from("consumer@1.0.0(types@1.0.0)"))
}

/// The cyclic aliased peer graph of pnpm/pnpm#14449: `vite` and the
/// `core` nested under `vite-plus` are two occurrences of one package,
/// so whichever is walked second reuses the first one's peer-cache
/// verdict, and `@vitejs/devtools` closes the peer cycle. The direct
/// dependency order decides which occurrence becomes the cache owner.
fn cyclic_alias_peer_tree(direct_aliases: [&str; 3]) -> ResolvedTree {
    let core_direct = NodeId::next();
    let core_nested = NodeId::next();
    let devtools = NodeId::next();
    let vite_plus = NodeId::next();

    let mut vite_plus_children = BTreeMap::new();
    vite_plus_children.insert("core".to_string(), core_nested.clone());

    let direct = direct_aliases
        .into_iter()
        .map(|alias| {
            let (node_id, id) = match alias {
                "@vitejs/devtools" => (&devtools, "@vitejs/devtools@1.0.0"),
                "vite" => (&core_direct, "core@1.0.0"),
                "vite-plus" => (&vite_plus, "vite-plus@1.0.0"),
                _ => unreachable!("unknown direct dependency alias {alias}"),
            };
            DirectDep { alias: alias.to_string(), node_id: node_id.clone(), id: id.to_string() }
        })
        .collect();

    ResolvedTree {
        direct,
        packages: HashMap::from_iter([
            (
                Arc::from("core@1.0.0".to_string()),
                package_with_peer_dependencies(
                    "core",
                    "1.0.0",
                    &[("@vitejs/devtools", "*", true)],
                    false,
                ),
            ),
            (
                Arc::from("@vitejs/devtools@1.0.0".to_string()),
                package("@vitejs/devtools", "1.0.0", &[("vite", "*")], false),
            ),
            ("vite-plus@1.0.0".into(), package("vite-plus", "1.0.0", &[], false)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (core_direct, tree_node("core@1.0.0", BTreeMap::new(), 0)),
            (core_nested, tree_node("core@1.0.0", BTreeMap::new(), 1)),
            (devtools, tree_node("@vitejs/devtools@1.0.0", BTreeMap::new(), 0)),
            (vite_plus, tree_node("vite-plus@1.0.0", vite_plus_children, 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter([
            "@vitejs/devtools".to_string(),
            "vite".to_string(),
        ]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    }
}

/// Both `core` occurrences must share the cycle-collapsed depPath, and
/// every edge of the result must point at an emitted graph node.
fn assert_cyclic_alias_peer_graph_is_closed(result: &ResolvePeersResult) {
    let vite_core = &result.direct_dependencies_by_alias["vite"];
    let vite_plus_path = &result.direct_dependencies_by_alias["vite-plus"];
    let nested_core = &result.graph[vite_plus_path].children["core"];

    assert_eq!(nested_core, vite_core);
    assert_eq!(vite_core, &DepPath::from("core@1.0.0(@vitejs/devtools@1.0.0)"));
    assert_eq!(
        result.direct_dependencies_by_alias["@vitejs/devtools"],
        DepPath::from("@vitejs/devtools@1.0.0(core@1.0.0)"),
    );
    for node in result.graph.values() {
        for (alias, child) in &node.children {
            assert!(
                result.graph.contains_key(child),
                "edge {alias} of {} points at a missing graph node {child}: {:#?}",
                node.dep_path,
                result.graph,
            );
        }
    }
}

/// Ported from upstream `resolvePeers.ts`'s `locked peer provider
/// preferences` suite: a second resolution pass receives the first
/// pass's `paths_by_node_id` and re-pins compatible locked providers.
mod locked_peer_provider_preferences {
    use super::{DepPath, DirectDep, NodeId, ResolvePeersOptions, ResolvedTree, resolve_peers};
    use crate::resolve_peers::test_support::{package, package_with_peer_dependencies, tree_node};
    use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
    use std::{collections::BTreeMap, sync::Arc};

    struct LockedTreeIds {
        current_peer: NodeId,
        retained_peer: NodeId,
        retainer: NodeId,
        wrapper: NodeId,
        consumer: NodeId,
    }

    fn ids() -> LockedTreeIds {
        LockedTreeIds {
            current_peer: NodeId::leaf("peer@1.0.0"),
            retained_peer: NodeId::leaf("peer@2.0.0"),
            retainer: NodeId::next(),
            wrapper: NodeId::next(),
            consumer: NodeId::next(),
        }
    }

    /// Mirror of upstream `createTree` (`resolvePeers.ts:814`): the
    /// importer directly depends on `peer@1.0.0` (the current
    /// provider), `retainer` (which keeps `peer@2.0.0` reachable), and
    /// `wrapper`, whose child `consumer` carries the locked context
    /// binding `peer` to `peer@2.0.0`.
    fn locked_provider_tree(ids: &LockedTreeIds, peer_range: &str) -> ResolvedTree {
        let mut current_peer_node = tree_node("peer@1.0.0", BTreeMap::new(), 0);
        current_peer_node.locked_mut().previous_dep_path = Some(DepPath::from("peer@1.0.0"));
        let mut retained_peer_node = tree_node("peer@2.0.0", BTreeMap::new(), 1);
        retained_peer_node.locked_mut().previous_dep_path = Some(DepPath::from("peer@2.0.0"));
        let mut consumer_node = tree_node("consumer@1.0.0", BTreeMap::new(), 1);
        consumer_node.locked_mut().locked_peer_context =
            Some(BTreeMap::from([("peer".to_string(), DepPath::from("peer@2.0.0"))]));
        ResolvedTree {
            direct: vec![
                DirectDep {
                    alias: "peer".to_string(),
                    node_id: ids.current_peer.clone(),
                    id: "peer@1.0.0".to_string(),
                },
                DirectDep {
                    alias: "retainer".to_string(),
                    node_id: ids.retainer.clone(),
                    id: "retainer@1.0.0".to_string(),
                },
                DirectDep {
                    alias: "wrapper".to_string(),
                    node_id: ids.wrapper.clone(),
                    id: "wrapper@1.0.0".to_string(),
                },
            ],
            packages: HashMap::from_iter([
                ("peer@1.0.0".into(), package("peer", "1.0.0", &[], true)),
                ("peer@2.0.0".into(), package("peer", "2.0.0", &[], true)),
                ("retainer@1.0.0".into(), package("retainer", "1.0.0", &[], false)),
                ("wrapper@1.0.0".into(), package("wrapper", "1.0.0", &[], false)),
                (
                    Arc::from("consumer@1.0.0".to_string()),
                    package_with_peer_dependencies(
                        "consumer",
                        "1.0.0",
                        &[("peer", peer_range, false)],
                        false,
                    ),
                ),
            ]),
            dependencies_tree: HashMap::from_iter([
                (ids.current_peer.clone(), current_peer_node),
                (ids.retained_peer.clone(), retained_peer_node),
                (
                    ids.retainer.clone(),
                    tree_node(
                        "retainer@1.0.0",
                        BTreeMap::from([("peer".to_string(), ids.retained_peer.clone())]),
                        0,
                    ),
                ),
                (
                    ids.wrapper.clone(),
                    tree_node(
                        "wrapper@1.0.0",
                        BTreeMap::from([("consumer".to_string(), ids.consumer.clone())]),
                        0,
                    ),
                ),
                (ids.consumer.clone(), consumer_node),
            ]),
            all_peer_dep_names: HashSet::from_iter(["peer".to_string()]),
            policy_violations: Vec::new(),
            applied_patches: HashSet::default(),
            children_by_id: HashMap::default(),
        }
    }

    /// TS: `prefers a compatible locked provider that remains reachable
    /// in the current graph` (`resolvePeers.ts:890`).
    #[test]
    fn compatible_locked_peer_provider_is_reused() {
        let ids = ids();
        let mut tree = locked_provider_tree(&ids, ">=1");
        let initial = resolve_peers(
            &mut tree,
            ResolvePeersOptions {
                collect_paths_by_node_id: true,
                ..ResolvePeersOptions::default()
            },
        );
        assert!(
            initial.graph.contains_key(&DepPath::from("consumer@1.0.0(peer@1.0.0)")),
            "the first pass binds the current provider; graph keys: {:#?}",
            initial.graph.keys().collect::<Vec<_>>(),
        );

        let preferred = resolve_peers(
            &mut tree,
            ResolvePeersOptions {
                resolved_peer_provider_paths: Some(initial.paths_by_node_id),
                ..ResolvePeersOptions::default()
            },
        );
        assert!(
            preferred.graph.contains_key(&DepPath::from("consumer@1.0.0(peer@2.0.0)")),
            "the second pass re-pins the locked provider; graph keys: {:#?}",
            preferred.graph.keys().collect::<Vec<_>>(),
        );
    }

    /// TS: `does not reuse a locked provider outside the current peer
    /// range` (`resolvePeers.ts:1100`).
    #[test]
    fn locked_peer_provider_outside_the_current_range_is_not_reused() {
        let ids = ids();
        let mut tree = locked_provider_tree(&ids, "^1.0.0");
        let initial = resolve_peers(
            &mut tree,
            ResolvePeersOptions {
                collect_paths_by_node_id: true,
                ..ResolvePeersOptions::default()
            },
        );

        let preferred = resolve_peers(
            &mut tree,
            ResolvePeersOptions {
                resolved_peer_provider_paths: Some(initial.paths_by_node_id),
                ..ResolvePeersOptions::default()
            },
        );
        assert!(
            preferred.graph.contains_key(&DepPath::from("consumer@1.0.0(peer@1.0.0)")),
            "the current in-range provider stays bound; graph keys: {:#?}",
            preferred.graph.keys().collect::<Vec<_>>(),
        );
        assert!(
            !preferred.graph.contains_key(&DepPath::from("consumer@1.0.0(peer@2.0.0)")),
            "the out-of-range locked provider must not be re-pinned; graph keys: {:#?}",
            preferred.graph.keys().collect::<Vec<_>>(),
        );
    }
}

/// A graph node carrying nothing but its identity — enough for the
/// pending-edge tests, which only read and write `children`.
fn graph_node(dep_path: &DepPath) -> crate::dependencies_graph::DependenciesGraphNode {
    crate::dependencies_graph::DependenciesGraphNode {
        dep_path: dep_path.clone(),
        resolved_package_id: dep_path.to_string(),
        resolve_result: std::sync::Arc::new(resolve_result("parent", "1.0.0")),
        children: BTreeMap::new(),
        optional_children: HashSet::default(),
        peer_dependencies: BTreeMap::new(),
        transitive_peer_dependencies: HashSet::default(),
        resolved_peer_names: HashSet::default(),
        depth: 0,
        installable: true,
        is_pure: true,
        optional: false,
    }
}

/// See [`fn@peer_cycle_fixture`].
struct PeerCycleShape {
    ring_len: usize,
    /// Adds `skip` edges (`ringN → ringN+2`), `home` edges back to
    /// `ring00`, and peerless fanout under the even members.
    with_skips: bool,
    /// Ring members that depend on the `wc` consumer of peer `w`.
    wc_members: Vec<usize>,
    /// Whether every ring member peers on `p` (provided at the top).
    rings_peer_on_p: bool,
    /// The range `wc` declares for its peer `w`.
    wc_w_range: &'static str,
    /// A `w` version provided as an importer-level direct dep.
    importer_w_version: Option<&'static str>,
}

impl Default for PeerCycleShape {
    fn default() -> Self {
        PeerCycleShape {
            ring_len: 4,
            with_skips: false,
            wc_members: Vec::new(),
            rings_peer_on_p: false,
            wc_w_range: "*",
            importer_w_version: None,
        }
    }
}
/// A ring of `ring_len` packages (`ring00 → ring01 → … → ring00`), with
/// a consumer of peer `w` hanging off the `wc_members`. Each entry in
/// `entries` is a direct dep
/// `(alias, ring index it points at, its own w version)` — the
/// per-entry `w` keeps one entry's untruncated ring verdicts from
/// plainly matching another's, which is what forces the walk onto the
/// cycle-verdict cache. Everything realizes lazily from
/// `children_by_id`, the way production trees reach the peer walk.
///
/// The shape matters: `ring00` re-entered through the full lap has its
/// `ring01` edge cut — no `w` in that subtree — while `ring00` reached
/// under a `ring02` entry keeps `ring01` and resolves `w`. Same
/// package, same `p` context, different truncation, different verdict:
/// the pair a cycle-verdict cache must never merge.
fn peer_cycle_fixture(entries: &[(&str, usize, &str)], shape: &PeerCycleShape) -> ResolvedTree {
    let ring_id = |index: usize| ring_pkg_id(index, shape.ring_len);

    let mut packages = HashMap::default();
    let mut children_by_id: HashMap<Arc<str>, Arc<Vec<crate::resolved_tree::ChildEdge>>> =
        HashMap::default();
    let ring_peers: &[(&str, &str)] = if shape.rings_peer_on_p { &[("p", "*")] } else { &[] };
    for index in 0..shape.ring_len {
        let name = format!("ring{index:02}");
        packages.insert(Arc::from(ring_id(index)), package(&name, "1.0.0", ring_peers, false));
        let edges = ring_member_edges(index, shape, &mut packages, &mut children_by_id);
        children_by_id.insert(Arc::from(ring_id(index)), Arc::new(edges));
    }
    packages
        .insert(Arc::from("wc@1.0.0"), package("wc", "1.0.0", &[("w", shape.wc_w_range)], false));
    packages.insert(Arc::from("p@1.0.0"), package("p", "1.0.0", &[], true));

    let mut dependencies_tree = HashMap::default();
    let mut direct = Vec::new();
    let add_direct = |id: &str,
                      alias: &str,
                      dependencies_tree: &mut HashMap<NodeId, _>,
                      direct: &mut Vec<DirectDep>| {
        let node_id = NodeId::next();
        dependencies_tree.insert(
            node_id.clone(),
            crate::resolved_tree::DependenciesTreeNode::new(
                Arc::from(id),
                crate::resolved_tree::TreeChildren::Lazy {
                    parent_ids: Arc::new(Vec::new()).into(),
                },
                0,
                true,
            ),
        );
        direct.push(DirectDep { alias: alias.to_string(), node_id, id: id.to_string() });
    };
    add_direct("p@1.0.0", "p", &mut dependencies_tree, &mut direct);
    if let Some(w_version) = shape.importer_w_version {
        let w_pkg = format!("w@{w_version}");
        packages.entry(Arc::from(&*w_pkg)).or_insert_with(|| package("w", w_version, &[], true));
        children_by_id.insert(Arc::from(&*w_pkg), Arc::new(Vec::new()));
        add_direct(&w_pkg, "w", &mut dependencies_tree, &mut direct);
    }
    for (alias, ring_index, w_version) in entries {
        let entry_pkg = format!("{alias}@1.0.0");
        let w_pkg = format!("w@{w_version}");
        packages.entry(Arc::from(&*w_pkg)).or_insert_with(|| package("w", w_version, &[], true));
        packages.insert(Arc::from(&*entry_pkg), package(alias, "1.0.0", &[], false));
        children_by_id.insert(
            Arc::from(&*entry_pkg),
            Arc::new(vec![ring_edge("ring", &ring_id(*ring_index)), ring_edge("w", &w_pkg)]),
        );
        add_direct(&entry_pkg, alias, &mut dependencies_tree, &mut direct);
    }

    ResolvedTree {
        direct,
        packages,
        dependencies_tree,
        all_peer_dep_names: HashSet::from_iter(["p".to_string(), "w".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id,
    }
}

fn ring_pkg_id(index: usize, ring_len: usize) -> String {
    format!("ring{:02}@1.0.0", index % ring_len)
}

fn ring_edge(alias: &str, pkg_id: &str) -> crate::resolved_tree::ChildEdge {
    crate::resolved_tree::ChildEdge {
        alias: alias.to_string(),
        pkg_id: Arc::from(pkg_id),
        optional: false,
    }
}

/// The child edges of one ring member, registering the fan-out packages the
/// even members carry.
fn ring_member_edges(
    index: usize,
    shape: &PeerCycleShape,
    packages: &mut HashMap<Arc<str>, crate::resolved_tree::ResolvedPackage>,
    children_by_id: &mut HashMap<Arc<str>, Arc<Vec<crate::resolved_tree::ChildEdge>>>,
) -> Vec<crate::resolved_tree::ChildEdge> {
    let ring_id = |index: usize| ring_pkg_id(index, shape.ring_len);
    let mut edges = vec![ring_edge("next", &ring_id(index + 1))];
    if shape.with_skips {
        edges.push(ring_edge("skip", &ring_id(index + 2)));
        if index.is_multiple_of(2) {
            if index != 0 {
                // Every even member also re-enters the ring's entry, so one
                // lap re-enters the cycle many times — each re-entry a
                // truncated verdict whose subtree the cache can skip.
                edges.push(ring_edge("home", &ring_id(0)));
            }
            push_ring_fanout_edges(index, packages, children_by_id, &mut edges);
        }
    }
    if shape.wc_members.contains(&index) {
        // A `wc` member consumes `w`, so its untruncated verdicts —
        // and every keyless cached item covering it — carry the
        // entry's own `w` and never transfer across entries.
        edges.push(ring_edge("wc", "wc@1.0.0"));
    }
    edges
}

/// Fanout under the transferable members: what a cache hit saves is
/// realizing the hit node's children, so the win only counts when there are
/// children worth skipping.
fn push_ring_fanout_edges(
    index: usize,
    packages: &mut HashMap<Arc<str>, crate::resolved_tree::ResolvedPackage>,
    children_by_id: &mut HashMap<Arc<str>, Arc<Vec<crate::resolved_tree::ChildEdge>>>,
    edges: &mut Vec<crate::resolved_tree::ChildEdge>,
) {
    for fan in 0..30 {
        let fan_pkg = format!("fan{index:02}x{fan:02}@1.0.0");
        packages.insert(
            Arc::from(&*fan_pkg),
            package(&format!("fan{index:02}x{fan:02}"), "1.0.0", &[("p", "*")], false),
        );
        children_by_id.insert(Arc::from(&*fan_pkg), Arc::new(Vec::new()));
        edges.push(ring_edge(&format!("fan{fan:02}"), &fan_pkg));
    }
}

/// The graph `entries` produce over the pnpm/pnpm#13865 ring, as sorted
/// depPath keys, for comparing walk orders.
fn peer_cycle_graph_keys(entries: &[(&str, usize, &str)], shape: &PeerCycleShape) -> Vec<String> {
    let mut tree = peer_cycle_fixture(entries, shape);
    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
    let mut keys: Vec<String> = result.graph.keys().map(|path| path.as_str().to_string()).collect();
    keys.sort_unstable();
    keys
}

/// The [`fn@peer_cycle_graph_keys`] shape shared by the walk-order
/// tests: one `wc` member, `w` provided per entry.
fn order_test_shape(rings_peer_on_p: bool) -> PeerCycleShape {
    PeerCycleShape { wc_members: vec![1], rings_peer_on_p, ..Default::default() }
}
