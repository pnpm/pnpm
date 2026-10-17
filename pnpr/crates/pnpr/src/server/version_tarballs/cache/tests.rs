use super::{CanonicalPackageName, VersionTgz, VersionTgzCache};
use std::{sync::Arc, time::Duration};

fn tgz() -> Arc<VersionTgz> {
    Arc::new(VersionTgz {
        filename: "foo-1.0.0.tgz".to_string(),
        dist: serde_json::from_str("{}").unwrap(),
        integrity_key: None,
    })
}

fn foo() -> CanonicalPackageName {
    CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap()
}

#[test]
fn a_dist_answers_only_its_own_scope() {
    let cache = VersionTgzCache::default();
    cache.put("hosted:acme".to_string(), &foo(), "1.0.0", tgz(), Duration::from_mins(1));
    assert!(
        cache
            .get("hosted:acme", &foo(), "1.0.0")
            .is_some(),
    );
    assert!(
        cache
            .get("hosted:other", &foo(), "1.0.0")
            .is_none(),
    );
    assert!(
        cache
            .get("hosted:acme", &foo(), "1.0.1")
            .is_none(),
    );
}

#[test]
fn a_dist_with_no_ttl_is_not_kept() {
    let cache = VersionTgzCache::default();
    cache.put("upstream:npmjs".to_string(), &foo(), "1.0.0", tgz(), Duration::ZERO);
    assert!(
        cache
            .get("upstream:npmjs", &foo(), "1.0.0")
            .is_none(),
    );
}
