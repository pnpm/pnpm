use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use super::{
    bin_summary, format_bytes, format_field_value, format_person, format_time_ago_since,
    get_nested_property, parse_date, published_info, publisher, render_fields, render_summary,
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

/// A rich info object that exercises every optional summary section.
fn rich_info() -> Value {
    json!({
        "name": "is-negative",
        "version": "1.0.0",
        "license": "MIT",
        "description": "Check if a number is negative",
        "homepage": "https://npmjs.example/is-negative",
        "deprecated": "use something else",
        "keywords": ["negative", "number"],
        "bin": { "is-negative": "cli.js" },
        "dist": {
            "tarball": "https://npmjs.example/is-negative.tgz",
            "shasum": "1d06e1c0",
            "integrity": "sha512-abc",
            "unpackedSize": 1234
        },
        "dependencies": { "left-pad": "^1.0.0" },
        "maintainers": [{ "name": "alice", "email": "alice@example.com" }, { "name": "bob" }],
        "depsCount": 1,
        "versionsCount": 3,
        "distTags": { "latest": "1.0.0" },
        "time": { "1.0.0": "2015-01-01T00:00:00.000Z" }
    })
}

#[test]
fn render_summary_includes_every_section() {
    let out = render_summary(&rich_info());
    for needle in [
        "is-negative@1.0.0",
        "deps: ",
        "versions: ",
        "Check if a number is negative",
        "is-negative", // homepage host / keyword
        "DEPRECATED! - use something else",
        "keywords:",
        "bin:",
        ".tarball:",
        ".shasum:",
        ".integrity:",
        ".unpackedSize:",
        "dependencies:",
        "maintainers:",
        "dist-tags:",
        "latest:",
        "published ",
    ] {
        assert!(out.contains(needle), "summary missing {needle:?}:\n{out}");
    }
}

#[test]
fn render_summary_minimal_shows_deps_none() {
    let out = render_summary(&json!({ "name": "x", "version": "1.0.0" }));
    assert!(out.contains("x@1.0.0"), "{out}");
    assert!(out.contains("deps: none"), "{out}");
}

#[test]
fn bin_summary_covers_string_object_and_absent() {
    // A string `bin` on an unscoped package derives the package name.
    let unscoped = bin_summary(&json!({ "name": "hello-bin", "bin": "cli.js" }));
    assert_eq!(unscoped.len(), 2);
    assert!(unscoped[0].is_empty(), "the bin section opens with a blank line: {unscoped:?}");
    assert!(unscoped[1].contains("hello-bin"), "{:?}", unscoped[1]);
    // A string `bin` on a scoped package strips the scope.
    let scoped = bin_summary(&json!({ "name": "@scope/hello-bin", "bin": "cli.js" }));
    assert!(scoped[1].contains("hello-bin") && !scoped[1].contains("@scope"), "{:?}", scoped[1]);
    // An object `bin` lists its keys.
    let object = bin_summary(&json!({ "name": "x", "bin": { "one": "a.js" } }));
    assert!(object[1].contains("one"), "{:?}", object[1]);
    // No / empty bin renders nothing.
    assert!(bin_summary(&json!({ "name": "x" })).is_empty(), "a missing bin renders nothing");
    assert!(
        bin_summary(&json!({ "name": "x", "bin": "" })).is_empty(),
        "an empty bin renders nothing",
    );
}

#[test]
fn render_fields_multi_text_formats_by_type() {
    let info = json!({ "name": "x", "dist": { "a": 1 } });
    let selected = ["name".to_string(), "dist".to_string()];
    let out = render_fields(&info, &selected, false);
    assert!(out.contains("name = 'x'"), "{out}");
    assert!(out.contains(r#"dist = {"a":1}"#), "{out}"); // objects render as compact JSON
    // An absent field renders as `field = ` with an empty value.
    let with_absent = ["missing".to_string(), "name".to_string()];
    let out2 = render_fields(&info, &with_absent, false);
    assert!(out2.lines().any(|line| line == "missing = "), "{out2}");
}

#[test]
fn publisher_prefers_npm_user_then_maintainers_then_author() {
    // `_npmUser` wins, with and without an email.
    let with_email =
        publisher(&json!({ "_npmUser": { "name": "alice", "email": "a@b.c" } })).unwrap();
    assert!(with_email.contains("alice"), "{with_email}");
    let no_email = publisher(&json!({ "_npmUser": { "name": "bob" } })).unwrap();
    assert!(no_email.contains("bob"), "{no_email}");
    // Then the first maintainer, with `et al.` when there is more than one.
    let many = publisher(&json!({ "maintainers": [{ "name": "a" }, { "name": "b" }] })).unwrap();
    assert!(many.contains("et al."), "{many}");
    let single = publisher(&json!({ "maintainers": [{ "name": "solo" }] })).unwrap();
    assert!(single.contains("solo") && !single.contains("et al."), "{single}");
    // Then the author (returned verbatim, not colorized).
    assert_eq!(publisher(&json!({ "author": "Jane Doe" })), Some("Jane Doe".to_string()));
    // Nothing to attribute.
    assert_eq!(publisher(&json!({})), None);
}

#[test]
fn published_info_covers_publisher_absence_and_bad_time() {
    let with_publisher = json!({
        "version": "1.0.0",
        "time": { "1.0.0": "2015-01-01T00:00:00.000Z" },
        "maintainers": [{ "name": "alice" }],
    });
    let line = published_info(&with_publisher).expect("a published line");
    assert!(
        line.contains("published ") && line.contains(" ago") && line.contains("alice"),
        "{line}",
    );

    let no_publisher =
        json!({ "version": "1.0.0", "time": { "1.0.0": "2015-01-01T00:00:00.000Z" } });
    let line = published_info(&no_publisher).expect("a published line");
    assert!(
        line.contains("published ") && line.contains(" ago") && !line.contains(" by "),
        "{line}",
    );

    // A future timestamp degrades to "just now".
    let future = json!({ "version": "1.0.0", "time": { "1.0.0": "2999-01-01T00:00:00.000Z" } });
    let line = published_info(&future).unwrap();
    assert!(line.contains("just now"), "{line}");

    // Missing version, missing time, and an unparsable timestamp all yield no line.
    assert_eq!(published_info(&json!({ "version": "1.0.0" })), None);
    assert_eq!(published_info(&json!({ "time": {} })), None);
    assert_eq!(published_info(&json!({ "version": "1.0.0", "time": { "1.0.0": "nope" } })), None);
}

#[test]
fn format_person_renders_name_and_optional_email() {
    let with_email = format_person(&json!({ "name": "alice", "email": "alice@example.com" }));
    assert!(
        with_email.contains("alice") && with_email.contains("alice@example.com"),
        "{with_email}",
    );
    assert!(with_email.contains('<') && with_email.contains('>'), "{with_email}");

    let without_email = format_person(&json!({ "name": "bob" }));
    assert!(without_email.contains("bob") && !without_email.contains('<'), "{without_email}");
}
