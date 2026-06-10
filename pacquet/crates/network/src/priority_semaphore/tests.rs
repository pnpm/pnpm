use pretty_assertions::assert_eq;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use super::PrioritySemaphore;

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
