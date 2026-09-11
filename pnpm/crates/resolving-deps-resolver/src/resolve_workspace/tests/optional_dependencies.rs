use super::{
    DependencyGroup, FailureShape, HashMap, Mutex, RecordingResolver, WorkspaceImporter, assert_eq,
    fake_manifest, fake_result, importer_opts, optional_failure_fixture, resolve_workspace,
    workspace_opts,
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
    opts.auto_install_peers = true;
    opts.wanted_lockfile = Some(std::sync::Arc::new(pnpm_lockfile::Lockfile {
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
        opts.auto_install_peers = true;
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
        opts.skipped_optional_log =
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
