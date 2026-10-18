use super::{CAPACITY, CanonicalPackageName, VersionTgz, VersionTgzCache};
use pnpr_storage::DocumentStamp;
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

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

fn stamp(size: u64) -> DocumentStamp {
    DocumentStamp { modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1), size }
}

#[test]
fn a_dist_answers_only_its_namespace_and_version() {
    let cache = VersionTgzCache::default();
    cache.put("npmjs".to_string(), &foo(), "1.0.0", stamp(1), tgz());
    assert!(
        cache
            .get("npmjs", &foo(), "1.0.0", stamp(1))
            .is_some(),
    );
    assert!(
        cache
            .get("private", &foo(), "1.0.0", stamp(1))
            .is_none(),
    );
    assert!(
        cache
            .get("npmjs", &foo(), "1.0.1", stamp(1))
            .is_none(),
    );
}

#[test]
fn a_rewritten_packument_retires_its_dists() {
    let cache = VersionTgzCache::default();
    cache.put("npmjs".to_string(), &foo(), "1.0.0", stamp(1), tgz());
    assert!(
        cache
            .get("npmjs", &foo(), "1.0.0", stamp(2))
            .is_none(),
    );
}

#[test]
fn a_full_cache_keeps_most_of_its_entries() {
    let cache = VersionTgzCache::default();
    for version in 0..=CAPACITY {
        cache.put("npmjs".to_string(), &foo(), &version.to_string(), stamp(1), tgz());
    }
    let kept = cache.entries.lock().unwrap().len();
    assert_eq!(kept, CAPACITY - CAPACITY / 4 + 1);
}
