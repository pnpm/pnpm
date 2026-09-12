use super::{
    DependencyGroup, HashMap, Mutex, ReplacingHook, ResolveDependencyTreeError,
    ResolveDependencyTreeOptions, ResolveOptions, StubResolver, assert_eq, fake_manifest,
    fake_result, resolve_dependency_tree,
};

/// A transitive dependency whose alias contains `..` segments would
/// escape the `node_modules` directory when joined onto a modules
/// path. The walker rejects it before any further resolution work.
#[tokio::test]
async fn transitive_dep_with_traversal_alias_is_rejected() {
    let mut table = HashMap::default();
    table.insert(
        ("normal".to_string(), "1.0.0".to_string()),
        fake_result(
            "normal",
            "1.0.0",
            serde_json::json!({
                "name": "normal",
                "version": "1.0.0",
                "dependencies": { "@x/../../../../../.git/hooks": "1.0.0" },
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "normal": "1.0.0" }));

    let err = resolve_dependency_tree(
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
    .expect_err("traversal alias must error");
    match err {
        ResolveDependencyTreeError::InvalidDependencyName { parent, alias } => {
            assert_eq!(alias, "@x/../../../../../.git/hooks");
            assert!(
                parent.contains("normal"),
                "parent must name the offending package, got {parent:?}",
            );
        }
        other => panic!("expected InvalidDependencyName, got {other:?}"),
    }
}

/// pnpm's `createReadPackageHook` order is packageExtensions → readPackage
/// hooks → overrides: a pnpmfile hook that replaces the manifest must not
/// erase the overrides. Here the hook replaces foo's manifest (bar pinned to
/// ^2.0.0) and the overrides hook rewrites bar to ^3.0.0 — the resolved edge
/// must follow the override.
#[tokio::test]
async fn overrides_hook_applies_after_the_pnpmfile_hook() {
    let mut table = HashMap::default();
    table.insert(
        ("foo".to_string(), "^1.0.0".to_string()),
        fake_result(
            "foo",
            "1.2.0",
            serde_json::json!({
                "name": "foo",
                "version": "1.2.0",
                "dependencies": { "bar": "^1.0.0" }
            }),
        ),
    );
    table.insert(
        ("bar".to_string(), "^3.0.0".to_string()),
        fake_result("bar", "3.1.0", serde_json::json!({ "name": "bar", "version": "3.1.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));

    let replacing_hook = ReplacingHook {
        replacement: serde_json::json!({
            "name": "foo",
            "version": "1.2.0",
            "dependencies": { "bar": "^2.0.0" }
        }),
    };
    let overrides_hook: crate::ManifestHook = std::sync::Arc::new(|manifest| {
        let mut owned = (*manifest).clone();
        if let Some(deps) = owned.get_mut("dependencies").and_then(serde_json::Value::as_object_mut)
            && deps.contains_key("bar")
        {
            deps.insert("bar".to_string(), serde_json::Value::String("^3.0.0".to_string()));
        }
        std::sync::Arc::new(owned)
    });

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: None,
            manifest_hook: None,
            overrides_hook: Some(overrides_hook),
            pnpmfile_hook: Some(std::sync::Arc::new(replacing_hook)),
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .unwrap();

    assert!(tree.packages.contains_key("bar@3.1.0"), "override must win over the hook");
    let calls = resolver.calls.lock().unwrap().clone();
    assert!(
        calls.contains(&("bar".to_string(), "^3.0.0".to_string())),
        "bar must be resolved with the overridden range, got: {calls:?}",
    );
}
