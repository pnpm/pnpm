//! Unit tests for the post-walk depPath and graph passes.

use crate::{
    node_id::NodeId,
    resolve_peers::{
        finalize::NodeRecord,
        test_support::{package, tree_node, walker_for_tests},
    },
    resolved_tree::ResolvedTree,
};
use pnpm_deps_path::DepPath;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[test]
fn final_graph_keeps_first_equal_depth_payload_and_unions_transitive_peers() {
    let first = NodeId::next();
    let second = NodeId::next();
    let first_child = NodeId::leaf("child-a@1.0.0");
    let second_child = NodeId::leaf("child-b@1.0.0");
    let final_dep_path = DepPath::from("same@1.0.0(peer@1.0.0)");

    let mut tree = ResolvedTree {
        direct: Vec::new(),
        packages: HashMap::from_iter([
            ("same@1.0.0".into(), package("same", "1.0.0", &[("peer", "*")], false)),
            ("child-a@1.0.0".into(), package("child-a", "1.0.0", &[], true)),
            ("child-b@1.0.0".into(), package("child-b", "1.0.0", &[], true)),
        ]),
        dependencies_tree: HashMap::from_iter([
            (first.clone(), tree_node("same@1.0.0", BTreeMap::new(), 1)),
            (second.clone(), tree_node("same@1.0.0", BTreeMap::new(), 1)),
            (first_child.clone(), tree_node("child-a@1.0.0", BTreeMap::new(), 2)),
            (second_child.clone(), tree_node("child-b@1.0.0", BTreeMap::new(), 2)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["peer".to_string()]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
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
            optional_child_aliases: HashSet::default(),
            transitive_peer_dependencies: HashSet::from_iter(["debug".to_string()]),
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
            optional_child_aliases: HashSet::default(),
            transitive_peer_dependencies: HashSet::default(),
            depth: 1,
            installable: true,
            is_pure: false,
            order: 0,
        },
    );

    let graph = walker.build_final_graph(&HashMap::from_iter([
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
        packages: HashMap::from_iter([(
            "consumer@1.0.0".into(),
            package("consumer", "1.0.0", &[], false),
        )]),
        dependencies_tree: HashMap::from_iter([
            (first.clone(), tree_node("consumer@1.0.0", BTreeMap::new(), 1)),
            (second.clone(), tree_node("consumer@1.0.0", BTreeMap::new(), 1)),
        ]),
        all_peer_dep_names: HashSet::default(),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };
    let mut walker = walker_for_tests(&mut tree);
    walker.node_dep_paths.insert(first.clone(), parent_dep_path.clone());
    walker.node_dep_paths.insert(second.clone(), parent_dep_path.clone());
    walker.node_records.insert(
        first.clone(),
        NodeRecord {
            edges: BTreeMap::from([("webpack-cli".to_string(), first_child.clone())]),
            optional_child_aliases: HashSet::default(),
            transitive_peer_dependencies: HashSet::default(),
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
            optional_child_aliases: HashSet::default(),
            transitive_peer_dependencies: HashSet::default(),
            depth: 1,
            installable: true,
            is_pure: false,
            order: 1,
        },
    );

    let graph = walker.build_final_graph(&HashMap::from_iter([
        (first, parent_dep_path.clone()),
        (second, parent_dep_path.clone()),
        (first_child, analyzer_child_dep_path),
        (second_child, matching_child_dep_path.clone()),
    ]));

    assert_eq!(graph[&parent_dep_path].children.get("webpack-cli"), Some(&matching_child_dep_path));
}

#[test]
fn final_graph_peer_edge_keeps_the_providers_own_peer_suffix() {
    let provider_analyzer = NodeId::next();
    let provider_bare = NodeId::next();
    let consumer = NodeId::next();
    // A shallower `find_hit` revisit: short-circuited before a `NodeRecord`
    // was created, so only `node_dep_paths` carries its depth.
    let consumer_revisit = NodeId::next();

    let provider_analyzer_dep_path = DepPath::from(
        "webpack-cli@6.0.1(webpack-bundle-analyzer@4.10.2)(webpack-dev-server@5.2.2)(webpack@5.107.2)",
    );
    let trimmed_dep_path =
        DepPath::from("webpack-cli@6.0.1(webpack-dev-server@5.2.2)(webpack@5.107.2)");
    let provider_bare_dep_path = DepPath::from("webpack-cli@6.0.1(webpack@5.107.2)");
    let consumer_dep_path = DepPath::from(
        "@webpack-cli/serve@3.0.1(webpack-cli@6.0.1)(webpack-dev-server@5.2.2)(webpack@5.107.2)",
    );

    let mut tree = ResolvedTree {
        direct: Vec::new(),
        packages: HashMap::from_iter([
            (
                "webpack-cli@6.0.1".into(),
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
                "@webpack-cli/serve@3.0.1".into(),
                package(
                    "@webpack-cli/serve",
                    "3.0.1",
                    &[("webpack", "*"), ("webpack-cli", "*"), ("webpack-dev-server", "*")],
                    false,
                ),
            ),
        ]),
        dependencies_tree: HashMap::from_iter([
            (provider_analyzer.clone(), tree_node("webpack-cli@6.0.1", BTreeMap::new(), 0)),
            (provider_bare.clone(), tree_node("webpack-cli@6.0.1", BTreeMap::new(), 0)),
            (consumer.clone(), tree_node("@webpack-cli/serve@3.0.1", BTreeMap::new(), 1)),
            (consumer_revisit.clone(), tree_node("@webpack-cli/serve@3.0.1", BTreeMap::new(), 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter([
            "webpack".into(),
            "webpack-cli".into(),
            "webpack-dev-server".into(),
            "webpack-bundle-analyzer".into(),
        ]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };
    let mut walker = walker_for_tests(&mut tree);
    walker.node_dep_paths.insert(provider_analyzer.clone(), provider_analyzer_dep_path.clone());
    walker.node_dep_paths.insert(provider_bare.clone(), provider_bare_dep_path.clone());
    walker.node_dep_paths.insert(consumer.clone(), consumer_dep_path.clone());
    walker.node_dep_paths.insert(consumer_revisit.clone(), consumer_dep_path.clone());
    walker.node_records.insert(
        provider_analyzer.clone(),
        NodeRecord {
            edges: BTreeMap::new(),
            optional_child_aliases: HashSet::default(),
            transitive_peer_dependencies: HashSet::default(),
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
            optional_child_aliases: HashSet::default(),
            transitive_peer_dependencies: HashSet::default(),
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
            optional_child_aliases: HashSet::default(),
            transitive_peer_dependencies: HashSet::default(),
            depth: 1,
            installable: true,
            is_pure: false,
            order: 2,
        },
    );
    walker.node_external_peers.insert(
        provider_analyzer.clone(),
        Arc::new(HashMap::from_iter([
            ("webpack-bundle-analyzer".to_string(), NodeId::leaf("webpack-bundle-analyzer@4.10.2")),
            ("webpack-dev-server".to_string(), NodeId::leaf("webpack-dev-server@5.2.2")),
            ("webpack".to_string(), NodeId::leaf("webpack@5.107.2")),
        ])),
    );
    walker.node_external_peers.insert(
        provider_bare.clone(),
        Arc::new(HashMap::from_iter([("webpack".to_string(), NodeId::leaf("webpack@5.107.2"))])),
    );
    walker.node_external_peers.insert(
        consumer.clone(),
        Arc::new(HashMap::from_iter([
            ("webpack-bundle-analyzer".to_string(), NodeId::leaf("webpack-bundle-analyzer@4.10.2")),
            ("webpack-cli".to_string(), provider_analyzer.clone()),
            ("webpack-dev-server".to_string(), NodeId::leaf("webpack-dev-server@5.2.2")),
            ("webpack".to_string(), NodeId::leaf("webpack@5.107.2")),
        ])),
    );

    let graph = walker.build_final_graph(&HashMap::from_iter([
        (provider_analyzer, provider_analyzer_dep_path.clone()),
        (provider_bare, provider_bare_dep_path),
        (consumer, consumer_dep_path.clone()),
        (consumer_revisit, consumer_dep_path.clone()),
    ]));

    assert_eq!(
        graph[&consumer_dep_path].children.get("webpack-cli"),
        Some(&provider_analyzer_dep_path),
    );
    assert_eq!(graph[&consumer_dep_path].depth, 0);
    assert!(
        !graph.contains_key(&trimmed_dep_path),
        "no variant is fabricated for the edge; final graph keys: {:#?}",
        graph.keys().collect::<BTreeSet<_>>(),
    );
}

#[test]
fn final_graph_peer_edge_keeps_provider_transitive_peer_suffixes() {
    let provider = NodeId::next();
    let consumer = NodeId::next();
    // A shallower `find_hit` revisit: short-circuited before a `NodeRecord`
    // was created, so only `node_dep_paths` carries its depth.
    let provider_revisit = NodeId::next();

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
        packages: HashMap::from_iter([
            (
                "webpack-dev-server@5.2.2".into(),
                package(
                    "webpack-dev-server",
                    "5.2.2",
                    &[("webpack", "*"), ("webpack-cli", "*")],
                    false,
                ),
            ),
            (
                "webpack-cli@6.0.1".into(),
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
        dependencies_tree: HashMap::from_iter([
            (provider.clone(), tree_node("webpack-dev-server@5.2.2", BTreeMap::new(), 1)),
            (provider_revisit.clone(), tree_node("webpack-dev-server@5.2.2", BTreeMap::new(), 0)),
            (consumer.clone(), tree_node("webpack-cli@6.0.1", BTreeMap::new(), 0)),
        ]),
        all_peer_dep_names: HashSet::from_iter([
            "bufferutil".into(),
            "tslib".into(),
            "utf-8-validate".into(),
            "webpack".into(),
            "webpack-cli".into(),
            "webpack-dev-server".into(),
            "webpack-bundle-analyzer".into(),
        ]),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::default(),
    };
    let mut walker = walker_for_tests(&mut tree);
    walker.node_dep_paths.insert(provider.clone(), provider_dep_path.clone());
    walker.node_dep_paths.insert(provider_revisit.clone(), provider_dep_path.clone());
    walker.node_dep_paths.insert(consumer.clone(), consumer_dep_path.clone());
    walker.node_records.insert(
        provider.clone(),
        NodeRecord {
            edges: BTreeMap::new(),
            optional_child_aliases: HashSet::default(),
            transitive_peer_dependencies: HashSet::from_iter([
                "bufferutil".into(),
                "tslib".into(),
                "utf-8-validate".into(),
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
            optional_child_aliases: HashSet::default(),
            transitive_peer_dependencies: HashSet::default(),
            depth: 0,
            installable: true,
            is_pure: false,
            order: 1,
        },
    );
    walker.node_external_peers.insert(
        provider.clone(),
        Arc::new(HashMap::from_iter([
            ("bufferutil".to_string(), NodeId::leaf("bufferutil@4.1.0")),
            ("tslib".to_string(), NodeId::leaf("tslib@2.8.1")),
            ("utf-8-validate".to_string(), NodeId::leaf("utf-8-validate@5.0.10")),
            ("webpack-cli".to_string(), consumer.clone()),
            ("webpack".to_string(), NodeId::leaf("webpack@5.107.2")),
        ])),
    );
    walker.node_external_peers.insert(
        consumer.clone(),
        Arc::new(HashMap::from_iter([
            ("webpack-bundle-analyzer".to_string(), NodeId::leaf("webpack-bundle-analyzer@4.10.2")),
            ("webpack-dev-server".to_string(), provider.clone()),
            ("webpack".to_string(), NodeId::leaf("webpack@5.107.2")),
        ])),
    );

    let graph = walker.build_final_graph(&HashMap::from_iter([
        (provider, provider_dep_path.clone()),
        (provider_revisit, provider_dep_path.clone()),
        (consumer, consumer_dep_path.clone()),
    ]));

    assert_eq!(
        graph[&consumer_dep_path].children.get("webpack-dev-server"),
        Some(&provider_dep_path),
    );
    assert_eq!(graph[&provider_dep_path].depth, 0);
    assert!(
        !graph.contains_key(&trimmed_provider_dep_path),
        "no trimmed provider variant is fabricated; final graph keys: {:#?}",
        graph.keys().collect::<BTreeSet<_>>(),
    );
}
