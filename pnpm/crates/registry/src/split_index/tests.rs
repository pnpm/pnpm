use crate::Package;

use super::add_other_versions;

const SPLIT: &str = r#"{
    "name": "acme",
    "dist-tags": { "latest": "2.0.0" },
    "time": { "2.0.0": "2026-01-01T00:00Z" },
    "versions": { "2.0.0": { "name": "acme", "version": "2.0.0", "dist": { "integrity": "sha512-a" } } },
    "pnpr": { "timesSince": "2025-12-02T00:00Z", "otherVersions": {
        "0.1.0": { "time": null },
        "1.0.0": {},
        "1.1.0": { "time": "2025-12-20T00:00Z", "deprecated": "use 2", "trust": "trustedPublisher", "revision": 2 }
    } }
}"#;

#[test]
fn other_versions_become_marked_versions() {
    let mut meta: Package = serde_json::from_str(SPLIT).expect("parse");
    add_other_versions(SPLIT, &mut meta).expect("other versions");
    assert_eq!(meta.versions.len(), 4);
    assert_eq!(meta.published_at("0.1.0"), None);
    assert_eq!(meta.published_at("1.0.0"), Some("2025-12-02T00:00Z"));
    assert_eq!(meta.published_at("1.1.0"), Some("2025-12-20T00:00Z"));
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
    assert!(
        !meta.versions
            .get("2.0.0")
            .expect("2.0.0")
            .dist
            .is_external(),
    );
}

#[test]
fn a_document_without_other_versions_is_unchanged() {
    let body = r#"{ "name": "acme", "dist-tags": {}, "versions": {} }"#;
    let mut meta: Package = serde_json::from_str(body).expect("parse");
    add_other_versions(body, &mut meta).expect("other versions");
    assert!(meta.versions.is_empty());
}
