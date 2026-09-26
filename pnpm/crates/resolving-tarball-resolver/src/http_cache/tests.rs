use pnpm_tarball::CacheHeaders;

use super::{CacheControl, Freshness, TarballResolutionRecord};

fn record(cache_control: &str, fetched_at: u64) -> TarballResolutionRecord {
    TarballResolutionRecord::from_response(
        "https://example.com/pkg.tgz".to_owned(),
        "https://example.com/pkg.tgz".to_owned(),
        "https://example.com/pkg.tgz".to_owned(),
        "sha512-abc".to_owned(),
        &CacheHeaders {
            etag: Some(r#""pkg""#.to_owned()),
            cache_control: Some(cache_control.to_owned()),
            ..CacheHeaders::default()
        },
        fetched_at,
    )
}

#[test]
fn immutable_max_age_is_fresh_until_it_expires() {
    let record = record("public, max-age=31536000, immutable", 1_000);
    assert_eq!(record.freshness(1_000 + 60_000), Freshness::Fresh);
    assert_eq!(record.freshness(1_000 + 31_536_000_000), Freshness::Revalidate);
}

#[test]
fn max_age_zero_must_be_revalidated() {
    let record = record("max-age=0, must-revalidate", 1_000);
    assert_eq!(record.freshness(1_000), Freshness::Revalidate);
}

#[test]
fn no_store_is_unusable() {
    let record = record("no-store", 1_000);
    assert_eq!(record.freshness(1_000), Freshness::Unusable);
    assert!(!super::should_store(Some("no-store"), Some(r#""pkg""#)));
}

#[test]
fn directive_names_are_case_insensitive() {
    let parsed = CacheControl::parse("No-Store, NO-CACHE, Immutable, Max-Age=60");
    assert_eq!(
        parsed,
        CacheControl { no_store: true, no_cache: true, immutable: true, max_age: Some(60) },
    );
    assert!(!super::should_store(Some("No-Store, max-age=60"), Some(r#""pkg""#)));
}

#[test]
fn invalid_max_age_is_ignored() {
    for header in ["max-age=-1", "max-age=+60", "max-age=1.5", "max-age=", "max-age"] {
        assert_eq!(CacheControl::parse(header).max_age, None, "{header}");
    }
    assert_eq!(CacheControl::parse(r#"max-age="60""#).max_age, Some(60));
}

#[test]
fn age_header_counts_toward_freshness() {
    let headers = CacheHeaders {
        cache_control: Some("max-age=3600".to_owned()),
        age: Some("3000".to_owned()),
        ..CacheHeaders::default()
    };
    let record = TarballResolutionRecord::from_response(
        "https://example.com/pkg.tgz".to_owned(),
        "https://example.com/pkg.tgz".to_owned(),
        "https://example.com/pkg.tgz".to_owned(),
        "sha512-abc".to_owned(),
        &headers,
        1_000,
    );
    assert_eq!(record.freshness(1_000 + 599_000), Freshness::Fresh);
    assert_eq!(record.freshness(1_000 + 600_000), Freshness::Revalidate);
}

#[test]
fn date_header_counts_toward_freshness() {
    let now_ms = 1_700_000_000_000;
    let headers = CacheHeaders {
        cache_control: Some("max-age=3600".to_owned()),
        date: Some(httpdate::fmt_http_date(
            std::time::UNIX_EPOCH + std::time::Duration::from_millis(now_ms - 3_600_000),
        )),
        ..CacheHeaders::default()
    };
    let record = TarballResolutionRecord::from_response(
        "https://example.com/pkg.tgz".to_owned(),
        "https://example.com/pkg.tgz".to_owned(),
        "https://example.com/pkg.tgz".to_owned(),
        "sha512-abc".to_owned(),
        &headers,
        now_ms,
    );
    assert_eq!(record.freshness(now_ms), Freshness::Revalidate);
}

#[test]
fn not_modified_renews_freshness_from_its_own_headers() {
    let stale = record("max-age=60", 1_000);
    let renewed = stale.renewed(
        &CacheHeaders { age: Some("30".to_owned()), ..CacheHeaders::default() },
        1_000_000,
    );
    assert_eq!(renewed.etag, stale.etag);
    assert_eq!(renewed.cache_control, stale.cache_control);
    assert_eq!(renewed.freshness(1_000_000 + 29_000), Freshness::Fresh);
    assert_eq!(renewed.freshness(1_000_000 + 30_000), Freshness::Revalidate);
}
