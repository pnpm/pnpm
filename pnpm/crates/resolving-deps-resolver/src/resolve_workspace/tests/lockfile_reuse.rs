use super::{
    Arc, DependencyGroup, FailureShape, HashMap, Mutex, RecordingResolver, Utc,
    WarmupProbeResolver, WorkspaceImporter, assert_eq, caret_entry, deps, fake_manifest,
    fake_result, graph_versions_of, importer_opts, importer_scoped_update_lockfile,
    lockfile_recording_time, lockfile_with_package, optional_failure_fixture, recorded_time,
    resolve_pinned_versus_fresh, resolve_single_importer, resolve_workspace, reuse_graph_lockfile,
    reuse_steal_lockfile, workspace_opts,
};
use chrono::TimeZone;

/// Against a registry whose abbreviated metadata omits publish times,
/// the dates the lockfile already recorded are what the cutoff is
/// derived from — otherwise a re-resolve would compute a different
/// cutoff and could pick different subdependency versions.
#[tokio::test]
async fn time_based_cutoff_falls_back_to_the_lockfiles_recorded_time() {
    let mut table = HashMap::default();
    table.insert(
        ("a".to_string(), "^1.0.0".to_string()),
        fake_result(
            "a",
            "1.0.0",
            None,
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

    let mut opts = workspace_opts(true, true);
    opts.wanted_lockfile =
        Some(Arc::new(lockfile_recording_time(&[("a@1.0.0", "2024-05-20T08:00:00.000Z")])));

    let result = resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |_| {
        importer_opts(tmp.path().to_path_buf(), None)
    })
    .await
    .unwrap();

    assert_eq!(
        resolver.opts_for("sub"),
        (false, Some(Utc.with_ymd_and_hms(2024, 5, 20, 9, 0, 0).unwrap())),
        "the recorded publish date stands in for the missing one",
    );
    assert_eq!(
        result.time,
        recorded_time(&[("a@1.0.0", "2024-05-20T08:00:00.000Z")]),
        "a recorded date is carried forward, not dropped",
    );
}

#[tokio::test]
async fn skips_an_optional_dependency_whose_resolution_fails_with_no_locked_entry() {
    let (_tmp, manifest, resolver) = optional_failure_fixture(FailureShape::Plain);
    let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];
    let skipped = std::sync::Arc::new(Mutex::new(Vec::new()));
    let mut opts = workspace_opts(false, false);
    opts.skipped_optional_log = Some(std::sync::Arc::new({
        let skipped = std::sync::Arc::clone(&skipped);
        move |notification| skipped.lock().unwrap().push(notification)
    }));
    let result = resolve_workspace(
        &resolver,
        &importers,
        &[DependencyGroup::Prod, DependencyGroup::Optional],
        opts,
        |_| importer_opts(std::path::PathBuf::from("/repo"), None),
    )
    .await
    .expect("resolution failure of an optional dependency is skipped");

    let direct = &result.peers.direct_dependencies_by_importer["."];
    assert!(direct.contains_key("kept"), "the regular dep resolves: {direct:?}");
    assert!(!direct.contains_key("broken"), "the failing optional edge is dropped: {direct:?}");
    let skipped = skipped.lock().unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].name.as_deref(), Some("broken"));
    assert_eq!(skipped[0].version.as_deref(), Some("^1.0.0"));
    assert_eq!(skipped[0].bare_specifier, "^1.0.0");
    assert!(
        skipped[0].parents.is_empty(),
        "a direct optional dep has an empty parents chain: {:?}",
        skipped[0].parents,
    );
    assert_eq!(skipped[0].prefix, "/repo");
    assert!(
        skipped[0].details.contains("No matching version found for broken@^1.0.0"),
        "details carry the resolver error: {}",
        skipped[0].details,
    );
}

// Covers <https://github.com/pnpm/pnpm/issues/12853>: a wanted lockfile
// that already resolved the optional dependency must fail the install
// loudly instead of silently dropping the locked entries.
#[tokio::test]
async fn fails_on_an_optional_dependency_that_cannot_be_resolved_with_a_satisfying_locked_entry() {
    let (_tmp, manifest, resolver) = optional_failure_fixture(FailureShape::Plain);
    let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];
    let mut opts = workspace_opts(false, false);
    opts.wanted_lockfile = Some(std::sync::Arc::new(lockfile_with_package("broken@1.2.0")));
    // Model `pacquet dedupe`: the prior lockfile rides along for the
    // locked-entry check but nothing is reused from it.
    opts.update_reuse_scope = crate::UpdateReuseScope::None;
    let result = resolve_workspace(
        &resolver,
        &importers,
        &[DependencyGroup::Prod, DependencyGroup::Optional],
        opts,
        |_| importer_opts(std::path::PathBuf::from("/repo"), None),
    )
    .await;

    let Err(crate::ResolveImporterError::Resolve(err)) = result else {
        panic!("a locked optional dependency must fail loudly");
    };
    let help = miette::Diagnostic::help(&err).expect("carries the lockfile hint").to_string();
    assert!(help.contains("the lockfile contains a resolution for it"), "unexpected hint: {help}");
    assert!(
        matches!(err, crate::ResolveDependencyTreeError::LockedOptionalResolutionFailure(_)),
        "unexpected error: {err}",
    );
}

#[tokio::test]
async fn skips_an_optional_dependency_when_the_locked_entry_does_not_satisfy_the_wanted_range() {
    let (_tmp, manifest, resolver) = optional_failure_fixture(FailureShape::Plain);
    let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];
    let mut opts = workspace_opts(false, false);
    opts.wanted_lockfile = Some(std::sync::Arc::new(lockfile_with_package("broken@0.9.0")));
    opts.update_reuse_scope = crate::UpdateReuseScope::None;
    let result = resolve_workspace(
        &resolver,
        &importers,
        &[DependencyGroup::Prod, DependencyGroup::Optional],
        opts,
        |_| importer_opts(std::path::PathBuf::from("/repo"), None),
    )
    .await
    .expect("an out-of-range locked entry keeps the skip behavior");

    let direct = &result.peers.direct_dependencies_by_importer["."];
    assert!(!direct.contains_key("broken"), "the failing optional edge is dropped: {direct:?}");
}

/// The loud-failure path for a locked optional dependency has to cover
/// the coded failures too, for the same reason as the skip arm.
#[tokio::test]
async fn fails_loudly_on_a_locked_optional_dependency_for_every_coded_resolver_failure() {
    for failure in [FailureShape::NoMatchingVersion, FailureShape::RegistryResponse] {
        let (_tmp, manifest, resolver) = optional_failure_fixture(failure);
        let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];
        let mut opts = workspace_opts(false, false);
        opts.wanted_lockfile = Some(std::sync::Arc::new(lockfile_with_package("broken@1.2.0")));
        opts.update_reuse_scope = crate::UpdateReuseScope::None;
        let result = resolve_workspace(
            &resolver,
            &importers,
            &[DependencyGroup::Prod, DependencyGroup::Optional],
            opts,
            |_| importer_opts(std::path::PathBuf::from("/repo"), None),
        )
        .await;

        let Err(crate::ResolveImporterError::Resolve(err)) = result else {
            panic!("a locked optional dependency must fail loudly");
        };
        assert!(
            matches!(err, crate::ResolveDependencyTreeError::LockedOptionalResolutionFailure(_)),
            "unexpected error: {err}",
        );
    }
}

/// A dependency reused from the wanted lockfile still notifies the
/// deprecation sink: the synthesized manifest round-trips the
/// lockfile's `deprecated` metadata precisely so warm installs keep
/// warning, matching pnpm's repeat-install behavior.
#[tokio::test]
async fn reused_lockfile_entries_still_notify_the_deprecation_sink() {
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "old": "^1.0.0" }));
    let importers = [WorkspaceImporter { id: "root".to_string(), manifest: &manifest }];
    let resolver =
        RecordingResolver { table: HashMap::default(), seen: Mutex::new(HashMap::default()) };
    let mut lockfile = importer_scoped_update_lockfile(&["root"], "old", "^1.0.0", "1.2.0", None);
    lockfile
        .packages
        .as_mut()
        .expect("lockfile carries packages")
        .get_mut(&"old@1.2.0".parse::<pnpm_lockfile::PkgNameVerPeer>().expect("parse key"))
        .expect("direct entry")
        .deprecated = Some("use new instead".to_string());
    let notifications = std::sync::Arc::new(Mutex::new(Vec::new()));
    let sink = std::sync::Arc::clone(&notifications);
    let mut opts = workspace_opts(false, false);
    opts.wanted_lockfile = Some(std::sync::Arc::new(lockfile));
    opts.deprecation_log = Some(std::sync::Arc::new(move |deprecation: crate::Deprecation| {
        sink.lock().unwrap().push(deprecation);
    }));
    resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
        importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None)
    })
    .await
    .expect("resolve workspace reusing the lockfile");

    let notifications = notifications.lock().unwrap();
    let [deprecation] = notifications.as_slice() else {
        panic!("expected one deprecation from the reused entry: {notifications:?}");
    };
    assert_eq!(deprecation.pkg_name, "old");
    assert_eq!(deprecation.pkg_version, "1.2.0");
    assert_eq!(deprecation.deprecated, "use new instead");
    assert_eq!(deprecation.depth, 0);
}

/// A parent whose subtree contains a dependency cycle re-resolves
/// fresh (the conservative cycle guard), but when it lands back on its
/// recorded version its child edges keep their prior refs and re-enter
/// the reuse gate — so `stable`'s cycle-free subtree is reused and its
/// open `open: *` edge keeps the recorded 1.0.0 pin instead of
/// re-picking the registry's newest 2.0.0.
#[tokio::test]
async fn fresh_resolved_parent_on_recorded_version_reuses_child_subtrees() {
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "app": "^1.0.0" }));
    let importers = [WorkspaceImporter { id: "proj".to_string(), manifest: &manifest }];
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
            (
                ("app".to_string(), "^1.0.0".to_string()),
                fake_result(
                    "app",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "app",
                        "version": "1.0.0",
                        "dependencies": { "cyclic": "^1.0.0", "stable": "^1.0.0" },
                    }),
                ),
            ),
            // The cycle-membered subtree is denied reuse, so its edges
            // resolve freshly with their recorded versions pinned as
            // exact specs — the table serves the pinned form.
            (
                ("cyclic".to_string(), "1.0.0".to_string()),
                fake_result(
                    "cyclic",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "cyclic",
                        "version": "1.0.0",
                        "dependencies": { "loop": "^1.0.0" },
                    }),
                ),
            ),
            (
                ("loop".to_string(), "1.0.0".to_string()),
                fake_result(
                    "loop",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "loop",
                        "version": "1.0.0",
                        "dependencies": { "cyclic": "^1.0.0" },
                    }),
                ),
            ),
            (
                ("stable".to_string(), "^1.0.0".to_string()),
                fake_result(
                    "stable",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "stable",
                        "version": "1.0.0",
                        "dependencies": { "open": "*" },
                    }),
                ),
            ),
            (
                ("open".to_string(), "*".to_string()),
                fake_result(
                    "open",
                    "2.0.0",
                    None,
                    serde_json::json!({ "name": "open", "version": "2.0.0" }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let mut opts = workspace_opts(false, false);
    opts.wanted_lockfile = Some(std::sync::Arc::new(reuse_graph_lockfile(
        "proj",
        &[("app", "^1.0.0", "1.0.0")],
        &[
            ("app@1.0.0", &[("cyclic", "1.0.0"), ("stable", "1.0.0")]),
            ("cyclic@1.0.0", &[("loop", "1.0.0")]),
            ("loop@1.0.0", &[("cyclic", "1.0.0")]),
            ("stable@1.0.0", &[("open", "1.0.0")]),
            ("open@1.0.0", &[]),
        ],
        &[],
    )));
    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None)
        })
        .await
        .expect("resolve workspace with a cycle next to a reusable subtree");

    for name in ["app", "cyclic", "loop", "stable", "open"] {
        assert_eq!(graph_versions_of(&result, name), ["1.0.0"], "{name} must keep 1.0.0");
    }
}

/// A children-ownership handover whose peer-shadow context is
/// unchanged must not discard other occurrences' realized subtrees.
///
/// `pkg-b`'s lockfile pins `wrapperB → mid2 → leaf2@1.0.0`, so its walk
/// reuses that subtree. The root importer's required-peer hoist of
/// `mid2` (for `needyC`) then claims `mid2`'s children ownership at
/// depth 0 with the same (empty) peer-shadow set. Rewriting the
/// displaced owner's occurrences to lazy on such a handover would
/// re-resolve the reused subtree's open ranges — churning the locked
/// `leaf2@1.0.0` to the registry's newer `1.5.0` even though nothing
/// about `mid2`'s child resolution context changed.
#[tokio::test]
async fn unchanged_shadow_ownership_handover_keeps_reused_subtree() {
    let mut table = HashMap::default();
    table.insert(
        ("wrapperB".to_string(), "1.0.0".to_string()),
        fake_result(
            "wrapperB",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "wrapperB",
                "version": "1.0.0",
                "dependencies": { "mid2": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("shared2".to_string(), "1.0.0".to_string()),
        fake_result(
            "shared2",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "shared2",
                "version": "1.0.0",
                "dependencies": { "mid2": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("needyC".to_string(), "1.0.0".to_string()),
        fake_result(
            "needyC",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "needyC",
                "version": "1.0.0",
                "peerDependencies": { "mid2": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("mid2".to_string(), "1.0.0".to_string()),
        fake_result(
            "mid2",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "mid2",
                "version": "1.0.0",
                "dependencies": { "leaf2": "^1.0.0" },
            }),
        ),
    );
    table.insert(
        ("leaf2".to_string(), "^1.0.0".to_string()),
        fake_result(
            "leaf2",
            "1.5.0",
            None,
            serde_json::json!({ "name": "leaf2", "version": "1.5.0" }),
        ),
    );
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
    let (tmp_b, b_manifest) = fake_manifest(serde_json::json!({ "wrapperB": "1.0.0" }));
    // The root carries only the peer consumer: with importers walked in
    // id order the root's wave runs first, so the shared subtree must
    // be claimed by pkg-b's wave and reach the root only through the
    // later peer-hoist round for a handover to occur at all.
    let (tmp_root, root_manifest) = fake_manifest(serde_json::json!({ "needyC": "1.0.0" }));
    let importers = [
        WorkspaceImporter { id: "pkg-b".to_string(), manifest: &b_manifest },
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
    ];
    let dirs = [tmp_b.path(), tmp_root.path()];

    let mut opts = workspace_opts(false, false);
    opts.auto_install_peers = true;
    opts.wanted_lockfile = Some(std::sync::Arc::new(reuse_steal_lockfile()));
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

    let root_direct = result.peers.direct_dependencies_by_importer.get(".").expect("root importer");
    assert_eq!(
        root_direct.get("mid2").map(std::string::ToString::to_string),
        Some("mid2@1.0.0".to_string()),
        "needyC's required peer mid2 is hoisted to the root importer",
    );
    let mid2 = result
        .peers
        .graph
        .get(&pnpm_deps_path::DepPath::from("mid2@1.0.0".to_string()))
        .expect("mid2 in graph");
    assert_eq!(
        mid2.children.get("leaf2").map(std::string::ToString::to_string),
        Some("leaf2@1.0.0".to_string()),
        "the lockfile-reused subtree must survive the ownership handover",
    );
}

/// Publishing a fresh answer over pinned children re-resolves the open
/// ranges reuse exists to hold still, and leaves the occurrences that
/// realized the pinned subtree reading children the record no longer
/// holds (<https://github.com/pnpm/pnpm/issues/13837>).
#[tokio::test]
async fn a_pinned_subtree_keeps_its_children_against_a_fresh_walk() {
    for slow in [("fresh", "1.0.0"), ("reused", "1.0.0")] {
        let tree = resolve_pinned_versus_fresh(slow).await;
        let recorded: Vec<&str> = tree
            .children_by_id
            .get("shared@1.0.0")
            .expect("shared children")
            .iter()
            .map(|edge| &*edge.pkg_id)
            .collect();
        assert_eq!(recorded, ["pin@1.0.0"], "the pins stand, held back: {slow:?}");
        assert!(
            !tree.packages.contains_key("pin@1.5.0"),
            "and nothing re-resolves the range they pinned, held back: {slow:?}",
        );
    }
}

#[tokio::test]
async fn warm_up_of_a_speculative_only_edge_leaves_patch_bookkeeping_alone() {
    // Without autoInstallPeers a dependency shadowed by a peer is dropped
    // only when the parent scope supplies the peer; the warm-up does not
    // know that scope and asks for `q@^2.0.0` speculatively. The real
    // walk never accepts that edge, so its patch must not count as
    // applied.
    let resolver = WarmupProbeResolver::new(HashMap::from_iter([
        caret_entry("a", deps(&[("p", "^1.0.0")])),
        caret_entry(
            "p",
            serde_json::json!({
                "dependencies": { "q": "^2.0.0" },
                "peerDependencies": { "q": "^1.0.0" },
            }),
        ),
        caret_entry("q", serde_json::json!({})),
        (
            ("q".to_string(), "^2.0.0".to_string()),
            fake_result("q", "2.0.0", None, serde_json::json!({})),
        ),
    ]));
    let mut groups = pnpm_patching::PatchGroupRecord::new();
    let mut group = pnpm_patching::PatchGroup::default();
    group.exact.insert(
        "2.0.0".to_string(),
        pnpm_patching::ExtendedPatchInfo {
            hash: "abc123".to_string(),
            patch_file_path: None,
            key: "q@2.0.0".to_string(),
        },
    );
    groups.insert("q".to_string(), group);
    let result = resolve_single_importer(
        &resolver,
        serde_json::json!({ "a": "^1.0.0", "q": "^1.0.0" }),
        workspace_opts(false, false),
        Some(Arc::new(groups)),
    )
    .await
    .expect("resolve");
    assert_eq!(resolver.calls_for("q", "^2.0.0"), 1, "the edge was warmed speculatively");
    assert_eq!(graph_versions_of(&result, "q"), ["1.0.0"], "and never entered the graph");
    assert!(
        !result.merged_tree.applied_patches.contains("q@2.0.0"),
        "a patch the real walk never applied must not count as applied",
    );
}
