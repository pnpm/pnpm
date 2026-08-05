use super::cached_verdict;
use chrono::{DateTime, Utc};
use pretty_assertions::assert_eq;

fn at(rfc3339: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(rfc3339).expect("parse timestamp").with_timezone(&Utc)
}

#[test]
fn dates_a_cached_verdict_relative_to_now() {
    let now = at("2026-07-25T12:00:00.000Z");
    assert_eq!(cached_verdict(Some("2026-07-25T11:59:59.747Z"), now), "verified 253ms ago");
    assert_eq!(cached_verdict(Some("2026-07-25T11:58:00.000Z"), now), "verified 2m ago");
    assert_eq!(cached_verdict(Some("2026-07-23T12:00:00.000Z"), now), "verified 2d ago");
}

/// A cache record from before the timestamp existed, and one whose
/// timestamp is unusable, both fall back to the timeless wording rather
/// than inventing an age.
#[test]
fn falls_back_to_the_timeless_wording_without_a_usable_timestamp() {
    let now = at("2026-07-25T12:00:00.000Z");
    assert_eq!(cached_verdict(None, now), "previously verified");
    assert_eq!(cached_verdict(Some("last tuesday"), now), "previously verified");
}

/// A clock that moved backwards between the verification run and this
/// install must not render a negative age.
#[test]
fn clamps_a_future_timestamp_to_zero() {
    let now = at("2026-07-25T12:00:00.000Z");
    assert_eq!(cached_verdict(Some("2026-07-25T12:00:05.000Z"), now), "verified 0ms ago");
}
