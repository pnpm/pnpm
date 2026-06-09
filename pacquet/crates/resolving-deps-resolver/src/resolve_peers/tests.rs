use super::{ResolvePeersOptions, Walker, resolve_peers, satisfies_with_prereleases};
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
    // Mirrors Yarn's `satisfiesWithPrereleases` carve-out: a peer
    // candidate at `18.0.0-rc.1` should satisfy a `^18.0.0` peer
    // requirement. node-semver's default `satisfies` rejects this
    // pairing, so the prerelease-strip retry has to catch it.
    assert!(satisfies_with_prereleases("18.0.0-rc.1", "^18.0.0"));
    assert!(satisfies_with_prereleases("1.2.3-beta.0", "^1.2.0"));
    // Out-of-range prereleases still fail.
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
fn own_peer_is_resolved_from_aliased_child_real_name() {
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
    let dep_path = DepPath::from("consumer@1.0.0(peer-c@2.0.0)");

    assert!(
        result.graph.contains_key(&dep_path),
        "consumer should resolve peer-c from the peer-c1 alias child: {:#?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
    assert_eq!(
        result.graph[&dep_path].children.get("peer-c"),
        Some(&DepPath::from("peer-c@2.0.0")),
    );
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
fn cached_optional_peer_resolution_bubbles_to_later_parent_without_provider() {
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
    let config_dep_path = DepPath::from("config@1.0.0(types@1.0.0)");
    let cli_dep_path = DepPath::from("cli@1.0.0(types@1.0.0)");

    assert_eq!(result.direct_dependencies_by_alias.get("core"), Some(&DepPath::from("core@1.0.0")));
    assert_eq!(result.direct_dependencies_by_alias.get("cli"), Some(&cli_dep_path));
    assert_eq!(result.graph[&cli_dep_path].children.get("config"), Some(&config_dep_path));
    assert!(result.graph[&cli_dep_path].resolved_peer_names.contains("types"));
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
        in_progress: HashSet::new(),
        pending_peer_edges: Vec::new(),
        pure_pkgs: HashSet::new(),
        peers_cache: HashMap::new(),
        parent_pkgs_of_node: HashMap::new(),
        node_records: HashMap::new(),
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
            ((*name).to_string(), PeerDep { version: (*version).to_string(), optional: *optional })
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
