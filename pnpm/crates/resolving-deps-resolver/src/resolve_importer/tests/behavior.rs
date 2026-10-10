use std::{path::Path, sync::Arc};

use super::{
    DepPath, DependencyGroup, HashMap, Mutex, ResolveResult, StubResolver, assert_eq, default_opts,
    fake_manifest, fake_result, merge_ranges, resolve_importer,
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
    opts.peers.auto_install_peers = false;
    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], opts)
        .await
        .unwrap();

    #[expect(
        clippy::needless_collect,
        reason = "Collecting into a Vec keeps the assertion readable; `.any(...)` on the iterator would be denser without saving meaningful work."
    )]
    let direct: Vec<&str> = result.peers_result.direct_dependencies_by_alias
        .keys()
        .map(String::as_str)
        .collect();
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

    let direct: Vec<&str> = result.peers_result.direct_dependencies_by_alias
        .keys()
        .map(String::as_str)
        .collect();
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

    let repeated: Vec<&str> = spellings
        .iter()
        .cycle()
        .copied()
        .take(200)
        .collect();

    assert_eq!(merge_ranges(&repeated, false).as_deref(), Some(merged));
}

/// Consumers whose unions overlap leave overlapping pairs of alternatives
/// in each intersection, and those merge back into a single range.
#[test]
fn overlapping_consumer_unions_merge_into_one_range() {
    let ranges: Vec<String> = (0..64)
        .map(|patch| format!(">=1.0.{patch} <2.0.{patch} || >=1.1.{patch} <2.1.{patch}"))
        .collect();
    let ranges: Vec<&str> = ranges
        .iter()
        .map(String::as_str)
        .collect();

    assert_eq!(merge_ranges(&ranges, false).as_deref(), Some(">=1.0.63 <2.1.0"));
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
    opts.peers.auto_install_peers_from_highest_match = true;
    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], opts)
        .await
        .unwrap();

    let direct: Vec<&str> = result.peers_result.direct_dependencies_by_alias
        .keys()
        .map(String::as_str)
        .collect();
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
    let x_ranges: Vec<String> = calls
        .iter()
        .filter(|(n, _)| n == "x")
        .map(|(_, r)| r.clone())
        .collect();
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

/// A `name@1.0.0` that declares one peer at `peer_range`.
fn peer_declaring_lib(name: &str, peer: &str, peer_range: &str) -> ResolveResult {
    fake_result(
        name,
        "1.0.0",
        serde_json::json!({
            "name": name,
            "version": "1.0.0",
            "peerDependencies": { peer: peer_range },
        }),
    )
}

/// The specifiers `name` was resolved with.
fn resolved_specifiers(resolver: &StubResolver, name: &str) -> Vec<String> {
    resolver.calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(alias, _)| alias == name)
        .map(|(_, specifier)| specifier.clone())
        .collect()
}

/// The workspace shorthand names no version, so the hoist keeps its
/// protocol to resolve the peer to the workspace project.
#[tokio::test]
async fn auto_installs_a_workspace_shorthand_peer_with_its_protocol() {
    let mut table = HashMap::default();
    table.insert(
        ("lib".to_string(), "1.0.0".to_string()),
        peer_declaring_lib("lib", "peer", "workspace:^"),
    );
    table.insert(
        ("peer".to_string(), "workspace:^".to_string()),
        fake_result("peer", "1.0.0", serde_json::json!({ "name": "peer", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "lib": "1.0.0" }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    assert_eq!(resolved_specifiers(&resolver, "peer"), vec!["workspace:^".to_string()]);
    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("peer"),
        Some(&DepPath::from("peer@1.0.0".to_string())),
    );
}

#[tokio::test]
async fn auto_installs_a_peer_declared_with_compatible_workspace_and_plain_ranges() {
    let mut table = HashMap::default();
    table.insert(
        ("wants-workspace".to_string(), "1.0.0".to_string()),
        peer_declaring_lib("wants-workspace", "peer-c", "workspace:^1.0.0"),
    );
    table.insert(
        ("wants-plain".to_string(), "1.0.0".to_string()),
        peer_declaring_lib("wants-plain", "peer-c", "^1.0.0"),
    );
    table.insert(
        ("peer-c".to_string(), "^1.0.0".to_string()),
        fake_result("peer-c", "1.0.0", serde_json::json!({ "name": "peer-c", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "wants-workspace": "1.0.0",
        "wants-plain": "1.0.0",
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("peer-c"),
        Some(&DepPath::from("peer-c@1.0.0".to_string())),
    );
}

#[tokio::test]
async fn auto_installs_a_peer_declared_with_different_workspace_shorthands() {
    let mut table = HashMap::default();
    table.insert(
        ("wants-caret".to_string(), "1.0.0".to_string()),
        peer_declaring_lib("wants-caret", "peer", "workspace:^"),
    );
    table.insert(
        ("wants-tilde".to_string(), "1.0.0".to_string()),
        peer_declaring_lib("wants-tilde", "peer", "workspace:~"),
    );
    table.insert(
        ("peer".to_string(), "workspace:*".to_string()),
        fake_result("peer", "1.0.0", serde_json::json!({ "name": "peer", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "wants-caret": "1.0.0",
        "wants-tilde": "1.0.0",
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    assert_eq!(resolved_specifiers(&resolver, "peer"), vec!["workspace:*".to_string()]);
    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("peer"),
        Some(&DepPath::from("peer@1.0.0".to_string())),
    );
}

/// The shorthand keeps the hoisted peer on the workspace project while the
/// plain range narrows its version.
#[tokio::test]
async fn auto_installs_a_peer_declared_with_a_workspace_shorthand_and_a_plain_range() {
    let mut table = HashMap::default();
    table.insert(
        ("wants-workspace".to_string(), "1.0.0".to_string()),
        peer_declaring_lib("wants-workspace", "peer-c", "workspace:^"),
    );
    table.insert(
        ("wants-plain".to_string(), "1.0.0".to_string()),
        peer_declaring_lib("wants-plain", "peer-c", "^1.0.0"),
    );
    table.insert(
        ("peer-c".to_string(), "workspace:^1.0.0".to_string()),
        fake_result("peer-c", "1.0.0", serde_json::json!({ "name": "peer-c", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "wants-workspace": "1.0.0",
        "wants-plain": "1.0.0",
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    assert_eq!(resolved_specifiers(&resolver, "peer-c"), vec!["workspace:^1.0.0".to_string()]);
    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("peer-c"),
        Some(&DepPath::from("peer-c@1.0.0".to_string())),
    );
}

/// The overrider stands in for a version-scoped `peer-c@^1` override,
/// which only matches a semver range.
#[tokio::test]
async fn version_scoped_override_applies_to_a_workspace_peer_with_a_semver_body() {
    let mut table = HashMap::default();
    table.insert(
        ("lib".to_string(), "1.0.0".to_string()),
        peer_declaring_lib("lib", "peer-c", "workspace:^1.0.0"),
    );
    table.insert(
        ("peer-c".to_string(), "1.2.3".to_string()),
        fake_result("peer-c", "1.2.3", serde_json::json!({ "name": "peer-c", "version": "1.2.3" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "lib": "1.0.0" }));
    let selector: node_semver::Range = "^1".parse().unwrap();
    let overrider = move |name: &str, range: &str, _project_dir: &Path| {
        let matches = name == "peer-c"
            && range
                .parse::<node_semver::Range>()
                .is_ok_and(|range| selector.allows_any(&range));
        matches.then(|| "1.2.3".to_string())
    };
    let mut opts = default_opts();
    opts.resolution.override_bare_specifier = Some(Arc::new(overrider));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], opts)
        .await
        .unwrap();

    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("peer-c"),
        Some(&DepPath::from("peer-c@1.2.3".to_string())),
    );
}

/// A published package can carry a `workspace:` peer its publisher did
/// not rewrite. Its version body still installs from the registry.
#[tokio::test]
async fn auto_installs_a_published_workspace_peer_from_its_version() {
    let mut table = HashMap::default();
    table.insert(
        ("lib".to_string(), "1.0.0".to_string()),
        peer_declaring_lib("lib", "peer", "workspace:1.0.0"),
    );
    table.insert(
        ("peer".to_string(), "1.0.0".to_string()),
        fake_result("peer", "1.0.0", serde_json::json!({ "name": "peer", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "lib": "1.0.0" }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    assert_eq!(resolved_specifiers(&resolver, "peer"), vec!["1.0.0".to_string()]);
    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("peer"),
        Some(&DepPath::from("peer@1.0.0".to_string())),
    );
}
