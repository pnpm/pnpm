use super::{
    DelayedAliasResolver, DependencyGroup, HashMap, Mutex, OverlayPickResolver, RecordingHooks,
    ResolveDependencyTreeError, ResolveDependencyTreeOptions, ResolveOptions, StubResolver,
    assert_eq, dependency_result, fake_manifest, fake_result, resolve_dependency_tree,
    resolve_settlement_tree, settlement_versions,
};

/// A package's children follow the occurrence that owns them — the
/// shallowest, here the one under `wrap`, whose level prefers
/// `pin@1.0.0` — however slowly that occurrence resolves
/// (<https://github.com/pnpm/pnpm/issues/13685>).
#[tokio::test(start_paused = true)]
async fn children_follow_the_shallowest_occurrence_however_late_it_resolves() {
    let resolver = OverlayPickResolver {
        versions: settlement_versions([
            dependency_result("slow", &serde_json::json!({ "wrap": "1.0.0" })),
            dependency_result("wrap", &serde_json::json!({ "pin": "1.0.0", "shared": "^1.0.0" })),
            dependency_result("deep", &serde_json::json!({ "mid": "1.0.0" })),
            dependency_result("mid", &serde_json::json!({ "nested": "1.0.0" })),
            dependency_result("nested", &serde_json::json!({ "shared": "^1.0.0" })),
        ]),
        delayed: ("wrap".to_string(), "1.0.0".to_string()),
    };

    let tree =
        resolve_settlement_tree(&resolver, serde_json::json!({ "deep": "1.0.0", "slow": "1.0.0" }))
            .await;

    let shared_children = tree.children_by_id.get("shared@1.0.0").expect("shared children");
    assert_eq!(shared_children.len(), 1);
    assert_eq!(&*shared_children[0].pkg_id, "pin@1.0.0");
}

/// Occurrences at the same depth are settled by their parent path, so
/// the one under `a-parent` decides even when the one under `b-parent`
/// resolves first.
#[tokio::test(start_paused = true)]
async fn same_depth_occurrences_are_settled_by_parent_path() {
    let resolver = OverlayPickResolver {
        versions: settlement_versions([
            dependency_result(
                "a-parent",
                &serde_json::json!({ "pin": "1.0.0", "shared": "^1.0.0" }),
            ),
            dependency_result(
                "b-parent",
                &serde_json::json!({ "pin": "1.5.0", "shared": "1.0.0" }),
            ),
        ]),
        delayed: ("shared".to_string(), "^1.0.0".to_string()),
    };

    let tree = resolve_settlement_tree(
        &resolver,
        serde_json::json!({ "a-parent": "1.0.0", "b-parent": "1.0.0" }),
    )
    .await;

    let shared_children = tree.children_by_id.get("shared@1.0.0").expect("shared children");
    assert_eq!(shared_children.len(), 1);
    assert_eq!(&*shared_children[0].pkg_id, "pin@1.0.0");
}

#[tokio::test]
async fn walks_dependencies_and_builds_flat_tree() {
    let mut table = HashMap::default();
    table.insert(
        ("foo".to_string(), "^1.0.0".to_string()),
        fake_result(
            "foo",
            "1.2.0",
            serde_json::json!({
                "name": "foo",
                "version": "1.2.0",
                "dependencies": { "bar": "^2.0.0" }
            }),
        ),
    );
    table.insert(
        ("bar".to_string(), "^2.0.0".to_string()),
        fake_result(
            "bar",
            "2.3.0",
            serde_json::json!({
                "name": "bar",
                "version": "2.3.0",
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));

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

    assert_eq!(tree.direct.len(), 1);
    assert_eq!(tree.direct[0].alias, "foo");
    assert_eq!(tree.direct[0].id, "foo@1.2.0");
    assert_eq!(tree.packages.len(), 2);
    assert!(tree.packages.contains_key("foo@1.2.0"));
    let foo_node_id = &tree.direct[0].node_id;
    let foo_tree_node = tree.dependencies_tree.get(foo_node_id).unwrap();
    assert_eq!(foo_tree_node.children.realized().len(), 1);
    let bar_node_id = foo_tree_node.children.realized().get("bar").unwrap();
    let bar_tree_node = tree.dependencies_tree.get(bar_node_id).unwrap();
    assert_eq!(&*bar_tree_node.resolved_package_id, "bar@2.3.0");
    assert!(tree.policy_violations.is_empty());
}

#[tokio::test]
async fn shallower_revisit_takes_over_shared_children_context() {
    let mut table = HashMap::default();
    table.insert(
        ("a".to_string(), "1.0.0".to_string()),
        fake_result(
            "a",
            "1.0.0",
            serde_json::json!({
                "name": "a",
                "version": "1.0.0",
                "dependencies": { "cycle": "1.0.0" }
            }),
        ),
    );
    table.insert(
        ("cycle".to_string(), "1.0.0".to_string()),
        fake_result(
            "cycle",
            "1.0.0",
            serde_json::json!({
                "name": "cycle",
                "version": "1.0.0",
                "dependencies": { "shared": "1.0.0" }
            }),
        ),
    );
    table.insert(
        ("c".to_string(), "1.0.0".to_string()),
        fake_result(
            "c",
            "1.0.0",
            serde_json::json!({
                "name": "c",
                "version": "1.0.0",
                "dependencies": { "shared": "1.0.0" }
            }),
        ),
    );
    table.insert(
        ("shared".to_string(), "1.0.0".to_string()),
        fake_result(
            "shared",
            "1.0.0",
            serde_json::json!({
                "name": "shared",
                "version": "1.0.0",
                "dependencies": { "cycle": "1.0.0" }
            }),
        ),
    );
    let resolver = DelayedAliasResolver { table, delayed_alias: "c".to_string() };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "a": "1.0.0",
        "c": "1.0.0"
    }));

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

    let shared_children = tree.children_by_id.get("shared@1.0.0").expect("shared children");
    assert_eq!(shared_children.len(), 1);
    assert_eq!(shared_children[0].alias, "cycle");
    assert_eq!(&*shared_children[0].pkg_id, "cycle@1.0.0");
}

/// A chain that declines every spec (every `resolve()` returns
/// `Ok(None)`) must NOT silently drop the edge — that would leave
/// installs missing transitive deps and report success. The walker
/// surfaces `SpecNotSupported` with the offending specifier
/// rendered as `alias@specifier`, so callers can produce the
/// `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER` diagnostic the chain
/// dispatcher does.
#[tokio::test]
async fn declined_specifier_surfaces_spec_not_supported_error() {
    let resolver = StubResolver { table: HashMap::default(), calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "git+ssh://example.com" }));

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
    .expect_err("declined spec must error");
    match err {
        ResolveDependencyTreeError::SpecNotSupported { specifier } => {
            assert_eq!(specifier, "foo@git+ssh://example.com");
        }
        other => panic!("expected SpecNotSupported, got {other:?}"),
    }
}

/// A host needs the directory to tell a workspace project's dependency
/// instance apart from registry packages and substitute its raw manifest.
#[tokio::test]
async fn read_package_hook_receives_the_directory_of_directory_resolutions() {
    use pnpm_lockfile::{DirectoryResolution, LockfileResolution};

    let mut injected = fake_result(
        "injected",
        "1.0.0",
        serde_json::json!({ "name": "injected", "version": "1.0.0" }),
    );
    injected.resolution = LockfileResolution::Directory(DirectoryResolution {
        directory: "packages/injected".to_string(),
    });
    let mut table = HashMap::default();
    table.insert(("injected".to_string(), "file:packages/injected".to_string()), injected);
    table.insert(
        ("regular".to_string(), "^2.0.0".to_string()),
        fake_result(
            "regular",
            "2.1.0",
            serde_json::json!({ "name": "regular", "version": "2.1.0" }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "injected": "file:packages/injected",
        "regular": "^2.0.0",
    }));
    let calls = std::sync::Arc::new(Mutex::new(Vec::new()));
    let hooks = RecordingHooks { calls: std::sync::Arc::clone(&calls) };

    resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: None,
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: Some(std::sync::Arc::new(hooks)),
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .unwrap();

    let mut calls = calls.lock().unwrap().clone();
    calls.sort();
    assert_eq!(
        calls,
        vec![
            ("injected".to_string(), Some("packages/injected".to_string())),
            ("regular".to_string(), None),
        ],
    );
}
