use super::{
    Arc, BTreeMap, DepPath, DirectDep, HashMap, HashSet, NodeId, ResolvePeersOptions, ResolvedTree,
    assert_cyclic_alias_peer_graph_is_closed, cyclic_alias_peer_tree, named_registry_peer_tree,
    package, package_with_peer_dependencies, resolve_peers, tree_node,
};

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
