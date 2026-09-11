use super::{
    BTreeMap, DependencyGroup, HashMap, Mutex, OverlapRecordingResolver, RecordingResolver,
    WarmupProbeResolver, WorkspaceImporter, assert_eq, caret_entry, deps, fake_manifest,
    fake_result, graph_versions_of, importer_opts, resolve_single_importer, resolve_workspace,
    workspace_opts,
};

#[tokio::test]
async fn lowest_direct_applies_no_publish_cutoff() {
    let mut table = HashMap::default();
    table.insert(
        ("a".to_string(), "^1.0.0".to_string()),
        fake_result(
            "a",
            "1.0.0",
            Some("2024-05-20T08:00:00.000Z"),
            serde_json::json!({ "name": "a", "version": "1.0.0", "dependencies": { "sub": "^2.0.0" } }),
        ),
    );
    table.insert(
        ("sub".to_string(), "^2.0.0".to_string()),
        fake_result("sub", "2.0.0", None, serde_json::json!({ "name": "sub", "version": "2.0.0" })),
    );
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
    let (tmp, manifest) = fake_manifest(serde_json::json!({ "a": "^1.0.0" }));
    let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];

    resolve_workspace(
        &resolver,
        &importers,
        &[DependencyGroup::Prod],
        workspace_opts(true, false),
        |_| importer_opts(tmp.path().to_path_buf(), None),
    )
    .await
    .unwrap();

    assert_eq!(resolver.opts_for("a"), (true, None));
    assert_eq!(
        resolver.opts_for("sub"),
        (false, None),
        "no time-based cutoff in lowest-direct mode",
    );
}

/// The reverse of the sharing case above: when the first importer's
/// walk could NOT satisfy the optional peer either (it only hoisted it
/// later), the miss stays visible to every importer — each hoists its
/// own copy, so the shared subtree carries the peer suffix under both
/// (pnpm 11.6.0 behaviour for e.g. `clipanion`'s `typanion` under
/// importers that share `@yarnpkg/*` chains with the root).
#[tokio::test]
async fn shared_subtree_miss_unsatisfied_by_first_importer_still_hoists() {
    let mut table = HashMap::default();
    table.insert(
        ("top".to_string(), "1.0.0".to_string()),
        fake_result(
            "top",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "top",
                "version": "1.0.0",
                "dependencies": { "mid": "1.0.0", "carrier": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("mid".to_string(), "1.0.0".to_string()),
        fake_result(
            "mid",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "mid",
                "version": "1.0.0",
                "peerDependencies": { "opt": "*" },
                "peerDependenciesMeta": { "opt": { "optional": true } },
            }),
        ),
    );
    table.insert(
        ("carrier".to_string(), "1.0.0".to_string()),
        fake_result(
            "carrier",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "carrier",
                "version": "1.0.0",
                "dependencies": { "opt": "25.0.0" },
            }),
        ),
    );
    table.insert(
        ("opt".to_string(), "25.0.0".to_string()),
        fake_result(
            "opt",
            "25.0.0",
            None,
            serde_json::json!({ "name": "opt", "version": "25.0.0" }),
        ),
    );
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
    let (tmp_root, root_manifest) = fake_manifest(serde_json::json!({ "top": "1.0.0" }));
    let (tmp_a, a_manifest) = fake_manifest(serde_json::json!({ "top": "1.0.0" }));
    let importers = [
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
        WorkspaceImporter { id: "pkg-a".to_string(), manifest: &a_manifest },
    ];
    let dirs = [tmp_root.path(), tmp_a.path()];

    let mut opts = workspace_opts(false, false);
    opts.auto_install_peers = true;
    let mut next = 0;
    let result = resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |_| {
        let dir = dirs[next].to_path_buf();
        next += 1;
        let mut opts = importer_opts(dir, None);
        opts.auto_install_peers = true;
        opts
    })
    .await
    .unwrap();

    for importer in [".", "pkg-a"] {
        let direct = result.peers.direct_dependencies_by_importer.get(importer).expect("importer");
        assert_eq!(
            direct.get("top").map(std::string::ToString::to_string),
            Some("top@1.0.0(opt@25.0.0)".to_string()),
            "{importer} hoists the peer the first walk could not satisfy",
        );
    }
}

/// A newly-resolved package whose manifest carries `deprecated` is
/// forwarded to the deprecation sink once, and an
/// `allowedDeprecatedVersions` entry satisfied by the resolved version
/// suppresses the notification.
#[tokio::test]
async fn deprecated_manifests_notify_the_deprecation_sink_unless_allowed() {
    for (allowed_range, expect_notification) in
        [(None, true), (Some("^1.0.0"), false), (Some("^2.0.0"), true)]
    {
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "old": "^1.0.0" }));
        let importers = [WorkspaceImporter { id: "root".to_string(), manifest: &manifest }];
        let resolver = RecordingResolver {
            table: HashMap::from_iter([(
                ("old".to_string(), "^1.0.0".to_string()),
                fake_result(
                    "old",
                    "1.2.0",
                    None,
                    serde_json::json!({
                        "name": "old",
                        "version": "1.2.0",
                        "deprecated": "use new instead",
                    }),
                ),
            )]),
            seen: Mutex::new(HashMap::default()),
        };
        let notifications = std::sync::Arc::new(Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&notifications);
        let mut opts = workspace_opts(false, false);
        if let Some(range) = allowed_range {
            opts.allowed_deprecated_versions =
                BTreeMap::from([("old".to_string(), range.to_string())]);
        }
        opts.deprecation_log = Some(std::sync::Arc::new(move |deprecation: crate::Deprecation| {
            sink.lock().unwrap().push(deprecation);
        }));
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None)
        })
        .await
        .expect("resolve workspace with a deprecated dependency");

        let notifications = notifications.lock().unwrap();
        if !expect_notification {
            assert!(
                notifications.is_empty(),
                "range {allowed_range:?} must suppress the warning: {notifications:?}",
            );
            continue;
        }
        let [deprecation] = notifications.as_slice() else {
            panic!("expected one deprecation for range {allowed_range:?}: {notifications:?}");
        };
        assert_eq!(deprecation.pkg_name, "old");
        assert_eq!(deprecation.pkg_version, "1.2.0");
        assert_eq!(deprecation.deprecated, "use new instead");
        assert_eq!(deprecation.depth, 0);
        assert_eq!(
            deprecation.prefix,
            std::path::PathBuf::from("/repo").join("root").display().to_string(),
        );
    }
}

#[tokio::test]
async fn deprecated_package_is_reported_only_on_its_first_occurrence() {
    let (_transitive_tmp, transitive_manifest) =
        fake_manifest(serde_json::json!({ "wrapper": "1.0.0" }));
    let (_direct_tmp, direct_manifest) = fake_manifest(serde_json::json!({ "old": "1.0.0" }));
    // Ids chosen so the transitive importer is walked first under the
    // resolver's id-ordered importer processing.
    let importers = [
        WorkspaceImporter { id: "a-transitive".to_string(), manifest: &transitive_manifest },
        WorkspaceImporter { id: "b-direct".to_string(), manifest: &direct_manifest },
    ];
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
    .expect("resolve a deprecated package reached at two depths");

    let notifications = notifications.lock().unwrap();
    let [deprecation] = notifications.as_slice() else {
        panic!("expected one deprecation from the first occurrence: {notifications:?}");
    };
    assert_eq!(deprecation.depth, 1);
    assert_eq!(
        deprecation.prefix,
        std::path::PathBuf::from("/repo").join("a-transitive").display().to_string(),
    );
}

/// Importers' initial waves must not overlap. Interleaving them leaves
/// the resolved packages and their children identical but not the
/// occurrence nodes: importers race for a package's children-ownership
/// claim, and a transient holder still leaves its occurrences behind.
/// Occurrence identity feeds peer-variant computation, so a count that
/// depends on the interleaving is a lockfile that depends on it too.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn importer_waves_do_not_overlap() {
    let (_a_tmp, a_manifest) = fake_manifest(serde_json::json!({ "shared": "^1.0.0" }));
    let (_b_tmp, b_manifest) = fake_manifest(serde_json::json!({ "shared": "^1.0.0" }));
    let (_c_tmp, c_manifest) = fake_manifest(serde_json::json!({ "shared": "^1.0.0" }));
    let resolver = OverlapRecordingResolver::new();
    let importers = vec![
        WorkspaceImporter { id: "packages/a".to_string(), manifest: &a_manifest },
        WorkspaceImporter { id: "packages/b".to_string(), manifest: &b_manifest },
        WorkspaceImporter { id: "packages/c".to_string(), manifest: &c_manifest },
    ];

    resolve_workspace(
        &resolver,
        &importers,
        &[DependencyGroup::Prod],
        workspace_opts(false, false),
        |importer| importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None),
    )
    .await
    .expect("resolve workspace");

    let overlaps = resolver.overlaps.lock().unwrap();
    assert!(
        overlaps.is_empty(),
        "importers resolved concurrently: {:?}",
        overlaps.first().expect("checked non-empty"),
    );
}

#[tokio::test]
async fn warm_up_requests_descendants_beyond_one_level_before_the_level_barrier_lifts() {
    // `slow` is a direct dep that resolves only once the grandchild `c`
    // has been requested. The level barrier waits for `slow`, so only a
    // warm-up that recurses past `a`'s children can ever ask for `c`.
    let mut resolver = WarmupProbeResolver::new(HashMap::from_iter([
        caret_entry("slow", serde_json::json!({})),
        caret_entry("a", deps(&[("b", "^1.0.0")])),
        caret_entry("b", deps(&[("c", "^1.0.0")])),
        caret_entry("c", serde_json::json!({})),
    ]));
    resolver.gate = Some(("slow".to_string(), "c".to_string()));
    let result = resolve_single_importer(
        &resolver,
        serde_json::json!({ "slow": "^1.0.0", "a": "^1.0.0" }),
        workspace_opts(false, false),
        None,
    )
    .await
    .expect("the grandchild is warmed while the first level is still resolving");
    assert_eq!(graph_versions_of(&result, "c"), ["1.0.0"]);
}

#[tokio::test]
async fn warm_up_resolves_each_edge_once_across_a_diamond() {
    let resolver = WarmupProbeResolver::new(HashMap::from_iter([
        caret_entry("x", deps(&[("z", "^1.0.0")])),
        caret_entry("y", deps(&[("z", ">=1.0.0")])),
        caret_entry("z", deps(&[("w", "^1.0.0")])),
        (
            ("z".to_string(), ">=1.0.0".to_string()),
            fake_result("z", "1.0.0", None, deps(&[("w", "^1.0.0")])),
        ),
        caret_entry("w", serde_json::json!({})),
    ]));
    let result = resolve_single_importer(
        &resolver,
        serde_json::json!({ "x": "^1.0.0", "y": "^1.0.0" }),
        workspace_opts(false, false),
        None,
    )
    .await
    .expect("resolve");
    assert_eq!(graph_versions_of(&result, "z"), ["1.0.0"]);
    // Two ranges reach `z`, so it is asked for once per range; its own
    // child is asked for once however many branches reach `z`.
    assert_eq!(resolver.calls_for("z", "^1.0.0"), 1);
    assert_eq!(resolver.calls_for("z", ">=1.0.0"), 1);
    assert_eq!(resolver.calls_for("w", "^1.0.0"), 1);
}
