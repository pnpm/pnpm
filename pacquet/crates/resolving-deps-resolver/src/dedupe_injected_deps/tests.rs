use super::{DirectByImporter, dedupe_injected_deps};
use crate::dependencies_graph::{DependenciesGraph, DependenciesGraphNode};
use pacquet_deps_path::DepPath;
use pacquet_lockfile::{DirectoryResolution, LockfileResolution};
use pacquet_resolving_resolver_base::{PkgResolutionId, ResolveResult};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};

fn make_node(id: &str, children: BTreeMap<String, DepPath>) -> DependenciesGraphNode {
    DependenciesGraphNode {
        dep_path: DepPath::from(id.to_string()),
        resolved_package_id: id.to_string(),
        resolve_result: std::sync::Arc::new(ResolveResult {
            id: PkgResolutionId::from(id.to_string()),
            name_ver: None,
            latest: None,
            published_at: None,
            manifest: None,
            resolution: LockfileResolution::Directory(DirectoryResolution {
                directory: "stub".to_string(),
            }),
            resolved_via: "workspace".to_string(),
            normalized_bare_specifier: None,
            alias: None,
            policy_violation: None,
        }),
        children,
        optional_children: HashSet::new(),
        peer_dependencies: BTreeMap::new(),
        transitive_peer_dependencies: HashSet::new(),
        resolved_peer_names: HashSet::new(),
        depth: 0,
        installable: true,
        is_pure: true,
        optional: false,
    }
}

#[test]
fn rewrites_childless_injected_dep_to_link() {
    let lockfile_dir = PathBuf::from("/ws");
    let p1_root = lockfile_dir.join("project-1");
    let p2_root = lockfile_dir.join("project-2");

    let mut graph: DependenciesGraph = std::collections::HashMap::new();
    let file_path = DepPath::from("file:project-1".to_string());
    graph.insert(file_path.clone(), make_node("file:project-1", BTreeMap::new()));

    let mut direct: DirectByImporter = BTreeMap::new();
    direct.insert("project-1".to_string(), BTreeMap::new());
    direct.insert("project-2".to_string(), BTreeMap::from([("project-1".to_string(), file_path)]));

    let mut roots = BTreeMap::new();
    roots.insert("project-1".to_string(), p1_root);
    roots.insert("project-2".to_string(), p2_root);

    dedupe_injected_deps(&mut graph, &mut direct, &roots, &lockfile_dir);

    let after = direct.get("project-2").unwrap().get("project-1").unwrap();
    assert_eq!(after.as_str(), "link:../project-1");
    assert!(graph.is_empty(), "unreachable file: snapshot should be pruned");
}

#[test]
fn leaves_injected_dep_when_children_differ() {
    let lockfile_dir = PathBuf::from("/ws");
    let p1_root = lockfile_dir.join("project-1");
    let p2_root = lockfile_dir.join("project-2");

    let lib_v1 = DepPath::from("lib@1.0.0".to_string());
    let lib_v2 = DepPath::from("lib@2.0.0".to_string());

    let mut graph: DependenciesGraph = std::collections::HashMap::new();
    graph.insert(lib_v1.clone(), make_node("lib@1.0.0", BTreeMap::new()));
    graph.insert(lib_v2.clone(), make_node("lib@2.0.0", BTreeMap::new()));
    let injected = DepPath::from("file:project-1".to_string());
    graph.insert(
        injected.clone(),
        make_node("file:project-1", BTreeMap::from([("lib".to_string(), lib_v2)])),
    );

    let mut direct: DirectByImporter = BTreeMap::new();
    direct.insert("project-1".to_string(), BTreeMap::from([("lib".to_string(), lib_v1)]));
    direct.insert("project-2".to_string(), BTreeMap::from([("project-1".to_string(), injected)]));

    let mut roots = BTreeMap::new();
    roots.insert("project-1".to_string(), p1_root);
    roots.insert("project-2".to_string(), p2_root);

    dedupe_injected_deps(&mut graph, &mut direct, &roots, &lockfile_dir);

    let after = direct.get("project-2").unwrap().get("project-1").unwrap();
    assert_eq!(after.as_str(), "file:project-1");
}

#[test]
fn rewrites_when_children_subset_of_target_direct_deps() {
    let lockfile_dir = PathBuf::from("/ws");
    let p1_root = lockfile_dir.join("project-1");
    let p2_root = lockfile_dir.join("project-2");

    let lib = DepPath::from("lib@1.0.0".to_string());

    let mut graph: DependenciesGraph = std::collections::HashMap::new();
    graph.insert(lib.clone(), make_node("lib@1.0.0", BTreeMap::new()));
    let injected = DepPath::from("file:project-1".to_string());
    graph.insert(
        injected.clone(),
        make_node("file:project-1", BTreeMap::from([("lib".to_string(), lib.clone())])),
    );

    let mut direct: DirectByImporter = BTreeMap::new();
    direct.insert("project-1".to_string(), BTreeMap::from([("lib".to_string(), lib.clone())]));
    direct.insert(
        "project-2".to_string(),
        BTreeMap::from([("project-1".to_string(), injected.clone())]),
    );

    let mut roots = BTreeMap::new();
    roots.insert("project-1".to_string(), p1_root);
    roots.insert("project-2".to_string(), p2_root);

    dedupe_injected_deps(&mut graph, &mut direct, &roots, &lockfile_dir);

    let after = direct.get("project-2").unwrap().get("project-1").unwrap();
    assert_eq!(after.as_str(), "link:../project-1");
    assert!(graph.contains_key(&lib));
    assert!(!graph.contains_key(&injected));
}

#[test]
fn ignores_non_workspace_file_deps() {
    let lockfile_dir = PathBuf::from("/ws");
    let mut graph: DependenciesGraph = std::collections::HashMap::new();
    let tarball = DepPath::from("file:vendor/some-tarball.tgz".to_string());
    graph.insert(tarball.clone(), make_node("file:vendor/some-tarball.tgz", BTreeMap::new()));

    let mut direct: DirectByImporter = BTreeMap::new();
    direct.insert(".".to_string(), BTreeMap::from([("some-tarball".to_string(), tarball)]));

    let mut roots = BTreeMap::new();
    roots.insert(".".to_string(), lockfile_dir.clone());

    dedupe_injected_deps(&mut graph, &mut direct, &roots, &lockfile_dir);

    let after = direct.get(".").unwrap().get("some-tarball").unwrap();
    assert_eq!(after.as_str(), "file:vendor/some-tarball.tgz");
}
