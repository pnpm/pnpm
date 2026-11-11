mod get_admission;

use super::{
    SCRATCH_UNIT_BYTES, SCRATCH_UNITS, STAGING_WAIT, acquire_staging_permit, reserve_scratch_space,
    retain_download_permit,
};
use axum::body::{Body, to_bytes};
use tokio::sync::Semaphore;

#[tokio::test]
async fn temporary_download_capacity_is_retained_until_the_response_finishes_or_drops() {
    static CAPACITY: Semaphore = Semaphore::const_new(1);
    let permit = CAPACITY.acquire().await.unwrap();
    let body = retain_download_permit(Body::from("payload"), permit);
    assert!(CAPACITY.try_acquire().is_err());
    assert_eq!(to_bytes(body, usize::MAX).await.unwrap(), "payload");
    let permit = CAPACITY.try_acquire().unwrap();
    let body = retain_download_permit(Body::from("payload"), permit);
    assert!(CAPACITY.try_acquire().is_err());
    drop(body);
    assert!(CAPACITY.try_acquire().is_ok());
}

#[test]
fn occupied_scratch_space_refuses_downloads_and_is_released_on_drop() {
    static CAPACITY: Semaphore = Semaphore::const_new(SCRATCH_UNITS as usize);
    let full_size = u64::from(SCRATCH_UNITS) * SCRATCH_UNIT_BYTES;
    let permit = reserve_scratch_space(&CAPACITY, full_size).unwrap();
    let body = retain_download_permit(Body::from("payload"), permit);
    let error = reserve_scratch_space(&CAPACITY, 1).unwrap_err();
    assert!(matches!(error, pnpr_error::RegistryError::StorageUnavailable { .. }));
    drop(body);
    let oversized = reserve_scratch_space(&CAPACITY, full_size + 1).unwrap();
    assert!(reserve_scratch_space(&CAPACITY, 1).is_err());
    drop(oversized);
    assert!(reserve_scratch_space(&CAPACITY, 1).is_ok());
}

#[tokio::test(start_paused = true)]
async fn verification_staging_has_a_bounded_wait() {
    static CAPACITY: Semaphore = Semaphore::const_new(0);
    let started = tokio::time::Instant::now();
    assert!(acquire_staging_permit(&CAPACITY).await.is_err());
    assert_eq!(started.elapsed(), STAGING_WAIT);
}

#[tokio::test]
async fn four_stalled_small_downloads_do_not_block_a_fifth_cold_download() {
    let temp = tempfile::TempDir::new().unwrap();
    let store = super::super::S3Store::new(
        std::sync::Arc::new(object_store::memory::InMemory::new()),
        String::new(),
        temp.path().to_path_buf(),
    );
    let mut responses = Vec::new();
    for index in 0..5 {
        let bytes = format!("payload-{index}");
        let key =
            crate::BlobId::of_bytes(pnpr_package_name::Ecosystem::Npm, bytes.as_bytes()).key(1);
        let path = store.staging_tmp_path().await.unwrap();
        tokio::fs::write(&path, bytes.as_bytes()).await.unwrap();
        store.upload_archive(&path, &key).await.unwrap();
        let response =
            tokio::time::timeout(std::time::Duration::from_secs(5), store.open_archive(&key))
                .await
                .unwrap()
                .unwrap()
                .unwrap();
        responses.push(response);
    }
    assert_eq!(responses.len(), 5);
    for (body, _) in responses {
        assert!(!to_bytes(body, usize::MAX).await.unwrap().is_empty());
    }
}
