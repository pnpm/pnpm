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
