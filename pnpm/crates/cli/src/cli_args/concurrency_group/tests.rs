use super::{
    HELD_CONCURRENCY_GROUPS_ENV, SlotOutcome, acquire_slot, add_held_group,
    pool::{SlotPool, WaitSnapshot},
    with_held_group,
};
use pnpm_config::{Config, TaskSettings};
use pnpm_reporter::LogEvent;
use std::{
    collections::HashMap,
    fs,
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
        .acquire("", 0, |_| notices += 1, &never)
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
        .acquire("", 0, |_| {}, &cancelled)
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

fn recv_wait(rx: &mpsc::Receiver<WaitSnapshot>) -> WaitSnapshot {
    rx.recv_timeout(Duration::from_secs(5))
        .expect("the waiter announced itself")
}

fn recv_got(rx: &mpsc::Receiver<Instant>) -> Instant {
    rx.recv_timeout(Duration::from_secs(5))
        .expect("the waiter took a slot")
}

fn spawn_waiter(
    pool: SlotPool,
    priority: i32,
) -> (mpsc::Receiver<WaitSnapshot>, mpsc::Receiver<Instant>, thread::JoinHandle<()>) {
    let (waiting_tx, waiting) = mpsc::channel();
    let (got_tx, got) = mpsc::channel();
    let thread = thread::spawn(move || {
        let slot = pool
            .acquire(
                "wait",
                priority,
                |snapshot| {
                    let _ = waiting_tx.send(snapshot.clone());
                },
                &never,
            )
            .expect("acquire")
            .expect("a slot");
        got_tx
            .send(Instant::now())
            .expect("report the grant");
        drop(slot);
    });
    (waiting, got, thread)
}

#[test]
fn waiters_start_in_the_order_they_began_waiting() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let pool = SlotPool { dir: dir.path().to_path_buf(), limit: 1 };
    let held = pool
        .try_acquire()
        .expect("try first")
        .expect("the slot is free");

    let (a_waiting, a_got, a) = spawn_waiter(pool.clone(), 0);
    let a_line = recv_wait(&a_waiting);
    let (b_waiting, b_got, b) = spawn_waiter(pool, 0);
    let b_line = recv_wait(&b_waiting);

    dbg!(&a_line, &b_line);
    assert_eq!(a_line.position, 1);
    assert_eq!(b_line.position, 2);
    assert_eq!(b_line.total, 2);

    drop(held);
    let t_a = recv_got(&a_got);
    let t_b = recv_got(&b_got);
    a.join().expect("first waiter");
    b.join().expect("second waiter");
    assert!(t_a <= t_b, "the first waiter started after the second");
}

#[test]
fn a_higher_priority_waiter_starts_before_earlier_arrivals() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let pool = SlotPool { dir: dir.path().to_path_buf(), limit: 1 };
    let held = pool
        .try_acquire()
        .expect("try first")
        .expect("the slot is free");

    let (low_waiting, low_got, low) = spawn_waiter(pool.clone(), 0);
    let low_line = recv_wait(&low_waiting);
    let (high_waiting, high_got, high) = spawn_waiter(pool, 10);
    let high_line = recv_wait(&high_waiting);

    dbg!(&low_line, &high_line);
    assert_eq!(high_line.position, 1, "the later high-priority waiter is not first in line");
    assert_eq!(low_line.position, 1, "the first waiter was not first before the jump");
    assert_eq!(high_line.total, 2);

    drop(held);
    let t_high = recv_got(&high_got);
    let t_low = recv_got(&low_got);
    high.join().expect("high-priority waiter");
    low.join().expect("low-priority waiter");
    assert!(t_high <= t_low, "the high-priority waiter started after the earlier arrival");
}

#[test]
fn a_cancelled_waiter_does_not_block_the_next() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let pool = SlotPool { dir: dir.path().to_path_buf(), limit: 1 };
    let held = pool
        .try_acquire()
        .expect("try first")
        .expect("the slot is free");

    let started = Instant::now();
    let cancel_after = Duration::from_millis(200);
    let cancelled = || started.elapsed() >= cancel_after;
    let first = pool
        .acquire("", 0, |_| {}, &cancelled)
        .expect("the cancelled wait itself succeeds");
    assert!(first.is_none(), "the cancelled waiter took a slot");

    let (waiting, got, next) = spawn_waiter(pool, 0);
    let line = recv_wait(&waiting);
    dbg!(&line);
    assert_eq!(line.position, 1, "the cancelled waiter stayed in line");

    drop(held);
    recv_got(&got);
    next.join().expect("next waiter");
}

#[test]
fn a_stale_waiter_file_is_skipped() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let pool = SlotPool { dir: dir.path().to_path_buf(), limit: 1 };
    let waiters = dir.path().join("waiters");
    fs::create_dir_all(&waiters).expect("create waiters dir");
    fs::write(waiters.join("0"), "").expect("stale lock");
    fs::write(waiters.join("0.stamp"), "priority 0\npid 1 in /stale").expect("stale stamp");
    fs::write(dir.path().join("seq"), "1").expect("advance ticket sequence");

    let slot = pool
        .acquire("", 0, |_| {}, &never)
        .expect("acquire")
        .expect("a slot");
    assert!(!waiters.join("0").exists(), "the stale lock stayed");
    assert!(!waiters.join("0.stamp").exists(), "the stale stamp stayed");
    drop(slot);
}

#[test]
fn a_later_waiter_takes_a_slot_the_head_cannot_reach() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().to_path_buf();
    let narrow = SlotPool { dir: path.clone(), limit: 1 };
    let wide = SlotPool { dir: path, limit: 2 };
    let held = wide
        .try_acquire()
        .expect("try first")
        .expect("slot 0 is free");

    let (narrow_waiting, narrow_got, narrow_thread) = spawn_waiter(narrow, 0);
    let narrow_line = recv_wait(&narrow_waiting);
    dbg!(&narrow_line);
    assert_eq!(narrow_line.position, 1);

    let (_wide_waiting, wide_got, wide_thread) = spawn_waiter(wide, 0);
    let t_wide = recv_got(&wide_got);
    wide_thread.join().expect("wider waiter");
    assert!(
        matches!(narrow_got.try_recv(), Err(mpsc::TryRecvError::Empty)),
        "the narrower waiter took a slot it cannot reach",
    );

    drop(held);
    let t_narrow = recv_got(&narrow_got);
    narrow_thread.join().expect("narrower waiter");
    dbg!(t_wide, t_narrow);
    assert!(t_wide <= t_narrow, "the wider waiter started after the narrower head");
}

#[test]
fn an_empty_seq_does_not_reuse_a_live_ticket() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let pool = SlotPool { dir: dir.path().to_path_buf(), limit: 1 };
    let held = pool
        .try_acquire()
        .expect("try first")
        .expect("the slot is free");

    let (first_waiting, first_got, first) = spawn_waiter(pool.clone(), 0);
    let first_line = recv_wait(&first_waiting);
    fs::write(dir.path().join("seq"), "").expect("empty the ticket counter");

    let (second_waiting, second_got, second) = spawn_waiter(pool, 0);
    let second_line = recv_wait(&second_waiting);
    dbg!(&first_line, &second_line);
    assert_eq!(second_line.position, 2, "the new waiter reused the live ticket");

    drop(held);
    recv_got(&first_got);
    recv_got(&second_got);
    first.join().expect("first waiter");
    second.join().expect("second waiter");
}

#[test]
fn status_lists_holders_and_waiters_in_line_order() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let pool = SlotPool { dir: dir.path().to_path_buf(), limit: 1 };
    let held = pool
        .try_acquire()
        .expect("try first")
        .expect("the slot is free");

    let (waiting, got, thread) = spawn_waiter(pool.clone(), 3);
    recv_wait(&waiting);
    let status = pool.status().expect("status");
    dbg!(&status);
    assert_eq!(status.holders.len(), 1);
    dbg!(&status.holders[0].info);
    assert!(status.holders[0].info.contains("pid "));
    assert!(status.holders[0].elapsed.is_some());
    assert_eq!(status.waiters.len(), 1);
    assert_eq!(status.waiters[0].priority, 3);
    assert_eq!(status.waiters[0].command.as_deref(), Some("wait"));
    assert!(status.waiters[0].elapsed.is_some());

    drop(held);
    recv_got(&got);
    thread.join().expect("waiter");
}

#[test]
fn status_lists_a_locked_slot_without_a_holder_stamp() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let pool = SlotPool { dir: dir.path().to_path_buf(), limit: 1 };
    let held = pool
        .try_acquire()
        .expect("try first")
        .expect("the slot is free");
    fs::remove_file(dir.path().join("0.holder")).expect("remove stamp");

    let status = pool.status().expect("status");
    dbg!(&status);
    assert_eq!(status.holders.len(), 1);
    assert_eq!(status.holders[0].info, "slot 0");
    assert!(status.holders[0].elapsed.is_none());
    drop(held);
}

#[test]
fn format_elapsed_prints_compact_units() {
    use super::stamp::format_elapsed;
    assert_eq!(format_elapsed(Duration::from_secs(0)), "0s");
    assert_eq!(format_elapsed(Duration::from_secs(59)), "59s");
    assert_eq!(format_elapsed(Duration::from_mins(1)), "1m");
    assert_eq!(format_elapsed(Duration::from_mins(1) + Duration::from_secs(15)), "1m 15s");
    assert_eq!(format_elapsed(Duration::from_hours(1)), "1h");
    assert_eq!(format_elapsed(Duration::from_mins(61)), "1h 1m");
}

#[test]
fn parse_process_stamp_reads_since_and_old_stamps() {
    use super::stamp::parse_process_stamp;
    let stamped = parse_process_stamp("since 10\ncmd hold\npid 1 in /tmp");
    assert_eq!(stamped.since, Some(10));
    assert_eq!(stamped.command.as_deref(), Some("hold"));
    assert_eq!(stamped.info, "pid 1 in /tmp");
    let old = parse_process_stamp("pid 1 in /tmp");
    assert_eq!(old.since, None);
    assert_eq!(old.command, None);
    assert_eq!(old.info, "pid 1 in /tmp");
}
