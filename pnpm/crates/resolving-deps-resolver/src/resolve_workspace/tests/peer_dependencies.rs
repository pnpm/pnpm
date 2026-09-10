use super::{
    DependencyGroup, DirectoryResolution, HashMap, LockfileResolution, ManifestAvailability, Mutex,
    PkgResolutionId, RecordingResolver, WarmupProbeResolver, WorkspaceImporter,
    announced_finalized_packages, assert_eq, caret_entry, deps, fake_manifest, fake_result,
    graph_versions_of, importer_opts, link_root_dep_peer_provider,
    project_relative_root_dep_is_not_a_provider, resolve_single_importer,
    resolve_with_transient_shared_walk, resolve_workspace, table_entry, workspace_opts,
};

#[tokio::test]
async fn workspace_root_direct_deps_resolve_child_importer_peers() {
    let (_root_tmp, root_manifest) = fake_manifest(serde_json::json!({
        "typescript": "~5.9.3",
    }));
    let (_app_tmp, app_manifest) = fake_manifest(serde_json::json!({
        "rollup": "^4.0.0",
        "plugin": "^1.0.0",
    }));
    let mut table = HashMap::default();
    table.insert(
        ("typescript".to_string(), "~5.9.3".to_string()),
        fake_result(
            "typescript",
            "5.9.3",
            None,
            serde_json::json!({ "name": "typescript", "version": "5.9.3" }),
        ),
    );
    table.insert(
        ("typescript".to_string(), "5.9.3".to_string()),
        fake_result(
            "typescript",
            "5.9.3",
            None,
            serde_json::json!({ "name": "typescript", "version": "5.9.3" }),
        ),
    );
    table.insert(
        ("rollup".to_string(), "^4.0.0".to_string()),
        fake_result(
            "rollup",
            "4.0.0",
            None,
            serde_json::json!({ "name": "rollup", "version": "4.0.0" }),
        ),
    );
    table.insert(
        ("plugin".to_string(), "^1.0.0".to_string()),
        fake_result(
            "plugin",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "plugin",
                "version": "1.0.0",
                "peerDependencies": {
                    "rollup": "^4.0.0",
                    "typescript": "^5.0.0"
                }
            }),
        ),
    );
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
    let importers = vec![
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
        WorkspaceImporter { id: "packages/app".to_string(), manifest: &app_manifest },
    ];
    let mut opts = workspace_opts(false, false);
    opts.resolve_peers_from_workspace_root = true;

    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let project_dir = match importer.id.as_str() {
                "." => std::path::PathBuf::from("/repo"),
                "packages/app" => std::path::PathBuf::from("/repo/packages/app"),
                _ => unreachable!("unexpected importer"),
            };
            importer_opts(project_dir, None)
        })
        .await
        .expect("resolve workspace");

    assert_eq!(
        result.peers.direct_dependencies_by_importer["packages/app"]["plugin"].as_str(),
        "plugin@1.0.0(rollup@4.0.0)(typescript@5.9.3)",
    );
}

#[tokio::test]
async fn local_workspace_package_version_can_satisfy_another_importers_optional_peer() {
    let mut table = HashMap::default();
    table.insert(
        ("needs-opt".to_string(), "1.0.0".to_string()),
        fake_result(
            "needs-opt",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "needs-opt",
                "version": "1.0.0",
                "peerDependencies": { "opt": "^1.0.0" },
                "peerDependenciesMeta": { "opt": { "optional": true } },
            }),
        ),
    );
    let mut local_opt =
        fake_result("opt", "1.0.0", None, serde_json::json!({ "name": "opt", "version": "1.0.0" }));
    local_opt.id = PkgResolutionId::from("link:packages/opt".to_string());
    local_opt.name_ver = None;
    local_opt.resolution = LockfileResolution::Directory(DirectoryResolution {
        directory: "packages/opt".to_string(),
    });
    local_opt.resolved_via = "local-filesystem".to_string();
    table.insert(("opt".to_string(), "workspace:*".to_string()), local_opt);
    table.insert(
        ("opt".to_string(), "1.0.0".to_string()),
        fake_result("opt", "1.0.0", None, serde_json::json!({ "name": "opt", "version": "1.0.0" })),
    );
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
    let (tmp_root, root_manifest) = fake_manifest(serde_json::json!({ "opt": "workspace:*" }));
    let (tmp_a, a_manifest) = fake_manifest(serde_json::json!({ "needs-opt": "1.0.0" }));
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

    let direct = result.peers.direct_dependencies_by_importer.get("pkg-a").expect("pkg-a");
    assert_eq!(
        direct.get("needs-opt").map(std::string::ToString::to_string),
        Some("needs-opt@1.0.0(opt@1.0.0)".to_string()),
    );
    assert_eq!(
        direct.get("opt").map(std::string::ToString::to_string),
        Some("opt@1.0.0".to_string()),
    );
}

/// The pin satisfies `host`'s optional peer range and the owner's
/// pick does not, so this picker installs nothing only if unreachable
/// candidates are filtered out.
#[tokio::test]
async fn transiently_walked_subtree_versions_do_not_bias_optional_peer_hoists() {
    let host_manifest = serde_json::json!({
        "name": "host",
        "version": "1.0.0",
        "peerDependencies": { "dep": "^1.0.0" },
        "peerDependenciesMeta": { "dep": { "optional": true } },
    });
    let result = resolve_with_transient_shared_walk(
        host_manifest,
        "1.0.0",
        &[("^1.0.0", "2.0.0"), ("1.0.0", "1.0.0")],
    )
    .await;

    let direct = result.peers.direct_dependencies_by_importer.get(".").expect("root importer");
    assert_eq!(
        direct.get("host").map(std::string::ToString::to_string),
        Some("host@1.0.0".to_string()),
        "the unreachable dep@1.0.0 must not satisfy host's optional peer",
    );
    assert_eq!(direct.get("dep"), None, "no optional-peer hoist may install dep");
    assert_eq!(graph_versions_of(&result, "dep"), ["2.0.0"]);
}

/// The required-peer picker dedupes onto the highest satisfying
/// candidate, so the pin is higher than (and as satisfying as) the
/// owner's pick — the arrangement where unreachable-candidate
/// filtering is observable through this picker.
#[tokio::test]
async fn transiently_walked_subtree_versions_do_not_bias_required_peer_hoists() {
    let host_manifest = serde_json::json!({
        "name": "host",
        "version": "1.0.0",
        "peerDependencies": { "dep": "^1.0.0" },
    });
    let result = resolve_with_transient_shared_walk(
        host_manifest,
        "1.9.0",
        &[("^1.0.0", "1.5.0"), ("1.5.0", "1.5.0"), ("1.9.0", "1.9.0")],
    )
    .await;

    let direct = result.peers.direct_dependencies_by_importer.get(".").expect("root importer");
    assert_eq!(
        direct.get("host").map(std::string::ToString::to_string),
        Some("host@1.0.0(dep@1.5.0)".to_string()),
        "the required-peer hoist must dedupe onto the reachable version",
    );
    assert_eq!(
        direct.get("dep").map(std::string::ToString::to_string),
        Some("dep@1.5.0".to_string()),
    );
    assert_eq!(graph_versions_of(&result, "dep"), ["1.5.0"]);
}

/// The root's `react` wins even though it doesn't satisfy `lucide-react`'s
/// declared peer range: keeping one copy across the workspace is the point
/// of the setting.
#[tokio::test]
async fn non_root_importer_hoists_the_root_importers_peer_provider() {
    let (_root_tmp, root_manifest) = fake_manifest(serde_json::json!({ "react": "19.2.0" }));
    let (_app_tmp, app_manifest) = fake_manifest(serde_json::json!({ "lucide": "1.0.0" }));
    let importers = vec![
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
        WorkspaceImporter { id: "app-b".to_string(), manifest: &app_manifest },
    ];
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
            (
                ("react".to_string(), "19.2.0".to_string()),
                fake_result(
                    "react",
                    "19.2.0",
                    None,
                    serde_json::json!({ "name": "react", "version": "19.2.0" }),
                ),
            ),
            (
                ("react".to_string(), "^18.0.0".to_string()),
                fake_result(
                    "react",
                    "18.3.1",
                    None,
                    serde_json::json!({ "name": "react", "version": "18.3.1" }),
                ),
            ),
            (
                ("lucide".to_string(), "1.0.0".to_string()),
                fake_result(
                    "lucide",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "lucide",
                        "version": "1.0.0",
                        "peerDependencies": { "react": "^18.0.0" },
                    }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let mut opts = workspace_opts(false, false);
    opts.auto_install_peers = true;
    opts.resolve_peers_from_workspace_root = true;
    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let mut importer_opts =
                importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None);
            importer_opts.resolve_peers_from_workspace_root = true;
            importer_opts
        })
        .await
        .expect("resolve workspace with a root-provided peer");

    assert_eq!(graph_versions_of(&result, "react"), ["19.2.0"], "one react, the root's");
    let app_deps = &result.peers.direct_dependencies_by_importer["app-b"];
    assert_eq!(app_deps["react"].as_str(), "react@19.2.0");
    assert_eq!(app_deps["lucide"].as_str(), "lucide@1.0.0(react@19.2.0)");
}

/// A tarball / git / local root dep leaves `name_ver` unset, and the alias
/// it was declared under need not be its name.
#[tokio::test]
async fn root_dep_named_only_by_its_manifest_still_provides_the_peer() {
    const TARBALL: &str = "https://tarballs.example/real-peer-1.0.0.tgz";
    let (_root_tmp, root_manifest) = fake_manifest(serde_json::json!({ "aliased": TARBALL }));
    let (_app_tmp, app_manifest) = fake_manifest(serde_json::json!({ "consumer": "1.0.0" }));
    let importers = vec![
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
        WorkspaceImporter { id: "app-b".to_string(), manifest: &app_manifest },
    ];
    let mut unnamed = fake_result(
        "real-peer",
        "1.0.0",
        None,
        serde_json::json!({ "name": "real-peer", "version": "1.0.0" }),
    );
    unnamed.name_ver = None;
    unnamed.id = pnpm_resolving_resolver_base::PkgResolutionId::from(TARBALL.to_string());
    unnamed.alias = Some("aliased".to_string());
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
            (("aliased".to_string(), TARBALL.to_string()), unnamed.clone()),
            (("real-peer".to_string(), TARBALL.to_string()), unnamed),
            (
                ("real-peer".to_string(), "^1.0.0".to_string()),
                fake_result(
                    "real-peer",
                    "1.9.9",
                    None,
                    serde_json::json!({ "name": "real-peer", "version": "1.9.9" }),
                ),
            ),
            (
                ("consumer".to_string(), "1.0.0".to_string()),
                fake_result(
                    "consumer",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "consumer",
                        "version": "1.0.0",
                        "peerDependencies": { "real-peer": "^1.0.0" },
                    }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let mut opts = workspace_opts(false, false);
    opts.auto_install_peers = true;
    opts.resolve_peers_from_workspace_root = true;
    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let mut importer_opts =
                importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None);
            importer_opts.resolve_peers_from_workspace_root = true;
            importer_opts
        })
        .await
        .expect("resolve workspace with a manifest-named root peer provider");

    assert!(
        !result.peers.graph.keys().any(|dep_path| dep_path.as_str().contains("1.9.9")),
        "the peer must come from the root's tarball dep, not a second copy off the registry: {:?}",
        result.peers.graph.keys().map(|k| k.as_str().to_string()).collect::<Vec<_>>(),
    );
}

/// A `file:` tarball has no directory to read a version out of, so the
/// root's specifier reaches no candidate at all. Both shapes a local
/// resolution can take are covered: with a manifest the dep is nameable
/// and carries the resolver's own specifier, without one it is named by
/// its alias from the declared specifier.
#[tokio::test]
async fn a_project_relative_root_dep_is_not_offered_as_a_peer_provider() {
    project_relative_root_dep_is_not_a_provider(
        "file:./vendor/real-peer.tgz",
        ManifestAvailability::Absent,
    )
    .await;
}

#[tokio::test]
async fn a_nameable_project_relative_root_dep_is_not_offered_as_a_peer_provider() {
    project_relative_root_dep_is_not_a_provider(
        "file:./vendor/real-peer.tgz",
        ManifestAvailability::Present,
    )
    .await;
}

/// The path form of `workspace:` names a directory relative to the
/// declaring project, so it goes through the same manifest read as
/// `link:` and `file:` — unlike the range form, which
/// [`fn@a_workspace_range_root_dep_is_offered_as_a_peer_provider`] covers.
/// Nothing is on disk at the path here, so it yields no candidate.
#[tokio::test]
async fn a_workspace_path_root_dep_is_not_offered_as_a_peer_provider() {
    project_relative_root_dep_is_not_a_provider(
        "workspace:../packages/real-peer",
        ManifestAvailability::Present,
    )
    .await;
}

/// The root's authority over the peer does not depend on the protocol it
/// declared the package with: the linked package's own version stands in
/// for its path, so a sibling's newer copy loses to it exactly as it would
/// to a registry dependency of the root.
#[tokio::test]
async fn a_link_root_dep_provides_the_peer_at_the_linked_packages_version() {
    link_root_dep_peer_provider(Some("1.2.3"), "real-peer@1.2.3").await;
}

/// Nothing stands in for the path when the target names no version, so the
/// root offers no candidate and the peer falls through to the graph — the
/// path itself must never survive as the specifier.
#[tokio::test]
async fn a_versionless_link_root_dep_is_not_offered_as_a_peer_provider() {
    link_root_dep_peer_provider(None, "real-peer@1.9.9").await;
}

/// A `workspace:` range selects the same workspace package from every
/// importer, so the root's copy satisfies another importer's peer instead
/// of a second copy off the registry.
#[tokio::test]
async fn a_workspace_range_root_dep_is_offered_as_a_peer_provider() {
    const WORKSPACE_RANGE: &str = "workspace:^1.0.0";
    const LINK: &str = "link:../packages/real-peer";
    let (_root_tmp, root_manifest) =
        fake_manifest(serde_json::json!({ "real-peer": WORKSPACE_RANGE }));
    let (_app_tmp, app_manifest) = fake_manifest(serde_json::json!({ "consumer": "1.0.0" }));
    let importers = vec![
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
        WorkspaceImporter { id: "app-b".to_string(), manifest: &app_manifest },
    ];
    let mut linked = fake_result(
        "real-peer",
        "1.0.0",
        None,
        serde_json::json!({ "name": "real-peer", "version": "1.0.0" }),
    );
    linked.name_ver = None;
    linked.normalized_bare_specifier = Some(WORKSPACE_RANGE.to_string());
    linked.id = pnpm_resolving_resolver_base::PkgResolutionId::from(LINK.to_string());
    linked.resolution = LockfileResolution::Directory(DirectoryResolution {
        directory: "../packages/real-peer".to_string(),
    });
    linked.resolved_via = "workspace".to_string();
    let resolver = RecordingResolver {
        table: HashMap::from_iter([
            (("real-peer".to_string(), WORKSPACE_RANGE.to_string()), linked),
            (
                ("real-peer".to_string(), "^1.0.0".to_string()),
                fake_result(
                    "real-peer",
                    "1.9.9",
                    None,
                    serde_json::json!({ "name": "real-peer", "version": "1.9.9" }),
                ),
            ),
            (
                ("consumer".to_string(), "1.0.0".to_string()),
                fake_result(
                    "consumer",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "consumer",
                        "version": "1.0.0",
                        "peerDependencies": { "real-peer": "^1.0.0" },
                    }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let mut opts = workspace_opts(false, false);
    opts.auto_install_peers = true;
    opts.resolve_peers_from_workspace_root = true;
    let result =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |importer| {
            let mut importer_opts =
                importer_opts(std::path::PathBuf::from("/repo").join(&importer.id), None);
            importer_opts.resolve_peers_from_workspace_root = true;
            importer_opts
        })
        .await
        .expect("resolve workspace with a workspace: range root dep");

    assert_eq!(result.peers.direct_dependencies_by_importer["."]["real-peer"].as_str(), LINK);
    assert_eq!(
        result.peers.direct_dependencies_by_importer["app-b"]["real-peer"].as_str(),
        "link:../../packages/real-peer",
        "app-b's peer is the same workspace package, reached from app-b's own directory",
    );
    assert!(
        !result.peers.graph.keys().any(|dep_path| dep_path.as_str().contains("1.9.9")),
        "no second copy off the registry: {:?}",
        result.peers.graph.keys().map(|key| key.as_str().to_string()).collect::<Vec<_>>(),
    );
}

/// End-to-end companion of
/// `resolve_peers::tests::cached_subtree_reuse_reports_no_peer_providers`:
/// pkg-b reuses two foreign-owned subtrees — `mid`, whose walk resolved
/// `peerpkg`, and `s2wrap`, whose consumer's miss the owning importer
/// satisfied from its own ancestors (hiding it from pkg-b's hoist).
#[tokio::test]
async fn importer_sharing_foreign_subtrees_binds_peers_from_workspace_root() {
    let mut table = HashMap::default();
    table.insert(
        ("mid".to_string(), "1.0.0".to_string()),
        fake_result(
            "mid",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "mid",
                "version": "1.0.0",
                "dependencies": { "peerpkg": "2.0.0", "consumer": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("consumer".to_string(), "1.0.0".to_string()),
        fake_result(
            "consumer",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "consumer",
                "version": "1.0.0",
                "peerDependencies": { "peerpkg": "*", "peerx": "*" },
            }),
        ),
    );
    table.insert(
        ("holder".to_string(), "1.0.0".to_string()),
        fake_result(
            "holder",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "holder",
                "version": "1.0.0",
                "dependencies": { "peerpkg": "2.0.0", "s2wrap": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("s2wrap".to_string(), "1.0.0".to_string()),
        fake_result(
            "s2wrap",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "s2wrap",
                "version": "1.0.0",
                "dependencies": { "consumer2": "1.0.0" },
            }),
        ),
    );
    // pkg-b reaches `s2wrap` through `bwrap` so both importers see it at
    // depth 1 and the children-owner claim falls to pkg-a2 (lower
    // importer order) — a direct depth-0 reference would make pkg-b the
    // owner and no cross-importer sharing would occur.
    table.insert(
        ("bwrap".to_string(), "1.0.0".to_string()),
        fake_result(
            "bwrap",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "bwrap",
                "version": "1.0.0",
                "dependencies": { "s2wrap": "1.0.0" },
            }),
        ),
    );
    table.insert(
        ("consumer2".to_string(), "1.0.0".to_string()),
        fake_result(
            "consumer2",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "consumer2",
                "version": "1.0.0",
                "peerDependencies": { "peerpkg": "*" },
            }),
        ),
    );
    for version in ["1.0.0", "2.0.0"] {
        table.insert(
            ("peerpkg".to_string(), version.to_string()),
            fake_result(
                "peerpkg",
                version,
                None,
                serde_json::json!({ "name": "peerpkg", "version": version }),
            ),
        );
    }
    // `peerx` keeps `mid`'s subtree non-pure: `consumer` resolves it
    // against the importer's own direct dep, so the subtree's verdict
    // enters the peers cache (pure subtrees bypass it) and pkg-b's
    // revisit exercises the cache-replay path under test.
    table.insert(
        ("peerx".to_string(), "1.0.0".to_string()),
        fake_result(
            "peerx",
            "1.0.0",
            None,
            serde_json::json!({ "name": "peerx", "version": "1.0.0" }),
        ),
    );
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
    let (tmp_root, root_manifest) = fake_manifest(serde_json::json!({ "peerpkg": "1.0.0" }));
    let (tmp_a, a_manifest) =
        fake_manifest(serde_json::json!({ "mid": "1.0.0", "peerx": "1.0.0" }));
    let (tmp_a2, a2_manifest) = fake_manifest(serde_json::json!({ "holder": "1.0.0" }));
    let (tmp_b, b_manifest) =
        fake_manifest(serde_json::json!({ "mid": "1.0.0", "bwrap": "1.0.0", "peerx": "1.0.0" }));
    let importers = [
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
        WorkspaceImporter { id: "pkg-a".to_string(), manifest: &a_manifest },
        WorkspaceImporter { id: "pkg-a2".to_string(), manifest: &a2_manifest },
        WorkspaceImporter { id: "pkg-b".to_string(), manifest: &b_manifest },
    ];
    let dirs = [tmp_root.path(), tmp_a.path(), tmp_a2.path(), tmp_b.path()];

    let mut opts = workspace_opts(false, false);
    opts.auto_install_peers = true;
    opts.resolve_peers_from_workspace_root = true;
    let mut next = 0;
    let result = resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |_| {
        let dir = dirs[next].to_path_buf();
        next += 1;
        let mut opts = importer_opts(dir, None);
        opts.auto_install_peers = true;
        opts.resolve_peers_from_workspace_root = true;
        opts
    })
    .await
    .unwrap();

    // Under pkg-a2, `consumer2` binds to holder's peerpkg@2.0.0.
    let a2_direct =
        result.peers.direct_dependencies_by_importer.get("pkg-a2").expect("pkg-a2 importer");
    assert_eq!(
        a2_direct.get("holder").map(std::string::ToString::to_string),
        Some("holder@1.0.0".to_string()),
        "holder satisfies its subtree's peer internally",
    );
    let a_direct =
        result.peers.direct_dependencies_by_importer.get("pkg-a").expect("pkg-a importer");
    assert_eq!(
        a_direct.get("mid").map(std::string::ToString::to_string),
        Some("mid@1.0.0(peerx@1.0.0)".to_string()),
        "consumer's peerx resolves against pkg-a's direct dep",
    );

    // Under pkg-b, `consumer2` has no provider in its own tree: its peer
    // must fall back to the workspace root's peerpkg@1.0.0, not bind to
    // the peerpkg@2.0.0 provider a reused subtree's walk resolved.
    let b_direct =
        result.peers.direct_dependencies_by_importer.get("pkg-b").expect("pkg-b importer");
    assert_eq!(
        b_direct.get("bwrap").map(std::string::ToString::to_string),
        Some("bwrap@1.0.0(peerpkg@1.0.0)".to_string()),
        "pkg-b's own consumers must not inherit a reused subtree's provider",
    );
    assert_eq!(
        b_direct.get("mid").map(std::string::ToString::to_string),
        Some("mid@1.0.0(peerx@1.0.0)".to_string()),
    );
}

#[tokio::test]
async fn finalized_packages_are_announced_once_their_peer_free_subtree_settles() {
    let announced = announced_finalized_packages(
        serde_json::json!({ "pure": "^1.0.0", "peered": "^1.0.0" }),
        HashMap::from_iter([
            table_entry("pure", serde_json::json!({ "dependencies": { "leaf": "^1.0.0" } })),
            table_entry("leaf", serde_json::json!({})),
            table_entry(
                "peered",
                serde_json::json!({
                    "dependencies": { "leaf": "^1.0.0" },
                    "peerDependencies": { "react": "*" },
                    "peerDependenciesMeta": { "react": { "optional": true } },
                }),
            ),
        ]),
    )
    .await;
    // `peered` declares a peer, so neither it nor its dep path is final;
    // `pure` and `leaf` are, and `pure` is announced with its edge.
    assert_eq!(
        announced,
        vec![
            ("leaf@1.0.0".to_string(), vec![]),
            ("pure@1.0.0".to_string(), vec!["leaf@1.0.0".to_string()]),
        ],
    );
}

#[tokio::test]
async fn finalized_packages_include_peer_free_cycles() {
    let announced = announced_finalized_packages(
        serde_json::json!({ "ping": "^1.0.0" }),
        HashMap::from_iter([
            table_entry("ping", serde_json::json!({ "dependencies": { "pong": "^1.0.0" } })),
            table_entry("pong", serde_json::json!({ "dependencies": { "ping": "^1.0.0" } })),
        ]),
    )
    .await;
    let ids = announced.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>();
    assert_eq!(ids, ["ping@1.0.0", "pong@1.0.0"]);
}

#[tokio::test]
async fn warm_up_skips_dependencies_a_package_declares_as_its_own_peers() {
    // `p` lists `q` both as a dependency and as a peer. Under
    // autoInstallPeers the walk drops the dependency edge and resolves
    // the peer at the importer, so the dependency range must never be
    // asked for, not even speculatively two levels down.
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
    let mut opts = workspace_opts(false, false);
    opts.auto_install_peers = true;
    let result =
        resolve_single_importer(&resolver, serde_json::json!({ "a": "^1.0.0" }), opts, None)
            .await
            .expect("resolve");
    assert_eq!(graph_versions_of(&result, "q"), ["1.0.0"]);
    assert_eq!(resolver.calls_for("q", "^2.0.0"), 0);
}
