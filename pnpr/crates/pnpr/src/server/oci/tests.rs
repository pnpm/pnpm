use super::{Endpoint, api_base, parse_endpoint, query_param};

fn endpoint(tail: &str) -> Endpoint {
    parse_endpoint(tail).expect("tail names an endpoint")
}

#[test]
fn a_repository_name_runs_up_to_the_trailing_verb() {
    match endpoint("acme/team/app/manifests/1.0") {
        Endpoint::Manifest { name, reference } => {
            assert_eq!(name, "acme/team/app");
            assert_eq!(reference, "1.0");
        }
        _ => panic!("expected a manifest endpoint"),
    }
    match endpoint("alpine/blobs/sha256-abc") {
        Endpoint::Blob { name, digest } => {
            assert_eq!(name, "alpine");
            assert_eq!(digest, "sha256-abc");
        }
        _ => panic!("expected a blob endpoint"),
    }
}

#[test]
fn an_upload_is_told_apart_from_a_blob_by_its_uploads_segment() {
    assert!(matches!(endpoint("acme/app/blobs/uploads"), Endpoint::StartUpload { .. }));
    assert!(matches!(endpoint("acme/app/blobs/uploads/"), Endpoint::StartUpload { .. }));
    match endpoint("acme/app/blobs/uploads/deadbeef") {
        Endpoint::Upload { name, id } => {
            assert_eq!(name, "acme/app");
            assert_eq!(id, "deadbeef");
        }
        _ => panic!("expected an upload endpoint"),
    }
}

#[test]
fn tags_and_the_catalog_are_recognized() {
    assert!(matches!(endpoint("_catalog"), Endpoint::Catalog));
    match endpoint("acme/app/tags/list") {
        Endpoint::Tags { name } => assert_eq!(name, "acme/app"),
        _ => panic!("expected a tags endpoint"),
    }
}

#[test]
fn a_tail_with_no_repository_name_is_not_an_endpoint() {
    for tail in ["manifests/1.0", "blobs/uploads", "tags/list", "acme/app", ""] {
        assert!(parse_endpoint(tail).is_none(), "{tail:?} should name no endpoint");
    }
}

#[test]
fn the_api_base_is_whatever_preceded_the_captured_tail() {
    assert_eq!(api_base("/v2/acme/app/manifests/1.0", "acme/app/manifests/1.0"), "/v2");
    assert_eq!(
        api_base("/oci/~images/v2/acme/app/blobs/uploads/", "acme/app/blobs/uploads/"),
        "/oci/~images/v2",
    );
    // A registry named `v2` does not confuse the split: the base is matched
    // from the end, not by looking for the API segment.
    assert_eq!(api_base("/~v2/v2/acme/app/tags/list", "acme/app/tags/list"), "/~v2/v2");
}

#[test]
fn a_digest_is_read_from_the_raw_query() {
    assert_eq!(query_param(Some("digest=sha256%3Aabc"), "digest").as_deref(), Some("sha256:abc"));
    assert_eq!(query_param(Some("a=1&digest=sha256:abc"), "digest").as_deref(), Some("sha256:abc"));
    assert_eq!(query_param(Some("mount=x&from=y"), "digest"), None);
    assert_eq!(query_param(None, "digest"), None);
}

#[test]
fn the_blob_ceiling_bounds_the_whole_upload_not_one_chunk() {
    use super::{MAX_BLOB_BYTES, advance_within_ceiling};

    let ceiling = MAX_BLOB_BYTES as u64;
    assert_eq!(advance_within_ceiling(0, 1), Some(1));
    assert_eq!(advance_within_ceiling(ceiling - 1, 1), Some(ceiling));
    // A chunk that is itself small still refuses once the upload is full,
    // which is what the per-request body limit cannot see.
    assert_eq!(advance_within_ceiling(ceiling, 1), None);
    assert_eq!(advance_within_ceiling(ceiling - 1, 2), None);
    // Saturating, so a length near the top refuses rather than wrapping.
    assert_eq!(advance_within_ceiling(u64::MAX, 1), None);
}

#[test]
fn a_content_range_is_read_whole_or_not_at_all() {
    use super::parse_range_start;

    assert_eq!(parse_range_start("0-4"), Some(0));
    assert_eq!(parse_range_start(" 5 - 10 "), Some(5));
    // Reading only the text before the hyphen would take this for byte 0 and
    // let the chunk through whenever the upload happened to be there.
    assert_eq!(parse_range_start("0-garbage"), None);
    assert_eq!(parse_range_start("garbage-4"), None);
    assert_eq!(parse_range_start("4"), None);
    assert_eq!(parse_range_start(""), None);
}
