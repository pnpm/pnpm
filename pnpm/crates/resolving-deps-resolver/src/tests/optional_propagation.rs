use super::{
    DependencyGroup, HashMap, Mutex, PackageManifest, ResolveDependencyTreeOptions, ResolveOptions,
    StubResolver, fake_result, resolve_dependency_tree,
};

/// `package.json` builder that takes both `dependencies` and
/// `optionalDependencies` blocks — the bundled [`fake_manifest`]
/// helper only writes to `dependencies` so it can't exercise the
/// importer-level optional flag.
#[expect(
    clippy::needless_pass_by_value,
    reason = "test helpers take owned literal fixtures by value to keep call sites clean"
)]
fn manifest_with_groups(
    prod: serde_json::Value,
    optional: serde_json::Value,
) -> (tempfile::TempDir, PackageManifest) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("package.json");
    let json = serde_json::json!({
        "name": "root",
        "version": "0.0.0",
        "dependencies": prod,
        "optionalDependencies": optional,
    });
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).expect("write package.json");
    let manifest = PackageManifest::from_path(path).expect("parse package.json");
    (tmp, manifest)
}

#[tokio::test]
async fn direct_optional_dep_seeds_resolved_package_optional_true() {
    let mut table = HashMap::default();
    table.insert(
        ("opt".to_string(), "^1.0.0".to_string()),
        fake_result("opt", "1.0.0", serde_json::json!({ "name": "opt", "version": "1.0.0" })),
    );
    table.insert(
        ("regular".to_string(), "^1.0.0".to_string()),
        fake_result(
            "regular",
            "1.0.0",
            serde_json::json!({ "name": "regular", "version": "1.0.0" }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = manifest_with_groups(
        serde_json::json!({ "regular": "^1.0.0" }),
        serde_json::json!({ "opt": "^1.0.0" }),
    );

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod, DependencyGroup::Optional],
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

    assert!(
        tree.packages.get("opt@1.0.0").expect("opt resolved").optional,
        "direct optionalDependencies entry marks the resolved package optional",
    );
    assert!(
        !tree.packages.get("regular@1.0.0").expect("regular resolved").optional,
        "direct dependencies entry stays optional: false",
    );
}

#[tokio::test]
async fn transitive_dep_under_optional_inherits_optional_true() {
    let mut table = HashMap::default();
    table.insert(
        ("opt".to_string(), "^1.0.0".to_string()),
        fake_result(
            "opt",
            "1.0.0",
            serde_json::json!({
                "name": "opt",
                "version": "1.0.0",
                "dependencies": { "transitive": "^1.0.0" }
            }),
        ),
    );
    table.insert(
        ("transitive".to_string(), "^1.0.0".to_string()),
        fake_result(
            "transitive",
            "1.0.0",
            serde_json::json!({ "name": "transitive", "version": "1.0.0" }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) =
        manifest_with_groups(serde_json::json!({}), serde_json::json!({ "opt": "^1.0.0" }));

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod, DependencyGroup::Optional],
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

    assert!(
        tree.packages.get("transitive@1.0.0").expect("transitive resolved").optional,
        "child of an optional-only parent inherits optional: true",
    );
}

#[tokio::test]
async fn shared_dep_via_non_optional_and_optional_paths_keeps_optional_false() {
    let mut table = HashMap::default();
    table.insert(
        ("opt".to_string(), "^1.0.0".to_string()),
        fake_result(
            "opt",
            "1.0.0",
            serde_json::json!({
                "name": "opt",
                "version": "1.0.0",
                "dependencies": { "shared": "^1.0.0" }
            }),
        ),
    );
    table.insert(
        ("regular".to_string(), "^1.0.0".to_string()),
        fake_result(
            "regular",
            "1.0.0",
            serde_json::json!({
                "name": "regular",
                "version": "1.0.0",
                "dependencies": { "shared": "^1.0.0" }
            }),
        ),
    );
    table.insert(
        ("shared".to_string(), "^1.0.0".to_string()),
        fake_result("shared", "1.0.0", serde_json::json!({ "name": "shared", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = manifest_with_groups(
        serde_json::json!({ "regular": "^1.0.0" }),
        serde_json::json!({ "opt": "^1.0.0" }),
    );

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod, DependencyGroup::Optional],
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

    let shared = tree.packages.get("shared@1.0.0").expect("shared resolved");
    assert!(
        !shared.optional,
        "AND-fold: a non-optional path through any consumer wins over an optional one",
    );
}

#[tokio::test]
async fn manifest_level_optional_dependencies_edge_propagates_to_child() {
    let mut table = HashMap::default();
    table.insert(
        ("regular".to_string(), "^1.0.0".to_string()),
        fake_result(
            "regular",
            "1.0.0",
            serde_json::json!({
                "name": "regular",
                "version": "1.0.0",
                "optionalDependencies": { "transitive": "^1.0.0" }
            }),
        ),
    );
    table.insert(
        ("transitive".to_string(), "^1.0.0".to_string()),
        fake_result(
            "transitive",
            "1.0.0",
            serde_json::json!({ "name": "transitive", "version": "1.0.0" }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) =
        manifest_with_groups(serde_json::json!({ "regular": "^1.0.0" }), serde_json::json!({}));

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod, DependencyGroup::Optional],
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

    assert!(
        !tree.packages.get("regular@1.0.0").expect("regular resolved").optional,
        "regular dep stays non-optional",
    );
    assert!(
        tree.packages.get("transitive@1.0.0").expect("transitive resolved").optional,
        "child reached only via a parent's optionalDependencies edge is optional",
    );
}

/// npm merges `optionalDependencies` into `dependencies` at publish
/// time, so a registry manifest lists the same child in both maps.
#[tokio::test]
async fn dep_listed_in_both_manifest_groups_yields_one_optional_edge() {
    let mut table = HashMap::default();
    table.insert(
        ("regular".to_string(), "^1.0.0".to_string()),
        fake_result(
            "regular",
            "1.0.0",
            serde_json::json!({
                "name": "regular",
                "version": "1.0.0",
                "dependencies": { "plat": "^1.0.0" },
                "optionalDependencies": { "plat": "^2.0.0" }
            }),
        ),
    );
    // Only the `dependencies` range resolves: a merge that kept the
    // `optionalDependencies` range would miss the table and drop the
    // edge as an optional resolution failure.
    table.insert(
        ("plat".to_string(), "^1.0.0".to_string()),
        fake_result("plat", "1.0.0", serde_json::json!({ "name": "plat", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) =
        manifest_with_groups(serde_json::json!({ "regular": "^1.0.0" }), serde_json::json!({}));

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod, DependencyGroup::Optional],
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

    assert!(
        tree.packages.get("plat@1.0.0").expect("plat resolved").optional,
        "the merged edge carries optional: true",
    );
    let plat_calls =
        resolver.calls.lock().unwrap().iter().filter(|(alias, _)| alias == "plat").count();
    assert_eq!(
        plat_calls, 1,
        "one merged edge resolves once; a duplicate non-optional edge would resolve again and defeat the optional-edge gates (e.g. the unsupported-platform prefetch skip)",
    );
}
