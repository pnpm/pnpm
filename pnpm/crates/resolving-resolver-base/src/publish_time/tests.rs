use super::parse_packument_timestamp;
use chrono::{TimeZone, Utc};

#[test]
fn parses_full_rfc3339_with_and_without_fraction() {
    assert_eq!(
        parse_packument_timestamp("2024-03-15T09:42:13.123Z"),
        Some(
            Utc.with_ymd_and_hms(2024, 3, 15, 9, 42, 13).unwrap()
                + chrono::Duration::milliseconds(123)
        ),
    );
    assert_eq!(
        parse_packument_timestamp("2024-03-15T09:42:13Z"),
        Some(Utc.with_ymd_and_hms(2024, 3, 15, 9, 42, 13).unwrap()),
    );
}

#[test]
fn parses_minute_precision_without_seconds() {
    assert_eq!(
        parse_packument_timestamp("2024-03-15T09:42Z"),
        Some(Utc.with_ymd_and_hms(2024, 3, 15, 9, 42, 0).unwrap()),
    );
}

#[test]
fn parses_bare_date_as_midnight_utc() {
    assert_eq!(
        parse_packument_timestamp("2024-03-15"),
        Some(Utc.with_ymd_and_hms(2024, 3, 15, 0, 0, 0).unwrap()),
    );
}

#[test]
fn rejects_unrecognized_forms() {
    // No zone on a time-bearing string is ambiguous (local vs UTC).
    assert_eq!(parse_packument_timestamp("2024-03-15T09:42"), None);
    assert_eq!(parse_packument_timestamp("2024-13-99"), None);
    assert_eq!(parse_packument_timestamp("garbage"), None);
    assert_eq!(parse_packument_timestamp(""), None);
}
