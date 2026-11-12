use std::time::Duration;

use super::KeyedSemaphores;

#[tokio::test]
async fn limits_concurrency_per_key_without_blocking_other_keys() {
    let limiter = KeyedSemaphores::new(2);

    let first_a = limiter.acquire("account-a").await;
    let second_a = limiter.acquire("account-a").await;
    assert_eq!(limiter.tracked_keys(), 1);

    // Third permit for `account-a` must wait while two are held.
    let blocked =
        tokio::time::timeout(Duration::from_millis(20), limiter.acquire("account-a")).await;
    assert!(blocked.is_err(), "third permit for the same key must wait");

    // Another key acquires immediately despite `account-a` being saturated.
    let first_b = limiter.acquire("account-b").await;
    assert_eq!(limiter.tracked_keys(), 2);

    drop(first_a);
    let third_a = limiter.acquire("account-a").await;

    drop(second_a);
    drop(third_a);
    assert_eq!(limiter.tracked_keys(), 1);

    drop(first_b);
    assert_eq!(limiter.tracked_keys(), 0);
}

#[tokio::test]
async fn cancelling_waiter_evicts_entry_once_active_holder_finishes() {
    let limiter = KeyedSemaphores::new(1);

    let active = limiter.acquire("account-a").await;
    let cancelled =
        tokio::time::timeout(Duration::from_millis(15), limiter.acquire("account-a")).await;
    assert!(cancelled.is_err());
    assert_eq!(limiter.tracked_keys(), 1);

    drop(active);
    assert_eq!(
        limiter.tracked_keys(),
        0,
        "cancelled waiter must not leave a dangling map entry after the holder drops",
    );
}
