use super::spawn_extraction;
use pretty_assertions::assert_eq;
use std::sync::mpsc;
use tokio::{
    runtime::Builder,
    sync::{Semaphore, oneshot},
};

#[tokio::test]
async fn cancelled_waiter_keeps_capacity_until_running_extraction_exits() {
    let semaphore = Box::leak(Box::new(Semaphore::new(1)));
    let permit = semaphore.acquire().await.unwrap();
    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = oneshot::channel();
    let extraction = spawn_extraction(permit, move || {
        started_tx.send(()).unwrap();
        let _ = release_rx.recv();
        let _ = finished_tx.send(());
    });
    let waiter = tokio::spawn(extraction);
    started_rx.await.unwrap();

    waiter.abort();
    let error = waiter.await.unwrap_err();
    let available_while_running = semaphore.available_permits();
    release_tx.send(()).unwrap();
    finished_rx.await.unwrap();
    let _returned_permit = semaphore.acquire().await.unwrap();

    assert!(error.is_cancelled(), "waiter result: {error:?}");
    assert_eq!(available_while_running, 0);
}

#[test]
fn cancelled_waiter_keeps_capacity_for_queued_extraction() {
    let runtime = Builder::new_current_thread().max_blocking_threads(1).build().unwrap();
    runtime.block_on(async {
        let semaphore = Box::leak(Box::new(Semaphore::new(1)));
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            started_tx.send(()).unwrap();
            let _ = release_rx.recv();
        });
        started_rx.await.unwrap();

        let permit = semaphore.acquire().await.unwrap();
        let (finished_tx, finished_rx) = oneshot::channel();
        let extraction = spawn_extraction(permit, move || finished_tx.send(42).unwrap());
        let waiter = tokio::spawn(extraction);
        waiter.abort();
        let error = waiter.await.unwrap_err();
        let available_while_queued = semaphore.available_permits();
        release_tx.send(()).unwrap();
        blocker.await.unwrap();
        assert_eq!(finished_rx.await.unwrap(), 42);
        let _returned_permit = semaphore.acquire().await.unwrap();

        assert!(error.is_cancelled(), "waiter result: {error:?}");
        assert_eq!(available_while_queued, 0);
    });
}

#[tokio::test]
async fn extraction_returns_result_and_releases_capacity_on_error() {
    let semaphore = Box::leak(Box::new(Semaphore::new(1)));
    let permit = semaphore.acquire().await.unwrap();
    let result = spawn_extraction(permit, || Err::<(), _>("invalid archive")).await.unwrap();

    assert_eq!(result, Err("invalid archive"));
    assert_eq!(semaphore.available_permits(), 1);
}

#[tokio::test]
async fn extraction_releases_capacity_on_panic() {
    let semaphore = Box::leak(Box::new(Semaphore::new(1)));
    let permit = semaphore.acquire().await.unwrap();
    let error = spawn_extraction(permit, || panic!("extraction failed")).await.unwrap_err();

    assert!(error.is_panic(), "extraction result: {error:?}");
    assert_eq!(semaphore.available_permits(), 1);
}
