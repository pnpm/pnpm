use super::{
    BTreeMap, DependencyGroup, HashMap, Mutex, RecordingResolver, WorkspaceImporter, assert_eq,
    fake_manifest, fake_result, graph_versions_of, importer_opts, importer_scoped_update_lockfile,
    resolve_importer_scoped_update_direct, resolve_workspace, workspace_opts,
};

#[tokio::test]
async fn importer_scoped_update_drop_only_is_order_independent() {
    for order in [["selected", "unselected"], ["unselected", "selected"]] {
        let direct = resolve_importer_scoped_update_direct(
            order,
            crate::UpdateReuseScope::Except(std::iter::once(("pkg".to_string(), None)).collect()),
        )
        .await;
        assert_eq!(direct["selected"], "pkg@100.1.0");
        assert_eq!(direct["unselected"], "pkg@100.0.0");
    }
}

#[tokio::test]
async fn importer_scoped_update_drop_all_is_order_independent() {
    for order in [["selected", "unselected"], ["unselected", "selected"]] {
        let direct =
            resolve_importer_scoped_update_direct(order, crate::UpdateReuseScope::None).await;
        assert_eq!(direct["selected"], "pkg@100.1.0");
        assert_eq!(direct["unselected"], "pkg@100.0.0");
    }
}

#[tokio::test]
async fn importer_scoped_update_route_owns_shared_parent_children_in_either_order() {
    for order in [["selected", "unselected"], ["unselected", "selected"]] {
        let (_selected_tmp, selected_manifest) =
            fake_manifest(serde_json::json!({ "parent": "^1.0.0" }));
        let (_unselected_tmp, unselected_manifest) =
            fake_manifest(serde_json::json!({ "parent": "^1.0.0" }));
        let manifests = HashMap::from_iter([
            ("selected", &selected_manifest),
            ("unselected", &unselected_manifest),
        ]);
        let importers = order
            .iter()
            .map(|id| WorkspaceImporter { id: (*id).to_string(), manifest: manifests[id] })
            .collect::<Vec<_>>();
        let resolver = RecordingResolver {
            table: HashMap::from_iter([
                (
                    ("parent".to_string(), "^1.0.0".to_string()),
                    fake_result(
                        "parent",
                        "1.0.0",
                        None,
                        serde_json::json!({
                            "name": "parent",
                            "version": "1.0.0",
                            "dependencies": { "pkg": "^100.0.0" },
                        }),
                    ),
                ),
                (
                    ("pkg".to_string(), "^100.0.0".to_string()),
                    fake_result(
                        "pkg",
                        "100.1.0",
                        None,
                        serde_json::json!({ "name": "pkg", "version": "100.1.0" }),
                    ),
                ),
            ]),
            seen: Mutex::new(HashMap::default()),
        };
        let mut opts = workspace_opts(false, false);
        opts.wanted_lockfile = Some(std::sync::Arc::new(importer_scoped_update_lockfile(
            &["selected", "unselected"],
            "parent",
            "^1.0.0",
            "1.0.0",
            Some(("pkg", "100.0.0")),
        )));
        opts.update_reuse_scopes_by_importer = BTreeMap::from([(
            "selected".to_string(),
            crate::UpdateReuseScope::Except(std::iter::once(("pkg".to_string(), None)).collect()),
        )]);
        let result =
            resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
                importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None)
            })
            .await
            .expect("resolve shared parent update");

        for importer_id in ["selected", "unselected"] {
            assert_eq!(
                result.peers.direct_dependencies_by_importer[importer_id]["parent"].as_str(),
                "parent@1.0.0",
            );
        }
        let parent_children =
            result.merged_tree.children_by_id.get("parent@1.0.0").expect("parent children");
        assert_eq!(parent_children.len(), 1);
        assert_eq!(&*parent_children[0].pkg_id, "pkg@100.1.0");
        // Recording the winner's children is not enough on its own: the
        // occurrence that ran first realized the ones it resolved, and
        // only the handover makes it re-read them.
        assert_eq!(graph_versions_of(&result, "pkg"), ["100.1.0"], "order {order:?}");
    }
}

/// The listing order of importers is not part of the input: processing
/// is id-ordered, so a reversed listing attributes the deprecation to
/// the same first occurrence (pnpm/pnpm#13846).
#[tokio::test]
async fn deprecation_attribution_does_not_depend_on_importer_listing_order() {
    let deprecation_prefix_for = |reversed: bool| async move {
        let (_transitive_tmp, transitive_manifest) =
            fake_manifest(serde_json::json!({ "wrapper": "1.0.0" }));
        let (_direct_tmp, direct_manifest) = fake_manifest(serde_json::json!({ "old": "1.0.0" }));
        let mut importers = vec![
            WorkspaceImporter { id: "a-transitive".to_string(), manifest: &transitive_manifest },
            WorkspaceImporter { id: "b-direct".to_string(), manifest: &direct_manifest },
        ];
        if reversed {
            importers.reverse();
        }
        let resolver = RecordingResolver {
            table: HashMap::from_iter([
                (
                    ("wrapper".to_string(), "1.0.0".to_string()),
                    fake_result(
                        "wrapper",
                        "1.0.0",
                        None,
                        serde_json::json!({
                            "name": "wrapper",
                            "version": "1.0.0",
                            "dependencies": { "old": "1.0.0" },
                        }),
                    ),
                ),
                (
                    ("old".to_string(), "1.0.0".to_string()),
                    fake_result(
                        "old",
                        "1.0.0",
                        None,
                        serde_json::json!({
                            "name": "old",
                            "version": "1.0.0",
                            "deprecated": "use new instead",
                        }),
                    ),
                ),
            ]),
            seen: Mutex::new(HashMap::default()),
        };
        let notifications = std::sync::Arc::new(Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&notifications);
        let mut opts = workspace_opts(false, false);
        opts.deprecation_log = Some(std::sync::Arc::new(move |deprecation: crate::Deprecation| {
            sink.lock().unwrap().push(deprecation);
        }));
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None)
        })
        .await
        .expect("resolve the workspace");
        let notifications = notifications.lock().unwrap();
        let [deprecation] = notifications.as_slice() else {
            panic!("expected one deprecation: {notifications:?}");
        };
        deprecation.prefix.clone()
    };

    let listed = deprecation_prefix_for(false).await;
    let reversed = deprecation_prefix_for(true).await;
    assert_eq!(listed, reversed, "attribution must not depend on the listing order");
}
