use super::{
    DepPath, DependencyGroup, HashMap, Mutex, StubResolver, assert_eq, default_opts, fake_manifest,
    fake_result, resolve_importer,
};

#[tokio::test]
async fn auto_install_dedupes_via_range_intersection_when_identical() {
    let mut table = HashMap::default();
    table.insert(
        ("wants-peer-c-1".to_string(), "1.0.0".to_string()),
        fake_result(
            "wants-peer-c-1",
            "1.0.0",
            serde_json::json!({
                "name": "wants-peer-c-1",
                "version": "1.0.0",
                "peerDependencies": { "peer-c": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("wants-peer-c-1.0.0".to_string(), "1.0.0".to_string()),
        fake_result(
            "wants-peer-c-1.0.0",
            "1.0.0",
            serde_json::json!({
                "name": "wants-peer-c-1.0.0",
                "version": "1.0.0",
                "peerDependencies": { "peer-c": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("peer-c".to_string(), "1.0.0".to_string()),
        fake_result("peer-c", "1.0.0", serde_json::json!({ "name": "peer-c", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "wants-peer-c-1": "1.0.0",
        "wants-peer-c-1.0.0": "1.0.0",
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct.contains(&"peer-c"), "single intersected peer-c should be hoisted: {direct:?}");
    let peer_c_entries: Vec<&DepPath> = result
        .peers_result
        .graph
        .keys()
        .filter(|dp| dp.to_string().starts_with("peer-c@"))
        .collect();
    assert_eq!(peer_c_entries.len(), 1, "expected one peer-c entry, got: {peer_c_entries:?}");
}
