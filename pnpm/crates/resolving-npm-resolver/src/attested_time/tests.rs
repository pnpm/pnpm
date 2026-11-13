use super::{append_attested_time, load_attested_times};
use pretty_assertions::assert_eq;
use std::{fs, path::Path};
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
fn an_unfinished_record_costs_only_itself() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("acme.jsonl");
    fs::write(&path, r#"{"version":"1.0.0","publ"#).expect("write unfinished record");

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

fn append_versions_of(path: &Path, writer: usize) {
    for patch in 0..50 {
        let version = format!("{writer}.0.{patch}");
        append_attested_time(path, &version, "2025-01-01T00:00:00.000Z").expect("append");
    }
}

#[test]
fn concurrent_appends_keep_every_record() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("acme.jsonl");

    std::thread::scope(|scope| {
        for writer in 0..8 {
            let path = &path;
            scope.spawn(move || append_versions_of(path, writer));
        }
    });

    assert_eq!(load_attested_times(&path).len(), 400);
}
