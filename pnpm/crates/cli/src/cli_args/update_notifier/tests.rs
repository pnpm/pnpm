use super::{
    UPDATE_CHECK_GRACE_PERIOD, checked_recently, read_state, settle, to_utc_string, write_state,
};
use chrono::{TimeZone, Utc};
use serde_json::{Map, Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use tempfile::tempdir;

/// Flips a flag when dropped, so a test can tell an aborted task (its
/// future is dropped) from one merely detached.
struct FlagOnDrop(Arc<AtomicBool>);

impl Drop for FlagOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// A check still waiting on the registry gets the grace period, not the
/// registry client's timeouts, and is aborted rather than left behind.
#[tokio::test]
async fn a_check_still_running_is_abandoned_after_the_grace_period() {
    let aborted = Arc::new(AtomicBool::new(false));
    let flag = FlagOnDrop(Arc::clone(&aborted));
    let pending = tokio::spawn(async move {
        let _flag = flag;
        std::future::pending::<()>().await;
    });

    let started = Instant::now();
    settle(Some(pending), &Ok::<(), miette::Report>(())).await;
    let waited = started.elapsed();
    assert!(
        waited >= UPDATE_CHECK_GRACE_PERIOD,
        "the check gets the whole grace period, waited {waited:?}",
    );

    // Cancellation lands when the runtime next polls the task.
    for _ in 0..100 {
        if aborted.load(Ordering::SeqCst) {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(aborted.load(Ordering::SeqCst), "the abandoned check must be aborted");
}

/// A check the registry answered in time is waited for, so its notice
/// reaches the terminal before the process exits.
#[tokio::test]
async fn a_check_that_finishes_in_time_is_waited_for() {
    let finished = Arc::new(AtomicBool::new(false));
    let pending = tokio::spawn({
        let finished = Arc::clone(&finished);
        async move {
            tokio::task::yield_now().await;
            finished.store(true, Ordering::SeqCst);
        }
    });

    settle(Some(pending), &Ok::<(), miette::Report>(())).await;

    assert!(finished.load(Ordering::SeqCst), "the check must have run to completion");
}

/// A failed command does not wait at all: the error is what the user
/// needs to read.
#[tokio::test]
async fn a_failed_command_drops_the_check_at_once() {
    let aborted = Arc::new(AtomicBool::new(false));
    let flag = FlagOnDrop(Arc::clone(&aborted));
    let pending = tokio::spawn(async move {
        let _flag = flag;
        std::future::pending::<()>().await;
    });

    let started = Instant::now();
    settle(Some(pending), &Err::<(), miette::Report>(miette::miette!("the install failed"))).await;
    assert!(
        started.elapsed() < UPDATE_CHECK_GRACE_PERIOD,
        "a failed command must not wait out the grace period",
    );

    for _ in 0..100 {
        if aborted.load(Ordering::SeqCst) {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(aborted.load(Ordering::SeqCst), "the check must be aborted");
}

fn state_with_last_check(value: &Value) -> Map<String, Value> {
    let Value::Object(fields) = json!({ "lastUpdateCheck": value }) else {
        unreachable!("the literal above is an object")
    };
    fields
}

#[test]
fn to_utc_string_matches_the_javascript_rendering() {
    let time = Utc.with_ymd_and_hms(2026, 8, 3, 4, 5, 6).unwrap();
    assert_eq!(to_utc_string(time), "Mon, 03 Aug 2026 04:05:06 GMT");
}

#[test]
fn a_check_from_this_hour_suppresses_the_next_one() {
    let now = Utc.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let state = state_with_last_check(&json!(to_utc_string(now - chrono::Duration::hours(1))));
    assert!(checked_recently(&state, now));
}

#[test]
fn a_check_older_than_a_day_is_due_again() {
    let now = Utc.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let state = state_with_last_check(&json!(to_utc_string(now - chrono::Duration::hours(25))));
    assert!(!checked_recently(&state, now));
}

#[test]
fn an_unusable_timestamp_reads_as_never_checked() {
    let now = Utc.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    assert!(!checked_recently(&Map::new(), now));
    assert!(!checked_recently(&state_with_last_check(&json!("not a date")), now));
    assert!(!checked_recently(&state_with_last_check(&json!(42)), now));
}

/// A clock that moved backwards, or a hand-edited state file, must not
/// silence the check until the recorded time comes around.
#[test]
fn a_timestamp_in_the_future_reads_as_never_checked() {
    let now = Utc.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let state = state_with_last_check(&json!(to_utc_string(now + chrono::Duration::days(365))));
    assert!(!checked_recently(&state, now));
}

#[test]
fn recording_a_check_keeps_the_other_state_keys() {
    let dir = tempdir().unwrap();
    let state_file = dir.path().join("pnpm-state.json");
    std::fs::write(&state_file, r#"{"lastUpdateCheck":"Sun, 01 Feb 2026 00:00:00 GMT","x":1}"#)
        .unwrap();

    let now = Utc.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let state = read_state(&state_file);
    assert!(!checked_recently(&state, now));
    write_state(&state_file, state, now);

    let written = read_state(&state_file);
    assert_eq!(written.get("x"), Some(&json!(1)));
    assert!(checked_recently(&written, now));
}

#[test]
fn a_missing_or_corrupt_state_file_reads_as_empty() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("pnpm-state.json");
    assert!(read_state(&missing).is_empty());

    let corrupt = dir.path().join("corrupt.json");
    std::fs::write(&corrupt, "[]").unwrap();
    assert!(read_state(&corrupt).is_empty());
}

/// pnpm writes this file through `write-file-atomic`, which replaces a
/// symlinked target rather than following it; pacquet's atomic write must
/// do the same, or a state directory someone else can write to becomes a
/// way to overwrite an arbitrary file.
#[cfg(unix)]
#[test]
fn a_symlinked_state_file_is_replaced_rather_than_followed() {
    let dir = tempdir().unwrap();
    let elsewhere = dir.path().join("elsewhere.json");
    std::fs::write(&elsewhere, "untouched").unwrap();
    let state_file = dir.path().join("pnpm-state.json");
    std::os::unix::fs::symlink(&elsewhere, &state_file).unwrap();

    let now = Utc.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    write_state(&state_file, Map::new(), now);

    assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "untouched");
    assert!(checked_recently(&read_state(&state_file), now));
}

#[test]
fn the_state_directory_is_created_on_demand() {
    let dir = tempdir().unwrap();
    let state_file = dir
        .path()
        .join("nested")
        .join("pnpm-state.json");
    let now = Utc.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();

    write_state(&state_file, Map::new(), now);

    assert!(checked_recently(&read_state(&state_file), now));
}
