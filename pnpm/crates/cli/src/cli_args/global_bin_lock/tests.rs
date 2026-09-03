use super::{acquire_global_bin_lock, try_acquire_global_bin_lock};
use std::{sync::mpsc, thread, time::Duration};

#[test]
fn serializes_global_bin_writers() {
    let global_bin_dir = tempfile::tempdir().expect("create global bin directory");
    let held = acquire_global_bin_lock(global_bin_dir.path()).expect("take first lock");
    let path = global_bin_dir.path().to_path_buf();
    let (attempt_sender, attempt_receiver) = mpsc::channel();
    let (sender, receiver) = mpsc::channel();
    let waiter = thread::spawn(move || {
        attempt_sender.send(()).expect("report lock attempt");
        let lock = acquire_global_bin_lock(&path).expect("take second lock");
        sender.send(lock).expect("report second lock");
    });

    attempt_receiver.recv_timeout(Duration::from_secs(2)).expect("second lock attempted");
    assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());
    drop(held);
    let successor = receiver.recv_timeout(Duration::from_secs(2)).expect("second lock proceeds");
    drop(successor);
    waiter.join().expect("join lock waiter");
}

#[test]
fn try_lock_does_not_wait_for_a_global_bin_writer() {
    let global_bin_dir = tempfile::tempdir().expect("create global bin directory");
    let held = acquire_global_bin_lock(global_bin_dir.path()).expect("take first lock");

    assert!(try_acquire_global_bin_lock(global_bin_dir.path()).expect("try second lock").is_none());

    drop(held);
}
