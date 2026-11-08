use super::{SPAWNING, spawning, watchdog_spawning};
use tokio::sync::{Mutex, MutexGuard};

static TEST_LOCK: Mutex<()> = Mutex::const_new(());

pub(crate) fn test_lock() -> MutexGuard<'static, ()> {
    TEST_LOCK.blocking_lock()
}

#[cfg(unix)]
pub(crate) async fn test_lock_async() -> MutexGuard<'static, ()> {
    TEST_LOCK.lock().await
}

#[test]
fn watchdog_spawn_waits_for_child_spawn() {
    let _test_lock = test_lock();
    let child_spawn = spawning();
    assert!(SPAWNING.try_write().is_err());
    drop(child_spawn);
    assert!(SPAWNING.try_write().is_ok());
}

#[test]
fn child_spawns_can_run_together() {
    let _test_lock = test_lock();
    let first_spawn = spawning();
    let second_spawn = spawning();
    assert!(SPAWNING.try_read().is_ok());
    drop((first_spawn, second_spawn));
}

#[test]
fn child_spawn_waits_for_watchdog_spawn() {
    let _test_lock = test_lock();
    let _watchdog_spawn = watchdog_spawning();
    assert!(SPAWNING.try_read().is_err());
}
