use super::parse_packument_timestamp;

#[test]
fn parses_full_rfc3339_with_and_without_fraction() {
    assert_eq!(
        parse_packument_timestamp("2024-03-15T09:42:13.123Z"),
        Some(
            "2024-03-15T09:42:13Z".parse::<jiff::Timestamp>().unwrap()
                + jiff::SignedDuration::from_millis(123)
        ),
    );
    assert_eq!(
        parse_packument_timestamp("2024-03-15T09:42:13Z"),
        Some("2024-03-15T09:42:13Z".parse::<jiff::Timestamp>().unwrap()),
    );
}

#[test]
fn parses_minute_precision_without_seconds() {
    assert_eq!(
        parse_packument_timestamp("2024-03-15T09:42Z"),
        Some("2024-03-15T09:42:00Z".parse::<jiff::Timestamp>().unwrap()),
    );
}

#[test]
fn parses_bare_date_as_midnight_utc() {
    assert_eq!(
        parse_packument_timestamp("2024-03-15"),
        Some("2024-03-15T00:00:00Z".parse::<jiff::Timestamp>().unwrap()),
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
