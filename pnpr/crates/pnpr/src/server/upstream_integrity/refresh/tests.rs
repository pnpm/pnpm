use super::{
    BlobEviction,
    reconcile,
};
use pnpr_package_name::{
    CanonicalPackageName,
    Ecosystem,
};
use serde_json::{
    Value,
    json,
};

#[test]
fn refresh_preserves_matching_pins_and_evicts_removed_or_changed_tarballs() {
    let name = CanonicalPackageName::parse("example", Ecosystem::Npm).unwrap();
    let integrity = ssri::Integrity::from(b"example".as_slice()).to_string();
    let mut old = json!({"versions": {}});
    for version in ["1.0.0", "2.0.0", "3.0.0"] {
        old["versions"][version] = json!({"dist": {
            "tarball": format!("https://registry.test/example/-/example-{version}.tgz"),
            "integrity": integrity,
        }});
    }
    let mut new = old.clone();
    new["versions"]["1.0.0"]["dist"]
        .as_object_mut()
        .unwrap()
        .remove("integrity");
    new["versions"]
        .as_object_mut()
        .unwrap()
        .remove("2.0.0");
    new["versions"]["3.0.0"]["dist"]["shasum"] = json!("changed");
    let result = reconcile(
        Some(&serde_json::to_vec(&old).unwrap()),
        serde_json::to_vec(&new).unwrap(),
        &name,
    )
    .unwrap();
    let doc: Value = serde_json::from_slice(&result.packument.bytes).unwrap();
    assert_eq!(doc["versions"]["1.0.0"]["dist"]["integrity"], integrity);
    let BlobEviction::Files(removed) = result.eviction else {
        panic!("valid metadata permits selective eviction");
    };
    assert_eq!(removed.len(), 2);
    assert!(removed.contains("example-2.0.0.tgz"));
    assert!(removed.contains("example-3.0.0.tgz"));
}

#[test]
fn malformed_cached_metadata_is_replaced_after_purging_its_blobs() {
    let name = CanonicalPackageName::parse("example", Ecosystem::Npm).unwrap();
    let fresh = br#"{"versions":{}}"#.to_vec();
    for previous in [b"malformed".as_slice(), b"{}", br#"{"versions":[]}"#] {
        let result = reconcile(Some(previous), fresh.clone(), &name).unwrap();
        assert_eq!(result.packument.bytes, fresh);
        assert!(matches!(result.eviction, BlobEviction::Package));
    }
}

fn test_state(dir: &std::path::Path, upstream: &str) -> crate::server::AppState {
    let path = dir.join("pnpr.yaml");
    std::fs::write(
        &path,
        format!(
            "storage: ./store\nroutes:\n  allowedPrivateNetworks: [127.0.0.0/8]\nregistries:\n  mirror:\n    type: upstream\n    url: {upstream}\n    public: true\ndefaultRegistry: mirror\n",
        ),
    )
    .unwrap();
    let config = pnpr_fixtures::load_seeded_config(
        &path,
        "127.0.0.1:0".parse().unwrap(),
        Some("http://example.test".to_string()),
    );
    crate::server::routing::state_and_router(config, pnpr_auth::AuthState::in_memory(), None)
        .unwrap()
        .0
}

#[tokio::test]
async fn superseded_refresh_refetches_instead_of_404_when_cached_doc_was_evicted() {
    let mut server = mockito::Server::new_async().await;
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path(), &server.url());
    let routing = state.routing();
    let upstream = &routing.upstream_clients["mirror"];
    let name = CanonicalPackageName::parse("example", Ecosystem::Npm).unwrap();
    let packument = |tag: &str| {
        serde_json::to_vec(&json!({ "versions": { "1.0.0": { "tag": tag } } })).unwrap()
    };

    let older_seq = state.inner.proxy.packument_writes.begin_fetch();
    let newer_seq = state.inner.proxy.packument_writes.begin_fetch();
    super::refresh_cached_packument(&state, "evict-ns", &name, packument("newer"), newer_seq)
        .await
        .unwrap();
    // Simulate cache maintenance evicting the stored document before `older_seq` finishes.
    state.inner.storage.remove_upstream_package("evict-ns", &name).await.unwrap();

    let retry_mock = server
        .mock("GET", "/example")
        .with_status(200)
        .with_body(packument("refetched"))
        .expect(1)
        .create_async()
        .await;
    let result = crate::server::upstream_packuments::cache_upstream_packument(
        &state,
        "evict-ns",
        upstream,
        &name,
        pnpr_upstream::PackumentFetch::Modified(pnpr_upstream::FetchedPackument {
            bytes: packument("stale-older"),
        }),
        older_seq,
    )
    .await
    .unwrap()
    .expect("evicted superseding document refetches instead of returning None");
    retry_mock.assert_async().await;
    let parsed: Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(parsed["versions"]["1.0.0"]["tag"], "refetched");
}

#[tokio::test]
async fn cancelled_purge_completes_and_blocks_older_fetch_writes() {
    use std::sync::Arc;
    use tokio::sync::Notify;

    let server = mockito::Server::new_async().await;
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path(), &server.url());
    let name = CanonicalPackageName::parse("example", Ecosystem::Npm).unwrap();
    state.inner.storage
        .write_upstream_document("cancel-ns", &name, br#"{"versions":{}}"#)
        .await
        .unwrap();

    let older_seq = state.inner.proxy.packument_writes.begin_fetch();
    let purge_seq = state.inner.proxy.packument_writes.begin_fetch();
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let entered_in_remove = Arc::clone(&entered);
    let release_in_remove = Arc::clone(&release);
    let state_in_remove = state.clone();
    let pkg_in_remove = name.clone();

    let mut purge_fut =
        Box::pin(crate::server::upstream_packuments::purge::purge_cached_package_with(
            &state,
            "cancel-ns",
            &name,
            purge_seq,
            move || async move {
                entered_in_remove.notify_one();
                release_in_remove.notified().await;
                state_in_remove.inner.storage.remove_upstream_package("cancel-ns", &pkg_in_remove)
                    .await
            },
        ));
    tokio::select! {
        _ = &mut purge_fut => panic!("purge must wait on release"),
        () = entered.notified() => {}
    }
    // Drop the initiating purge future mid-cleanup.
    drop(purge_fut);
    release.notify_one();

    // Acquiring the package lock waits for the spawned purge task to finish committing.
    let _ =
        crate::server::upstream_packuments::lock_upstream_package(&state, "cancel-ns", &name).await;
    assert!(state.inner.proxy.packument_writes.is_purged("cancel-ns".to_owned(), &name));
    let older = super::refresh_cached_packument(
        &state,
        "cancel-ns",
        &name,
        br#"{"versions":{"1.0.0":{}}}"#.to_vec(),
        older_seq,
    )
    .await
    .unwrap();
    assert!(matches!(older, super::RefreshOutcome::SupersededByPurge));
}

#[tokio::test]
async fn ordinary_read_failure_does_not_delay_forced_refresh_for_expired_tarball() {
    let mut server = mockito::Server::new_async().await;
    let tmp = tempfile::tempdir().unwrap();
    let state = test_state(tmp.path(), &server.url());
    let routing = state.routing();
    let upstream = &routing.upstream_clients["mirror"];
    let name = CanonicalPackageName::parse("example", Ecosystem::Npm).unwrap();

    let outage = server
        .mock("GET", "/example")
        .with_status(503)
        .create_async()
        .await;
    assert!(
        crate::server::load_upstream_packument(
            &state,
            "ns",
            upstream,
            &name,
            std::time::Duration::ZERO
        )
        .await
        .is_err(),
    );
    outage.assert_async().await;
    outage.remove_async().await;
    let refreshed_mock = server
        .mock("GET", "/example")
        .with_status(200)
        .with_body(r#"{"versions":{"1.0.0":{"tag":"fresh-sig"}}}"#)
        .expect(1)
        .create_async()
        .await;
    let reloaded =
        crate::server::upstream_packuments::reload_upstream_packument_for_expired_tarball(
            &state, "ns", upstream, &name,
        )
        .await
        .unwrap()
        .expect("forced refresh reads the current metadata");
    refreshed_mock.assert_async().await;
    let parsed: Value = serde_json::from_slice(&reloaded).unwrap();
    assert_eq!(parsed["versions"]["1.0.0"]["tag"], "fresh-sig");
}
