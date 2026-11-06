use super::{
    ResolvePeersOptions, WorkspacePeerSettings, build_projects, resolve_with_cache_oracle,
};
use crate::PeerDependentsDedupe;
use rustc_hash::FxHashSet as HashSet;
use std::{collections::BTreeMap, path::Path};

const ORDERS: [[usize; 3]; 6] = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]];

#[test]
fn generated_peer_contexts_match_full_walk() {
    for providers in 0..27 {
        for flags in 0..16 {
            check_contexts(providers, flags);
        }
    }
}

fn check_contexts(providers: usize, flags: usize) {
    let optional = flags & 1 != 0;
    let default_dependency = flags & 2 != 0;
    let nested_provider = flags & 4 != 0;
    let dedupe_peers = flags & 8 != 0;
    let (tree, importers) =
        context_fixture(providers, optional, default_dependency, nested_provider);
    let mut expected_by_dedupe = BTreeMap::new();
    for order in ORDERS {
        let ordered: Vec<_> = order
            .iter()
            .enumerate()
            .map(|(rank, &index)| {
                let mut importer = importers[index].clone();
                importer.id = rank.to_string();
                importer
            })
            .collect();
        for dedupe in [PeerDependentsDedupe::Off, PeerDependentsDedupe::On] {
            let mut tree = tree.clone();
            let result = resolve_with_cache_oracle(
                &mut tree,
                &ordered,
                Path::new("/repo"),
                WorkspacePeerSettings { dedupe_peer_dependents: dedupe, ..Default::default() },
                ResolvePeersOptions { dedupe_peers, ..Default::default() },
            );
            let by_project: BTreeMap<_, _> = ordered
                .iter()
                .map(|importer| {
                    (
                        importer.root_dir.clone(),
                        result.direct_dependencies_by_importer[&importer.id].clone(),
                    )
                })
                .collect();
            let expected =
                expected_by_dedupe.entry(dedupe.dedupes()).or_insert_with(|| by_project.clone());
            assert_eq!(
                &by_project, expected,
                "providers={providers}, optional={optional}, default_dependency={default_dependency}, nested_provider={nested_provider}, order={order:?}, dedupe={dedupe:?}",
            );
        }
    }
}

fn context_fixture(
    providers: usize,
    optional: bool,
    default_dependency: bool,
    nested_provider: bool,
) -> (super::ResolvedTree, Vec<super::ImporterPeerInput>) {
    let projects = project_dependencies(providers, nested_provider);
    let inner_peers = [("peer", "*", optional)];
    let default_child = [("peer", "peer@2.0.0")];
    let nested_peers = [("inner", "*", false)];
    let (mut tree, importers) = build_projects(
        &projects
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>(),
        &[
            ("inner", &inner_peers, if default_dependency { &default_child } else { &[] }),
            (
                "middle",
                if nested_provider { &nested_peers } else { &[] },
                if nested_provider { &[] } else { &[("inner", "inner@1.0.0")] },
            ),
            ("outer", &[], &[("middle", "middle@1.0.0")]),
            ("consumer", &[("middle", "*", false)], &[]),
        ],
    );
    if default_dependency {
        let inner = tree.packages.get_mut("inner@1.0.0").unwrap();
        inner.peer_set =
            crate::PeerSet::new(inner.peer_set.declared().clone(), |name| name == "peer");
    }
    (tree, importers)
}

fn project_dependencies(
    providers: usize,
    nested_provider: bool,
) -> Vec<Vec<(&'static str, &'static str)>> {
    (0..3)
        .map(|index| {
            let mut deps = vec![
                ("middle", "middle@1.0.0"),
                ("outer", "outer@1.0.0"),
                ("consumer", "consumer@1.0.0"),
            ];
            if nested_provider {
                deps.push(("inner", "inner@1.0.0"));
            }
            match providers / 3usize.pow(index) % 3 {
                0 => {}
                1 => deps.push(("peer", "peer@1.0.0")),
                2 => deps.push(("peer", "peer@2.0.0")),
                _ => unreachable!(),
            }
            deps
        })
        .collect()
}

#[test]
fn full_walk_keeps_peer_cycle_handling() {
    use crate::resolve_peers::{
        ImporterPeerInput, resolve_peers_workspace, test_support::PeerCacheMode,
        tests::cyclic_alias_peer_tree,
    };

    for aliases in
        [["vite", "@vitejs/devtools", "vite-plus"], ["vite-plus", "@vitejs/devtools", "vite"]]
    {
        let mut tree = cyclic_alias_peer_tree(aliases);
        let importers = [ImporterPeerInput {
            id: ".".into(),
            direct: tree.direct.clone(),
            root_dir: "/repo".into(),
            modules_dir: None,
            rejected_optional_peers: HashSet::default(),
        }];
        let result = resolve_peers_workspace(
            &mut tree,
            &importers,
            Path::new("/repo"),
            WorkspacePeerSettings::default(),
            ResolvePeersOptions { cache_mode: PeerCacheMode::Bypass, ..Default::default() },
        );
        let direct = &result.direct_dependencies_by_importer["."];
        assert_eq!(result.graph[&direct["vite"]].resolved_package_id, "core@1.0.0");
        assert_eq!(
            result.graph[&direct["@vitejs/devtools"]].resolved_package_id,
            "@vitejs/devtools@1.0.0",
        );
        for node in result.graph.values() {
            for child in node.edges.children.values() {
                assert!(result.graph.contains_key(child), "{} -> {child}", node.dep_path);
            }
            if node.resolved_package_id == "core@1.0.0" {
                assert_eq!(node.edges.children["@vitejs/devtools"], direct["@vitejs/devtools"]);
            }
        }
    }
}
