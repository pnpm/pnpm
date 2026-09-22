use super::{
    Arc, BTreeMap, DepPath, DirectDep, HashMap, HashSet, NodeId, PkgResolutionId,
    ResolvePeersOptions, ResolvedTree, package, resolve_peers, tree_node,
};

#[test]
fn tarball_peer_versions_preserve_source_identity() {
    for tarball in ["file:first.tgz", "file:second.tgz"] {
        for version in ["1.0.0", "2.0.0"] {
            let provider_id = format!("provider@{tarball}");
            let provider_node = NodeId::leaf(&provider_id);
            let consumer_node = NodeId::next();
            let mut provider = package("provider", version, &[], true);
            provider.id = provider_id.clone().into();
            let result = Arc::make_mut(&mut provider.result);
            result.id = PkgResolutionId::from(tarball.to_string());
            result.alias = Some("provider".to_string());
            result.package.name_ver = None;
            result.package.manifest = Some(Arc::new(serde_json::json!({
                "name": "provider", "version": version,
            })));
            let mut tree = ResolvedTree {
                direct: vec![
                    DirectDep {
                        alias: "consumer".to_string(),
                        node_id: consumer_node.clone(),
                        id: "consumer@1.0.0".to_string(),
                    },
                    DirectDep {
                        alias: "provider".to_string(),
                        node_id: provider_node.clone(),
                        id: provider_id.clone(),
                    },
                ],
                packages: HashMap::from_iter([
                    (provider_id.clone().into(), provider),
                    (
                        "consumer@1.0.0".into(),
                        package("consumer", "1.0.0", &[("provider", "^1.0.0")], false),
                    ),
                ]),
                dependencies_tree: HashMap::from_iter([
                    (provider_node, tree_node(&provider_id, BTreeMap::new(), 0)),
                    (consumer_node, tree_node("consumer@1.0.0", BTreeMap::new(), 0)),
                ]),
                all_peer_dep_names: HashSet::from_iter(["provider".to_string()]),
                ..ResolvedTree::default()
            };
            let result = resolve_peers(
                &mut tree,
                ResolvePeersOptions { dedupe_peers: true, ..ResolvePeersOptions::default() },
            );
            assert_eq!(
                result.direct_dependencies_by_alias["consumer"],
                DepPath::from(format!("consumer@1.0.0(provider@{tarball})")),
            );
            let issues = result.peer_dependency_issues;
            assert!(issues.missing.is_empty(), "missing peers: {:?}", issues.missing);
            if version == "1.0.0" {
                assert!(issues.bad.is_empty(), "bad peers: {:?}", issues.bad);
            } else {
                assert_eq!(issues.bad["provider"][0].found_version, version);
            }
        }
    }
}

#[test]
fn distinct_tarball_providers_with_same_manifest_version_do_not_collapse() {
    let provider_a_id = "provider@file:first.tgz";
    let provider_b_id = "provider@file:second.tgz";
    let provider_a_node = NodeId::leaf(provider_a_id);
    let provider_b_node = NodeId::leaf(provider_b_id);
    let consumer_node = NodeId::next();
    let middle_node = NodeId::next();

    let make_provider = |id: &str, tarball: &str| {
        let mut provider = package("provider", "1.0.0", &[], true);
        provider.id = id.into();
        let result = Arc::make_mut(&mut provider.result);
        result.id = PkgResolutionId::from(tarball.to_string());
        result.alias = Some("provider".to_string());
        result.package.name_ver = None;
        result.package.manifest = Some(Arc::new(serde_json::json!({
            "name": "provider", "version": "1.0.0",
        })));
        provider
    };

    let middle_children = BTreeMap::from_iter([
        ("consumer".to_string(), consumer_node.clone()),
        ("provider".to_string(), provider_b_node.clone()),
    ]);

    let mut tree = ResolvedTree {
        direct: vec![
            DirectDep {
                alias: "middle".to_string(),
                node_id: middle_node.clone(),
                id: "middle@1.0.0".to_string(),
            },
            DirectDep {
                alias: "provider".to_string(),
                node_id: provider_a_node.clone(),
                id: provider_a_id.to_string(),
            },
        ],
        packages: HashMap::from_iter([
            (provider_a_id.into(), make_provider(provider_a_id, "file:first.tgz")),
            (provider_b_id.into(), make_provider(provider_b_id, "file:second.tgz")),
            (
                "middle@1.0.0".into(),
                package("middle", "1.0.0", &[("consumer", "1.0.0"), ("provider", "1.0.0")], false),
            ),
            (
                "consumer@1.0.0".into(),
                package("consumer", "1.0.0", &[("provider", "^1.0.0")], false),
            ),
        ]),
        dependencies_tree: HashMap::from_iter([
            (provider_a_node, tree_node(provider_a_id, BTreeMap::new(), 0)),
            (provider_b_node, tree_node(provider_b_id, BTreeMap::new(), 1)),
            (middle_node, tree_node("middle@1.0.0", middle_children, 0)),
            (consumer_node, tree_node("consumer@1.0.0", BTreeMap::new(), 1)),
        ]),
        all_peer_dep_names: HashSet::from_iter(["provider".to_string()]),
        ..ResolvedTree::default()
    };

    let result = resolve_peers(
        &mut tree,
        ResolvePeersOptions { dedupe_peers: true, ..ResolvePeersOptions::default() },
    );
    let middle_dep = &result.direct_dependencies_by_alias["middle"];
    assert_eq!(
        result.graph[middle_dep].edges.children["consumer"],
        DepPath::from("consumer@1.0.0(provider@file:second.tgz)"),
    );
}
