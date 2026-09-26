use super::{Package, persist_upgraded_to_mirror};
use crate::mirror::{load_meta_headers, save_fail::fail_next_mirror_save};
use tempfile::tempdir;

const FULL_ETAG: &str = r#""full-etag""#;
const MODIFIED: &str = "2015-06-10T00:00:00.000Z";

fn upgraded_meta() -> Package {
    let mut meta: Package = serde_json::from_value(serde_json::json!({
        "name": "is-positive",
        "dist-tags": { "latest": "1.0.0" },
        "modified": MODIFIED,
        "versions": {},
    }))
    .expect("deserialize Package");
    meta.etag = Some(FULL_ETAG.to_string());
    meta
}

#[test]
fn persist_upgraded_to_mirror_writes_no_etag_to_the_indexed_mirror() {
    let dir = tempdir().expect("tempdir");
    let pkg_mirror = dir.path().join("is-positive.jsonl");

    persist_upgraded_to_mirror(&pkg_mirror, &upgraded_meta(), false, false);

    let headers = load_meta_headers(&pkg_mirror).expect("headers readable");
    assert_eq!(headers.etag, None);
    assert_eq!(headers.modified.as_deref(), Some(MODIFIED));
    assert!(!headers.uncacheable);
}

#[test]
fn persist_upgraded_to_mirror_records_an_uncacheable_full_response() {
    let dir = tempdir().expect("tempdir");
    let pkg_mirror = dir.path().join("is-positive.jsonl");

    persist_upgraded_to_mirror(&pkg_mirror, &upgraded_meta(), false, true);

    let headers = load_meta_headers(&pkg_mirror).expect("headers readable");
    assert!(headers.uncacheable);
    assert_eq!(headers.etag, None);
}

#[test]
fn failed_uncacheable_upgrade_persist_removes_the_previous_mirror() {
    let dir = tempdir().expect("tempdir");
    let pkg_mirror = dir.path().join("is-positive.jsonl");
    persist_upgraded_to_mirror(&pkg_mirror, &upgraded_meta(), false, false);
    let before = load_meta_headers(&pkg_mirror).expect("seeded headers");
    assert!(!before.uncacheable);

    fail_next_mirror_save();
    assert!(persist_upgraded_to_mirror(&pkg_mirror, &upgraded_meta(), false, true).is_none());
    assert!(!pkg_mirror.exists());
}

#[test]
fn failed_cacheable_upgrade_persist_keeps_the_previous_mirror() {
    let dir = tempdir().expect("tempdir");
    let pkg_mirror = dir.path().join("is-positive.jsonl");
    persist_upgraded_to_mirror(&pkg_mirror, &upgraded_meta(), false, false);

    fail_next_mirror_save();
    assert!(persist_upgraded_to_mirror(&pkg_mirror, &upgraded_meta(), false, false).is_none());

    let headers = load_meta_headers(&pkg_mirror).expect("previous mirror remains");
    assert!(!headers.uncacheable);
    assert_eq!(headers.modified.as_deref(), Some(MODIFIED));
}

#[test]
fn persist_upgraded_to_mirror_writes_no_etag_to_the_filtered_mirror() {
    let dir = tempdir().expect("tempdir");
    let pkg_mirror = dir.path().join("is-positive.jsonl");

    persist_upgraded_to_mirror(&pkg_mirror, &upgraded_meta(), true, false);

    let headers = load_meta_headers(&pkg_mirror).expect("headers readable");
    assert_eq!(headers.etag, None);
    assert_eq!(headers.modified.as_deref(), Some(MODIFIED));
}
