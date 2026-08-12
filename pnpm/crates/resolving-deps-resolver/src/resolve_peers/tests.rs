use super::{
    ImporterPeerInput, ResolvePeersOptions,
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
use pacquet_deps_path::{DepPath, PeerId};
use pacquet_resolving_resolver_base::PkgResolutionId;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::collections::BTreeMap;

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
            ("x@1.0.0".to_string(), package("x", "1.0.0", &[], true)),
            ("x@2.0.0".to_string(), package("x", "2.0.0", &[], true)),
            ("p@1.0.0".to_string(), package("p", "1.0.0", &[("x", "*")], false)),
            ("plugin@1.0.0".to_string(), package("plugin", "1.0.0", &[("p", "*")], false)),
            ("mid@1.0.0".to_string(), package("mid", "1.0.0", &[], false)),
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
            ("types@1.0.0".to_string(), package("types", "1.0.0", &[], true)),
            (
                "consumer@1.0.0".to_string(),
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
            ("peer@2.0.0".to_string(), package("peer", "2.0.0", &[], true)),
            (
                "consumer@1.0.0".to_string(),
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
            ("types@1.0.0".to_string(), package("types", "1.0.0", &[], true)),
            (
                "consumer@1.0.0".to_string(),
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
            ("consumer@1.0.0".to_string(), package("consumer", "1.0.0", &[], false)),
            ("peer@1.0.0".to_string(), package("peer", "1.0.0", &[], true)),
            ("plugin@1.0.0".to_string(), package("plugin", "1.0.0", &[("peer", "*")], false)),
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
            ("a@1.0.0".to_string(), package("a", "1.0.0", &[("c", "*")], false)),
            ("b@1.0.0".to_string(), package("b", "1.0.0", &[("a", "*")], false)),
            ("c@1.0.0".to_string(), package("c", "1.0.0", &[], true)),
            ("x@1.0.0".to_string(), package("x", "1.0.0", &[("b", "*")], false)),
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
            ("peer@1.0.0".to_string(), package("peer", "1.0.0", &[], true)),
            ("peer@2.0.0".to_string(), package("peer", "2.0.0", &[], true)),
            ("first@1.0.0".to_string(), package("first", "1.0.0", &[("peer", "*")], false)),
            ("second@1.0.0".to_string(), package("second", "1.0.0", &[("peer", "*")], false)),
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
                "source-map-loader@1.0.0".to_string(),
                package("source-map-loader", "1.0.0", &[("webpack", "*")], false),
            ),
            (
                "webpack-cli@6.0.0".to_string(),
                package("webpack-cli", "6.0.0", &[("webpack", "*")], false),
            ),
            (
                "webpack@5.0.0".to_string(),
                package("webpack", "5.0.0", &[("webpack-cli", "*")], false),
            ),
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
                "parent@1.0.0".to_string(),
                package_with_peer_dependencies(
                    "parent",
                    "1.0.0",
                    &[("own-peer", "*", false)],
                    false,
                ),
            ),
            (
                "child@1.0.0".to_string(),
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
            ("peer-c@2.0.0".to_string(), package("peer-c", "2.0.0", &[], true)),
            (
                "consumer@1.0.0".to_string(),
                package_with_peer_dependencies(
                    "consumer",
                    "1.0.0",
                    &[("peer-c", "*", false)],
                    false,
                ),
            ),
            ("parent@1.0.0".to_string(), package("parent", "1.0.0", &[], false)),
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
            ("alias-real@1.0.0".to_string(), package("alias-real", "1.0.0", &[], true)),
            ("peer-c@2.0.0".to_string(), package("peer-c", "2.0.0", &[], true)),
            ("unused@1.0.0".to_string(), package("unused", "1.0.0", &[], true)),
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
            ("types@1.0.0".to_string(), package("types", "1.0.0", &[], true)),
            (
                "config@1.0.0".to_string(),
                package_with_peer_dependencies("config", "1.0.0", &[("types", "*", true)], false),
            ),
            ("core@1.0.0".to_string(), package("core", "1.0.0", &[], false)),
            ("cli@1.0.0".to_string(), package("cli", "1.0.0", &[], false)),
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
            ("shared@1.0.0".to_string(), package("shared", "1.0.0", &[], true)),
            ("parent@1.0.0".to_string(), package("parent", "1.0.0", &[], false)),
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
            ("ts@1.0.0".to_string(), package("ts", "1.0.0", &[], true)),
            ("ts@2.0.0".to_string(), package("ts", "2.0.0", &[], true)),
            ("parser@1.0.0".to_string(), package("parser", "1.0.0", &[("ts", "*")], false)),
            (
                "plugin@1.0.0".to_string(),
                package("plugin", "1.0.0", &[("parser", "*"), ("ts", "*")], false),
            ),
            ("bundle@1.0.0".to_string(), package("bundle", "1.0.0", &[], false)),
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
                ("@babel/core@7.0.0".to_string(), package("@babel/core", "7.0.0", &[], true)),
                (
                    "styled-jsx@1.0.0".to_string(),
                    package_with_peer_dependencies(
                        "styled-jsx",
                        "1.0.0",
                        &[("@babel/core", "*", true)],
                        false,
                    ),
                ),
                ("app@1.0.0".to_string(), package("app", "1.0.0", &[], false)),
                ("mid@1.0.0".to_string(), package("mid", "1.0.0", &[], false)),
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
            ("prov@1.0.0".to_string(), package("prov", "1.0.0", &[], true)),
            ("consumer@1.0.0".to_string(), package("consumer", "1.0.0", &[("prov", "*")], false)),
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
        hoisted_optional_peer_node_ids: HashSet::default(),
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
            ("prov@1.0.0".to_string(), package("prov", "1.0.0", &[], true)),
            ("consumer@1.0.0".to_string(), package("consumer", "1.0.0", &[("prov", "*")], false)),
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
            hoisted_optional_peer_node_ids: HashSet::default(),
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
            hoisted_optional_peer_node_ids: HashSet::default(),
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
            ("peer@1.0.0".to_string(), package("peer", "1.0.0", &[], true)),
            ("peer@2.0.0".to_string(), package("peer", "2.0.0", &[], true)),
            ("consumer@1.0.0".to_string(), package("consumer", "1.0.0", &[("peer", "*")], false)),
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
            hoisted_optional_peer_node_ids: HashSet::default(),
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
            hoisted_optional_peer_node_ids: HashSet::default(),
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
            ("plugin@1.0.0".to_string(), package("plugin", "1.0.0", &[("parser", "*")], false)),
            ("plugin@2.0.0".to_string(), package("plugin", "2.0.0", &[("parser", "*")], false)),
            (
                "utils@1.0.0".to_string(),
                package("utils", "1.0.0", &[("resolver", "*"), ("parser", "*")], false),
            ),
            ("resolver@1.0.0".to_string(), package("resolver", "1.0.0", &[("plugin", "*")], false)),
            ("parser@1.0.0".to_string(), package("parser", "1.0.0", &[], true)),
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
            "plugin".to_string(),
            "resolver".to_string(),
            "parser".to_string(),
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
        hoisted_optional_peer_node_ids: HashSet::default(),
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
                "link:packages/peer".to_string(),
                linked_package("peer", "link:packages/peer", "packages/peer"),
            ),
            ("consumer@1.0.0".to_string(), package("consumer", "1.0.0", &[("peer", "*")], false)),
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
        hoisted_optional_peer_node_ids: HashSet::default(),
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
                "link:packages/peer".to_string(),
                linked_package("peer", "link:packages/peer", "../../packages/peer"),
            ),
            ("consumer@1.0.0".to_string(), package("consumer", "1.0.0", &[("peer", "*")], false)),
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
            "link:packages/shared".to_string(),
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
            ("lib-a@1.0.0".to_string(), package("lib-a", "1.0.0", &[("lib-b", "^1.0.0")], true)),
            ("lib-b@1.0.0".to_string(), package("lib-b", "1.0.0", &[("lib-a", "^1.0.0")], true)),
            (
                "consumer@1.0.0".to_string(),
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
            ("main@1.0.0".to_string(), package("main", "1.0.0", &[("plugin", "^1.0.0")], false)),
            ("plugin@1.0.0".to_string(), package("plugin", "1.0.0", &[("main", "^1.0.0")], true)),
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
            ("host@1.0.0".to_string(), package("host", "1.0.0", &[], false)),
            ("main@1.0.0".to_string(), package("main", "1.0.0", &[("plugin", "^1.0.0")], false)),
            ("plugin@1.0.0".to_string(), package("plugin", "1.0.0", &[("main", "^1.0.0")], false)),
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
    use std::collections::BTreeMap;

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
                ("peer@1.0.0".to_string(), package("peer", "1.0.0", &[], true)),
                ("peer@2.0.0".to_string(), package("peer", "2.0.0", &[], true)),
                ("retainer@1.0.0".to_string(), package("retainer", "1.0.0", &[], false)),
                ("wrapper@1.0.0".to_string(), package("wrapper", "1.0.0", &[], false)),
                (
                    "consumer@1.0.0".to_string(),
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
            "child".to_string(),
            first_child.clone(),
        );
    }

    assert!(graph_children.is_empty(), "the child has no depPath yet, so nothing is a graph edge");
    assert_eq!(walker.pending_peer_edges.len(), 1, "the same triple is buffered once");

    // Same slot, different child: a distinct triple, so it is kept.
    walker.add_graph_child_or_pending(
        &mut graph_children,
        &parent,
        "child".to_string(),
        second_child,
    );
    assert_eq!(walker.pending_peer_edges.len(), 2, "dedup is by triple, not by (parent, alias)");
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

    walker.add_graph_child_or_pending(
        &mut graph_children,
        &parent,
        "child".to_string(),
        child.clone(),
    );
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
