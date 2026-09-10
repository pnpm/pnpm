use super::{
    DepPath, DependencyGroup, HashMap, Mutex, ResolveDependencyTreeOptions, ResolveOptions,
    ResolvePeersOptions, StubResolver, assert_eq, fake_manifest, fake_result,
    resolve_dependency_tree, resolve_peers,
};

#[tokio::test]
async fn pure_package_has_dep_path_equal_to_pkg_id() {
    let mut table = HashMap::default();
    table.insert(
        ("foo".to_string(), "^1.0.0".to_string()),
        fake_result("foo", "1.0.0", serde_json::json!({ "name": "foo", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));
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
    assert_eq!(
        result.direct_dependencies_by_alias.get("foo"),
        Some(&DepPath::from("foo@1.0.0".to_string())),
    );
    assert!(result.peer_dependency_issues.missing.is_empty());
    assert!(result.peer_dependency_issues.bad.is_empty());
}

/// A package's graph-node `depth` is the minimum across all
/// occurrences, even when the shallower one short-circuits through the
/// `pure_pkgs` fast path (which has no `NodeRecord`). `p` is reached at
/// depth 2 via `a → b → p` (walked first, so its record carries depth
/// 2) and at depth 1 via `c → p` (a pure-pkgs revisit). The rebuilt
/// graph must record depth 1. Regression guard for the `build_final_graph`
/// depth tie-break.
#[tokio::test]
async fn shallower_pure_pkgs_revisit_lowers_graph_depth() {
    let mut table = HashMap::default();
    table.insert(
        ("a".to_string(), "1.0.0".to_string()),
        fake_result(
            "a",
            "1.0.0",
            serde_json::json!({
                "name": "a",
                "version": "1.0.0",
                "dependencies": { "b": "1.0.0" }
            }),
        ),
    );
    table.insert(
        ("b".to_string(), "1.0.0".to_string()),
        fake_result(
            "b",
            "1.0.0",
            serde_json::json!({
                "name": "b",
                "version": "1.0.0",
                "dependencies": { "p": "1.0.0" }
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
                "dependencies": { "p": "1.0.0" }
            }),
        ),
    );
    // `p` has a dep `q` so it gets per-occurrence NodeIds (a shared
    // leaf would already carry its minimum depth in the tree). Its
    // whole subtree is peer-free, so the second, shallower occurrence
    // under `c` takes the `pure_pkgs` fast path — which records no
    // `NodeRecord`, the case the rebuild must still account for.
    table.insert(
        ("p".to_string(), "1.0.0".to_string()),
        fake_result(
            "p",
            "1.0.0",
            serde_json::json!({
                "name": "p",
                "version": "1.0.0",
                "dependencies": { "q": "1.0.0" }
            }),
        ),
    );
    table.insert(
        ("q".to_string(), "1.0.0".to_string()),
        fake_result("q", "1.0.0", serde_json::json!({ "name": "q", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    // `a` precedes `c` so `p` is first walked at depth 2 (`a → b → p`),
    // then revisited at depth 1 (`c → p`).
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "a": "1.0.0", "c": "1.0.0" }));
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
    let p_node = &result.graph[&DepPath::from("p@1.0.0".to_string())];
    assert_eq!(p_node.depth, 1, "p's graph depth must be the minimum (1), not 2");
}

/// A pure package (no peer deps, peer-clean subtree) reached
/// through multiple parents only realizes its children for the
/// occurrence the peer resolver walks first. Subsequent
/// occurrences hit the `purePkgs` short-circuit before
/// `realize_children` runs, so their [`TreeChildren::Lazy`]
/// stays Lazy. Regression guard against accidentally moving the
/// realize call above the short-circuit.
#[tokio::test]
async fn pure_revisit_leaves_lazy_children_unrealized() {
    use crate::resolved_tree::TreeChildren;
    let mut table = HashMap::default();
    table.insert(
        ("p1".to_string(), "^1.0.0".to_string()),
        fake_result(
            "p1",
            "1.0.0",
            serde_json::json!({
                "name": "p1",
                "version": "1.0.0",
                "dependencies": { "pure": "^1.0.0" }
            }),
        ),
    );
    table.insert(
        ("p2".to_string(), "^1.0.0".to_string()),
        fake_result(
            "p2",
            "1.0.0",
            serde_json::json!({
                "name": "p2",
                "version": "1.0.0",
                "dependencies": { "pure": "^1.0.0" }
            }),
        ),
    );
    // `pure` has a child so it is non-leaf (per-occurrence
    // NodeId), but no peer deps anywhere in the subtree — that
    // makes it eligible for `purePkgs`.
    table.insert(
        ("pure".to_string(), "^1.0.0".to_string()),
        fake_result(
            "pure",
            "1.0.0",
            serde_json::json!({
                "name": "pure",
                "version": "1.0.0",
                "dependencies": { "pure_leaf": "^1.0.0" }
            }),
        ),
    );
    table.insert(
        ("pure_leaf".to_string(), "^1.0.0".to_string()),
        fake_result(
            "pure_leaf",
            "1.0.0",
            serde_json::json!({ "name": "pure_leaf", "version": "1.0.0" }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "p1": "^1.0.0", "p2": "^1.0.0" }));

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

    // Sanity: `pure` got two per-occurrence tree entries, the
    // first carrying Realized children (eager walk), the second
    // carrying Lazy children (revisit).
    let pure_pre: Vec<(&crate::node_id::NodeId, bool)> = tree
        .dependencies_tree
        .iter()
        .filter(|(_, node)| node.resolved_package_id == "pure@1.0.0".into())
        .map(|(id, node)| (id, matches!(node.children, TreeChildren::Lazy { .. })))
        .collect();
    assert_eq!(pure_pre.len(), 2, "expected two occurrences of pure, got {pure_pre:?}");
    assert!(
        pure_pre.iter().any(|(_, is_lazy)| !*is_lazy),
        "first walk should produce a Realized entry",
    );
    assert!(pure_pre.iter().any(|(_, is_lazy)| *is_lazy), "revisit should produce a Lazy entry");

    resolve_peers(&mut tree, ResolvePeersOptions::default());

    // After peer resolution: the lazy occurrence stays Lazy
    // because `purePkgs` short-circuits before `realize_children`
    // is called.
    let still_lazy = tree
        .dependencies_tree
        .iter()
        .filter(|(_, node)| node.resolved_package_id == "pure@1.0.0".into())
        .filter(|(_, node)| matches!(node.children, TreeChildren::Lazy { .. }))
        .count();
    assert_eq!(
        still_lazy, 1,
        "purePkgs short-circuit must leave the revisit's lazy children un-realized",
    );
}
