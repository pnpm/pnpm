use super::{
    DependencyGroup, HashMap, Mutex, ResolveDependencyTreeOptions, ResolveOptions, StubResolver,
    assert_eq, fake_manifest, fake_result, resolve_dependency_tree,
};

#[tokio::test]
async fn dedupes_when_the_same_package_appears_in_two_subtrees() {
    let mut table = HashMap::default();
    table.insert(
        ("a".to_string(), "^1.0.0".to_string()),
        fake_result(
            "a",
            "1.0.0",
            serde_json::json!({
                "name": "a",
                "version": "1.0.0",
                "dependencies": { "shared": "^1.0.0" }
            }),
        ),
    );
    table.insert(
        ("b".to_string(), "^1.0.0".to_string()),
        fake_result(
            "b",
            "1.0.0",
            serde_json::json!({
                "name": "b",
                "version": "1.0.0",
                "dependencies": { "shared": "^1.0.0" }
            }),
        ),
    );
    table.insert(
        ("shared".to_string(), "^1.0.0".to_string()),
        fake_result(
            "shared",
            "1.0.0",
            serde_json::json!({
                "name": "shared",
                "version": "1.0.0",
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "a": "^1.0.0", "b": "^1.0.0" }));

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: None,
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .unwrap();

    assert_eq!(tree.packages.len(), 3);
    assert!(tree.packages.contains_key("a@1.0.0"));
    assert!(tree.packages.contains_key("b@1.0.0"));
    assert!(tree.packages.contains_key("shared@1.0.0"));

    let a_tree = tree.dependencies_tree.get(&tree.direct[0].node_id).unwrap();
    let b_tree = tree.dependencies_tree.get(&tree.direct[1].node_id).unwrap();
    let shared_via_a = a_tree.children.realized().get("shared").unwrap();
    let shared_via_b = b_tree.children.realized().get("shared").unwrap();
    assert_eq!(shared_via_a, shared_via_b);
    let shared_occurrences = tree
        .dependencies_tree
        .values()
        .filter(|n| n.resolved_package_id == "shared@1.0.0".into())
        .count();
    assert_eq!(shared_occurrences, 1);
}
