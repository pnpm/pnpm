use super::{build_publish_document, clean_version};
use crate::registry_config_keys::parse_supported_registry_url;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

fn registry() -> crate::registry_config_keys::NormalizedRegistryUrl {
    parse_supported_registry_url("https://registry.example/").unwrap().normalized_url
}

#[test]
fn cleans_versions() {
    assert_eq!(clean_version("=v1.2.3").unwrap(), "1.2.3");
    assert_eq!(clean_version("  1.0.0 ").unwrap(), "1.0.0");
    assert!(clean_version("not-a-version").is_err());
}

#[test]
fn builds_document_with_dist_and_attachment() {
    let manifest = json!({ "name": "@scope/pkg", "version": "1.0.0", "description": "hi" });
    let document =
        build_publish_document(&manifest, b"tarball", &registry(), None, "latest", false).unwrap();

    assert_eq!(document["name"], "@scope/pkg");
    assert_eq!(document["dist-tags"]["latest"], "1.0.0");
    let version = &document["versions"]["1.0.0"];
    assert_eq!(version["_id"], "@scope/pkg@1.0.0");
    assert!(version["dist"]["integrity"].as_str().unwrap().starts_with("sha512-"));
    // libnpmpublish stores an http:// tarball URL even for an https registry.
    assert_eq!(
        version["dist"]["tarball"],
        "http://registry.example/@scope/pkg/-/@scope/pkg-1.0.0.tgz",
    );
    let attachments = document["_attachments"].as_object().unwrap();
    assert!(attachments.contains_key("@scope/pkg-1.0.0.tgz"));
    assert_eq!(document["access"], Value::Null);
}

#[test]
fn rejects_restricted_access_for_unscoped_package() {
    let manifest = json!({ "name": "pkg", "version": "1.0.0" });
    let err = build_publish_document(
        &manifest,
        b"x",
        &registry(),
        Some(super::Access::Restricted),
        "latest",
        false,
    )
    .unwrap_err();
    assert!(matches!(err, super::PublishPackedPkgError::UnscopedRestricted { .. }));
}

#[test]
fn rejects_private_package() {
    let manifest = json!({ "name": "pkg", "version": "1.0.0", "private": true });
    let err =
        build_publish_document(&manifest, b"x", &registry(), None, "latest", false).unwrap_err();
    assert!(matches!(err, super::PublishPackedPkgError::Private));
}
