use pretty_assertions::assert_eq;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use super::PrioritySemaphore;
use crate::UNPRIORITIZED;

/// Spawn a task that acquires with `priority`, records `label` on
/// grant, and releases immediately. Returns once the waiter is queued,
/// so registration order across calls is deterministic.
async fn spawn_waiter(
    sem: &Arc<PrioritySemaphore>,
    order: &Arc<Mutex<Vec<&'static str>>>,
    label: &'static str,
    priority: u64,
) -> tokio::task::JoinHandle<()> {
    let queued_before = sem.queued_waiters();
    let handle = {
        let sem = Arc::clone(sem);
        let order = Arc::clone(order);
        tokio::spawn(async move {
            let _permit = sem.acquire(priority).await;
            order.lock().unwrap().push(label);
        })
    };
    while sem.queued_waiters() == queued_before {
        tokio::task::yield_now().await;
    }
    handle
}

#[tokio::test]
async fn saturated_semaphore_grants_highest_priority_first() {
    let sem = Arc::new(PrioritySemaphore::new(1));
    let holder = sem.acquire(0).await;

    let order = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();
    for (label, priority) in [("small", 1), ("large", 500), ("medium", 30)] {
        handles.push(spawn_waiter(&sem, &order, label, priority).await);
    }

    drop(holder);
    for handle in handles {
        handle.await.unwrap();
    }
    assert_eq!(*order.lock().unwrap(), vec!["large", "medium", "small"]);
}

#[tokio::test]
async fn equal_priorities_are_granted_fifo() {
    let sem = Arc::new(PrioritySemaphore::new(1));
    let holder = sem.acquire(0).await;

    let order = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();
    for label in ["first", "second", "third"] {
        handles.push(spawn_waiter(&sem, &order, label, 7).await);
    }

    drop(holder);
    for handle in handles {
        handle.await.unwrap();
    }
    assert_eq!(*order.lock().unwrap(), vec!["first", "second", "third"]);
}

#[tokio::test]
async fn cancelled_waiter_passes_its_grant_to_the_next_waiter() {
    let sem = Arc::new(PrioritySemaphore::new(1));
    let holder = sem.acquire(0).await;

    let order = Arc::new(Mutex::new(Vec::new()));
    let cancelled = spawn_waiter(&sem, &order, "cancelled", 100).await;
    let survivor = spawn_waiter(&sem, &order, "survivor", 1).await;

    cancelled.abort();
    let abort_result = cancelled.await;
    dbg!(&abort_result);
    assert!(abort_result.unwrap_err().is_cancelled());

    drop(holder);
    tokio::time::timeout(Duration::from_secs(5), survivor)
        .await
        .expect("the freed permit should reach the surviving waiter")
        .unwrap();
    assert_eq!(*order.lock().unwrap(), vec!["survivor"]);
}

#[tokio::test]
async fn released_permit_with_no_waiters_returns_to_the_free_pool() {
    let sem = PrioritySemaphore::new(1);
    drop(sem.acquire(0).await);
    drop(sem.acquire(0).await);
    assert_eq!(sem.queued_waiters(), 0);
}

/// During a metadata flood, queued downloads still get slots up to
/// their reserved share — strict metadata-first was measured to
/// serialize cold installs (no download starts until resolution
/// drains), so the reserve is what keeps the two phases overlapping.
#[tokio::test]
async fn downloads_keep_their_reserved_share_during_a_metadata_flood() {
    // 2 permits -> a throughput reserve of 1.
    let sem = Arc::new(PrioritySemaphore::new(2));
    let holder_meta = sem.acquire(UNPRIORITIZED).await;
    let holder_download = sem.acquire(5).await;

    let order = Arc::new(Mutex::new(Vec::new()));
    let meta_one = spawn_waiter(&sem, &order, "meta-1", UNPRIORITIZED).await;
    let meta_two = spawn_waiter(&sem, &order, "meta-2", UNPRIORITIZED).await;
    let download = spawn_waiter(&sem, &order, "download", 9).await;

    // Freeing the download slot drops throughput below its reserve, so
    // the queued download outranks the earlier-queued metadata.
    drop(holder_download);
    for handle in [download, meta_one, meta_two] {
        handle.await.unwrap();
    }
    drop(holder_meta);
    assert_eq!(*order.lock().unwrap(), vec!["download", "meta-1", "meta-2"]);
}

/// The reserve is a floor, not free rein: with downloads already at
/// their share, a freed latency slot goes to queued metadata before a
/// higher-priority download — resolution progress can't be starved by
/// a backlog of large archives either.
#[tokio::test]
async fn metadata_outranks_downloads_beyond_the_reserve() {
    let sem = Arc::new(PrioritySemaphore::new(2));
    let holder_meta = sem.acquire(UNPRIORITIZED).await;
    let holder_download = sem.acquire(5).await;

    let order = Arc::new(Mutex::new(Vec::new()));
    let download = spawn_waiter(&sem, &order, "download", 9).await;
    let meta = spawn_waiter(&sem, &order, "meta", UNPRIORITIZED).await;

    // Freeing the metadata slot keeps throughput at its reserve (the
    // download holder is still running), so queued metadata wins even
    // though the download was queued first with a high priority.
    drop(holder_meta);
    meta.await.unwrap();
    drop(holder_download);
    download.await.unwrap();
    assert_eq!(*order.lock().unwrap(), vec!["meta", "download"]);
}

/// With a single permit the reserve must be zero — a reserve covering
/// the whole pool would invert the starvation guarantee and let queued
/// downloads block metadata outright.
#[tokio::test]
async fn single_permit_pool_still_serves_metadata_first() {
    let sem = Arc::new(PrioritySemaphore::new(1));
    let holder = sem.acquire(5).await;

    let order = Arc::new(Mutex::new(Vec::new()));
    let download = spawn_waiter(&sem, &order, "download", 9).await;
    let meta = spawn_waiter(&sem, &order, "meta", UNPRIORITIZED).await;

    drop(holder);
    meta.await.unwrap();
    download.await.unwrap();
    assert_eq!(*order.lock().unwrap(), vec!["meta", "download"]);
}
