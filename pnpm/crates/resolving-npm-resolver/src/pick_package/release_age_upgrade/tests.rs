use tempfile::tempdir;

use super::{Package, persist_upgraded_to_mirror};
use crate::mirror::load_meta_headers;

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

    persist_upgraded_to_mirror(&pkg_mirror, &upgraded_meta(), false);

    let headers = load_meta_headers(&pkg_mirror).expect("headers readable");
    assert_eq!(headers.etag, None);
    assert_eq!(headers.modified.as_deref(), Some(MODIFIED));
}

#[test]
fn persist_upgraded_to_mirror_writes_no_etag_to_the_filtered_mirror() {
    let dir = tempdir().expect("tempdir");
    let pkg_mirror = dir.path().join("is-positive.jsonl");

    persist_upgraded_to_mirror(&pkg_mirror, &upgraded_meta(), true);

    let headers = load_meta_headers(&pkg_mirror).expect("headers readable");
    assert_eq!(headers.etag, None);
    assert_eq!(headers.modified.as_deref(), Some(MODIFIED));
}
