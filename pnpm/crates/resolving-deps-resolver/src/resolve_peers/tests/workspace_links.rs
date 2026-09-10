use super::{
    BTreeMap, DepPath, DirectDep, HashMap, HashSet, ImporterPeerInput, NodeId, ResolvePeersOptions,
    ResolvedTree, linked_package, package, resolve_peers, resolve_peers_workspace, tree_node,
};

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
