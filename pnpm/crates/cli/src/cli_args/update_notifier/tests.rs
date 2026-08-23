use super::{checked_recently, read_state, to_utc_string, write_state};
use chrono::{TimeZone, Utc};
use serde_json::{Map, Value, json};
use tempfile::tempdir;

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
    let state_file = dir.path().join("nested").join("pnpm-state.json");
    let now = Utc.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();

    write_state(&state_file, Map::new(), now);

    assert!(checked_recently(&read_state(&state_file), now));
}
