use super::parse_install_v2;

const DOCUMENT: &str = r#"{
    "name": "acme",
    "distTags": { "latest": "2.0.0" },
    "modified": "2026-01-01T00:00:00.000Z",
    "timesSince": "2025-12-02T00:00Z",
    "versions": {
        "0.1.0": { "time": null },
        "1.0.0": {},
        "1.1.0": { "time": "2025-12-20T00:00Z", "deprecated": "use 2", "trust": "trustedPublisher", "revision": 2 },
        "2.0.0": {
            "time": "2026-01-01T00:00Z",
            "trust": "provenance",
            "integrity": "sha512-a",
            "requiresBuild": false,
            "manifest": { "dependencies": { "dep": "^1.0.0" } }
        }
    }
}"#;

#[test]
fn entries_without_a_manifest_become_marked_versions() {
    let meta = parse_install_v2(DOCUMENT).expect("parse");
    assert_eq!(meta.name, "acme");
    assert_eq!(meta.dist_tag("latest"), Some("2.0.0"));
    assert_eq!(meta.versions.len(), 4);
    assert!(meta.versions.is_deprecated("1.1.0"));
    assert!(!meta.versions.is_deprecated("1.0.0"));
    let old = meta.versions.get("1.1.0").expect("1.1.0");
    assert!(old.dist.is_external());
    assert_eq!(old.dist.revision, Some(serde_json::json!(2)));
    assert!(
        old.npm_user
            .as_ref()
            .and_then(|user| user.trusted_publisher.as_ref())
            .is_some(),
    );
}

#[test]
fn an_entry_with_a_manifest_is_a_complete_version() {
    let meta = parse_install_v2(DOCUMENT).expect("parse");
    let latest = meta.versions.get("2.0.0").expect("2.0.0");
    assert!(!latest.dist.is_external());
    assert_eq!(latest.name, "acme");
    assert_eq!(latest.version.to_string(), "2.0.0");
    assert!(latest.dist.integrity.is_some());
    assert!(latest.dist.attestations.is_some());
    assert_eq!(latest.dist.pnpr.as_ref().and_then(|pnpr| pnpr.requires_build), Some(false));
}

#[test]
fn a_release_record_is_a_complete_version() {
    let record = br#"{ "name": "acme", "version": "1.0.0", "integrity": "sha512-a", "requiresBuild": true, "manifest": { "dependencies": { "dep": "^1.0.0" } } }"#;
    let version = super::parse_release_record(record).expect("parse");
    assert_eq!(version.name, "acme");
    assert!(!version.dist.is_external());
    assert!(version.dist.integrity.is_some());
    assert_eq!(version.dist.pnpr.as_ref().and_then(|pnpr| pnpr.requires_build), Some(true));
}

#[test]
fn a_version_without_a_time_is_published_at_times_since() {
    let meta = parse_install_v2(DOCUMENT).expect("parse");
    assert_eq!(meta.published_at("0.1.0"), None);
    assert_eq!(meta.published_at("1.0.0"), Some("2025-12-02T00:00Z"));
    assert_eq!(meta.published_at("1.1.0"), Some("2025-12-20T00:00Z"));
    assert_eq!(meta.modified.as_deref(), Some("2026-01-01T00:00:00.000Z"));
}
