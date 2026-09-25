use super::{Freshness, TarballResolutionRecord};

fn record(cache_control: &str, fetched_at: u64) -> TarballResolutionRecord {
    TarballResolutionRecord {
        url: "https://example.com/pkg.tgz".to_owned(),
        tarball: "https://example.com/pkg.tgz".to_owned(),
        integrity: "sha512-abc".to_owned(),
        etag: Some(r#""pkg""#.to_owned()),
        cache_control: Some(cache_control.to_owned()),
        fetched_at,
    }
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
