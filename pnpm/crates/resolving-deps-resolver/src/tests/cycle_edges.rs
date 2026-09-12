use super::{
    DependencyGroup, HashMap, Mutex, ResolveDependencyTreeOptions, ResolveOptions, StubResolver,
    fake_manifest, fake_result, resolve_dependency_tree,
};
use crate::resolve_peers::{ResolvePeersOptions, resolve_peers};

/// A two-package cycle keeps its closing edge: the tree builder's
/// cycle gate drops only the *second* lap of a cycle, so `b`'s
/// snapshot still lists `a` (the repeated node's pruned children
/// are restored via the previously-resolved-children merge).
#[tokio::test]
async fn cycle_closing_edge_reaches_the_graph() {
    let mut table = HashMap::default();
    table.insert(
        ("a".to_string(), "1.0.0".to_string()),
        fake_result(
            "a",
            "1.0.0",
            serde_json::json!({
                "name": "a",
                "version": "1.0.0",
                "dependencies": { "b": "1.0.0" },
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
                "dependencies": { "a": "1.0.0" },
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "a": "1.0.0" }));

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

    let a_node =
        result.graph.get(&crate::DepPath::from("a@1.0.0".to_string())).expect("a in graph");
    assert!(a_node.children.contains_key("b"), "a keeps its b edge");
    let b_node =
        result.graph.get(&crate::DepPath::from("b@1.0.0".to_string())).expect("b in graph");
    assert!(
        b_node.children.contains_key("a"),
        "the cycle-closing edge b -> a must reach the graph: {:?}",
        b_node.children.keys().collect::<Vec<_>>(),
    );
}
