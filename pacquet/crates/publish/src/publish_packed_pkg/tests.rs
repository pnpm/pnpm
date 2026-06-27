use super::{DistHashes, build_publish_document, clean_version, is_otp_challenge};
use crate::registry_config_keys::parse_supported_registry_url;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

fn registry() -> crate::registry_config_keys::NormalizedRegistryUrl {
    parse_supported_registry_url("https://registry.example/").unwrap().normalized_url
}

fn hashes() -> DistHashes<'static> {
    DistHashes { integrity: "sha512-deadbeef", shasum: "abc123" }
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
        build_publish_document(&manifest, b"tarball", &registry(), None, "latest", &hashes())
            .unwrap();

    assert_eq!(document["name"], "@scope/pkg");
    assert_eq!(document["dist-tags"]["latest"], "1.0.0");
    let version = &document["versions"]["1.0.0"];
    assert_eq!(version["_id"], "@scope/pkg@1.0.0");
    assert_eq!(version["dist"]["integrity"], "sha512-deadbeef");
    assert_eq!(version["dist"]["shasum"], "abc123");
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
fn detects_otp_challenge_by_header_token_or_body() {
    assert!(is_otp_challenge(Some("ipaddress, otp"), ""));
    assert!(is_otp_challenge(Some("OTP"), ""));
    assert!(is_otp_challenge(None, "you must provide a one-time pass"));
    // A bare substring in another token must not over-match.
    assert!(!is_otp_challenge(Some(r#"Basic realm="notop""#), "denied"));
    assert!(!is_otp_challenge(None, "forbidden"));
}

#[test]
fn manifest_level_tag_overrides_the_default() {
    let manifest = json!({ "name": "pkg", "version": "1.0.0", "tag": "next" });
    let document =
        build_publish_document(&manifest, b"x", &registry(), None, "latest", &hashes()).unwrap();
    assert_eq!(document["dist-tags"]["next"], "1.0.0");
    assert!(document["dist-tags"].get("latest").is_none());
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
        &hashes(),
    )
    .unwrap_err();
    assert!(matches!(err, super::PublishPackedPkgError::UnscopedRestricted { .. }));
}

#[test]
fn rejects_private_package() {
    let manifest = json!({ "name": "pkg", "version": "1.0.0", "private": true });
    let err = build_publish_document(&manifest, b"x", &registry(), None, "latest", &hashes())
        .unwrap_err();
    assert!(matches!(err, super::PublishPackedPkgError::Private));
}
