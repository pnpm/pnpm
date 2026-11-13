use super::{
    Arc, DepPath, DependencyGroup, HashMap, Mutex, PreferredVersions, SnapshotEntry, StubResolver,
    VersionSelectorEntry, VersionSelectorType, VersionSelectorWithWeight, VersionSelectors,
    default_opts, fake_manifest, fake_result, peer_context_lockfile,
};
use crate::{
    ResolveImporterResult, WorkspaceTreeCtx, resolve_importer::resolve_importer_with_workspace,
};
use pnpm_resolving_resolver_base::{DIRECT_DEP_SELECTOR_WEIGHT, EXISTING_VERSION_SELECTOR_WEIGHT};
use serde_json::json;

#[tokio::test]
async fn manifest_preferences_survive_a_wanted_lockfile_without_auto_install() {
    for lockfile in [None, Some(Arc::new(peer_context_lockfile(None, [])))] {
        let result = resolve_with_peer_preference(DIRECT_DEP_SELECTOR_WEIGHT, lockfile).await;
        assert_eq!(
            result.peers_result.direct_dependencies_by_alias.get("peer"),
            Some(&DepPath::from("peer@2.0.0")),
        );
    }
}

#[tokio::test]
async fn unlisted_importers_do_not_hoist_other_importers_locked_required_peers() {
    let lockfile =
        peer_context_lockfile(None, [("other@1.0.0(peer@2.0.0)", SnapshotEntry::default())]);
    let result =
        resolve_with_peer_preference(EXISTING_VERSION_SELECTOR_WEIGHT, Some(Arc::new(lockfile)))
            .await;
    assert!(!result.peers_result.direct_dependencies_by_alias.contains_key("peer"));
}

#[tokio::test]
async fn retained_required_peer_pin_survives_without_a_current_provider() {
    let mut lockfile =
        peer_context_lockfile(None, [("consumer@1.0.0(peer@2.0.0)", SnapshotEntry::default())]);
    lockfile.importers.insert(
        ".".to_string(),
        serde_json::from_value(json!({
            "dependencies": {
                "consumer": { "specifier": "1.0.0", "version": "1.0.0(peer@2.0.0)" },
            },
        }))
        .unwrap(),
    );
    let result =
        resolve_with_peer_preference(EXISTING_VERSION_SELECTOR_WEIGHT, Some(Arc::new(lockfile)))
            .await;
    assert_eq!(
        result.peers_result.direct_dependencies_by_alias.get("consumer"),
        Some(&DepPath::from("consumer@1.0.0(peer@2.0.0)")),
    );
}

async fn resolve_with_peer_preference(
    weight: u32,
    lockfile: Option<Arc<pnpm_lockfile::Lockfile>>,
) -> ResolveImporterResult {
    let resolver = StubResolver {
        table: HashMap::from_iter([
            (
                ("consumer".into(), "1.0.0".into()),
                fake_result(
                    "consumer",
                    "1.0.0",
                    json!({"name":"consumer", "version":"1.0.0", "peerDependencies":{"peer":"^2.0.0"}}),
                ),
            ),
            (
                ("peer".into(), "2.0.0".into()),
                fake_result("peer", "2.0.0", json!({"name":"peer", "version":"2.0.0"})),
            ),
        ]),
        calls: Mutex::new(Vec::new()),
    };
    let (_root, manifest) = fake_manifest(json!({"consumer":"1.0.0"}));
    let mut opts = default_opts();
    opts.peers.auto_install_peers = false;
    opts.resolution.all_preferred_versions = Arc::new(PreferredVersions::from_iter([(
        "peer".into(),
        VersionSelectors::from_iter([(
            "2.0.0".into(),
            VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
                selector_type: VersionSelectorType::Version,
                weight,
            }),
        )]),
    )]));
    let workspace = Arc::new(
        WorkspaceTreeCtx::default().with_auto_install_peers(false).with_wanted_lockfile(lockfile),
    );
    resolve_importer_with_workspace(
        &resolver,
        ".",
        0,
        &manifest,
        [DependencyGroup::Prod],
        opts,
        workspace,
    )
    .await
    .unwrap()
}
