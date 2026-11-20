use super::{
    duration_minutes,
    format_millis,
    format_rfc3339,
    format_seconds,
    parse_packument_timestamp,
    parse_rfc3339,
};

#[test]
fn serialized_precision_and_utc_offset_are_stable() {
    for (input, expected) in [
        ("2024-01-01T00:00:00Z", "2024-01-01T00:00:00+00:00"),
        ("2024-01-01T00:00:00.1Z", "2024-01-01T00:00:00.100+00:00"),
        ("2024-01-01T00:00:00.0001Z", "2024-01-01T00:00:00.000100+00:00"),
        ("2024-01-01T00:00:00.0000001Z", "2024-01-01T00:00:00.000000100+00:00"),
        ("2024-01-01T01:00:00+01:00", "2024-01-01T00:00:00+00:00"),
        ("1969-12-31T23:59:59.999999999Z", "1969-12-31T23:59:59.999999999+00:00"),
    ] {
        assert_eq!(format_rfc3339(parse_rfc3339(input).unwrap()), expected, "{input}");
    }
}

#[test]
fn fixed_precision_truncates_and_pads() {
    for (input, millis, seconds) in [
        ("2024-01-01T00:00:00Z", "2024-01-01T00:00:00.000Z", "2024-01-01T00:00:00Z"),
        ("2024-01-01T00:00:00.999999999Z", "2024-01-01T00:00:00.999Z", "2024-01-01T00:00:00Z"),
        ("1969-12-31T23:59:59.999999999Z", "1969-12-31T23:59:59.999Z", "1969-12-31T23:59:59Z"),
    ] {
        let timestamp = parse_rfc3339(input).unwrap();
        assert_eq!(format_millis(timestamp), millis);
        assert_eq!(format_seconds(timestamp), seconds);
    }
}

#[test]
fn strict_timestamps_require_seconds_and_an_offset() {
    for input in [
        "2024-01-01",
        "2024-01-01T00:00Z",
        "2024-01-01T00:00:00",
        "2024-01-01T00:00:00Z[UTC]",
        "2024-01-01T00:00:00+0000",
        "2024-02-30T00:00:00Z",
        "2024-01-01T00:00:éZ",
        "",
    ] {
        assert!(parse_rfc3339(input).is_err(), "{input}");
    }
}

#[test]
fn packument_timestamps_accept_only_documented_coarsened_forms() {
    for (input, expected) in [
        ("2024-02-29", "2024-02-29T00:00:00Z"),
        ("2024-02-29T10:15Z", "2024-02-29T10:15:00Z"),
        ("2024-02-29T10:15:30.123+01:00", "2024-02-29T09:15:30.123Z"),
    ] {
        assert_eq!(parse_packument_timestamp(input), Some(expected.parse().unwrap()));
    }
    for input in ["2024-02-29T10:15", "2024-02-29T10:15:30", "2024-02-30", "2024-02-29T10Z"] {
        assert_eq!(parse_packument_timestamp(input), None, "{input}");
    }
}

#[test]
fn release_age_duration_rejects_overflow() {
    assert_eq!(duration_minutes(60).unwrap().as_secs(), 3600);
    assert_eq!(duration_minutes(i64::MAX), None);
}

#[test]
fn long_fractions_keep_nanosecond_precision() {
    assert_eq!(
        parse_rfc3339("2024-01-01T00:00:00.123456789123Z").unwrap(),
        parse_rfc3339("2024-01-01T00:00:00.123456789Z").unwrap(),
    );
}

#[test]
fn leap_seconds_do_not_move_a_release_earlier() {
    assert_eq!(
        parse_rfc3339("2016-12-31T23:59:60.123Z").unwrap(),
        parse_rfc3339("2017-01-01T00:00:00.123Z").unwrap(),
    );
}

#[test]
fn unix_milliseconds_round_down_before_the_epoch() {
    assert_eq!(super::unix_millis(parse_rfc3339("1969-12-31T23:59:59.999999999Z").unwrap()), -1);
    assert_eq!(super::unix_millis(parse_rfc3339("1970-01-01T00:00:00.000000001Z").unwrap()), 0);
}

#[test]
fn scalar_timestamps_cover_the_entire_rfc3339_year_range() {
    for (input, expected) in [
        ("9999-12-31T23:59:59.999Z", 253_402_300_799_999),
        ("9999-12-31T22:59:59.999-01:00", 253_402_300_799_999),
        ("1969-12-31T23:59:59.999999999Z", -1),
        ("0000-01-01T00:00:00Z", -62_167_219_200_000),
    ] {
        assert_eq!(super::parse_rfc3339_millis(input), Ok(expected), "{input}");
    }
}
