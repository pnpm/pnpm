use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use super::{
    format_bytes, format_field_value, format_time_ago_since, get_nested_property, parse_date,
};

fn utc(rfc3339: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(rfc3339).expect("valid timestamp").with_timezone(&Utc)
}

#[test]
fn format_bytes_uses_decimal_units() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(500), "500 B");
    assert_eq!(format_bytes(1_500), "1.5 kB");
    assert_eq!(format_bytes(1_000_000), "1 MB");
    assert_eq!(format_bytes(1_250_000), "1.25 MB");
}

#[test]
fn get_nested_property_walks_dotted_paths() {
    let info = json!({ "name": "foo", "dist": { "shasum": "abc" } });
    assert_eq!(get_nested_property(&info, "name"), Some(json!("foo")));
    assert_eq!(get_nested_property(&info, "dist.shasum"), Some(json!("abc")));
    assert_eq!(get_nested_property(&info, "dist.missing"), None);
    assert_eq!(get_nested_property(&info, "name.shasum"), None);
}

#[test]
fn format_field_value_matches_pnpm_rules() {
    assert_eq!(format_field_value(None), "");
    assert_eq!(format_field_value(Some(&Value::Null)), "");
    assert_eq!(format_field_value(Some(&json!("hello"))), "hello");
    assert_eq!(format_field_value(Some(&json!(42))), "42");
    assert_eq!(format_field_value(Some(&json!(true))), "true");
    assert_eq!(format_field_value(Some(&json!({ "a": 1 }))), "{\n  \"a\": 1\n}");
}

#[test]
fn format_time_ago_rejects_future_dates() {
    let now = utc("2024-01-01T00:00:00Z");
    let future = utc("2024-01-02T00:00:00Z");
    assert_eq!(format_time_ago_since(future, now), None);
}

#[test]
fn format_time_ago_buckets_by_largest_unit() {
    let now = utc("2024-06-15T12:00:00Z");
    let cases = [
        ("2024-06-15T11:59:58Z", "a few seconds ago"),
        ("2024-06-15T11:59:00Z", "1 minute ago"),
        ("2024-06-15T11:00:00Z", "1 hour ago"),
        ("2024-06-15T09:00:00Z", "3 hours ago"),
        ("2024-06-14T12:00:00Z", "1 day ago"),
        ("2024-06-10T12:00:00Z", "5 days ago"),
        ("2024-05-01T12:00:00Z", "1 month ago"),
        ("2023-06-15T12:00:00Z", "1 year ago"),
        ("2021-06-15T12:00:00Z", "3 years ago"),
    ];
    for (published, expected) in cases {
        let ago = format_time_ago_since(utc(published), now);
        assert_eq!(ago.as_deref(), Some(expected), "for {published}");
    }
}

#[test]
fn parse_date_accepts_iso_timestamps_only() {
    assert!(parse_date("2024-01-01T00:00:00.000Z").is_some());
    assert!(parse_date("not-a-date").is_none());
}
