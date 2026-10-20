use super::{CanonicalPackageName, DerivedTar, TarIntegrityCache};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
fn a_read_that_started_before_a_record_keeps_the_recorded_table() {
    let cache = TarIntegrityCache::default();
    let name = CanonicalPackageName::parse("left-pad", pnpr_package_name::Ecosystem::Npm).unwrap();
    let read_started = Instant::now();
    let recorded = Arc::new(BTreeMap::from([(
        "tgz".to_string(),
        DerivedTar { tar_integrity: "tar".to_string(), requires_build: Some(false), scanned: true },
    )]));
    cache.put(&name, Arc::clone(&recorded), read_started + Duration::from_millis(1));
    cache.put(&name, Arc::default(), read_started);
    assert_eq!(cache.get(&name), Some(recorded));
}

#[test]
fn deserializes_legacy_string_and_record_entries() {
    let decoded: BTreeMap<String, DerivedTar> = serde_json::from_str(
        r#"{
            "sha512-legacy": "sha512-tar-a",
            "sha512-new": { "tarIntegrity": "sha512-tar-b", "requiresBuild": true },
            "sha512-scanned-unknown": { "tarIntegrity": "sha512-tar-c" }
        }"#,
    )
    .unwrap();
    assert_eq!(
        decoded.get("sha512-legacy"),
        Some(&DerivedTar {
            tar_integrity: "sha512-tar-a".to_string(),
            requires_build: None,
            scanned: false,
        }),
    );
    assert_eq!(
        decoded.get("sha512-new"),
        Some(&DerivedTar {
            tar_integrity: "sha512-tar-b".to_string(),
            requires_build: Some(true),
            scanned: true,
        }),
    );
    assert_eq!(
        decoded.get("sha512-scanned-unknown"),
        Some(&DerivedTar {
            tar_integrity: "sha512-tar-c".to_string(),
            requires_build: None,
            scanned: true,
        }),
    );
}
