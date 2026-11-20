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

#[tokio::test]
#[cfg_attr(
    not(unix),
    ignore = "regular file parent produces NotFound rather than NotADirectory on Windows"
)]
async fn inaccessible_purged_package_dir_keeps_suppression_without_blocking_other_packages() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let broken = pkg("@corp/broken");
    let healthy = pkg("healthy");

    storage.write_upstream_document("broken-ns", &broken, b"{\"broken\":true}").await.unwrap();
    storage.write_upstream_document("healthy-ns", &healthy, b"{\"healthy\":true}").await.unwrap();
    storage.mark_upstream_package_purged("broken-ns", &broken).await.unwrap();

    // Replace the scope parent with a regular file so `metadata(package_dir)` fails with ENOTDIR.
    let broken_dir = storage.cached.namespaced("broken-ns").package_dir(&broken);
    let scope_parent = broken_dir
        .parent()
        .unwrap()
        .to_path_buf();
    std::fs::remove_dir_all(&scope_parent).unwrap();
    std::fs::write(&scope_parent, b"not-a-directory").unwrap();
    assert_eq!(
        std::fs::metadata(&broken_dir).unwrap_err().kind(),
        std::io::ErrorKind::NotADirectory,
    );

    drop(storage);
    let restarted = storage_in(&tmp);
    assert_eq!(
        restarted
            .read_upstream_document_any("healthy-ns", &healthy)
            .await
            .unwrap()
            .as_deref(),
        Some(b"{\"healthy\":true}".as_slice()),
    );
    assert!(restarted.is_upstream_package_purged("broken-ns", &broken).await.unwrap());
    assert!(
        restarted
            .read_upstream_document_any("broken-ns", &broken)
            .await
            .unwrap()
            .is_none(),
    );
}

#[tokio::test]
async fn corrupt_purge_marker_suppresses_only_its_own_package_across_restart() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let corrupt = pkg("@corp/corrupt");
    let healthy = pkg("healthy");

    storage.write_upstream_document("corp-ns", &corrupt, b"{\"stale\":true}").await.unwrap();
    storage.write_upstream_document("corp-ns", &healthy, b"{\"healthy\":true}").await.unwrap();
    storage.mark_upstream_package_purged("corp-ns", &corrupt).await.unwrap();

    let corrupt_marker = super::purged::marker_path(&storage.cached.root, "corp-ns", &corrupt);
    assert!(corrupt_marker.exists());
    std::fs::write(&corrupt_marker, b"{not-valid-json").unwrap();

    drop(storage);
    let restarted = storage_in(&tmp);
    assert_eq!(
        restarted
            .read_upstream_document_any("corp-ns", &healthy)
            .await
            .unwrap()
            .as_deref(),
        Some(b"{\"healthy\":true}".as_slice()),
    );
    assert!(restarted.is_upstream_package_purged("corp-ns", &corrupt).await.unwrap());
    assert!(
        restarted
            .read_upstream_document_any("corp-ns", &corrupt)
            .await
            .unwrap()
            .is_none(),
    );
}
