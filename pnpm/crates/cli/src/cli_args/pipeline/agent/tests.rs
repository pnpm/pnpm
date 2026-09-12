use super::lock_agent;

#[test]
fn a_second_watcher_cannot_use_a_running_watchers_checkout() {
    let directory = tempfile::tempdir().unwrap();
    let first = lock_agent(directory.path()).unwrap();
    assert!(
        lock_agent(directory.path()).is_err(),
        "a second watcher must not acquire the checkout",
    );
    drop(first);
    assert!(
        lock_agent(directory.path()).is_ok(),
        "the checkout must be released when the watcher stops",
    );
}
