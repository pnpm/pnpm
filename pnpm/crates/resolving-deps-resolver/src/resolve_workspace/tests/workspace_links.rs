use super::{
    Arc, BTreeMap, DependencyGroup, DirectoryResolution, HashMap, LinkWorkspacePackages,
    LockfileResolution, Mutex, PkgResolutionId, ProjectRelativeWorkspaceResolver,
    RecordedReadPackageCalls, RecordingHooks, RecordingResolver, ResolveResult, WorkspaceImporter,
    assert_eq, fake_manifest, fake_result, graph_versions_of, importer_opts, resolve_workspace,
    reuse_graph_lockfile, workspace_opts,
};

#[tokio::test]
async fn workspace_resolution_is_shared_and_rendered_per_importer() {
    let (_a_tmp, a_manifest) = fake_manifest(serde_json::json!({ "shared": "workspace:^" }));
    let (_b_tmp, b_manifest) = fake_manifest(serde_json::json!({ "shared": "workspace:^" }));
    let (_c_tmp, c_manifest) = fake_manifest(serde_json::json!({ "shared": "workspace:^" }));
    let resolver =
        ProjectRelativeWorkspaceResolver::new(std::path::PathBuf::from("/repo/packages/shared"));
    let importers = vec![
        WorkspaceImporter { id: "packages/a".to_string(), manifest: &a_manifest },
        WorkspaceImporter { id: "apps/b".to_string(), manifest: &b_manifest },
        WorkspaceImporter { id: "packages/c".to_string(), manifest: &c_manifest },
    ];
    let lockfile_dir = std::path::PathBuf::from("/repo");
    let workspace_packages = std::sync::Arc::new(std::collections::BTreeMap::default());
    let hook_calls: RecordedReadPackageCalls = Arc::new(Mutex::new(Vec::new()));
    let mut opts = workspace_opts(false, false);
    opts.lockfile_dir.clone_from(&lockfile_dir);
    opts.pnpmfile_hook = Some(Arc::new(RecordingHooks { calls: Arc::clone(&hook_calls) }));

    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let project_dir = match importer.id.as_str() {
                "packages/a" => std::path::PathBuf::from("/repo/packages/a"),
                "apps/b" => std::path::PathBuf::from("/repo/apps/b"),
                "packages/c" => std::path::PathBuf::from("/repo/packages/c"),
                _ => unreachable!("unexpected importer"),
            };
            let mut opts = importer_opts(project_dir, None);
            opts.lockfile_dir = Some(lockfile_dir.clone());
            opts.base_opts.lockfile_dir.clone_from(&lockfile_dir);
            opts.base_opts.link_workspace_packages = LinkWorkspacePackages::Deep;
            opts.base_opts.workspace_packages = Some(std::sync::Arc::clone(&workspace_packages));
            opts
        })
        .await
        .expect("resolve workspace");

    assert_eq!(
        result.peers.direct_dependencies_by_importer["packages/a"]["shared"].as_str(),
        "link:../shared",
    );
    assert_eq!(
        result.peers.direct_dependencies_by_importer["apps/b"]["shared"].as_str(),
        "link:../../packages/shared",
    );
    assert_eq!(
        result.peers.direct_dependencies_by_importer["packages/c"]["shared"].as_str(),
        "link:../shared",
    );
    assert_eq!(resolver.workspace_resolution_count(), 1);
    let mut shared_hook_dirs = hook_calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _)| name == "shared")
        .map(|(_, dir)| dir.clone())
        .collect::<Vec<_>>();
    shared_hook_dirs.sort();
    assert_eq!(
        shared_hook_dirs,
        [Some("../../packages/shared".to_string()), Some("../shared".to_string())],
    );
}

#[tokio::test]
async fn semver_workspace_matches_stay_scoped_to_each_importer() {
    // `link_workspace_packages` lets a plain semver range land on a
    // workspace package too, but only a named `workspace:` selector is
    // guaranteed to depend on the importer solely through the rendered link.
    // A range keeps its per-importer resolution.
    let (_a_tmp, a_manifest) = fake_manifest(serde_json::json!({ "shared": "^1.0.0" }));
    let (_b_tmp, b_manifest) = fake_manifest(serde_json::json!({ "shared": "^1.0.0" }));
    let resolver = ProjectRelativeWorkspaceResolver::claiming(
        "^1.0.0",
        std::path::PathBuf::from("/repo/packages/shared"),
    );
    let importers = vec![
        WorkspaceImporter { id: "packages/a".to_string(), manifest: &a_manifest },
        WorkspaceImporter { id: "apps/b".to_string(), manifest: &b_manifest },
    ];
    let lockfile_dir = std::path::PathBuf::from("/repo");
    let workspace_packages = std::sync::Arc::new(std::collections::BTreeMap::default());
    let mut opts = workspace_opts(false, false);
    opts.lockfile_dir.clone_from(&lockfile_dir);

    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let project_dir = match importer.id.as_str() {
                "packages/a" => std::path::PathBuf::from("/repo/packages/a"),
                "apps/b" => std::path::PathBuf::from("/repo/apps/b"),
                _ => unreachable!("unexpected importer"),
            };
            let mut opts = importer_opts(project_dir, None);
            opts.lockfile_dir = Some(lockfile_dir.clone());
            opts.base_opts.lockfile_dir.clone_from(&lockfile_dir);
            opts.base_opts.link_workspace_packages = LinkWorkspacePackages::Deep;
            opts.base_opts.workspace_packages = Some(std::sync::Arc::clone(&workspace_packages));
            opts
        })
        .await
        .expect("resolve workspace");

    assert_eq!(
        result.peers.direct_dependencies_by_importer["packages/a"]["shared"].as_str(),
        "link:../shared",
    );
    assert_eq!(
        result.peers.direct_dependencies_by_importer["apps/b"]["shared"].as_str(),
        "link:../../packages/shared",
    );
    assert_eq!(resolver.workspace_resolution_count(), 2);
}

#[tokio::test]
async fn canonical_snapshot_link_keeps_direct_links_relative_to_each_importer() {
    let (_nested_tmp, nested_manifest) = fake_manifest(serde_json::json!({
        "shared": "workspace:^",
        "wrapper": "1.0.0",
    }));
    let (_shallow_tmp, shallow_manifest) = fake_manifest(serde_json::json!({
        "shared": "workspace:^",
        "wrapper": "1.0.0",
    }));
    let lockfile_dir = std::path::PathBuf::from("/repo");
    let workspace_packages = std::sync::Arc::new(std::collections::BTreeMap::default());
    let resolver = ProjectRelativeWorkspaceResolver::new(lockfile_dir.join("packages/shared"));
    let importers = vec![
        WorkspaceImporter { id: "apps/nested/app".to_string(), manifest: &nested_manifest },
        WorkspaceImporter { id: "packages/consumer".to_string(), manifest: &shallow_manifest },
    ];
    let mut opts = workspace_opts(false, false);
    opts.lockfile_dir.clone_from(&lockfile_dir);

    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let project_dir = match importer.id.as_str() {
                "apps/nested/app" => std::path::PathBuf::from("/repo/apps/nested/app"),
                "packages/consumer" => std::path::PathBuf::from("/repo/packages/consumer"),
                _ => unreachable!("unexpected importer"),
            };
            let mut opts = importer_opts(project_dir, None);
            opts.lockfile_dir = Some(lockfile_dir.clone());
            opts.base_opts.lockfile_dir.clone_from(&lockfile_dir);
            opts.base_opts.link_workspace_packages = LinkWorkspacePackages::Deep;
            opts.base_opts.workspace_packages = Some(std::sync::Arc::clone(&workspace_packages));
            opts
        })
        .await
        .expect("resolve workspace");

    assert_eq!(
        result.peers.direct_dependencies_by_importer["apps/nested/app"]["shared"].as_str(),
        "link:../../../packages/shared",
    );
    assert_eq!(
        result.peers.direct_dependencies_by_importer["packages/consumer"]["shared"].as_str(),
        "link:../shared",
    );
    let wrapper =
        result.peers.graph.get(&crate::DepPath::from("wrapper@1.0.0")).expect("wrapper graph node");
    assert_eq!(wrapper.children.get("shared"), Some(&crate::DepPath::from("link:packages/shared")));
    assert!(result.merged_tree.packages.contains_key("link:packages/shared"));
    assert!(!result.merged_tree.packages.contains_key("link:../../../packages/shared"));
    assert_eq!(resolver.workspace_resolution_count(), 1);
}

#[tokio::test]
async fn catalogs_work_in_injected_workspace_packages() {
    let (_project1_tmp, project1) = fake_manifest(serde_json::json!({ "project2": "workspace:*" }));
    let (_project2_tmp, project2) = fake_manifest(serde_json::json!({ "is-positive": "catalog:" }));
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
            (
                ("project2".to_string(), "workspace:*".to_string()),
                ResolveResult {
                    id: PkgResolutionId::from("file:packages/project2".to_string()),
                    name_ver: None,
                    latest: None,
                    published_at: None,
                    manifest: Some(std::sync::Arc::new(serde_json::json!({
                        "name": "project2",
                        "version": "0.0.0",
                        "dependencies": { "is-positive": "catalog:" },
                    }))),
                    resolution: LockfileResolution::Directory(DirectoryResolution {
                        directory: "packages/project2".to_string(),
                    }),
                    resolved_via: "workspace".to_string(),
                    normalized_bare_specifier: None,
                    alias: Some("project2".to_string()),
                    policy_violation: None,
                },
            ),
            (
                ("is-positive".to_string(), "1.0.0".to_string()),
                fake_result(
                    "is-positive",
                    "1.0.0",
                    None,
                    serde_json::json!({ "name": "is-positive", "version": "1.0.0" }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let importers = [
        WorkspaceImporter { id: "packages/project1".to_string(), manifest: &project1 },
        WorkspaceImporter { id: "packages/project2".to_string(), manifest: &project2 },
    ];
    let catalogs = BTreeMap::from([(
        "default".to_string(),
        BTreeMap::from([("is-positive".to_string(), "1.0.0".to_string())]),
    )]);

    let result = resolve_workspace(
        &resolver,
        &importers,
        &[DependencyGroup::Prod],
        workspace_opts(false, false),
        |importer| {
            let mut opts =
                importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None);
            opts.catalogs = catalogs.clone();
            opts
        },
    )
    .await
    .expect("resolve catalog dependency of injected workspace package");

    assert!(result.merged_tree.packages.contains_key("project2@file:packages/project2"));
    assert!(result.merged_tree.packages.contains_key("is-positive@1.0.0"));
    let children = result
        .merged_tree
        .children_by_id
        .get("project2@file:packages/project2")
        .expect("injected workspace package children");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].alias, "is-positive");
    assert_eq!(&*children[0].pkg_id, "is-positive@1.0.0");
    assert!(!children[0].optional);
}

/// A `catalog:` direct dep whose catalog entry is unchanged must not be
/// treated as a changed direct dep: the wanted spec reaches the walker
/// with the protocol already resolved to the catalog's range, and
/// comparing that against the recorded `catalog:` specifier would
/// decline subtree reuse for every dependent — `parent` would
/// re-resolve to the registry's newer 1.1.0 (and re-pick its open
/// `tool: *` edge at 2.0.0) with no manifest change, churning the
/// lockfile.
#[tokio::test]
async fn unchanged_catalog_dep_keeps_dependent_subtree_pins() {
    let (_tmp, manifest) =
        fake_manifest(serde_json::json!({ "tool": "catalog:", "parent": "^1.0.0" }));
    let importers = [WorkspaceImporter { id: "proj".to_string(), manifest: &manifest }];
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
            (
                ("tool".to_string(), "^1.0.0".to_string()),
                fake_result(
                    "tool",
                    "1.0.0",
                    None,
                    serde_json::json!({ "name": "tool", "version": "1.0.0" }),
                ),
            ),
            (
                ("tool".to_string(), "*".to_string()),
                fake_result(
                    "tool",
                    "2.0.0",
                    None,
                    serde_json::json!({ "name": "tool", "version": "2.0.0" }),
                ),
            ),
            (
                ("parent".to_string(), "^1.0.0".to_string()),
                fake_result(
                    "parent",
                    "1.1.0",
                    None,
                    serde_json::json!({
                        "name": "parent",
                        "version": "1.1.0",
                        "dependencies": { "tool": "*" },
                    }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let mut opts = workspace_opts(false, false);
    opts.wanted_lockfile = Some(std::sync::Arc::new(reuse_graph_lockfile(
        "proj",
        &[("tool", "catalog:", "1.0.0"), ("parent", "^1.0.0", "1.0.0")],
        &[("tool@1.0.0", &[]), ("parent@1.0.0", &[("tool", "1.0.0")])],
        &[("default", "tool", "^1.0.0", "1.0.0")],
    )));
    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let mut opts =
                importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None);
            opts.catalogs = BTreeMap::from([(
                "default".to_string(),
                BTreeMap::from([("tool".to_string(), "^1.0.0".to_string())]),
            )]);
            opts
        })
        .await
        .expect("resolve workspace with an unchanged catalog dep");

    assert_eq!(graph_versions_of(&result, "parent"), ["1.0.0"]);
    assert_eq!(graph_versions_of(&result, "tool"), ["1.0.0"]);
}

/// A catalog range bump is a real direct-dep change: dependents that
/// pin the old version must re-resolve so their pins land on the new
/// catalog pick.
#[tokio::test]
async fn catalog_range_bump_refreshes_dependent_pins() {
    let (_tmp, manifest) =
        fake_manifest(serde_json::json!({ "tool": "catalog:", "parent": "^1.0.0" }));
    let importers = [WorkspaceImporter { id: "proj".to_string(), manifest: &manifest }];
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
            (
                ("tool".to_string(), "^2.0.0".to_string()),
                fake_result(
                    "tool",
                    "2.0.0",
                    None,
                    serde_json::json!({ "name": "tool", "version": "2.0.0" }),
                ),
            ),
            (
                ("tool".to_string(), "*".to_string()),
                fake_result(
                    "tool",
                    "2.0.0",
                    None,
                    serde_json::json!({ "name": "tool", "version": "2.0.0" }),
                ),
            ),
            (
                ("tool".to_string(), "2.0.0".to_string()),
                fake_result(
                    "tool",
                    "2.0.0",
                    None,
                    serde_json::json!({ "name": "tool", "version": "2.0.0" }),
                ),
            ),
            (
                ("parent".to_string(), "^1.0.0".to_string()),
                fake_result(
                    "parent",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "parent",
                        "version": "1.0.0",
                        "dependencies": { "tool": "*" },
                    }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let mut opts = workspace_opts(false, false);
    opts.wanted_lockfile = Some(std::sync::Arc::new(reuse_graph_lockfile(
        "proj",
        &[("tool", "catalog:", "1.0.0"), ("parent", "^1.0.0", "1.0.0")],
        &[("tool@1.0.0", &[]), ("parent@1.0.0", &[("tool", "1.0.0")])],
        &[("default", "tool", "^1.0.0", "1.0.0")],
    )));
    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let mut opts =
                importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None);
            opts.catalogs = BTreeMap::from([(
                "default".to_string(),
                BTreeMap::from([("tool".to_string(), "^2.0.0".to_string())]),
            )]);
            opts
        })
        .await
        .expect("resolve workspace with a bumped catalog range");

    assert_eq!(graph_versions_of(&result, "tool"), ["2.0.0"]);
}
