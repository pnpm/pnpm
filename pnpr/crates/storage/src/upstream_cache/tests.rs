use super::{CanonicalPackageName, Duration, Ecosystem, PURGED_UPSTREAM_DIR, Storage};
use pnpr_config::{DatabaseBackend, HostedStoreConfig};
use tempfile::TempDir;

fn storage_in(tmp: &TempDir) -> Storage {
    Storage::new(
        &HostedStoreConfig::Fs,
        tmp.path().join("storage"),
        tmp.path().join("cache"),
        &DatabaseBackend::Sqlite(tmp.path().join("metadata.sqlite")),
    )
    .unwrap()
}

fn pkg(name: &str) -> CanonicalPackageName {
    CanonicalPackageName::parse(name, Ecosystem::Npm).unwrap()
}

#[tokio::test]
async fn durable_purge_marker_suppresses_retained_cache_across_restarts_until_cleared() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("@corp/example");
    let ns = "~upstreams/corp-hash";

    storage.write_upstream_document(ns, &name, b"{\"name\":\"@corp/example\"}").await.unwrap();
    let mut blob = storage.open_upstream_blob_tmp(ns, &name, "example-1.0.0.tgz").await.unwrap();
    blob.write_all(b"archive").await.unwrap();
    blob.finalize().await.unwrap();

    storage.mark_upstream_package_purged(ns, &name).await.unwrap();
    assert!(
        storage.cached
            .namespaced(ns)
            .document_path(&name)
            .exists(),
    );
    assert!(
        tmp.path()
            .join("cache")
            .join(PURGED_UPSTREAM_DIR)
            .exists(),
    );

    drop(storage);
    let restarted = storage_in(&tmp);
    assert!(restarted.is_upstream_package_purged(ns, &name).await.unwrap());
    assert!(
        restarted
            .read_upstream_document(ns, &name, Duration::from_secs(3600))
            .await
            .unwrap()
            .is_none(),
    );
    assert!(
        restarted
            .read_upstream_document_any(ns, &name)
            .await
            .unwrap()
            .is_none(),
    );
    assert!(
        restarted
            .upstream_document_stamp(ns, &name)
            .await
            .unwrap()
            .is_none(),
    );
    assert!(
        restarted
            .fresh_upstream_document_stamp(ns, &name, Duration::from_secs(3600))
            .await
            .unwrap()
            .is_none(),
    );
    assert!(
        restarted
            .open_upstream_blob(ns, &name, "example-1.0.0.tgz")
            .await
            .unwrap()
            .is_none(),
    );
    assert!(restarted.update_upstream_document(ns, &name, b"{}").await.is_err());

    // Republishing before any read on a fresh instance still clears the on-disk marker.
    drop(restarted);
    let republished = storage_in(&tmp);
    republished.write_upstream_document(ns, &name, b"{\"republished\":true}").await.unwrap();
    assert!(!republished.is_upstream_package_purged(ns, &name).await.unwrap());
    assert_eq!(
        republished
            .read_upstream_document_any(ns, &name)
            .await
            .unwrap()
            .as_deref(),
        Some(b"{\"republished\":true}".as_slice()),
    );

    // A marker whose package directory was later removed is pruned on startup.
    republished.mark_upstream_package_purged(ns, &name).await.unwrap();
    std::fs::remove_dir_all(republished.cached.namespaced(ns).package_dir(&name)).unwrap();
    drop(republished);
    let cleaned = storage_in(&tmp);
    assert!(!cleaned.is_upstream_package_purged(ns, &name).await.unwrap());
}
