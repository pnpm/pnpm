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

#[test]
fn own_peer_is_resolved_from_peer_relevant_child() {
    let types = NodeId::leaf("types@1.0.0");
    let consumer = NodeId::next();

    let mut consumer_children = BTreeMap::new();
    consumer_children.insert("types".to_string(), types.clone());

    let mut tree = ResolvedTree {
        direct: vec![DirectDep {
            alias: "consumer".to_string(),
            node_id: consumer.clone(),
            id: "consumer@1.0.0".to_string(),
        }],
        packages: HashMap::from_iter([
            ("types@1.0.0".into(), package("types", "1.0.0", &[], true)),
            (
                Arc::from("consumer@1.0.0".to_string()),
                package_with_peer_dependencies("consumer", "1.0.0", &[("types", "*", true)], false),
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

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
    let dep_path = DepPath::from("consumer@1.0.0(types@1.0.0)");

    assert_eq!(result.direct_dependencies_by_alias.get("consumer"), Some(&dep_path));
    assert_eq!(result.graph[&dep_path].children.get("types"), Some(&DepPath::from("types@1.0.0")));
    assert!(result.graph[&dep_path].resolved_peer_names.contains("types"));
}

#[test]
fn named_registry_peer_is_matched_via_extracted_range() {
    let (mut tree, dep_path) = named_registry_peer_tree("work:^1.0.0");

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

    assert!(result.graph[&dep_path].resolved_peer_names.contains("types"));
    assert!(!result.peer_dependency_issues.bad.contains_key("types"));
}

#[test]
fn named_registry_peer_reports_bad_when_extracted_range_unmet() {
    let (mut tree, _dep_path) = named_registry_peer_tree("work:^2.0.0");

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

    assert!(result.peer_dependency_issues.bad.contains_key("types"));
}

#[test]
fn reports_a_conflict_for_an_optional_peer_with_an_incompatible_provider() {
    let provider = NodeId::leaf("peer@2.0.0");
    let consumer = NodeId::next();
    let mut tree = ResolvedTree {
        direct: vec![
            DirectDep {
                alias: "consumer".to_string(),
                node_id: consumer.clone(),
                id: "consumer@1.0.0".to_string(),
            },
            DirectDep {
                alias: "peer".to_string(),
                node_id: provider.clone(),
                id: "peer@2.0.0".to_string(),
            },
        ],
        packages: HashMap::from_iter([
            ("peer@2.0.0".into(), package("peer", "2.0.0", &[], true)),
            (
                Arc::from("consumer@1.0.0".to_string()),
                package_with_peer_dependencies(
                    "consumer",
                    "1.0.0",
                    &[("peer", "^1.0.0", true)],
                    false,
                ),
            ),
        ]),
        dependencies_tree: HashMap::from_iter([
            (provider, tree_node("peer@2.0.0", BTreeMap::new(), 0)),
            (consumer, tree_node("consumer@1.0.0", BTreeMap::new(), 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["peer".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

    assert_eq!(result.peer_dependency_issues.bad["peer"].len(), 1);
    assert!(result.peer_dependency_issues.bad["peer"][0].optional);
}

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

#[test]
fn alias_child_resolves_peer_by_real_package_name() {
    let provider = NodeId::leaf("peer@1.0.0");
    let plugin = NodeId::next();
    let consumer = NodeId::next();

    let mut consumer_children = BTreeMap::new();
    consumer_children.insert("not-peer".to_string(), provider.clone());
    consumer_children.insert("plugin".to_string(), plugin.clone());

    let mut tree = ResolvedTree {
        direct: vec![DirectDep {
            alias: "consumer".to_string(),
            node_id: consumer.clone(),
            id: "consumer@1.0.0".to_string(),
        }],
        packages: HashMap::from_iter([
            ("consumer@1.0.0".into(), package("consumer", "1.0.0", &[], false)),
            ("peer@1.0.0".into(), package("peer", "1.0.0", &[], true)),
            ("plugin@1.0.0".into(), package("plugin", "1.0.0", &[("peer", "*")], false)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (provider, tree_node("peer@1.0.0", BTreeMap::new(), 1)),
            (plugin, tree_node("plugin@1.0.0", BTreeMap::new(), 1)),
            (consumer, tree_node("consumer@1.0.0", consumer_children, 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["peer".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

    assert!(
        result.graph.contains_key(&DepPath::from("plugin@1.0.0(peer@1.0.0)")),
        "alias `not-peer` should satisfy peer `peer` by its real package name: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
    assert!(
        !result.graph.contains_key(&DepPath::from("plugin@1.0.0")),
        "plugin must not stay peer-less when a sibling provides the peer: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
    assert!(!result.peer_dependency_issues.missing.contains_key("peer"));
}

#[test]
fn transitive_pending_peer_uses_provider_final_suffix() {
    let c_node_id = NodeId::leaf("c@1.0.0");
    let a_node_id = NodeId::next();
    let b_node_id = NodeId::next();
    let x_node_id = NodeId::next();

    let mut a_children = BTreeMap::new();
    a_children.insert("b".to_string(), b_node_id.clone());
    a_children.insert("x".to_string(), x_node_id.clone());

    let mut tree = ResolvedTree {
        direct: vec![
            DirectDep {
                alias: "a".to_string(),
                node_id: a_node_id.clone(),
                id: "a@1.0.0".to_string(),
            },
            DirectDep {
                alias: "c".to_string(),
                node_id: c_node_id.clone(),
                id: "c@1.0.0".to_string(),
            },
        ],
        packages: HashMap::from_iter([
            ("a@1.0.0".into(), package("a", "1.0.0", &[("c", "*")], false)),
            ("b@1.0.0".into(), package("b", "1.0.0", &[("a", "*")], false)),
            ("c@1.0.0".into(), package("c", "1.0.0", &[], true)),
            ("x@1.0.0".into(), package("x", "1.0.0", &[("b", "*")], false)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (a_node_id, tree_node("a@1.0.0", a_children, 0)),
            (b_node_id, tree_node("b@1.0.0", BTreeMap::new(), 1)),
            (c_node_id, tree_node("c@1.0.0", BTreeMap::new(), 0)),
            (x_node_id, tree_node("x@1.0.0", BTreeMap::new(), 1)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["a".to_string(), "b".to_string(), "c".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
    let expected = DepPath::from("x@1.0.0(b@1.0.0(a@1.0.0(c@1.0.0)))");
    let provisional = DepPath::from("x@1.0.0(b@1.0.0(a@1.0.0))");

    assert!(
        result.graph.contains_key(&expected),
        "x must use b's final peer suffix: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
    assert!(
        !result.graph.contains_key(&provisional),
        "x must not keep b's provisional peer suffix: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
}

#[test]
fn resolved_peer_providers_from_direct_outputs_are_last_write_wins() {
    let first_peer = NodeId::leaf("peer@1.0.0");
    let second_peer = NodeId::leaf("peer@2.0.0");
    let first = NodeId::next();
    let second = NodeId::next();

    let mut first_children = BTreeMap::new();
    first_children.insert("peer".to_string(), first_peer.clone());

    let mut second_children = BTreeMap::new();
    second_children.insert("peer".to_string(), second_peer.clone());

    let mut tree = ResolvedTree {
        direct: vec![
            DirectDep {
                alias: "first".to_string(),
                node_id: first.clone(),
                id: "first@1.0.0".to_string(),
            },
            DirectDep {
                alias: "second".to_string(),
                node_id: second.clone(),
                id: "second@1.0.0".to_string(),
            },
        ],
        packages: HashMap::from_iter([
            ("peer@1.0.0".into(), package("peer", "1.0.0", &[], true)),
            ("peer@2.0.0".into(), package("peer", "2.0.0", &[], true)),
            ("first@1.0.0".into(), package("first", "1.0.0", &[("peer", "*")], false)),
            ("second@1.0.0".into(), package("second", "1.0.0", &[("peer", "*")], false)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (first_peer, tree_node("peer@1.0.0", BTreeMap::new(), 1)),
            (second_peer.clone(), tree_node("peer@2.0.0", BTreeMap::new(), 1)),
            (first, tree_node("first@1.0.0", first_children, 0)),
            (second, tree_node("second@1.0.0", second_children, 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["peer".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

    assert_eq!(result.resolved_peer_providers_by_alias.get("peer"), Some(&second_peer));
}

#[test]
fn peer_name_cycle_collapses_provider_suffixes() {
    let loader = NodeId::next();
    let webpack_cli = NodeId::next();
    let webpack = NodeId::next();

    let mut tree = ResolvedTree {
        direct: vec![
            DirectDep {
                alias: "source-map-loader".to_string(),
                node_id: loader.clone(),
                id: "source-map-loader@1.0.0".to_string(),
            },
            DirectDep {
                alias: "webpack-cli".to_string(),
                node_id: webpack_cli.clone(),
                id: "webpack-cli@6.0.0".to_string(),
            },
            DirectDep {
                alias: "webpack".to_string(),
                node_id: webpack.clone(),
                id: "webpack@5.0.0".to_string(),
            },
        ],
        packages: HashMap::from_iter([
            (
                "source-map-loader@1.0.0".into(),
                package("source-map-loader", "1.0.0", &[("webpack", "*")], false),
            ),
            (
                "webpack-cli@6.0.0".into(),
                package("webpack-cli", "6.0.0", &[("webpack", "*")], false),
            ),
            ("webpack@5.0.0".into(), package("webpack", "5.0.0", &[("webpack-cli", "*")], false)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (loader, tree_node("source-map-loader@1.0.0", BTreeMap::new(), 0)),
            (webpack_cli, tree_node("webpack-cli@6.0.0", BTreeMap::new(), 0)),
            (webpack, tree_node("webpack@5.0.0", BTreeMap::new(), 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["webpack".to_string(), "webpack-cli".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

    assert_eq!(
        result.direct_dependencies_by_alias.get("source-map-loader"),
        Some(&DepPath::from("source-map-loader@1.0.0(webpack@5.0.0)")),
    );
    assert_eq!(
        result.direct_dependencies_by_alias.get("webpack-cli"),
        Some(&DepPath::from("webpack-cli@6.0.0(webpack@5.0.0)")),
    );
    assert_eq!(
        result.direct_dependencies_by_alias.get("webpack"),
        Some(&DepPath::from("webpack@5.0.0(webpack-cli@6.0.0)")),
    );
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

#[test]
fn cached_cyclic_alias_peer_occurrences_share_a_closed_dep_path() {
    let mut tree = cyclic_alias_peer_tree(["@vitejs/devtools", "vite", "vite-plus"]);

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

    assert_cyclic_alias_peer_graph_is_closed(&result);
}

/// Walking `vite-plus` first makes the nested `core` the cache owner
/// and the direct `vite` the cache hit, so the peer edge of
/// `@vitejs/devtools` targets the hit occurrence. The cycle must still
/// be detected through the owner.
#[test]
fn cached_cyclic_alias_peer_occurrence_targeted_by_a_peer_collapses_the_cycle() {
    let mut tree = cyclic_alias_peer_tree(["vite-plus", "vite", "@vitejs/devtools"]);

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

    assert_cyclic_alias_peer_graph_is_closed(&result);
}

#[test]
fn missing_names_by_pkg_records_only_children_context_missing_peers() {
    let parent = NodeId::next();
    let child = NodeId::next();

    let mut parent_children = BTreeMap::new();
    parent_children.insert("child".to_string(), child.clone());

    let mut tree = ResolvedTree {
        direct: vec![DirectDep {
            alias: "parent".to_string(),
            node_id: parent.clone(),
            id: "parent@1.0.0".to_string(),
        }],
        packages: HashMap::from_iter([
            (
                "parent@1.0.0".into(),
                package_with_peer_dependencies(
                    "parent",
                    "1.0.0",
                    &[("own-peer", "*", false)],
                    false,
                ),
            ),
            (
                "child@1.0.0".into(),
                package_with_peer_dependencies(
                    "child",
                    "1.0.0",
                    &[("child-peer", "*", false)],
                    false,
                ),
            ),
        ]),
        dependencies_tree: HashMap::from_iter([
            (parent, tree_node("parent@1.0.0", parent_children, 0)),
            (child, tree_node("child@1.0.0", BTreeMap::new(), 1)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["own-peer".to_string(), "child-peer".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
    let parent_missing = result.missing_names_by_pkg.get("parent@1.0.0").unwrap();

    assert!(parent_missing.contains("child-peer"));
    assert!(!parent_missing.contains("own-peer"));
}

#[test]
fn own_peer_is_resolved_from_aliased_sibling_real_name() {
    let peer_c = NodeId::leaf("peer-c@2.0.0");
    let consumer = NodeId::next();
    let parent = NodeId::next();

    let mut parent_children = BTreeMap::new();
    parent_children.insert("consumer".to_string(), consumer.clone());
    parent_children.insert("peer-c1".to_string(), peer_c.clone());

    let mut tree = ResolvedTree {
        direct: vec![DirectDep {
            alias: "parent".to_string(),
            node_id: parent.clone(),
            id: "parent@1.0.0".to_string(),
        }],
        packages: HashMap::from_iter([
            ("peer-c@2.0.0".into(), package("peer-c", "2.0.0", &[], true)),
            (
                Arc::from("consumer@1.0.0".to_string()),
                package_with_peer_dependencies(
                    "consumer",
                    "1.0.0",
                    &[("peer-c", "*", false)],
                    false,
                ),
            ),
            ("parent@1.0.0".into(), package("parent", "1.0.0", &[], false)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (peer_c, tree_node("peer-c@2.0.0", BTreeMap::new(), 1)),
            (consumer, tree_node("consumer@1.0.0", BTreeMap::new(), 1)),
            (parent, tree_node("parent@1.0.0", parent_children, 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["peer-c".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
    let dep_path = DepPath::from("consumer@1.0.0(peer-c@2.0.0)");

    assert!(
        result.graph.contains_key(&dep_path),
        "consumer should resolve peer-c from the sibling installed as peer-c1: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
    assert_eq!(
        result.graph[&dep_path].children.get("peer-c"),
        Some(&DepPath::from("peer-c@2.0.0")),
    );
    assert!(!result.peer_dependency_issues.missing.contains_key("peer-c"));
}

#[test]
fn importer_parent_refs_skip_direct_deps_irrelevant_by_alias_and_real_name() {
    let alias_relevant = NodeId::leaf("alias-real@1.0.0");
    let real_name_relevant = NodeId::leaf("peer-c@2.0.0");
    let irrelevant = NodeId::leaf("unused@1.0.0");

    let mut tree = ResolvedTree {
        direct: vec![
            DirectDep {
                alias: "alias-peer".to_string(),
                node_id: alias_relevant.clone(),
                id: "alias-real@1.0.0".to_string(),
            },
            DirectDep {
                alias: "peer-c1".to_string(),
                node_id: real_name_relevant.clone(),
                id: "peer-c@2.0.0".to_string(),
            },
            DirectDep {
                alias: "unused".to_string(),
                node_id: irrelevant.clone(),
                id: "unused@1.0.0".to_string(),
            },
        ],
        packages: HashMap::from_iter([
            ("alias-real@1.0.0".into(), package("alias-real", "1.0.0", &[], true)),
            ("peer-c@2.0.0".into(), package("peer-c", "2.0.0", &[], true)),
            ("unused@1.0.0".into(), package("unused", "1.0.0", &[], true)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (alias_relevant, tree_node("alias-real@1.0.0", BTreeMap::new(), 0)),
            (real_name_relevant, tree_node("peer-c@2.0.0", BTreeMap::new(), 0)),
            (irrelevant, tree_node("unused@1.0.0", BTreeMap::new(), 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["alias-peer".to_string(), "peer-c".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };
    let walker = walker_for_tests(&mut tree);

    let refs = walker.build_importer_parents_from(&walker.tree.direct);

    assert!(refs.contains_key("alias-peer"));
    assert!(refs.contains_key("peer-c1"));
    assert!(refs.contains_key("peer-c"));
    assert!(!refs.contains_key("unused"));
}

#[test]
fn cached_optional_peer_resolution_does_not_match_later_parent_without_provider() {
    let types = NodeId::leaf("types@1.0.0");
    let config_from_core = NodeId::next();
    let config_from_cli = NodeId::next();
    let core = NodeId::next();
    let cli = NodeId::next();

    let mut core_children = BTreeMap::new();
    core_children.insert("config".to_string(), config_from_core.clone());
    core_children.insert("types".to_string(), types.clone());

    let mut cli_children = BTreeMap::new();
    cli_children.insert("config".to_string(), config_from_cli.clone());

    let mut tree = ResolvedTree {
        direct: vec![
            DirectDep {
                alias: "core".to_string(),
                node_id: core.clone(),
                id: "core@1.0.0".to_string(),
            },
            DirectDep {
                alias: "cli".to_string(),
                node_id: cli.clone(),
                id: "cli@1.0.0".to_string(),
            },
        ],
        packages: HashMap::from_iter([
            ("types@1.0.0".into(), package("types", "1.0.0", &[], true)),
            (
                Arc::from("config@1.0.0".to_string()),
                package_with_peer_dependencies("config", "1.0.0", &[("types", "*", true)], false),
            ),
            ("core@1.0.0".into(), package("core", "1.0.0", &[], false)),
            ("cli@1.0.0".into(), package("cli", "1.0.0", &[], false)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (types, tree_node("types@1.0.0", BTreeMap::new(), 1)),
            (config_from_core, tree_node("config@1.0.0", BTreeMap::new(), 1)),
            (config_from_cli, tree_node("config@1.0.0", BTreeMap::new(), 1)),
            (core, tree_node("core@1.0.0", core_children, 0)),
            (cli, tree_node("cli@1.0.0", cli_children, 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["types".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
    let config_with_types = DepPath::from("config@1.0.0(types@1.0.0)");
    let config_without_types = DepPath::from("config@1.0.0");
    let cli_dep_path = DepPath::from("cli@1.0.0");

    assert_eq!(result.direct_dependencies_by_alias.get("core"), Some(&DepPath::from("core@1.0.0")));
    assert_eq!(result.direct_dependencies_by_alias.get("cli"), Some(&cli_dep_path));
    assert!(result.graph.contains_key(&config_with_types));
    assert!(result.graph.contains_key(&config_without_types));
    assert_eq!(result.graph[&cli_dep_path].children.get("config"), Some(&config_without_types));
    assert!(!result.graph[&cli_dep_path].resolved_peer_names.contains("types"));
}

#[test]
fn same_leaf_node_under_multiple_aliases_preserves_every_edge() {
    let shared = NodeId::leaf("shared@1.0.0");
    let parent = NodeId::next();

    let mut parent_children = BTreeMap::new();
    parent_children.insert("alpha".to_string(), shared.clone());
    parent_children.insert("beta".to_string(), shared.clone());

    let mut tree = ResolvedTree {
        direct: vec![DirectDep {
            alias: "parent".to_string(),
            node_id: parent.clone(),
            id: "parent@1.0.0".to_string(),
        }],
        packages: HashMap::from_iter([
            ("shared@1.0.0".into(), package("shared", "1.0.0", &[], true)),
            ("parent@1.0.0".into(), package("parent", "1.0.0", &[], false)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (shared, tree_node("shared@1.0.0", BTreeMap::new(), 1)),
            (parent, tree_node("parent@1.0.0", parent_children, 0)),
        ]),
        all_peer_dep_names: HashSet::default(),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
    let parent_dep_path = DepPath::from("parent@1.0.0");
    let shared_dep_path = DepPath::from("shared@1.0.0");
    let parent_node = result.graph.get(&parent_dep_path).expect("parent graph node");

    assert_eq!(parent_node.children.get("alpha"), Some(&shared_dep_path));
    assert_eq!(parent_node.children.get("beta"), Some(&shared_dep_path));
}

#[test]
fn same_package_child_replaces_inherited_parent_when_peer_diamond_conflicts() {
    let ts1 = NodeId::leaf("ts@1.0.0");
    let ts2 = NodeId::leaf("ts@2.0.0");
    let parser_root = NodeId::next();
    let parser_child = NodeId::next();
    let plugin = NodeId::next();
    let bundle = NodeId::next();

    let mut bundle_children = BTreeMap::new();
    bundle_children.insert("parser".to_string(), parser_child.clone());
    bundle_children.insert("plugin".to_string(), plugin.clone());
    bundle_children.insert("ts".to_string(), ts1.clone());

    let mut tree = ResolvedTree {
        direct: vec![
            DirectDep { alias: "ts".to_string(), node_id: ts2.clone(), id: "ts@2.0.0".to_string() },
            DirectDep {
                alias: "parser".to_string(),
                node_id: parser_root.clone(),
                id: "parser@1.0.0".to_string(),
            },
            DirectDep {
                alias: "bundle".to_string(),
                node_id: bundle.clone(),
                id: "bundle@1.0.0".to_string(),
            },
        ],
        packages: HashMap::from_iter([
            ("ts@1.0.0".into(), package("ts", "1.0.0", &[], true)),
            ("ts@2.0.0".into(), package("ts", "2.0.0", &[], true)),
            ("parser@1.0.0".into(), package("parser", "1.0.0", &[("ts", "*")], false)),
            (
                Arc::from("plugin@1.0.0".to_string()),
                package("plugin", "1.0.0", &[("parser", "*"), ("ts", "*")], false),
            ),
            ("bundle@1.0.0".into(), package("bundle", "1.0.0", &[], false)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (ts1, tree_node("ts@1.0.0", BTreeMap::new(), 1)),
            (ts2, tree_node("ts@2.0.0", BTreeMap::new(), 0)),
            (parser_root, tree_node("parser@1.0.0", BTreeMap::new(), 0)),
            (parser_child, tree_node("parser@1.0.0", BTreeMap::new(), 1)),
            (plugin, tree_node("plugin@1.0.0", BTreeMap::new(), 1)),
            (bundle, tree_node("bundle@1.0.0", bundle_children, 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["parser".to_string(), "ts".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
    let consistent = DepPath::from("plugin@1.0.0(parser@1.0.0(ts@1.0.0))(ts@1.0.0)");
    let inconsistent = DepPath::from("plugin@1.0.0(parser@1.0.0(ts@2.0.0))(ts@1.0.0)");

    assert!(
        result.graph.contains_key(&consistent),
        "plugin should use the nested parser context: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
    assert!(
        !result.graph.contains_key(&inconsistent),
        "plugin must not mix the root parser context with nested ts: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
}

// Parity check for <https://github.com/pnpm/pnpm/pull/12514>.
//
// A shared package (`styled-jsx`) declaring an *optional* peer (`@babel/core`)
// is reached through two occurrences at different depths: a shallow one whose
// parent provides `@babel/core`, and a deeper one whose ancestors do not. The
// shallow occurrence resolves the optional peer into its suffix; the deeper one
// must not inherit it. The deeper occurrence's suffix must be a function of
// graph structure alone, so each iteration resolves a freshly built tree
// (fresh `HashMap`s, whose iteration order varies per process) to catch any
// hashing order leaking into the result.
#[test]
fn shared_package_optional_transitive_peer_resolves_deterministically() {
    fn build_tree() -> ResolvedTree {
        let babel = NodeId::leaf("@babel/core@7.0.0");
        let styled_shallow = NodeId::next();
        let styled_deep = NodeId::next();
        let app = NodeId::next();
        let mid = NodeId::next();

        let mut app_children = BTreeMap::new();
        app_children.insert("styled-jsx".to_string(), styled_shallow.clone());
        app_children.insert("@babel/core".to_string(), babel.clone());

        let mut mid_children = BTreeMap::new();
        mid_children.insert("styled-jsx".to_string(), styled_deep.clone());

        ResolvedTree {
            direct: vec![
                DirectDep {
                    alias: "app".to_string(),
                    node_id: app.clone(),
                    id: "app@1.0.0".to_string(),
                },
                DirectDep {
                    alias: "mid".to_string(),
                    node_id: mid.clone(),
                    id: "mid@1.0.0".to_string(),
                },
            ],
            packages: HashMap::from_iter([
                ("@babel/core@7.0.0".into(), package("@babel/core", "7.0.0", &[], true)),
                (
                    Arc::from("styled-jsx@1.0.0".to_string()),
                    package_with_peer_dependencies(
                        "styled-jsx",
                        "1.0.0",
                        &[("@babel/core", "*", true)],
                        false,
                    ),
                ),
                ("app@1.0.0".into(), package("app", "1.0.0", &[], false)),
                ("mid@1.0.0".into(), package("mid", "1.0.0", &[], false)),
            ]),
            dependencies_tree: HashMap::from_iter([
                (babel, tree_node("@babel/core@7.0.0", BTreeMap::new(), 1)),
                (styled_shallow, tree_node("styled-jsx@1.0.0", BTreeMap::new(), 1)),
                (styled_deep, tree_node("styled-jsx@1.0.0", BTreeMap::new(), 2)),
                (app, tree_node("app@1.0.0", app_children, 0)),
                (mid, tree_node("mid@1.0.0", mid_children, 1)),
            ]),
            all_peer_dep_names: HashSet::from_iter(["@babel/core".to_string()]),
            policy_violations: Vec::new(),
            applied_patches: HashSet::default(),
            children_by_id: HashMap::default(),
        }
    }

    let styled_with_babel = DepPath::from("styled-jsx@1.0.0(@babel/core@7.0.0)");
    let styled_without_babel = DepPath::from("styled-jsx@1.0.0");
    let app_dep_path = DepPath::from("app@1.0.0");
    let mid_dep_path = DepPath::from("mid@1.0.0");

    let mut first_keys: Option<Vec<String>> = None;
    for _ in 0..16 {
        let mut tree = build_tree();
        let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

        // The shallow occurrence resolves the optional peer from its sibling; the
        // deeper occurrence, with no provider in scope, keeps the bare suffix.
        assert_eq!(
            result.graph[&app_dep_path].children.get("styled-jsx"),
            Some(&styled_with_babel),
        );
        assert_eq!(
            result.graph[&mid_dep_path].children.get("styled-jsx"),
            Some(&styled_without_babel),
        );
        assert!(result.graph.contains_key(&styled_with_babel));
        assert!(result.graph.contains_key(&styled_without_babel));

        let mut keys: Vec<String> = result.graph.keys().map(DepPath::to_string).collect();
        keys.sort();
        match &first_keys {
            None => first_keys = Some(keys),
            Some(expected) => assert_eq!(&keys, expected, "graph keys must not vary across runs"),
        }
    }
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

/// Same as [`pruned_hoisted_provider_falls_back_to_root_resolution`] but
/// through the multi-importer entry point.
#[test]
fn pruned_hoisted_provider_falls_back_in_workspace_pass() {
    let prov = NodeId::leaf("prov@1.0.0");
    let consumer = NodeId::next();

    let importer = ImporterPeerInput {
        id: ".".to_string(),
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
        root_dir: std::path::PathBuf::from("/repo"),
        modules_dir: None,
    };
    let mut tree = ResolvedTree {
        direct: Vec::new(),
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

    let result = resolve_peers_workspace(
        &mut tree,
        &[importer],
        std::path::Path::new("/repo"),
        false,
        false,
        false,
        ResolvePeersOptions {
            hoisted_peer_provider_node_ids: HashSet::from_iter([prov]),
            ..ResolvePeersOptions::default()
        },
    );

    assert_eq!(
        result.direct_dependencies_by_importer.get(".").and_then(|deps| deps.get("prov")),
        Some(&DepPath::from("prov@1.0.0")),
        "the pruned provider must get a depPath from the fallback",
    );
    assert!(
        result.graph.contains_key(&DepPath::from("consumer@1.0.0(prov@1.0.0)")),
        "the consumer must bind the fallback-resolved provider: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
}

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

#[test]
fn single_importer_link_is_rendered_relative_to_project_root() {
    let shared = NodeId::leaf("link:packages/shared");
    let mut tree = ResolvedTree {
        direct: vec![DirectDep {
            alias: "shared".to_string(),
            node_id: shared.clone(),
            id: "link:packages/shared".to_string(),
        }],
        packages: HashMap::from_iter([(
            "link:packages/shared".into(),
            linked_package("shared", "link:packages/shared", "packages/shared"),
        )]),
        dependencies_tree: HashMap::from_iter([(
            shared,
            tree_node("link:packages/shared", BTreeMap::new(), -1),
        )]),
        all_peer_dep_names: HashSet::default(),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };

    let result = resolve_peers(
        &mut tree,
        ResolvePeersOptions {
            lockfile_dir: Some(std::path::PathBuf::from("/repo")),
            project_dir: Some(std::path::PathBuf::from("/repo/apps/nested/app")),
            ..ResolvePeersOptions::default()
        },
    );

    assert_eq!(
        result.direct_dependencies_by_alias["shared"].as_str(),
        "link:../../../packages/shared",
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
fn peer_cycle_fixture(entries: &[(&str, usize, &str)], shape: PeerCycleShape) -> ResolvedTree {
    let PeerCycleShape {
        ring_len,
        with_skips,
        wc_members,
        rings_peer_on_p,
        wc_w_range,
        importer_w_version,
    } = shape;
    let ring_id = |index: usize| format!("ring{:02}@1.0.0", index % ring_len);
    let edge = |alias: &str, pkg_id: &str| crate::resolved_tree::ChildEdge {
        alias: alias.to_string(),
        pkg_id: Arc::from(pkg_id),
        optional: false,
    };

    let mut packages = HashMap::default();
    let mut children_by_id: HashMap<Arc<str>, Arc<Vec<crate::resolved_tree::ChildEdge>>> =
        HashMap::default();
    let ring_peers: &[(&str, &str)] = if rings_peer_on_p { &[("p", "*")] } else { &[] };
    for index in 0..ring_len {
        let name = format!("ring{index:02}");
        packages.insert(Arc::from(ring_id(index)), package(&name, "1.0.0", ring_peers, false));
        let mut edges = vec![edge("next", &ring_id(index + 1))];
        if with_skips {
            edges.push(edge("skip", &ring_id(index + 2)));
        }
        if with_skips && index % 2 == 0 && index != 0 {
            // Every even member also re-enters the ring's entry, so one
            // lap re-enters the cycle many times — each re-entry a
            // truncated verdict whose subtree the cache can skip.
            edges.push(edge("home", &ring_id(0)));
        }
        if with_skips && index % 2 == 0 {
            // Fanout under the transferable members: what a cache hit
            // saves is realizing the hit node's children, so the win
            // only counts when there are children worth skipping.
            for fan in 0..30 {
                let fan_pkg = format!("fan{index:02}x{fan:02}@1.0.0");
                packages.insert(
                    Arc::from(&*fan_pkg),
                    package(&format!("fan{index:02}x{fan:02}"), "1.0.0", &[("p", "*")], false),
                );
                children_by_id.insert(Arc::from(&*fan_pkg), Arc::new(Vec::new()));
                edges.push(edge(&format!("fan{fan:02}"), &fan_pkg));
            }
        }
        if wc_members.contains(&index) {
            // A `wc` member consumes `w`, so its untruncated verdicts —
            // and every keyless cached item covering it — carry the
            // entry's own `w` and never transfer across entries.
            edges.push(edge("wc", "wc@1.0.0"));
        }
        children_by_id.insert(Arc::from(ring_id(index)), Arc::new(edges));
    }
    packages.insert(Arc::from("wc@1.0.0"), package("wc", "1.0.0", &[("w", wc_w_range)], false));
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
    if let Some(w_version) = importer_w_version {
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
            Arc::new(vec![edge("ring", &ring_id(*ring_index)), edge("w", &w_pkg)]),
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
        PeerCycleShape { wc_members: vec![1, 3], rings_peer_on_p: true, ..Default::default() },
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
        PeerCycleShape {
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

/// The graph `entries` produce over the pnpm/pnpm#13865 ring, as sorted
/// depPath keys, for comparing walk orders.
fn peer_cycle_graph_keys(entries: &[(&str, usize, &str)], shape: PeerCycleShape) -> Vec<String> {
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

/// The canonical cut is a property of the graph, not the walk: entering
/// the ring at `ring00` or at `ring02` first cannot make peer variants
/// appear or disappear (pnpm/pnpm#13865, pnpm/pnpm#13846).
#[test]
fn walk_order_cannot_change_the_graph() {
    let first_order = peer_cycle_graph_keys(
        &[("entry00", 0, "1.0.0"), ("entry01", 2, "2.0.0")],
        order_test_shape(true),
    );
    let second_order = peer_cycle_graph_keys(
        &[("entry01", 2, "2.0.0"), ("entry00", 0, "1.0.0")],
        order_test_shape(true),
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

/// The same shape without `p`: members whose canonical subtree reaches
/// no peer consumer merge to bare depPaths — identically in either walk
/// order.
#[test]
fn a_backedge_cut_member_merges_to_a_bare_dep_path() {
    let first_order = peer_cycle_graph_keys(
        &[("entry00", 0, "1.0.0"), ("entry01", 2, "2.0.0")],
        order_test_shape(false),
    );
    let second_order = peer_cycle_graph_keys(
        &[("entry01", 2, "2.0.0"), ("entry00", 0, "1.0.0")],
        order_test_shape(false),
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
        peer_cycle_graph_keys(&[("entry00", 0, "1.0.0"), ("entry01", 2, "2.0.0")], shape());
    let second_order =
        peer_cycle_graph_keys(&[("entry01", 2, "2.0.0"), ("entry00", 0, "1.0.0")], shape());
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
        PeerCycleShape { wc_members: vec![1], ..Default::default() },
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
        PeerCycleShape { wc_members: vec![1], wc_w_range: "<3.0.0", ..Default::default() },
    );
    let direct = tree.direct.clone();
    let mut walker = crate::resolve_peers::test_support::walker_for_tests(&mut tree);
    let importer_parents = Arc::new(walker.build_importer_parents_from(&direct));
    let importer_parent_dep_paths = walker.parent_dep_paths_from_refs(&importer_parents);
    for dep in &direct {
        walker.resolve_node(
            &dep.node_id,
            &importer_parents,
            &importer_parent_dep_paths,
            &crate::resolve_peers::context::SharedChain::default(),
            &crate::resolve_peers::context::SharedChain::default(),
            &crate::resolve_peers::context::SharedChain::default(),
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
