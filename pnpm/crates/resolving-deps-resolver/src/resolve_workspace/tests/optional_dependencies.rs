use super::{
    DependencyGroup, FailingAliasResolver, FailureShape, HashMap, Mutex, PackageManifest,
    RecordingResolver, WorkspaceImporter, assert_eq, fake_manifest, fake_result, importer_opts,
    optional_failure_fixture, resolve_workspace, workspace_opts,
};
use std::str::FromStr;

/// A package shared across importers keeps the children missing-peer
/// report from the importer that resolved it first, so a later importer
/// never hoists an optional peer declared inside that shared subtree.
/// The final workspace-wide peer pass still uses each importer's actual
/// provider context, so an importer without the provider gets the
/// peerless variant instead of reusing the first importer's suffixed
/// variant.
#[tokio::test]
async fn shared_subtree_owner_context_suppresses_later_optional_hoist() {
    let mut table = HashMap::default();
    table.insert(
        ("shared".to_string(), "1.0.0".to_string()),
        fake_result(
            "shared",
            "1.0.0",
            None,
            serde_json::json!({
                "name": "shared",
                "version": "1.0.0",
                "dependencies": { "mid": "1.0.0" },
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
    for version in ["18.0.0", "25.0.0"] {
        table.insert(
            ("opt".to_string(), version.to_string()),
            fake_result(
                "opt",
                version,
                None,
                serde_json::json!({ "name": "opt", "version": version }),
            ),
        );
    }
    // `carrier` puts `opt@25.0.0` into the run-resolved preferred
    // versions during the root importer's walk — deep enough that it
    // is not in any peer scope — so a later hoist would pick it as the
    // max satisfying version.
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
    let resolver = RecordingResolver { table, seen: Mutex::new(HashMap::default()) };
    let (tmp_root, root_manifest) = fake_manifest(
        serde_json::json!({ "shared": "1.0.0", "opt": "18.0.0", "carrier": "1.0.0" }),
    );
    let (tmp_a, a_manifest) = fake_manifest(serde_json::json!({ "shared": "1.0.0" }));
    let importers = [
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
        WorkspaceImporter { id: "pkg-a".to_string(), manifest: &a_manifest },
    ];
    let dirs = [tmp_root.path(), tmp_a.path()];

    let mut opts = workspace_opts(false, false);
    opts.peers.auto_install_peers = true;
    let mut next = 0;
    let result = resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |_| {
        let dir = dirs[next].to_path_buf();
        next += 1;
        let mut opts = importer_opts(dir, None);
        opts.peers.auto_install_peers = true;
        opts
    })
    .await
    .unwrap();

    let root_direct = result.peers.direct_dependencies_by_importer.get(".").expect("root importer");
    assert_eq!(
        root_direct.get("shared").map(std::string::ToString::to_string),
        Some("shared@1.0.0(opt@18.0.0)".to_string()),
    );
    let a_direct =
        result.peers.direct_dependencies_by_importer.get("pkg-a").expect("pkg-a importer");
    assert_eq!(
        a_direct.get("shared").map(std::string::ToString::to_string),
        Some("shared@1.0.0".to_string()),
        "pkg-a must not hoist opt, but it also must not reuse root's opt provider",
    );

    let (tmp_root, root_manifest) = fake_manifest(
        serde_json::json!({ "shared": "1.0.0", "opt": "18.0.0", "carrier": "1.0.0" }),
    );
    let (tmp_a, a_manifest) = fake_manifest(serde_json::json!({ "shared": "1.0.0" }));
    let importers = [
        WorkspaceImporter { id: ".".to_string(), manifest: &root_manifest },
        WorkspaceImporter { id: "pkg-a".to_string(), manifest: &a_manifest },
    ];
    let dirs = [tmp_root.path(), tmp_a.path()];
    let mut opts = workspace_opts(false, false);
    opts.peers.auto_install_peers = true;
    opts.reuse.lockfile = Some(std::sync::Arc::new(pnpm_lockfile::Lockfile {
        lockfile_version: pnpm_lockfile::LockfileVersion::<9>::try_from(
            pnpm_lockfile::ComVer::new(9, 0),
        )
        .unwrap(),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: std::collections::HashMap::new(),
        packages: None,
        snapshots: Some(std::collections::HashMap::from([(
            pnpm_lockfile::PkgNameVerPeer::from_str("shared@1.0.0(opt@25.0.0)").unwrap(),
            pnpm_lockfile::SnapshotEntry::default(),
        )])),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    }));
    let mut next = 0;
    let result = resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |_| {
        let dir = dirs[next].to_path_buf();
        next += 1;
        let mut opts = importer_opts(dir, None);
        opts.peers.auto_install_peers = true;
        opts
    })
    .await
    .unwrap();

    let a_direct =
        result.peers.direct_dependencies_by_importer.get("pkg-a").expect("pkg-a importer");
    assert_eq!(
        a_direct.get("shared").map(std::string::ToString::to_string),
        Some("shared@1.0.0(opt@25.0.0)".to_string()),
        "a locked peer provider must remain eligible for importer-local hoisting",
    );
}

#[tokio::test]
async fn shared_subtree_owner_context_is_available_before_optional_hoisting() {
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
                        "dependencies": { "shared": "1.0.0" },
                    }),
                ),
            ),
            (
                ("shared".to_string(), "1.0.0".to_string()),
                fake_result(
                    "shared",
                    "1.0.0",
                    None,
                    serde_json::json!({
                        "name": "shared",
                        "version": "1.0.0",
                        "dependencies": { "mid": "1.0.0" },
                    }),
                ),
            ),
            (
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
            ),
            (
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
            ),
            (
                ("opt".to_string(), "18.0.0".to_string()),
                fake_result(
                    "opt",
                    "18.0.0",
                    None,
                    serde_json::json!({ "name": "opt", "version": "18.0.0" }),
                ),
            ),
            (
                ("opt".to_string(), "25.0.0".to_string()),
                fake_result(
                    "opt",
                    "25.0.0",
                    None,
                    serde_json::json!({ "name": "opt", "version": "25.0.0" }),
                ),
            ),
        ]),
        seen: Mutex::new(HashMap::default()),
    };
    let (tmp_nested, nested_manifest) =
        fake_manifest(serde_json::json!({ "wrapper": "1.0.0", "carrier": "1.0.0" }));
    let (tmp_owner, owner_manifest) =
        fake_manifest(serde_json::json!({ "shared": "1.0.0", "opt": "18.0.0" }));
    let importers = [
        WorkspaceImporter { id: "nested".to_string(), manifest: &nested_manifest },
        WorkspaceImporter { id: "owner".to_string(), manifest: &owner_manifest },
    ];
    let dirs = [tmp_nested.path(), tmp_owner.path()];
    let mut opts = workspace_opts(false, false);
    opts.peers.auto_install_peers = true;
    let mut next = 0;

    let result = resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |_| {
        let dir = dirs[next].to_path_buf();
        next += 1;
        let mut opts = importer_opts(dir, None);
        opts.peers.auto_install_peers = true;
        opts
    })
    .await
    .unwrap();

    let nested =
        result.peers.direct_dependencies_by_importer.get("nested").expect("nested importer");
    assert_eq!(
        nested.get("wrapper").map(std::string::ToString::to_string),
        Some("wrapper@1.0.0".to_string()),
        "the importer visited before the shared-subtree owner must not hoist the owner's peer",
    );
}

/// The coded resolver failures arrive as their own error variants rather
/// than the generic `Resolve` envelope, so each has to stay in the
/// optional-dependency skip arm: an optional dependency the registry has
/// no version of — or no package for — must keep dropping its edge
/// instead of failing the install.
#[tokio::test]
async fn skips_an_optional_dependency_for_every_coded_resolver_failure() {
    for failure in [FailureShape::NoMatchingVersion, FailureShape::RegistryResponse] {
        let (_tmp, manifest, resolver) = optional_failure_fixture(failure);
        let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];
        let skipped = std::sync::Arc::new(Mutex::new(Vec::new()));
        let mut opts = workspace_opts(false, false);
        let sink = std::sync::Arc::clone(&skipped);
        opts.hooks.skipped_optional_log =
            Some(std::sync::Arc::new(move |notification| sink.lock().unwrap().push(notification)));
        let result = resolve_workspace(
            &resolver,
            &importers,
            &[DependencyGroup::Prod, DependencyGroup::Optional],
            opts,
            |_| importer_opts(std::path::PathBuf::from("/repo"), None),
        )
        .await
        .expect("a coded resolution failure of an optional dependency is skipped");

        let direct = &result.peers.direct_dependencies_by_importer["."];
        assert!(direct.contains_key("kept"), "the regular dep resolves: {direct:?}");
        assert!(!direct.contains_key("broken"), "the failing optional edge is dropped: {direct:?}");
        let skipped = skipped.lock().unwrap();
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].name.as_deref(), Some("broken"));
    }
}

/// Resolve one importer against `packages` (name and manifest fields, each
/// at version `1.0.0`), with a `missing` package the registry does not serve.
async fn resolve_with_missing_package(
    importer: serde_json::Value,
    packages: &[(&str, serde_json::Value)],
) -> Result<crate::ResolveWorkspaceResult, crate::ResolveImporterError> {
    resolve_with_missing_package_and(importer, packages, |_, _| {}).await
}

/// [`resolve_with_missing_package`], with `customize` adjusting the
/// resolver and the options first.
async fn resolve_with_missing_package_and(
    importer: serde_json::Value,
    packages: &[(&str, serde_json::Value)],
    customize: impl FnOnce(&mut FailingAliasResolver, &mut crate::WorkspaceResolveOptions),
) -> Result<crate::ResolveWorkspaceResult, crate::ResolveImporterError> {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("package.json");
    std::fs::write(&path, serde_json::to_string(&importer).unwrap()).expect("write package.json");
    let manifest = PackageManifest::from_path(path).expect("parse package.json");
    let mut resolver = FailingAliasResolver {
        table: packages
            .iter()
            .map(|(name, fields)| {
                let mut manifest = serde_json::json!({ "name": name, "version": "1.0.0" });
                manifest
                    .as_object_mut()
                    .unwrap()
                    .extend(fields.as_object().unwrap().clone());
                (
                    (name.to_string(), "1.0.0".to_string()),
                    fake_result(name, "1.0.0", None, manifest),
                )
            })
            .collect(),
        failing: std::collections::HashSet::from_iter(["missing".to_string()]),
        failure: FailureShape::RegistryResponse,
    };
    let mut opts = workspace_opts(false, false);
    customize(&mut resolver, &mut opts);
    let importers = [WorkspaceImporter { id: ".".to_string(), manifest: &manifest }];
    resolve_workspace(
        &resolver,
        &importers,
        &[DependencyGroup::Prod, DependencyGroup::Optional],
        opts,
        |_| importer_opts(tmp.path().to_path_buf(), None),
    )
    .await
}

type SkippedLog = std::sync::Arc<Mutex<Vec<crate::SkippedOptionalDependency>>>;

fn record_skipped(opts: &mut crate::WorkspaceResolveOptions) -> SkippedLog {
    let skipped = SkippedLog::default();
    let sink = std::sync::Arc::clone(&skipped);
    opts.hooks.skipped_optional_log =
        Some(std::sync::Arc::new(move |notification| sink.lock().unwrap().push(notification)));
    skipped
}

fn graph_keys(result: &crate::ResolveWorkspaceResult) -> Vec<String> {
    let mut keys: Vec<String> = result.peers.graph
        .keys()
        .map(ToString::to_string)
        .collect();
    keys.sort();
    keys
}

/// A regular dependency that fails to resolve anywhere below an optional
/// dependency drops that optional dependency with its whole subtree, as npm
/// does, on every platform.
#[tokio::test]
async fn drops_the_optional_dependency_above_an_unresolvable_regular_dependency() {
    let mut skipped = SkippedLog::default();
    let result = resolve_with_missing_package_and(
        serde_json::json!({
            "dependencies": { "kept": "1.0.0" },
            "optionalDependencies": { "opt": "1.0.0" },
        }),
        &[
            ("kept", serde_json::json!({})),
            ("opt", serde_json::json!({ "dependencies": { "mid": "1.0.0", "leaf": "1.0.0" } })),
            ("mid", serde_json::json!({ "dependencies": { "missing": "1.0.0" } })),
            ("leaf", serde_json::json!({})),
        ],
        |_, opts| skipped = record_skipped(opts),
    )
    .await
    .expect("the unresolvable dependency drops its optional ancestor");

    assert_eq!(graph_keys(&result), ["kept@1.0.0"]);
    let direct = &result.peers.direct_dependencies_by_importer["."];
    assert_eq!(direct.keys().collect::<Vec<_>>(), ["kept"]);
    let skipped = skipped.lock().unwrap();
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert_eq!(skipped[0].name.as_deref(), Some("opt"));
    assert_eq!(skipped[0].bare_specifier, "1.0.0");
    assert!(skipped[0].parents.is_empty());
    assert!(skipped[0].details.contains("missing"), "{}", skipped[0].details);
}

/// Dropping an optional dependency of a package keeps the package. The
/// report names the range the package asked for, the project, and the
/// chain of packages from the project down.
#[tokio::test]
async fn drops_a_nested_optional_dependency_and_keeps_its_parent() {
    let mut skipped = SkippedLog::default();
    let result = resolve_with_missing_package_and(
        serde_json::json!({ "dependencies": { "top": "1.0.0" } }),
        &[
            ("top", serde_json::json!({ "dependencies": { "parent": "1.0.0" } })),
            ("parent", serde_json::json!({ "optionalDependencies": { "opt": "^1.0.0" } })),
            ("opt", serde_json::json!({ "dependencies": { "missing": "1.0.0" } })),
        ],
        |resolver, opts| {
            let opt = resolver.table[&("opt".to_string(), "1.0.0".to_string())].clone();
            resolver.table.insert(("opt".to_string(), "^1.0.0".to_string()), opt);
            skipped = record_skipped(opts);
        },
    )
    .await
    .expect("the nested optional dependency is dropped");

    assert_eq!(graph_keys(&result), ["parent@1.0.0", "top@1.0.0"]);
    let parent = &result.peers.graph[&crate::DepPath::from("parent@1.0.0")];
    assert!(parent.edges.children.is_empty(), "{:?}", parent.edges.children);
    let skipped = skipped.lock().unwrap();
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert_eq!(skipped[0].name.as_deref(), Some("opt"));
    assert_eq!(skipped[0].bare_specifier, "^1.0.0");
    let parents: Vec<&str> = skipped[0].parents
        .iter()
        .map(|parent| parent.id.as_str())
        .collect();
    assert_eq!(parents, ["top@1.0.0", "parent@1.0.0"]);
    assert_ne!(skipped[0].prefix, "/lockfile-dir", "the project, not the lockfile directory");
}

/// Without an optional dependency above it, the failure still fails the
/// install.
#[tokio::test]
async fn fails_on_an_unresolvable_dependency_of_a_regular_dependency() {
    let result = resolve_with_missing_package(
        serde_json::json!({ "dependencies": { "regular": "1.0.0" } }),
        &[("regular", serde_json::json!({ "dependencies": { "missing": "1.0.0" } }))],
    )
    .await;

    assert!(result.is_err(), "the failure is not inside an optional subtree");
}

/// A package reached both below an optional dependency and through regular
/// dependencies only cannot be dropped, whichever occurrence walked its
/// children.
#[tokio::test]
async fn fails_when_a_regular_path_reaches_the_broken_package() {
    for (optional, regular) in [("a-opt", "b-regular"), ("b-opt", "a-regular")] {
        let result = resolve_with_missing_package(
            serde_json::json!({
                "dependencies": { regular: "1.0.0" },
                "optionalDependencies": { optional: "1.0.0" },
            }),
            &[
                (optional, serde_json::json!({ "dependencies": { "shared": "1.0.0" } })),
                (regular, serde_json::json!({ "dependencies": { "shared": "1.0.0" } })),
                ("shared", serde_json::json!({ "dependencies": { "missing": "1.0.0" } })),
            ],
        )
        .await;

        assert!(result.is_err(), "`{regular}` needs `shared`, which cannot be installed");
    }
}

/// A left-out package adds no policy violation and no `time:` entry.
#[tokio::test]
async fn left_out_packages_leave_no_policy_violation_or_publish_date() {
    let result = resolve_with_missing_package_and(
        serde_json::json!({
            "dependencies": { "kept": "1.0.0" },
            "optionalDependencies": { "opt": "1.0.0" },
        }),
        &[
            ("kept", serde_json::json!({})),
            ("opt", serde_json::json!({ "dependencies": { "mid": "1.0.0" } })),
            ("mid", serde_json::json!({ "dependencies": { "missing": "1.0.0" } })),
        ],
        |resolver, opts| {
            opts.version.time_based = true;
            for result in resolver.table.values_mut() {
                result.package.published_at = Some("2026-01-01T00:00:00.000Z".to_string());
                let name_ver = result.package.name_ver.clone().unwrap();
                result.policy_violation =
                    Some(pnpm_resolving_resolver_base::ResolutionPolicyViolation {
                        name: name_ver.name,
                        version: name_ver.suffix.to_string(),
                        resolution: result.resolution.clone(),
                        code: "ERR_PNPM_TEST_POLICY",
                        reason: "test".to_string(),
                    });
            }
        },
    )
    .await
    .expect("the unresolvable dependency drops its optional ancestor");

    let violations: Vec<String> = result.merged_tree.policy_violations
        .iter()
        .map(|violation| format!("{}@{}", violation.name, violation.version))
        .collect();
    assert_eq!(violations, ["kept@1.0.0"]);
    assert_eq!(result.time.keys().collect::<Vec<_>>(), ["kept@1.0.0"]);
}

/// Of several failed dependencies, the report names the alphabetically
/// first, whichever failure arrived first.
#[tokio::test]
async fn reports_the_first_failed_dependency_by_name() {
    for _ in 0..8 {
        let mut skipped = SkippedLog::default();
        resolve_with_missing_package_and(
            serde_json::json!({ "optionalDependencies": { "opt": "1.0.0" } }),
            &[(
                "opt",
                serde_json::json!({ "dependencies": { "z-missing": "1.0.0", "a-missing": "1.0.0" } }),
            )],
            |resolver, opts| {
                resolver.failing = std::collections::HashSet::from_iter([
                    "a-missing".to_string(),
                    "z-missing".to_string(),
                ]);
                skipped = record_skipped(opts);
            },
        )
        .await
        .expect("the optional dependency is dropped");

        let skipped = skipped.lock().unwrap();
        assert_eq!(skipped.len(), 1, "{skipped:?}");
        assert!(skipped[0].details.contains("a-missing"), "{}", skipped[0].details);
    }
}
