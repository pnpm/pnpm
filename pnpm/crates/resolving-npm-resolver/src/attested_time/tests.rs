use super::{append_attested_time, load_attested_times};
use pretty_assertions::assert_eq;
use std::fs;
use tempfile::TempDir;

#[test]
fn appended_times_are_loaded_by_version() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("registry/acme.jsonl");

    append_attested_time(&path, "1.0.0", "2025-01-01T00:00:00.000Z").expect("append");
    append_attested_time(&path, "1.1.0", "2025-02-01T00:00:00.000Z").expect("append");

    let times = load_attested_times(&path);
    assert_eq!(times.len(), 2);
    assert_eq!(times["1.0.0"], "2025-01-01T00:00:00.000Z");
    assert_eq!(times["1.1.0"], "2025-02-01T00:00:00.000Z");
}

#[test]
fn a_line_that_does_not_parse_is_skipped() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("acme.jsonl");
    fs::write(&path, "{\"version\":\"1.0.0\",\"publ\n").expect("write torn line");

    append_attested_time(&path, "1.1.0", "2025-02-01T00:00:00.000Z").expect("append");

    let times = load_attested_times(&path);
    assert_eq!(times.len(), 1);
    assert_eq!(times["1.1.0"], "2025-02-01T00:00:00.000Z");
}

#[test]
fn a_missing_file_holds_no_times() {
    let dir = TempDir::new().expect("tempdir");
    assert!(load_attested_times(&dir.path().join("acme.jsonl")).is_empty());
}
