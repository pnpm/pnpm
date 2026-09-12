use super::{
    DepPath, DependencyGroup, HashMap, Mutex, ResolveDependencyTreeOptions, ResolveOptions,
    ResolvePeersOptions, StubResolver, assert_eq, fake_manifest, fake_result,
    resolve_dependency_tree, resolve_peers,
};

#[tokio::test]
async fn missing_peer_is_reported() {
    let mut table = HashMap::default();
    table.insert(
        ("react-dom".to_string(), "18.0.0".to_string()),
        fake_result(
            "react-dom",
            "18.0.0",
            serde_json::json!({
                "name": "react-dom",
                "version": "18.0.0",
                "peerDependencies": { "react": "^18.0.0" }
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "react-dom": "18.0.0" }));
    let mut tree = resolve_dependency_tree(
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

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
    assert!(result.peer_dependency_issues.missing.contains_key("react"));
    assert_eq!(
        result.direct_dependencies_by_alias.get("react-dom"),
        Some(&DepPath::from("react-dom@18.0.0".to_string())),
    );
}
