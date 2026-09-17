use super::{
    HELD_CONCURRENCY_GROUPS_ENV, SlotOutcome, SlotPool, acquire_slot, add_held_group,
    with_held_group,
};
use pnpm_config::{Config, TaskSettings};
use pnpm_reporter::LogEvent;
use std::{
    collections::HashMap,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

fn no_emit(_: &LogEvent) {}

fn never() -> bool {
    false
}

fn acquire(config: &Config, script: &str) -> SlotOutcome {
    acquire_slot(config, script, no_emit, &never, None).expect("acquire")
}

/// A config whose `build` task is in group `cargo`, limited to `limit`
/// slots under `state_dir`.
fn config(state_dir: &std::path::Path, limit: Option<u32>) -> Config {
    let mut build = TaskSettings::default();
    build.concurrency_group = Some("cargo".to_string());
    Config {
        tasks: std::iter::once(("build".to_string(), build)).collect(),
        concurrency_groups: limit
            .map(|limit| ("cargo".to_string(), limit))
            .into_iter()
            .collect(),
        state_dir: state_dir.to_path_buf(),
        ..Config::default()
    }
}

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
    assert_eq!(pool.holders().len(), 1, "a freed slot's stale stamp is not listed");
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
        .acquire(|| notices += 1, &never)
        .expect("acquire after the release")
        .expect("the wait ended with a slot");
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
fn a_cancelled_wait_ends_without_a_slot() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let pool = SlotPool { dir: dir.path().to_path_buf(), limit: 1 };
    let held = pool
        .try_acquire()
        .expect("try first")
        .expect("the slot is free");

    let started = Instant::now();
    let cancel_after = Duration::from_millis(200);
    let cancelled = || started.elapsed() >= cancel_after;
    let outcome = pool
        .acquire(|| {}, &cancelled)
        .expect("the wait itself succeeds");

    dbg!(started.elapsed());
    assert!(outcome.is_none(), "a cancelled wait hands out no slot");
    assert!(started.elapsed() >= cancel_after);
    drop(held);
}

#[test]
fn a_task_without_a_limited_group_takes_no_slot() {
    let dir = tempfile::tempdir().expect("create temp dir");
    for limit in [None, Some(0)] {
        let config = config(dir.path(), limit);
        assert!(matches!(acquire(&config, "build"), SlotOutcome::Ungated));
    }
    let config = config(dir.path(), Some(1));
    assert!(matches!(acquire(&config, "lint"), SlotOutcome::Ungated));
    assert!(!dir.path().join("run-slots").exists(), "no pool is created without a limit");
}

#[test]
fn a_limited_group_creates_its_pool_under_the_state_dir() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = config(dir.path(), Some(1));
    let SlotOutcome::Held(slot) = acquire(&config, "build") else {
        panic!("a limited group hands out a slot");
    };
    let pool = dir
        .path()
        .join("run-slots")
        .join("cargo");
    assert_eq!(slot.group(), "cargo");
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

#[test]
fn the_held_group_is_added_to_the_spawned_environment() {
    let extra_env: HashMap<String, String> =
        std::iter::once(("NODE_OPTIONS".to_string(), "--flag".to_string())).collect();
    let env = with_held_group(&extra_env, "cargo");
    assert_eq!(env.get(HELD_CONCURRENCY_GROUPS_ENV).map(String::as_str), Some("cargo"));
    assert_eq!(env.get("NODE_OPTIONS").map(String::as_str), Some("--flag"));
}

#[test]
fn a_nested_task_of_a_held_group_reuses_the_parent_slot() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = config(dir.path(), Some(1));
    let inherited = Some("node,cargo");

    let held_group = acquire_slot(&config, "build", no_emit, &never, inherited).expect("acquire");
    let spawned = add_held_group(&HashMap::new(), inherited, "cargo");

    assert!(matches!(held_group, SlotOutcome::Ungated), "the parent's slot covers this task");
    assert_eq!(
        spawned.get(HELD_CONCURRENCY_GROUPS_ENV).map(String::as_str),
        Some("node,cargo"),
        "a held group is listed once",
    );
}
