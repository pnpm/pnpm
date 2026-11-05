//! Collecting incarnations no commit made live: what a refused, stopped, or
//! late publish or upload stored.

use super::{blob, collect, name, setup};
use crate::RegistryError;
use pnpr_oci::Digest;
use pnpr_package_name::Ecosystem;
use pnpr_storage::{
    BlobId, Storage,
    layout::local_archive_path,
    metadata::{ArtifactBlob, BlobRef},
};
use std::{path::Path, time::Duration};

const DAY: Duration = Duration::from_hours(24);

/// Store `bytes` under incarnation `incarnation` of their identity directly,
/// as a publish does before its commit.
fn store(root: &Path, bytes: &[u8], incarnation: u64) -> BlobRef {
    let blob = BlobRef { id: BlobId::of_bytes(Ecosystem::Oci, bytes), incarnation };
    let path = local_archive_path(root, "", Ecosystem::Oci, &blob.key());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
    blob
}

fn keys(root: &Path) -> Vec<String> {
    let mut keys: Vec<String> = walkdir::WalkDir::new(root.join("default/oci/archives"))
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    keys.sort();
    keys
}

/// The mapping of `bytes` as the image blob of `acme/app`, at `blob`.
fn mapping(bytes: &[u8], blob: BlobRef) -> ArtifactBlob {
    ArtifactBlob {
        ecosystem: Ecosystem::Oci,
        name: "acme/app".to_string(),
        version: String::new(),
        artifact: Digest::of(bytes).blob_filename(),
        blob,
        size: bytes.len() as u64,
    }
}

/// An incarnation stored with no row may be a publish on its way: it is kept
/// while young, and once old enough recorded gone, which refuses that
/// publish's commit, and removed. The bytes stored again take the next one.
#[tokio::test]
async fn an_uncommitted_incarnation_is_fenced_off_and_removed_once_old_enough() {
    let (temp, storage) = setup();
    let root = temp.path().join("store");
    let stored = store(&root, b"in flight", 1);
    assert_eq!(collect(&storage, DAY, false, 0).await.unwrap(), (0, 0));
    assert_eq!(keys(&root), [stored.key()]);
    assert_eq!(collect(&storage, Duration::ZERO, false, 0).await.unwrap(), (1, 9));
    assert!(keys(&root).is_empty());
    let late = storage.map_hosted_artifact(mapping(b"in flight", stored.clone())).await;
    assert!(matches!(late, Err(RegistryError::ArchiveReclaimed { .. })), "{late:?}");
    blob(&storage, &name("acme/app"), b"in flight").await;
    assert_eq!(keys(&root), [BlobRef { incarnation: 2, ..stored }.key()]);
}

/// An incarnation below the live one can never be committed, so it goes
/// whatever its age, and the live one stays.
#[tokio::test]
async fn an_incarnation_below_the_live_one_goes_whatever_its_age() {
    let (temp, storage) = setup();
    let root = temp.path().join("store");
    let repository = name("acme/app");
    let digest = blob(&storage, &repository, b"layer").await;
    let removed = storage.unmap_hosted_artifacts(&repository, None, vec![digest.blob_filename()]);
    assert_eq!(removed.await.unwrap().map(|removed| removed.len()), Some(1));
    blob(&storage, &repository, b"layer").await;
    // A stalled upload of the first incarnation lands late.
    let late = store(&root, b"layer", 1);
    assert_eq!(collect(&storage, DAY, false, 0).await.unwrap(), (1, 5));
    assert_eq!(keys(&root), [BlobRef { incarnation: 2, ..late }.key()]);
    assert_live(&storage, &digest, b"layer").await;
}

/// A name that is no key pnpr stores archives under is never removed: an
/// operator or a newer pnpr may have put it there.
#[tokio::test]
async fn a_name_that_is_no_key_is_never_removed() {
    let (temp, storage) = setup();
    let shard = temp.path().join("store/default/oci/archives/ab");
    std::fs::create_dir_all(&shard).unwrap();
    for file in ["README", "sha256-zz.1", "pkg-1.0.0.tgz"] {
        std::fs::write(shard.join(file), file).unwrap();
    }
    assert_eq!(collect(&storage, Duration::ZERO, false, 0).await.unwrap(), (0, 0));
    assert_eq!(std::fs::read_dir(&shard).unwrap().count(), 3);
}

async fn assert_live(storage: &Storage, digest: &Digest, bytes: &[u8]) {
    let (body, _) = storage
        .open_hosted_blob(&name("acme/app"), &digest.blob_filename())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(axum::body::to_bytes(body, usize::MAX).await.unwrap(), bytes);
}
