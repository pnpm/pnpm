use super::{
    OverlayPickResolver, dependency_result, fake_result, resolve_settlement_tree,
    settlement_versions,
};

/// The winning occurrence's peer-shadowed set is the one that filters
/// the package's children: `shared` declares `pin` as both a
/// dependency and a peer, so the edge survives only where the scope
/// reaching it cannot supply the peer itself. Down `a-parent`, which
/// resolves `pin` beside the node that depends on `shared`, it does
/// not — and `a-parent` sorts first.
#[tokio::test(start_paused = true)]
async fn peer_shadowing_follows_the_occurrence_that_wins_the_level() {
    let shadowing_shared = fake_result(
        "shared",
        "1.0.0",
        serde_json::json!({
            "name": "shared",
            "version": "1.0.0",
            "dependencies": { "pin": "1.0.0" },
            "peerDependencies": { "pin": "^1.0.0" }
        }),
    );
    // `shared` is reached at the same depth down both paths, and its
    // own peer-shadowing scope is the level of the parent that reaches
    // it: only `a-parent`'s level resolves `pin`.
    let mut versions = settlement_versions([
        dependency_result("a-parent", &serde_json::json!({ "mid-a": "1.0.0", "pin": "1.0.0" })),
        dependency_result("mid-a", &serde_json::json!({ "shared": "^1.0.0" })),
        dependency_result("b-parent", &serde_json::json!({ "mid-b": "1.0.0" })),
        dependency_result("mid-b", &serde_json::json!({ "shared": "1.0.0" })),
    ]);
    versions.insert("shared".to_string(), vec![shadowing_shared]);
    let resolver =
        OverlayPickResolver { versions, delayed: ("shared".to_string(), "^1.0.0".to_string()) };

    let tree = resolve_settlement_tree(
        &resolver,
        serde_json::json!({ "a-parent": "1.0.0", "b-parent": "1.0.0" }),
    )
    .await;

    let shared_children = tree.children_by_id.get("shared@1.0.0").expect("shared children");
    eprintln!("SHARED CHILDREN:\n{shared_children:#?}\n");
    assert!(shared_children.is_empty());
}
