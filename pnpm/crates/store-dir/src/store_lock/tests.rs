use super::StoreDir;
use std::{sync::mpsc, thread, time::Duration};
use tempfile::tempdir;

#[test]
fn prune_waits_for_store_consumers() {
    let root = tempdir().unwrap();
    let store = StoreDir::new(root.path());
    let consumer = store.lock_for_use().unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (acquired_tx, acquired_rx) = mpsc::channel();

    let waiter = thread::spawn(move || {
        started_tx.send(()).unwrap();
        let lock = store.lock_for_prune().unwrap();
        acquired_tx.send(()).unwrap();
        lock
    });
    started_rx.recv().unwrap();
    assert!(
        acquired_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
    );

    drop(consumer);
    acquired_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    drop(waiter.join().unwrap());
}

#[test]
fn store_consumers_may_overlap() {
    let root = tempdir().unwrap();
    let store = StoreDir::new(root.path());
    let first = store.lock_for_use().unwrap();
    let second = store.lock_for_use().unwrap();
    drop((first, second));
}

#[test]
fn prune_waits_for_frozen_store_consumers() {
    let root = tempdir().unwrap();
    let store = StoreDir::new(root.path());
    let consumer = store.lock_for_frozen_use().unwrap();
    let (acquired_tx, acquired_rx) = mpsc::channel();

    let waiter = thread::spawn(move || {
        let lock = store.lock_for_prune().unwrap();
        acquired_tx.send(()).unwrap();
        lock
    });
    assert!(
        acquired_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
    );

    drop(consumer);
    acquired_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    drop(waiter.join().unwrap());
}
