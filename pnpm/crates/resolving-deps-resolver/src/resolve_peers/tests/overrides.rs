use super::{
    BTreeMap, DepPath, DirectDep, HashMap, HashSet, NodeId, ResolvePeersOptions, ResolvedTree,
    package, resolve_peers, tree_node, walker_for_tests,
};

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
            ("alias-real@1.0.0".into(), package("alias-real", "1.0.0", &[], true)),
            ("peer-c@2.0.0".into(), package("peer-c", "2.0.0", &[], true)),
            ("unused@1.0.0".into(), package("unused", "1.0.0", &[], true)),
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
            ("shared@1.0.0".into(), package("shared", "1.0.0", &[], true)),
            ("parent@1.0.0".into(), package("parent", "1.0.0", &[], false)),
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
