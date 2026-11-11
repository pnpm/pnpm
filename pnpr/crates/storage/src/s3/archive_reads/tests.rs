use super::retain_download_permit;
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
