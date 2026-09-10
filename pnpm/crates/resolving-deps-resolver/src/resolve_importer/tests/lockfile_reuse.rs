use super::{
    DependencyGroup, HashMap, Mutex, StubResolver, assert_eq, default_opts, fake_manifest,
    fake_result, resolve_importer,
};

#[tokio::test]
async fn reuses_preferred_version_instead_of_resolving_fresh() {
    let mut table = HashMap::default();
    table.insert(
        ("react".to_string(), "18.2.0".to_string()),
        fake_result("react", "18.2.0", serde_json::json!({ "name": "react", "version": "18.2.0" })),
    );
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
    // hoistPeers picks the already-resolved 18.2.0 instead of "^18.0.0".
    // The stub returns the same result for both keys so a stray
    // "^18.0.0" resolve call would still work — but the assertion
    // below also checks the call list.
    table.insert(
        ("react".to_string(), "18.2.0".to_string()),
        fake_result("react", "18.2.0", serde_json::json!({ "name": "react", "version": "18.2.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) =
        fake_manifest(serde_json::json!({ "react": "18.2.0", "react-dom": "18.0.0" }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let calls = resolver.calls.lock().unwrap();
    let react_call_count = calls.iter().filter(|(name, _)| name == "react").count();
    assert_eq!(react_call_count, 1, "should not re-resolve react via a hoisted spec");

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct.contains(&"react"));
    assert!(direct.contains(&"react-dom"));
}
