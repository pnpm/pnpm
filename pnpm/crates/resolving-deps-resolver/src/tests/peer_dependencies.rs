use super::{
    OverlayPickResolver, assert_eq, dependency_result, fake_result, resolve_settlement_tree,
    settlement_versions,
};

#[tokio::test(start_paused = true)]
async fn peer_shadowing_follows_the_occurrence_that_wins_the_level() {
    for provider_parent in ["a-parent", "b-parent"] {
        let shared = fake_result(
            "shared",
            "1.0.0",
            serde_json::json!({
                "name": "shared",
                "version": "1.0.0",
                "dependencies": { "pin": "1.0.0" },
                "peerDependencies": { "pin": "^1.0.0" }
            }),
        );
        let mut parent_a = serde_json::json!({ "mid-a": "1.0.0" });
        let mut parent_b = serde_json::json!({ "mid-b": "1.0.0" });
        match provider_parent {
            "a-parent" => parent_a["pin"] = "1.0.0".into(),
            "b-parent" => parent_b["pin"] = "1.0.0".into(),
            _ => unreachable!(),
        }
        let mut versions = settlement_versions([
            dependency_result("a-parent", &parent_a),
            dependency_result("mid-a", &serde_json::json!({ "shared": "^1.0.0" })),
            dependency_result("b-parent", &parent_b),
            dependency_result("mid-b", &serde_json::json!({ "shared": "1.0.0" })),
        ]);
        versions.insert("shared".to_string(), vec![shared]);
        for delayed_specifier in ["^1.0.0", "1.0.0"] {
            let resolver = OverlayPickResolver {
                versions: versions.clone(),
                delayed: ("shared".to_string(), delayed_specifier.to_string()),
            };
            let tree = resolve_settlement_tree(
                &resolver,
                serde_json::json!({ "a-parent": "1.0.0", "b-parent": "1.0.0" }),
            )
            .await;

            let shared_children = tree.children_by_id.get("shared@1.0.0").unwrap();
            let peer_set = &tree.packages["shared@1.0.0"].peer_set;
            dbg!(provider_parent, delayed_specifier, shared_children, peer_set);
            let owner_takes_peer = provider_parent == "a-parent";
            assert_eq!(shared_children.is_empty(), owner_takes_peer);
            assert_eq!(peer_set.taken().contains_key("pin"), owner_takes_peer);
            assert_eq!(peer_set.declared()["pin"].version, "^1.0.0");
            assert_eq!(tree.all_peer_dep_names.contains("pin"), owner_takes_peer);
        }
    }
}
