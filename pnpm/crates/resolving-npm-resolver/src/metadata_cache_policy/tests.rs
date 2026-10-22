use super::{DocumentKind, MirrorWrite, Revalidation, mirror_may_answer};
use crate::{
    fetch_full_metadata::{ACCEPT_ABBREVIATED_DOC, ACCEPT_FULL_DOC},
    mirror::MetaHeaders,
};
use pnpm_config::NetworkPolicy;
use pnpm_registry::Package;
use pretty_assertions::assert_eq;
use reqwest::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, HeaderMap, HeaderValue};

const ABBREVIATED_ETAG: &str = r#""abbreviated""#;
const FULL_ETAG: &str = r#""full""#;
const MODIFIED: &str = "2015-06-10T00:00:00.000Z";

fn stored(etag: Option<&str>, full_etag: Option<&str>, uncacheable: bool) -> MetaHeaders {
    MetaHeaders {
        etag: etag.map(str::to_string),
        full_etag: full_etag.map(str::to_string),
        modified: Some(MODIFIED.to_string()),
        uncacheable,
    }
}

fn response_headers(
    etag: Option<&str>,
    cache_control: Option<&str>,
    abbreviated: bool,
) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(etag) = etag {
        headers.insert(ETAG, HeaderValue::from_str(etag).unwrap());
    }
    if let Some(cache_control) = cache_control {
        headers.insert(CACHE_CONTROL, HeaderValue::from_str(cache_control).unwrap());
    }
    if abbreviated {
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/vnd.npm.install-v1+json"),
        );
    }
    headers
}

fn package() -> Package {
    serde_json::from_value(serde_json::json!({
        "name": "is-positive",
        "dist-tags": {},
        "modified": MODIFIED,
        "versions": {},
    }))
    .expect("deserialize Package")
}

#[test]
fn document_kind_follows_the_mirror_slot_and_its_stored_tags() {
    let abbreviated = stored(Some(ABBREVIATED_ETAG), None, false);
    let upgraded = stored(None, Some(FULL_ETAG), false);
    let empty_full_etag = stored(None, Some(""), false);
    let cases = [
        (false, None, DocumentKind::Abbreviated),
        (false, Some(&abbreviated), DocumentKind::Abbreviated),
        (false, Some(&upgraded), DocumentKind::FullInAbbreviatedMirror),
        (false, Some(&empty_full_etag), DocumentKind::Abbreviated),
        (true, None, DocumentKind::Full),
        (true, Some(&upgraded), DocumentKind::Full),
    ];
    for (full_metadata, headers, expected) in cases {
        eprintln!("full_metadata={full_metadata} headers={headers:?}");
        assert_eq!(DocumentKind::for_mirror(full_metadata, headers), expected);
    }
}

#[test]
fn only_the_abbreviated_document_is_requested_as_abbreviated() {
    assert_eq!(DocumentKind::Abbreviated.accept(), ACCEPT_ABBREVIATED_DOC);
    assert_eq!(DocumentKind::Full.accept(), ACCEPT_FULL_DOC);
    assert_eq!(DocumentKind::FullInAbbreviatedMirror.accept(), ACCEPT_FULL_DOC);
}

#[test]
fn revalidation_sends_the_validators_of_the_requested_representation() {
    let abbreviated = stored(Some(ABBREVIATED_ETAG), None, false);
    let both_tags = stored(Some(ABBREVIATED_ETAG), Some(FULL_ETAG), false);
    let uncacheable = stored(Some(ABBREVIATED_ETAG), None, true);
    let none = Revalidation { etag: None, modified: None, bypass_cache: false };
    let bypassed = Revalidation { bypass_cache: true, ..none };
    let cases = [
        (DocumentKind::Abbreviated, None, false, none),
        (DocumentKind::Abbreviated, None, true, bypassed),
        (
            DocumentKind::Abbreviated,
            Some(&abbreviated),
            false,
            Revalidation { etag: Some(ABBREVIATED_ETAG), modified: Some(MODIFIED), ..none },
        ),
        // A mirror that recorded the other representation sends no validators.
        (DocumentKind::Abbreviated, Some(&both_tags), false, none),
        (DocumentKind::FullInAbbreviatedMirror, Some(&abbreviated), false, none),
        (
            DocumentKind::Full,
            Some(&both_tags),
            false,
            Revalidation { etag: Some(ABBREVIATED_ETAG), modified: Some(MODIFIED), ..none },
        ),
        (
            DocumentKind::FullInAbbreviatedMirror,
            Some(&both_tags),
            false,
            Revalidation { etag: Some(FULL_ETAG), modified: Some(MODIFIED), ..none },
        ),
        (DocumentKind::FullInAbbreviatedMirror, Some(&both_tags), true, bypassed),
        (
            DocumentKind::Abbreviated,
            Some(&uncacheable),
            false,
            Revalidation {
                etag: Some(ABBREVIATED_ETAG),
                modified: Some(MODIFIED),
                bypass_cache: true,
            },
        ),
        (DocumentKind::Abbreviated, Some(&uncacheable), true, bypassed),
    ];
    for (kind, headers, bypass, expected) in cases {
        eprintln!("kind={kind:?} headers={headers:?} bypass={bypass}");
        assert_eq!(kind.revalidation(headers, bypass), expected);
    }
}

#[test]
fn mirror_write_records_the_tag_where_its_representation_reads_it() {
    let meta = package();
    let cases = [
        (DocumentKind::Abbreviated, Some(ABBREVIATED_ETAG), None),
        (DocumentKind::Full, Some(ABBREVIATED_ETAG), None),
        (DocumentKind::FullInAbbreviatedMirror, None, Some(ABBREVIATED_ETAG)),
    ];
    for (kind, etag, full_etag) in cases {
        let headers = response_headers(Some(ABBREVIATED_ETAG), None, true);
        let recorded = MirrorWrite::on_response(kind, &headers).headers(&meta);
        assert_eq!(recorded, stored(etag, full_etag, false), "kind={kind:?}");
    }
}

#[test]
fn mirror_write_records_whether_the_response_forbade_caching() {
    let meta = package();
    let cases = [
        (None, false),
        (Some("public, max-age=300"), false),
        (Some("public, max-age=900, no-store"), true),
        (Some("max-age=0"), true),
        (Some("no-cache"), true),
    ];
    for (cache_control, uncacheable) in cases {
        let headers = response_headers(None, cache_control, true);
        let recorded = MirrorWrite::on_response(DocumentKind::Abbreviated, &headers).headers(&meta);
        assert_eq!(recorded.uncacheable, uncacheable, "cache_control={cache_control:?}");
    }
}

#[test]
fn only_an_abbreviated_request_answered_with_another_type_is_normalized() {
    let cases = [
        (DocumentKind::Abbreviated, true, false),
        (DocumentKind::Abbreviated, false, true),
        (DocumentKind::Full, false, false),
        (DocumentKind::FullInAbbreviatedMirror, false, false),
    ];
    for (kind, abbreviated_content_type, normalizes) in cases {
        let headers = response_headers(None, None, abbreviated_content_type);
        assert_eq!(
            MirrorWrite::on_response(kind, &headers).normalizes_to_abbreviated(),
            normalizes,
            "kind={kind:?} abbreviated_content_type={abbreviated_content_type}",
        );
    }
}

#[test]
fn a_failed_save_removes_the_mirror_only_for_an_uncacheable_response() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("is-positive.jsonl");
    for (cache_control, removed) in [(None, false), (Some("no-store"), true)] {
        std::fs::write(&path, "previous").expect("seed mirror");
        let headers = response_headers(None, cache_control, true);
        MirrorWrite::on_response(DocumentKind::Abbreviated, &headers)
            .discard_after_failed_save(&path);
        assert_eq!(!path.exists(), removed, "cache_control={cache_control:?}");
    }
}

#[test]
fn only_an_online_pick_skips_a_mirror_that_forbade_caching() {
    let cacheable = stored(Some(ABBREVIATED_ETAG), None, false);
    let uncacheable = stored(Some(ABBREVIATED_ETAG), None, true);
    let cases = [
        (NetworkPolicy::Online, None, true),
        (NetworkPolicy::Online, Some(&cacheable), true),
        (NetworkPolicy::Online, Some(&uncacheable), false),
        (NetworkPolicy::PreferCache, Some(&uncacheable), true),
        (NetworkPolicy::Offline, Some(&uncacheable), true),
    ];
    for (network, headers, may_answer) in cases {
        assert_eq!(
            mirror_may_answer(network, headers),
            may_answer,
            "network={network:?} headers={headers:?}",
        );
    }
}
