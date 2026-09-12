use super::{
    DepPath, DependencyGroup, HashMap, Mutex, StubResolver, assert_eq, default_opts, fake_manifest,
    fake_result, merge_ranges, resolve_importer,
};

#[tokio::test]
async fn does_not_hoist_when_disabled() {
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

    let mut opts = default_opts();
    opts.auto_install_peers = false;
    let result =
        resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], opts).await.unwrap();

    #[expect(
        clippy::needless_collect,
        reason = "Collecting into a Vec keeps the assertion readable; `.any(...)` on the iterator would be denser without saving meaningful work."
    )]
    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(!direct.contains(&"react"));
    assert!(result.peers_result.peer_dependency_issues.missing.contains_key("react"));
}

#[tokio::test]
async fn auto_install_does_not_install_when_no_intersection() {
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
        ("wants-peer-c-2".to_string(), "1.0.0".to_string()),
        fake_result(
            "wants-peer-c-2",
            "1.0.0",
            serde_json::json!({
                "name": "wants-peer-c-2",
                "version": "1.0.0",
                "peerDependencies": { "peer-c": "2.0.0" },
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "wants-peer-c-1": "1.0.0",
        "wants-peer-c-2": "1.0.0",
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(!direct.contains(&"peer-c"), "peer-c must not be hoisted on conflict: {direct:?}");
}

#[test]
fn repeated_consumer_ranges_merge_into_the_unique_intersection() {
    let react = "^16.8 || ^17.0 || ^18.0 || ^19.0 || ^19.0.0-rc";
    let narrower = "^18.0.0 || ^19.0.0";
    let merged = ">=18.0.0 <19.0.0-0||>=19.0.0 <20.0.0-0";

    assert_eq!(merge_ranges(&[react, narrower], false).as_deref(), Some(merged));

    let mut repeated = vec![react; 10];
    repeated.push(narrower);

    assert_eq!(merge_ranges(&repeated, false).as_deref(), Some(merged));
}

/// Deduplication alone only sees ranges that are spelled identically.
/// Consumers reach the same peer through spellings that differ, and each
/// one that survives into the union multiplies the next intersection, so
/// the merged range has to stay bounded across them too.
#[test]
fn differently_spelled_equivalent_ranges_do_not_grow_the_merged_range() {
    let spellings = [
        "^16.8 || ^17.0 || ^18.0 || ^19.0 || ^19.0.0-rc",
        "^16.8.0 || ^17.0.0 || ^18.0.0 || ^19.0.0 || ^19.0.0-rc",
        ">=16.8 <17 || >=17 <18 || >=18 <19 || >=19 <20 || ^19.0.0-rc",
        "16.8 - 16.x || ^17.0 || ^18.0 || ^19.0 || ^19.0.0-rc",
    ];
    let merged =
        ">=16.8.0 <17.0.0-0||>=17.0.0 <18.0.0-0||>=18.0.0 <19.0.0-0||>=19.0.0-rc <20.0.0-0";

    assert_eq!(merge_ranges(&spellings, false).as_deref(), Some(merged));

    let repeated: Vec<&str> = spellings.iter().cycle().copied().take(200).collect();

    assert_eq!(merge_ranges(&repeated, false).as_deref(), Some(merged));
}

/// A scheme specifier is not a semver range, so intersecting it would
/// drop the peer instead of hoisting it.
#[test]
fn repeated_scheme_specifier_stays_verbatim() {
    assert_eq!(
        merge_ranges(&["workspace:^", "workspace:^"], false).as_deref(),
        Some("workspace:^"),
    );
}

#[tokio::test]
async fn auto_install_from_highest_match_installs_on_conflict() {
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
        ("wants-peer-c-2".to_string(), "1.0.0".to_string()),
        fake_result(
            "wants-peer-c-2",
            "1.0.0",
            serde_json::json!({
                "name": "wants-peer-c-2",
                "version": "1.0.0",
                "peerDependencies": { "peer-c": "2.0.0" },
            }),
        ),
    );
    table.insert(
        ("peer-c".to_string(), "1.0.0 || 2.0.0".to_string()),
        fake_result("peer-c", "2.0.0", serde_json::json!({ "name": "peer-c", "version": "2.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "wants-peer-c-1": "1.0.0",
        "wants-peer-c-2": "1.0.0",
    }));

    let mut opts = default_opts();
    opts.auto_install_peers_from_highest_match = true;
    let result =
        resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], opts).await.unwrap();

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct.contains(&"peer-c"), "peer-c should land via `||` join: {direct:?}");
}

#[tokio::test]
async fn auto_install_does_not_hoist_when_root_already_has_dep() {
    let mut table = HashMap::default();
    table.insert(
        ("xyz".to_string(), "1.0.0".to_string()),
        fake_result(
            "xyz",
            "1.0.0",
            serde_json::json!({
                "name": "xyz",
                "version": "1.0.0",
                "peerDependencies": { "x": "^1.0.0" },
            }),
        ),
    );
    table.insert(
        ("x".to_string(), "1.0.0".to_string()),
        fake_result("x", "1.0.0", serde_json::json!({ "name": "x", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "xyz": "1.0.0",
        "x": "1.0.0",
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let calls = resolver.calls.lock().unwrap();
    let x_ranges: Vec<String> =
        calls.iter().filter(|(n, _)| n == "x").map(|(_, r)| r.clone()).collect();
    assert_eq!(
        x_ranges,
        vec!["1.0.0".to_string()],
        "`x` should resolve only via the importer's direct spec, got: {x_ranges:?}",
    );
    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("xyz"),
        Some(&DepPath::from("xyz@1.0.0(x@1.0.0)".to_string())),
    );
}
