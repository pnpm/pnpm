use super::{
    dependencies_graph_to_lockfile, make_named_registry_node, make_node, make_node_with_optional,
    make_resolve_result, single_importer_opts, write_manifest,
};
use pnpm_deps_path::DepPath;
use pnpm_lockfile::{ImporterDepVersion, LockfileResolution, PackageKey, PkgName};
use pnpm_resolving_deps_resolver::{
    ChildEdge, DependenciesGraph, DependenciesGraphNode, DependenciesTreeNode, DirectDep, NodeId,
    PeerDep, ResolvePeersOptions, ResolvedPackage, ResolvedTree, TreeChildren, resolve_peers,
};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

#[test]
fn peer_suffixed_dep_path_splits_into_distinct_snapshot_and_package_keys() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": {
            "react": "^17.0.2",
            "react-dom": "^17.0.2",
        },
    }));

    let react = make_node(
        "react",
        "17.0.2",
        json!({ "name": "react", "version": "17.0.2" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::default(),
    );

    let mut react_dom_children = BTreeMap::new();
    react_dom_children.insert("react".to_string(), DepPath::from("react@17.0.2".to_string()));
    let mut react_dom_peers = BTreeMap::new();
    react_dom_peers
        .insert("react".to_string(), PeerDep { version: "17.0.2".to_string(), optional: false });
    let react_dom_dep_path = DepPath::from("react-dom@17.0.2(react@17.0.2)".to_string());
    let react_dom = DependenciesGraphNode {
        dep_path: react_dom_dep_path.clone(),
        resolved_package_id: "react-dom@17.0.2".to_string(),
        resolve_result: std::sync::Arc::new(make_resolve_result(
            "react-dom",
            "17.0.2",
            json!({
                "name": "react-dom",
                "version": "17.0.2",
                "peerDependencies": { "react": "17.0.2" },
            }),
        )),
        children: react_dom_children,
        optional_children: HashSet::default(),
        peer_dependencies: react_dom_peers,
        transitive_peer_dependencies: HashSet::default(),
        resolved_peer_names: std::iter::once("react".to_string()).collect(),
        depth: 1,
        installable: true,
        is_pure: false,
        optional: false,
    };

    let mut graph = DependenciesGraph::default();
    graph.insert(react.dep_path.clone(), react);
    graph.insert(react_dom_dep_path.clone(), react_dom);

    let mut direct = BTreeMap::new();
    direct.insert("react".to_string(), DepPath::from("react@17.0.2".to_string()));
    direct.insert("react-dom".to_string(), react_dom_dep_path);

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, true, false, None, None,
    ));

    let snapshots = lockfile.snapshots.as_ref().expect("snapshots");
    let snap_key: PackageKey = "react-dom@17.0.2(react@17.0.2)".parse().unwrap();
    assert!(snapshots.contains_key(&snap_key), "snapshot keyed by peer-suffixed depPath");
    let pkg_key: PackageKey = "react-dom@17.0.2".parse().unwrap();
    let packages = lockfile.packages.as_ref().expect("packages");
    let metadata = packages.get(&pkg_key).expect("package metadata for peer-stripped key");
    assert!(metadata.peer_dependencies.is_some(), "peer_deps on packages metadata");

    let importer = lockfile.root_project().unwrap();
    let dom =
        importer.dependencies.as_ref().unwrap().get(&PkgName::parse("react-dom").unwrap()).unwrap();
    match &dom.version {
        ImporterDepVersion::Regular(ver) => {
            assert_eq!(ver.to_string(), "17.0.2(react@17.0.2)");
        }
        other => panic!("expected Regular(...), got {other:?}"),
    }
}
#[test]
fn snapshot_preserves_optional_child_edges_from_resolved_tree() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "outer": "^1.0.0" },
    }));

    let outer_id: Arc<str> = "outer@1.0.0".into();
    let inner_id: Arc<str> = "inner@1.0.0".into();
    let outer_node_id = NodeId::next();

    let mut tree = ResolvedTree {
        direct: vec![DirectDep {
            alias: "outer".to_string(),
            node_id: outer_node_id.clone(),
            id: outer_id.to_string(),
        }],
        packages: HashMap::from_iter([
            (
                Arc::<str>::clone(&outer_id),
                ResolvedPackage {
                    id: Arc::<str>::clone(&outer_id),
                    result: Arc::new(make_resolve_result(
                        "outer",
                        "1.0.0",
                        json!({ "name": "outer", "version": "1.0.0" }),
                    )),
                    peer_dependencies: BTreeMap::new(),
                    optional: false,
                    is_leaf: false,
                },
            ),
            (
                Arc::<str>::clone(&inner_id),
                ResolvedPackage {
                    id: Arc::<str>::clone(&inner_id),
                    result: Arc::new(make_resolve_result(
                        "inner",
                        "1.0.0",
                        json!({ "name": "inner", "version": "1.0.0" }),
                    )),
                    peer_dependencies: BTreeMap::new(),
                    optional: true,
                    is_leaf: true,
                },
            ),
        ]),
        dependencies_tree: HashMap::from_iter([(
            outer_node_id,
            DependenciesTreeNode::new(
                Arc::<str>::clone(&outer_id),
                TreeChildren::Lazy { parent_ids: Arc::new(Vec::new()).into() },
                0,
                true,
            ),
        )]),
        all_peer_dep_names: HashSet::default(),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::from_iter([(
            outer_id,
            Arc::new(vec![ChildEdge {
                alias: "inner".to_string(),
                pkg_id: inner_id,
                optional: true,
            }]),
        )]),
    };

    let resolved = resolve_peers(&mut tree, ResolvePeersOptions::default());
    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest,
        &resolved.graph,
        resolved.direct_dependencies_by_alias,
        false,
        false,
        None,
        None,
    ));

    let snapshots = lockfile.snapshots.as_ref().unwrap();
    let outer_key: PackageKey = "outer@1.0.0".parse().unwrap();
    let outer_snap = &snapshots[&outer_key];
    assert!(outer_snap.dependencies.is_none(), "optional child must not be written as regular");
    let opt = outer_snap.optional_dependencies.as_ref().expect("opt deps map");
    assert!(opt.contains_key(&PkgName::parse("inner").unwrap()));

    let inner_key: PackageKey = "inner@1.0.0".parse().unwrap();
    assert!(snapshots[&inner_key].optional, "optional child edge keeps the child optional");
}
#[test]
fn snapshot_records_transitive_peer_dependencies_sorted() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "outer": "^1.0.0" },
    }));

    let mut transitive: HashSet<String> = HashSet::default();
    transitive.insert("z-peer".to_string());
    transitive.insert("a-peer".to_string());
    let outer = make_node(
        "outer",
        "1.0.0",
        json!({ "name": "outer", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        transitive,
    );
    let mut graph = DependenciesGraph::default();
    graph.insert(outer.dep_path.clone(), outer);

    let mut direct = BTreeMap::new();
    direct.insert("outer".to_string(), DepPath::from("outer@1.0.0".to_string()));

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, true, false, None, None,
    ));

    let snapshots = lockfile.snapshots.as_ref().unwrap();
    let outer_key: PackageKey = "outer@1.0.0".parse().unwrap();
    let recorded = snapshots[&outer_key]
        .transitive_peer_dependencies
        .as_ref()
        .expect("transitive peers recorded");
    assert_eq!(recorded.as_slice(), ["a-peer".to_string(), "z-peer".to_string()].as_slice());
}
#[test]
fn auto_installed_peer_not_declared_in_manifest_is_skipped_from_pruner_seeds() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "optionalDependencies": { "parent": "^1.0.0" },
    }));

    let peer_x = make_node_with_optional(
        "peer-x",
        "1.0.0",
        json!({ "name": "peer-x", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::default(),
        true,
    );

    let mut parent_children = BTreeMap::new();
    parent_children.insert("peer-x".to_string(), DepPath::from("peer-x@1.0.0".to_string()));
    let parent = make_node_with_optional(
        "parent",
        "1.0.0",
        json!({
            "name": "parent",
            "version": "1.0.0",
            "dependencies": { "peer-x": "^1.0.0" },
        }),
        parent_children,
        BTreeMap::new(),
        HashSet::default(),
        true,
    );

    let mut graph = DependenciesGraph::default();
    graph.insert(parent.dep_path.clone(), parent);
    graph.insert(peer_x.dep_path.clone(), peer_x);

    let mut direct = BTreeMap::new();
    direct.insert("parent".to_string(), DepPath::from("parent@1.0.0".to_string()));
    direct.insert("peer-x".to_string(), DepPath::from("peer-x@1.0.0".to_string()));

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, true, false, None, None,
    ));

    let snapshots = lockfile.snapshots.as_ref().expect("snapshots map");
    let parent_key: PackageKey = "parent@1.0.0".parse().unwrap();
    let peer_x_key: PackageKey = "peer-x@1.0.0".parse().unwrap();
    assert!(snapshots[&parent_key].optional, "parent is the importer's optional direct dep");
    assert!(
        snapshots[&peer_x_key].optional,
        "auto-installed peer reachable only via parent's optional path stays optional",
    );
}
/// An alias the writer can't resolve must never drop the tarball URL:
/// testing it against the default registry could classify it as
/// reconstructible and leave an entry no install can fetch.
#[test]
fn an_unresolvable_alias_keeps_the_tarball_url() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "foo": "work:1.0.0" },
    }));

    // Canonical under the *default* registry, which is what makes the
    // unguarded fallback drop it.
    let tarball = "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz";
    let node = make_named_registry_node("foo", "work", "1.0.0", tarball);
    let mut graph = DependenciesGraph::default();
    graph.insert(node.dep_path.clone(), node);

    let mut direct = BTreeMap::new();
    direct.insert("foo".to_string(), DepPath::from("foo@work:1.0.0".to_string()));

    // `work` is deliberately absent from the map.
    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, true, false, None, None,
    ));

    let packages = lockfile.packages.as_ref().expect("packages map");
    let key: PackageKey = "foo@work:1.0.0".parse().unwrap();
    let metadata = packages.get(&key).expect("registry-qualified entry");
    match &metadata.resolution {
        LockfileResolution::Tarball(resolution) => assert_eq!(resolution.tarball, tarball),
        other => panic!("an unresolvable alias must keep its tarball URL, got {other:?}"),
    }
}
