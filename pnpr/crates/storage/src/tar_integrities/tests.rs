use super::{CanonicalPackageName, DerivedTar, TarIntegrityCache, is_tar_entry_recorded};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
fn identical_unscanned_entry_is_considered_recorded_until_stats_arrive() {
    let unscanned = DerivedTar::new("sha512-tar", None, None, None);
    let table = BTreeMap::from([("sha512-tgz".to_string(), unscanned.clone())]);

    assert!(is_tar_entry_recorded(&table, "sha512-tgz", &unscanned));
    assert!(!is_tar_entry_recorded(
        &table,
        "sha512-tgz",
        &DerivedTar::new("sha512-tar", Some(false), Some(2), Some(42)),
    ));
}

#[test]
fn a_read_that_started_before_a_record_keeps_the_recorded_table() {
    let cache = TarIntegrityCache::default();
    let name = CanonicalPackageName::parse("left-pad", pnpr_package_name::Ecosystem::Npm).unwrap();
    let read_started = Instant::now();
    let recorded = Arc::new(BTreeMap::from([(
        "tgz".to_string(),
        DerivedTar::new("tar", Some(false), Some(2), Some(42)),
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
            "sha512-new": {
                "tarIntegrity": "sha512-tar-b",
                "requiresBuild": true,
                "fileCount": 3,
                "unpackedSize": 17
            },
            "sha512-pre-stats": {
                "tarIntegrity": "sha512-tar-c",
                "requiresBuild": false
            }
        }"#,
    )
    .unwrap();
    assert_eq!(
        decoded.get("sha512-legacy"),
        Some(&DerivedTar {
            tar_integrity: "sha512-tar-a".to_string(),
            requires_build: None,
            file_count: None,
            unpacked_size: None,
            bins: None,
            scanned: false,
        }),
    );
    assert_eq!(
        decoded.get("sha512-new"),
        Some(&DerivedTar {
            tar_integrity: "sha512-tar-b".to_string(),
            requires_build: Some(true),
            file_count: Some(3),
            unpacked_size: Some(17),
            bins: None,
            scanned: true,
        }),
    );
    assert_eq!(
        decoded.get("sha512-pre-stats"),
        Some(&DerivedTar {
            tar_integrity: "sha512-tar-c".to_string(),
            requires_build: Some(false),
            file_count: None,
            unpacked_size: None,
            bins: None,
            scanned: false,
        }),
    );
}
