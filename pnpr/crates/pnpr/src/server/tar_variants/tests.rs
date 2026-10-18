use super::{COPY_QUOTA_BYTES, CopyReservation, MAX_VARIANT_TAR_BYTES, Ordering, TarVariantQueue};
use std::{path::PathBuf, sync::Arc};

fn reservation(queue: &TarVariantQueue, variant: &str) -> CopyReservation {
    assert!(queue.claim(variant), "{variant} is free to claim");
    CopyReservation {
        quota: Arc::clone(&queue.copies),
        variant: variant.to_string(),
        bytes: 0,
        tar_path: PathBuf::new(),
    }
}

#[test]
fn one_copy_per_variant_is_in_flight_at_a_time() {
    let queue = TarVariantQueue::new();
    let copy = reservation(&queue, "a.tar.zst");
    assert!(!queue.claim("a.tar.zst"), "a concurrent miss shares the copy in flight");
    drop(copy);
    assert!(queue.claim("a.tar.zst"), "a dropped copy frees its variant");
}

#[test]
fn copies_stay_within_the_shared_quota() {
    let queue = TarVariantQueue::new();
    queue.copies.reserved.store(COPY_QUOTA_BYTES - 10, Ordering::Release);
    let mut copy = reservation(&queue, "b.tar.zst");
    assert!(copy.reserve(10));
    assert!(!copy.reserve(1), "the quota is full");
    drop(copy);
    assert_eq!(queue.copies.reserved.load(Ordering::Acquire), COPY_QUOTA_BYTES - 10);
}

#[test]
fn a_copy_stops_at_the_variant_size_limit() {
    let queue = TarVariantQueue::new();
    let mut copy = reservation(&queue, "c.tar.zst");
    assert!(!copy.reserve(MAX_VARIANT_TAR_BYTES + 1));
    assert_eq!(queue.copies.reserved.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn a_copy_offered_to_a_full_queue_is_dropped() {
    let storage_dir = tempfile::TempDir::new().unwrap();
    let storage = pnpr_storage::Storage::new(
        &pnpr_config::HostedStoreConfig::Fs,
        storage_dir.path().join("storage"),
        storage_dir.path().join("cache"),
    )
    .unwrap();
    let name =
        super::CanonicalPackageName::parse("left-pad", pnpr_package_name::Ecosystem::Npm).unwrap();
    let queue = TarVariantQueue::new();
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    let mut kept = Vec::new();
    for key in ["sha512-a", "sha512-b"] {
        let copy = super::TarCopy::start_with_sender(&queue, &storage, &name, key, sender.clone())
            .await
            .unwrap();
        let path = copy.job.copy.tar_path.clone();
        copy.write(b"tar bytes")
            .unwrap()
            .submit();
        kept.push(path);
    }
    assert!(kept[0].exists(), "the first copy waits in the queue");
    assert!(!kept[1].exists(), "the queue was full, so the second copy is removed");
    assert_eq!(
        queue.copies.reserved.load(Ordering::Acquire),
        9,
        "only the queued copy holds quota",
    );
    drop(receiver);
}
