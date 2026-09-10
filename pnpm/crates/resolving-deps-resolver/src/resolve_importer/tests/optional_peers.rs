use super::{
    Arc, DepPath, DependencyGroup, HashMap, HashSet, Mutex, PreferredVersions,
    ResolveImporterOptions, StubResolver, VersionSelectorEntry, VersionSelectorType,
    VersionSelectors, aliased_fake_result, assert_eq, default_opts, fake_manifest,
    fake_manifest_json, fake_result, resolve_importer,
};

/// An optional peer with a real `peerDependencies` entry whose
/// provider is resolved anywhere in the tree (here: deep under a
/// sibling) IS hoisted: every run-resolved version folds into the
/// preferred-versions set and the optional-peer hoist resolves the
/// peer against it after the wave (verified against pnpm 11.6.0 — the
/// `eslint` + `cosmiconfig-typescript-loader` shape, where `eslint`
/// gains `(jiti@x)`). For
/// <https://github.com/pnpm/pnpm/issues/12266>.
#[tokio::test]
async fn optional_peer_with_real_entry_is_hoisted_from_resolved_tree() {
    let mut table = HashMap::default();
    table.insert(
        ("needs-opt".to_string(), "1.0.0".to_string()),
        fake_result(
            "needs-opt",
            "1.0.0",
            serde_json::json!({
                "name": "needs-opt",
                "version": "1.0.0",
                "peerDependencies": { "opt": "^1.0.0" },
                "peerDependenciesMeta": { "opt": { "optional": true } },
            }),
        ),
    );
    table.insert(
        ("provider".to_string(), "1.0.0".to_string()),
        fake_result(
            "provider",
            "1.0.0",
            serde_json::json!({
                "name": "provider",
                "version": "1.0.0",
                "dependencies": { "opt": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("opt".to_string(), "1.0.0".to_string()),
        fake_result("opt", "1.0.0", serde_json::json!({ "name": "opt", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "needs-opt": "1.0.0",
        "provider": "1.0.0",
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct.contains(&"opt"), "optional peer `opt` must be hoisted: {direct:?}");
    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("needs-opt"),
        Some(&DepPath::from("needs-opt@1.0.0(opt@1.0.0)".to_string())),
    );
}

/// A peer declared only via `peerDependenciesMeta` on a direct package
/// (no `peerDependencies` entry — the `debug` / `supports-color`
/// shape) is an implied optional `"*"` peer and joins the optional-peer
/// hoist exactly like an explicitly declared optional peer: when a
/// satisfying version is already in the graph, it is deduped onto that
/// version and resolved for the requiring package. This keeps the
/// outcome identical whether the peer came from the registry manifest
/// or from a lockfile snapshot (which materializes implied peers into
/// `peerDependencies`), so resolution does not drift across lockfile
/// round-trips.
#[tokio::test]
async fn meta_only_optional_peer_is_hoisted_like_a_declared_optional_peer() {
    let mut table = HashMap::default();
    table.insert(
        ("needs-opt".to_string(), "1.0.0".to_string()),
        fake_result(
            "needs-opt",
            "1.0.0",
            serde_json::json!({
                "name": "needs-opt",
                "version": "1.0.0",
                "peerDependenciesMeta": { "opt": { "optional": true } },
            }),
        ),
    );
    table.insert(
        ("provider".to_string(), "1.0.0".to_string()),
        fake_result(
            "provider",
            "1.0.0",
            serde_json::json!({
                "name": "provider",
                "version": "1.0.0",
                "dependencies": { "opt": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("opt".to_string(), "1.0.0".to_string()),
        fake_result("opt", "1.0.0", serde_json::json!({ "name": "opt", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "needs-opt": "1.0.0",
        "provider": "1.0.0",
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct.contains(&"opt"), "meta-only peer `opt` must be hoisted: {direct:?}");
    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("needs-opt"),
        Some(&DepPath::from("needs-opt@1.0.0(opt@1.0.0)".to_string())),
    );
}

#[tokio::test]
async fn real_peer_provider_from_direct_child_is_appended_as_hidden_direct_dep() {
    let mut table = HashMap::default();
    table.insert(
        ("host".to_string(), "1.0.0".to_string()),
        fake_result(
            "host",
            "1.0.0",
            serde_json::json!({
                "name": "host",
                "version": "1.0.0",
                "dependencies": {
                    "peer-user": "1.0.0",
                    "provider": "1.0.0",
                },
            }),
        ),
    );
    table.insert(
        ("peer-user".to_string(), "1.0.0".to_string()),
        fake_result(
            "peer-user",
            "1.0.0",
            serde_json::json!({
                "name": "peer-user",
                "version": "1.0.0",
                "peerDependencies": { "provider": "^1.0.0" },
            }),
        ),
    );
    table.insert(
        ("provider".to_string(), "1.0.0".to_string()),
        fake_result(
            "provider",
            "1.0.0",
            serde_json::json!({ "name": "provider", "version": "1.0.0" }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "host": "1.0.0" }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("provider"),
        Some(&DepPath::from("provider@1.0.0".to_string())),
        "provider should be available to the importer peer pass without being a manifest dep",
    );
    assert!(
        result
            .peers_result
            .graph
            .contains_key(&DepPath::from("peer-user@1.0.0(provider@1.0.0)".to_string())),
        "peer-user should resolve provider from host's child dependency",
    );
}

#[tokio::test]
async fn meta_only_peer_provider_from_direct_child_is_appended_as_hidden_direct_dep() {
    let mut table = HashMap::default();
    table.insert(
        ("host".to_string(), "1.0.0".to_string()),
        fake_result(
            "host",
            "1.0.0",
            serde_json::json!({
                "name": "host",
                "version": "1.0.0",
                "dependencies": {
                    "peer-user": "1.0.0",
                    "provider": "1.0.0",
                },
            }),
        ),
    );
    table.insert(
        ("peer-user".to_string(), "1.0.0".to_string()),
        fake_result(
            "peer-user",
            "1.0.0",
            serde_json::json!({
                "name": "peer-user",
                "version": "1.0.0",
                "peerDependenciesMeta": { "provider": { "optional": true } },
            }),
        ),
    );
    table.insert(
        ("provider".to_string(), "1.0.0".to_string()),
        fake_result(
            "provider",
            "1.0.0",
            serde_json::json!({ "name": "provider", "version": "1.0.0" }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "host": "1.0.0" }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("provider"),
        Some(&DepPath::from("provider@1.0.0".to_string())),
        "a resolved meta-only peer feeds auto-installed hidden direct deps like a declared one",
    );
    assert!(
        result
            .peers_result
            .graph
            .contains_key(&DepPath::from("peer-user@1.0.0(provider@1.0.0)".to_string())),
        "meta-only peers resolve in the final peer graph when provider is in scope",
    );
}

#[tokio::test]
async fn auto_install_does_not_install_same_missing_peer_twice() {
    let mut table = HashMap::default();
    table.insert(
        ("outer".to_string(), "1.0.0".to_string()),
        fake_result(
            "outer",
            "1.0.0",
            serde_json::json!({
                "name": "outer",
                "version": "1.0.0",
                "dependencies": { "inner": "1.0.0" },
                "peerDependencies": { "y": "^1.0.0" },
            }),
        ),
    );
    table.insert(
        ("inner".to_string(), "1.0.0".to_string()),
        fake_result(
            "inner",
            "1.0.0",
            serde_json::json!({
                "name": "inner",
                "version": "1.0.0",
                "peerDependencies": { "y": "^1.0.0" },
            }),
        ),
    );
    table.insert(
        ("y".to_string(), "^1.0.0".to_string()),
        fake_result("y", "1.0.0", serde_json::json!({ "name": "y", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "outer": "1.0.0" }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let y_entries: Vec<&DepPath> =
        result.peers_result.graph.keys().filter(|dp| dp.to_string().starts_with("y@")).collect();
    assert_eq!(y_entries.len(), 1, "expected one y entry, got: {y_entries:?}");
    let calls = resolver.calls.lock().unwrap();
    let y_calls = calls.iter().filter(|(n, _)| n == "y").count();
    assert_eq!(y_calls, 1, "y should be resolved at most once");
}

/// Prefer the peer dependency version already used in the root: when
/// the importer declares the peer itself, its pinned version wins via
/// the importer-peerDependencies seed — even if `latest` would resolve
/// higher.
#[tokio::test]
async fn auto_install_prefers_peer_version_pinned_in_importer_peerdeps() {
    let mut table = HashMap::default();
    table.insert(
        ("has-y-peer".to_string(), "1.0.0".to_string()),
        fake_result(
            "has-y-peer",
            "1.0.0",
            serde_json::json!({
                "name": "has-y-peer",
                "version": "1.0.0",
                "peerDependencies": { "y": ">=1.0.0" },
            }),
        ),
    );
    // The importer pinned `y: ^1.0.0` so the resolver only sees that
    // spec — never `>=1.0.0` (the peer range). Were the importer
    // peerDeps not walked, the picker would fall back to the peer
    // range and might pick y@2.0.0.
    table.insert(
        ("y".to_string(), "^1.0.0".to_string()),
        fake_result("y", "1.0.0", serde_json::json!({ "name": "y", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest_json(serde_json::json!({
        "name": "root",
        "version": "0.0.0",
        "peerDependencies": {
            "has-y-peer": "1.0.0",
            "y": "^1.0.0",
        },
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct.contains(&"y"), "importer's own peer dep should land as direct: {direct:?}");
    assert!(direct.contains(&"has-y-peer"));
    let calls = resolver.calls.lock().unwrap();
    let y_ranges: Vec<String> =
        calls.iter().filter(|(n, _)| n == "y").map(|(_, r)| r.clone()).collect();
    assert_eq!(
        y_ranges,
        vec!["^1.0.0".to_string()],
        "y should resolve via importer's spec only, got: {y_ranges:?}",
    );
}

#[tokio::test]
async fn auto_install_hoisted_peer_dep_reuses_regular_dep_version() {
    let mut table = HashMap::default();
    table.insert(
        ("has-c-in-deps".to_string(), "1.0.0".to_string()),
        fake_result(
            "has-c-in-deps",
            "1.0.0",
            serde_json::json!({
                "name": "has-c-in-deps",
                "version": "1.0.0",
                "dependencies": { "c": "2.0.0" },
            }),
        ),
    );
    table.insert(
        ("wants-c".to_string(), "1.0.0".to_string()),
        fake_result(
            "wants-c",
            "1.0.0",
            serde_json::json!({
                "name": "wants-c",
                "version": "1.0.0",
                "peerDependencies": { "c": "^2.0.0" },
            }),
        ),
    );
    table.insert(
        ("c".to_string(), "2.0.0".to_string()),
        fake_result("c", "2.0.0", serde_json::json!({ "name": "c", "version": "2.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "has-c-in-deps": "1.0.0",
        "wants-c": "1.0.0",
    }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let c_entries: Vec<String> = result
        .peers_result
        .graph
        .keys()
        .map(ToString::to_string)
        .filter(|dp| dp.starts_with("c@"))
        .collect();
    assert_eq!(
        c_entries,
        vec!["c@2.0.0".to_string()],
        "expected one c@2.0.0 entry (not a second copy via the peer arm), got: {c_entries:?}",
    );
}

/// Regression test for <https://github.com/pnpm/pnpm/issues/11999>.
///
/// Pacquet's `resolve_peers` walks synchronously with an `in_progress`
/// set, so the deadlock that hit pnpm does not occur here — but the
/// scenario has to keep terminating with a graph entry for the aliased
/// root and for each pair of mutually-peer-depending leaves.
///
/// Layout (from the bug report): an aliased install `a@npm:a-real`
/// pulls in `b-real` and `c-real`. Each of those depends on one half
/// of a mutual-peer pair (`x` ↔ `y`) and peer-depends on the aliased
/// root (`a@npm:a-real`). The hoist loop auto-installs `x` and `y` at
/// the importer level, where their cycle surfaces.
#[tokio::test]
async fn aliased_install_with_transitive_mutual_peer_cycle_terminates() {
    let mut table = HashMap::default();
    table.insert(
        ("a".to_string(), "npm:a-real@1.0.0".to_string()),
        aliased_fake_result(
            "a",
            "a-real",
            "1.0.0",
            serde_json::json!({
                "name": "a-real",
                "version": "1.0.0",
                "dependencies": {
                    "b": "npm:b-real@1.0.0",
                    "c": "npm:c-real@1.0.0",
                },
            }),
        ),
    );
    table.insert(
        ("b".to_string(), "npm:b-real@1.0.0".to_string()),
        aliased_fake_result(
            "b",
            "b-real",
            "1.0.0",
            serde_json::json!({
                "name": "b-real",
                "version": "1.0.0",
                "dependencies": { "x": "1.0.0" },
                "peerDependencies": { "a": "npm:a-real@1.0.0" },
            }),
        ),
    );
    table.insert(
        ("c".to_string(), "npm:c-real@1.0.0".to_string()),
        aliased_fake_result(
            "c",
            "c-real",
            "1.0.0",
            serde_json::json!({
                "name": "c-real",
                "version": "1.0.0",
                "dependencies": { "y": "1.0.0" },
                "peerDependencies": { "a": "npm:a-real@1.0.0" },
            }),
        ),
    );
    table.insert(
        ("x".to_string(), "1.0.0".to_string()),
        fake_result(
            "x",
            "1.0.0",
            serde_json::json!({
                "name": "x",
                "version": "1.0.0",
                "peerDependencies": { "y": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("y".to_string(), "1.0.0".to_string()),
        fake_result(
            "y",
            "1.0.0",
            serde_json::json!({
                "name": "y",
                "version": "1.0.0",
                "peerDependencies": { "x": "1.0.0" },
            }),
        ),
    );

    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "a": "npm:a-real@1.0.0" }));

    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], default_opts())
        .await
        .unwrap();

    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert!(direct.contains(&"a"), "aliased root must surface as a direct dep: {direct:?}");
    assert!(direct.contains(&"x"), "missing peer x must be auto-installed: {direct:?}");
    assert!(direct.contains(&"y"), "missing peer y must be auto-installed: {direct:?}");

    let a_dep_path = result
        .peers_result
        .direct_dependencies_by_alias
        .get("a")
        .expect("alias `a` must be in the result")
        .to_string();
    assert!(
        a_dep_path.starts_with("a-real@1.0.0"),
        "aliased dep path must start with the real package id, got {a_dep_path}",
    );

    let dep_paths: HashSet<String> =
        result.peers_result.graph.keys().map(ToString::to_string).collect();
    assert!(
        dep_paths.iter().any(|dp| dp.starts_with("x@1.0.0")),
        "x must appear in the graph: {dep_paths:?}",
    );
    assert!(
        dep_paths.iter().any(|dp| dp.starts_with("y@1.0.0")),
        "y must appear in the graph: {dep_paths:?}",
    );
}

/// `hoistPeers` is `autoInstallPeers || dedupePeerDependents`: with both
/// off, a missing optional peer stays missing even though a preferred
/// version is in scope, so the packages that declare it keep an
/// unsuffixed snapshot.
#[tokio::test]
async fn both_hoist_settings_off_leaves_the_optional_peer_missing() {
    let mut table = HashMap::default();
    table.insert(
        ("abc".to_string(), "1.0.0".to_string()),
        fake_result(
            "abc",
            "1.0.0",
            serde_json::json!({
                "name": "abc",
                "version": "1.0.0",
                "peerDependencies": { "peer-c": "^1.0.0" },
                "peerDependenciesMeta": { "peer-c": { "optional": true } },
            }),
        ),
    );
    table.insert(
        ("peer-c".to_string(), "1.0.0".to_string()),
        fake_result("peer-c", "1.0.0", serde_json::json!({ "name": "peer-c", "version": "1.0.0" })),
    );
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "abc": "1.0.0" }));

    // A sibling importer already resolved the peer, so the optional
    // hoist would have a version to pick.
    let seeded_preferred_versions = || {
        let mut selectors = VersionSelectors::new();
        selectors
            .insert("1.0.0".to_string(), VersionSelectorEntry::Plain(VersionSelectorType::Version));
        PreferredVersions::from([("peer-c".to_string(), selectors)])
    };

    let hoisting_off = ResolveImporterOptions {
        auto_install_peers: false,
        dedupe_peer_dependents: false,
        all_preferred_versions: Arc::new(seeded_preferred_versions()),
        ..default_opts()
    };
    let resolver = StubResolver { table: table.clone(), calls: Mutex::new(Vec::new()) };
    let result = resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], hoisting_off)
        .await
        .unwrap();
    let direct: Vec<&str> =
        result.peers_result.direct_dependencies_by_alias.keys().map(String::as_str).collect();
    assert_eq!(direct, ["abc"]);
    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("abc"),
        Some(&DepPath::from("abc@1.0.0".to_string())),
    );

    // `dedupePeerDependents` alone still hoists it, without
    // `autoInstallPeers`.
    let dedupe_only = ResolveImporterOptions {
        auto_install_peers: false,
        dedupe_peer_dependents: true,
        all_preferred_versions: Arc::new(seeded_preferred_versions()),
        ..default_opts()
    };
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let result =
        resolve_importer(&resolver, &manifest, [DependencyGroup::Prod], dedupe_only).await.unwrap();
    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("abc"),
        Some(&DepPath::from("abc@1.0.0(peer-c@1.0.0)".to_string())),
    );
}
