mod cache_equivalence;

use super::{
    super::{WorkspaceResolvePeersResult, test_support::resolve_with_cache_oracle},
    Arc, BTreeMap, DepPath, HashMap, ImporterPeerInput, NodeId, ResolvePeersOptions, ResolvedTree,
    WorkspacePeerSettings, add_lazy_direct_dep, child_edge, package,
    package_with_peer_dependencies, tree_node,
};

use crate::PeerDependentsDedupe;
use rustc_hash::FxHashSet as HashSet;

type ProjectDependencies<'a> = &'a [(&'a str, &'a str)];
type PackageShape<'a> = (&'a str, &'a [(&'a str, &'a str, bool)], ProjectDependencies<'a>);

fn build_projects(
    projects: &[ProjectDependencies<'_>],
    packages: &[PackageShape<'_>],
) -> (ResolvedTree, Vec<ImporterPeerInput>) {
    let mut tree = ResolvedTree::default();
    for &(name, peers, children) in packages {
        let id: Arc<str> = format!("{name}@1.0.0").into();
        tree.packages.insert(
            Arc::clone(&id),
            package_with_peer_dependencies(
                name,
                "1.0.0",
                peers,
                children.is_empty() && peers.is_empty(),
            ),
        );
        tree.children_by_id.insert(
            id,
            children
                .iter()
                .map(|&(alias, id)| child_edge(alias, id))
                .collect::<Vec<_>>()
                .into(),
        );
        tree.all_peer_dep_names.extend(peers.iter().map(|&(name, _, _)| name.to_string()));
    }
    for version in ["1.0.0", "2.0.0"] {
        tree.packages.insert(format!("peer@{version}").into(), package("peer", version, &[], true));
    }
    let importers: Vec<_> = projects
        .iter()
        .enumerate()
        .map(|(index, deps)| {
            let mut direct = Vec::new();
            for &(alias, id) in *deps {
                add_lazy_direct_dep(&mut tree.dependencies_tree, &mut direct, alias, id);
            }
            ImporterPeerInput {
                id: index.to_string(),
                direct,
                root_dir: format!("/repo/{index}").into(),
                modules_dir: None,
                rejected_optional_peers: HashSet::default(),
            }
        })
        .collect();
    (tree, importers)
}

fn resolve_projects(
    projects: &[ProjectDependencies<'_>],
    packages: &[PackageShape<'_>],
    dedupe: PeerDependentsDedupe,
) -> WorkspaceResolvePeersResult {
    let (mut tree, importers) = build_projects(projects, packages);
    resolve_with_cache_oracle(
        &mut tree,
        &importers,
        std::path::Path::new("/repo"),
        WorkspacePeerSettings { dedupe_peer_dependents: dedupe, ..Default::default() },
        ResolvePeersOptions::default(),
    )
}

#[test]
fn project_peer_provider_keeps_its_nested_peer_variant() {
    let (mut tree, importers) = build_projects(
        &[
            &[("provider", "provider@1.0.0"), ("peer", "peer@1.0.0"), ("wrapper", "wrapper@1.0.0")],
            &[
                ("provider", "provider@1.0.0"),
                ("peer", "peer@1.0.0"),
                ("consumer", "consumer@1.0.0"),
            ],
        ],
        &[
            (
                "provider",
                &[("inner", "*", false)],
                &[("peer", "peer@2.0.0"), ("inner", "inner@1.0.0")],
            ),
            ("inner", &[("peer", "*", false)], &[("peer", "peer@2.0.0")]),
            ("consumer", &[("provider", "*", false)], &[]),
            ("wrapper", &[], &[("consumer", "consumer@1.0.0")]),
        ],
    );
    for (index, importer) in importers.iter().enumerate() {
        let peer_id = if index == 0 { "peer@1.0.0" } else { "peer@2.0.0" };
        let peer = NodeId::leaf(peer_id);
        let inner = NodeId::next();
        tree.dependencies_tree.insert(peer.clone(), tree_node(peer_id, BTreeMap::new(), 1));
        tree.dependencies_tree.insert(inner.clone(), tree_node("inner@1.0.0", BTreeMap::new(), 1));
        tree.dependencies_tree.insert(
            importer.direct[0].node_id.clone(),
            tree_node(
                "provider@1.0.0",
                BTreeMap::from([("peer".into(), peer), ("inner".into(), inner)]),
                0,
            ),
        );
    }
    for dedupe in [PeerDependentsDedupe::Off, PeerDependentsDedupe::On] {
        for shift in 0..3 {
            let mut tree = tree.clone();
            let mut importers = importers.clone();
            for importer in &mut importers {
                importer.direct.rotate_left(shift);
            }
            let result = resolve_with_cache_oracle(
                &mut tree,
                &importers,
                std::path::Path::new("/repo"),
                WorkspacePeerSettings { dedupe_peer_dependents: dedupe, ..Default::default() },
                ResolvePeersOptions::default(),
            );
            let direct = &result.direct_dependencies_by_importer["1"];
            assert_eq!(
                result.graph[&direct["consumer"]].edges.children["provider"],
                direct["provider"],
            );
        }
    }
}

#[test]
fn unbound_optional_peer_dedupes_consistently_across_dependency_paths() {
    let result = resolve_projects(
        &[
            &[("middle", "middle@1.0.0"), ("outer", "outer@1.0.0")],
            &[("middle", "middle@1.0.0"), ("peer", "peer@1.0.0")],
            &[("outer", "outer@1.0.0"), ("peer", "peer@2.0.0")],
        ],
        &[
            ("inner", &[("peer", "*", true)], &[]),
            ("middle", &[], &[("inner", "inner@1.0.0")]),
            ("outer", &[], &[("middle", "middle@1.0.0")]),
        ],
        PeerDependentsDedupe::On,
    );
    let direct = &result.direct_dependencies_by_importer["0"];
    assert_eq!(result.graph[&direct["outer"]].edges.children["middle"], direct["middle"]);
}

#[test]
fn unbound_optional_peer_dedupes_consistently_with_different_parent_variants() {
    let result = resolve_projects(
        &[
            &[("middle", "middle@1.0.0"), ("outer", "outer@1.0.0")],
            &[("outer", "outer@1.0.0"), ("peer", "peer@1.0.0")],
            &[("middle", "middle@1.0.0"), ("peer", "peer@2.0.0")],
        ],
        &[
            ("inner", &[("peer", "*", true)], &[]),
            ("middle", &[], &[("inner", "inner@1.0.0")]),
            ("outer", &[], &[("middle", "middle@1.0.0")]),
        ],
        PeerDependentsDedupe::On,
    );
    let direct = &result.direct_dependencies_by_importer["0"];
    assert_eq!(result.graph[&direct["outer"]].edges.children["middle"], direct["middle"]);
}

#[test]
fn locked_optional_peers_do_not_split_an_unbound_project() {
    let (mut tree, importers) = build_projects(
        &[
            &[("middle", "middle@1.0.0"), ("outer", "outer@1.0.0")],
            &[("middle", "middle@1.0.0"), ("peer", "peer@1.0.0")],
            &[("outer", "outer@1.0.0"), ("peer", "peer@2.0.0")],
        ],
        &[
            ("inner", &[("peer", "*", true)], &[]),
            ("middle", &[], &[("inner", "inner@1.0.0")]),
            ("outer", &[], &[("middle", "middle@1.0.0")]),
        ],
    );
    let mut paths = HashMap::default();
    for (index, version) in [(1, "1.0.0"), (2, "2.0.0")] {
        let peer = &importers[index].direct[1].node_id;
        let path = DepPath::from(format!("peer@{version}"));
        tree.dependencies_tree
            .get_mut(peer)
            .unwrap()
            .locked_mut()
            .previous_dep_path = Some(path.clone());
        paths.insert(peer.clone(), path.clone());
        let inner = NodeId::next();
        let mut inner_node = tree_node("inner@1.0.0", BTreeMap::new(), 2);
        inner_node.locked_mut().locked_peer_context = Some(BTreeMap::from([("peer".into(), path)]));
        tree.dependencies_tree.insert(inner.clone(), inner_node);
        let middle =
            if index == 1 { importers[0].direct[0].node_id.clone() } else { NodeId::next() };
        tree.dependencies_tree.insert(
            middle.clone(),
            tree_node("middle@1.0.0", BTreeMap::from([("inner".into(), inner)]), 1),
        );
        if index == 2 {
            tree.dependencies_tree.insert(
                importers[0].direct[1].node_id.clone(),
                tree_node("outer@1.0.0", BTreeMap::from([("middle".into(), middle)]), 0),
            );
        }
    }
    for dedupe in [PeerDependentsDedupe::Off, PeerDependentsDedupe::On] {
        let mut tree = tree.clone();
        let result = resolve_with_cache_oracle(
            &mut tree,
            &importers,
            std::path::Path::new("/repo"),
            WorkspacePeerSettings { dedupe_peer_dependents: dedupe, ..Default::default() },
            ResolvePeersOptions {
                scope: crate::PeerResolutionScope {
                    resolved_peer_provider_paths: Some(paths.clone()),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let direct = &result.direct_dependencies_by_importer["0"];
        assert_eq!(result.graph[&direct["outer"]].edges.children["middle"], direct["middle"]);
        match dedupe {
            PeerDependentsDedupe::Off => {
                assert_eq!(direct["middle"], DepPath::from("middle@1.0.0"));
            }
            PeerDependentsDedupe::On | PeerDependentsDedupe::OnWithProjectScopedOptionalPeers => {}
        }
        assert_eq!(
            result.direct_dependencies_by_importer["1"]["middle"],
            DepPath::from("middle@1.0.0(peer@1.0.0)"),
        );
        assert_eq!(
            result.direct_dependencies_by_importer["2"]["outer"],
            DepPath::from("outer@1.0.0(peer@2.0.0)"),
        );
    }
}
