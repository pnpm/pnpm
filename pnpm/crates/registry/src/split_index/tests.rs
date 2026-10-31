use crate::Package;

use super::add_external_versions;

const SPLIT: &str = r#"{
    "name": "acme",
    "dist-tags": { "latest": "2.0.0" },
    "time": { "2.0.0": "2026-01-01T00:00Z" },
    "versions": { "2.0.0": { "name": "acme", "version": "2.0.0", "dist": { "integrity": "sha512-a" } } },
    "pnpr": { "externalVersions": {
        "1.0.0": "2025-01-01T00:00Z",
        "1.1.0": ["2025-02-01T00:00Z", { "deprecated": "use 2", "trust": "trustedPublisher", "revision": 2 }]
    } }
}"#;

#[test]
fn external_versions_become_marked_versions() {
    let mut meta: Package = serde_json::from_str(SPLIT).expect("parse");
    add_external_versions(SPLIT, &mut meta).expect("external versions");
    assert_eq!(meta.versions.len(), 3);
    assert_eq!(meta.published_at("1.0.0"), Some("2025-01-01T00:00Z"));
    assert_eq!(meta.published_at("1.1.0"), Some("2025-02-01T00:00Z"));
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
fn a_document_without_external_versions_is_unchanged() {
    let body = r#"{ "name": "acme", "dist-tags": {}, "versions": {} }"#;
    let mut meta: Package = serde_json::from_str(body).expect("parse");
    add_external_versions(body, &mut meta).expect("external versions");
    assert!(meta.versions.is_empty());
}
