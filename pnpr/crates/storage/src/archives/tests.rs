use super::testing::{promote_file, served, store_file};
use crate::{
    BlobId, HostedStoreConfig, Storage,
    layout::local_archive_path,
    metadata::{BlobRef, Uncommitted},
};
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use tempfile::TempDir;

fn storage_in(tmp: &TempDir) -> Storage {
    Storage::new(
        &HostedStoreConfig::Fs,
        tmp.path().join("storage"),
        tmp.path().join("cache"),
        &pnpr_config::MetadataBackend::Sqlite(tmp.path().join("metadata.sqlite")),
    )
    .unwrap()
}

fn pkg(name: &str) -> CanonicalPackageName {
    CanonicalPackageName::parse(name, Ecosystem::Npm).unwrap()
}

/// Where the default npm namespace of the store in `tmp` keeps `blob`.
fn path_of(tmp: &TempDir, blob: &BlobRef) -> std::path::PathBuf {
    local_archive_path(&tmp.path().join("storage"), "", Ecosystem::Npm, &blob.key())
}

/// A deleter that stalls between marking an incarnation and removing it
/// removes exactly that incarnation's key: the same bytes published again
/// meanwhile went under the next one, which stays and is served.
#[tokio::test]
async fn a_stalled_delete_leaves_the_next_incarnation_served() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let first = store_file(&storage, &pkg("a"), "a-1.0.0.tgz", b"bytes").await
        .artifact
        .blob;
    let database = storage.sql_metadata();
    let file = vec!["a-1.0.0.tgz".to_string()];
    database
        .unmap_artifacts(
            "",
            Ecosystem::Npm,
            "a",
            (
                None,
                file.into_iter()
                    .map(Into::into)
                    .collect(),
            ),
        )
        .await
        .unwrap()
        .unwrap();
    let ids = vec![first.id.clone()];
    let marked = database.mark_reclaiming("", Ecosystem::Npm, ids).await.unwrap();
    assert_eq!(marked, vec![first.clone()]);

    let next = store_file(&storage, &pkg("b"), "b-1.0.0.tgz", b"bytes").await
        .artifact
        .blob;
    assert_eq!(next, BlobRef { id: first.id.clone(), incarnation: 2 });
    // The stalled deleter resumes.
    assert!(
        storage.hosted
            .remove_archive(&first.key())
            .await
            .unwrap(),
    );
    database.finish_reclaims("", Ecosystem::Npm, marked).await.unwrap();

    assert!(!path_of(&tmp, &first).exists());
    assert!(path_of(&tmp, &next).is_file());
    let served = served(&storage, &pkg("b"), "b-1.0.0.tgz").await;
    assert_eq!(served.as_deref(), Some(&b"bytes"[..]));
}

/// Files of two packages with the same bytes share one stored archive,
/// which stays while either maps it.
#[tokio::test]
async fn identical_bytes_of_two_packages_are_stored_once() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let left = store_file(&storage, &pkg("left"), "left-1.0.0.tgz", b"same").await;
    let right = store_file(&storage, &pkg("right"), "right-1.0.0.tgz", b"same").await;
    assert_eq!(left.artifact.blob, right.artifact.blob);
    assert!(left.stored);
    assert!(!right.stored, "live bytes are reused, not stored again");
    assert_eq!(
        storage
            .stored_incarnations()
            .await
            .unwrap()
            .len(),
        1,
    );

    let storage = &storage;
    let unmap = |name: &'static str, file: &'static str| async move {
        storage.unmap_hosted_artifacts(&pkg(name), None, vec![file.to_string()]).await
    };
    assert_eq!(unmap("left", "left-1.0.0.tgz").await.unwrap(), Some(Vec::new()));
    assert_eq!(served(storage, &pkg("left"), "left-1.0.0.tgz").await, None);
    let kept = served(storage, &pkg("right"), "right-1.0.0.tgz").await;
    assert_eq!(kept.as_deref(), Some(&b"same"[..]));

    let reclaimed = unmap("right", "right-1.0.0.tgz").await.unwrap();
    assert_eq!(reclaimed, Some(vec![left.artifact.blob.clone()]));
    assert!(!path_of(&tmp, &left.artifact.blob).exists());
    assert!(
        storage
            .stored_incarnations()
            .await
            .unwrap()
            .is_empty(),
    );
}

/// Two publishes of bytes neither found stored both store the first
/// incarnation, both map it, and one archive holds it.
#[tokio::test]
async fn concurrent_promotions_of_new_bytes_share_the_first_incarnation() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let left = promote_file(&storage, &pkg("left"), "left-1.0.0.tgz", b"shared").await;
    let right = promote_file(&storage, &pkg("right"), "right-1.0.0.tgz", b"shared").await;
    assert_eq!(left.artifact.blob.incarnation, 1);
    assert_eq!(right.artifact.blob, left.artifact.blob);
    storage.map_hosted_artifact(left.artifact).await.unwrap();
    storage.map_hosted_artifact(right.artifact).await.unwrap();
    assert_eq!(
        storage
            .stored_incarnations()
            .await
            .unwrap()
            .len(),
        1,
    );
    for (name, file) in [("left", "left-1.0.0.tgz"), ("right", "right-1.0.0.tgz")] {
        assert_eq!(served(&storage, &pkg(name), file).await.as_deref(), Some(&b"shared"[..]));
    }
}

/// An incarnation no commit made live is removed, and a name that is no
/// key pnpr stores archives under is never touched.
#[tokio::test]
async fn an_uncommitted_incarnation_is_removed_and_other_names_are_left() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let promoted = promote_file(&storage, &pkg("pkg"), "pkg-1.0.0.tgz", b"refused").await;
    let blob = promoted.artifact.blob;
    let stray = path_of(&tmp, &blob).with_file_name("notes.txt");
    std::fs::write(&stray, b"an operator's").unwrap();
    let listed: Vec<BlobRef> = storage
        .stored_incarnations()
        .await
        .unwrap()
        .into_iter()
        .map(|(blob, _)| blob)
        .collect();
    assert_eq!(listed, vec![blob.clone()]);

    let removed = storage
        .remove_uncommitted(vec![Uncommitted { blob: blob.clone(), may_fence: true }])
        .await
        .unwrap();
    assert_eq!(removed, vec![blob.clone()]);
    assert!(!path_of(&tmp, &blob).exists());
    assert!(stray.is_file());
    // The fenced incarnation stays gone: storing the bytes again takes the
    // next one.
    let again = store_file(&storage, &pkg("pkg"), "pkg-1.0.0.tgz", b"refused").await;
    assert_eq!(
        again.artifact.blob,
        BlobRef { id: BlobId::of_bytes(Ecosystem::Npm, b"refused"), incarnation: 2 },
    );
}

/// A collector removes only the mapping it scanned. Another collector
/// unmapped the file first and an upload mapped it again, which changes no
/// package generation: the stale collector's removal leaves the new mapping
/// and the file is still served.
#[tokio::test]
async fn a_stale_collector_leaves_a_mapping_made_since_its_scan() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("a");
    store_file(&storage, &name, "a-1.0.0.tgz", b"bytes").await;
    let (expected, scanned) = storage.package_mappings(&name).await.unwrap();

    let first = storage.unmap_hosted_artifacts(&name, expected, scanned.clone()).await.unwrap();
    assert!(first.is_some());
    assert_eq!(served(&storage, &name, "a-1.0.0.tgz").await, None);
    store_file(&storage, &name, "a-1.0.0.tgz", b"bytes").await;

    let stale = storage.unmap_hosted_artifacts(&name, expected, scanned).await.unwrap();
    assert_eq!(stale.as_deref(), Some(&[][..]));
    assert_eq!(served(&storage, &name, "a-1.0.0.tgz").await.as_deref(), Some(&b"bytes"[..]));
}

/// Uploading bytes a file is already mapped to maps it afresh: a collector
/// that scanned the mapping before the upload leaves the file mapped.
#[tokio::test]
async fn a_reupload_of_mapped_bytes_outlives_a_stale_collector() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("a");
    store_file(&storage, &name, "a-1.0.0.tgz", b"bytes").await;
    let (expected, scanned) = storage.package_mappings(&name).await.unwrap();

    store_file(&storage, &name, "a-1.0.0.tgz", b"bytes").await;
    let (_, remapped) = storage.package_mappings(&name).await.unwrap();
    assert_ne!(remapped[0].mapping, scanned[0].mapping);

    let stale = storage.unmap_hosted_artifacts(&name, expected, scanned).await.unwrap();
    assert_eq!(stale.as_deref(), Some(&[][..]));
    assert_eq!(served(&storage, &name, "a-1.0.0.tgz").await.as_deref(), Some(&b"bytes"[..]));
}

/// A file mapped to other bytes keeps that mapping when other bytes are
/// mapped to it: the first mapping stands.
#[tokio::test]
async fn mapping_other_bytes_to_a_mapped_file_keeps_the_first() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let name = pkg("a");
    store_file(&storage, &name, "a-1.0.0.tgz", b"first").await;
    let (_, before) = storage.package_mappings(&name).await.unwrap();
    store_file(&storage, &name, "a-1.0.0.tgz", b"second").await;
    let (_, after) = storage.package_mappings(&name).await.unwrap();
    assert_eq!(after[0].mapping, before[0].mapping);
    assert_eq!(served(&storage, &name, "a-1.0.0.tgz").await.as_deref(), Some(&b"first"[..]));
}
