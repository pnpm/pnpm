use super::{MACHINE_RUN_SLOT_ENV, SlotPool, acquire_machine_run_slot};
use pnpm_config::Config;
use pnpm_reporter::LogEvent;
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

fn no_emit(_: &LogEvent) {}

#[test]
fn a_pool_hands_out_exactly_its_limit() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let pool = SlotPool { dir: dir.path().to_path_buf(), limit: 2 };

    let first = pool
        .try_acquire()
        .expect("try first")
        .expect("first slot is free");
    let second = pool
        .try_acquire()
        .expect("try second")
        .expect("second slot is free");
    assert!(
        pool.try_acquire()
            .expect("try third")
            .is_none(),
        "both slots are held",
    );

    let holders = pool.holders();
    dbg!(&holders);
    assert_eq!(holders.len(), 2);
    assert!(
        holders
            .iter()
            .all(|holder| holder.starts_with("pid ")),
    );

    drop(first);
    let third = pool
        .try_acquire()
        .expect("try again")
        .expect("a slot is free again");
    drop(second);
    drop(third);
}

#[test]
fn acquire_waits_for_a_slot_to_free_up() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let pool = SlotPool { dir: dir.path().to_path_buf(), limit: 1 };
    let held = pool
        .try_acquire()
        .expect("try first")
        .expect("the slot is free");

    let (released, on_release) = mpsc::channel();
    let hold_for = Duration::from_millis(300);
    let holder = thread::spawn(move || {
        thread::sleep(hold_for);
        drop(held);
        released
            .send(Instant::now())
            .expect("report the release");
    });

    let mut notices = 0;
    let started = Instant::now();
    let slot = pool
        .acquire(|| notices += 1)
        .expect("acquire after the release");
    let acquired_at = Instant::now();
    holder.join().expect("holder thread");
    let released_at = on_release.recv().expect("release time");

    dbg!(started.elapsed(), notices);
    assert!(acquired_at >= released_at, "the slot was handed out while still held");
    assert!(started.elapsed() >= hold_for);
    assert_eq!(notices, 1, "the wait is announced once");
    drop(slot);
}

#[test]
fn no_limit_takes_no_slot() {
    let dir = tempfile::tempdir().expect("create temp dir");
    for limit in [None, Some(0)] {
        let config = Config {
            machine_run_concurrency: limit,
            state_dir: dir.path().to_path_buf(),
            ..Config::default()
        };
        assert!(acquire_machine_run_slot(&config, no_emit).expect("acquire").is_none());
    }
    assert!(!dir.path().join("run-slots").exists(), "no pool is created without a limit");
}

#[test]
fn a_limit_creates_the_group_pool_under_the_state_dir() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = Config {
        machine_run_concurrency: Some(1),
        machine_run_concurrency_group: "agents".to_string(),
        state_dir: dir.path().to_path_buf(),
        ..Config::default()
    };
    let slot = acquire_machine_run_slot(&config, no_emit).expect("acquire").expect("a slot");
    let pool = dir
        .path()
        .join("run-slots")
        .join("agents");
    assert!(pool.join("0").is_file());
    assert!(
        SlotPool { dir: pool, limit: 1 }
            .try_acquire()
            .expect("try")
            .is_none(),
        "the slot is held through the config path",
    );
    drop(slot);
}

/// `MACHINE_RUN_SLOT_ENV` is process-global, so this is the one test that
/// sets it, and it restores the previous value before returning.
#[test]
fn a_nested_invocation_of_the_same_group_reuses_the_parent_slot() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = Config {
        machine_run_concurrency: Some(1),
        machine_run_concurrency_group: "agents".to_string(),
        state_dir: dir.path().to_path_buf(),
        ..Config::default()
    };
    let previous = std::env::var_os(MACHINE_RUN_SLOT_ENV);
    // SAFETY: the tests of this crate that touch the environment run
    // through this one function, and nothing else in the process reads
    // `MACHINE_RUN_SLOT_ENV` while it runs.
    unsafe { std::env::set_var(MACHINE_RUN_SLOT_ENV, "agents") };
    let same_group = acquire_machine_run_slot(&config, no_emit);
    let other_group = acquire_machine_run_slot(
        &Config { machine_run_concurrency_group: "other".to_string(), ..config },
        no_emit,
    );
    // SAFETY: see above.
    unsafe {
        match previous {
            Some(previous) => std::env::set_var(MACHINE_RUN_SLOT_ENV, previous),
            None => std::env::remove_var(MACHINE_RUN_SLOT_ENV),
        }
    }

    assert!(same_group.expect("acquire").is_none(), "the parent's slot covers this run");
    assert!(other_group.expect("acquire").is_some(), "another group has its own pool");
}
