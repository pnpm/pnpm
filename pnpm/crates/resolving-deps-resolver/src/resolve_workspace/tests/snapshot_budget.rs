use std::{path::PathBuf, sync::Mutex};

use pnpm_package_manifest::DependencyGroup;
use rustc_hash::FxHashMap as HashMap;

use super::{
    RecordingResolver, WorkspaceImporter, fake_manifest, fake_result, importer_opts,
    resolve_workspace, workspace_opts,
};
use crate::{
    resolve_dependency_tree::ResolveDependencyTreeError, resolve_importer::ResolveImporterError,
};

fn peer_variant_resolver(include_unmet_peer: bool) -> RecordingResolver {
    let mut consumer_peers = serde_json::json!({ "peer": "*" });
    if include_unmet_peer {
        consumer_peers["hoisted-peer"] = serde_json::json!("^1.0.0");
    }
    let mut table = HashMap::from_iter([
        (
            ("consumer".to_string(), "1.0.0".to_string()),
            fake_result(
                "consumer",
                "1.0.0",
                None,
                serde_json::json!({
                    "name": "consumer",
                    "version": "1.0.0",
                    "peerDependencies": consumer_peers,
                }),
            ),
        ),
        (
            ("hoisted-peer".to_string(), "^1.0.0".to_string()),
            fake_result(
                "hoisted-peer",
                "1.0.0",
                None,
                serde_json::json!({ "name": "hoisted-peer", "version": "1.0.0" }),
            ),
        ),
    ]);
    for ver in ["1.0.0", "2.0.0", "3.0.0"] {
        table.insert(
            ("peer".to_string(), ver.to_string()),
            fake_result("peer", ver, None, serde_json::json!({ "name": "peer", "version": ver })),
        );
    }
    RecordingResolver { table, seen: Mutex::new(HashMap::default()) }
}

#[tokio::test]
async fn resolve_workspace_enforces_max_snapshots_in_final_peer_walk() {
    let (_tmp_a, manifest_a) =
        fake_manifest(serde_json::json!({ "peer": "1.0.0", "consumer": "1.0.0" }));
    let (_tmp_b, manifest_b) =
        fake_manifest(serde_json::json!({ "peer": "2.0.0", "consumer": "1.0.0" }));
    let (_tmp_c, manifest_c) =
        fake_manifest(serde_json::json!({ "peer": "3.0.0", "consumer": "1.0.0" }));
    let importers = [
        WorkspaceImporter { id: "app-a".to_string(), manifest: &manifest_a },
        WorkspaceImporter { id: "app-b".to_string(), manifest: &manifest_b },
        WorkspaceImporter { id: "app-c".to_string(), manifest: &manifest_c },
    ];
    let resolver = peer_variant_resolver(false);
    let mut opts = workspace_opts(false, false);
    opts.peers.auto_install_peers = false;
    opts.hooks.max_snapshots = Some(2);

    let Err(err) =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |imp| {
            importer_opts(PathBuf::from("/repo").join(&imp.id), None)
        })
        .await
    else {
        panic!("peer variants exceeding max_snapshots must fail resolve_workspace");
    };

    assert!(
        matches!(
            err,
            ResolveImporterError::Resolve(ResolveDependencyTreeError::SnapshotLimitExceeded {
                max_snapshots: 2,
            }),
        ),
        "unexpected error: {err:?}",
    );
}

#[tokio::test]
async fn resolve_workspace_enforces_max_snapshots_during_peer_hoist_discovery() {
    let (_tmp_a, manifest_a) =
        fake_manifest(serde_json::json!({ "peer": "1.0.0", "consumer": "1.0.0" }));
    let (_tmp_b, manifest_b) =
        fake_manifest(serde_json::json!({ "peer": "2.0.0", "consumer": "1.0.0" }));
    let (_tmp_c, manifest_c) =
        fake_manifest(serde_json::json!({ "peer": "3.0.0", "consumer": "1.0.0" }));
    let importers = [
        WorkspaceImporter { id: "app-a".to_string(), manifest: &manifest_a },
        WorkspaceImporter { id: "app-b".to_string(), manifest: &manifest_b },
        WorkspaceImporter { id: "app-c".to_string(), manifest: &manifest_c },
    ];
    let resolver = peer_variant_resolver(true);
    let mut opts = workspace_opts(false, false);
    opts.peers.auto_install_peers = true;
    // `app-a` and `app-b` produce `consumer` variants 1 and 2; `app-c` hits variant 3 in
    // `PeerHoistDiscovery` and must abort before hoisting `hoisted-peer` from the resolver.
    opts.hooks.max_snapshots = Some(2);

    let Err(err) =
        resolve_workspace(&resolver, &importers, &[DependencyGroup::Prod], opts, |imp| {
            let mut imp_opts = importer_opts(PathBuf::from("/repo").join(&imp.id), None);
            imp_opts.peers.auto_install_peers = true;
            imp_opts
        })
        .await
    else {
        panic!("peer hoist discovery exceeding max_snapshots must abort before hoisting");
    };

    assert!(
        matches!(
            err,
            ResolveImporterError::Resolve(ResolveDependencyTreeError::SnapshotLimitExceeded {
                max_snapshots: 2,
            }),
        ),
        "unexpected error: {err:?}",
    );
    let hoisted_resolved = resolver.seen
        .lock()
        .unwrap()
        .contains_key("hoisted-peer");
    assert!(!hoisted_resolved, "discovery must abort before resolving missing hoisted peers");
}
