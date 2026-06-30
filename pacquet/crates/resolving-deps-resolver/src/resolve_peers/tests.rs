use super::{NodeRecord, ResolvePeersOptions, Walker, resolve_peers, satisfies_with_prereleases};
use crate::{
    dependencies_graph::{DependenciesGraph, PeerDependencyIssues},
    node_id::NodeId,
    resolved_tree::{
        DependenciesTreeNode, DirectDep, PeerDep, ResolvedPackage, ResolvedTree, TreeChildren,
    },
};
use pacquet_deps_path::DepPath;
use pacquet_lockfile::{LockfileResolution, PkgName, PkgNameVer, TarballResolution};
use pacquet_resolving_resolver_base::ResolveResult;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    str::FromStr,
    sync::Arc,
};

#[test]
fn satisfies_handles_basic_ranges() {
    assert!(satisfies_with_prereleases("1.2.3", "^1.0.0"));
    assert!(!satisfies_with_prereleases("2.0.0", "^1.0.0"));
    assert!(satisfies_with_prereleases("18.0.0", "*"));
}

#[test]
fn satisfies_falls_back_to_equality_for_unparsable_ranges() {
    assert!(satisfies_with_prereleases("workspace:^1.0.0", "workspace:^1.0.0"));
    assert!(!satisfies_with_prereleases("1.0.0", "workspace:^1.0.0"));
}

#[test]
fn satisfies_accepts_prerelease_against_non_prerelease_range() {
    assert!(satisfies_with_prereleases("18.0.0-rc.1", "^18.0.0"));
    assert!(satisfies_with_prereleases("1.2.3-beta.0", "^1.2.0"));
    assert!(!satisfies_with_prereleases("19.0.0-rc.1", "^18.0.0"));
}

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
        packages: HashMap::from([
            ("x@1.0.0".to_string(), package("x", "1.0.0", &[], true)),
            ("x@2.0.0".to_string(), package("x", "2.0.0", &[], true)),
            ("p@1.0.0".to_string(), package("p", "1.0.0", &[("x", "*")], false)),
            ("plugin@1.0.0".to_string(), package("plugin", "1.0.0", &[("p", "*")], false)),
            ("mid@1.0.0".to_string(), package("mid", "1.0.0", &[], false)),
        ]),
        dependencies_tree: HashMap::from([
            (x1, tree_node("x@1.0.0", BTreeMap::new(), 0)),
            (x2, tree_node("x@2.0.0", BTreeMap::new(), 1)),
            (p_root, tree_node("p@1.0.0", BTreeMap::new(), 0)),
            (p_child, tree_node("p@1.0.0", BTreeMap::new(), 1)),
            (plugin, tree_node("plugin@1.0.0", BTreeMap::new(), 1)),
            (mid, tree_node("mid@1.0.0", mid_children, 0)),
        ]),
        all_peer_dep_names: HashSet::from(["p".to_string(), "x".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::new(),
        children_by_id: HashMap::new(),
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
        packages: HashMap::from([
            ("types@1.0.0".to_string(), package("types", "1.0.0", &[], true)),
            (
                "consumer@1.0.0".to_string(),
                package_with_peer_dependencies("consumer", "1.0.0", &[("types", "*", true)], false),
            ),
        ]),
        dependencies_tree: HashMap::from([
            (types, tree_node("types@1.0.0", BTreeMap::new(), 1)),
            (consumer, tree_node("consumer@1.0.0", consumer_children, 0)),
        ]),
        all_peer_dep_names: HashSet::from(["types".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::new(),
        children_by_id: HashMap::new(),
    };

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
    let dep_path = DepPath::from("consumer@1.0.0(types@1.0.0)");

    assert_eq!(result.direct_dependencies_by_alias.get("consumer"), Some(&dep_path));
    assert_eq!(result.graph[&dep_path].children.get("types"), Some(&DepPath::from("types@1.0.0")));
    assert!(result.graph[&dep_path].resolved_peer_names.contains("types"));
}

#[test]
fn non_peer_alias_child_does_not_resolve_by_real_package_name() {
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
        packages: HashMap::from([
            ("consumer@1.0.0".to_string(), package("consumer", "1.0.0", &[], false)),
            ("peer@1.0.0".to_string(), package("peer", "1.0.0", &[], true)),
            ("plugin@1.0.0".to_string(), package("plugin", "1.0.0", &[("peer", "*")], false)),
        ]),
        dependencies_tree: HashMap::from([
            (provider, tree_node("peer@1.0.0", BTreeMap::new(), 1)),
            (plugin, tree_node("plugin@1.0.0", BTreeMap::new(), 1)),
            (consumer, tree_node("consumer@1.0.0", consumer_children, 0)),
        ]),
        all_peer_dep_names: HashSet::from(["peer".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::new(),
        children_by_id: HashMap::new(),
    };

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

    assert!(
        result.graph.contains_key(&DepPath::from("plugin@1.0.0")),
        "plugin should not resolve peer by provider's real package name: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
    assert!(
        !result.graph.contains_key(&DepPath::from("plugin@1.0.0(peer@1.0.0)")),
        "non-peer alias `not-peer` must not satisfy peer `peer`: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
    assert!(result.peer_dependency_issues.missing.contains_key("peer"));
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
        packages: HashMap::from([
            ("a@1.0.0".to_string(), package("a", "1.0.0", &[("c", "*")], false)),
            ("b@1.0.0".to_string(), package("b", "1.0.0", &[("a", "*")], false)),
            ("c@1.0.0".to_string(), package("c", "1.0.0", &[], true)),
            ("x@1.0.0".to_string(), package("x", "1.0.0", &[("b", "*")], false)),
        ]),
        dependencies_tree: HashMap::from([
            (a_node_id, tree_node("a@1.0.0", a_children, 0)),
            (b_node_id, tree_node("b@1.0.0", BTreeMap::new(), 1)),
            (c_node_id, tree_node("c@1.0.0", BTreeMap::new(), 0)),
            (x_node_id, tree_node("x@1.0.0", BTreeMap::new(), 1)),
        ]),
        all_peer_dep_names: HashSet::from(["a".to_string(), "b".to_string(), "c".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::new(),
        children_by_id: HashMap::new(),
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
        packages: HashMap::from([
            ("peer@1.0.0".to_string(), package("peer", "1.0.0", &[], true)),
            ("peer@2.0.0".to_string(), package("peer", "2.0.0", &[], true)),
            ("first@1.0.0".to_string(), package("first", "1.0.0", &[("peer", "*")], false)),
            ("second@1.0.0".to_string(), package("second", "1.0.0", &[("peer", "*")], false)),
        ]),
        dependencies_tree: HashMap::from([
            (first_peer, tree_node("peer@1.0.0", BTreeMap::new(), 1)),
            (second_peer.clone(), tree_node("peer@2.0.0", BTreeMap::new(), 1)),
            (first, tree_node("first@1.0.0", first_children, 0)),
            (second, tree_node("second@1.0.0", second_children, 0)),
        ]),
        all_peer_dep_names: HashSet::from(["peer".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::new(),
        children_by_id: HashMap::new(),
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
        packages: HashMap::from([
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
        dependencies_tree: HashMap::from([
            (loader, tree_node("source-map-loader@1.0.0", BTreeMap::new(), 0)),
            (webpack_cli, tree_node("webpack-cli@6.0.0", BTreeMap::new(), 0)),
            (webpack, tree_node("webpack@5.0.0", BTreeMap::new(), 0)),
        ]),
        all_peer_dep_names: HashSet::from(["webpack".to_string(), "webpack-cli".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::new(),
        children_by_id: HashMap::new(),
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
        packages: HashMap::from([
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
        dependencies_tree: HashMap::from([
            (parent, tree_node("parent@1.0.0", parent_children, 0)),
            (child, tree_node("child@1.0.0", BTreeMap::new(), 1)),
        ]),
        all_peer_dep_names: HashSet::from(["own-peer".to_string(), "child-peer".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::new(),
        children_by_id: HashMap::new(),
    };

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
    let parent_missing = result.missing_names_by_pkg.get("parent@1.0.0").unwrap();

    assert!(parent_missing.contains("child-peer"));
    assert!(!parent_missing.contains("own-peer"));
}

#[test]
fn own_peer_is_not_resolved_from_aliased_child_real_name() {
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
        packages: HashMap::from([
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
        dependencies_tree: HashMap::from([
            (peer_c, tree_node("peer-c@2.0.0", BTreeMap::new(), 1)),
            (consumer, tree_node("consumer@1.0.0", BTreeMap::new(), 1)),
            (parent, tree_node("parent@1.0.0", parent_children, 0)),
        ]),
        all_peer_dep_names: HashSet::from(["peer-c".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::new(),
        children_by_id: HashMap::new(),
    };

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
    let dep_path = DepPath::from("consumer@1.0.0");

    assert!(
        result.graph.contains_key(&dep_path),
        "consumer should not resolve peer-c from a child installed as peer-c1: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
    assert!(!result.graph[&dep_path].children.contains_key("peer-c"));
    assert!(result.peer_dependency_issues.missing.contains_key("peer-c"));
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
        packages: HashMap::from([
            ("alias-real@1.0.0".to_string(), package("alias-real", "1.0.0", &[], true)),
            ("peer-c@2.0.0".to_string(), package("peer-c", "2.0.0", &[], true)),
            ("unused@1.0.0".to_string(), package("unused", "1.0.0", &[], true)),
        ]),
        dependencies_tree: HashMap::from([
            (alias_relevant, tree_node("alias-real@1.0.0", BTreeMap::new(), 0)),
            (real_name_relevant, tree_node("peer-c@2.0.0", BTreeMap::new(), 0)),
            (irrelevant, tree_node("unused@1.0.0", BTreeMap::new(), 0)),
        ]),
        all_peer_dep_names: HashSet::from(["alias-peer".to_string(), "peer-c".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::new(),
        children_by_id: HashMap::new(),
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
        packages: HashMap::from([
            ("types@1.0.0".to_string(), package("types", "1.0.0", &[], true)),
            (
                "config@1.0.0".to_string(),
                package_with_peer_dependencies("config", "1.0.0", &[("types", "*", true)], false),
            ),
            ("core@1.0.0".to_string(), package("core", "1.0.0", &[], false)),
            ("cli@1.0.0".to_string(), package("cli", "1.0.0", &[], false)),
        ]),
        dependencies_tree: HashMap::from([
            (types, tree_node("types@1.0.0", BTreeMap::new(), 1)),
            (config_from_core, tree_node("config@1.0.0", BTreeMap::new(), 1)),
            (config_from_cli, tree_node("config@1.0.0", BTreeMap::new(), 1)),
            (core, tree_node("core@1.0.0", core_children, 0)),
            (cli, tree_node("cli@1.0.0", cli_children, 0)),
        ]),
        all_peer_dep_names: HashSet::from(["types".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::new(),
        children_by_id: HashMap::new(),
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
        packages: HashMap::from([
            ("shared@1.0.0".to_string(), package("shared", "1.0.0", &[], true)),
            ("parent@1.0.0".to_string(), package("parent", "1.0.0", &[], false)),
        ]),
        dependencies_tree: HashMap::from([
            (shared, tree_node("shared@1.0.0", BTreeMap::new(), 1)),
            (parent, tree_node("parent@1.0.0", parent_children, 0)),
        ]),
        all_peer_dep_names: HashSet::new(),
        policy_violations: Vec::new(),
        applied_patches: HashSet::new(),
        children_by_id: HashMap::new(),
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
        packages: HashMap::from([
            ("ts@1.0.0".to_string(), package("ts", "1.0.0", &[], true)),
            ("ts@2.0.0".to_string(), package("ts", "2.0.0", &[], true)),
            ("parser@1.0.0".to_string(), package("parser", "1.0.0", &[("ts", "*")], false)),
            (
                "plugin@1.0.0".to_string(),
                package("plugin", "1.0.0", &[("parser", "*"), ("ts", "*")], false),
            ),
            ("bundle@1.0.0".to_string(), package("bundle", "1.0.0", &[], false)),
        ]),
        dependencies_tree: HashMap::from([
            (ts1, tree_node("ts@1.0.0", BTreeMap::new(), 1)),
            (ts2, tree_node("ts@2.0.0", BTreeMap::new(), 0)),
            (parser_root, tree_node("parser@1.0.0", BTreeMap::new(), 0)),
            (parser_child, tree_node("parser@1.0.0", BTreeMap::new(), 1)),
            (plugin, tree_node("plugin@1.0.0", BTreeMap::new(), 1)),
            (bundle, tree_node("bundle@1.0.0", bundle_children, 0)),
        ]),
        all_peer_dep_names: HashSet::from(["parser".to_string(), "ts".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::new(),
        children_by_id: HashMap::new(),
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

#[test]
fn previously_resolved_children_prefers_closest_same_package_ancestor() {
    let far_parent = NodeId::next();
    let close_parent = NodeId::next();
    let far_child = NodeId::leaf("shared@1.0.0");
    let close_child = NodeId::leaf("shared@2.0.0");

    let mut far_children = BTreeMap::new();
    far_children.insert("shared".to_string(), far_child);
    let mut close_children = BTreeMap::new();
    close_children.insert("shared".to_string(), close_child.clone());

    let mut tree = ResolvedTree {
        direct: Vec::new(),
        packages: HashMap::from([("loop@1.0.0".to_string(), package("loop", "1.0.0", &[], false))]),
        dependencies_tree: HashMap::from([
            (far_parent.clone(), tree_node("loop@1.0.0", far_children, 0)),
            (close_parent.clone(), tree_node("loop@1.0.0", close_children, 2)),
        ]),
        all_peer_dep_names: HashSet::new(),
        policy_violations: Vec::new(),
        applied_patches: HashSet::new(),
        children_by_id: HashMap::new(),
    };
    let mut walker = walker_for_tests(&mut tree);

    let children = walker.previously_resolved_children(
        &[far_parent, close_parent],
        &["loop@1.0.0".to_string()],
        "loop@1.0.0",
    );

    assert_eq!(children.get("shared"), Some(&close_child));
}

#[test]
fn final_graph_keeps_first_equal_depth_payload_and_unions_transitive_peers() {
    let first = NodeId::next();
    let second = NodeId::next();
    let first_child = NodeId::leaf("child-a@1.0.0");
    let second_child = NodeId::leaf("child-b@1.0.0");
    let final_dep_path = DepPath::from("same@1.0.0(peer@1.0.0)");

    let mut tree = ResolvedTree {
        direct: Vec::new(),
        packages: HashMap::from([
            ("same@1.0.0".to_string(), package("same", "1.0.0", &[("peer", "*")], false)),
            ("child-a@1.0.0".to_string(), package("child-a", "1.0.0", &[], true)),
            ("child-b@1.0.0".to_string(), package("child-b", "1.0.0", &[], true)),
        ]),
        dependencies_tree: HashMap::from([
            (first.clone(), tree_node("same@1.0.0", BTreeMap::new(), 1)),
            (second.clone(), tree_node("same@1.0.0", BTreeMap::new(), 1)),
            (first_child.clone(), tree_node("child-a@1.0.0", BTreeMap::new(), 2)),
            (second_child.clone(), tree_node("child-b@1.0.0", BTreeMap::new(), 2)),
        ]),
        all_peer_dep_names: HashSet::from(["peer".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::new(),
        children_by_id: HashMap::new(),
    };
    let mut walker = walker_for_tests(&mut tree);
    walker.node_dep_paths.insert(first.clone(), final_dep_path.clone());
    walker.node_dep_paths.insert(second.clone(), final_dep_path.clone());
    walker.node_dep_paths.insert(first_child.clone(), DepPath::from("child-a@1.0.0"));
    walker.node_dep_paths.insert(second_child.clone(), DepPath::from("child-b@1.0.0"));
    walker.node_records.insert(
        second.clone(),
        NodeRecord {
            edges: BTreeMap::from([("peer".to_string(), second_child)]),
            peer_edges: HashSet::new(),
            optional_child_aliases: HashSet::new(),
            transitive_peer_dependencies: HashSet::from(["debug".to_string()]),
            depth: 1,
            installable: true,
            is_pure: false,
            order: 1,
        },
    );
    walker.node_records.insert(
        first.clone(),
        NodeRecord {
            edges: BTreeMap::from([("peer".to_string(), first_child)]),
            peer_edges: HashSet::new(),
            optional_child_aliases: HashSet::new(),
            transitive_peer_dependencies: HashSet::new(),
            depth: 1,
            installable: true,
            is_pure: false,
            order: 0,
        },
    );

    let graph = walker.build_final_graph(&HashMap::from([
        (first, final_dep_path.clone()),
        (second, final_dep_path.clone()),
    ]));

    assert_eq!(graph[&final_dep_path].children.get("peer"), Some(&DepPath::from("child-a@1.0.0")));
    assert!(graph[&final_dep_path].transitive_peer_dependencies.contains("debug"));
}

#[test]
fn final_graph_duplicate_parent_prefers_child_variant_matching_parent_peers() {
    let first = NodeId::next();
    let second = NodeId::next();
    let first_child = NodeId::next();
    let second_child = NodeId::next();
    let parent_dep_path = DepPath::from(
        "consumer@1.0.0(webpack-cli@6.0.1)(webpack-dev-server@5.2.2)(webpack@5.107.2)",
    );
    let analyzer_child_dep_path = DepPath::from(
        "webpack-cli@6.0.1(webpack-bundle-analyzer@4.10.2)(webpack-dev-server@5.2.2)(webpack@5.107.2)",
    );
    let matching_child_dep_path =
        DepPath::from("webpack-cli@6.0.1(webpack-dev-server@5.2.2)(webpack@5.107.2)");

    let mut tree = ResolvedTree {
        direct: Vec::new(),
        packages: HashMap::from([(
            "consumer@1.0.0".to_string(),
            package("consumer", "1.0.0", &[], false),
        )]),
        dependencies_tree: HashMap::from([
            (first.clone(), tree_node("consumer@1.0.0", BTreeMap::new(), 1)),
            (second.clone(), tree_node("consumer@1.0.0", BTreeMap::new(), 1)),
        ]),
        all_peer_dep_names: HashSet::new(),
        policy_violations: Vec::new(),
        applied_patches: HashSet::new(),
        children_by_id: HashMap::new(),
    };
    let mut walker = walker_for_tests(&mut tree);
    walker.node_dep_paths.insert(first.clone(), parent_dep_path.clone());
    walker.node_dep_paths.insert(second.clone(), parent_dep_path.clone());
    walker.node_records.insert(
        first.clone(),
        NodeRecord {
            edges: BTreeMap::from([("webpack-cli".to_string(), first_child.clone())]),
            peer_edges: HashSet::new(),
            optional_child_aliases: HashSet::new(),
            transitive_peer_dependencies: HashSet::new(),
            depth: 1,
            installable: true,
            is_pure: false,
            order: 0,
        },
    );
    walker.node_records.insert(
        second.clone(),
        NodeRecord {
            edges: BTreeMap::from([("webpack-cli".to_string(), second_child.clone())]),
            peer_edges: HashSet::new(),
            optional_child_aliases: HashSet::new(),
            transitive_peer_dependencies: HashSet::new(),
            depth: 1,
            installable: true,
            is_pure: false,
            order: 1,
        },
    );

    let graph = walker.build_final_graph(&HashMap::from([
        (first, parent_dep_path.clone()),
        (second, parent_dep_path.clone()),
        (first_child, analyzer_child_dep_path),
        (second_child, matching_child_dep_path.clone()),
    ]));

    assert_eq!(graph[&parent_dep_path].children.get("webpack-cli"), Some(&matching_child_dep_path));
}

#[test]
fn final_graph_peer_edge_uses_provider_variant_without_unavailable_extra_peers() {
    let provider_analyzer = NodeId::next();
    let provider_bare = NodeId::next();
    let consumer = NodeId::next();

    let provider_analyzer_dep_path = DepPath::from(
        "webpack-cli@6.0.1(webpack-bundle-analyzer@4.10.2)(webpack-dev-server@5.2.2)(webpack@5.107.2)",
    );
    let provider_middle_dep_path =
        DepPath::from("webpack-cli@6.0.1(webpack-dev-server@5.2.2)(webpack@5.107.2)");
    let provider_bare_dep_path = DepPath::from("webpack-cli@6.0.1(webpack@5.107.2)");
    let consumer_dep_path = DepPath::from(
        "@webpack-cli/serve@3.0.1(webpack-cli@6.0.1)(webpack-dev-server@5.2.2)(webpack@5.107.2)",
    );

    let mut tree = ResolvedTree {
        direct: Vec::new(),
        packages: HashMap::from([
            (
                "webpack-cli@6.0.1".to_string(),
                package(
                    "webpack-cli",
                    "6.0.1",
                    &[
                        ("webpack", "*"),
                        ("webpack-dev-server", "*"),
                        ("webpack-bundle-analyzer", "*"),
                    ],
                    false,
                ),
            ),
            (
                "@webpack-cli/serve@3.0.1".to_string(),
                package(
                    "@webpack-cli/serve",
                    "3.0.1",
                    &[("webpack", "*"), ("webpack-cli", "*"), ("webpack-dev-server", "*")],
                    false,
                ),
            ),
        ]),
        dependencies_tree: HashMap::from([
            (provider_analyzer.clone(), tree_node("webpack-cli@6.0.1", BTreeMap::new(), 0)),
            (provider_bare.clone(), tree_node("webpack-cli@6.0.1", BTreeMap::new(), 0)),
            (consumer.clone(), tree_node("@webpack-cli/serve@3.0.1", BTreeMap::new(), 1)),
        ]),
        all_peer_dep_names: HashSet::from([
            "webpack".to_string(),
            "webpack-cli".to_string(),
            "webpack-dev-server".to_string(),
            "webpack-bundle-analyzer".to_string(),
        ]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::new(),
        children_by_id: HashMap::new(),
    };
    let mut walker = walker_for_tests(&mut tree);
    walker.node_records.insert(
        provider_analyzer.clone(),
        NodeRecord {
            edges: BTreeMap::new(),
            peer_edges: HashSet::new(),
            optional_child_aliases: HashSet::new(),
            transitive_peer_dependencies: HashSet::new(),
            depth: 0,
            installable: true,
            is_pure: false,
            order: 0,
        },
    );
    walker.node_records.insert(
        provider_bare.clone(),
        NodeRecord {
            edges: BTreeMap::new(),
            peer_edges: HashSet::new(),
            optional_child_aliases: HashSet::new(),
            transitive_peer_dependencies: HashSet::new(),
            depth: 0,
            installable: true,
            is_pure: false,
            order: 1,
        },
    );
    walker.node_records.insert(
        consumer.clone(),
        NodeRecord {
            edges: BTreeMap::from([("webpack-cli".to_string(), provider_analyzer.clone())]),
            peer_edges: HashSet::from(["webpack-dev-server".to_string(), "webpack".to_string()]),
            optional_child_aliases: HashSet::new(),
            transitive_peer_dependencies: HashSet::new(),
            depth: 1,
            installable: true,
            is_pure: false,
            order: 2,
        },
    );
    walker.node_external_peers.insert(
        provider_analyzer.clone(),
        HashMap::from([
            ("webpack-bundle-analyzer".to_string(), NodeId::leaf("webpack-bundle-analyzer@4.10.2")),
            ("webpack-dev-server".to_string(), NodeId::leaf("webpack-dev-server@5.2.2")),
            ("webpack".to_string(), NodeId::leaf("webpack@5.107.2")),
        ]),
    );
    walker.node_external_peers.insert(
        provider_bare.clone(),
        HashMap::from([("webpack".to_string(), NodeId::leaf("webpack@5.107.2"))]),
    );
    walker.node_external_peers.insert(
        consumer.clone(),
        HashMap::from([
            ("webpack-bundle-analyzer".to_string(), NodeId::leaf("webpack-bundle-analyzer@4.10.2")),
            ("webpack-cli".to_string(), provider_analyzer.clone()),
            ("webpack-dev-server".to_string(), NodeId::leaf("webpack-dev-server@5.2.2")),
            ("webpack".to_string(), NodeId::leaf("webpack@5.107.2")),
        ]),
    );

    let graph = walker.build_final_graph(&HashMap::from([
        (provider_analyzer, provider_analyzer_dep_path),
        (provider_bare, provider_bare_dep_path),
        (consumer, consumer_dep_path.clone()),
    ]));

    assert_eq!(
        graph[&consumer_dep_path].children.get("webpack-cli"),
        Some(&provider_middle_dep_path),
    );
}

#[test]
fn final_graph_peer_edge_keeps_provider_transitive_peer_suffixes() {
    let provider = NodeId::next();
    let consumer = NodeId::next();

    let provider_dep_path = DepPath::from(
        "webpack-dev-server@5.2.2(bufferutil@4.1.0)(tslib@2.8.1)(utf-8-validate@5.0.10)(webpack-cli@6.0.1)(webpack@5.107.2)",
    );
    let trimmed_provider_dep_path =
        DepPath::from("webpack-dev-server@5.2.2(webpack-cli@6.0.1)(webpack@5.107.2)");
    let consumer_dep_path = DepPath::from(
        "webpack-cli@6.0.1(webpack-bundle-analyzer@4.10.2)(webpack-dev-server@5.2.2)(webpack@5.107.2)",
    );

    let mut tree = ResolvedTree {
        direct: Vec::new(),
        packages: HashMap::from([
            (
                "webpack-dev-server@5.2.2".to_string(),
                package(
                    "webpack-dev-server",
                    "5.2.2",
                    &[("webpack", "*"), ("webpack-cli", "*")],
                    false,
                ),
            ),
            (
                "webpack-cli@6.0.1".to_string(),
                package(
                    "webpack-cli",
                    "6.0.1",
                    &[
                        ("webpack", "*"),
                        ("webpack-bundle-analyzer", "*"),
                        ("webpack-dev-server", "*"),
                    ],
                    false,
                ),
            ),
        ]),
        dependencies_tree: HashMap::from([
            (provider.clone(), tree_node("webpack-dev-server@5.2.2", BTreeMap::new(), 1)),
            (consumer.clone(), tree_node("webpack-cli@6.0.1", BTreeMap::new(), 0)),
        ]),
        all_peer_dep_names: HashSet::from([
            "bufferutil".to_string(),
            "tslib".to_string(),
            "utf-8-validate".to_string(),
            "webpack".to_string(),
            "webpack-cli".to_string(),
            "webpack-dev-server".to_string(),
            "webpack-bundle-analyzer".to_string(),
        ]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::new(),
        children_by_id: HashMap::new(),
    };
    let mut walker = walker_for_tests(&mut tree);
    walker.node_records.insert(
        provider.clone(),
        NodeRecord {
            edges: BTreeMap::new(),
            peer_edges: HashSet::new(),
            optional_child_aliases: HashSet::new(),
            transitive_peer_dependencies: HashSet::from([
                "bufferutil".to_string(),
                "tslib".to_string(),
                "utf-8-validate".to_string(),
            ]),
            depth: 1,
            installable: true,
            is_pure: false,
            order: 0,
        },
    );
    walker.node_records.insert(
        consumer.clone(),
        NodeRecord {
            edges: BTreeMap::from([("webpack-dev-server".to_string(), provider.clone())]),
            peer_edges: HashSet::from([
                "webpack".to_string(),
                "webpack-bundle-analyzer".to_string(),
                "webpack-dev-server".to_string(),
            ]),
            optional_child_aliases: HashSet::new(),
            transitive_peer_dependencies: HashSet::new(),
            depth: 0,
            installable: true,
            is_pure: false,
            order: 1,
        },
    );
    walker.node_external_peers.insert(
        provider.clone(),
        HashMap::from([
            ("bufferutil".to_string(), NodeId::leaf("bufferutil@4.1.0")),
            ("tslib".to_string(), NodeId::leaf("tslib@2.8.1")),
            ("utf-8-validate".to_string(), NodeId::leaf("utf-8-validate@5.0.10")),
            ("webpack-cli".to_string(), consumer.clone()),
            ("webpack".to_string(), NodeId::leaf("webpack@5.107.2")),
        ]),
    );
    walker.node_external_peers.insert(
        consumer.clone(),
        HashMap::from([
            ("webpack-bundle-analyzer".to_string(), NodeId::leaf("webpack-bundle-analyzer@4.10.2")),
            ("webpack-dev-server".to_string(), provider.clone()),
            ("webpack".to_string(), NodeId::leaf("webpack@5.107.2")),
        ]),
    );

    let graph = walker.build_final_graph(&HashMap::from([
        (provider, provider_dep_path.clone()),
        (consumer, consumer_dep_path.clone()),
    ]));

    assert_eq!(
        graph[&consumer_dep_path].children.get("webpack-dev-server"),
        Some(&provider_dep_path),
    );
    assert!(!graph.contains_key(&trimmed_provider_dep_path));
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
            packages: HashMap::from([
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
            dependencies_tree: HashMap::from([
                (babel, tree_node("@babel/core@7.0.0", BTreeMap::new(), 1)),
                (styled_shallow, tree_node("styled-jsx@1.0.0", BTreeMap::new(), 1)),
                (styled_deep, tree_node("styled-jsx@1.0.0", BTreeMap::new(), 2)),
                (app, tree_node("app@1.0.0", app_children, 0)),
                (mid, tree_node("mid@1.0.0", mid_children, 1)),
            ]),
            all_peer_dep_names: HashSet::from(["@babel/core".to_string()]),
            policy_violations: Vec::new(),
            applied_patches: HashSet::new(),
            children_by_id: HashMap::new(),
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

fn tree_node(pkg_id: &str, children: BTreeMap<String, NodeId>, depth: i32) -> DependenciesTreeNode {
    DependenciesTreeNode {
        resolved_package_id: pkg_id.to_string(),
        children: TreeChildren::Realized(children),
        depth,
        installable: true,
    }
}

fn walker_for_tests(tree: &mut ResolvedTree) -> Walker<'_> {
    Walker {
        tree,
        opts: ResolvePeersOptions::default(),
        graph: DependenciesGraph::new(),
        issues: PeerDependencyIssues::default(),
        node_dep_paths: HashMap::new(),
        node_external_peers: HashMap::new(),
        node_missing_peers: HashMap::new(),
        node_missing_peers_of_children: HashMap::new(),
        resolved_peer_providers_by_alias: BTreeMap::new(),
        in_progress: HashSet::new(),
        pending_peer_edges: Vec::new(),
        pure_pkgs: HashSet::new(),
        peers_cache: HashMap::new(),
        parent_pkgs_of_node: HashMap::new(),
        node_records: HashMap::new(),
        next_record_order: 0,
    }
}

fn package(
    name: &str,
    version: &str,
    peer_dependencies: &[(&str, &str)],
    is_leaf: bool,
) -> ResolvedPackage {
    let peer_dependencies: Vec<_> =
        peer_dependencies.iter().map(|(name, version)| (*name, *version, false)).collect();
    package_with_peer_dependencies(name, version, &peer_dependencies, is_leaf)
}

fn package_with_peer_dependencies(
    name: &str,
    version: &str,
    peer_dependencies: &[(&str, &str, bool)],
    is_leaf: bool,
) -> ResolvedPackage {
    let peer_dependencies = peer_dependencies
        .iter()
        .map(|(name, version, optional)| {
            (
                (*name).to_string(),
                PeerDep { version: (*version).to_string(), optional: *optional, meta_only: false },
            )
        })
        .collect();
    ResolvedPackage {
        id: format!("{name}@{version}"),
        result: Arc::new(resolve_result(name, version)),
        peer_dependencies,
        optional: false,
        is_leaf,
    }
}

fn resolve_result(name: &str, version: &str) -> ResolveResult {
    let name_ver = PkgNameVer::new(
        PkgName::parse(name).unwrap(),
        node_semver::Version::from_str(version).unwrap(),
    );
    ResolveResult {
        id: (&name_ver).into(),
        name_ver: Some(name_ver),
        latest: Some(version.to_string()),
        published_at: None,
        manifest: None,
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: format!("https://registry.example/{name}-{version}.tgz"),
            integrity: None,
            git_hosted: None,
            path: None,
        }),
        resolved_via: "npm-registry".to_string(),
        normalized_bare_specifier: None,
        alias: Some(name.to_string()),
        policy_violation: None,
    }
}
